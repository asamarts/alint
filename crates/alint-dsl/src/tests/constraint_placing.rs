//! Whether a `constraint` rule's findings fit when it places them is the same
//! in `fix` as in `check`: both evaluate the relations the rules read before
//! any rule runs, and record what they took of the run's bytes, so a rule is
//! checked against what the relations left, not against what the relations
//! of the rules before it took. After a fix clears the caches, the relations
//! are evaluated again and recorded anew, as a check of the tree as it is
//! then would.

use alint_core::relation::{DecodeCache, LimitKind, Limits};
use alint_core::{Applicability, Engine, FileIndex, RuleResult, WalkOptions, walk};

use super::super::constraint::load_tree;

/// Rule `a` over `a.json`'s entries, whose two values repeat, so it has one
/// finding to place in a file padded to 60 KB; and rule `b` over
/// `big.json`'s 400 long keys, which its relation holds. Before them, with
/// `create`, rule `make` creates `MADE.md`, a fix applied before the
/// constraint rules run.
fn config(create: bool) -> String {
    let make = if create {
        "  - id: make\n    kind: file_exists\n    paths: MADE.md\n    level: error\n    fix:\n      \
         file_create:\n        content: \"made\\n\"\n"
    } else {
        ""
    };
    format!(
        "version: 2\nrelations:\n  ra:\n    from: {{ paths: a.json }}\n    entries: {{ format: \
         json, at: \"$\" }}\n    fields: {{ l: entry.value }}\n  rb:\n    from: {{ paths: big.json \
         }}\n    entries: {{ format: json, at: \"$\" }}\n    fields: {{ k: entry.key }}\nrules:\n\
         {make}  - id: a\n    kind: constraint\n    input: ra\n    assert: {{ unique: {{ key: [l] \
         }} }}\n    level: error\n  - id: b\n    kind: constraint\n    input: rb\n    assert: {{ \
         unique: {{ key: [k] }} }}\n    level: error\n"
    )
}

fn files() -> Vec<(String, String)> {
    let a = format!("{{\"a\": \"dup\",{}\"b\": \"dup\"}}", "\n".repeat(60_000));
    let keys: Vec<String> = (0..400)
        .map(|index| format!("\"{index:04}{}\": {index}", "k".repeat(60)))
        .collect();
    let big = format!("{{{}}}", keys.join(", "));
    vec![("a.json".into(), a), ("big.json".into(), big)]
}

fn engine(config: &alint_core::Config) -> Engine {
    let mut registry = alint_core::RuleRegistry::new();
    alint_rules::register_builtin(&mut registry);
    let entries = config
        .rules
        .iter()
        .map(|spec| alint_core::RuleEntry::new(registry.build(spec).unwrap()))
        .collect();
    Engine::from_entries(entries, registry)
}

fn index_under(root: &std::path::Path, limits: &Limits) -> FileIndex {
    let index = walk(root, &walk_options()).unwrap();
    index.enable_relation_caches_under(limits);
    index
}

fn walk_options() -> WalkOptions {
    WalkOptions::default().with_global_git_excludes(false)
}

fn cache(index: &FileIndex) -> &DecodeCache {
    index.decode_cache().unwrap()
}

/// Rule `a`'s outcome: its findings, and the limit its failures name.
fn outcome(findings: usize, failures: &[alint_core::AnalysisFailure]) -> (usize, Vec<bool>) {
    (
        findings,
        failures
            .iter()
            .map(|failure| failure.message.contains("`bytes_per_run`"))
            .collect(),
    )
}

fn checked(results: &[RuleResult]) -> (usize, Vec<bool>) {
    let a = results
        .iter()
        .find(|result| &*result.rule_id == "a")
        .unwrap();
    outcome(a.violations.len(), &a.analysis_failures)
}

/// Over limits from where `a`'s file does not fit beside the relations to
/// where it does, `fix --dry-run` places `a`'s finding, or fails to, as
/// `check` does. Rule `a` runs first: evaluating its relation alone, as
/// `fix` did, left out what `b`'s took, and placed the finding where
/// `check` failed.
#[test]
fn fix_places_a_finding_where_check_does_under_every_limit() {
    let config = config(false);
    let files = files();
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_str()))
        .collect();
    let (dir, loaded) = load_tree(&config, &files);
    let loaded = loaded.unwrap();
    let engine = engine(&loaded);
    let mut seen = std::collections::BTreeSet::new();
    // The relations take about 210 KB: `a`'s file fits from about 275 KB.
    for limit in (200_000..=360_000).step_by(8_000) {
        let limits = Limits::default()
            .with(LimitKind::BytesPerRun, limit)
            .unwrap();
        let index = index_under(dir.path(), &limits);
        let check = checked(&engine.run(dir.path(), &index).unwrap().results);
        let index = index_under(dir.path(), &limits);
        let fix = engine
            .fix(
                dir.path(),
                &index,
                &walk_options(),
                true,
                Applicability::Unsafe,
            )
            .unwrap();
        let a = fix
            .results
            .iter()
            .find(|result| &*result.rule_id == "a")
            .map_or((0, Vec::new()), |a| {
                outcome(a.items.len(), &a.analysis_failures)
            });
        assert_eq!(a, check, "under {limit} bytes");
        seen.insert(check);
    }
    // The sweep passes from a placing failure to a placed finding.
    assert!(seen.contains(&(0, vec![true])), "{seen:?}");
    assert!(seen.contains(&(1, vec![])), "{seen:?}");
}

/// A fix applied before the constraint rules run clears the caches; the
/// relations are evaluated again before the rules after it run, and what
/// they take is recorded anew, so `a` fits, or fails to, as in a check of
/// the fixed tree. Recording what the relations took when `a` first asked
/// counted `a`'s relation alone, and `a`'s file fitted where a check of the
/// tree it fixed refused it.
#[test]
fn after_a_fix_a_finding_is_placed_as_a_check_of_the_fixed_tree_places_it() {
    let files = files();
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_str()))
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    for limit in (200_000..=360_000).step_by(16_000) {
        let (dir, loaded) = load_tree(&config(true), &files);
        let engine = engine(&loaded.unwrap());
        let limits = Limits::default()
            .with(LimitKind::BytesPerRun, limit)
            .unwrap();
        // The first pass runs over this index, under the limit.
        let first = index_under(dir.path(), &limits);
        engine
            .fix(
                dir.path(),
                &first,
                &walk_options(),
                false,
                Applicability::Unsafe,
            )
            .unwrap();
        assert!(dir.path().join("MADE.md").is_file(), "the fix applied");
        let fixed = index_under(dir.path(), &limits);
        let check = checked(&engine.run(dir.path(), &fixed).unwrap().results);
        assert_eq!(
            (
                cache(&first).may_hold_refusals(),
                cache(&first).largest_held()
            ),
            (
                cache(&fixed).may_hold_refusals(),
                cache(&fixed).largest_held()
            ),
            "under {limit} bytes, where a check of the fixed tree gives {check:?}"
        );
        seen.insert(check);
    }
    assert!(seen.contains(&(0, vec![true])), "{seen:?}");
    assert!(seen.contains(&(1, vec![])), "{seen:?}");
}

/// What the relations took counts their rows, which the relation memo
/// holds, with their decodes' key paths, which the decode cache holds: a
/// limit with room for the decodes and `a`'s file, but not the rows, gives
/// `a` no verdict. Counting the decodes alone placed the finding.
#[test]
fn what_the_relations_took_counts_their_rows_with_their_decodes() {
    let config = config(false);
    let files = files();
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_str()))
        .collect();
    let (dir, loaded) = load_tree(&config, &files);
    let engine = engine(&loaded.unwrap());
    let run = |limits: &Limits| {
        let index = index_under(dir.path(), limits);
        let results = engine.run(dir.path(), &index).unwrap().results;
        let cache = cache(&index);
        (
            checked(&results),
            cache.bytes_taken(),
            cache.relations_recorded().unwrap(),
            cache.largest_held(),
        )
    };
    let (placed, decodes, relations, held) = run(&Limits::default());
    assert_eq!(placed, (1, vec![]));
    assert!(relations > decodes, "{relations} {decodes}");
    let limits = Limits::default()
        .with(LimitKind::BytesPerRun, decodes + held)
        .unwrap();
    assert_eq!(run(&limits).0, (0, vec![true]));
    let limits = Limits::default()
        .with(LimitKind::BytesPerRun, relations + held)
        .unwrap();
    assert_eq!(run(&limits).0, (1, vec![]));
}
