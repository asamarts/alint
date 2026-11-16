//! `--no-spawn`: a config with a rule, fix or fact that starts a process fails
//! to load, naming it, and nothing runs. Each case arms a canary (the command
//! is `touch <sentinel>`) for every metadata-declared spawning kind, a
//! spawning fix, a spawning kind nested in `require:`, and a `custom:` fact.
//! The same configs without the flag run their canary (the positive control).
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
    let commands: [&[&str]; 7] = [
        &["check", "--no-spawn"],
        &["--no-spawn"],
        &["fix", "--no-spawn"],
        &["baseline", "--no-spawn", "--output", "base.json"],
        &["facts", "--no-spawn"],
        &["list", "--no-spawn"],
        &["validate-config", "--no-spawn"],
    ];
    for args in commands {
        let dir = tempfile::tempdir().unwrap();
        let sentinel = dir.path().join("SENTINEL");
        std::fs::write(dir.path().join("data.txt"), "hi\n").unwrap();
        for (named, body) in cases(&sentinel) {
            std::fs::write(dir.path().join(".alint.yml"), &body).unwrap();
            let out = run(dir.path(), args);
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(!out.status.success(), "{args:?} {named}: {stderr}");
            assert!(
                stderr.contains(&format!("{named} spawns a process; --no-spawn refuses it")),
                "{args:?} {named}: {stderr}"
            );
            assert!(!sentinel.exists(), "{args:?} {named}: the process ran");
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
    // An off rule runs nowhere, so plain `check` accepts the config.
    assert!(run(dir.path(), &["check", "--no-spawn"]).status.success());
    let out = run(dir.path(), &["explain", "spawner", "--trace", "--no-spawn"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("--no-spawn refuses it"), "{stderr}");
    assert!(!sentinel.exists());
}

#[test]
fn no_spawn_is_rejected_where_it_means_nothing() {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        &["init", "--no-spawn"][..],
        &["suggest", "--no-spawn"],
        &["export-agents-md", "--no-spawn"],
        &["rules", "list", "--no-spawn"],
        &["test", "--no-spawn", "--allow-spawn"],
    ] {
        let out = run(dir.path(), args);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {stderr}");
        assert!(stderr.contains("--no-spawn"), "{args:?}: {stderr}");
    }
    assert!(
        !dir.path().join(".alint.yml").exists(),
        "init wrote a config"
    );
}
