//! End-to-end contract for the load-time pattern hints (issue #343): a content
//! pattern whose `^` or `$` cannot do what it almost certainly means gets one
//! `alint: warning:` line on stderr from `check`, `fix` and `validate-config`.
//! The hints are advisory: findings, stdout and the exit code are unchanged,
//! and `--quiet` silences them.

use std::path::{Path, PathBuf};
use std::process::Output;

fn alint() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn run(root: &Path, args: &[&str]) -> Output {
    crate::hermetic::command(alint())
        .args(args)
        .current_dir(root)
        .output()
        .expect("spawn alint")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn warnings(out: &Output) -> Vec<String> {
    stderr(out)
        .lines()
        .filter(|line| line.starts_with("alint: warning: "))
        .map(str::to_owned)
        .collect()
}

/// The tree and config from issue #343, with `pattern` swapped in.
fn issue_tree(pattern: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "test in first line\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "# comment\ntest in 2. line\n").unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        format!(
            "version: 1\nrules:\n  - id: file_content\n    kind: file_content_forbidden\n    \
             paths: '*.txt'\n    pattern: '{pattern}'\n    level: error\n"
        ),
    )
    .unwrap();
    dir
}

const LEADING_CARET: &str = "alint: warning: rule `file_content`: `^` matches only at the start \
                             of the file, not at the start of each line; use `(?m)^` to anchor \
                             to lines, or `\\A` if you mean the start of the file";

#[test]
fn check_warns_once_and_keeps_findings_and_exit_code() {
    let dir = issue_tree("^test");
    let out = run(dir.path(), &["check", "--color", "never"]);
    assert_eq!(warnings(&out), vec![LEADING_CARET.to_string()]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "the a.txt finding still fails the run"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("a.txt") && !stdout.contains("b.txt"),
        "{stdout}"
    );

    // The suggested fix finds both files and no longer warns.
    let dir = issue_tree("(?m)^test");
    let out = run(dir.path(), &["check", "--color", "never"]);
    assert_eq!(warnings(&out), Vec::<String>::new(), "{}", stderr(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("a.txt") && stdout.contains("b.txt"),
        "{stdout}"
    );
}

#[test]
fn json_stdout_is_untouched_and_quiet_silences_the_warning() {
    let dir = issue_tree("^test");
    let out = run(dir.path(), &["check", "--format", "json"]);
    assert_eq!(warnings(&out).len(), 1);
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    assert!(report.get("results").is_some(), "{report}");

    let out = run(dir.path(), &["--quiet", "check", "--color", "never"]);
    assert_eq!(warnings(&out), Vec::<String>::new());
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn fix_warns_too() {
    let dir = issue_tree("^test");
    let out = run(dir.path(), &["fix", "--dry-run", "--color", "never"]);
    assert_eq!(warnings(&out), vec![LEADING_CARET.to_string()]);
}

#[test]
fn validate_config_reports_hints_in_both_formats_and_stays_valid() {
    let dir = issue_tree("^test");
    let out = run(dir.path(), &["validate-config"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(warnings(&out), vec![LEADING_CARET.to_string()]);

    let out = run(dir.path(), &["validate-config", "--format", "json"]);
    assert_eq!(out.status.code(), Some(0));
    let envelope: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(envelope["valid"], true);
    let expected = LEADING_CARET.trim_start_matches("alint: warning: ");
    assert_eq!(envelope["warnings"], serde_json::json!([expected]));

    let dir = issue_tree("(?m)^test");
    let out = run(dir.path(), &["validate-config", "--format", "json"]);
    let envelope: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(envelope["warnings"], serde_json::json!([]));
}

#[test]
fn nested_rules_and_facts_are_covered() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("packages/a")).unwrap();
    std::fs::write(dir.path().join("packages/a/VERSION"), "1.2.3\n").unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        "version: 1\n\
         facts:\n  - id: has_version\n    file_content_matches:\n      paths: packages/a/VERSION\n      \
         pattern: '^[0-9.]+$'\n\
         rules:\n  - id: per-package\n    kind: for_each_dir\n    select: 'packages/*'\n    require:\n      \
         - kind: file_content_matches\n        paths: '{path}/VERSION'\n        pattern: '^[0-9.]+$'\n    \
         level: warning\n",
    )
    .unwrap();
    let out = run(dir.path(), &["check", "--color", "never"]);
    let warnings = warnings(&out);
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(
        warnings[0].starts_with("alint: warning: rule `per-package"),
        "{warnings:?}"
    );
    assert!(warnings[0].contains("end of the file"), "{warnings:?}");
    assert!(
        warnings[1].starts_with("alint: warning: fact `has_version`"),
        "{warnings:?}"
    );
}

#[test]
fn only_limits_hints_to_the_selected_rules() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "x\n").unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        "version: 1\nrules:\n  \
         - id: first\n    kind: file_content_forbidden\n    paths: '*.txt'\n    pattern: '^a'\n    level: warning\n  \
         - id: second\n    kind: file_content_forbidden\n    paths: '*.txt'\n    pattern: '^b'\n    level: warning\n",
    )
    .unwrap();
    let out = run(
        dir.path(),
        &["check", "--only", "second", "--color", "never"],
    );
    let warnings = warnings(&out);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("rule `second`"), "{warnings:?}");
}
