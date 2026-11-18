//! Where a `constraint` finding and its related locations sit: a cell's
//! resource and the line and column of its span.

use std::collections::HashMap;
use std::sync::Arc;

use alint_core::relation::eval::Cell;
use alint_core::relation::{ReadCache, RelPath, Resource, ResourceRead};
use alint_core::{Context, RelatedLocation, SourceRange};

/// The text of each resource a finding points into, read once for the rule
/// (through the run's read cache, which shares what a consumer retained).
pub(super) struct Places<'c> {
    ctx: &'c Context<'c>,
    local: Option<ReadCache>,
    texts: HashMap<RelPath, Option<Text>>,
}

/// A resource's text, checked as UTF-8 once, where each of its lines starts,
/// and how many characters come before each block of [`BLOCK`] bytes, so
/// that a column costs at most a block's count however long its line is.
struct Text {
    bytes: Arc<[u8]>,
    line_starts: Vec<usize>,
    block_chars: Vec<usize>,
}

/// The bytes a column count walks at most.
const BLOCK: usize = 1024;

impl<'c> Places<'c> {
    pub(super) fn new(ctx: &'c Context<'c>) -> Self {
        let local = ctx.index.read_cache().is_none().then(ReadCache::new);
        Self {
            ctx,
            local,
            texts: HashMap::new(),
        }
    }

    /// The line and column range of `cell`'s span, when it has one in a
    /// resource that reads as UTF-8.
    pub(super) fn range(&mut self, cell: &Cell) -> Option<SourceRange> {
        let span = cell.span?;
        let path = cell.at.resource.path();
        if !self.texts.contains_key(path) {
            let reads = self
                .local
                .as_ref()
                .or_else(|| self.ctx.index.read_cache())
                .expect("a read cache: the run's or a local one");
            // Read as the relation read it: as a literal path, which the
            // cache keeps inside the root and out of a Git directory, and
            // opens only when it is a regular file. The file may have been
            // swapped since for a symlink or a FIFO.
            let literal = Resource::literal(path.clone());
            let read = reads.read_unretained(self.ctx.root, &literal, 0);
            let text = match read.as_ref() {
                ResourceRead::Present(bytes) => Text::new(Arc::clone(bytes)),
                _ => None,
            };
            self.texts.insert(path.clone(), text);
        }
        let text = self.texts.get(path)?.as_ref()?;
        let (start_line, start_column) = text.line_column(span.start)?;
        let (end_line, end_column) = text.line_column(span.end)?;
        SourceRange::new(start_line, start_column, end_line, end_column)
    }

    /// A related location at `cell`, labelled `label`.
    pub(super) fn related(&mut self, cell: &Cell, label: &str) -> RelatedLocation {
        let location = RelatedLocation::new(cell.at.resource.path().as_path(), label);
        match self.range(cell) {
            Some(range) => location.with_range(range),
            None => location,
        }
    }
}

impl Text {
    /// `bytes` as a text, when they are UTF-8.
    fn new(bytes: Arc<[u8]>) -> Option<Self> {
        let text = std::str::from_utf8(&bytes).ok()?;
        let line_starts = line_starts(text);
        let mut block_chars = Vec::with_capacity(bytes.len() / BLOCK + 1);
        let mut chars = 0;
        for block in bytes.chunks(BLOCK) {
            block_chars.push(chars);
            chars += char_count(block);
        }
        block_chars.push(chars);
        Some(Self {
            bytes,
            line_starts,
            block_chars,
        })
    }

    fn line_column(&self, offset: usize) -> Option<(usize, usize)> {
        if !self.is_char_boundary(offset) {
            return None;
        }
        let line = self.line_starts.partition_point(|start| *start <= offset);
        let line = line.checked_sub(1)?;
        let start = *self.line_starts.get(line)?;
        let column = self.chars_before(offset) - self.chars_before(start) + 1;
        Some((line + 1, column))
    }

    /// Whether a character starts at `offset`, or the text ends there.
    fn is_char_boundary(&self, offset: usize) -> bool {
        match self.bytes.get(offset) {
            Some(&byte) => !is_continuation(byte),
            None => offset == self.bytes.len(),
        }
    }

    /// The characters before `offset`, a character boundary.
    fn chars_before(&self, offset: usize) -> usize {
        let block = offset / BLOCK;
        self.block_chars[block] + char_count(&self.bytes[block * BLOCK..offset])
    }
}

/// Whether `byte` continues a UTF-8 character rather than starting one.
fn is_continuation(byte: u8) -> bool {
    byte & 0xC0 == 0x80
}

/// The characters that start in `bytes`, a slice of UTF-8 text.
fn char_count(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&byte| !is_continuation(byte)).count()
}

/// Byte offsets at which each line starts, treating LF, CRLF and a lone CR as
/// one line ending; a leading BOM occupies no column (as `key_parity` counts).
fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![if text.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    }];
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
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str) -> Text {
        Text::new(Arc::from(text.as_bytes())).expect("UTF-8")
    }

    #[test]
    fn lf_crlf_and_a_lone_cr_each_end_a_line_and_a_bom_takes_no_column() {
        assert_eq!(line_starts("a\nb\r\nc\rd"), [0, 2, 5, 7]);
        let text = text("\u{feff}ab\nc");
        assert_eq!(text.line_column(3), Some((1, 1)));
        assert_eq!(text.line_column(4), Some((1, 2)));
        assert_eq!(text.line_column(6), Some((2, 1)));
        // An offset inside a character or past the end has no place.
        assert_eq!(text.line_column(1), None);
        assert_eq!(text.line_column(8), None);
        assert!(Text::new(Arc::from(&b"\xff"[..])).is_none());
    }

    #[test]
    fn a_column_counts_characters() {
        assert_eq!(text("\u{e9}\u{e9}x").line_column(4), Some((1, 3)));
        // Across blocks, on a line that starts in one block and ends in
        // another.
        let long = format!("x\n{}y", "\u{e9}".repeat(BLOCK * 3));
        let offset = long.len() - 1;
        assert_eq!(
            text(&long).line_column(offset),
            Some((2, BLOCK * 3 + 1)),
            "the column of `y`"
        );
        assert_eq!(text(&long).line_column(offset - 1), None);
    }

    /// The text is checked as UTF-8 once, and a column costs a block's count
    /// at most: 20,000 places in a 4 MiB text, its lines long and of
    /// characters of two bytes, take well under a second, where a check per
    /// place takes many.
    #[test]
    fn a_place_costs_neither_a_check_of_the_text_nor_a_count_of_its_line() {
        let chars = 64 * 1024;
        let line = "\u{e9}".repeat(chars);
        let body = [line.as_str(); 32].join("\n");
        let started = std::time::Instant::now();
        let text = text(&body);
        for place in 0..20_000 {
            let (row, column) = (place % 32, (place * 7919) % chars);
            let offset = row * (line.len() + 1) + column * 2;
            assert_eq!(text.line_column(offset), Some((row + 1, column + 1)));
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "20,000 places took {:?}",
            started.elapsed()
        );
    }
}
