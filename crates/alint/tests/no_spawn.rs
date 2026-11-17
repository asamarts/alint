//! `--no-spawn` (and `--untrusted`, which refuses all it does): a config with
//! a rule, fix or fact that starts a process fails to load, naming it and the
//! flag, and nothing runs. Each case arms a canary (the command
//! is `touch <sentinel>`) for every metadata-declared spawning kind, a
//! spawning fix, a spawning kind nested in `require:`, and a `custom:` fact.
//! The same configs without the flag run their canary (the positive control).
//! A rule that runs Git is refused too, and so are `--changed` and `--base`:
//! Git runs the commands a repository's `.git/config` names, and a canary
//! there (`core.fsmonitor`) shows none ran.
//!
//! `#![cfg(unix)]`: the canary uses `/bin/sh` and `touch`.

#![cfg(unix)]

use std::path::Path;
use std::process::Output;

fn run(dir: &Path, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_alint"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn alint")
}

/// The flags that refuse every process a config would start.
const REFUSING_FLAGS: [&str; 2] = ["--no-spawn", "--untrusted"];

/// Each subcommand that loads a config, with `flag`.
fn config_commands(flag: &'static str) -> [Vec<&'static str>; 7] {
    [
        vec!["check", flag],
        vec![flag],
        vec!["fix", flag],
        vec!["baseline", flag, "--output", "base.json"],
        vec!["facts", flag],
        vec!["list", flag],
        vec!["validate-config", flag],
    ]
}

fn touch(sentinel: &Path) -> String {
    format!("[\"sh\", \"-c\", \"touch {}\"]", sentinel.display())
}

/// The minimal fields of each spawning kind (as in `spawn_gate.rs`).
fn spawning_rule(kind: &str, sentinel: &Path) -> String {
    let extra = match kind {
        "command" => "\n    paths: \"*.txt\"",
        "generated_file_fresh" => "\n    file: out.txt",
        "command_idempotent" => "",
        other => panic!("unhandled spawning kind {other:?}; add its minimal fields here"),
    };
    format!(
        "  - id: spawner\n    kind: {kind}{extra}\n    command: {}\n    level: error\n",
        touch(sentinel)
    )
}

fn spawning_kinds() -> Vec<String> {
    alint_rules::builtin_registry()
        .kind_metadata()
        .filter(|(_kind, meta)| {
            meta.capabilities
                .iter()
                .any(|entry| matches!(entry.capability, alint_core::Capability::SpawnUserCommand))
        })
        .map(|(kind, _meta)| kind.to_string())
        .collect()
}

/// `(what the error names, the config body)` for every way a config spawns.
fn cases(sentinel: &Path) -> Vec<(String, String)> {
    let mut cases: Vec<(String, String)> = spawning_kinds()
        .into_iter()
        .map(|kind| {
            (
                format!("rule \"spawner\" (kind: {kind})"),
                format!("version: 1\nrules:\n{}", spawning_rule(&kind, sentinel)),
            )
        })
        .collect();
    cases.push((
        "rule \"per-dir\" (require[0] kind: command)".to_string(),
        format!(
            "version: 1\nrules:\n  - id: per-dir\n    kind: for_each_dir\n    select: \"*\"\n    \
             require:\n      - kind: command\n        paths: \"*.txt\"\n        command: {}\n    \
             level: error\n",
            touch(sentinel)
        ),
    ));
    cases.push((
        "rule \"untrack\" (fix: git_untrack)".to_string(),
        "version: 1\nrules:\n  - id: untrack\n    kind: file_absent\n    paths: \"*.log\"\n    \
         git_tracked_only: true\n    fix:\n      git_untrack: {}\n    level: error\n"
            .to_string(),
    ));
    cases.push((
        "fact \"probe\" (custom:)".to_string(),
        format!(
            "version: 1\nfacts:\n  - id: probe\n    custom:\n      argv: {}\nrules:\n  \
             - id: readme\n    kind: file_exists\n    paths: data.txt\n    level: error\n",
            touch(sentinel)
        ),
    ));
    cases
}

#[test]
fn no_spawn_refuses_every_spawning_rule_fix_and_fact() {
    for flag in REFUSING_FLAGS {
        for args in config_commands(flag) {
            let dir = tempfile::tempdir().unwrap();
            let sentinel = dir.path().join("SENTINEL");
            std::fs::write(dir.path().join("data.txt"), "hi\n").unwrap();
            for (named, body) in cases(&sentinel) {
                std::fs::write(dir.path().join(".alint.yml"), &body).unwrap();
                let out = run(dir.path(), &args);
                let stderr = String::from_utf8_lossy(&out.stderr);
                assert!(!out.status.success(), "{args:?} {named}: {stderr}");
                assert!(
                    stderr.contains(&format!("{named} spawns a process; {flag} refuses it")),
                    "{args:?} {named}: {stderr}"
                );
                assert!(!sentinel.exists(), "{args:?} {named}: the process ran");
            }
        }
    }
}

#[test]
fn without_no_spawn_the_same_configs_run_their_canary() {
    for index in 0..cases(Path::new("unused")).len() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel = dir.path().join("SENTINEL");
        let (named, body) = cases(&sentinel).swap_remove(index);
        if named.contains("git_untrack") {
            continue; // a fix runs only under `alint fix`, on a tracked file
        }
        std::fs::write(dir.path().join("data.txt"), "hi\n").unwrap();
        // A directory for `for_each_dir` to visit.
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/x.txt"), "hi\n").unwrap();
        std::fs::write(dir.path().join(".alint.yml"), body).unwrap();
        let out = run(dir.path(), &["check"]);
        assert!(
            sentinel.exists(),
            "{named}: positive control did not run: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// Run `git` with the host's configuration left out, in `dir`.
fn git(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(output.status.success(), "{output:?}");
}

/// A repository with `a.txt` and `sub/b.txt` committed whose `.git/config`
/// names a command (`core.fsmonitor`, which Git runs when it reads the
/// index) that writes the returned canary.
fn git_canary_repository(dir: &Path) -> std::path::PathBuf {
    let canary = dir.join("CANARY");
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    std::fs::create_dir(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub/b.txt"), "b\n").unwrap();
    git(dir, &["init", "-q"]);
    git(dir, &["add", "a.txt", "sub/b.txt"]);
    git(
        dir,
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
    git(
        dir,
        &[
            "config",
            "core.fsmonitor",
            &format!("printf ran > '{}'; false", canary.display()),
        ],
    );
    canary
}

/// `(what the error names, the config body)` for every way a rule runs Git:
/// `git_tracked_only`, a `git_*` kind, `scope_filter.changed_since`, and one
/// nested in `require:`.
fn git_cases() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "rule \"tracked\" (git_tracked_only)",
            "version: 1\nrules:\n  - id: tracked\n    kind: file_exists\n    paths: a.txt\n    \
             git_tracked_only: true\n    level: error\n",
        ),
        (
            "rule \"denied\" (kind: git_no_denied_paths)",
            "version: 1\nrules:\n  - id: denied\n    kind: git_no_denied_paths\n    \
             denied: [\"*.secret\"]\n    level: error\n",
        ),
        (
            "rule \"changed\" (scope_filter.changed_since)",
            "version: 1\nrules:\n  - id: changed\n    kind: file_max_size\n    paths: \"*.txt\"\n    \
             max_bytes: 100\n    scope_filter:\n      changed_since: HEAD\n    level: error\n",
        ),
        (
            "rule \"per-dir\" (require[0] scope_filter.changed_since)",
            "version: 1\nrules:\n  - id: per-dir\n    kind: for_each_dir\n    select: sub\n    \
             require:\n      - kind: file_max_size\n        paths: \"{path}/*.txt\"\n        \
             max_bytes: 100\n        scope_filter:\n          changed_since: HEAD\n    \
             level: error\n",
        ),
    ]
}

/// Git runs the commands a repository's `.git/config` names, so `--no-spawn`
/// and `--untrusted` refuse a rule that runs Git as they refuse a spawning
/// one, and alint runs no Git.
#[test]
fn no_spawn_refuses_every_git_rule_and_runs_no_git() {
    for flag in REFUSING_FLAGS {
        for args in config_commands(flag) {
            for (named, body) in git_cases() {
                let dir = tempfile::tempdir().unwrap();
                let canary = git_canary_repository(dir.path());
                std::fs::write(dir.path().join(".alint.yml"), body).unwrap();
                let out = run(dir.path(), &args);
                let stderr = String::from_utf8_lossy(&out.stderr);
                assert!(!out.status.success(), "{args:?} {named}: {stderr}");
                assert!(
                    stderr.contains(&format!("{named} runs Git")),
                    "{args:?} {named}: {stderr}"
                );
                assert!(
                    stderr.contains(&format!("{flag} refuses it")),
                    "{args:?} {named}: {stderr}"
                );
                assert!(!canary.exists(), "{args:?} {named}: Git ran");
            }
        }
    }
}

#[test]
fn no_spawn_refuses_changed_and_base() {
    for flag in REFUSING_FLAGS {
        no_spawn_refuses_changed_and_base_with(flag);
    }
}

fn no_spawn_refuses_changed_and_base_with(flag: &str) {
    for args in [
        &["check", flag, "--changed"][..],
        &["check", flag, "--base", "HEAD"],
        &["fix", flag, "--changed"],
        &["explain", "readme", "--trace", flag, "--changed"],
    ] {
        let dir = tempfile::tempdir().unwrap();
        let canary = git_canary_repository(dir.path());
        std::fs::write(
            dir.path().join(".alint.yml"),
            "version: 1\nrules:\n  - id: readme\n    kind: file_exists\n    paths: a.txt\n    \
             level: error\n",
        )
        .unwrap();
        let out = run(dir.path(), args);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{args:?}: {stderr}");
        assert!(
            stderr.contains(&format!("`{flag}` refuses them")),
            "{args:?}: {stderr}"
        );
        assert!(!canary.exists(), "{args:?}: Git ran");
        // Without the flag, `--changed` runs Git (the positive control):
        // listing the working tree's changes reads the index. (`--base`
        // diffs commits, which reads no index.)
        if !args.contains(&"--base") {
            let without: Vec<&str> = args.iter().copied().filter(|a| *a != flag).collect();
            run(dir.path(), &without);
            assert!(
                canary.exists(),
                "{without:?}: positive control did not run Git"
            );
        }
    }
}

#[test]
fn without_no_spawn_the_git_rules_run_git() {
    for (named, body) in git_cases() {
        let dir = tempfile::tempdir().unwrap();
        let canary = git_canary_repository(dir.path());
        std::fs::write(dir.path().join(".alint.yml"), body).unwrap();
        let out = run(dir.path(), &["check"]);
        assert!(
            canary.exists(),
            "{named}: positive control did not run Git: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn explain_trace_refuses_the_traced_spawning_rule_even_when_off() {
    let dir = tempfile::tempdir().unwrap();
    let sentinel = dir.path().join("SENTINEL");
    std::fs::write(dir.path().join("data.txt"), "hi\n").unwrap();
    let body = format!(
        "version: 1\nrules:\n{}",
        spawning_rule("command", &sentinel)
    )
    .replace("level: error", "level: off");
    std::fs::write(dir.path().join(".alint.yml"), body).unwrap();
    for flag in REFUSING_FLAGS {
        // An off rule runs nowhere, so plain `check` accepts the config.
        assert!(run(dir.path(), &["check", flag]).status.success());
        let out = run(dir.path(), &["explain", "spawner", "--trace", flag]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{stderr}");
        assert!(stderr.contains(&format!("{flag} refuses it")), "{stderr}");
        assert!(!sentinel.exists());
    }
}

#[test]
fn no_spawn_is_rejected_where_it_means_nothing() {
    let dir = tempfile::tempdir().unwrap();
    for flag in REFUSING_FLAGS {
        for args in [
            &["init", flag][..],
            &["suggest", flag],
            &["export-agents-md", flag],
            &["rules", "list", flag],
            &["test", flag, "--allow-spawn"],
        ] {
            let out = run(dir.path(), args);
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert_eq!(out.status.code(), Some(2), "{args:?}: {stderr}");
            assert!(stderr.contains(flag), "{args:?}: {stderr}");
        }
    }
    assert!(
        !dir.path().join(".alint.yml").exists(),
        "init wrote a config"
    );
}
