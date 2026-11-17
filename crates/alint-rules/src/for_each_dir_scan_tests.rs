//! A rule nested under `for_each_dir` with a glob that starts with the
//! iterated directory (`{path}/*.md`) tests only the entries of that
//! directory, not the whole index once per iteration (ENG-4 of the second
//! post-v0.22.1 audit: 2,000 / 4,000 / 8,000 directories took 0.49 s /
//! 1.29 s / 4.50 s). The count of paths `Scope::matches` tested tells the two
//! apart; the findings show the narrowed rule still sees every file.

use std::collections::HashMap;
use std::path::Path;

use alint_core::{Context, FactValues, Rule, Violation};

use crate::test_support::spec_yaml;

/// Directories under `pkg/`, each with a README, two locale files and a
/// `sub/` directory; a few break one rule each.
const DIRS: usize = 60;

fn tree() -> tempfile::TempDir {
    let tmp = tempfile::Builder::new()
        .prefix("alint-scan-test-")
        .tempdir()
        .expect("tempdir");
    for i in 0..DIRS {
        let dir = tmp.path().join(format!("pkg/d{i:03}"));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/.keep"), b"").unwrap();
        if i != 0 {
            let readme: &[u8] = if i == 3 {
                b"# no newline"
            } else {
                b"# readme\n"
            };
            std::fs::write(dir.join("README.md"), readme).unwrap();
        }
        std::fs::write(dir.join("en.json"), br#"{"a": 1, "b": 2}"#).unwrap();
        let fr: &[u8] = if i == 1 {
            br#"{"a": 1}"#
        } else {
            br#"{"a": 1, "b": 2}"#
        };
        std::fs::write(dir.join("fr.json"), fr).unwrap();
        if i == 2 {
            std::fs::write(dir.join("junk.tmp"), b"x").unwrap();
        }
    }
    tmp
}

/// Evaluate a top-level rule and count the paths its scopes tested.
fn evaluate_counting(
    rule: &dyn Rule,
    root: &Path,
    index: &alint_core::FileIndex,
) -> (Vec<Violation>, u64) {
    let registry = crate::builtin_registry();
    let facts = FactValues::default();
    let vars = HashMap::new();
    let ctx = Context::new(root, index)
        .with_registry(Some(&registry))
        .with_facts(Some(&facts))
        .with_vars(Some(&vars));
    let before = alint_core::scope_match_count().unwrap();
    let violations = rule.evaluate(&ctx).expect("rule evaluates");
    (
        violations,
        alint_core::scope_match_count().unwrap() - before,
    )
}

#[test]
fn a_nested_glob_tests_only_the_iterated_directory() {
    if alint_core::scope_match_count().is_none() {
        eprintln!("built without debug assertions: nothing counts the tested paths");
        return;
    }
    let tmp = tree();
    let index = alint_core::walk(tmp.path(), &crate::test_support::walk_options()).unwrap();
    let entries = u64::try_from(index.entries.len()).unwrap();
    // (case, the rule's own lines, the nested rule's lines, findings)
    let cases: [(&str, &str, &str, &[&str]); 9] = [
        (
            "file_exists glob",
            "",
            "kind: file_exists\n    paths: '{path}/*.md'",
            &["pkg/d000"],
        ),
        (
            "file_absent glob",
            "",
            "kind: file_absent\n    paths: '{path}/*.tmp'",
            &["pkg/d002/junk.tmp"],
        ),
        (
            "a per-file kind read rule by rule",
            "",
            "kind: final_newline\n    paths: '{path}/*.md'",
            &["pkg/d003/README.md"],
        ),
        (
            "a kind with its own file loop",
            "",
            "kind: file_max_size\n    paths: '{path}/*.json'\n    max_bytes: 12",
            // Every `en.json` and every `fr.json` but d001's is 16 bytes.
            &[],
        ),
        (
            "dir_exists glob",
            "",
            "kind: dir_exists\n    paths: '{path}/s*'",
            &[],
        ),
        (
            "a nested for_each_dir's select",
            "",
            "kind: for_each_dir\n    select: '{path}/*'\n    require:\n      \
             - kind: dir_exists\n        paths: '{path}/sub'",
            &[],
        ),
        (
            "key_parity peers",
            "",
            "kind: key_parity\n    reference: '{path}/en.json'\n    peers: '{path}/*.json'",
            &["pkg/d001/fr.json"],
        ),
        (
            "expect_matches",
            "",
            "kind: file_max_size\n    paths: '{path}/*.md'\n    max_bytes: 100\n    \
             expect_matches: true",
            &["pkg/d000"],
        ),
        (
            "iter.has_file",
            "when_iter: 'iter.has_file(\"*.md\")'\n",
            "kind: file_exists\n    paths: '{path}/README.md'",
            // Only the directories with a README are selected (d000 has none).
            &[],
        ),
    ];
    let mut whole_index = Vec::new();
    for (case, own, nested, expected) in cases {
        let rule = crate::builtin_registry()
            .build(&spec_yaml(&format!(
                "id: t\nkind: for_each_dir\nselect: 'pkg/*'\nlevel: error\n{own}\
                 require:\n  - {nested}\n"
            )))
            .unwrap_or_else(|error| panic!("{case}: {error}"));
        let (violations, tested) = evaluate_counting(rule.as_ref(), tmp.path(), &index);
        let mut paths: Vec<String> = violations
            .iter()
            .map(|v| {
                let path = v.path.as_deref().unwrap_or(Path::new("<none>"));
                path.to_string_lossy().replace('\\', "/")
            })
            .collect();
        paths.sort();
        let expected = if case == "a kind with its own file loop" {
            // 16-byte files over a 12-byte limit: all but d001's `fr.json`.
            let mut all: Vec<String> = (0..DIRS)
                .flat_map(|i| {
                    let mut files = vec![format!("pkg/d{i:03}/en.json")];
                    if i != 1 {
                        files.push(format!("pkg/d{i:03}/fr.json"));
                    }
                    files
                })
                .collect();
            all.sort();
            all
        } else {
            expected.iter().map(|path| (*path).to_owned()).collect()
        };
        assert_eq!(paths, expected, "{case}: {violations:?}");
        // The parent's `select:` tests the directories under `pkg/` once, and
        // each iteration the few entries of its own directory: fewer than two
        // tests an entry. The whole index once per iteration would be DIRS
        // times the entries (from 3,841 to 21,658 tests here).
        if tested > 2 * entries {
            whole_index.push(format!("{case}: {tested} paths tested"));
        }
    }
    assert!(
        whole_index.is_empty(),
        "{entries} entries in {DIRS} directories: {whole_index:#?}"
    );
}
