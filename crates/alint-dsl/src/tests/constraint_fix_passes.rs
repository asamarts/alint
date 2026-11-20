//! What a pass of `fix` reads for the relations its rules read: each file
//! once, however many fixes come before the rules that read them, in a pass
//! that writes (`fix`) and in one that stages (`fix --diff`). Evaluating the
//! relations again after every applied fix read each file once per fix, and
//! took its rows again from the run's rows each time.

use std::fmt::Write as _;

use alint_core::relation::{LimitKind, Limits};
use alint_core::{Applicability, Engine, FileIndex, FixReport, WalkOptions, walk};

use super::super::constraint::load_tree;

const MANIFESTS: usize = 4;
const DEPENDENCIES: usize = 10;

/// `makes` rules `make{k}` that each create `MADE{k}.md`, a fix applied in
/// every pass that writes, then rule `deps` over every manifest's
/// dependencies, which agree, so it has no finding.
fn config(makes: usize) -> String {
    let mut config = String::from(
        "version: 2\nrelations:\n  deps:\n    from: { paths: \"pkgs/*.json\" }\n    entries: { \
         format: json, at: \"$.dependencies\" }\n    fields: { d: entry.key, r: entry.value \
         }\nrules:\n",
    );
    for k in 0..makes {
        write!(
            config,
            "  - id: make{k}\n    kind: file_exists\n    paths: MADE{k}.md\n    level: error\n    \
             fix:\n      file_create:\n        content: \"made\\n\"\n"
        )
        .unwrap();
    }
    config.push_str(
        "  - id: deps\n    kind: constraint\n    input: deps\n    assert: { unique: { key: [d], \
         determines: [r] } }\n    level: error\n",
    );
    config
}

/// `MANIFESTS` manifests of `DEPENDENCIES` dependencies each, all `^1.0.0`.
fn files() -> Vec<(String, String)> {
    (0..MANIFESTS)
        .map(|index| {
            let dependencies: Vec<String> = (0..DEPENDENCIES)
                .map(|dependency| format!("\"dep{dependency}\": \"^1.0.0\""))
                .collect();
            (
                format!("pkgs/p{index}.json"),
                format!(
                    "{{\"name\": \"p{index}\", \"dependencies\": {{{}}}}}",
                    dependencies.join(", ")
                ),
            )
        })
        .collect()
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

fn walk_options() -> WalkOptions {
    WalkOptions::default().with_global_git_excludes(false)
}

fn index_under(root: &std::path::Path, limits: &Limits) -> FileIndex {
    let index = walk(root, &walk_options()).unwrap();
    index.enable_relation_caches_under(limits);
    index
}

/// Every rule's analysis failures in `report`: none, and `fix --diff`
/// exits 0 for a tree whose rules all pass once fixed.
fn failures(report: &FixReport) -> Vec<String> {
    report
        .results
        .iter()
        .flat_map(|result| &result.analysis_failures)
        .map(|failure| failure.message.to_string())
        .collect()
}

/// A pass of `fix` decodes each manifest once, with no fix, one, or four
/// before the rule that reads them, and so does `fix --diff`.
#[test]
fn a_fix_pass_decodes_each_file_once_however_many_fixes_come_first() {
    let files = files();
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_str()))
        .collect();
    for makes in [0, 1, 4] {
        let (dir, loaded) = load_tree(&config(makes), &files);
        let rules = engine(&loaded.unwrap());
        // The first pass runs over this index.
        let index = index_under(dir.path(), &Limits::default());
        let report = rules
            .fix(
                dir.path(),
                &index,
                &walk_options(),
                false,
                Applicability::Unsafe,
            )
            .unwrap();
        assert_eq!(failures(&report), Vec::<String>::new());
        assert_eq!(dir.path().join("MADE0.md").is_file(), makes > 0);
        let decodes = index.decode_cache().unwrap().decodes();
        assert_eq!(decodes, MANIFESTS, "fix, {makes} fixes first");

        let (dir, loaded) = load_tree(&config(makes), &files);
        let rules = engine(&loaded.unwrap());
        let index = index_under(dir.path(), &Limits::default());
        let (report, staged) = rules
            .stage_fixes(dir.path(), &index, Applicability::Unsafe)
            .unwrap();
        assert_eq!(failures(&report), Vec::<String>::new());
        assert_eq!(staged.len(), makes, "each create is staged");
        assert!(!dir.path().join("MADE0.md").exists(), "nothing is written");
        let decodes = index.decode_cache().unwrap().decodes();
        assert_eq!(decodes, MANIFESTS, "fix --diff, {makes} fixes first");
    }
}

/// `fix --diff` with five fixes before the rule takes the relation's rows
/// once from the run's rows: under room for twice them, the rule has its
/// verdict and the report no analysis failure, so `fix --diff` exits 0.
/// Reading the relation again after each staged fix took them six times and
/// failed the rule on `rows_per_run`.
#[test]
fn fix_diff_takes_a_relations_rows_once_however_many_fixes_come_first() {
    let files = files();
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_str()))
        .collect();
    let (dir, loaded) = load_tree(&config(5), &files);
    let rules = engine(&loaded.unwrap());
    let limits = Limits::default()
        .with(LimitKind::RowsPerRun, 2 * MANIFESTS * DEPENDENCIES)
        .unwrap();
    let index = index_under(dir.path(), &limits);
    let (report, staged) = rules
        .stage_fixes(dir.path(), &index, Applicability::Unsafe)
        .unwrap();
    assert_eq!(staged.len(), 5);
    assert_eq!(failures(&report), Vec::<String>::new());
}

/// Rule `deps`, a fix, and rule `again` over the same relation: `fix --diff`
/// writes nothing, so it clears nothing, and `again` reads what `deps` read;
/// `fix` writes the created file, so `again` reads the tree again.
#[test]
fn fix_diff_clears_nothing_and_fix_reads_again_after_a_fix() {
    let config = config(0)
        + "  - id: make0\n    kind: file_exists\n    paths: MADE0.md\n    level: error\n    fix:\n      \
           file_create:\n        content: \"made\\n\"\n  - id: again\n    kind: constraint\n    \
           input: deps\n    assert: { unique: { key: [d], determines: [r] } }\n    level: error\n";
    let files = files();
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_str()))
        .collect();
    let (dir, loaded) = load_tree(&config, &files);
    let rules = engine(&loaded.unwrap());
    let index = index_under(dir.path(), &Limits::default());
    let (report, staged) = rules
        .stage_fixes(dir.path(), &index, Applicability::Unsafe)
        .unwrap();
    assert_eq!((failures(&report), staged.len()), (Vec::new(), 1));
    assert_eq!(index.decode_cache().unwrap().decodes(), MANIFESTS);

    let index = index_under(dir.path(), &Limits::default());
    rules
        .fix(
            dir.path(),
            &index,
            &walk_options(),
            false,
            Applicability::Unsafe,
        )
        .unwrap();
    assert!(dir.path().join("MADE0.md").is_file());
    assert_eq!(index.decode_cache().unwrap().decodes(), 2 * MANIFESTS);
}
