//! A rule nested under `for_each_dir` with a glob that starts with the
//! iterated directory (`{path}/*.md`) tests only the entries of that
//! directory, not the whole index once per iteration (ENG-4 of the second
//! post-v0.22.1 audit: 2,000 / 4,000 / 8,000 directories took 0.49 s /
//! 1.29 s / 4.50 s). The count of paths `Scope::matches` tested tells the two
//! apart; the findings show the narrowed rule still sees every file.
//!
//! Every place a rule tests a scope against the index has a case here, so
//! scanning the whole index there again fails a test (ENG-4 of the third
//! post-v0.22.1 audit): a kind's own `evaluate` loop, the per-file dispatch,
//! `scope_matches_any` (`expect_matches: true`), `iter.has_file`, a nested
//! iteration's selection, and the sensitive-path visitors. `no_submodules` has
//! none: its scope is the fixed root-level `.gitmodules`, whose candidates are
//! every entry (a root-level pattern starts with no directory), so its loop
//! tests the whole index however it is written. A pattern that is the iterated
//! directory itself (`{path}`) starts with its parent's name only, so it reads
//! every sibling; the cases select below it (`{path}/s*`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use alint_core::{Context, FactValues, Rule, Violation};

use crate::test_support::spec_yaml;

/// Directories under `pkg/`, each with a README, two locale files, a member
/// list naming `sub` and a `sub/` directory; a few break one rule each.
const DIRS: usize = 60;

/// A `file_graph` whose every `en.json` derives its `fr.json`.
const FILE_GRAPH: &str = "kind: file_graph\n    nodes: '{path}/*.json'\n    edges:\n      \
                          derive_target:\n        from: '(.*)en\\.json$'\n        \
                          to: '${1}fr.json'\n    require: no_dangling";

fn tree() -> tempfile::TempDir {
    let tmp = tempfile::Builder::new()
        .prefix("alint-scan-test-")
        .tempdir()
        .expect("tempdir");
    std::fs::write(
        tmp.path().join("schema.json"),
        br#"{"type": "object", "required": ["a"]}"#,
    )
    .unwrap();
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
        std::fs::write(dir.join("en.json"), br#"{"a": "1", "b": "2"}"#).unwrap();
        let fr: &[u8] = if i == 1 {
            br#"{"a": "1"}"#
        } else {
            br#"{"a": "1", "b": "2"}"#
        };
        std::fs::write(dir.join("fr.json"), fr).unwrap();
        std::fs::write(dir.join("members.txt"), b"sub\n").unwrap();
        if i == 2 {
            std::fs::write(dir.join("junk.tmp"), b"x").unwrap();
            std::fs::write(dir.join("sub/junk.tmp"), b"x").unwrap();
        }
    }
    tmp
}

/// A `for_each_dir` over `pkg/*` with the parent's own lines and one nested
/// rule, and the paths its findings name (sorted).
struct Case {
    name: &'static str,
    own: &'static str,
    nested: &'static str,
    expected: Vec<String>,
}

fn case(name: &'static str, nested: &'static str, expected: &[&str]) -> Case {
    Case {
        name,
        own: "",
        nested,
        expected: expected.iter().map(|path| (*path).to_owned()).collect(),
    }
}

/// Every iterated directory, `d000` to `d059`, except `but`, followed by
/// `below` (`/en.json`, or nothing for the directory itself).
fn in_every_dir(below: &str, but: &[usize]) -> Vec<String> {
    (0..DIRS)
        .filter(|i| !but.contains(i))
        .map(|i| format!("pkg/d{i:03}{below}"))
        .collect()
}

fn build(case: &Case) -> Box<dyn Rule> {
    crate::builtin_registry()
        .build(&spec_yaml(&format!(
            "id: t\nkind: for_each_dir\nselect: 'pkg/*'\nlevel: error\n{}\
             require:\n  - {}\n",
            case.own, case.nested
        )))
        .unwrap_or_else(|error| panic!("{}: {error}", case.name))
}

/// Run `f` with a context like the engine's over `root` and count the paths
/// `Scope::matches` tested meanwhile.
fn counting<T>(
    root: &Path,
    index: &alint_core::FileIndex,
    f: impl FnOnce(&Context) -> T,
) -> (T, u64) {
    let registry = crate::builtin_registry();
    let facts = FactValues::default();
    let vars = HashMap::new();
    // `git_blame_age` reads through a blame cache; outside a repository every
    // lookup fails, and the rule skips the file after its scope selected it.
    let blame = alint_core::git::BlameCache::new(root.to_path_buf());
    let analysis_time = alint_core::AnalysisTimeResolution::Available {
        time: "2026-10-16".parse().unwrap(),
        source: alint_core::AnalysisTimeSource::Explicit,
    };
    let ctx = Context::new(root, index)
        .with_registry(Some(&registry))
        .with_facts(Some(&facts))
        .with_vars(Some(&vars))
        .with_git_blame(Some(&blame))
        .with_analysis_time(Some(&analysis_time));
    let before = alint_core::scope_match_count().unwrap();
    let out = f(&ctx);
    (out, alint_core::scope_match_count().unwrap() - before)
}

/// The parent's `select:` tests the directories under `pkg/` once, and each
/// iteration the few entries of its own directory once for each scope the rule
/// reads: a few tests an entry. The whole index once in each iteration would
/// be DIRS times the entries (thousands of tests more here).
fn scans_whole_index(tested: u64, entries: u64) -> bool {
    tested > 5 * entries
}

fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn finding_paths(violations: &[Violation]) -> Vec<String> {
    let mut paths: Vec<String> = violations
        .iter()
        .map(|v| slashed(v.path.as_deref().unwrap_or(Path::new("<none>"))))
        .collect();
    paths.sort();
    paths
}

/// The cases each rule's `evaluate` reads its scope in.
#[allow(clippy::too_many_lines)]
fn evaluated_cases() -> Vec<Case> {
    let mut cases = vec![
        case(
            "file_exists glob",
            "kind: file_exists\n    paths: '{path}/*.md'",
            &["pkg/d000"],
        ),
        case(
            "file_absent glob",
            "kind: file_absent\n    paths: '{path}/*.tmp'",
            &["pkg/d002/junk.tmp"],
        ),
        case(
            "a per-file kind read rule by rule",
            "kind: final_newline\n    paths: '{path}/*.md'",
            &["pkg/d003/README.md"],
        ),
        Case {
            // Every `en.json` and every `fr.json` but d001's is 20 bytes.
            expected: {
                let mut all = in_every_dir("/en.json", &[]);
                all.extend(in_every_dir("/fr.json", &[1]));
                all.sort();
                all
            },
            ..case(
                "a kind with its own file loop",
                "kind: file_max_size\n    paths: '{path}/*.json'\n    max_bytes: 12",
                &[],
            )
        },
        case(
            "dir_exists glob",
            "kind: dir_exists\n    paths: '{path}/s*'",
            &[],
        ),
        case(
            "a nested for_each_dir's select",
            "kind: for_each_dir\n    select: '{path}/*'\n    require:\n      \
             - kind: dir_exists\n        paths: '{path}/sub'",
            &[],
        ),
        case(
            "a nested for_each_file's select",
            "kind: for_each_file\n    select: '{path}/*.json'\n    require:\n      \
             - kind: file_exists\n        paths: '{path}/README.md'",
            &["pkg/d000/en.json", "pkg/d000/fr.json"],
        ),
        case(
            "a nested every_matching_has's select",
            // The outermost iteration substitutes `{path}` at every depth.
            "kind: every_matching_has\n    select: '{path}/s*'\n    require:\n      \
             - kind: dir_exists\n        paths: '{path}/sub'",
            &[],
        ),
        case(
            "key_parity peers",
            "kind: key_parity\n    reference: '{path}/en.json'\n    peers: '{path}/*.json'",
            &["pkg/d001/fr.json"],
        ),
        case(
            "expect_matches",
            "kind: file_max_size\n    paths: '{path}/*.md'\n    max_bytes: 100\n    \
             expect_matches: true",
            &["pkg/d000"],
        ),
        Case {
            own: "when_iter: 'iter.has_file(\"*.md\")'\n",
            // Only the directories with a README are selected (d000 has none).
            ..case(
                "iter.has_file",
                "kind: file_exists\n    paths: '{path}/README.md'",
                &[],
            )
        },
        case(
            "pair",
            "kind: pair\n    primary: '{path}/*.json'\n    partner: '{path}/README.md'",
            &["pkg/d000/en.json", "pkg/d000/fr.json"],
        ),
        Case {
            // Nested, the key is the directory's, so its two locale files share it.
            expected: in_every_dir("/en.json", &[]),
            ..case(
                "unique_by",
                "kind: unique_by\n    select: '{path}/*.json'\n    key: '{path}'",
                &[],
            )
        },
        Case {
            // Only d002's `sub/` holds a `.tmp` file.
            expected: in_every_dir("/sub", &[2]),
            ..case(
                "dir_contains",
                "kind: dir_contains\n    select: '{path}/s*'\n    require: ['*.tmp']",
                &[],
            )
        },
        case(
            "dir_only_contains",
            "kind: dir_only_contains\n    select: '{path}/s*'\n    allow: ['.keep']",
            &["pkg/d002/sub/junk.tmp"],
        ),
        Case {
            // `a` and `b` differ, so every finding names the source glob's files.
            expected: in_every_dir("/en.json", &[]),
            ..case(
                "cross_file over a source glob and a target glob",
                "kind: cross_file\n    relation: set_equals\n    \
                 source: { files: '{path}/f*.json', extract: { json: '$.a' } }\n    \
                 targets: { files: '{path}/e*.json', extract: { json: '$.b' } }",
                &[],
            )
        },
        case(
            "cross_file identical over a target glob",
            "kind: cross_file\n    relation: identical\n    \
             source: { file: '{path}/en.json' }\n    targets: { files: '{path}/*.json' }",
            &["pkg/d001/fr.json"],
        ),
        case(
            "cross_file registered over a source glob",
            "kind: cross_file\n    relation: registered\n    \
             source: { files: '{path}/*.json' }\n    register_as: '1'\n    \
             targets: [ { file: '{path}/en.json', extract: { json: '$.*' } } ]",
            &[],
        ),
        case(
            "no_symlinks",
            "kind: no_symlinks\n    paths: '{path}/*'\n    expect_matches: true",
            &[],
        ),
        case(
            "registry_paths_resolve",
            "kind: registry_paths_resolve\n    source: '{path}/m*.txt'\n    \
             extract: { regex: '(\\w+)' }\n    entries_are_globs: true\n    \
             orphans: { space: '{path}/s*' }",
            &[],
        ),
        case("file_graph", FILE_GRAPH, &[]),
        case(
            "markdown_links_resolve",
            "kind: markdown_links_resolve\n    paths: '{path}/*.md'\n    expect_matches: true",
            &["pkg/d000"],
        ),
        case(
            "markdown_fragments_resolve",
            "kind: markdown_fragments_resolve\n    paths: '{path}/*.md'\n    expect_matches: true",
            &["pkg/d000"],
        ),
        case(
            "editorconfig_conforms",
            "kind: editorconfig_conforms\n    paths: '{path}/*.md'\n    expect_matches: true",
            &["pkg/d000"],
        ),
        case(
            "a structured path kind",
            "kind: json_path_equals\n    paths: '{path}/*.json'\n    path: '$.b'\n    \
             equals: '2'",
            &["pkg/d001/fr.json"],
        ),
        case(
            "json_schema_passes",
            "kind: json_schema_passes\n    paths: '{path}/*.json'\n    schema_path: schema.json",
            &[],
        ),
        case(
            "pair_hash",
            "kind: pair_hash\n    source: '{path}/*.tmp'\n    target: '{path}/README.md'",
            &["pkg/d000/README.md", "pkg/d002/junk.tmp"],
        ),
        Case {
            // A missing target names every source as a related location.
            expected: in_every_dir("/x.md", &[]),
            ..case(
                "pair_hash with no target",
                "kind: pair_hash\n    source: '{path}/*.json'\n    target: '{path}/x.md'",
                &[],
            )
        },
        case(
            "git_blame_age",
            // Nothing matches, so no `git blame` runs; the loop still reads the
            // directory's files.
            "kind: git_blame_age\n    paths: '{path}/*.none'\n    pattern: 'x'\n    \
             max_age_days: 1",
            &[],
        ),
        case(
            "filename_case",
            "kind: filename_case\n    paths: '{path}/*.json'\n    case: snake",
            &[],
        ),
        case(
            "filename_regex",
            "kind: filename_regex\n    paths: '{path}/*.json'\n    pattern: '^[a-z]+$'\n    \
             stem: true",
            &[],
        ),
        case(
            "no_illegal_windows_names",
            "kind: no_illegal_windows_names\n    paths: '{path}/*'",
            &[],
        ),
        case(
            "file_content_forbidden",
            "kind: file_content_forbidden\n    paths: '{path}/*.md'\n    pattern: 'newline'",
            &["pkg/d003/README.md"],
        ),
        case(
            "file_content_matches",
            "kind: file_content_matches\n    paths: '{path}/*.md'\n    pattern: 'readme'",
            &["pkg/d003/README.md"],
        ),
        case(
            "file_header",
            "kind: file_header\n    paths: '{path}/*.md'\n    pattern: '^# readme'",
            &["pkg/d003/README.md"],
        ),
        case(
            "file_footer",
            "kind: file_footer\n    paths: '{path}/*.md'\n    pattern: 'readme'",
            &["pkg/d003/README.md"],
        ),
        case(
            "file_starts_with",
            "kind: file_starts_with\n    paths: '{path}/*.md'\n    prefix: '# readme'",
            &["pkg/d003/README.md"],
        ),
        case(
            "file_ends_with",
            "kind: file_ends_with\n    paths: '{path}/*.md'\n    suffix: \"\\n\"",
            &["pkg/d003/README.md"],
        ),
        case(
            "file_is_text",
            "kind: file_is_text\n    paths: '{path}/*'",
            &[],
        ),
        case(
            "file_min_size",
            "kind: file_min_size\n    paths: '{path}/*.json'\n    min_bytes: 12",
            &["pkg/d001/fr.json"],
        ),
        case("no_bom", "kind: no_bom\n    paths: '{path}/*.md'", &[]),
        case(
            "no_empty_files",
            "kind: no_empty_files\n    paths: '{path}/*'",
            &[],
        ),
        case(
            "no_case_conflicts",
            "kind: no_case_conflicts\n    paths: '{path}/*'",
            &[],
        ),
        case(
            "max_directory_depth",
            "kind: max_directory_depth\n    paths: '{path}/**'\n    max_depth: 10",
            &[],
        ),
        case(
            "max_files_per_directory",
            "kind: max_files_per_directory\n    paths: '{path}/*'\n    max_files: 100",
            &[],
        ),
        case(
            "executable_bit",
            "kind: executable_bit\n    paths: '{path}/*.json'\n    require: false",
            &[],
        ),
        case(
            "executable_has_shebang",
            "kind: executable_has_shebang\n    paths: '{path}/*.json'",
            &[],
        ),
        case(
            "shebang_has_executable",
            "kind: shebang_has_executable\n    paths: '{path}/*.json'",
            &[],
        ),
        case(
            "dir_absent",
            "kind: dir_absent\n    paths: '{path}/x*'",
            &[],
        ),
    ];
    // These run `true`, which Windows does not have.
    if cfg!(unix) {
        cases.push(case(
            "command",
            "kind: command\n    paths: '{path}/*.tmp'\n    command: ['true']",
            &[],
        ));
        cases.push(Case {
            // Only d002 has an output to snapshot; the others report none.
            expected: in_every_dir("", &[2]),
            ..case(
                "generated_file_fresh",
                "kind: generated_file_fresh\n    outputs: '{path}/*.tmp'\n    command: ['true']",
                &[],
            )
        });
    }
    cases
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
    let mut whole_index = Vec::new();
    for case in evaluated_cases() {
        let rule = build(&case);
        let (violations, tested) = counting(tmp.path(), &index, |ctx| {
            rule.evaluate(ctx).expect("rule evaluates")
        });
        assert_eq!(
            finding_paths(&violations),
            case.expected,
            "{}: {violations:?}",
            case.name
        );
        if scans_whole_index(tested, entries) {
            whole_index.push(format!("{}: {tested} paths tested", case.name));
        }
    }
    assert!(
        whole_index.is_empty(),
        "{entries} entries in {DIRS} directories: {whole_index:#?}"
    );
}

/// The rules whose sensitive-path visitor reads its own scope, each visiting
/// every JSON file of the iterated directories.
#[test]
fn a_nested_rule_visits_only_the_iterated_directory() {
    if alint_core::scope_match_count().is_none() {
        eprintln!("built without debug assertions: nothing counts the tested paths");
        return;
    }
    let tmp = tree();
    let index = alint_core::walk(tmp.path(), &crate::test_support::walk_options()).unwrap();
    let entries = u64::try_from(index.entries.len()).unwrap();
    let mut json = in_every_dir("/en.json", &[]);
    json.extend(in_every_dir("/fr.json", &[]));
    json.sort();
    let cases = [
        case(
            "a kind's default visitor",
            "kind: file_max_size\n    paths: '{path}/*.json'\n    max_bytes: 100",
            &[],
        ),
        case(
            "a structured path kind",
            "kind: json_path_equals\n    paths: '{path}/*.json'\n    path: '$.a'\n    \
             equals: '1'",
            &[],
        ),
        case("file_graph", FILE_GRAPH, &[]),
        case(
            "cross_file",
            "kind: cross_file\n    relation: set_equals\n    \
             source: { files: '{path}/f*.json', extract: { json: '$.*' } }\n    \
             targets: { files: '{path}/e*.json', extract: { json: '$.*' } }",
            &[],
        ),
    ];
    let mut whole_index = Vec::new();
    for case in cases {
        let rule = build(&case);
        let (visited, tested) = counting(tmp.path(), &index, |ctx| {
            let mut visited: Vec<PathBuf> = Vec::new();
            rule.visit_sensitive_paths(true, ctx, &mut |path| visited.push(path.to_path_buf()));
            visited
        });
        let mut visited: Vec<String> = visited.iter().map(|path| slashed(path)).collect();
        visited.sort();
        visited.dedup();
        assert_eq!(visited, json, "{}", case.name);
        if scans_whole_index(tested, entries) {
            whole_index.push(format!("{}: {tested} paths tested", case.name));
        }
    }
    assert!(
        whole_index.is_empty(),
        "{entries} entries in {DIRS} directories: {whole_index:#?}"
    );
}
