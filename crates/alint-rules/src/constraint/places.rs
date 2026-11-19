//! Where a `constraint` finding and its related locations sit: a cell's
//! resource and the line and column of its span.
//!
//! A finding asks for its places as it is built ([`Places::ask`]), and the
//! rule places them all at the end ([`Places::resolve`]), one resource at a
//! time: the resource is read, each place asked of it is found in one pass
//! over its text, and the text is let go before the next resource is read.
//! A `read:` value's span is found in that text first ([`find_spans`]): the
//! relation read it without one. So placing holds one resource's text (and
//! what finding its `read:` values holds) at a time, however many resources
//! the findings sit in, and each resource's text, with the offsets asked of
//! it and what finding its `read:` values held, must fit in what the run's
//! relations left of its bytes
//! ([`LimitKind::BytesPerRun`](alint_core::relation::LimitKind::BytesPerRun),
//! [`DecodeCache::may_hold`]): past them the rule has no verdict. Placing
//! takes nothing from the run's bytes, since it lets each text go, so
//! whether a rule's findings fit depends on its files and the relations
//! alone, never on the rules placing beside it.

use std::collections::BTreeMap;

use alint_core::relation::eval::{Cell, Span, find_spans};
use alint_core::relation::{
    DecodeCache, LimitExceeded, ReadCache, RelPath, Resource, ResourceRead,
};
use alint_core::{Context, SourceRange};

/// The places a rule's findings ask for, by resource, until it resolves
/// them.
pub(super) struct Places<'c> {
    ctx: &'c Context<'c>,
    /// Each resource's spans, each with the index of the place that asked.
    asked: BTreeMap<RelPath, Vec<(usize, Span)>>,
    count: usize,
}

/// A place a finding asked for, which [`Places::resolve`] fills in.
#[derive(Debug, Clone, Copy)]
pub(super) struct Asked {
    index: usize,
    /// Only its start is shown: the cell holds a sensitive value (or key),
    /// whose length its end would give away.
    pub(super) point: bool,
}

/// Where each place asked for is, when its resource reads as UTF-8 and its
/// span is on a character boundary inside it.
pub(super) struct Placed(Vec<Option<SourceRange>>);

impl Placed {
    /// The range of `asked`: one with no length when only its start is
    /// shown.
    pub(super) fn range(&self, asked: Asked) -> Option<SourceRange> {
        let range = self.0[asked.index].as_ref()?;
        if asked.point {
            SourceRange::new(
                range.start_line(),
                range.start_column(),
                range.start_line(),
                range.start_column(),
            )
        } else {
            Some(range.clone())
        }
    }
}

impl<'c> Places<'c> {
    pub(super) fn new(ctx: &'c Context<'c>) -> Self {
        Self {
            ctx,
            asked: BTreeMap::new(),
            count: 0,
        }
    }

    /// Ask for the place of `cell`'s span, when it has one; `point` shows
    /// only its start.
    pub(super) fn ask(&mut self, cell: &Cell, point: bool) -> Option<Asked> {
        let span = cell.span.clone()?;
        let index = self.count;
        self.count += 1;
        self.asked
            .entry(cell.at.resource.path().clone())
            .or_default()
            .push((index, span));
        Some(Asked { index, point })
    }

    /// Every place asked for, each resource read once, in path order, and
    /// let go before the next; or the first resource whose text, with its
    /// offsets, does not fit in what the run's relations left of its bytes,
    /// and the limit it reached.
    pub(super) fn resolve(self) -> Result<Placed, (RelPath, LimitExceeded)> {
        let mut placed = vec![None; self.count];
        if self.count == 0 {
            return Ok(Placed(placed));
        }
        let local_reads = self.ctx.index.read_cache().is_none().then(ReadCache::new);
        let reads = local_reads
            .as_ref()
            .or_else(|| self.ctx.index.read_cache())
            .expect("a read cache: the run's or a local one");
        let local_bytes = self
            .ctx
            .index
            .decode_cache()
            .is_none()
            .then(DecodeCache::new);
        let budget = local_bytes
            .as_ref()
            .or_else(|| self.ctx.index.decode_cache())
            .expect("a decode cache: the run's or a local one");
        for (path, spans) in self.asked {
            // Read as the relation read it: as a literal path, which the
            // cache keeps inside the root and out of a Git directory, and
            // opens only when it is a regular file. The file may have been
            // swapped since for a symlink or a FIFO.
            let read = reads.read_unretained(self.ctx.root, &Resource::literal(path.clone()), 0);
            let ResourceRead::Present(bytes) = read.as_ref() else {
                continue;
            };
            // A `read:` value's span is found now, in the text read again;
            // what finding it held counts with the text.
            let (found, finding) = find_spans(bytes, spans.iter().map(|(_, span)| span));
            let spans: Vec<(usize, _)> = spans
                .iter()
                .zip(found)
                .filter_map(|((index, _), span)| Some((*index, span?)))
                .collect();
            let mut offsets: Vec<usize> = spans
                .iter()
                .flat_map(|(_, span)| [span.start, span.end])
                .collect();
            offsets.sort_unstable();
            offsets.dedup();
            let held = bytes.len() + offsets.capacity() * std::mem::size_of::<usize>() + finding;
            if let Err(limit) = budget.may_hold(held) {
                return Err((path, limit));
            }
            let Some(found) = positions(bytes, &offsets) else {
                continue;
            };
            let at = |offset: usize| found[offsets.binary_search(&offset).expect("asked for")];
            for (index, span) in spans {
                if let (Some((start_line, start_column)), Some((end_line, end_column))) =
                    (at(span.start), at(span.end))
                {
                    placed[index] =
                        SourceRange::new(start_line, start_column, end_line, end_column);
                }
            }
        }
        Ok(Placed(placed))
    }
}

/// The line and column of each of `offsets` (sorted, without repeats) in
/// `bytes`, in one pass, or `None` when `bytes` are not UTF-8. LF, CRLF and
/// a lone CR each end a line, and a leading BOM occupies no column (as
/// `key_parity` counts); an offset inside a character, inside the BOM or
/// past the end has no place.
fn positions(bytes: &[u8], offsets: &[usize]) -> Option<Vec<Option<(usize, usize)>>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let start = if text.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    };
    let mut found = Vec::with_capacity(offsets.len());
    // The line at `cursor`, and the characters of it before `cursor`.
    let (mut cursor, mut line, mut column) = (start, 1, 0);
    for &offset in offsets {
        if offset < start || offset > bytes.len() {
            found.push(None);
            continue;
        }
        while cursor < offset {
            match bytes[cursor] {
                b'\n' => (line, column) = (line + 1, 0),
                b'\r' if bytes.get(cursor + 1) != Some(&b'\n') => (line, column) = (line + 1, 0),
                byte if !is_continuation(byte) => column += 1,
                _ => {}
            }
            cursor += 1;
        }
        let boundary = bytes.get(offset).is_none_or(|&byte| !is_continuation(byte));
        found.push(boundary.then_some((line, column + 1)));
    }
    Some(found)
}

/// Whether `byte` continues a UTF-8 character rather than starting one.
fn is_continuation(byte: u8) -> bool {
    byte & 0xC0 == 0x80
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The place of `offset` as the line table this replaced found it: the
    /// oracle the one pass is checked against.
    fn oracle(text: &str, offset: usize) -> Option<(usize, usize)> {
        let mut starts = vec![if text.starts_with('\u{feff}') { 3 } else { 0 }];
        let bytes = text.as_bytes();
        let mut index = starts[0];
        while index < bytes.len() {
            match bytes[index] {
                b'\n' => starts.push(index + 1),
                b'\r' => {
                    if bytes.get(index + 1) == Some(&b'\n') {
                        index += 1;
                    }
                    starts.push(index + 1);
                }
                _ => {}
            }
            index += 1;
        }
        if offset > text.len() || !text.is_char_boundary(offset) {
            return None;
        }
        let line = starts
            .partition_point(|start| *start <= offset)
            .checked_sub(1)?;
        let column = text[starts[line]..offset].chars().count() + 1;
        Some((line + 1, column))
    }

    fn one(text: &str, offset: usize) -> Option<(usize, usize)> {
        positions(text.as_bytes(), &[offset]).expect("UTF-8")[0]
    }

    #[test]
    fn lf_crlf_and_a_lone_cr_each_end_a_line_and_a_bom_takes_no_column() {
        assert_eq!(one("a\nb\r\nc\rd", 2), Some((2, 1)));
        assert_eq!(one("a\nb\r\nc\rd", 4), Some((2, 3)), "the LF of a CRLF");
        assert_eq!(one("a\nb\r\nc\rd", 5), Some((3, 1)));
        assert_eq!(one("a\nb\r\nc\rd", 7), Some((4, 1)));
        assert_eq!(one("\u{feff}ab\nc", 3), Some((1, 1)));
        assert_eq!(one("\u{feff}ab\nc", 4), Some((1, 2)));
        assert_eq!(one("\u{feff}ab\nc", 6), Some((2, 1)));
        // An offset inside the BOM, inside a character or past the end has
        // no place, and a text that is not UTF-8 has none at all.
        assert_eq!(one("\u{feff}ab\nc", 0), None);
        assert_eq!(one("\u{feff}ab\nc", 1), None);
        assert_eq!(one("\u{feff}ab\nc", 8), None);
        assert_eq!(one("ab", 2), Some((1, 3)));
        assert!(positions(b"\xff", &[0]).is_none());
    }

    /// Every offset of texts mixing line endings, a BOM and characters of
    /// one to four bytes, in one pass, is where the line table put it.
    #[test]
    fn one_pass_places_every_offset_as_a_line_table_does() {
        for text in [
            "",
            "a",
            "\u{feff}",
            "\u{feff}\r\n\u{e9}\rx\n\n\r\r\n\u{1f600}y",
            "\r\n\r\n\u{e9}\u{e9}\n\u{20ac}\r",
            "\u{2028}a\u{85}b\n",
        ] {
            let offsets: Vec<usize> = (0..=text.len() + 1).collect();
            let found = positions(text.as_bytes(), &offsets).expect("UTF-8");
            for (offset, place) in offsets.iter().zip(found) {
                assert_eq!(place, oracle(text, *offset), "{text:?} at {offset}");
            }
        }
    }

    #[test]
    fn a_column_counts_characters() {
        assert_eq!(one("\u{e9}\u{e9}x", 4), Some((1, 3)));
        let long = format!("x\n{}y", "\u{e9}".repeat(3_000));
        assert_eq!(
            one(&long, long.len() - 1),
            Some((2, 3_001)),
            "the column of `y`"
        );
        assert_eq!(one(&long, long.len() - 2), None);
    }
}
