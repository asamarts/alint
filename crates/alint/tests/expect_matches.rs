//! End-to-end contract for `expect_matches`: empty scopes fail only when opted
//! in, after `when:`, against the full tree under `--changed`, and unsupported
//! empty-set semantics are rejected at config load.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn alint() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(alint())
        .args(args)
        .current_dir(root)
        .output()
        .expect("spawn alint")
}

fn code(out: &Output) -> i32 {
    out.status.code().unwrap_or(-1)
}

fn write_config(root: &Path, rule: &str) {
    std::fs::write(
        root.join(".alint.yml"),
        format!("version: 1\nrules:\n{rule}"),
    )
    .unwrap();
}

fn content_rule(extra: &str) -> String {
    format!(
        "  - id: guarded-content\n\
         \x20   kind: file_content_forbidden\n\
         \x20   paths: required.txt\n\
         \x20   pattern: forbidden\n\
         \x20   level: error\n{extra}"
    )
}

#[test]
fn empty_scope_is_backward_compatible_by_default_and_fails_when_opted_in() {
    let dir = tempfile::tempdir().unwrap();

    write_config(dir.path(), &content_rule(""));
    let default = run(dir.path(), &["check"]);
    assert_eq!(
        code(&default),
        0,
        "{}",
        String::from_utf8_lossy(&default.stderr)
    );

    write_config(
        dir.path(),
        &content_rule("    expect_matches: true\n    fix: { replace: { replacement: safe } }\n"),
    );
    let explain = run(
        dir.path(),
        &["explain", "guarded-content", "--format", "json"],
    );
    let explain_json: serde_json::Value =
        serde_json::from_slice(&explain.stdout).expect("explain output is JSON");
    assert_eq!(explain_json["expect_matches"], true);
    let list = run(dir.path(), &["list", "--format", "json"]);
    let list_json: serde_json::Value =
        serde_json::from_slice(&list.stdout).expect("list output is JSON");
    assert_eq!(list_json["rules"][0]["expect_matches"], true);

    let guarded = run(dir.path(), &["check", "--format", "json"]);
    assert_eq!(
        code(&guarded),
        1,
        "{}",
        String::from_utf8_lossy(&guarded.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&guarded.stdout).expect("check output is JSON");
    let rendered = json.to_string();
    assert!(rendered.contains("guarded-content"), "{json:#}");
    assert!(rendered.contains("matched none"), "{json:#}");
    assert!(rendered.contains("required.txt"), "{json:#}");
    assert_eq!(json["results"][0]["fixable"], false, "{json:#}");
    assert_eq!(
        json["results"][0]["violations"][0]["fixable"], false,
        "{json:#}"
    );
}

#[test]
fn false_when_gate_disables_the_scope_assertion() {
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        &content_rule("    when: \"false\"\n    expect_matches: true\n"),
    );

    let out = run(dir.path(), &["check"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("guarded-content"),
        "when-false rule should be absent: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn scope_filter_is_part_of_the_effective_scope() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("required.txt"), "safe\n").unwrap();
    write_config(
        dir.path(),
        &content_rule(
            "    expect_matches: true\n    scope_filter:\n      has_ancestor: package.json\n",
        ),
    );

    let empty_after_filter = run(dir.path(), &["check"]);
    assert_eq!(code(&empty_after_filter), 1);
    assert!(
        String::from_utf8_lossy(&empty_after_filter.stdout).contains("scope_filter"),
        "{}",
        String::from_utf8_lossy(&empty_after_filter.stdout)
    );

    std::fs::write(dir.path().join("package.json"), "{}\n").unwrap();
    let matched = run(dir.path(), &["check"]);
    assert_eq!(
        code(&matched),
        0,
        "{}",
        String::from_utf8_lossy(&matched.stdout)
    );
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .expect("spawn git");
    assert!(status.success(), "git {args:?} failed");
}

#[test]
fn changed_mode_checks_scope_against_full_tree_without_running_unchanged_file() {
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        &content_rule("    expect_matches: true\n    fix: { replace: { replacement: safe } }\n"),
    );
    std::fs::write(dir.path().join("required.txt"), "forbidden\n").unwrap();
    std::fs::write(dir.path().join("other.txt"), "before\n").unwrap();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(
        dir.path(),
        &[
            "-c",
            "user.name=alint test",
            "-c",
            "user.email=alint@example.invalid",
            "commit",
            "-qm",
            "fixture",
        ],
    );

    // The required file exists but is outside the diff. The assertion checks
    // the full index and passes; normal evaluation remains changed-only, so its
    // forbidden content is not reported.
    std::fs::write(dir.path().join("other.txt"), "after\n").unwrap();
    let valid = run(dir.path(), &["check", "--changed"]);
    assert_eq!(
        code(&valid),
        0,
        "{}",
        String::from_utf8_lossy(&valid.stdout)
    );

    // Even an empty diff cannot hide a globally empty required scope.
    std::fs::remove_file(dir.path().join("required.txt")).unwrap();
    git(dir.path(), &["add", "required.txt", "other.txt"]);
    git(
        dir.path(),
        &[
            "-c",
            "user.name=alint test",
            "-c",
            "user.email=alint@example.invalid",
            "commit",
            "-qm",
            "remove target",
        ],
    );
    let missing = run(dir.path(), &["check", "--changed"]);
    assert_eq!(
        code(&missing),
        1,
        "{}",
        String::from_utf8_lossy(&missing.stdout)
    );
    assert!(
        String::from_utf8_lossy(&missing.stdout).contains("matched none"),
        "{}",
        String::from_utf8_lossy(&missing.stdout)
    );
}

#[test]
fn existence_and_selector_rules_reject_expect_matches() {
    for (kind, fields) in [
        ("file_exists", "    paths: README.md\n"),
        (
            "for_each_file",
            "    select: \"**/*.rs\"\n    require:\n      - kind: file_exists\n        paths: \"{path}.meta\"\n",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            &format!(
                "  - id: unsupported\n    kind: {kind}\n{fields}    expect_matches: true\n    level: error\n"
            ),
        );
        let out = run(dir.path(), &["validate-config"]);
        assert_eq!(
            code(&out),
            1,
            "{kind}: {}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("expect_matches"),
            "{kind}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        "  - id: unsupported-nested\n\
         \x20   kind: for_each_dir\n\
         \x20   select: packages/*\n\
         \x20   require:\n\
         \x20     - kind: file_exists\n\
         \x20       paths: \"{path}/README.md\"\n\
         \x20       expect_matches: true\n\
         \x20   level: error\n",
    );
    let nested = run(dir.path(), &["validate-config"]);
    assert_eq!(
        code(&nested),
        1,
        "{}",
        String::from_utf8_lossy(&nested.stdout)
    );
    assert!(
        String::from_utf8_lossy(&nested.stderr).contains("expect_matches"),
        "{}",
        String::from_utf8_lossy(&nested.stderr)
    );
}

#[test]
fn nested_scope_assertion_is_checked_per_parent_iteration() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("packages/a")).unwrap();
    std::fs::write(dir.path().join("packages/a/package.json"), "{}\n").unwrap();
    write_config(
        dir.path(),
        "  - id: package-readmes\n\
         \x20   kind: for_each_dir\n\
         \x20   select: packages/*\n\
         \x20   require:\n\
         \x20     - kind: file_content_forbidden\n\
         \x20       paths: \"{path}/README.md\"\n\
         \x20       expect_matches: true\n\
         \x20       pattern: forbidden\n\
         \x20   level: error\n",
    );

    let missing = run(dir.path(), &["check"]);
    assert_eq!(
        code(&missing),
        1,
        "{}",
        String::from_utf8_lossy(&missing.stdout)
    );
    let stdout = String::from_utf8_lossy(&missing.stdout);
    assert!(stdout.contains("package-readmes"), "{stdout}");
    assert!(stdout.contains("packages/a/README.md"), "{stdout}");

    std::fs::write(dir.path().join("packages/a/README.md"), "safe\n").unwrap();
    let matched = run(dir.path(), &["check"]);
    assert_eq!(
        code(&matched),
        0,
        "{}",
        String::from_utf8_lossy(&matched.stdout)
    );
}

#[test]
fn fix_reports_empty_scope_as_unfixable_without_mutation() {
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path(), &content_rule("    expect_matches: true\n"));
    let before = std::fs::read(dir.path().join(".alint.yml")).unwrap();

    let out = run(dir.path(), &["fix", "--dry-run", "--format", "json"]);
    assert_eq!(code(&out), 1, "{}", String::from_utf8_lossy(&out.stdout));
    let rendered = String::from_utf8_lossy(&out.stdout);
    assert!(rendered.contains("unfixable"), "{rendered}");
    assert!(rendered.contains("matched none"), "{rendered}");
    assert_eq!(
        std::fs::read(dir.path().join(".alint.yml")).unwrap(),
        before
    );

    let baseline = run(
        dir.path(),
        &["baseline", "--output", "scope-baseline.json", "."],
    );
    assert_eq!(
        code(&baseline),
        0,
        "{}",
        String::from_utf8_lossy(&baseline.stderr)
    );
    let baselined_fix = run(
        dir.path(),
        &["fix", "--dry-run", "--baseline", "scope-baseline.json", "."],
    );
    assert_eq!(
        code(&baselined_fix),
        0,
        "{}",
        String::from_utf8_lossy(&baselined_fix.stdout)
    );
    assert!(
        String::from_utf8_lossy(&baselined_fix.stdout).contains("baselined"),
        "{}",
        String::from_utf8_lossy(&baselined_fix.stdout)
    );
}
