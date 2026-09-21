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
