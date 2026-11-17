//! Cross-command color contract.
//!
//! Every alint subcommand that produces human output must:
//!   1. Emit ANSI SGR escape sequences (`\x1b[…m`) when invoked
//!      with `--color=always`.
//!   2. Emit zero ANSI SGR escape sequences when invoked with
//!      `--color=never`.
//!
//! These two assertions are the single best-bang-for-buck guard
//! against the most common regression: a new (or refactored)
//! subcommand reverts to bare `println!` and silently drops
//! styling. v0.9.21 introduced this test after `alint list`
//! (and three other commands) shipped without honoring the
//! `--color` flag for two minor releases.
//!
//! Width handling per-command varies by output shape (the
//! grouped check renderer wraps; the one-line `list` doesn't),
//! so this test deliberately doesn't enforce a width contract —
//! see the per-command tests in `cli.rs` and the trycmd
//! snapshots for that.

use std::path::Path;
use std::process::Stdio;

use tempfile::TempDir;

const ALINT: &str = env!("CARGO_BIN_EXE_alint");

/// Materialise a tempdir containing a minimal repo whose config:
///   - declares one fact (so `facts` has output to render)
///   - declares one rule with a `policy_url` (so `explain` can
///     emit a URL line and the `--color=always` path through
///     `style::DOCS` is exercised)
///   - declares one rule that fires (so `check` and `fix` have
///     a violation to colour)
///
/// `suggest` proposes a bundled ruleset because the .alint.yml
/// only `extends:` from one ruleset, so the `agent-hygiene`
/// suggester has room to fire — that gives `suggest` something
/// to render under `--color=always`.
fn fixture_repo() -> TempDir {
    let tmp = tempfile::Builder::new()
        .prefix("alint-styling-test-")
        .tempdir()
        .expect("tempdir create");

    // .alint.yml — the smallest config that exercises every
    // command's render path.
    std::fs::write(
        tmp.path().join(".alint.yml"),
        b"version: 1\n\
extends:\n  \
  - alint://bundled/oss-baseline@v1\n\
facts:\n  \
  - id: has_node\n    \
    any_file_exists: package.json\n\
rules:\n  \
  - id: must-have-license\n    \
    kind: file_exists\n    \
    paths: LICENSE\n    \
    level: warning\n    \
    policy_url: https://opensource.guide/legal/\n",
    )
    .unwrap();

    // Touch a package.json so `has_node` resolves to `true` and
    // exercises the `style::SUCCESS` branch in render_facts_human.
    std::fs::write(
        tmp.path().join("package.json"),
        br#"{"name":"alint-styling-fixture","version":"0.0.0"}"#,
    )
    .unwrap();

    tmp
}

fn run(args: &[&str], cwd: &Path) -> (Vec<u8>, Vec<u8>) {
    let out = crate::hermetic::command(ALINT)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("alint exec");
    (out.stdout, out.stderr)
}

fn assert_has_ansi(label: &str, args: &[&str], stdout: &[u8]) {
    let s = std::str::from_utf8(stdout).expect("utf8 stdout");
    assert!(
        s.contains("\x1b["),
        "[{label}] (alint {}) expected ANSI SGR escapes in stdout, got:\n{s}",
        args.join(" "),
    );
}

fn assert_no_ansi(label: &str, args: &[&str], stdout: &[u8]) {
    let s = std::str::from_utf8(stdout).expect("utf8 stdout");
    assert!(
        !s.contains("\x1b["),
        "[{label}] (alint {}) expected NO ANSI SGR escapes in stdout, got:\n{s:?}",
        args.join(" "),
    );
}

/// Subcommands that produce human stdout output and therefore
/// owe color emission. Adding a new human-output subcommand?
/// Add it here too — the test will catch any regression.
fn human_output_subcommands() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("check", vec!["check"]),
        ("list", vec!["list"]),
        ("explain", vec!["explain", "must-have-license"]),
        ("fix", vec!["fix", "--dry-run"]),
        ("facts", vec!["facts"]),
        // `suggest --quiet` suppresses the stderr summary line so
        // any failure speaks for itself; --include-bundled lets
        // the bundled suggester emit even if the fixture already
        // extends one ruleset.
        ("suggest", vec!["suggest", "--quiet", "--include-bundled"]),
    ]
}

#[test]
fn every_human_command_emits_ansi_when_color_is_always() {
    let tmp = fixture_repo();
    for (name, base_args) in human_output_subcommands() {
        let mut args = base_args.clone();
        args.extend(["--color", "always"]);
        let (stdout, _stderr) = run(&args, tmp.path());
        assert_has_ansi(name, &args, &stdout);
    }
}

#[test]
fn every_human_command_strips_ansi_when_color_is_never() {
    let tmp = fixture_repo();
    for (name, base_args) in human_output_subcommands() {
        let mut args = base_args.clone();
        args.extend(["--color", "never"]);
        let (stdout, _stderr) = run(&args, tmp.path());
        assert_no_ansi(name, &args, &stdout);
    }
}

/// Pipes (i.e. our test harness) inherit `--color=auto`'s "no
/// TTY → no color" branch. Belt-and-suspenders check that the
/// default behaviour matches `--color=never` over a pipe.
#[test]
fn every_human_command_strips_ansi_under_default_color_over_pipe() {
    let tmp = fixture_repo();
    for (name, base_args) in human_output_subcommands() {
        let (stdout, _stderr) = run(&base_args, tmp.path());
        assert_no_ansi(name, &base_args, &stdout);
    }
}
