//! End-to-end tests for the per-rule `baseline_key` behaviours (slice 4).
//!
//! These exercise the *cross-run* properties the single-run coverage audit
//! (`alint-e2e/tests/coverage_audit_baseline_safety.rs`) can't: that a baseline
//! taken on one tree state behaves correctly after the tree is edited —
//! a new structured-query finding surfaces (the v1-regression guard), a
//! threshold finding stays grandfathered as the file grows (the v3 path-shape
//! default), and a first-offender finding stays grandfathered after the first
//! offender is fixed (the file-level key).

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

fn write(dir: &Path, rel: &str, content: &str) {
    let p = dir.join(rel);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(p, content).unwrap();
}

/// A new structured-query finding on an already-baselined file must NOT be
/// masked by the baseline (the v1 `structured_path` hole). The key is the
/// JSONPath + matched value, so a different failing script is a distinct
/// fingerprint.
#[test]
fn structured_query_new_finding_is_not_masked() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    write(
        root,
        ".alint.yml",
        "version: 1\n\
         rules:\n\
         \x20 - id: scripts-echo-only\n\
         \x20   kind: json_path_matches\n\
         \x20   paths: package.json\n\
         \x20   path: \"$.scripts[*]\"\n\
         \x20   matches: \"^echo \"\n\
         \x20   level: error\n",
    );
    // One failing script → baseline it.
    write(
        root,
        "package.json",
        "{\"scripts\":{\"build\":\"webpack\"}}\n",
    );
    assert_eq!(
        code(&run(root, &["check"])),
        1,
        "build fails before baseline"
    );
    assert_eq!(code(&run(root, &["baseline"])), 0, "snapshot it");
    assert_eq!(
        code(&run(root, &["check", "--baseline", ".alint-baseline.json"])),
        0,
        "the baselined 'webpack' finding is suppressed",
    );

    // Add a DIFFERENT failing script on the same file.
    write(
        root,
        "package.json",
        "{\"scripts\":{\"build\":\"webpack\",\"test\":\"jest\"}}\n",
    );
    let out = run(root, &["check", "--baseline", ".alint-baseline.json"]);
    assert_eq!(
        code(&out),
        1,
        "the NEW 'jest' finding must surface, not be masked"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("jest"), "new finding reported: {stdout}");
    assert!(
        !stdout.contains("webpack"),
        "the baselined finding stays suppressed: {stdout}",
    );
}

/// A threshold finding (`file_max_lines`) keyed on the path stays grandfathered
/// as the file grows further — the magnitude is not in the fingerprint (v3
/// path-shape default), so a baselined over-limit file doesn't churn.
#[test]
fn threshold_stays_suppressed_as_file_grows() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    write(
        root,
        ".alint.yml",
        "version: 1\n\
         rules:\n\
         \x20 - id: not-too-long\n\
         \x20   kind: file_max_lines\n\
         \x20   paths: [\"*.txt\"]\n\
         \x20   max_lines: 3\n\
         \x20   level: error\n",
    );
    write(root, "big.txt", "1\n2\n3\n4\n5\n"); // 5 lines > 3
    assert_eq!(
        code(&run(root, &["check"])),
        1,
        "over the limit before baseline"
    );
    assert_eq!(code(&run(root, &["baseline"])), 0, "snapshot it");

    // Grow the file well past the baselined size.
    write(root, "big.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
    assert_eq!(
        code(&run(root, &["check", "--baseline", ".alint-baseline.json"])),
        0,
        "still the same accepted finding as the file grows — no churn",
    );
}

/// A first-offender finding (`no_trailing_whitespace`) is keyed on the file, so
/// fixing the first offending line keeps the file grandfathered while a later
/// offender remains — and surfaces clean only once the file is fully fixed.
#[test]
fn first_offender_keyed_on_file_survives_the_first_fix() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    write(
        root,
        ".alint.yml",
        "version: 1\n\
         rules:\n\
         \x20 - id: no-trailing-ws\n\
         \x20   kind: no_trailing_whitespace\n\
         \x20   paths: [\"*.txt\"]\n\
         \x20   level: error\n",
    );
    // Trailing whitespace on lines 1 and 3 (first offender = line 1).
    write(root, "code.txt", "line1 \nline2\nline3 \n");
    assert_eq!(code(&run(root, &["check"])), 1, "dirty before baseline");
    assert_eq!(code(&run(root, &["baseline"])), 0, "snapshot it");

    // Fix the FIRST offender; line 3 still trails → first offender moves.
    write(root, "code.txt", "line1\nline2\nline3 \n");
    assert_eq!(
        code(&run(root, &["check", "--baseline", ".alint-baseline.json"])),
        0,
        "the file-level key keeps it suppressed while any offender remains",
    );

    // Fully clean → no violation; the baseline entry is now stale (warned, not
    // failed, by default).
    write(root, "code.txt", "line1\nline2\nline3\n");
    let clean = run(root, &["check", "--baseline", ".alint-baseline.json"]);
    assert_eq!(code(&clean), 0, "fully fixed → clean");
    let stderr = String::from_utf8_lossy(&clean.stderr);
    assert!(
        stderr.contains("no longer fire") || stderr.contains("stale"),
        "a fully-fixed baselined finding warns as stale: {stderr}",
    );
}
