//! Round-4 audit: detector-domain == fixer-domain convergence.
//!
//! A content rule's DETECTOR must flag exactly the files its FIXER can fix, or a
//! file in the gap is reported "fixable" forever yet never fixed (`alint fix`
//! never converges). Two gaps were found and closed:
//!
//!   * invalid-UTF-8-but-not-binary (a lone `0xFF`, no NUL): the byte-level
//!     detectors (bidi / zero-width / trailing-whitespace) flag it, so the
//!     fixers now strip at the BYTE level and preserve the junk byte instead of
//!     bailing on a strict `from_utf8`. The bidi case is security-relevant: a
//!     Trojan-Source override must not survive `alint fix` just because the
//!     attacker also dropped one invalid byte.
//!   * NUL-bearing binary: the fixers refuse it (editing binary corrupts it), so
//!     the detectors now skip it too -- `check` and `fix` agree.
//!
//! These live as an integration test rather than a scenario/property because a
//! raw `0xFF` byte cannot be represented in a UTF-8 YAML scenario tree.

use std::path::{Path, PathBuf};
use std::process::Command;

fn alint() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn write(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(p, bytes).unwrap();
}

fn config(root: &Path, yaml: &str) {
    std::fs::write(root.join(".alint.yml"), yaml).unwrap();
}

/// `alint check` exits 0 when the tree is clean, 1 when any violation remains.
/// Robust convergence signal (counting `✗` glyphs double-counts the summary).
fn check_is_clean(root: &Path) -> bool {
    Command::new(alint())
        .args(["check", "."])
        .current_dir(root)
        .output()
        .expect("run alint check")
        .status
        .code()
        == Some(0)
}

fn fix(root: &Path) -> std::process::Output {
    Command::new(alint())
        .args(["fix", "."])
        .current_dir(root)
        .output()
        .expect("run alint fix")
}

/// The security case (F1): a bidi override plus one invalid UTF-8 byte must be
/// stripped by `alint fix`, and the fix must converge. The junk byte survives.
#[test]
fn bidi_control_with_invalid_utf8_byte_is_stripped_and_converges() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // `a` `0xFF` U+202E(RLO, E2 80 AE) `b` `\n`
    write(root, "src/evil.rs", b"a\xFF\xE2\x80\xAEb\n");
    config(
        root,
        "version: 1\nrules:\n  - id: no-bidi\n    kind: no_bidi_controls\n    \
         paths: \"src/**/*.rs\"\n    level: error\n    fix: { file_strip_bidi: {} }\n",
    );
    assert!(!check_is_clean(root), "the bidi override must be flagged");
    let out = fix(root);
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("1 applied"),
        "fix must strip the bidi control, got: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(
        std::fs::read(root.join("src/evil.rs")).unwrap(),
        b"a\xFFb\n",
        "the RLO is gone and the junk 0xFF byte is preserved"
    );
    assert!(
        check_is_clean(root),
        "converged: no bidi violation remains after one fix"
    );
}

#[test]
fn zero_width_with_invalid_utf8_byte_is_stripped_and_converges() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // `a` `0xFF` U+200B(ZWSP, E2 80 8B) `b` `\n`
    write(root, "src/z.rs", b"a\xFF\xE2\x80\x8Bb\n");
    config(
        root,
        "version: 1\nrules:\n  - id: no-zw\n    kind: no_zero_width_chars\n    \
         paths: \"src/**/*.rs\"\n    level: error\n    fix: { file_strip_zero_width: {} }\n",
    );
    assert!(!check_is_clean(root));
    fix(root);
    assert_eq!(std::fs::read(root.join("src/z.rs")).unwrap(), b"a\xFFb\n");
    assert!(check_is_clean(root), "converged");
}

#[test]
fn trailing_whitespace_with_invalid_utf8_byte_is_trimmed_and_converges() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // `a` `0xFF` space space `\n`
    write(root, "src/w.rs", b"a\xFF  \n");
    config(
        root,
        "version: 1\nrules:\n  - id: ws\n    kind: no_trailing_whitespace\n    \
         paths: \"src/**/*.rs\"\n    level: error\n    fix: { file_trim_trailing_whitespace: {} }\n",
    );
    assert!(!check_is_clean(root));
    fix(root);
    assert_eq!(std::fs::read(root.join("src/w.rs")).unwrap(), b"a\xFF\n");
    assert!(check_is_clean(root), "converged");
}

/// F3: every byte-level content rule skips a NUL-bearing binary at the DETECTOR,
/// so `check` reports nothing and `fix` touches nothing (agreement, not the old
/// "flagged fixable forever, never fixed"). One representative file exercised by
/// all six rules on a broad glob.
#[test]
fn detectors_skip_binary_so_check_and_fix_agree() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // NUL-bearing "binary" that would otherwise trip trailing-ws, missing final
    // newline, CRLF, a blank run, a bidi control, and a zero-width char.
    let binary: &[u8] = b"a\x00 \r\n\n\n\n\xE2\x80\xAE\xE2\x80\x8Bb   ";
    write(root, "blob.bin", binary);
    config(
        root,
        "version: 1\nrules:\n\
         \x20 - id: ws\n    kind: no_trailing_whitespace\n    paths: \"**/*.bin\"\n    level: error\n    fix: { file_trim_trailing_whitespace: {} }\n\
         \x20 - id: eof\n    kind: final_newline\n    paths: \"**/*.bin\"\n    level: error\n    fix: { file_append_final_newline: {} }\n\
         \x20 - id: le\n    kind: line_endings\n    paths: \"**/*.bin\"\n    target: lf\n    level: error\n    fix: { file_normalize_line_endings: {} }\n\
         \x20 - id: blanks\n    kind: max_consecutive_blank_lines\n    paths: \"**/*.bin\"\n    max: 1\n    level: error\n    fix: { file_collapse_blank_lines: {} }\n\
         \x20 - id: bidi\n    kind: no_bidi_controls\n    paths: \"**/*.bin\"\n    level: error\n    fix: { file_strip_bidi: {} }\n\
         \x20 - id: zw\n    kind: no_zero_width_chars\n    paths: \"**/*.bin\"\n    level: error\n    fix: { file_strip_zero_width: {} }\n",
    );
    assert!(
        check_is_clean(root),
        "no content rule may flag a NUL-bearing binary (its fixer would refuse it)"
    );
    fix(root);
    assert_eq!(
        std::fs::read(root.join("blob.bin")).unwrap(),
        binary,
        "the binary is left byte-identical"
    );
}
