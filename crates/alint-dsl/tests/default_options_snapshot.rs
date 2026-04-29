//! Default-option snapshot: builds every rule from the canonical
//! `all_kinds.yaml` fixture, captures each one's Debug output,
//! and diffs against `snapshots/default_options.txt`.
//!
//! The Debug print of a built rule includes every option after
//! serde defaults have filled in — so a silent change to any
//! `#[serde(default = "...")]` value (e.g. shifting
//! `commented_out_code::min_lines` from 3 to 2) shows up here as
//! a snapshot delta. Run with `UPDATE_SNAPSHOTS=1` to refresh
//! after an intentional change.
//!
//! Pairs with `schema::fixture_covers_every_registered_rule_kind`:
//! that test guarantees every kind appears in the fixture; this
//! one freezes their default-resolved shape.

use std::fmt::Write as _;
use std::path::PathBuf;

const FIXTURE: &str = include_str!("fixtures/all_kinds.yaml");

fn snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("default_options.txt")
}

/// Collapse verbose nested-struct blocks (`Scope { ... }`,
/// `GlobSet { ... }`, regex-engine internals) into a one-line
/// placeholder so the snapshot tracks user-meaningful option
/// values, not unrelated crate-internal Debug churn.
///
/// Walks the multi-line Debug output and, whenever a line ends
/// in `Scope {` or `GlobSet {` (after stripping the field-name
/// prefix), elides everything up to the matching `}` at the
/// same indent level.
fn elide_verbose_blocks(input: &str) -> String {
    let lines: Vec<&str> = input.lines().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_end();
        let is_noisy = trimmed.ends_with("Scope {") || trimmed.ends_with("GlobSet {");
        if is_noisy {
            let indent = line.len() - line.trim_start().len();
            let prefix = &line[..line.rfind('{').unwrap()];
            out.push_str(prefix);
            out.push_str("{ ... },\n");
            i += 1;
            while i < lines.len() {
                let close = lines[i];
                let close_indent = close.len() - close.trim_start().len();
                if close_indent == indent && close.trim_start().starts_with('}') {
                    i += 1;
                    break;
                }
                i += 1;
            }
        } else {
            out.push_str(line);
            out.push('\n');
            i += 1;
        }
    }
    out
}

fn render_snapshot() -> String {
    let config = alint_dsl::parse(FIXTURE).expect("fixture should parse");
    let registry = alint_rules::builtin_registry();

    let mut entries: Vec<(String, String, String)> = config
        .rules
        .iter()
        .map(|spec| {
            let rule = registry
                .build(spec)
                .expect("fixture rule should build under default registry");
            let dbg = elide_verbose_blocks(&format!("{rule:#?}"));
            (spec.kind.clone(), spec.id.clone(), dbg)
        })
        .collect();

    // Sort by (kind, id) so insertion order in the fixture
    // doesn't drive snapshot churn.
    entries.sort_by(|a, b| (a.0.as_str(), a.1.as_str()).cmp(&(b.0.as_str(), b.1.as_str())));

    let mut out = String::new();
    out.push_str("# Default-option snapshot — see default_options_snapshot.rs\n\n");
    for (kind, id, dbg) in &entries {
        writeln!(out, "=== kind: {kind} | id: {id} ===\n{dbg}").unwrap();
    }
    out
}

#[test]
fn default_options_snapshot_matches() {
    let actual = render_snapshot();
    let path = snapshot_path();

    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).expect("write snapshot");
        eprintln!("wrote {}", path.display());
        return;
    }

    // Git's `core.autocrlf` may convert the checked-in LF
    // snapshot to CRLF on Windows checkout; the in-memory
    // `actual` always uses `\n`. Normalise so the comparison
    // measures content drift, not line-ending drift.
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");

    if expected != actual {
        let diff = first_diverging_window(&expected, &actual, 6);
        panic!(
            "Default-option snapshot drift detected.\n\
             Expected file: {}\n\
             Run with UPDATE_SNAPSHOTS=1 to refresh after an intentional change.\n\n\
             First diverging window (line {}):\n{diff}",
            path.display(),
            diff_line_number(&expected, &actual),
        );
    }
}

/// Find the first line where `expected` and `actual` differ
/// and emit `context` lines on each side as a unified-style
/// snippet. Surfaces the platform-specific delta directly in
/// the panic output so CI logs are diagnostic without
/// downloading artifacts.
fn first_diverging_window(expected: &str, actual: &str, context: usize) -> String {
    let exp_lines: Vec<&str> = expected.lines().collect();
    let act_lines: Vec<&str> = actual.lines().collect();
    let max_len = exp_lines.len().max(act_lines.len());
    let mut first_diff = max_len;
    for i in 0..max_len {
        let e = exp_lines.get(i).copied().unwrap_or("");
        let a = act_lines.get(i).copied().unwrap_or("");
        if e != a {
            first_diff = i;
            break;
        }
    }
    let start = first_diff.saturating_sub(context);
    let end = (first_diff + context + 1).min(max_len);
    let mut out = String::new();
    for i in start..end {
        let e = exp_lines.get(i).copied().unwrap_or("<EOF>");
        let a = act_lines.get(i).copied().unwrap_or("<EOF>");
        if e == a {
            writeln!(out, "  {i:>5} | {e}").unwrap();
        } else {
            writeln!(out, "- {i:>5} | {e}").unwrap();
            writeln!(out, "+ {i:>5} | {a}").unwrap();
        }
    }
    out
}

fn diff_line_number(expected: &str, actual: &str) -> usize {
    expected
        .lines()
        .zip(actual.lines())
        .position(|(e, a)| e != a)
        .unwrap_or(0)
}
