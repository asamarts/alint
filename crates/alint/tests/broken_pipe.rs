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
