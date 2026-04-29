//! Integration tests for the three remaining shell-out rule
//! kinds. Each one needs a real environment to exercise (a git
//! repo with tracked files, a real commit, or a binary on PATH)
//! that unit tests intentionally don't stand up.
//!
//! Mirrors the `git_blame_age.rs` integration-test pattern:
//! tempdir + minimal real git repo + one-rule engine + assert
//! on violation set.
//!
//! Tests skip silently (with a stderr note) when `git` isn't on
//! PATH; that's the same convention as `git_blame_age.rs` and
//! keeps these integration tests portable across CI lanes that
//! don't carry git binaries.

use std::path::Path;
use std::process::Command;

use alint_core::{Engine, RuleEntry, WalkOptions, walk};
use alint_rules::builtin_registry;

fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

fn run_git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git invocation");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn git_init(root: &Path) {
    run_git(root, &["init", "-q", "-b", "main"]);
    run_git(root, &["config", "user.name", "alint test"]);
    run_git(root, &["config", "user.email", "test@alint.test"]);
}

fn build_engine_from_yaml(yaml: &str) -> Engine {
    let spec: alint_core::RuleSpec = serde_yaml_ng::from_str(yaml).expect("rule spec parses");
    let registry = builtin_registry();
    let rule = registry.build(&spec).expect("rule builds");
    Engine::from_entries(vec![RuleEntry::new(rule)], registry)
}

fn run_engine(engine: &Engine, root: &Path) -> alint_core::Report {
    let index = walk(
        root,
        &WalkOptions {
            respect_gitignore: true,
            extra_ignores: Vec::new(),
        },
    )
    .unwrap();
    engine.run(root, &index).unwrap()
}

fn collect_violations(report: &alint_core::Report) -> Vec<&alint_core::Violation> {
    report
        .results
        .iter()
        .flat_map(|r| r.violations.iter())
        .collect()
}

// ─── git_no_denied_paths ────────────────────────────────────

#[test]
fn git_no_denied_paths_fires_on_tracked_secret() {
    if !git_available() {
        eprintln!("git unavailable; skipping git_no_denied_paths test");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);

    std::fs::write(root.join("README.md"), b"hi\n").unwrap();
    std::fs::write(root.join(".env"), b"SECRET=hunter2\n").unwrap();
    run_git(root, &["add", "README.md", ".env"]);
    run_git(root, &["commit", "-q", "-m", "init"]);

    let engine = build_engine_from_yaml(
        "id: no-secrets\n\
         kind: git_no_denied_paths\n\
         denied: [\"*.env\", \"id_rsa\"]\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    let v = collect_violations(&report);
    assert_eq!(v.len(), 1, "expected one violation on .env: {v:?}");
    assert_eq!(v[0].path.as_deref(), Some(Path::new(".env")));
}

#[test]
fn git_no_denied_paths_silent_when_secrets_untracked() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);

    // .env exists in working tree but isn't tracked.
    std::fs::write(root.join("README.md"), b"hi\n").unwrap();
    std::fs::write(root.join(".env"), b"SECRET=hunter2\n").unwrap();
    run_git(root, &["add", "README.md"]);
    run_git(root, &["commit", "-q", "-m", "init"]);

    let engine = build_engine_from_yaml(
        "id: no-secrets\n\
         kind: git_no_denied_paths\n\
         denied: [\"*.env\"]\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    assert!(
        collect_violations(&report).is_empty(),
        "untracked secret must not fire git_no_denied_paths"
    );
}

#[test]
fn git_no_denied_paths_silent_outside_git() {
    // No git_init: the rule must silently no-op when there's no
    // repo, like every other git-* rule.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".env"), b"x").unwrap();

    let engine = build_engine_from_yaml(
        "id: no-secrets\n\
         kind: git_no_denied_paths\n\
         denied: [\"*.env\"]\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    assert!(
        collect_violations(&report).is_empty(),
        "no-repo must not fire git_no_denied_paths"
    );
}

// ─── git_commit_message ─────────────────────────────────────

#[test]
fn git_commit_message_fires_when_head_does_not_match() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);

    std::fs::write(root.join("README.md"), b"hi\n").unwrap();
    run_git(root, &["add", "README.md"]);
    // Plain message — no conventional-commit prefix.
    run_git(root, &["commit", "-q", "-m", "wip"]);

    let engine = build_engine_from_yaml(
        "id: conventional\n\
         kind: git_commit_message\n\
         pattern: \"^(feat|fix|chore): \"\n\
         level: warning\n",
    );
    let report = run_engine(&engine, root);
    let v = collect_violations(&report);
    assert_eq!(v.len(), 1, "expected one violation: {v:?}");
    assert!(
        v[0].message.contains("commit message")
            || v[0].message.contains("pattern")
            || v[0].message.contains("wip"),
        "violation message should reference the bad commit: {}",
        v[0].message
    );
}

#[test]
fn git_commit_message_silent_when_head_matches() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);

    std::fs::write(root.join("a.txt"), b"x\n").unwrap();
    run_git(root, &["add", "a.txt"]);
    run_git(root, &["commit", "-q", "-m", "feat: add thing"]);

    let engine = build_engine_from_yaml(
        "id: conventional\n\
         kind: git_commit_message\n\
         pattern: \"^(feat|fix|chore): \"\n\
         level: warning\n",
    );
    let report = run_engine(&engine, root);
    assert!(
        collect_violations(&report).is_empty(),
        "feat: prefix must satisfy the pattern"
    );
}

#[test]
fn git_commit_message_silent_outside_git() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let engine = build_engine_from_yaml(
        "id: conventional\n\
         kind: git_commit_message\n\
         pattern: \"^.*$\"\n\
         level: warning\n",
    );
    let report = run_engine(&engine, root);
    assert!(
        collect_violations(&report).is_empty(),
        "no-repo must not fire git_commit_message"
    );
}

// ─── command ────────────────────────────────────────────────

#[cfg(unix)]
#[test]
fn command_passes_when_wrapped_cli_exits_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.rs"), b"fn main() {}\n").unwrap();

    let engine = build_engine_from_yaml(
        "id: trivial-pass\n\
         kind: command\n\
         paths: \"src/**/*.rs\"\n\
         command: [\"/bin/true\"]\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    assert!(
        collect_violations(&report).is_empty(),
        "/bin/true must produce no violations"
    );
}

#[cfg(unix)]
#[test]
fn command_fires_when_wrapped_cli_exits_nonzero() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.rs"), b"fn main() {}\n").unwrap();

    let engine = build_engine_from_yaml(
        "id: always-fails\n\
         kind: command\n\
         paths: \"src/**/*.rs\"\n\
         command: [\"/bin/false\"]\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    let v = collect_violations(&report);
    assert_eq!(v.len(), 1, "/bin/false must produce one violation: {v:?}");
    assert_eq!(v[0].path.as_deref(), Some(Path::new("src/a.rs")));
}

#[cfg(unix)]
#[test]
fn command_passes_alint_path_via_env() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/main.rs"), b"fn main() {}\n").unwrap();

    // Exits 0 only when ALINT_PATH was set to the relative file
    // path. Confirms the env-var bridge documented on the
    // `command` rule.
    let engine = build_engine_from_yaml(
        "id: env-check\n\
         kind: command\n\
         paths: \"src/**/*.rs\"\n\
         command:\n  \
           - /bin/sh\n  \
           - -c\n  \
           - '[ \"$ALINT_PATH\" = src/main.rs ] || exit 1'\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    assert!(
        collect_violations(&report).is_empty(),
        "ALINT_PATH should be set to the rel path"
    );
}

#[cfg(unix)]
#[test]
fn command_reports_spawn_failure_as_violation() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.rs"), b"x").unwrap();

    let engine = build_engine_from_yaml(
        "id: missing-bin\n\
         kind: command\n\
         paths: \"src/**/*.rs\"\n\
         command: [\"/nonexistent/bin/zzzzz\"]\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    let v = collect_violations(&report);
    assert_eq!(v.len(), 1, "missing bin must produce one violation");
    assert!(
        v[0].message.contains("spawn") || v[0].message.contains("PATH"),
        "spawn-failure message should reference the binary or PATH: {}",
        v[0].message
    );
}
