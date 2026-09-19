//! Check-side computation of the concrete edits a fix would make, for the
//! machine formats (SARIF `result.fixes[]`) to render.
//!
//! `alint check` reports *where* a rule is unhappy; a fix-carrying format also
//! wants to say *what* the fix would change, as a source region plus its
//! replacement text, so a consumer (GitHub Code Scanning's "fix" suggestions)
//! can preview or apply it without running `alint fix`.
//!
//! [`attach_proposed_edits`] fills [`Violation::proposed_edits`](crate::Violation) by re-running
//! each fixable finding's fixer against the file's current bytes (the same
//! `collect_edits` the LSP and the fix pass use), then mapping each byte-range
//! edit to a 1-based line/column [`EditRegion`]. It runs only when a fix-carrying
//! format asks (the CLI gates it on `--format sarif`), so the ordinary check
//! path pays nothing.
//!
//! Scope today: **located** fixers (the structured `set_value` / `remove_value`
//! / `replace` ops), whose edits are byte-range [`FixEdit::ReplaceRange`]s with a
//! 1:1 source region. Whole-file normalizers and file-ops
//! (`SetContent` / `CreateFile` / `DeleteFile` / `RenameFile`) attach in a
//! follow-on increment.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::engine::Engine;
use crate::report::Report;
use crate::rule::FixEdit;

/// A 1-based, half-open source region `[start, end)` in (line, column).
///
/// Lines and columns are 1-based; a column counts Unicode scalar values
/// (`char`s) from the start of its line, matching SARIF's `startColumn` /
/// `endColumn` convention. An insertion is `start == end`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditRegion {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

/// A concrete edit a fix would make to resolve a violation, expressed as
/// location plus replacement text so a machine format can render it (SARIF
/// `result.fixes[].artifactChanges[].replacements[]`).
///
/// `path` is root-relative, matching [`Violation::path`](crate::Violation).
/// `region` is the replaced span (`None` means "replace the whole artifact" —
/// a whole-file rewrite, reserved for a later increment). `inserted` is the
/// replacement text (empty for a pure deletion).
#[derive(Debug, Clone, PartialEq)]
pub struct ProposedEdit {
    pub path: PathBuf,
    pub region: Option<EditRegion>,
    pub inserted: String,
}

/// Convert a byte offset in `text` (valid UTF-8) to a 1-based `(line, column)`,
/// where `column` counts Unicode scalar values from the start of the line.
///
/// A `'\n'` ends a line and resets the column; a lone `'\r'` is an ordinary
/// character. An offset at or past the end of `text` clamps to the position
/// just after the last character (so a replacement that reaches EOF gets a
/// sensible end region). This mirrors the LSP's `byte_offset_to_position`, but
/// 1-based and counting `char`s rather than 0-based UTF-16 code units.
#[must_use]
pub fn byte_to_line_col(text: &str, byte_offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut column = 1usize;
    for (idx, ch) in text.char_indices() {
        if idx >= byte_offset {
            return (line, column);
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

/// Map one located [`FixEdit::ReplaceRange`] to a [`ProposedEdit`] against
/// `text` (the file bytes the range indexes, as UTF-8). Returns `None` for any
/// other `FixEdit` variant (those are not 1:1 source-region edits — they attach
/// in a later increment) or when the replacement bytes are not UTF-8 (SARIF's
/// `insertedContent.text` is a string, so a binary replacement has no region
/// rendering).
fn located_edit_to_proposed(edit: &FixEdit, text: &str) -> Option<ProposedEdit> {
    let FixEdit::ReplaceRange {
        path,
        range,
        content,
    } = edit
    else {
        return None;
    };
    let inserted = String::from_utf8(content.clone()).ok()?;
    let (start_line, start_column) = byte_to_line_col(text, range.start);
    let (end_line, end_column) = byte_to_line_col(text, range.end);
    Some(ProposedEdit {
        path: path.clone(),
        region: Some(EditRegion {
            start_line,
            start_column,
            end_line,
            end_column,
        }),
        inserted,
    })
}

/// Attach the concrete fixes alint would make to every fixable finding in
/// `report`, reading files under `root`. Populates
/// [`Violation::proposed_edits`](crate::Violation); leaves non-fixable findings
/// and findings whose fixer is not located untouched (empty).
///
/// A located fixer's `collect_edits` is *file-scoped* — it ignores the
/// individual violation and re-scans the whole file — so its edits belong to
/// the file, not one violation. When a rule fires more than once on a file
/// (unusual for the structured ops, which fire once per file), the edits attach
/// to the FIRST fixable violation for that `(rule, file)`; later ones stay empty
/// so the same fix is not rendered twice.
///
/// Only fixable findings are considered (matching what `check` promises as
/// fixable and what a bare `alint fix` would apply), and each file is read at
/// most once.
pub fn attach_proposed_edits(engine: &Engine, report: &mut Report, root: &Path) {
    let mut file_cache: HashMap<PathBuf, Option<Vec<u8>>> = HashMap::new();
    let mut attached: HashSet<(String, PathBuf)> = HashSet::new();

    for rr in &mut report.results {
        let Some(fixer) = engine.fixer_for(&rr.rule_id) else {
            continue;
        };
        // Located fixers only: their edits are byte-range `ReplaceRange`s with a
        // 1:1 source region. Whole-file / file-op fixers attach later.
        if !fixer.collects_located_edits() {
            continue;
        }
        for v in &mut rr.violations {
            if !v.is_fixable {
                continue;
            }
            let Some(path_arc) = v.path.clone() else {
                continue;
            };
            let rel: PathBuf = path_arc.as_ref().to_path_buf();
            if !attached.insert((rr.rule_id.to_string(), rel.clone())) {
                continue; // this file's edits already attached to an earlier finding
            }
            let bytes = file_cache
                .entry(rel.clone())
                .or_insert_with(|| std::fs::read(root.join(&rel)).ok());
            let Some(bytes) = bytes.as_deref() else {
                continue;
            };
            // Byte offsets map to line/col only over valid UTF-8. The structured
            // formats are UTF-8; a non-UTF-8 file yields no fix rather than a
            // wrong region.
            let Ok(text) = std::str::from_utf8(bytes) else {
                continue;
            };
            let edits = fixer.collect_edits(std::slice::from_ref(v), &rel, bytes, root);
            let proposed: Vec<ProposedEdit> = edits
                .iter()
                .filter_map(|ce| located_edit_to_proposed(&ce.edit, text))
                .collect();
            if !proposed.is_empty() {
                v.proposed_edits = proposed;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_to_line_col_is_one_based_from_start() {
        assert_eq!(byte_to_line_col("abc", 0), (1, 1));
        assert_eq!(byte_to_line_col("abc", 1), (1, 2));
        assert_eq!(byte_to_line_col("abc", 3), (1, 4)); // just past the last char
    }

    #[test]
    fn byte_to_line_col_advances_lines_on_newline() {
        let t = "ab\ncd\nef";
        assert_eq!(byte_to_line_col(t, 0), (1, 1)); // 'a'
        assert_eq!(byte_to_line_col(t, 2), (1, 3)); // the '\n' itself
        assert_eq!(byte_to_line_col(t, 3), (2, 1)); // 'c' starts line 2
        assert_eq!(byte_to_line_col(t, 6), (3, 1)); // 'e' starts line 3
    }

    #[test]
    fn byte_to_line_col_counts_scalars_not_bytes() {
        // "é" is two UTF-8 bytes but one column; the char after it is column 2.
        let t = "é=1";
        assert_eq!(byte_to_line_col(t, 0), (1, 1)); // 'é'
        assert_eq!(byte_to_line_col(t, 2), (1, 2)); // '=' is one column past 'é'
        assert_eq!(byte_to_line_col(t, 3), (1, 3)); // '1'
    }

    #[test]
    fn byte_to_line_col_clamps_past_end() {
        assert_eq!(byte_to_line_col("ab", 999), (1, 3));
        assert_eq!(byte_to_line_col("a\n", 999), (2, 1));
    }

    #[test]
    fn located_edit_maps_replace_range_to_a_region() {
        let text = "{\n  \"a\": 1\n}\n";
        // Replace the `1` on line 2 (byte offset of '1' is after `  "a": `).
        let one = text.find('1').unwrap();
        let edit = FixEdit::ReplaceRange {
            path: PathBuf::from("app.json"),
            range: one..one + 1,
            content: b"2".to_vec(),
        };
        let pe = located_edit_to_proposed(&edit, text).unwrap();
        assert_eq!(pe.path, PathBuf::from("app.json"));
        assert_eq!(pe.inserted, "2");
        let r = pe.region.unwrap();
        assert_eq!((r.start_line, r.start_column), (2, 8));
        assert_eq!((r.end_line, r.end_column), (2, 9));
    }

    #[test]
    fn located_edit_declines_non_range_and_non_utf8() {
        // A whole-file SetContent is not a 1:1 region edit here.
        let set = FixEdit::SetContent {
            path: PathBuf::from("x"),
            content: b"hi".to_vec(),
        };
        assert!(located_edit_to_proposed(&set, "hi").is_none());
        // A non-UTF-8 replacement has no text rendering.
        let bad = FixEdit::ReplaceRange {
            path: PathBuf::from("x"),
            range: 0..1,
            content: vec![0xff, 0xfe],
        };
        assert!(located_edit_to_proposed(&bad, "abc").is_none());
    }
}
