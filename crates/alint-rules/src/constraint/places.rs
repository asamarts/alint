//! Where a `constraint` finding and its related locations sit: a cell's
//! resource and the line and column of its span.

use std::collections::HashMap;
use std::sync::Arc;

use alint_core::relation::eval::Cell;
use alint_core::relation::{ReadCache, RelPath, ResourceRead};
use alint_core::{Context, RelatedLocation, SourceRange};

/// The text of each resource a finding points into, read once (through the
/// run's read cache, which holds what the relation read).
pub(super) struct Places<'c> {
    ctx: &'c Context<'c>,
    local: Option<ReadCache>,
    texts: HashMap<RelPath, Option<Text>>,
}

/// A resource's text and where each of its lines starts.
struct Text {
    bytes: Arc<[u8]>,
    line_starts: Vec<usize>,
}

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
        let resource = &cell.at.resource;
        if !self.texts.contains_key(resource.path()) {
            let reads = self
                .local
                .as_ref()
                .or_else(|| self.ctx.index.read_cache())
                .expect("a read cache: the run's or a local one");
            let read = reads.read(self.ctx.root, resource, 0);
            let text = match read.as_ref() {
                ResourceRead::Present(bytes) => std::str::from_utf8(bytes).ok().map(|text| Text {
                    line_starts: line_starts(text),
                    bytes: Arc::clone(bytes),
                }),
                _ => None,
            };
            self.texts.insert(resource.path().clone(), text);
        }
        let text = self.texts.get(resource.path())?.as_ref()?;
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
    fn line_column(&self, offset: usize) -> Option<(usize, usize)> {
        let text = std::str::from_utf8(&self.bytes).ok()?;
        if offset > text.len() || !text.is_char_boundary(offset) {
            return None;
        }
        let line = self.line_starts.partition_point(|start| *start <= offset);
        let line = line.checked_sub(1)?;
        let start = *self.line_starts.get(line)?;
        let column = text.get(start..offset)?.chars().count() + 1;
        Some((line + 1, column))
    }
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

    #[test]
    fn lf_crlf_and_a_lone_cr_each_end_a_line_and_a_bom_takes_no_column() {
        assert_eq!(line_starts("a\nb\r\nc\rd"), [0, 2, 5, 7]);
        let text = "\u{feff}ab\nc";
        let text = Text {
            line_starts: line_starts(text),
            bytes: Arc::from(text.as_bytes()),
        };
        assert_eq!(text.line_column(3), Some((1, 1)));
        assert_eq!(text.line_column(4), Some((1, 2)));
        assert_eq!(text.line_column(6), Some((2, 1)));
        // An offset inside a character or past the end has no place.
        assert_eq!(text.line_column(1), None);
        assert_eq!(text.line_column(8), None);
    }

    #[test]
    fn a_column_counts_characters() {
        let text = "\u{e9}\u{e9}x";
        let text = Text {
            line_starts: line_starts(text),
            bytes: Arc::from(text.as_bytes()),
        };
        assert_eq!(text.line_column(4), Some((1, 3)));
    }
}
