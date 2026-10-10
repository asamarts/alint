//! Audit 2026-10 finding 10: `alint check -f json | head -1` printed
//! "writing output: Broken pipe" and exited 2. A reader closing stdout early
//! is not an alint error: the run exits quietly with the exit code it would
//! have had (here 1, the report has errors).
#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

#[test]
fn closed_stdout_is_a_quiet_exit_with_the_would_be_code() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        "version: 1\nrules:\n  - id: no-todo\n    kind: file_content_forbidden\n    \
         paths: \"**/*.txt\"\n    pattern: TODO\n    level: error\n",
    )
    .unwrap();
    // Enough findings that the JSON report far exceeds a pipe buffer, so the
    // writer is still writing when the reader goes away.
    for i in 0..3000 {
        std::fs::write(tmp.path().join(format!("f{i:04}.txt")), "TODO\n").unwrap();
    }

    for (args, expected) in [
        (&["check", "--format", "json"][..], 1),
        (&["check", "--format", "sarif"][..], 1),
        (&["list"][..], 0),
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_alint"))
            .args(args)
            .current_dir(tmp.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        {
            let mut first = String::new();
            let mut reader = BufReader::new(child.stdout.take().unwrap());
            reader.read_line(&mut first).unwrap();
        } // stdout closed here, like `head -1`
        let out = child.wait_with_output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.to_lowercase().contains("broken pipe"),
            "{args:?}: {stderr}"
        );
        assert!(!stderr.contains("panic"), "{args:?}: {stderr}");
        assert_eq!(out.status.code(), Some(expected), "{args:?}: {stderr}");
    }
}

/// Run `alint args` in `dir` with stdout connected to a pipe whose read end
/// is ALREADY closed, so the very first stdout write hits EPIPE
/// (deterministic, unlike racing a reader).
fn run_into_closed_pipe(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    Command::new(env!("CARGO_BIN_EXE_alint"))
        .args(args)
        .current_dir(dir)
        .stdout(writer)
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

/// Every subcommand that writes to stdout survives a closed stdout: no
/// panic banner, no "Broken pipe" error, and the exit code it would have
/// had with a live reader.
#[test]
fn every_stdout_subcommand_tolerates_a_closed_stdout() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nrules:\n  - id: no-todo\n    kind: file_content_forbidden\n    \
         paths: \"**/*.txt\"\n    pattern: TODO\n    level: error\n  \
         - id: trail\n    kind: no_trailing_whitespace\n    paths: \"**/*.txt\"\n    \
         level: error\n    fix:\n      file_trim_trailing_whitespace: {}\n",
    )
    .unwrap();
    std::fs::write(repo.join("a.txt"), "TODO  \n").unwrap();
    std::fs::write(repo.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
    let bad = tmp.path().join("bad");
    std::fs::create_dir(&bad).unwrap();
    std::fs::write(
        bad.join(".alint.yml"),
        "version: 1\nrules:\n  - id: x\n    kind: nope\n",
    )
    .unwrap();

    let cases: &[(&std::path::Path, &[&str], i32)] = &[
        (&repo, &["check"], 1),
        (&repo, &["check", "--format", "json"], 1),
        (&repo, &["list"], 0),
        (&repo, &["explain", "no-todo"], 0),
        (&repo, &["fix", "--dry-run"], 1),
        (&repo, &["fix", "--dry-run", "--format", "json"], 1),
        (&repo, &["facts"], 0),
        (&repo, &["validate-config"], 0),
        (&repo, &["validate-config", "--format", "json"], 0),
        (&bad, &["validate-config"], 1),
        (&bad, &["validate-config", "--format", "json"], 1),
        (&repo, &["export-agents-md"], 0),
        (&repo, &["suggest"], 0),
        (&repo, &["rules", "list"], 0),
        (&repo, &["rules", "categories"], 0),
        (&repo, &["rules", "show", "file_exists"], 0),
    ];
    for (dir, args, expected) in cases {
        let out = run_into_closed_pipe(dir, args);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!stderr.contains("panic"), "{args:?}: {stderr}");
        assert!(
            !stderr.to_lowercase().contains("broken pipe"),
            "{args:?}: {stderr}"
        );
        assert_eq!(out.status.code(), Some(*expected), "{args:?}: {stderr}");
    }

    // `init` writes the config file, then its summary to stdout.
    let fresh = tmp.path().join("fresh");
    std::fs::create_dir(&fresh).unwrap();
    let out = run_into_closed_pipe(&fresh, &["init"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("panic"), "init: {stderr}");
    assert_eq!(out.status.code(), Some(0), "init: {stderr}");
    assert!(fresh.join(".alint.yml").is_file());
}
