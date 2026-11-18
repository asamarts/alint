//! CLI coverage for the v0.11 informational-notes channel + the
//! `--show-notes` flag. A `registry_paths_resolve` rule with a
//! non-literal (interpolated) entry skips that entry and surfaces it as
//! a note rather than a violation; the note is summarized on stderr by
//! default and listed in full with `--show-notes`.

use std::path::Path;
use std::process::Output;

fn run(dir: &Path, args: &[&str]) -> Output {
    crate::hermetic::command(env!("CARGO_BIN_EXE_alint"))
        .args(args)
        .arg(dir)
        .output()
        .expect("run alint")
}

fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  \
         - id: paths-resolve\n    kind: registry_paths_resolve\n    \
         source: registry.txt\n    extract: { lines: {} }\n    \
         entries_are_globs: true\n    expect: file\n    level: warning\n",
    )
    .unwrap();
    // A single non-literal entry: skipped (can't statically resolve an
    // interpolated path) -> one informational note, zero violations.
    std::fs::write(root.join("registry.txt"), "src/${MODULE}/lib.rs\n").unwrap();
    tmp
}

#[test]
fn default_run_prints_a_one_line_note_count() {
    let tmp = fixture();
    let out = run(tmp.path(), &["check"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("informational note(s); run with --show-notes"),
        "default run should print the one-line note count; stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("skipped non-literal entry"),
        "default run should not list note bodies; stderr:\n{stderr}"
    );
    // Note-only (no violations) -> success exit.
    assert!(
        out.status.success(),
        "note-only run should exit 0, got {:?}",
        out.status
    );
}

#[test]
fn show_notes_lists_the_note_in_full() {
    let tmp = fixture();
    let out = run(tmp.path(), &["check", "--show-notes"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("informational note(s):"),
        "--show-notes should print the list header; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("skipped non-literal entry"),
        "--show-notes should list the registry note; stderr:\n{stderr}"
    );
}
