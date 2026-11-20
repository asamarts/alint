//! What a fix pass reads again after a rule's fixes land, by what they
//! changed on disk, and what it reads of what `check` read already.
//!
//! A create, a content edit (it waits in the compose buffer until the pass
//! ends, and no rule reads that buffer) and a removal keep what the rules
//! before read of the other files; the relations are evaluated before the
//! first rule, as in `check`; and staging on the index `check` evaluated
//! (`check --format sarif`) reads nothing again. Clearing every cache after
//! a fix, or before the relations were evaluated, read files again and took
//! their rows again from the run's rows.

use std::fmt::Write as _;
use std::path::Path;

use alint_core::relation::{LimitKind, Limits};
use alint_core::{Applicability, Engine, FixReport, Report};

use super::super::super::constraint::load_tree;
use super::{DEPENDENCIES, MANIFESTS, engine, failures, files, index_under, walk_options};

/// The keys of each `i18n/` file `key_parity` reads.
const KEYS: usize = 50;

/// The rows a key of `deps` names: one per dependency of every manifest.
const DEPS_ROWS: usize = MANIFESTS * DEPENDENCIES;

/// A configuration of relations `deps` (each manifest's dependencies) and
/// `names` (each manifest's name), then `rules`, each named for its kind.
fn config(rules: &[&str]) -> String {
    let mut config = String::from(
        "version: 2\nrelations:\n  deps:\n    from: { paths: \"pkgs/*.json\" }\n    entries: { \
         format: json, at: \"$.dependencies\" }\n    fields: { d: entry.key, r: entry.value \
         }\n  names:\n    from: { paths: \"pkgs/*.json\" }\n    read: { json: \"$.name\" }\n    \
         fields: { n: value }\nrules:\n",
    );
    for rule in rules {
        match *rule {
            // Every manifest's dependencies agree, and its name is its own.
            "deps" | "deps2" => write!(
                config,
                "  - id: {rule}\n    kind: constraint\n    input: deps\n    assert: {{ unique: {{ \
                 key: [d], determines: [r] }} }}\n    level: error\n"
            ),
            "names" => write!(
                config,
                "  - id: names\n    kind: constraint\n    input: names\n    assert: {{ unique: {{ \
                 key: [n] }} }}\n    level: error\n"
            ),
            "readme" => write!(
                config,
                "  - id: readme\n    kind: file_exists\n    paths: README.md\n    level: error\n"
            ),
            "make" => write!(
                config,
                "  - id: make\n    kind: file_exists\n    paths: MADE.md\n    level: error\n    \
                 fix:\n      file_create:\n        content: \"made\\n\"\n"
            ),
            // `t/a.json`'s version is `src.json`'s, once synced.
            "ver" => write!(
                config,
                "  - id: ver\n    kind: cross_file\n    relation: equals\n    source: {{ file: \
                 src.json, extract: {{ json: \"$.v\" }} }}\n    targets: {{ files: \"t/*.json\", \
                 extract: {{ json: \"$.v\" }} }}\n    level: error\n    fix: {{ sync_from: {{ \
                 applicability: safe }} }}\n"
            ),
            "kp1" | "kp2" => write!(
                config,
                "  - id: {rule}\n    kind: key_parity\n    reference: i18n/en.json\n    peers: \
                 \"i18n/*.json\"\n    level: error\n"
            ),
            // The manifests end without a newline.
            "newline" => write!(
                config,
                "  - id: newline\n    kind: final_newline\n    paths: \"pkgs/*.json\"\n    level: \
                 error\n    fix: {{ file_append_final_newline: {{}} }}\n"
            ),
            "remove" => write!(
                config,
                "  - id: remove\n    kind: file_absent\n    paths: pkgs/bad.json\n    level: \
                 error\n    fix: {{ file_remove: {{}} }}\n"
            ),
            other => panic!("no rule {other}"),
        }
        .unwrap();
    }
    config
}

/// The manifests, `README.md`, `src.json` and `t/a.json` (whose versions
/// differ), two `i18n/` files of `KEYS` keys each, and `extra`.
fn tree(rules: &[&str], extra: &[(&str, &str)]) -> (tempfile::TempDir, Engine) {
    let keys: Vec<String> = (0..KEYS).map(|key| format!("\"k{key}\": \"v\"")).collect();
    let keys = format!("{{{}}}\n", keys.join(", "));
    let mut files = files();
    for (path, body) in [
        ("README.md", "hi\n"),
        ("src.json", "{\"v\": \"2\"}\n"),
        ("t/a.json", "{\"v\": \"1\"}\n"),
        ("i18n/en.json", keys.as_str()),
        ("i18n/fr.json", keys.as_str()),
    ]
    .into_iter()
    .chain(extra.iter().copied())
    {
        files.push((path.to_string(), body.to_string()));
    }
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_str()))
        .collect();
    let (dir, loaded) = load_tree(&config(rules), &files);
    let rules = engine(&loaded.unwrap());
    (dir, rules)
}

fn rows(limit: usize) -> Limits {
    Limits::default()
        .with(LimitKind::RowsPerRun, limit)
        .unwrap()
}

fn check_failures(report: &Report) -> Vec<String> {
    report
        .results
        .iter()
        .flat_map(|result| &result.analysis_failures)
        .map(|failure| failure.message.to_string())
        .collect()
}

/// The pass of `fix` over `index`, the first: its index is the one given.
fn fix(rules: &Engine, root: &Path, limits: &Limits, dry_run: bool) -> (FixReport, usize) {
    let index = index_under(root, limits);
    let report = rules
        .fix(
            root,
            &index,
            &walk_options(),
            dry_run,
            Applicability::Unsafe,
        )
        .unwrap();
    (report, index.decode_cache().unwrap().decodes())
}

/// `check --format sarif` stages its fixes on the index `check` evaluated:
/// staging decodes nothing, takes no row and records nothing again, so a
/// rule before the relations (`readme`) costs `ver` nothing, and its fix is
/// attached. Under room for half the relation's rows again, clearing the
/// caches before the relations were evaluated again failed `deps` and `ver`
/// on `rows_per_run`, and no fix was attached.
#[test]
fn staging_on_the_index_check_evaluated_reads_nothing_again() {
    let (dir, rules) = tree(&["readme", "deps", "ver"], &[]);
    let limits = rows(DEPS_ROWS + DEPS_ROWS / 2);
    let index = index_under(dir.path(), &limits);
    let report = rules.run(dir.path(), &index).unwrap();
    assert_eq!(check_failures(&report), Vec::<String>::new());
    let cache = index.decode_cache().unwrap();
    let checked = (cache.decodes(), cache.relations_recorded());
    let (staging, staged) = rules
        .stage_fixes(dir.path(), &index, Applicability::Safe)
        .unwrap();
    assert_eq!((failures(&staging), staged.len()), (Vec::new(), 1));
    assert_eq!((cache.decodes(), cache.relations_recorded()), checked);

    let index = index_under(dir.path(), &limits);
    let mut report = rules.run(dir.path(), &index).unwrap();
    alint_core::attach_proposed_edits(&rules, &mut report, dir.path(), &index);
    let ver = report
        .results
        .iter()
        .find(|result| &*result.rule_id == "ver")
        .unwrap();
    assert_eq!(ver.violations.len(), 1);
    assert_eq!(ver.violations[0].proposed_edits.len(), 1);
}

/// `key_parity` rules before and after the relations: `fix --dry-run`,
/// `fix --diff` and `fix` decode the `i18n/` files once, as `check` does,
/// so under room for `check`'s rows alone none fails. Clearing the caches
/// before the relations were evaluated made `kp2` decode them again and
/// fail on `rows_per_run`.
#[test]
fn a_rule_decoding_before_the_relations_is_not_decoded_again() {
    let (dir, rules) = tree(&["kp1", "deps", "kp2"], &[]);
    let limits = rows(DEPS_ROWS + 2 * KEYS + 10);
    let index = index_under(dir.path(), &limits);
    let report = rules.run(dir.path(), &index).unwrap();
    assert_eq!(check_failures(&report), Vec::<String>::new());
    let checked = index.decode_cache().unwrap().decodes();
    assert_eq!(checked, MANIFESTS + 2);
    let (dry_run, decodes) = fix(&rules, dir.path(), &limits, true);
    assert_eq!((failures(&dry_run), decodes), (Vec::new(), checked));
    let index = index_under(dir.path(), &limits);
    let (staging, _) = rules
        .stage_fixes(dir.path(), &index, Applicability::Unsafe)
        .unwrap();
    assert_eq!(failures(&staging), Vec::<String>::new());
    assert_eq!(index.decode_cache().unwrap().decodes(), checked);
    let (fixed, decodes) = fix(&rules, dir.path(), &limits, false);
    assert_eq!((failures(&fixed), decodes), (Vec::new(), checked));
}

/// Relation `names`, a create, then relation `deps`: the first pass of `fix`
/// decodes each manifest once for each, as `fix --diff` does. With room for
/// both relations' rows and `src.json`'s, `ver` syncs `t/a.json` in that pass.
/// Clearing the caches after the create made `deps` decode the manifests
/// again, and `ver` ran out of rows until the next pass.
#[test]
fn a_create_between_two_relations_reads_neither_again() {
    let (dir, rules) = tree(&["names", "make", "deps"], &[]);
    let (fixed, decodes) = fix(&rules, dir.path(), &Limits::default(), false);
    assert_eq!((failures(&fixed), decodes), (Vec::new(), 2 * MANIFESTS));
    assert!(dir.path().join("MADE.md").is_file());
    let (dir, rules) = tree(&["names", "make", "deps"], &[]);
    let index = index_under(dir.path(), &Limits::default());
    rules
        .stage_fixes(dir.path(), &index, Applicability::Unsafe)
        .unwrap();
    assert_eq!(index.decode_cache().unwrap().decodes(), 2 * MANIFESTS);

    let (dir, rules) = tree(&["names", "make", "deps", "ver"], &[]);
    let limits = rows(MANIFESTS + DEPS_ROWS + 1 + DEPS_ROWS / 2);
    let (fixed, decodes) = fix(&rules, dir.path(), &limits, false);
    assert_eq!(failures(&fixed), Vec::<String>::new());
    // `src.json` was decoded in the first pass, within the run's rows.
    assert_eq!(decodes, 2 * MANIFESTS + 1);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("t/a.json")).unwrap(),
        "{\"v\": \"2\"}\n"
    );
}

/// A content edit between two rules that read one relation reads nothing
/// again: the edit waits in the compose buffer until the pass ends, and
/// every rule of the pass reads the tree on disk, which is as it was.
/// Clearing the caches after it decoded every manifest again.
#[test]
fn a_content_edit_between_two_readers_reads_nothing_again() {
    let (dir, rules) = tree(&["deps", "newline", "deps2"], &[]);
    let (fixed, decodes) = fix(&rules, dir.path(), &Limits::default(), false);
    assert_eq!((failures(&fixed), decodes), (Vec::new(), MANIFESTS));
    let manifest = std::fs::read_to_string(dir.path().join("pkgs/p0.json")).unwrap();
    assert!(manifest.ends_with("}\n"), "{manifest:?}");
}

/// A removal between two rules that read one relation lets go of the removed
/// file's decode and of the relation's outcome, not of the other files'
/// decodes: `deps2` evaluates the relation again from them, and finds the
/// removed file gone. Clearing the caches after it decoded every other
/// manifest again. A removal before the first rule that reads the relation
/// comes before it is evaluated, so the removed file is never decoded.
#[test]
fn a_removal_between_two_readers_reads_only_the_removed_file_again() {
    let bad = "{\"name\": \"bad\", \"dependencies\": {\"dep0\": \"^1.0.0\"}}";
    let (dir, rules) = tree(&["deps", "remove", "deps2"], &[("pkgs/bad.json", bad)]);
    let (fixed, decodes) = fix(&rules, dir.path(), &Limits::default(), false);
    assert_eq!((failures(&fixed), decodes), (Vec::new(), MANIFESTS + 1));
    assert!(!dir.path().join("pkgs/bad.json").exists());
    let (dir, rules) = tree(&["remove", "deps"], &[("pkgs/bad.json", bad)]);
    let (fixed, decodes) = fix(&rules, dir.path(), &Limits::default(), false);
    assert_eq!((failures(&fixed), decodes), (Vec::new(), MANIFESTS));
}

/// A pass that writes, with a fix that may remove a file before the first
/// rule that reads a relation, evaluates the relations just before that rule
/// (a removal would let go of what they read), after `kp1` decoded and took
/// bytes through the caches. What they took is recorded as `check` records
/// it, so a rule places its findings against the same amount: what their
/// evaluation added, not what the run had taken before it.
#[test]
fn the_relations_record_what_evaluating_them_took() {
    let (dir, rules) = tree(&["kp1", "remove", "deps", "names"], &[]);
    let index = index_under(dir.path(), &Limits::default());
    rules.run(dir.path(), &index).unwrap();
    let checked = index.decode_cache().unwrap().relations_recorded();
    assert!(checked.is_some_and(|taken| taken > 0));
    let index = index_under(dir.path(), &Limits::default());
    let fixed = rules
        .fix(
            dir.path(),
            &index,
            &walk_options(),
            false,
            Applicability::Unsafe,
        )
        .unwrap();
    assert_eq!(failures(&fixed), Vec::<String>::new());
    let cache = index.decode_cache().unwrap();
    assert_eq!(cache.relations_recorded(), checked);
    assert!(
        cache.bytes_taken() > checked.unwrap(),
        "`kp1` took bytes too"
    );
}

/// A relation whose selector keeps the files a `scope_filter:` manifest
/// lists, read by the relation itself, and a create of that manifest between
/// two rules that read it: the second evaluates the relation again, and
/// reads the files the new manifest lists, as it read them when every fix
/// cleared the caches. The manifest's absence the first evaluation found is
/// not kept.
#[test]
fn a_create_of_a_manifest_a_relation_read_evaluates_it_again() {
    let config = "version: 2\nrelations:\n  members:\n    from:\n      paths: \"pkgs/*.json\"\n      \
                  scope_filter:\n        include_manifest_paths:\n          source: members.json\n          \
                  extract: { json: \"$.members[*]\" }\n    entries: { format: json, at: \
                  \"$.dependencies\" }\n    fields: { d: entry.key }\nrules:\n  - id: first\n    kind: \
                  constraint\n    input: members\n    assert: { unique: { key: [d] } }\n    level: \
                  warning\n  - id: make\n    kind: file_exists\n    paths: members.json\n    level: \
                  error\n    fix:\n      file_create:\n        content: \"{\\\"members\\\": \
                  [\\\"pkgs\\\"]}\\n\"\n  - id: second\n    kind: constraint\n    input: members\n    \
                  assert: { unique: { key: [d] } }\n    level: warning\n";
    let files = files();
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_str()))
        .collect();
    let (dir, loaded) = load_tree(config, &files);
    let rules = engine(&loaded.unwrap());
    let (_, decodes) = fix(&rules, dir.path(), &Limits::default(), false);
    assert!(dir.path().join("members.json").is_file());
    assert_eq!(decodes, MANIFESTS);
}
