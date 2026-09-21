//! Check-side computation of the concrete edits a fix would make, for the
//! machine formats (SARIF `result.fixes[]`) to render.
//!
//! `alint check` reports *where* a rule is unhappy; a fix-carrying format also
//! wants to say *what* the fix would change, as a source region plus its
//! replacement text, so a SARIF consumer (an editor's SARIF viewer, or
//! `sarif-multitool`) can preview or apply it without running `alint fix`.
//! (GitHub Code Scanning currently *ignores* `result.fixes` — its autofix is
//! Copilot-based — so this targets other SARIF consumers.)
//!
//! **Fidelity is the invariant.** The advertised fix MUST equal what `alint
//! fix` actually writes. So [`attach_proposed_edits`] does NOT trust the raw
//! output of `collect_edits`/`fix_edit`; it runs edits through the SAME pipeline
//! the fix pass uses ([`located_fix::apply_file_edits`] at the `Safe` threshold,
//! plus the `fix_size_limit` guard) and advertises only the edits that
//! **survive** it — tier-filtered, overlap-skipped, and post-splice
//! **verified/demoted**. An edit the fix pass would demote (e.g. a
//! `remove_value` batch that can only partially remove, which alint refuses
//! all-or-nothing) or overlap-skip is therefore NOT advertised, so a consumer
//! can never apply a change alint itself declines.
//!
//! Scope: **located** fixers (`set_value` / `remove_value` / `replace`) map
//! their surviving byte-range [`FixEdit::ReplaceRange`]s to source regions;
//! **whole-file** normalizers ([`FixEdit::SetContent`]) become a *minimal*
//! changed span (so independent same-file fixes compose, as the located ones
//! do); **create** fixers ([`FixEdit::CreateFile`]) an insertion. File deletion
//! / rename / chmod ([`FixEdit::DeleteFile`] / [`FixEdit::RenameFile`] /
//! [`FixEdit::SetMode`]) edit artifact *existence* / name / mode, which SARIF
//! `fix`es cannot express, so they carry no proposed edit.
//!
//! Runs only when a fix-carrying format asks (the CLI gates it on `--format
//! sarif`), so the ordinary check path pays nothing.

use std::collections::{BTreeMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::engine::Engine;
use crate::located_fix::{self, LocatedEdit, LocatedOutcome};
use crate::report::Report;
use crate::rule::{Applicability, FixEdit, Violation};
use crate::walker::read_capped_or_skip;

/// A 1-based source region `[start, end)` following SARIF's convention:
/// `start_line`/`start_column` are the first character of the region;
/// `end_line` is the line of the region's **last** character (SARIF §3.30.7,
/// inclusive) and `end_column` is one past that character (SARIF §3.30.8,
/// exclusive). Columns count Unicode scalar values from the start of the line
/// (matching alint's other column output). An insertion is `start == end`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
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
/// `inserted` is the replacement text (empty for a pure deletion).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ProposedEdit {
    pub path: PathBuf,
    pub region: EditRegion,
    pub inserted: String,
}

/// Convert a byte offset in `text` (valid UTF-8) to a 1-based `(line, column)`,
/// where `column` counts Unicode scalar values from the start of the line.
///
/// A `'\n'` ends a line and resets the column; a lone `'\r'` is an ordinary
/// character. A leading UTF-8 BOM (U+FEFF at offset 0) is an encoding signature,
/// not content, so it does NOT occupy a column — the first real character is
/// column 1 (matching the common consumer convention; without this a line-1 edit
/// in a BOM-prefixed file is off by one). An offset at or past the end clamps to
/// the position just after the last character.
#[must_use]
pub fn byte_to_line_col(text: &str, byte_offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut column = 1usize;
    for (idx, ch) in text.char_indices() {
        if idx >= byte_offset {
            return (line, column);
        }
        if idx == 0 && ch == '\u{feff}' {
            continue; // leading BOM: not a column
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

/// The SARIF `(end_line, end_column)` for a NON-empty region ending at byte
/// `end` in `text`. `end_column` is exclusive (the column following the last
/// char); `end_line` is the line of the last char — so when the last char is a
/// `'\n'`, the line stays on that newline's line (SARIF §3.30.7) rather than
/// rolling to the next, and the column is one past the newline on that line.
fn sarif_end(text: &str, end: usize) -> (usize, usize) {
    if text[..end].ends_with('\n') {
        // The '\n' is one byte, at `end - 1`; endLine is its line, endColumn one past it.
        let (l, c) = byte_to_line_col(text, end - 1);
        (l, c + 1)
    } else {
        byte_to_line_col(text, end)
    }
}

/// The [`EditRegion`] for a (char-aligned) byte range in `text`.
fn region_of(text: &str, range: &Range<usize>) -> EditRegion {
    let (start_line, start_column) = byte_to_line_col(text, range.start);
    let (end_line, end_column) = if range.start == range.end {
        (start_line, start_column) // empty region (pure insertion)
    } else {
        sarif_end(text, range.end)
    };
    EditRegion {
        start_line,
        start_column,
        end_line,
        end_column,
    }
}

/// Largest char boundary `<= i` in `bytes`.
fn floor_char_boundary(bytes: &[u8], mut i: usize) -> usize {
    if i >= bytes.len() {
        return bytes.len();
    }
    while i > 0 && bytes[i] & 0xC0 == 0x80 {
        i -= 1;
    }
    i
}

/// Smallest char boundary `>= i` in `bytes`.
fn ceil_char_boundary(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i] & 0xC0 == 0x80 {
        i += 1;
    }
    i
}

/// Re-express a byte-range edit (`replace orig[range] with content`) on a
/// **char-aligned** range with valid-UTF-8 replacement text, so it can render as
/// a SARIF text replacement even when the fixer's byte diff split a multi-byte
/// character (the whole-doc `minimal_replace` path can). Snaps the range outward
/// to char boundaries and folds the straddled bytes back into the content; the
/// result reproduces the same edited bytes. Returns `None` if the result is not
/// UTF-8 (a binary replacement has no text region).
fn char_align(orig: &[u8], range: Range<usize>, content: &[u8]) -> Option<(Range<usize>, String)> {
    let start = floor_char_boundary(orig, range.start);
    let end = ceil_char_boundary(orig, range.end);
    let mut buf = Vec::with_capacity((range.start - start) + content.len() + (end - range.end));
    buf.extend_from_slice(&orig[start..range.start]);
    buf.extend_from_slice(content);
    buf.extend_from_slice(&orig[range.end..end]);
    let inserted = String::from_utf8(buf).ok()?;
    Some((start..end, inserted))
}

/// Map one surviving located [`FixEdit::ReplaceRange`] (against the file's
/// `orig` bytes / `text`) to a [`ProposedEdit`].
fn located_edit_to_proposed(orig: &[u8], text: &str, edit: &FixEdit) -> Option<ProposedEdit> {
    let FixEdit::ReplaceRange {
        path,
        range,
        content,
    } = edit
    else {
        return None;
    };
    let (range, inserted) = char_align(orig, range.clone(), content)?;
    Some(ProposedEdit {
        path: path.clone(),
        region: region_of(text, &range),
        inserted,
    })
}

/// Map a whole-file / create [`FixEdit`] (from a non-located fixer's `fix_edit`)
/// to a [`ProposedEdit`] against the file's current `orig` bytes.
///
/// `SetContent` becomes a *minimal* changed span (via `minimal_replace`, then
/// char-aligned) so two normalizers on one file yield disjoint spans that
/// compose — a full-artifact rewrite would clobber. `CreateFile` becomes an
/// empty insertion at `(1,1)`. Delete / rename / chmod have no
/// content-replacement form and return `None`.
fn whole_file_edit_to_proposed(orig: &[u8], edit: &FixEdit) -> Option<ProposedEdit> {
    match edit {
        FixEdit::SetContent { path, content } => {
            let (byte_range, byte_content) = crate::structured_fix::minimal_replace(orig, content);
            if byte_range.is_empty() && byte_content.is_empty() {
                return None; // re-serialized byte-identical: no-op
            }
            let text = std::str::from_utf8(orig).ok()?;
            let (range, inserted) = char_align(orig, byte_range, &byte_content)?;
            let region = region_of(text, &range);
            // A change with no LINE/COLUMN extent and no inserted text is
            // inexpressible as a text region and applies to nothing -- notably a
            // leading-BOM strip, whose deleted bytes both map to (1,1) because
            // `byte_to_line_col` skips the BOM. Omit it rather than advertise an
            // empty edit that tells a consumer "apply this" when applying is a
            // no-op (audit MEDIUM, 2026-09-20). `alint fix` still strips it.
            if region.start_line == region.end_line
                && region.start_column == region.end_column
                && inserted.is_empty()
            {
                return None;
            }
            Some(ProposedEdit {
                path: path.clone(),
                region,
                inserted,
            })
        }
        FixEdit::CreateFile { path, content } => {
            let inserted = String::from_utf8(content.clone()).ok()?;
            Some(ProposedEdit {
                path: path.clone(),
                region: EditRegion {
                    start_line: 1,
                    start_column: 1,
                    end_line: 1,
                    end_column: 1,
                },
                inserted,
            })
        }
        FixEdit::ReplaceRange { .. } => {
            let text = std::str::from_utf8(orig).ok()?;
            located_edit_to_proposed(orig, text, edit)
        }
        FixEdit::DeleteFile { .. } | FixEdit::RenameFile { .. } | FixEdit::SetMode { .. } => None,
    }
}

/// Read a file for proposed-edit computation, applying the SAME guards the fix
/// pass does: skip a non-regular file or one over `fix_limit` (so a fix a bare
/// `alint fix` would skip is not advertised), and bound the read. `None` = no
/// fix.
fn read_fixable_file(abs: &Path, fix_limit: Option<u64>) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(abs).ok()?;
    if !meta.is_file() {
        return None; // refuse a FIFO / dir / symlink-to-special (TOCTOU vs the walk)
    }
    if fix_limit.is_some_and(|limit| meta.len() > limit) {
        return None; // over fix_size_limit: `alint fix` skips it, so advertise nothing
    }
    read_capped_or_skip(abs, meta.len())
}

/// Attach the concrete fixes alint would make to every fixable finding in
/// `report`, reading files under `root`. Populates
/// [`Violation::proposed_edits`](crate::Violation); leaves untouched (empty) any
/// non-fixable finding, any finding whose surviving-edit set is empty (the fix
/// pass would demote/skip it), and any fixer with no content-replacement form
/// (delete / rename / chmod).
///
/// Edits are derived PER FILE, exactly as `alint fix` composes them, so the
/// surfaces advertise what `fix` would write and nothing it would skip:
/// `located_proposed_edits` batches every located rule's edits for a file into
/// one overlap-deconflicting + verifying pass (cross-rule overlaps are dropped,
/// as `fix` drops them), and `whole_file_proposed_edits` threads the bytes
/// through a file's whole-file normalizers in config order (each sees the
/// previous one's output). Each surviving/incremental edit attaches to the first
/// fixable violation of the rule that produced it, for that file; later ones stay
/// empty so the same fix is not rendered twice. (A rare file touched by BOTH a
/// located and a whole-file fixer has their mutual composition unmodeled here;
/// `fix` still composes it.)
// NOTE (`--changed`): the fix pass demotes an edit that would write OUTSIDE the
// changed set (`writes_outside_changed`); this routine does not consult it.
// Reachable divergence needs a Safe, path-bearing, full-index fixer, of which
// none ship today (per-file rules are confined to the changed set, so their fix
// target is always in scope; the full-index fixers are `file_exists` (path-less
// create) and the Unsafe `file_remove`). If such a fixer is added, thread the
// changed set through here so SARIF does not advertise a fix `fix --changed`
// would decline.
pub fn attach_proposed_edits(engine: &Engine, report: &mut Report, root: &Path) {
    let fix_limit = engine.fix_size_limit();

    // Advertised edits keyed by the rule result they belong to. A path-bearing
    // finding keys on (result index, file); a create fixer's finding carries no
    // path, so it keys on the result index alone.
    let mut by_rule_file: BTreeMap<(usize, PathBuf), Vec<ProposedEdit>> = BTreeMap::new();
    let mut by_create_rule: BTreeMap<usize, Vec<ProposedEdit>> = BTreeMap::new();

    located_proposed_edits(engine, report, root, fix_limit, &mut by_rule_file);
    whole_file_proposed_edits(
        engine,
        report,
        root,
        fix_limit,
        &mut by_rule_file,
        &mut by_create_rule,
    );

    // Attach each rule's edits to its FIRST fixable violation for the file (a
    // create rule's to its first pathless fixable violation); later findings for
    // the same (rule, file) stay empty so the same fix is not rendered twice.
    for (ri, rr) in report.results.iter_mut().enumerate() {
        let mut done: HashSet<PathBuf> = HashSet::new();
        let mut create_done = false;
        for v in &mut rr.violations {
            if !v.is_fixable {
                continue;
            }
            if let Some(p) = v.path.as_deref() {
                let rel = p.to_path_buf();
                if done.contains(&rel) {
                    continue;
                }
                if let Some(pes) = by_rule_file.get(&(ri, rel.clone())) {
                    v.proposed_edits.clone_from(pes);
                    done.insert(rel);
                }
            } else if !create_done {
                if let Some(pes) = by_create_rule.get(&ri) {
                    v.proposed_edits.clone_from(pes);
                    create_done = true;
                }
            }
        }
    }
}

/// Located fixers, batched ACROSS rules per file. Gather every located rule's
/// edits for a file into ONE batch (each tagged with its rule's result index) and
/// run the SAME per-file overlap-deconflict + verify the fix pass uses, so two
/// located rules whose spans overlap never advertise edits `alint fix` would skip
/// (audit CRITICAL, 2026-09-20). Each surviving edit is attributed back to the
/// rule that produced it. The file is read ONCE so every rule's byte offsets index
/// the same buffer, exactly as the fix pass's per-file batch does.
fn located_proposed_edits(
    engine: &Engine,
    report: &Report,
    root: &Path,
    fix_limit: Option<u64>,
    out: &mut BTreeMap<(usize, PathBuf), Vec<ProposedEdit>>,
) {
    let mut files: BTreeMap<PathBuf, Vec<u8>> = BTreeMap::new();
    let mut batches: BTreeMap<PathBuf, Vec<LocatedEdit>> = BTreeMap::new();
    for (ri, rr) in report.results.iter().enumerate() {
        let Some(fixer) = engine.fixer_for(&rr.rule_id) else {
            continue;
        };
        if !fixer.collects_located_edits() {
            continue;
        }
        let mut per_file: BTreeMap<PathBuf, Vec<Violation>> = BTreeMap::new();
        for v in rr.violations.iter().filter(|v| v.is_fixable) {
            if let Some(p) = v.path.as_deref() {
                per_file.entry(p.to_path_buf()).or_default().push(v.clone());
            }
        }
        for (rel, vs) in per_file {
            if !files.contains_key(&rel) {
                let Some(b) = read_fixable_file(&root.join(&rel), fix_limit) else {
                    continue; // over-limit / unreadable: `fix` skips it too
                };
                files.insert(rel.clone(), b);
            }
            let edits = fixer.collect_edits(&vs, &rel, &files[&rel], root);
            let batch = batches.entry(rel).or_default();
            for ce in edits {
                let idx = batch.len();
                batch.push(LocatedEdit {
                    rule_index: ri,
                    violation_index: idx,
                    collected: ce,
                });
            }
        }
    }
    for (rel, batch) in batches {
        let bytes = &files[&rel];
        let Ok(text) = std::str::from_utf8(bytes) else {
            continue; // byte offsets map to line/col only over valid UTF-8
        };
        let (_, outcomes) = located_fix::apply_file_edits(bytes, batch, Applicability::Safe);
        for (le, outcome) in outcomes {
            if outcome != LocatedOutcome::Applied {
                continue;
            }
            if let Some(pe) = located_edit_to_proposed(bytes, text, &le.collected.edit) {
                out.entry((le.rule_index, rel.clone()))
                    .or_default()
                    .push(pe);
            }
        }
    }
}

/// Whole-file fixers, composed per file. Thread the file bytes through each
/// whole-file NORMALIZER in result (config) order so a fixer sees the previous
/// one's output -- as the fix pass's compose buffer does -- and advertise each
/// fixer's INCREMENTAL change; a fixer whose finding an earlier one already
/// resolved advertises nothing (audit HIGH, 2026-09-20). CREATE fixers (a
/// violation with no path -- the file does not exist yet) are standalone
/// insertions, keyed by result index.
///
/// A file touched by BOTH a located and a whole-file fixer is a rare edge these
/// two passes derive independently (each against the original bytes), so their
/// mutual composition is not modeled here; `alint fix` still composes them.
fn whole_file_proposed_edits(
    engine: &Engine,
    report: &Report,
    root: &Path,
    fix_limit: Option<u64>,
    out: &mut BTreeMap<(usize, PathBuf), Vec<ProposedEdit>>,
    creates_out: &mut BTreeMap<usize, Vec<ProposedEdit>>,
) {
    let mut per_file: BTreeMap<PathBuf, Vec<(usize, Violation)>> = BTreeMap::new();
    let mut creates: Vec<(usize, Violation)> = Vec::new();
    for (ri, rr) in report.results.iter().enumerate() {
        let Some(fixer) = engine.fixer_for(&rr.rule_id) else {
            continue;
        };
        if fixer.collects_located_edits() {
            continue;
        }
        for v in rr.violations.iter().filter(|v| v.is_fixable) {
            match v.path.as_deref() {
                Some(p) => per_file
                    .entry(p.to_path_buf())
                    .or_default()
                    .push((ri, v.clone())),
                None => creates.push((ri, v.clone())),
            }
        }
    }
    for (rel, items) in per_file {
        let Some(mut cur) = read_fixable_file(&root.join(&rel), fix_limit) else {
            continue;
        };
        for (ri, v) in items {
            let Some(fixer) = engine.fixer_for(&report.results[ri].rule_id) else {
                continue;
            };
            let Some(edit) = fixer.fix_edit(&v, &cur, root) else {
                continue;
            };
            if let Some(pe) = whole_file_edit_to_proposed(&cur, &edit) {
                out.entry((ri, rel.clone())).or_default().push(pe);
            }
            // Advance the composed bytes so the next whole-file fixer sees this
            // one's output (the compose-buffer fixpoint the fix pass runs).
            if let FixEdit::SetContent { content, .. } = &edit {
                cur.clone_from(content);
            }
        }
    }
    for (ri, v) in creates {
        let Some(fixer) = engine.fixer_for(&report.results[ri].rule_id) else {
            continue;
        };
        let Some(edit) = fixer.fix_edit(&v, &[], root) else {
            continue;
        };
        if let Some(pe) = whole_file_edit_to_proposed(&[], &edit) {
            creates_out.entry(ri).or_default().push(pe);
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
        assert_eq!(byte_to_line_col(t, 2), (1, 3)); // the '\n'
        assert_eq!(byte_to_line_col(t, 3), (2, 1)); // 'c' starts line 2
        assert_eq!(byte_to_line_col(t, 6), (3, 1)); // 'e' starts line 3
    }

    #[test]
    fn byte_to_line_col_counts_scalars_not_bytes() {
        let t = "é=1"; // 'é' is two bytes but one column
        assert_eq!(byte_to_line_col(t, 0), (1, 1)); // 'é'
        assert_eq!(byte_to_line_col(t, 2), (1, 2)); // '=' one column past 'é'
        assert_eq!(byte_to_line_col(t, 3), (1, 3)); // '1'
    }

    #[test]
    fn byte_to_line_col_skips_a_leading_bom() {
        // BOM (3 bytes) + `{`. The `{` at byte 3 is column 1, not column 2.
        let t = "\u{feff}{}";
        assert_eq!(byte_to_line_col(t, 3), (1, 1)); // '{'
        assert_eq!(byte_to_line_col(t, 4), (1, 2)); // '}'
    }

    #[test]
    fn sarif_end_stays_on_the_last_chars_line_at_a_newline() {
        // Region ending exactly after a '\n' keeps endLine on that newline's line
        // (SARIF §3.30.7), not the next line.
        let t = "abc\ndef\n";
        // region [0,4) covers "abc\n"; last char '\n' is on line 1, col 4.
        assert_eq!(sarif_end(t, 4), (1, 5));
        // region ending mid-line (not at '\n'): the following-char position.
        assert_eq!(sarif_end(t, 6), (2, 3)); // after "de" on line 2
    }

    #[test]
    fn region_of_handles_insertion_and_span() {
        let t = "abc\ndef\n";
        assert_eq!(
            region_of(t, &(4..4)),
            EditRegion {
                start_line: 2,
                start_column: 1,
                end_line: 2,
                end_column: 1
            }
        ); // empty region = insertion at line 2 col 1
        assert_eq!(
            region_of(t, &(0..3)),
            EditRegion {
                start_line: 1,
                start_column: 1,
                end_line: 1,
                end_column: 4
            }
        ); // "abc"
    }

    #[test]
    fn char_align_passes_through_aligned_edits() {
        let orig = b"hello world";
        let (r, s) = char_align(orig, 6..11, b"there").unwrap();
        assert_eq!(r, 6..11);
        assert_eq!(s, "there");
    }

    #[test]
    fn char_align_snaps_a_mid_char_byte_diff() {
        // "aé" -> "aè": é=C3 A9, è=C3 A8 share the leading C3, so a byte diff
        // yields range [2,3) (the A9 byte) with content [A8] — invalid alone.
        // char_align snaps to the whole 'é' char and yields valid text.
        let orig = "aé".as_bytes(); // 61 C3 A9
        let (r, s) = char_align(orig, 2..3, &[0xA8]).unwrap();
        assert_eq!(r, 1..3); // the whole 'é'
        assert_eq!(s, "è");
    }

    #[test]
    fn located_edit_maps_replace_range_to_a_region() {
        let text = "{\n  \"a\": 1\n}\n";
        let one = text.find('1').unwrap();
        let edit = FixEdit::ReplaceRange {
            path: PathBuf::from("app.json"),
            range: one..one + 1,
            content: b"2".to_vec(),
        };
        let pe = located_edit_to_proposed(text.as_bytes(), text, &edit).unwrap();
        assert_eq!(pe.path, PathBuf::from("app.json"));
        assert_eq!(pe.inserted, "2");
        assert_eq!((pe.region.start_line, pe.region.start_column), (2, 8));
        assert_eq!((pe.region.end_line, pe.region.end_column), (2, 9));
    }

    #[test]
    fn set_content_maps_to_a_minimal_changed_span() {
        // Trim trailing spaces: "a   \nb\n" -> "a\nb\n". The minimal change is
        // deleting the "   " on line 1, NOT a full-artifact rewrite.
        let orig = b"a   \nb\n";
        let edit = FixEdit::SetContent {
            path: PathBuf::from("x.txt"),
            content: b"a\nb\n".to_vec(),
        };
        let pe = whole_file_edit_to_proposed(orig, &edit).unwrap();
        assert_eq!(pe.inserted, ""); // pure deletion
        assert_eq!((pe.region.start_line, pe.region.start_column), (1, 2));
        assert_eq!((pe.region.end_line, pe.region.end_column), (1, 5));
    }

    #[test]
    fn set_content_appends_via_an_end_insertion() {
        // Append a final newline: "abc" -> "abc\n" is an insertion at EOF.
        let orig = b"abc";
        let edit = FixEdit::SetContent {
            path: PathBuf::from("x.txt"),
            content: b"abc\n".to_vec(),
        };
        let pe = whole_file_edit_to_proposed(orig, &edit).unwrap();
        assert_eq!(pe.inserted, "\n");
        assert_eq!((pe.region.start_line, pe.region.start_column), (1, 4));
        assert_eq!((pe.region.end_line, pe.region.end_column), (1, 4)); // empty region
    }

    #[test]
    fn set_content_that_is_a_noop_yields_none() {
        let orig = b"same\n";
        let edit = FixEdit::SetContent {
            path: PathBuf::from("x.txt"),
            content: b"same\n".to_vec(),
        };
        assert!(whole_file_edit_to_proposed(orig, &edit).is_none());
    }

    #[test]
    fn create_file_maps_to_an_empty_insertion_at_start() {
        let edit = FixEdit::CreateFile {
            path: PathBuf::from("new.txt"),
            content: b"hello\n".to_vec(),
        };
        let pe = whole_file_edit_to_proposed(b"", &edit).unwrap();
        assert_eq!(pe.inserted, "hello\n");
        assert_eq!(
            pe.region,
            EditRegion {
                start_line: 1,
                start_column: 1,
                end_line: 1,
                end_column: 1
            }
        );
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
        assert!(whole_file_edit_to_proposed(b"x", &del).is_none());
        assert!(whole_file_edit_to_proposed(b"x", &ren).is_none());
        assert!(whole_file_edit_to_proposed(b"x", &chmod).is_none());
    }
}
