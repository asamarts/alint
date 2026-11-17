//! `alint_core::git::deny_for_process`, which `alint --no-spawn` and
//! `--untrusted` call before a config loads: from then on no helper in
//! `alint_core::git` runs Git, which would run the commands a repository's
//! `.git/config` names. The load refuses every rule that needs Git first; this
//! is the backstop for a path that reaches Git without one. The denial lasts
//! for the process, so it is tested here, in a test binary whose other tests
//! run no Git, and not in the library's unit tests, which do.
#![cfg(unix)]

use std::path::Path;

fn git(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("XDG_CONFIG_HOME", "/dev/null")
        .output()
        .expect("run git");
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn a_denied_process_runs_no_git() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let canary = root.join("CANARY");
    std::fs::write(root.join("a.txt"), "a\n").unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "a.txt"]);
    git(
        root,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-qm",
            "init",
        ],
    );
    // Git runs `core.fsmonitor` when it reads the index.
    git(
        root,
        &[
            "config",
            "core.fsmonitor",
            &format!("printf ran > '{}'; false", canary.display()),
        ],
    );

    // Before the denial, reading the tracked files runs Git, and Git the
    // repository's command.
    assert!(alint_core::git::collect_tracked_paths(root).is_some());
    assert!(canary.exists(), "the control ran no Git");
    std::fs::remove_file(&canary).unwrap();

    alint_core::git::deny_for_process();
    assert!(alint_core::git::collect_tracked_paths(root).is_none());
    assert!(alint_core::git::collect_changed_paths(root, None).is_none());
    assert!(alint_core::git::collect_changed_paths(root, Some("HEAD")).is_none());
    assert!(alint_core::git::head_commit_time(root).is_err());
    assert!(!canary.exists(), "a denied process ran Git");
}
