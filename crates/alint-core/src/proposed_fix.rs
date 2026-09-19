//! Check-side computation of the concrete edits a fix would make, for the
//! machine formats (SARIF `result.fixes[]`) to render.
//!
//! `alint check` reports *where* a rule is unhappy; a fix-carrying format also
//! wants to say *what* the fix would change, as a source region plus its
//! replacement text, so a consumer (GitHub Code Scanning's "fix" suggestions)
//! can preview or apply it without running `alint fix`.
//!
//! [`attach_proposed_edits`] fills [`Violation::proposed_edits`](crate::Violation)
//! by re-running each fixable finding's fixer against the file's current bytes
//! (the same `collect_edits` / `fix_edit` the LSP and the fix pass use), then
//! mapping each resulting edit to a 1-based line/column [`EditRegion`]. It runs
//! only when a fix-carrying format asks (the CLI gates it on `--format sarif`),
//! so the ordinary check path pays nothing.
//!
//! Scope: **located** fixers (the structured `set_value` / `remove_value` /
//! `replace` ops) map their byte-range [`FixEdit::ReplaceRange`]s to 1:1 source
//! regions; **whole-file** normalizers ([`FixEdit::SetContent`]) become a
//! full-artifact replacement, and **create** fixers ([`FixEdit::CreateFile`]) an
//! insertion. File deletion / rename / chmod
//! ([`FixEdit::DeleteFile`] / [`FixEdit::RenameFile`] / [`FixEdit::SetMode`])
//! edit artifact *existence* / name / mode rather than content, which SARIF
//! `fix`es cannot express, so they carry no proposed edit.

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
/// and findings whose fixer has no content-replacement form (delete / rename /
/// chmod) untouched (empty).
///
/// A located fixer's `collect_edits` is *file-scoped* — it ignores the
/// individual violation and re-scans the whole file — and a whole-file fixer's
/// `fix_edit` likewise describes the whole file, so a fixer's edits belong to
/// the file, not one violation. When a rule fires more than once on a file
/// (unusual for these ops, which fire once per file), the edits attach to the
/// FIRST fixable violation for that `(rule, file)`; later ones stay empty so the
/// same fix is not rendered twice.
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
        // A located fixer (structured `set_value`/`remove_value`/`replace`) emits
        // byte-range `ReplaceRange`s via `collect_edits`; every other fixer is a
        // whole-file normalizer or a file-op and describes its change via
        // `fix_edit`. Both paths read nothing from disk beyond the file's current
        // bytes, so neither needs the fix pass's `FixContext`.
        let located = fixer.collects_located_edits();
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
            let bytes_opt = file_cache
                .entry(rel.clone())
                .or_insert_with(|| std::fs::read(root.join(&rel)).ok());
            // A missing file (`None`) is empty bytes: a located fixer finds nothing
            // to locate there, and a `CreateFile` fixer wants the empty state.
            let bytes: &[u8] = bytes_opt.as_deref().unwrap_or(&[]);
            let proposed: Vec<ProposedEdit> = if located {
                // Byte offsets map to line/col only over valid UTF-8. The structured
                // formats are UTF-8; a non-UTF-8 file yields no fix rather than a
                // wrong region.
                let Ok(text) = std::str::from_utf8(bytes) else {
                    continue;
                };
                fixer
                    .collect_edits(std::slice::from_ref(v), &rel, bytes, root)
                    .iter()
                    .filter_map(|ce| located_edit_to_proposed(&ce.edit, text))
                    .collect()
            } else {
                fixer
                    .fix_edit(v, bytes, root)
                    .and_then(|edit| whole_file_edit_to_proposed(&edit, bytes))
                    .into_iter()
                    .collect()
            };
            if !proposed.is_empty() {
                v.proposed_edits = proposed;
            }
        }
    }
}

/// Map a whole-file / file-op [`FixEdit`] (from a non-located fixer's
/// [`fix_edit`](crate::Fixer::fix_edit)) to a [`ProposedEdit`] against the
/// file's current `bytes`.
///
/// - [`FixEdit::SetContent`] replaces the entire artifact: the deleted region
///   spans from `(1,1)` to end-of-file, and `inserted` is the new content.
/// - [`FixEdit::CreateFile`] inserts a new artifact: an empty deleted region at
///   `(1,1)` with the file's content as `inserted`.
/// - [`FixEdit::ReplaceRange`] (unusual from `fix_edit`) reuses the located
///   mapping.
/// - [`FixEdit::DeleteFile`] / [`FixEdit::RenameFile`] / [`FixEdit::SetMode`]
///   have no SARIF content-replacement representation (SARIF `fix`es edit
///   artifact *content*, not existence, name, or mode), so they carry no fix.
///
/// Returns `None` for the unrepresentable variants and when content or the
/// existing bytes are not UTF-8 (a SARIF replacement is text).
fn whole_file_edit_to_proposed(edit: &FixEdit, bytes: &[u8]) -> Option<ProposedEdit> {
    match edit {
        FixEdit::SetContent { path, content } => {
            let inserted = String::from_utf8(content.clone()).ok()?;
            let text = std::str::from_utf8(bytes).ok()?;
            let (end_line, end_column) = byte_to_line_col(text, text.len());
            Some(ProposedEdit {
                path: path.clone(),
                region: Some(EditRegion {
                    start_line: 1,
                    start_column: 1,
                    end_line,
                    end_column,
                }),
                inserted,
            })
        }
        FixEdit::CreateFile { path, content } => {
            let inserted = String::from_utf8(content.clone()).ok()?;
            Some(ProposedEdit {
                path: path.clone(),
                region: Some(EditRegion {
                    start_line: 1,
                    start_column: 1,
                    end_line: 1,
                    end_column: 1,
                }),
                inserted,
            })
        }
        FixEdit::ReplaceRange { .. } => {
            let text = std::str::from_utf8(bytes).ok()?;
            located_edit_to_proposed(edit, text)
        }
        FixEdit::DeleteFile { .. } | FixEdit::RenameFile { .. } | FixEdit::SetMode { .. } => None,
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

    #[test]
    fn set_content_maps_to_a_full_artifact_region() {
        let bytes = b"old line 1\nold 2\n";
        let edit = FixEdit::SetContent {
            path: PathBuf::from("x.txt"),
            content: b"new\n".to_vec(),
        };
        let pe = whole_file_edit_to_proposed(&edit, bytes).unwrap();
        assert_eq!(pe.inserted, "new\n");
        let r = pe.region.unwrap();
        assert_eq!((r.start_line, r.start_column), (1, 1));
        // End is just past the final char of the two-line file: line 3, col 1.
        assert_eq!((r.end_line, r.end_column), (3, 1));
    }

    #[test]
    fn create_file_maps_to_an_empty_insertion_at_start() {
        let edit = FixEdit::CreateFile {
            path: PathBuf::from("new.txt"),
            content: b"hello\n".to_vec(),
        };
        // The file does not exist yet -> empty current bytes.
        let pe = whole_file_edit_to_proposed(&edit, b"").unwrap();
        assert_eq!(pe.inserted, "hello\n");
        let r = pe.region.unwrap();
        assert_eq!((r.start_line, r.start_column), (1, 1));
        assert_eq!((r.end_line, r.end_column), (1, 1)); // empty deleted region
    }

    #[test]
    fn delete_rename_chmod_have_no_content_fix() {
        let del = FixEdit::DeleteFile {
            path: PathBuf::from("x"),
        };
        let ren = FixEdit::RenameFile {
            from: PathBuf::from("a"),
            to: PathBuf::from("b"),
        };
        let chmod = FixEdit::SetMode {
            path: PathBuf::from("x"),
            mode: 0o755,
        };
        assert!(whole_file_edit_to_proposed(&del, b"x").is_none());
        assert!(whole_file_edit_to_proposed(&ren, b"x").is_none());
        assert!(whole_file_edit_to_proposed(&chmod, b"x").is_none());
    }
}
