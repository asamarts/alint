//! End-to-end tests for the `--only <RULE_ID>` filter (a global flag on
//! `check`/`fix`): it must restrict the run to the named rule(s), error
//! loudly on an unknown id (never silently lint nothing), and — being
//! global — work on the bare default `alint --only <id>` form too.
//!
//! Guards the filter's *behavior*; the prior coverage only asserted that
//! the `agent`-emitted `fix --only <id>` parses.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn alint() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(alint())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn alint")
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap_or(-1)
}

/// Two error rules — a final-newline check and a trailing-whitespace check —
/// on a file that violates BOTH (trailing space, no final newline).
fn fixture() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join(".alint.yml"),
        "version: 1\n\
         rules:\n\
         \x20 - id: needs-newline\n\
         \x20   kind: final_newline\n\
         \x20   paths: [\"**/*.txt\"]\n\
         \x20   level: error\n\
         \x20 - id: no-ws\n\
         \x20   kind: no_trailing_whitespace\n\
         \x20   paths: [\"**/*.txt\"]\n\
         \x20   level: error\n",
    )
    .unwrap();
    std::fs::write(d.path().join("a.txt"), "x ").unwrap();
    d
}

#[test]
fn only_restricts_to_the_named_rule() {
    let d = fixture();
    let root = d.path();

    // Both rules fire without --only.
    let all = run(root, &["check"]);
    assert_eq!(code(&all), 1);
    let all_s = String::from_utf8_lossy(&all.stdout);
    assert!(
        all_s.contains("needs-newline") && all_s.contains("no-ws"),
        "both rules fire unfiltered:\n{all_s}"
    );

    // --only no-ws: only that rule runs; needs-newline is absent.
    let only = run(root, &["check", "--only", "no-ws"]);
    assert_eq!(code(&only), 1, "the selected rule still fires");
    let only_s = String::from_utf8_lossy(&only.stdout);
    assert!(only_s.contains("no-ws"), "selected rule present:\n{only_s}");
    assert!(
        !only_s.contains("needs-newline"),
        "the other rule must be excluded:\n{only_s}"
    );
}

#[test]
fn only_is_global_on_the_bare_default_command() {
    let d = fixture();
    // No subcommand — `alint --only` must parse (the flag is global) and filter
    // the default `check`. Before #80 follow-up this errored "unexpected argument".
    let out = run(d.path(), &["--only", "no-ws"]);
    assert_eq!(code(&out), 1, "bare `alint --only` runs the default check");
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("no-ws") && !s.contains("needs-newline"),
        "bare default filtered to no-ws:\n{s}"
    );
}

#[test]
fn only_unknown_id_is_a_hard_error() {
    let d = fixture();
    let out = run(d.path(), &["check", "--only", "no-such-rule"]);
    assert_eq!(
        code(&out),
        2,
        "a typo must fail loudly (exit 2), not silently lint nothing"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("no-such-rule"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn only_empty_id_is_a_hard_error() {
    let d = fixture();
    let out = run(d.path(), &["check", "--only", ""]);
    assert_eq!(code(&out), 2, "an empty id matches no rule → hard error");
}

#[test]
fn fix_only_unknown_id_is_a_hard_error() {
    let d = fixture();
    let out = run(d.path(), &["fix", "--only", "no-such-rule"]);
    assert_eq!(
        code(&out),
        2,
        "fix --only honors the same loud-typo contract"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("no-such-rule"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
