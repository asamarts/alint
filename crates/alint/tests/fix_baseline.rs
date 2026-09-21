//! W4 gate: baseline-aware `fix`. A resolved baseline (via `--baseline` or the
//! config `baseline:` key) makes `fix` skip the GRANDFATHERED findings and
//! resolve only NEW ones -- mirroring `check --baseline`. A grandfathered finding
//! is reported as a benign `baselined` skip and does NOT drive a nonzero exit.
//!
//! These drive the real binary end-to-end (the e2e scenario harness can't express
//! `--baseline`). `--strict-baseline` / `--show-baselined` for `fix` are a tracked
//! follow-up (still `check`-only).

use std::path::Path;
use std::path::PathBuf;
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

const CONFIG: &str = "version: 1\nrules:\n  - id: no-ws\n    kind: no_trailing_whitespace\n    \
     paths: \"**/*.txt\"\n    level: error\n    fix: { file_trim_trailing_whitespace: {} }\n";

/// The core: `fix --baseline` fixes a NEW finding but leaves a grandfathered one
/// untouched, and exits 0 (the grandfathered skip is benign).
#[test]
fn fix_baseline_fixes_new_and_skips_grandfathered() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), CONFIG).unwrap();
    // A pre-existing (to-be-grandfathered) finding.
    std::fs::write(root.join("old.txt"), "grandfathered   \n").unwrap();
    // Record the baseline over the current state.
    let bl = run(root, &["baseline", "--output", "bl.json", "."]);
    assert!(
        bl.status.success(),
        "baseline: {}",
        String::from_utf8_lossy(&bl.stderr)
    );

    // Introduce a NEW finding.
    std::fs::write(root.join("new.txt"), "fresh   \n").unwrap();

    // `fix --baseline`: fix the new one, skip the grandfathered one.
    let out = run(root, &["fix", "--baseline", "bl.json", "."]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "fix --baseline must exit 0 (new fixed; grandfathered skipped benignly); stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("1 applied") && stdout.contains("baselined"),
        "one fix applied, the grandfathered one surfaced as baselined; got:\n{stdout}"
    );

    // The grandfathered file is byte-identical; the new file is trimmed.
    assert_eq!(
        std::fs::read_to_string(root.join("old.txt")).unwrap(),
        "grandfathered   \n",
        "the grandfathered finding must NOT be fixed"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("new.txt")).unwrap(),
        "fresh\n",
        "the new finding must be fixed"
    );
}

/// Re-running `fix --baseline` when only grandfathered findings remain is a
/// converged no-op that exits 0 (a CI `fix --baseline` step must not fail just
/// because accepted debt still stands).
#[test]
fn fix_baseline_converges_to_exit_zero_on_only_grandfathered() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), CONFIG).unwrap();
    std::fs::write(root.join("old.txt"), "grandfathered   \n").unwrap();
    let bl = run(root, &["baseline", "--output", "bl.json", "."]);
    assert!(bl.status.success());

    // No new findings: everything present is grandfathered.
    let out = run(root, &["fix", "--baseline", "bl.json", "."]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "only-grandfathered fix --baseline must exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // And the grandfathered file is untouched.
    assert_eq!(
        std::fs::read_to_string(root.join("old.txt")).unwrap(),
        "grandfathered   \n"
    );
}

/// A `*_path_matches` rule whose located `replace` fixer emits ONE edit per
/// failing node. This is the shape that exposed the W4 CRITICAL under-suppression
/// bug: the fixer re-derives every failing node from the file, so it must be
/// constrained to the engine's LIVE (non-grandfathered) violation set.
const PATH_MATCHES_CONFIG: &str = concat!(
    "version: 1\n",
    "rules:\n",
    "  - id: v-pins\n",
    "    kind: json_path_matches\n",
    "    paths: \"**/*.json\"\n",
    "    path: \"$.deps.*\"\n",
    "    matches: \"^v\"\n",
    "    level: error\n",
    "    fix: { replace: { pattern: \"^\", replacement: \"v\", applicability: safe } }\n",
);

/// W4 CRITICAL regression (audit 2026-09-20): a located `replace` fixer must NOT
/// rewrite GRANDFATHERED nodes. The fixer re-derives every failing node from the
/// file, so before the fix it ignored the live-only set the engine handed it and
/// mutated accepted debt -- `check --baseline` reported only the new node while
/// `fix --baseline` rewrote the grandfathered ones in the SAME file. `check` and
/// `fix` must agree: only the new node changes.
#[test]
fn fix_baseline_located_replace_leaves_grandfathered_nodes_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), PATH_MATCHES_CONFIG).unwrap();
    // Two pre-existing failing nodes (distinct values) -> grandfathered.
    std::fs::write(
        root.join("data.json"),
        "{\n  \"deps\": {\n    \"alpha\": \"1.0\",\n    \"beta\": \"2.0\"\n  }\n}\n",
    )
    .unwrap();
    let bl = run(root, &["baseline", "--output", "bl.json", "."]);
    assert!(
        bl.status.success(),
        "baseline: {}",
        String::from_utf8_lossy(&bl.stderr)
    );

    // A NEW failing node in the SAME file.
    std::fs::write(
        root.join("data.json"),
        "{\n  \"deps\": {\n    \"alpha\": \"1.0\",\n    \"beta\": \"2.0\",\n    \"gamma\": \"3.0\"\n  }\n}\n",
    )
    .unwrap();

    // `check --baseline` reports ONLY the new node (the baseline suppresses the
    // two grandfathered ones).
    let chk = run(root, &["check", "--baseline", "bl.json", "."]);
    assert_eq!(
        chk.status.code(),
        Some(1),
        "the new node still fails check --baseline"
    );

    // `fix --baseline` fixes ONLY the new node; the grandfathered two are
    // byte-identical.
    let out = run(root, &["fix", "--baseline", "bl.json", "."]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "fix --baseline exits 0 (new fixed, grandfathered baselined); stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let data = std::fs::read_to_string(root.join("data.json")).unwrap();
    assert!(
        data.contains("\"alpha\": \"1.0\""),
        "grandfathered alpha must be untouched; got:\n{data}"
    );
    assert!(
        data.contains("\"beta\": \"2.0\""),
        "grandfathered beta must be untouched; got:\n{data}"
    );
    assert!(
        data.contains("\"gamma\": \"v3.0\""),
        "the new node gamma must be fixed; got:\n{data}"
    );
}

/// W4 CRITICAL regression (budget half): when N identical failing values are
/// grandfathered with a count, and MORE identical values appear, `fix --baseline`
/// resolves exactly the delta (the live count) and leaves the grandfathered count
/// as accepted debt -- mirroring how `check --baseline` draws down the budget.
#[test]
fn fix_baseline_located_replace_respects_the_grandfathered_count() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), PATH_MATCHES_CONFIG).unwrap();
    // Two IDENTICAL failing values -> the baseline records count 2 for the one
    // "dup" fingerprint.
    std::fs::write(
        root.join("data.json"),
        "{\n  \"deps\": {\n    \"a\": \"dup\",\n    \"b\": \"dup\"\n  }\n}\n",
    )
    .unwrap();
    let bl = run(root, &["baseline", "--output", "bl.json", "."]);
    assert!(bl.status.success());

    // A THIRD identical value: 3 present, 2 grandfathered -> exactly 1 live.
    std::fs::write(
        root.join("data.json"),
        "{\n  \"deps\": {\n    \"a\": \"dup\",\n    \"b\": \"dup\",\n    \"c\": \"dup\"\n  }\n}\n",
    )
    .unwrap();

    let out = run(root, &["fix", "--baseline", "bl.json", "."]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let data = std::fs::read_to_string(root.join("data.json")).unwrap();
    // Exactly one occurrence fixed (3 present - 2 grandfathered); two remain as
    // debt. (`"vdup"` does not contain the substring `"dup"`, so the counts are
    // disjoint.)
    assert_eq!(
        data.matches("\"vdup\"").count(),
        1,
        "exactly one new node fixed; got:\n{data}"
    );
    assert_eq!(
        data.matches("\"dup\"").count(),
        2,
        "two grandfathered nodes remain as debt; got:\n{data}"
    );
}

/// W4 MEDIUM regression (audit F3, 2026-09-20): a WHOLE-FILE-fixer rule that
/// reports only the first offender (`no_zero_width_chars`) must not strip a
/// GRANDFATHERED occurrence when a NEW one precedes it. Keying the violation on
/// the path (the file is the unit of accepted debt) grandfathers the whole file,
/// so `fix --baseline` leaves it byte-identical.
#[test]
fn fix_baseline_whole_file_fixer_leaves_grandfathered_occurrences() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: no-zw\n    kind: no_zero_width_chars\n    \
         paths: \"**/*.txt\"\n    level: error\n    fix: { file_strip_zero_width: {} }\n",
    )
    .unwrap();
    // Grandfathered state: a zero-width char (U+200B, built via escape) on line 2.
    let before = "cleanline\nold\u{200B}content\n";
    std::fs::write(root.join("f.txt"), before).unwrap();
    let bl = run(root, &["baseline", "--output", "bl.json", "."]);
    assert!(bl.status.success());

    // A NEW zero-width char on line 1, ahead of the grandfathered one -- this is
    // what shifted the first-offender fingerprint and let the whole-file fixer run.
    let after_edit = "new\u{200B}line\nold\u{200B}content\n";
    std::fs::write(root.join("f.txt"), after_edit).unwrap();

    let out = run(root, &["fix", "--baseline", "bl.json", "."]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "the whole file is grandfathered -> benign exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The file is byte-identical: neither the grandfathered nor the new char is
    // stripped -- the whole-file strip fixer never runs on grandfathered content.
    assert_eq!(
        std::fs::read_to_string(root.join("f.txt")).unwrap(),
        after_edit,
        "no zero-width char may be stripped from a grandfathered file"
    );
}

/// A suggestion-tier `replace` on `*_path_matches`: the fix is offered but NEVER
/// auto-applied, so a live finding STANDS (exit per its level). Used to check the
/// "a new unresolved finding still fails" half of the `fix --baseline` contract.
fn suggest_config(level: &str) -> String {
    format!(
        "version: 1\nrules:\n  - id: v-pins\n    kind: json_path_matches\n    \
         paths: \"**/*.json\"\n    path: \"$.deps.*\"\n    matches: \"^v\"\n    \
         level: {level}\n    fix: {{ replace: {{ pattern: \"^\", replacement: \"v\", \
         applicability: suggestion }} }}\n"
    )
}

/// A missing or unparseable `--baseline` file is a LOUD error (exit 2), never a
/// silent full-fix -- the safety contract in this file's header.
#[test]
fn fix_rejects_a_missing_or_invalid_baseline_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), CONFIG).unwrap();
    std::fs::write(root.join("f.txt"), "trailing   \n").unwrap();

    let missing = run(root, &["fix", "--baseline", "does-not-exist.json", "."]);
    assert_eq!(
        missing.status.code(),
        Some(2),
        "a missing baseline must be a loud error, not a silent full-fix"
    );
    std::fs::write(root.join("bad.json"), "not a baseline\n").unwrap();
    let invalid = run(root, &["fix", "--baseline", "bad.json", "."]);
    assert_eq!(
        invalid.status.code(),
        Some(2),
        "an unparseable baseline must be a loud error"
    );
    // Neither error run may have fixed the file.
    assert_eq!(
        std::fs::read_to_string(root.join("f.txt")).unwrap(),
        "trailing   \n"
    );
}

/// The config `baseline:` key auto-enables baseline-aware `fix` -- no `--baseline`
/// flag needed (parity with `check`, which honors the same key).
#[test]
fn fix_honors_the_config_baseline_key() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), CONFIG).unwrap();
    std::fs::write(root.join("old.txt"), "grandfathered   \n").unwrap();
    let bl = run(root, &["baseline", "--output", "bl.json", "."]);
    assert!(bl.status.success());

    // Point the config at the baseline (no flag), then add a new finding.
    std::fs::write(
        root.join(".alint.yml"),
        format!("{CONFIG}baseline: bl.json\n"),
    )
    .unwrap();
    std::fs::write(root.join("new.txt"), "fresh   \n").unwrap();

    let out = run(root, &["fix", "."]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "config-key baseline fixes new + grandfathers old; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(root.join("old.txt")).unwrap(),
        "grandfathered   \n",
        "the config-baselined finding is grandfathered"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("new.txt")).unwrap(),
        "fresh\n",
        "the new finding is fixed"
    );
}

/// A NEW error-level finding whose fix is suggestion-tier (never auto-applied) is
/// UNRESOLVED even under `--baseline` -> exit 1. The baseline suppresses only the
/// grandfathered finding; a new one still fails the run.
#[test]
fn fix_baseline_new_suggestion_only_error_still_exits_one() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), suggest_config("error")).unwrap();
    std::fs::write(
        root.join("d.json"),
        "{\n  \"deps\": {\n    \"old\": \"1.0\"\n  }\n}\n",
    )
    .unwrap();
    let bl = run(root, &["baseline", "--output", "bl.json", "."]);
    assert!(bl.status.success());
    std::fs::write(
        root.join("d.json"),
        "{\n  \"deps\": {\n    \"old\": \"1.0\",\n    \"new\": \"2.0\"\n  }\n}\n",
    )
    .unwrap();

    let out = run(root, &["fix", "--baseline", "bl.json", "."]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "a new suggestion-only error is unresolved under --baseline; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The suggestion is not applied and old is grandfathered.
    assert!(
        std::fs::read_to_string(root.join("d.json"))
            .unwrap()
            .contains("\"new\": \"2.0\""),
        "the suggestion-tier fix is not auto-applied"
    );
}

/// A NEW warning-level unresolved finding under `--baseline` exits 0 by default
/// and 1 under `--fail-on-warning` -- identical to `fix` WITHOUT a baseline, so
/// the baseline never changes warning-vs-error exit semantics for NEW findings.
#[test]
fn fix_baseline_new_warning_respects_fail_on_warning() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), suggest_config("warning")).unwrap();
    std::fs::write(
        root.join("d.json"),
        "{\n  \"deps\": {\n    \"old\": \"1.0\"\n  }\n}\n",
    )
    .unwrap();
    let bl = run(root, &["baseline", "--output", "bl.json", "."]);
    assert!(bl.status.success());
    std::fs::write(
        root.join("d.json"),
        "{\n  \"deps\": {\n    \"old\": \"1.0\",\n    \"new\": \"2.0\"\n  }\n}\n",
    )
    .unwrap();

    let default = run(root, &["fix", "--baseline", "bl.json", "."]);
    assert_eq!(
        default.status.code(),
        Some(0),
        "a new warning is benign by default; stderr: {}",
        String::from_utf8_lossy(&default.stderr)
    );
    let strict = run(
        root,
        &["fix", "--baseline", "bl.json", "--fail-on-warning", "."],
    );
    assert_eq!(
        strict.status.code(),
        Some(1),
        "--fail-on-warning fails on the new warning"
    );
}

/// `--strict-baseline` / `--show-baselined` remain `check`-only for `fix` -- a
/// loud rejection, never a silent no-op.
#[test]
fn fix_rejects_strict_and_show_baselined() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), CONFIG).unwrap();
    for flag in ["--strict-baseline", "--show-baselined"] {
        let out = run(root, &[flag, "fix", "."]);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{flag} must be rejected for fix"
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("apply only to `check`"),
            "{flag}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
