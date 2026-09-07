//! Coverage audit: alint's own emitted output carries no em-dash (U+2014).
//!
//! alint dogfoods em-dash-free prose, so its CLI output (violation and error
//! messages, the fix and agent reports, `--help` text) and the generated
//! config-schema descriptions must be em-dash-free too. The output-hygiene
//! PRs swept these surfaces by hand; without a gate, the next un-visited
//! surface silently keeps its em-dashes and CI stays green (the committed
//! snapshots pin the emitted bytes as "expected", so a stale em-dash is
//! invisible until someone re-reads the file).
//!
//! This test scans the two testable output artifacts:
//!   * every committed `trycmd` snapshot (`crates/alint/tests/cli/*.stdout`
//!     and `*.stderr`), which capture alint's stdout/stderr byte-for-byte;
//!   * both committed schema copies (`schemas/v1/config.json` and the
//!     in-crate `crates/alint-dsl/schemas/v1/config.json`), whose
//!     `description` strings derive from the rule-option doc-comments.
//!
//! Out of scope (kept as-is): code comments, rustdoc on non-schema items,
//! test-only strings, `docs/rules.md`, the CHANGELOG, and intentional
//! non-em-dash glyphs such as truncation ellipses.

use std::fs;
use std::path::PathBuf;

/// U+2014 EM DASH.
const EM_DASH: char = '\u{2014}';

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR for this test = crates/alint-e2e.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/alint-e2e has a parent")
        .parent()
        .expect("crates has a parent (workspace root)")
        .to_path_buf()
}

#[test]
fn cli_snapshots_have_no_em_dash() {
    let dir = workspace_root().join("crates/alint/tests/cli");
    let mut offenders = Vec::new();
    for entry in fs::read_dir(&dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display())) {
        let path = entry.expect("read dir entry").path();
        match path.extension().and_then(|e| e.to_str()) {
            Some("stdout" | "stderr") => {}
            _ => continue,
        }
        let text =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        for (i, line) in text.lines().enumerate() {
            if line.contains(EM_DASH) {
                offenders.push(format!("{name}:{}: {line}", i + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "alint's emitted output (trycmd snapshots) must not contain an em-dash \
         (U+2014); use an ASCII hyphen, comma, or period. Offending lines:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn config_schema_descriptions_have_no_em_dash() {
    for rel in [
        "schemas/v1/config.json",
        "crates/alint-dsl/schemas/v1/config.json",
    ] {
        let path = workspace_root().join(rel);
        let text =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let count = text.matches(EM_DASH).count();
        assert_eq!(
            count, 0,
            "{rel} must have no em-dash (U+2014) in its schema descriptions \
             (they derive from JsonSchema doc-comments); found {count}. \
             Sweep the source `///` docs and run `cargo run -p xtask -- gen-schema`.",
        );
    }
}
