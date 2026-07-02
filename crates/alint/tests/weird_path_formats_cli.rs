//! Weird filenames × output formats — end-to-end through the real binary.
//!
//! The library-level renderer matrix lives in
//! `alint-e2e::tests::output_weird_paths`; this file proves the same property
//! survives the *whole* pipeline (walker → engine → CLI renderer) with real
//! files on disk. Its headline case is the regression that motivated the fix:
//! a repo containing a single non-UTF-8 filename used to make
//! `alint check --format json` (and `--format agent`) abort with
//! `exit 2: "path contains invalid UTF-8 characters"` while every other format
//! worked — one oddly-named file rendered the JSON output unusable.
//!
//! `#![cfg(unix)]` — non-UTF-8 filenames are a Unix concern, and several of the
//! UTF-8 cases (`"`, `\`, `<`) aren't even legal on Windows.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;

fn alint_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn run(dir: &Path, args: &[&str]) -> Output {
    std::process::Command::new(alint_bin())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn alint")
}

/// A repo whose one rule fires on every `*.txt` containing `FIXME`, so each
/// weird-named file yields exactly one path-bearing violation.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .prefix("alint-weirdpath-")
        .tempdir()
        .unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        "version: 1\nrules:\n  - id: no-fixme\n    kind: file_content_forbidden\n    \
         paths: \"**/*.txt\"\n    pattern: \"FIXME\"\n    level: error\n",
    )
    .unwrap();
    dir
}

/// JSON-family formats whose stdout must parse as JSON.
const JSON_FORMATS: &[&str] = &["json", "sarif", "gitlab", "agent"];
/// Formats that emit non-JSON but must still not crash.
const TEXT_FORMATS: &[&str] = &["human", "github", "markdown", "junit"];

#[test]
fn weird_utf8_filenames_render_across_all_formats() {
    let dir = repo();
    // Realistic weird-but-legal (on Unix) filenames, each a format's headache.
    for name in [
        "my file.txt",      // space
        "café.txt",         // non-ASCII UTF-8
        "文書.txt",         // CJK
        "🚀.txt",           // emoji (multi-byte)
        "quote\".txt",      // double quote (JSON string / XML attr)
        "back\\slash.txt",  // backslash (JSON escape)
        "angle<>&.txt",     // XML/HTML metacharacters
        "pipe|tick`.txt",   // markdown table / inline code
        "comma,colon:.txt", // github annotation separators
        "pct%hash#.txt",    // URI-reserved
    ] {
        std::fs::write(dir.path().join(name), "FIXME\n").unwrap();
    }

    for fmt in JSON_FORMATS.iter().chain(TEXT_FORMATS) {
        let out = run(dir.path(), &["check", ".", "--format", fmt]);
        // Violations exist → exit 1. The failure we guard against is exit 2
        // (renderer aborted) — never acceptable here.
        assert_ne!(
            out.status.code(),
            Some(2),
            "[{fmt}] aborted (exit 2) on weird UTF-8 filenames; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !out.stdout.is_empty(),
            "[{fmt}] produced no output for weird filenames"
        );
        if JSON_FORMATS.contains(fmt) {
            serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap_or_else(|e| {
                panic!(
                    "[{fmt}] stdout is not valid JSON: {e}\n{}",
                    String::from_utf8_lossy(&out.stdout)
                )
            });
        }
    }
}

// Linux-only: staging this end-to-end needs a filesystem that stores a
// filename with an invalid UTF-8 byte. Linux (ext4/tmpfs/…) does; macOS's
// APFS/HFS+ reject non-UTF-8 names outright, so the `fs::write` would fail
// there before the renderer is ever exercised. The renderer-level guarantee
// for a non-UTF-8 path is proven filesystem-free (an in-memory `OsString`)
// by `alint-e2e::output_weird_paths::every_format_handles_a_non_utf8_path`,
// which runs on every Unix.
#[cfg(target_os = "linux")]
#[test]
fn non_utf8_filename_does_not_break_json_or_agent() {
    use std::os::unix::ffi::OsStrExt;
    let dir = repo();
    // A filename with an invalid UTF-8 byte (0xFF). Legal on Linux filesystems.
    let mut name = std::ffi::OsString::from("bad");
    name.push(std::ffi::OsStr::from_bytes(b"\xff"));
    name.push("name.txt");
    std::fs::write(dir.path().join(&name), "FIXME\n").unwrap();

    // The two formats that used to abort: assert they now exit 1 (violations
    // found, not a usage error) AND emit parseable JSON carrying the lossy path.
    for (fmt, ptr) in [
        ("json", &["results", "0", "violations", "0", "path"][..]),
        ("agent", &["violations", "0", "file"][..]),
    ] {
        let out = run(dir.path(), &["check", ".", "--format", fmt]);
        assert_eq!(
            out.status.code(),
            Some(1),
            "[{fmt}] a non-UTF-8 filename must not abort output; got exit {:?}, stderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
        let v: serde_json::Value =
            serde_json::from_slice(&out.stdout).expect("output must be valid JSON");
        // Walk the pointer to the path/file field and confirm the lossy render
        // preserved the ASCII tail (invalid byte → U+FFFD replacement).
        let mut cur = &v;
        for key in ptr {
            cur = key
                .parse::<usize>()
                .map_or_else(|_| &cur[key], |idx| &cur[idx]);
        }
        let rendered = cur.as_str().expect("path field is a JSON string");
        assert!(
            rendered.contains("name.txt") && rendered.contains('\u{fffd}'),
            "[{fmt}] expected a lossy path (…\u{fffd}…name.txt), got {rendered:?}"
        );
    }

    // And every other format still works too (they always did — a guard that
    // the fix didn't regress them).
    for fmt in ["sarif", "gitlab", "junit", "github", "markdown", "human"] {
        let out = run(dir.path(), &["check", ".", "--format", fmt]);
        assert_ne!(
            out.status.code(),
            Some(2),
            "[{fmt}] regressed on a non-UTF-8 filename; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
