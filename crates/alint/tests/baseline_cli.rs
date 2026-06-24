//! End-to-end tests for `alint baseline` + `check --baseline`.
//!
//! Drives the built binary against throwaway repos to exercise the full
//! grandfathering flow: snapshot the current violations, then gate on
//! the delta. The matching/fingerprint internals are unit-tested in
//! `alint_core::baseline`; this asserts the CLI wiring and exit codes.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn alint() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

/// A repo with two error rules (a missing-final-newline check and a
/// forbidden-`TODO` check) and two violating files.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(".alint.yml"),
        "version: 1\n\
         rules:\n\
         \x20 - id: needs-newline\n\
         \x20   kind: final_newline\n\
         \x20   paths: [\"**/*.txt\"]\n\
         \x20   level: error\n\
         \x20 - id: no-todo\n\
         \x20   kind: file_content_forbidden\n\
         \x20   paths: [\"**/*.txt\"]\n\
         \x20   pattern: \"TODO\"\n\
         \x20   level: error\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("a.txt"), "no newline").unwrap();
    std::fs::write(dir.path().join("b.txt"), "has a TODO\nmore\n").unwrap();
    dir
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

#[test]
fn baseline_then_check_grandfathers_existing_and_gates_on_new() {
    let d = fixture();
    let root = d.path();

    // Plain check fails on the two pre-existing errors.
    assert_eq!(
        code(&run(root, &["check"])),
        1,
        "two errors before baseline"
    );

    // Snapshot them.
    let out = run(root, &["baseline"]);
    assert_eq!(code(&out), 0, "baseline writes successfully");
    let baseline_path = root.join(".alint-baseline.json");
    assert!(baseline_path.is_file(), "baseline file created");
    let text = std::fs::read_to_string(&baseline_path).unwrap();
    // Header + 2 entries, and the advisory message is present for review.
    assert!(
        text.lines()
            .next()
            .unwrap()
            .contains("\"schema_version\":1")
    );
    assert_eq!(
        text.lines()
            .filter(|l| l.contains("\"fingerprint\""))
            .count(),
        2
    );
    assert!(text.contains("file does not end with a newline"));

    // With the baseline, the pre-existing violations are suppressed → clean.
    let checked = run(root, &["check", "--baseline", ".alint-baseline.json"]);
    assert_eq!(code(&checked), 0, "all baselined → exit 0");
    let stderr = String::from_utf8_lossy(&checked.stderr);
    assert!(
        stderr.contains("2 baselined violation(s) suppressed"),
        "{stderr}"
    );

    // A NEW violation is reported and fails; the old ones stay suppressed.
    std::fs::write(root.join("c.txt"), "another TODO\n").unwrap();
    let with_new = run(root, &["check", "--baseline", ".alint-baseline.json"]);
    assert_eq!(code(&with_new), 1, "a new violation fails the gate");
    let out_s = String::from_utf8_lossy(&with_new.stdout);
    assert!(
        out_s.contains("c.txt"),
        "the new violation is reported: {out_s}"
    );
    assert!(
        !out_s.contains("a.txt"),
        "the baselined one is not: {out_s}"
    );
}

#[test]
fn show_baselined_lists_suppressed_findings() {
    let d = fixture();
    let root = d.path();
    run(root, &["baseline"]);
    let out = run(
        root,
        &[
            "check",
            "--baseline",
            ".alint-baseline.json",
            "--show-baselined",
        ],
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("[needs-newline]"), "{stderr}");
    assert!(stderr.contains("[no-todo]"), "{stderr}");
}

#[test]
fn missing_baseline_file_is_a_hard_error() {
    let d = fixture();
    let out = run(d.path(), &["check", "--baseline", "does-not-exist.json"]);
    assert_eq!(
        code(&out),
        2,
        "missing baseline → config-error exit, not a silent no-op"
    );
}

#[test]
fn unsupported_schema_version_is_rejected() {
    let d = fixture();
    std::fs::write(d.path().join("bad.json"), "{\"schema_version\":999}\n").unwrap();
    let out = run(d.path(), &["check", "--baseline", "bad.json"]);
    assert_eq!(code(&out), 2);
    assert!(String::from_utf8_lossy(&out.stderr).contains("schema_version 999 is unsupported"));
}

#[test]
fn regeneration_refuses_to_grandfather_new_debt_without_accept_new() {
    let d = fixture();
    let root = d.path();
    run(root, &["baseline"]); // baseline the original two
    std::fs::write(root.join("c.txt"), "another TODO\n").unwrap(); // introduce new debt

    // Re-running baseline must refuse to silently grandfather c.txt.
    let refused = run(root, &["baseline"]);
    assert_eq!(code(&refused), 2, "refuses without --accept-new");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("grandfather 1 new violation"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );

    // --accept-new takes it.
    assert_eq!(code(&run(root, &["baseline", "--accept-new"])), 0);
}

#[test]
fn baseline_rejects_changed_scope() {
    let d = fixture();
    let out = run(d.path(), &["baseline", "--changed"]);
    assert_ne!(code(&out), 0, "a baseline must be whole-tree");
    assert!(String::from_utf8_lossy(&out.stderr).contains("unexpected argument"));
}

#[test]
fn strict_baseline_fails_on_stale_entries() {
    let d = fixture();
    let root = d.path();
    run(root, &["baseline"]);

    // Fix one of the baselined violations → its entry goes stale.
    std::fs::write(root.join("a.txt"), "clean\n").unwrap();

    // Warn-only (default): stale doesn't fail the build.
    let warn = run(root, &["check", "--baseline", ".alint-baseline.json"]);
    assert_eq!(code(&warn), 0, "warn-only: fixing must not fail");
    assert!(String::from_utf8_lossy(&warn.stderr).contains("no longer fire"));

    // --strict-baseline: stale fails.
    let strict = run(
        root,
        &[
            "check",
            "--baseline",
            ".alint-baseline.json",
            "--strict-baseline",
        ],
    );
    assert_eq!(code(&strict), 1, "strict: stale fails the build");
}

/// A `baseline:` config key makes suppression active with no `--baseline`
/// flag, and `alint baseline` writes to that same path by default — so the
/// writer and reader never split-brain.
#[test]
fn config_key_baseline_suppresses_without_the_flag() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\n\
         baseline: .alint-baseline.json\n\
         rules:\n\
         \x20 - id: needs-newline\n\
         \x20   kind: final_newline\n\
         \x20   paths: [\"**/*.txt\"]\n\
         \x20   level: error\n",
    )
    .unwrap();
    std::fs::write(root.join("a.txt"), "no newline").unwrap();

    // With the config key set but no baseline file yet, `check` is a hard
    // error (exit 2) — a forgotten `alint baseline` must not silently run
    // ungated. You bootstrap with `alint baseline` (which writes), below.
    assert_eq!(
        code(&run(root, &["check"])),
        2,
        "config key + missing file → hard error, not a silent un-suppressed pass",
    );

    // `alint baseline` (no --output) writes to the config-key path.
    assert_eq!(code(&run(root, &["baseline"])), 0);
    assert!(
        root.join(".alint-baseline.json").is_file(),
        "baseline written to the config-key path by default",
    );

    // `alint check` with NO --baseline flag now honors the config key.
    assert_eq!(
        code(&run(root, &["check"])),
        0,
        "config-key baseline suppresses without the flag",
    );

    // A NEW violation still surfaces.
    std::fs::write(root.join("b.txt"), "also no newline").unwrap();
    assert_eq!(code(&run(root, &["check"])), 1, "new violation still gates");
}

/// The `--baseline` flag overrides the `baseline:` config key; and when only
/// the config key is in effect and its file is missing, that's the same hard
/// error as a missing `--baseline` (proving the key is actually consulted).
#[test]
fn baseline_flag_overrides_the_config_key() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\n\
         baseline: does-not-exist.json\n\
         rules:\n\
         \x20 - id: needs-newline\n\
         \x20   kind: final_newline\n\
         \x20   paths: [\"**/*.txt\"]\n\
         \x20   level: error\n",
    )
    .unwrap();
    std::fs::write(root.join("a.txt"), "no newline").unwrap();

    // A real baseline at a different path.
    assert_eq!(code(&run(root, &["baseline", "--output", "real.json"])), 0);

    // --baseline real.json overrides the (missing) config-key path → suppressed.
    assert_eq!(
        code(&run(root, &["check", "--baseline", "real.json"])),
        0,
        "the flag overrides the config key",
    );

    // Without the flag, the config key (a missing file) is consulted → the
    // documented hard error (exit 2), never a silent un-suppressed run.
    assert_eq!(
        code(&run(root, &["check"])),
        2,
        "config key points at a missing file → hard error, proving it's consulted",
    );
}

/// `check --baseline --format sarif` MARKS baselined findings (suppressions +
/// baselineState:unchanged) instead of dropping them, so GitHub Code Scanning
/// dismisses rather than closes-then-reopens the alert. A fully-baselined repo
/// still exits 0 — the exit code is gated on live findings only.
#[test]
fn sarif_marks_baselined_findings_not_removed() {
    let d = fixture();
    let root = d.path();
    assert_eq!(
        code(&run(root, &["baseline"])),
        0,
        "snapshot the 2 findings"
    );

    let out = run(
        root,
        &[
            "check",
            "--baseline",
            ".alint-baseline.json",
            "--format",
            "sarif",
        ],
    );
    assert_eq!(
        code(&out),
        0,
        "fully-baselined → exit 0 (gated on live only)"
    );
    let sarif = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        sarif.matches("\"baselineState\": \"unchanged\"").count(),
        2,
        "both baselined findings are emitted + marked, not removed:\n{sarif}",
    );
    assert_eq!(
        sarif.matches("\"kind\": \"external\"").count(),
        2,
        "each baselined finding carries an external suppression",
    );
    assert!(
        sarif.contains("\"partialFingerprints\""),
        "fingerprints present for Code Scanning correlation",
    );
}
