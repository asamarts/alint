//! Audit F1/F2 (2026-09-20): a `fix` skip's exit-code class is STRUCTURAL
//! (`SkipKind`), never sniffed from the human reason string. A skip reason is
//! built from the file path, so a path that starts with a sentinel prefix must
//! NOT forge the class:
//!  - F1: an error-level fix size-skipped on a file named `baselined:*` must still
//!    exit 1 (it is DECLINED, not grandfathered).
//!  - F2: a benign size residual under `--fix-only` on a file named `fix error:*`
//!    must exit 0 (it is DECLINED, not an I/O error).
//!
//! These drive the real binary because the forgery lives in the path-derived
//! reason string, which only the end-to-end fix path produces. Both filenames are
//! legal on POSIX (a colon, and a space, are allowed).

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

// A tiny `fix_size_limit` so any real file oversizes it: the trailing-whitespace
// fixer then DECLINES with a size-skip whose reason begins with the file path.
fn config(level: &str) -> String {
    format!(
        "version: 1\nfix_size_limit: 8\nrules:\n  - id: no-ws\n    \
         kind: no_trailing_whitespace\n    paths: \"**/*.txt\"\n    level: {level}\n    \
         fix: {{ file_trim_trailing_whitespace: {{}} }}\n"
    )
}

// Trailing whitespace (so `no_trailing_whitespace` fires on the check side) and
// well over the 8-byte fix_size_limit (so the fixer size-skips it).
const OVERSIZE: &str = "this line has trailing spaces to exceed eight bytes   \n";

/// F1: the size-skip reason starts with the file path, so a file named
/// `baselined:evil.txt` makes it begin with the literal `baselined:`. The skip is
/// DECLINED (not grandfathered), so an error-level run still exits 1.
#[test]
fn error_size_skip_on_a_baselined_named_file_still_exits_one() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), config("error")).unwrap();
    std::fs::write(root.join("baselined:evil.txt"), OVERSIZE).unwrap();

    let out = run(root, &["fix", "."]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "a declined size-skip is unresolved, not grandfathered; stdout: {} stderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // The file is too big to fix, so it is byte-identical (never mutated).
    assert_eq!(
        std::fs::read_to_string(root.join("baselined:evil.txt")).unwrap(),
        OVERSIZE
    );
}

/// F2: `--fix-only` exits 0 on a benign (declined) residual. A file named
/// `fix error:big.txt` makes the size-skip reason begin with the literal
/// `fix error:`, but the skip is DECLINED (not an I/O error), so `--fix-only`
/// still exits 0.
#[test]
fn fix_only_on_a_fix_error_named_file_still_exits_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), config("warning")).unwrap();
    std::fs::write(root.join("fix error:big.txt"), OVERSIZE).unwrap();

    let out = run(root, &["fix", "--fix-only", "."]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "a declined size residual is not a fix error; stdout: {} stderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
