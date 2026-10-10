//! Terminal-escape injection (audit 2026-10, finding 6): every path that
//! writes config- or repo-derived text to a terminal must neutralize
//! control characters (ESC sequences) and the invisible bidi / zero-width
//! formatting characters, not only the plain `check` report.
//!
//! The config carries a `policy_url` and `message` with an embedded
//! `ESC[2J` (clear screen) and a U+202E RIGHT-TO-LEFT OVERRIDE. alint's own
//! `--color always` styling still emits ESC, so the assertions look for the
//! injected sequences specifically.

use std::path::Path;
use std::process::{Command, Output};

const CLEAR: &str = "\u{1b}[2J";
const RLO: char = '\u{202E}';

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_alint"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run alint")
}

fn assert_clean(label: &str, bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    assert!(
        !text.contains(CLEAR),
        "{label}: raw ESC[2J leaked: {text:?}"
    );
    assert!(!text.contains(RLO), "{label}: raw U+202E leaked: {text:?}");
}

fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        "version: 1\nrules:\n  - id: evil\n    kind: file_content_forbidden\n    \
         paths: \"*.txt\"\n    pattern: TODO\n    level: error\n    \
         message: \"bad\\e[2Jthing\\u202Eevil\"\n    \
         policy_url: \"https://example.com/\\e[2J\\u202E\"\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("a.txt"), "TODO\n").unwrap();
    tmp
}

#[test]
fn list_sanitizes_policy_url() {
    let tmp = fixture();
    let o = run(tmp.path(), &["list", "--color", "always"]);
    assert!(o.status.success(), "{o:?}");
    assert!(String::from_utf8_lossy(&o.stdout).contains("evil"));
    assert_clean("list", &o.stdout);
}

#[test]
fn explain_sanitizes_message_and_policy_url() {
    let tmp = fixture();
    let o = run(tmp.path(), &["explain", "evil", "--color", "always"]);
    assert!(o.status.success(), "{o:?}");
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("\\x1b[2J"), "rendered visibly: {out}");
    assert_clean("explain", &o.stdout);
}

#[test]
fn export_agents_md_sanitizes_stdout() {
    let tmp = fixture();
    let o = run(tmp.path(), &["export-agents-md"]);
    assert!(o.status.success(), "{o:?}");
    assert_clean("export-agents-md", &o.stdout);
}

#[test]
fn check_human_sanitizes_bidi_controls() {
    let tmp = fixture();
    let o = run(tmp.path(), &["check", "--color", "always"]);
    assert_clean("check stdout", &o.stdout);
    assert_clean("check stderr", &o.stderr);
}

#[test]
fn show_notes_sanitizes_note_text() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        "version: 1\nrules:\n  \
         - id: paths-resolve\n    kind: registry_paths_resolve\n    \
         source: registry.txt\n    extract: { lines: {} }\n    \
         entries_are_globs: true\n    expect: file\n    level: warning\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("registry.txt"),
        format!("src/${{MODULE}}{CLEAR}{RLO}/lib.rs\n"),
    )
    .unwrap();
    let o = run(tmp.path(), &["check", "--show-notes"]);
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(stderr.contains("note:"), "{stderr}");
    assert_clean("notes", &o.stderr);
}

/// A repo file whose NAME carries the escape: `--show-baselined` lists it
/// on stderr.
#[cfg(unix)]
#[test]
fn show_baselined_sanitizes_paths() {
    let tmp = fixture();
    let evil = format!("x{CLEAR}{RLO}.txt");
    std::fs::write(tmp.path().join(&evil), "TODO\n").unwrap();
    let o = run(tmp.path(), &["baseline", "--quiet"]);
    assert!(o.status.success(), "{o:?}");
    let o = run(
        tmp.path(),
        &[
            "check",
            "--baseline",
            ".alint-baseline.json",
            "--show-baselined",
        ],
    );
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(stderr.contains("baselined:"), "{stderr}");
    assert_clean("show-baselined", &o.stderr);
}

/// `validate-config` (human) prints the config error on stderr: an ESC /
/// bidi char in the offending config text must not reach the terminal raw.
#[test]
fn validate_config_human_error_is_sanitized() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        "version: 1\n\"bad\\e[2Jkey\\u202E\": 1\nrules: []\n",
    )
    .unwrap();
    let o = run(tmp.path(), &["validate-config"]);
    assert_eq!(o.status.code(), Some(1), "{o:?}");
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(stderr.contains("\\x1b[2J"), "rendered visibly: {stderr}");
    assert_clean("validate-config stderr", &o.stderr);
    assert_clean("validate-config stdout", &o.stdout);
}

/// `explain` prints the `scope_filter:` gates: `has_ancestor`,
/// `changed_since` and the manifest predicate's source + resolved paths
/// (the latter read from repo content, here `package.json`).
#[test]
fn explain_sanitizes_scope_filter() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        "version: 1\nrules:\n  - id: r\n    kind: file_max_size\n    paths: \"**\"\n    \
         max_bytes: 10\n    level: error\n    scope_filter:\n      \
         has_ancestor: \"pkg\\e7x\\u202E\"\n      changed_since: \"main\\e7x\\u202E\"\n  \
         - id: m\n    kind: file_max_size\n    paths: \"**\"\n    max_bytes: 10\n    \
         level: error\n    scope_filter:\n      include_manifest_paths:\n        \
         source: package.json\n        extract: { json: \"$.workspaces[*]\" }\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("package.json"),
        "{\"workspaces\": [\"pkg/a\\u001b7x\\u202e\"]}\n",
    )
    .unwrap();
    // `--color never`: alint emits no ESC of its own, so ANY raw ESC is
    // injected. (`[` is a glob metachar `has_ancestor` rejects, so the
    // payload is `ESC 7`, save-cursor.)
    for id in ["r", "m"] {
        let o = run(tmp.path(), &["explain", id, "--color", "never"]);
        assert!(o.status.success(), "{o:?}");
        let out = String::from_utf8_lossy(&o.stdout);
        assert!(out.contains("\\x1b7x"), "{id}: rendered visibly: {out}");
        assert!(!out.contains('\u{1b}'), "{id}: raw ESC leaked: {out:?}");
        assert!(!out.contains(RLO), "{id}: raw U+202E leaked: {out:?}");
    }
}

/// `alint baseline` reports the file it wrote; the `baseline:` key (repo
/// config) names it.
#[cfg(unix)]
#[test]
fn baseline_wrote_line_is_sanitized() {
    let tmp = fixture();
    let cfg = std::fs::read_to_string(tmp.path().join(".alint.yml")).unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        format!("{cfg}baseline: \"b\\e[2J\\u202E.json\"\n"),
    )
    .unwrap();
    let o = run(tmp.path(), &["baseline"]);
    assert!(o.status.success(), "{o:?}");
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(stderr.contains("wrote"), "{stderr}");
    assert_clean("baseline", &o.stderr);
}

/// `suggest --explain` lists repo file names as evidence.
#[cfg(unix)]
#[test]
fn suggest_explain_sanitizes_evidence_paths() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join(format!("x{CLEAR}{RLO}.bak")), "old\n").unwrap();
    let o = run(tmp.path(), &["suggest", "--explain", "--color", "always"]);
    assert!(o.status.success(), "{o:?}");
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains(".bak"), "evidence lists the file: {out}");
    assert_clean("suggest", &o.stdout);
}
