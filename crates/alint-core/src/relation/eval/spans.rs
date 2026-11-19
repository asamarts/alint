//! Where a `read:` value is written, found only for the values a finding
//! places.
//!
//! A `read:` part reads its values as `cross_file` does, without their
//! spans: a value's span is found from the document's entries for a JSON,
//! YAML or TOML value, which cost several times the read itself, and most
//! rows end in no finding. So a `read:` cell's span is a [`ReadSpan`]: the
//! reader that read it and its row's ordinal, from which [`find_spans`]
//! finds where the value is written when a finding asks, one resource at a
//! time.

use std::fmt;
use std::sync::Arc;

use crate::extract::{ByteSpan, Extract};
use crate::relation::adapters::{locate, same_reading, walks};
use crate::relation::limits::Limits;

use super::Span;

/// What a `read:` part read with: its extractor and the limits it read
/// under, shared by the spans of the values it read.
#[derive(Debug)]
pub(super) struct Reading {
    extract: Extract,
    limits: Limits,
}

impl Reading {
    pub(super) fn new(extract: Extract, limits: Limits) -> Arc<Self> {
        Arc::new(Self { extract, limits })
    }
}

/// Where a `read:` value is written, still to be found ([`find_spans`]):
/// the reader that read it, and its row's place among the rows that reader
/// emitted for the resource ([`Provenance::ordinal`](super::Provenance::ordinal)).
#[derive(Clone)]
pub struct ReadSpan {
    reading: Arc<Reading>,
    ordinal: u32,
}

impl ReadSpan {
    pub(super) fn new(reading: &Arc<Reading>, ordinal: u32) -> Self {
        Self {
            reading: Arc::clone(reading),
            ordinal,
        }
    }

    /// Whether `self` and `other` were read alike: by one reader, or by two
    /// that read with one specification under the same limits.
    fn reads_like(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.reading, &other.reading)
            || (self.reading.limits == other.reading.limits
                && same_reading(&self.reading.extract, &other.reading.extract))
    }
}

impl PartialEq for ReadSpan {
    fn eq(&self, other: &Self) -> bool {
        self.ordinal == other.ordinal && self.reads_like(other)
    }
}

impl Eq for ReadSpan {}

impl fmt::Debug for ReadSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadSpan")
            .field("extract", &self.reading.extract)
            .field("ordinal", &self.ordinal)
            .finish()
    }
}

/// Where each of `spans`, the spans of cells of one resource, is in
/// `bytes`, the resource as read again: a [`Span::Bytes`] as it is, and a
/// [`Span::Read`] where its reader finds its value in `bytes` now (a `lines:`
/// value's trimmed line, a `regex:` value's capture, the value a JSON, YAML
/// or TOML object member holds), or nowhere: in a resource that is not
/// UTF-8, or no longer holds the row, and for a value in an array or a
/// repeated key's. Each reader walks `bytes` once, however many of its
/// values are asked for.
///
/// Also the most the walks held at once besides `bytes` (a structured
/// document's tree, and its entries when a value asked for is an object
/// member's), which the caller takes from the run's bytes
/// ([`LimitKind::BytesPerRun`](crate::relation::LimitKind::BytesPerRun)):
/// each walk lets go of what it held before the next.
pub fn find_spans<'s>(
    bytes: &[u8],
    spans: impl IntoIterator<Item = &'s Span>,
) -> (Vec<Option<ByteSpan>>, usize) {
    let mut found = Vec::new();
    // Each reader, with the cells it read: their places in `found`, and
    // their ordinals.
    let mut readers: Vec<(&Arc<Reading>, Vec<usize>, Vec<u32>)> = Vec::new();
    for span in spans {
        match span {
            Span::Bytes(at) => found.push(Some(*at)),
            Span::Read(read) => {
                let index = found.len();
                found.push(None);
                match readers
                    .iter_mut()
                    .find(|(reading, ..)| Arc::ptr_eq(reading, &read.reading))
                {
                    Some((_, indexes, ordinals)) => {
                        indexes.push(index);
                        ordinals.push(read.ordinal);
                    }
                    None => readers.push((&read.reading, vec![index], vec![read.ordinal])),
                }
            }
        }
    }
    // A span indexes the file's bytes, which a lossy text does not.
    let Ok(text) = std::str::from_utf8(bytes) else {
        return (found, 0);
    };
    let mut held = 0;
    for (reading, indexes, ordinals) in readers {
        let (spans, walked) = locate(&reading.extract, text, &ordinals, &reading.limits);
        held = held.max(walked);
        for (index, span) in indexes.into_iter().zip(spans) {
            found[index] = span;
        }
    }
    (found, held)
}

/// How many times a reader has walked a resource's text on this thread to
/// find where its `read:` values are written ([`find_spans`]). Test only: a
/// run whose rows end in no finding walks none, and one with a finding walks
/// only the resource it sits in.
#[doc(hidden)]
#[must_use]
pub fn read_span_walks() -> usize {
    walks()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::LinesOpts;
    use crate::structured_format::Format;

    fn read(extract: Extract, ordinal: u32) -> Span {
        Span::Read(ReadSpan::new(
            &Reading::new(extract, Limits::default()),
            ordinal,
        ))
    }

    /// A known span is kept, a `read:` value's found, each reader walking
    /// the text once; a text that is not UTF-8 places no `read:` value.
    #[test]
    fn each_reader_walks_the_text_once() {
        let lines = Reading::new(Extract::Lines(LinesOpts::default()), Limits::default());
        let json = Reading::new(
            Extract::structured(Format::Json, "$.*").unwrap(),
            Limits::default(),
        );
        let text = b"{\"a\": \"x\",\n \"b\": \"y\"}\n";
        let spans = [
            Span::Read(ReadSpan::new(&json, 1)),
            Span::Bytes(ByteSpan::new(0, 1)),
            Span::Read(ReadSpan::new(&lines, 1)),
            Span::Read(ReadSpan::new(&json, 0)),
        ];
        let before = read_span_walks();
        let (found, held) = find_spans(text, &spans);
        assert_eq!(read_span_walks() - before, 2);
        assert_eq!(
            found,
            [
                Some(ByteSpan::new(17, 20)),
                Some(ByteSpan::new(0, 1)),
                Some(ByteSpan::new(12, 21)),
                Some(ByteSpan::new(6, 9)),
            ]
        );
        assert!(held > text.len(), "{held}");
        let (found, held) = find_spans(b"{\"a\": \"\xff\"}", &spans);
        assert_eq!(found, [None, Some(ByteSpan::new(0, 1)), None, None]);
        assert_eq!(held, 0);
        assert_eq!(read_span_walks() - before, 2);
    }

    /// Two spans are equal when they were read alike, by one reader or by
    /// two of one specification, and are of one row.
    #[test]
    fn spans_read_alike_are_equal() {
        let json = || Extract::structured(Format::Json, "$.*").unwrap();
        assert_eq!(read(json(), 1), read(json(), 1));
        assert_ne!(read(json(), 1), read(json(), 0));
        let other = Extract::structured(Format::Json, "$.a").unwrap();
        assert_ne!(read(json(), 1), read(other, 1));
        assert_ne!(read(json(), 1), Span::Bytes(ByteSpan::new(0, 1)));
    }
}
