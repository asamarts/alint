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
