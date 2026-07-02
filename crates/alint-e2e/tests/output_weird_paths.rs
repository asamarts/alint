//! Weird-path × output-format matrix.
//!
//! A repo file path is attacker-influenced text — anyone who can add a file
//! names it — and it flows verbatim into every renderer. Each of the eight
//! output formats has its own quoting rules: JSON string escapes, SARIF
//! percent-encoded URIs, XML attribute entities (`JUnit`), GitHub
//! `::`-annotation escapes, Markdown inline code. A path carrying that format's
//! metacharacters, a control byte, or (on Unix) a non-UTF-8 byte must never
//! corrupt the output or, worse, abort it.
//!
//! The matrix renders a one-violation report anchored on each weird path
//! through all eight formats and asserts:
//!   * the render **succeeds and is non-empty** — the regression guard for the
//!     `json` / `agent` formats, which used to *error* (`exit 2:
//!     "path contains invalid UTF-8 characters"`) on a non-UTF-8 path because
//!     they serialized `&Path` through serde's strict impl; and
//!   * the structured formats stay **well-formed** — every JSON-family output
//!     parses as JSON, the `JUnit` output parses as XML.
//!
//! Renderers are driven through the public [`alint_output::Format`] API on
//! synthetic reports, so the awkward cases (control bytes, non-UTF-8) are
//! exercised deterministically without depending on the filesystem. The
//! end-to-end path (real files → real binary) is covered by
//! `alint::tests::weird_path_formats_cli`.

use alint_core::{Level, Report, RuleResult, Violation};
use alint_output::Format;
use std::path::PathBuf;

const FORMATS: &[(&str, Format)] = &[
    ("human", Format::Human),
    ("json", Format::Json),
    ("sarif", Format::Sarif),
    ("github", Format::Github),
    ("markdown", Format::Markdown),
    ("junit", Format::Junit),
    ("gitlab", Format::Gitlab),
    ("agent", Format::Agent),
];

/// A one-violation error report anchored on `path`.
fn report_for(path: PathBuf) -> Report {
    let v = Violation::new("weird path finding")
        .with_path(path)
        .with_location(1, 1);
    let rr = RuleResult::new("weird-path".into(), Level::Error, None, vec![v], false);
    Report { results: vec![rr] }
}

/// Render `report` as `fmt`. The `expect` is itself an assertion: a renderer
/// that returns `Err` on any path is the bug this file guards against.
fn render(fmt: Format, report: &Report) -> Vec<u8> {
    let mut buf = Vec::new();
    fmt.write(report, &mut buf)
        .expect("a renderer must never error on a weird path");
    buf
}

/// Portable weird paths (all valid UTF-8), each exercising some format's
/// metacharacters or a control byte.
fn utf8_cases() -> Vec<(&'static str, PathBuf)> {
    vec![
        ("space", PathBuf::from("dir with space/my file.txt")),
        ("latin", PathBuf::from("café/résumé.md")),
        ("cjk", PathBuf::from("文書/日本語.md")),
        ("emoji", PathBuf::from("📁/🚀.txt")),
        ("dquote", PathBuf::from(r#"he said "hi".txt"#)),
        ("backslash", PathBuf::from(r"a\b\c.txt")),
        ("angle_amp", PathBuf::from("a<b>&c.txt")),
        ("pipe_tick", PathBuf::from("a|b`c.txt")),
        ("punct", PathBuf::from("a,b:c%d#e.txt")),
        // ESC + CSI clear-screen: a terminal-injection attempt via a filename.
        ("ctrl_esc", PathBuf::from("a\u{1b}[2Jb.txt")),
        ("newline", PathBuf::from("a\nb.txt")),
    ]
}

fn assert_valid_json(label: &str, fmt: &str, bytes: &[u8]) {
    if let Err(e) = serde_json::from_slice::<serde_json::Value>(bytes) {
        panic!(
            "[{fmt}/{label}] output is not valid JSON: {e}\n---\n{}\n---",
            String::from_utf8_lossy(bytes)
        );
    }
}

fn assert_valid_xml(label: &str, fmt: &str, bytes: &[u8]) {
    let text = std::str::from_utf8(bytes)
        .unwrap_or_else(|e| panic!("[{fmt}/{label}] XML output is not UTF-8: {e}"));
    if let Err(e) = roxmltree::Document::parse(text) {
        panic!("[{fmt}/{label}] output is not well-formed XML: {e}\n---\n{text}\n---");
    }
}

/// Structural validation appropriate to each format.
fn assert_wellformed(label: &str, fmt_name: &str, fmt: Format, bytes: &[u8]) {
    assert!(!bytes.is_empty(), "[{fmt_name}/{label}] empty output");
    match fmt {
        Format::Json | Format::Sarif | Format::Gitlab | Format::Agent => {
            assert_valid_json(label, fmt_name, bytes);
        }
        Format::Junit => assert_valid_xml(label, fmt_name, bytes),
        Format::Human | Format::Github | Format::Markdown => {
            // Text formats: must stay valid UTF-8 with no raw NUL — a control
            // byte from the path can't smuggle a bare NUL into the stream.
            let text = std::str::from_utf8(bytes)
                .unwrap_or_else(|e| panic!("[{fmt_name}/{label}] output is not UTF-8: {e}"));
            assert!(
                !text.contains('\u{0}'),
                "[{fmt_name}/{label}] output contains a raw NUL byte"
            );
        }
    }
}

#[test]
fn every_format_handles_utf8_weird_paths() {
    for (label, path) in utf8_cases() {
        let report = report_for(path);
        for (fmt_name, fmt) in FORMATS {
            let bytes = render(*fmt, &report);
            assert_wellformed(label, fmt_name, *fmt, &bytes);
        }
    }
}

#[cfg(unix)]
#[test]
fn every_format_handles_a_non_utf8_path() {
    use std::os::unix::ffi::OsStringExt;
    // A path with an invalid UTF-8 byte (0xFF) — legal on Unix. `json` / `agent`
    // used to abort the whole document here (serde's strict `&Path`); every
    // format must now render it lossily (invalid bytes → U+FFFD), exactly as
    // SARIF / GitLab / JUnit / GitHub always have.
    let path = PathBuf::from(std::ffi::OsString::from_vec(b"bad\xffname.txt".to_vec()));
    let report = report_for(path);
    for (fmt_name, fmt) in FORMATS {
        let bytes = render(*fmt, &report);
        assert_wellformed("non_utf8", fmt_name, *fmt, &bytes);
    }
}

#[test]
fn json_family_round_trips_a_utf8_weird_path() {
    // For a valid-UTF-8 path the lossy conversion is a no-op, so the JSON
    // string must decode back to the exact path — metacharacters and all.
    let path = "café/a b\"c`.txt";
    let report = report_for(PathBuf::from(path));

    let j: serde_json::Value = serde_json::from_slice(&render(Format::Json, &report)).unwrap();
    assert_eq!(j["results"][0]["violations"][0]["path"], path);

    let g: serde_json::Value = serde_json::from_slice(&render(Format::Gitlab, &report)).unwrap();
    assert_eq!(g[0]["location"]["path"], path);

    let a: serde_json::Value = serde_json::from_slice(&render(Format::Agent, &report)).unwrap();
    assert_eq!(a["violations"][0]["file"], path);
}
