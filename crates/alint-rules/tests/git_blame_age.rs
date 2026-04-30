//! Integration tests for the `git_blame_age` rule kind.
//!
//! Unit tests in `git_blame_age.rs` exercise the build /
//! options / no-cache paths without shelling out to git. These
//! tests stand up a real git repo with two commits at
//! controlled `GIT_AUTHOR_DATE` / `GIT_COMMITTER_DATE`
//! timestamps to verify the rule fires only on the older of
//! two matching lines.

use std::path::Path;
use std::process::Command;

use alint_core::{Engine, Level, RuleEntry, WalkOptions, walk};
use alint_rules::builtin_registry;

/// Initialise a git repo at `root` with a stable identity that
/// `git commit` can use without consulting the user's global
/// config.
fn git_init(root: &Path) {
    run_git(root, &["init", "-q", "-b", "main"], &[]);
    run_git(root, &["config", "user.name", "alint test"], &[]);
    run_git(root, &["config", "user.email", "test@alint.test"], &[]);
}

/// `git add <path>` then `git commit -m <msg>` with author and
/// committer dates pinned via env vars. `date` is anything git
/// accepts, e.g. `"2020-01-01T00:00:00Z"` or `"@1577836800 +0000"`.
fn commit_with_date(root: &Path, path: &str, msg: &str, date: &str) {
    run_git(root, &["add", path], &[]);
    run_git(
        root,
        &["commit", "-q", "-m", msg],
        &[("GIT_AUTHOR_DATE", date), ("GIT_COMMITTER_DATE", date)],
    );
}

fn run_git(root: &Path, args: &[&str], envs: &[(&str, &str)]) {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(root).args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("git invocation");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Build a one-rule engine running `git_blame_age` against the
/// given pattern + threshold.
fn build_engine(pattern: &str, max_age_days: u64) -> Engine {
    let yaml = format!(
        "id: stale\n\
         kind: git_blame_age\n\
         paths: \"**/*.rs\"\n\
         pattern: '{pattern}'\n\
         max_age_days: {max_age_days}\n\
         level: warning\n",
    );
    let spec: alint_core::RuleSpec = serde_yaml_ng::from_str(&yaml).unwrap();
    let registry = builtin_registry();
    let rule = registry.build(&spec).unwrap();
    Engine::from_entries(vec![RuleEntry::new(rule)], registry)
}

#[test]
fn fires_on_old_line_silent_on_recent_line() {
    // Skip silently if `git` isn't on PATH — the rest of the
    // test suite handles non-git environments uniformly.
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("git unavailable; skipping git_blame_age integration test");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);

    // First commit: ten years ago. Includes a TODO marker
    // that the rule should fire on.
    std::fs::write(root.join("a.rs"), "// TODO old marker\nfn main() {}\n").unwrap();
    commit_with_date(
        root,
        "a.rs",
        "ancient",
        "@1500000000 +0000", // 2017-07-14
    );

    // Second commit: yesterday. New TODO that's well within
    // the threshold and must NOT fire.
    let mut bytes = std::fs::read(root.join("a.rs")).unwrap();
    bytes.extend_from_slice(b"// TODO recent marker\n");
    std::fs::write(root.join("a.rs"), bytes).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let yesterday = now - 86_400;
    let yesterday_arg = format!("@{yesterday} +0000");
    commit_with_date(root, "a.rs", "fresh", &yesterday_arg);

    let engine = build_engine(r"\bTODO\b", 180);
    let walk_opts = WalkOptions {
        respect_gitignore: true,
        extra_ignores: Vec::new(),
    };
    let index = walk(root, &walk_opts).unwrap();
    let report = engine.run(root, &index).unwrap();

    let violations: Vec<_> = report
        .results
        .iter()
        .flat_map(|r| r.violations.iter())
        .collect();
    assert_eq!(
        violations.len(),
        1,
        "expected exactly one violation (the old TODO); got: {violations:?}"
    );
    let v = violations[0];
    assert_eq!(v.line, Some(1), "old TODO is on line 1");
    assert!(
        v.message.contains("TODO"),
        "message should reference the matched marker: {}",
        v.message
    );
}

#[test]
fn ctx_match_placeholder_substitutes_capture_group() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);

    std::fs::write(
        root.join("a.rs"),
        "// FIXME this is broken\n// TODO untouched\n",
    )
    .unwrap();
    commit_with_date(root, "a.rs", "ancient", "@1500000000 +0000");

    // Custom message uses `{{ctx.match}}` to surface the
    // captured marker text. Rule pattern has a capture group
    // around the marker word.
    let yaml = "\
id: stale-markers
kind: git_blame_age
paths: \"**/*.rs\"
pattern: '\\b(FIXME|TODO)\\b'
max_age_days: 180
level: warning
message: \"`{{ctx.match}}` marker is ancient — resolve or remove\"
";
    let spec: alint_core::RuleSpec = serde_yaml_ng::from_str(yaml).unwrap();
    let registry = builtin_registry();
    let rule = registry.build(&spec).unwrap();
    let engine = Engine::from_entries(vec![RuleEntry::new(rule)], registry);

    let walk_opts = WalkOptions {
        respect_gitignore: true,
        extra_ignores: Vec::new(),
    };
    let index = walk(root, &walk_opts).unwrap();
    let report = engine.run(root, &index).unwrap();

    let messages: Vec<&str> = report
        .results
        .iter()
        .flat_map(|r| r.violations.iter())
        .map(|v| v.message.as_ref())
        .collect();
    assert_eq!(messages.len(), 2, "two markers should fire: {messages:?}");
    let combined = messages.join("\n");
    assert!(
        combined.contains("`FIXME`"),
        "FIXME capture should render: {combined}"
    );
    assert!(
        combined.contains("`TODO`"),
        "TODO capture should render: {combined}"
    );
}

#[test]
fn silent_outside_git() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // No git_init — bare tempdir.
    std::fs::write(root.join("a.rs"), "// TODO old marker\n").unwrap();

    let engine = build_engine(r"\bTODO\b", 180);
    let walk_opts = WalkOptions {
        respect_gitignore: true,
        extra_ignores: Vec::new(),
    };
    let index = walk(root, &walk_opts).unwrap();
    let report = engine.run(root, &index).unwrap();

    assert!(
        report.results.iter().all(|r| r.violations.is_empty()),
        "rule should silently no-op outside a git repo; got: {:?}",
        report.results
    );
}

#[test]
fn level_promotion_to_error_works() {
    // `level: error` is allowed even though the rule is
    // heuristic — users escalating per-rule once their
    // workflow proves the FP rate is acceptable should be
    // free to do so.
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);
    std::fs::write(root.join("a.rs"), "// TODO old\n").unwrap();
    commit_with_date(root, "a.rs", "ancient", "@1500000000 +0000");

    let yaml = "\
id: stale-todos
kind: git_blame_age
paths: \"**/*.rs\"
pattern: 'TODO'
max_age_days: 180
level: error
";
    let spec: alint_core::RuleSpec = serde_yaml_ng::from_str(yaml).unwrap();
    let registry = builtin_registry();
    let rule = registry.build(&spec).unwrap();
    let engine = Engine::from_entries(vec![RuleEntry::new(rule)], registry);

    let walk_opts = WalkOptions {
        respect_gitignore: true,
        extra_ignores: Vec::new(),
    };
    let index = walk(root, &walk_opts).unwrap();
    let report = engine.run(root, &index).unwrap();
    let levels: Vec<Level> = report
        .results
        .iter()
        .filter(|r| !r.violations.is_empty())
        .map(|r| r.level)
        .collect();
    assert_eq!(levels, vec![Level::Error]);
}
