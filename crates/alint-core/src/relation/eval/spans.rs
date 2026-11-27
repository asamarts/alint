//! Where a cell's value is written, found and checked only for the cells a
//! finding places.
//!
//! A `read:` part reads its values as `cross_file` does, without their
//! spans: a value's span is found from the document's entries for a JSON,
//! YAML or TOML value, which cost several times the read itself, and most
//! rows end in no finding. So a `read:` cell's span is a [`ReadSpan`]: the
//! reader that read it and its row's ordinal, from which [`find_spans`]
//! finds where the value is written when a finding asks, one resource at a
//! time. An entry's cell knows its span as it is read ([`BytesSpan`]).
//!
//! The resource is read again to place a cell, and may have changed since
//! the relation read it, so each span also holds a fingerprint of what its
//! text held then, the value or the key ([`fingerprint`]): a cell is placed
//! only where its text still holds that. It is taken from the value the
//! cell was read with, before a `transform:` changed it, so making it costs
//! a pass over each spanned value the row already holds, never one over the
//! resource; the resource is walked only to place a finding.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use crate::extract::{ByteSpan, EntryValue, Extract, JsonSyntax};
use crate::relation::adapters::{
    entry_table, entry_table_with_arrays, locate, same_reading, walks,
};
use crate::relation::key_paths::EntryTable;
use crate::relation::limits::Limits;
use crate::relation::pointer::Segment;
use crate::relation::value::Value;
use crate::structured_format::Format;

use super::Span;

/// A 32-bit fingerprint of `value`, by its kind and its text, which a span
/// keeps of what its text held when the cell was read: placing compares it
/// with what the text holds when the resource is read again. Two values
/// that differ share one with a chance of about one in four billion.
pub(crate) fn fingerprint(value: &Value) -> u32 {
    match value {
        Value::Null => mix(0, b""),
        Value::Bool(flag) => mix(1, &[u8::from(*flag)]),
        Value::Number(number) => mix(2, number.lexeme().as_bytes()),
        Value::String(text) => fingerprint_text(text),
        Value::Bytes(bytes) => mix(4, bytes),
        Value::Path(path) => mix(5, path.as_str().as_bytes()),
        // No spanned cell holds one: a list or a record is not a scalar.
        Value::List(_) | Value::Record(_) => mix(6, b""),
    }
}

/// [`fingerprint`] of a string value of `text`, or of a key.
pub(crate) fn fingerprint_text(text: &str) -> u32 {
    mix(3, text.as_bytes())
}

/// A hash of `bytes`, eight at a time, seeded with `kind` and the length,
/// then mixed so every bit of it moves the result (`splitmix64`'s finish).
/// Each word is taken in by [`fold`], so a byte moves the bits below it as
/// well as those above: a plain multiply moves only those above, and two
/// texts that differ in the last byte of one word and the first of the next
/// shared a fingerprint one time in sixteen.
fn mix(kind: u64, bytes: &[u8]) -> u32 {
    const K: u64 = 0x9E37_79B9_7F4A_7C15;
    const L: u64 = 0xD6E8_FEB8_6659_FD93;
    let length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let mut hash = fold(kind ^ K, length ^ L);
    let (words, rest) = bytes.as_chunks::<8>();
    for word in words {
        hash = fold(u64::from_le_bytes(*word) ^ K, hash ^ L);
    }
    if !rest.is_empty() {
        let mut word = [0; 8];
        word[..rest.len()].copy_from_slice(rest);
        hash = fold(u64::from_le_bytes(word) ^ K, hash ^ L);
    }
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94D0_49BB_1331_11EB);
    hash ^= hash >> 31;
    u32::try_from(hash >> 32).expect("32 bits")
        ^ u32::try_from(hash & 0xFFFF_FFFF).expect("32 bits")
}

/// The 128-bit product of `a` and `b`, its two halves xored together: each
/// bit of either moves bits of the result above and below its own.
fn fold(a: u64, b: u64) -> u64 {
    let product = u128::from(a) * u128::from(b);
    let half = |bits: u128| u64::try_from(bits & u128::from(u64::MAX)).expect("64 bits");
    half(product) ^ half(product >> 64)
}

/// What an entry's span held when the cell was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Holds {
    /// The entry's key: the span of an `entry.key`, `entry.parent`,
    /// `entry.root` and `entry.path` cell.
    Key,
    /// The entry's value: an `entry.value` or a `bound:` cell's, at the
    /// value, or at its key for a YAML block scalar, whose value has no
    /// span of its own.
    Value,
    /// A `records:` subfield value (`record: <JSONPath>`): for TOML, requires
    /// descending arrays of tables and inline tables inside arrays.
    RecordValue,
}

/// Where an entry's cell is written: its key's or its value's bytes, read
/// as `format`, with a 32-bit fingerprint of the key or the value they
/// held, which [`find_spans`] checks before it places the cell there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BytesSpan {
    span: ByteSpan,
    held: u32,
    format: Format,
    holds: Holds,
    strict: bool,
}

impl BytesSpan {
    /// The span of the key `key`, which `format` reads at `span`.
    pub(super) fn key(span: ByteSpan, format: Format, strict: bool, key: &str) -> Self {
        Self {
            span,
            held: fingerprint_text(key),
            format,
            holds: Holds::Key,
            strict: strict && format == Format::Json,
        }
    }

    /// The span of `value`, which `format` reads at `span`: the value's, or
    /// its entry's key's for a YAML block scalar, whose value has none.
    pub(super) fn value(span: ByteSpan, format: Format, strict: bool, value: &Value) -> Self {
        Self {
            span,
            held: fingerprint(value),
            format,
            holds: Holds::Value,
            strict: strict && format == Format::Json,
        }
    }

    /// The span of a `records:` subfield `value`, which `format` reads at
    /// `span` (descending TOML arrays of tables / inline tables on verification).
    pub(super) fn record_value(
        span: ByteSpan,
        format: Format,
        strict: bool,
        value: &Value,
    ) -> Self {
        Self {
            span,
            held: fingerprint(value),
            format,
            holds: Holds::RecordValue,
            strict: strict && format == Format::Json,
        }
    }

    /// The bytes.
    #[must_use]
    pub const fn bytes(&self) -> ByteSpan {
        self.span
    }
}

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
/// the reader that read it, its row's place among the rows that reader
/// emitted for the resource ([`Provenance::ordinal`](super::Provenance::ordinal)),
/// and a 32-bit fingerprint of the value, which [`find_spans`] checks.
#[derive(Clone)]
pub struct ReadSpan {
    reading: Arc<Reading>,
    ordinal: u32,
    held: u32,
}

impl ReadSpan {
    /// The span of `value`, the row `ordinal` that `reading` read.
    pub(super) fn new(reading: &Arc<Reading>, ordinal: u32, value: &Value) -> Self {
        Self {
            reading: Arc::clone(reading),
            ordinal,
            held: fingerprint(value),
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
        self.ordinal == other.ordinal && self.held == other.held && self.reads_like(other)
    }
}

impl Eq for ReadSpan {}

impl fmt::Debug for ReadSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadSpan")
            .field("extract", &self.reading.extract)
            .field("ordinal", &self.ordinal)
            .field("held", &self.held)
            .finish()
    }
}

/// Where each of `spans`, the spans of cells of one resource, is in
/// `bytes`, the resource as read again, when its text there still holds
/// what it held when the cell was read: a [`Span::Bytes`] at its bytes, and
/// a [`Span::Read`] where its reader finds its value in `bytes` now (a
/// `lines:` value's trimmed line, a `regex:` value's capture, the value a
/// JSON, YAML or TOML object member holds). Or nowhere: in a resource that
/// is not UTF-8, at a text that holds something else now (the resource
/// changed since the relation read it), and for a `read:` value in an array
/// or a repeated key's. Each reader walks `bytes` once, however many of its
/// values are asked for, and the entries' spans are checked in one read of
/// `bytes` per format.
///
/// Also the most the walks held at once besides `bytes` (a structured
/// document's tree, the parse its members are read from, and its entries),
/// which the caller counts
/// with `bytes` in what it may hold of the run's bytes
/// ([`LimitKind::BytesPerRun`](crate::relation::LimitKind::BytesPerRun),
/// [`DecodeCache::may_hold`](crate::relation::DecodeCache::may_hold)): each
/// walk lets go of what it held before the next.
pub fn find_spans<'s>(
    bytes: &[u8],
    spans: impl IntoIterator<Item = &'s Span>,
) -> (Vec<Option<ByteSpan>>, usize) {
    find_spans_with_limits(bytes, spans, &Limits::default())
}

/// [`find_spans`] honoring the caller's configured relation `limits` (such as
/// `limits.key_path_bytes_per_decode`) when verifying entry spans.
pub fn find_spans_with_limits<'s>(
    bytes: &[u8],
    spans: impl IntoIterator<Item = &'s Span>,
    limits: &Limits,
) -> (Vec<Option<ByteSpan>>, usize) {
    let mut found = Vec::new();
    let mut readers: Vec<Asked<'_>> = Vec::new();
    // The entries' spans, with their places in `found`.
    let mut entries: Vec<(usize, &BytesSpan)> = Vec::new();
    for span in spans {
        let index = found.len();
        found.push(None);
        match span {
            Span::Bytes(at) => entries.push((index, at)),
            Span::Read(read) => {
                let cell = (read.ordinal, read.held);
                match readers
                    .iter_mut()
                    .find(|asked| Arc::ptr_eq(asked.reading, &read.reading))
                {
                    Some(asked) => {
                        asked.places.push(index);
                        asked.cells.push(cell);
                    }
                    None => readers.push(Asked {
                        reading: &read.reading,
                        places: vec![index],
                        cells: vec![cell],
                    }),
                }
            }
        }
    }
    // A span indexes the file's bytes, which a lossy text does not.
    let Ok(text) = std::str::from_utf8(bytes) else {
        return (found, 0);
    };
    let mut held = 0;
    for (index, span, walked) in still_held(text, &entries, limits) {
        found[index] = span;
        held = held.max(walked);
    }
    for Asked {
        reading,
        places,
        cells,
    } in readers
    {
        let (spans, walked) = locate(&reading.extract, text, &cells, &reading.limits);
        held = held.max(walked);
        for (index, span) in places.into_iter().zip(spans) {
            found[index] = span;
        }
    }
    (found, held)
}

/// A reader's cells [`find_spans`] is asked for: their places in what it
/// finds, and their ordinals and fingerprints.
struct Asked<'s> {
    reading: &'s Arc<Reading>,
    places: Vec<usize>,
    cells: Vec<(u32, u32)>,
}

/// Each of `entries`' spans, with its place in what [`find_spans`] found,
/// when `text` still holds there what it held, and what reading `text`
/// held: the entries of `text`, read once for each format asked, must have
/// a member whose key is at the span and is the key fingerprinted, or whose
/// value is at the span (at its key, for a YAML block scalar) and is the
/// value fingerprinted. The entries are read as the `entries:` part read
/// them, so a span marks the same token's bytes, never part of one.
fn still_held(
    text: &str,
    entries: &[(usize, &BytesSpan)],
    limits: &Limits,
) -> Vec<(usize, Option<ByteSpan>, usize)> {
    let mut placed = Vec::with_capacity(entries.len());
    let mut pending: Vec<(usize, &BytesSpan)> = entries.to_vec();
    while let Some(&(_, first)) = pending.first() {
        let format = first.format;
        let strict = first.strict;
        let (these, rest): (Vec<_>, Vec<_>) = pending
            .into_iter()
            .partition(|(_, span)| span.format == format && span.strict == strict);
        pending = rest;
        // Each span asked, by its bytes and what it held.
        let mut asked: HashMap<(usize, usize, Holds), Vec<(usize, u32)>> = HashMap::new();
        for (index, span) in &these {
            asked
                .entry((span.span.start, span.span.end, span.holds))
                .or_default()
                .push((*index, span.held));
        }
        let mut held: HashSet<usize> = HashSet::new();
        let mut check = |at: ByteSpan, holds: Holds, now: &dyn Fn() -> Option<u32>| {
            if let Some(cells) = asked.get(&(at.start, at.end, holds)) {
                let now = now();
                held.extend(
                    cells
                        .iter()
                        .filter(|(_, was)| now == Some(*was))
                        .map(|(index, _)| *index),
                );
            }
        };
        let mut walked = 0;
        if format == Format::Json {
            // A JSON document's members are read from its syntax tree,
            // without the entries every member would make.
            if let Ok(syntax) = JsonSyntax::guarded_with_strict(text, strict) {
                walked = syntax.held();
                syntax.members(&mut |member| {
                    check(member.key_span, Holds::Key, &|| {
                        Some(fingerprint_text(member.key))
                    });
                    let value_fp = || match member.value() {
                        EntryValue::Scalar(value) => Some(fingerprint(&value)),
                        _ => None,
                    };
                    check(member.value_span, Holds::Value, &value_fp);
                    check(member.value_span, Holds::RecordValue, &value_fp);
                });
            }
        }
        let need_arrays = format == Format::Toml
            && these
                .iter()
                .any(|(_, span)| matches!(span.holds, Holds::RecordValue | Holds::Value));
        let table = (format != Format::Json)
            .then(|| {
                if need_arrays {
                    entry_table_with_arrays(format, text, limits).ok()
                } else {
                    entry_table(format, text, limits).ok()
                }
            })
            .flatten();
        if let Some(table) = &table {
            walked = walked.max(table_held(table));
            for (position, row) in table.rows.iter().enumerate() {
                check(
                    row.key_span,
                    Holds::Key,
                    &|| match table.paths.segments(row.path).last() {
                        Some(Segment::Key(key)) => Some(fingerprint_text(key)),
                        _ => None,
                    },
                );
                let at = row.value_span.unwrap_or(row.key_span);
                let value_fp = || match table.values.get(position) {
                    Some(EntryValue::Scalar(value)) => Some(fingerprint(value)),
                    _ => None,
                };
                check(at, Holds::Value, &value_fp);
                check(at, Holds::RecordValue, &value_fp);
            }
        }
        for (index, span) in these {
            let found = held.contains(&index).then_some(span.span);
            placed.push((index, found, walked));
        }
    }
    placed
}

/// Roughly what building `table` held at its peak: the parse it was built
/// from, and the table.
fn table_held(table: &EntryTable) -> usize {
    table.parsed + crate::relation::adapters::table_bytes(table)
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

    fn text(value: &str) -> Value {
        Value::String(value.into())
    }

    fn read(extract: Extract, ordinal: u32) -> Span {
        Span::Read(ReadSpan::new(
            &Reading::new(extract, Limits::default()),
            ordinal,
            &text("x"),
        ))
    }

    /// A known span is kept, a `read:` value's found, each reader walking
    /// the text once; a text that is not UTF-8 places nothing.
    #[test]
    fn each_reader_walks_the_text_once() {
        let lines = Reading::new(Extract::Lines(LinesOpts::default()), Limits::default());
        let json = Reading::new(
            Extract::structured(Format::Json, "$.*").unwrap(),
            Limits::default(),
        );
        let text_of = |value| text(value);
        let bytes = b"{\"a\": \"x\",\n \"b\": \"y\"}\n";
        let spans = [
            Span::Read(ReadSpan::new(&json, 1, &text_of("y"))),
            Span::Bytes(BytesSpan::key(
                ByteSpan::new(1, 4),
                Format::Json,
                false,
                "a",
            )),
            Span::Read(ReadSpan::new(&lines, 1, &text_of("\"b\": \"y\"}"))),
            Span::Read(ReadSpan::new(&json, 0, &text_of("x"))),
        ];
        let before = read_span_walks();
        let (found, held) = find_spans(bytes, &spans);
        assert_eq!(read_span_walks() - before, 2);
        assert_eq!(
            found,
            [
                Some(ByteSpan::new(17, 20)),
                Some(ByteSpan::new(1, 4)),
                Some(ByteSpan::new(12, 21)),
                Some(ByteSpan::new(6, 9)),
            ]
        );
        assert!(held > bytes.len(), "{held}");
        let (found, held) = find_spans(b"{\"a\": \"\xff\"}", &spans);
        assert_eq!(found, [None, None, None, None]);
        assert_eq!(held, 0);
        assert_eq!(read_span_walks() - before, 2);
    }

    /// Values a file may hold, which placing tells apart by their
    /// fingerprints: each kind's value of one text (and of none, and of
    /// the bytes a boolean is taken as), texts that differ only in the
    /// zeros a last word is padded with, texts of every length to three
    /// words that differ in one byte at each place, and many short texts
    /// alike but for their digits.
    fn told_apart() -> Vec<Value> {
        let mut values = vec![Value::Null, Value::Bool(false), Value::Bool(true)];
        for text in ["", "1", "\0", "\u{1}"] {
            values.push(Value::String(text.into()));
            values.push(Value::Bytes(text.as_bytes().into()));
            if let Some(number) = crate::relation::Number::from_lexeme(text) {
                values.push(Value::Number(number));
            }
            if let Some(path) = crate::relation::path::RelPath::parse(text) {
                values.push(Value::Path(path));
            }
        }
        values
            .extend((1..=16).map(|zeros| Value::String(format!("a{}", "\0".repeat(zeros)).into())));
        for length in 2..=24 {
            values.push(Value::String("a".repeat(length).into()));
            for at in 0..length {
                for other in ['b', 'q', '\u{7f}'] {
                    let mut chars: Vec<char> = "a".repeat(length).chars().collect();
                    chars[at] = other;
                    values.push(Value::String(chars.into_iter().collect::<String>().into()));
                }
            }
        }
        values.extend((0..20_000).map(|index| Value::String(format!("v{index}").into())));
        values
    }

    /// No two of [`told_apart`]'s values share a fingerprint, and each of
    /// its 32 bits is set in about half of them: a fingerprint that kept
    /// less of a value, or mixed it less, would place a changed value where
    /// it was.
    #[test]
    fn fingerprints_tell_values_apart() {
        let values = told_apart();
        let prints: Vec<u32> = values.iter().map(fingerprint).collect();
        let mut seen = HashMap::new();
        for (value, print) in values.iter().zip(&prints) {
            if let Some(earlier) = seen.insert(*print, value) {
                panic!("{earlier:?} and {value:?} share {print:#x}");
            }
        }
        let count = prints.len();
        for bit in 0..32 {
            let set = prints.iter().filter(|print| *print >> bit & 1 == 1).count();
            assert!(
                set * 100 > count * 47 && set * 100 < count * 53,
                "bit {bit} is set in {set} of {count}"
            );
        }
    }

    /// The fingerprints of a few texts, as [`mix`] makes them. Each of its
    /// steps keeps two values that differ apart, but a weaker step would
    /// share a fingerprint between them one time in billions rather than
    /// one in four billion, which no test of [`told_apart`]'s size sees:
    /// these values pin the function [`fingerprints_tell_values_apart`]
    /// was measured on, so a change to it is made, and measured, on purpose.
    #[test]
    fn fingerprints_are_the_function_measured() {
        let prints: Vec<u32> = [
            Value::Null,
            text(""),
            text("a"),
            text("abcdefgh"),
            text("abcdefghijklmnopq"),
        ]
        .iter()
        .map(fingerprint)
        .collect();
        assert_eq!(
            prints,
            [
                0x0A57_C656,
                0x72AA_F6BB,
                0xE39C_13A5,
                0x6AB4_8417,
                0x90B6_1BA7
            ],
            "{prints:#x?}"
        );
    }

    /// Two spans are equal when they were read alike, by one reader or by
    /// two of one specification, and are of one row and value.
    #[test]
    fn spans_read_alike_are_equal() {
        let json = || Extract::structured(Format::Json, "$.*").unwrap();
        assert_eq!(read(json(), 1), read(json(), 1));
        assert_ne!(read(json(), 1), read(json(), 0));
        let other = Extract::structured(Format::Json, "$.a").unwrap();
        assert_ne!(read(json(), 1), read(other, 1));
        let reading = Reading::new(json(), Limits::default());
        assert_ne!(
            Span::Read(ReadSpan::new(&reading, 1, &text("x"))),
            Span::Read(ReadSpan::new(&reading, 1, &text("y")))
        );
        let key = Span::Bytes(BytesSpan::key(
            ByteSpan::new(0, 1),
            Format::Json,
            false,
            "a",
        ));
        assert_ne!(read(json(), 1), key);
    }

    /// A span keeps what its text held in no more room than a cell had for
    /// one before: three words.
    #[test]
    fn a_span_takes_no_more_room() {
        assert!(
            std::mem::size_of::<Option<Span>>() <= 3 * std::mem::size_of::<usize>(),
            "{}",
            std::mem::size_of::<Option<Span>>()
        );
    }

    /// A file as a cell was read from it, the file read again in ways that
    /// do or do not still hold the cell's text at its span, and the span.
    type Case = (&'static str, &'static [(&'static str, bool)], Span);

    /// The span of key `key` of a document of `format` at `start..end`.
    fn key_at(format: Format, (start, end): (usize, usize), key: &str) -> Span {
        Span::Bytes(BytesSpan::key(
            ByteSpan::new(start, end),
            format,
            false,
            key,
        ))
    }

    /// The span of value `value` of a document of `format` at
    /// `start..end`.
    fn value_at(format: Format, (start, end): (usize, usize), value: &Value) -> Span {
        Span::Bytes(BytesSpan::value(
            ByteSpan::new(start, end),
            format,
            false,
            value,
        ))
    }

    /// The span at `start..end` of `bytes` (a key's, a value's or a block
    /// scalar's at its key, of `format`) holding `value`, as `bytes` is now.
    fn found(bytes: &str, (start, end): (usize, usize), value: &Span) -> bool {
        let (found, _) = find_spans(bytes.as_bytes(), [value]);
        assert!(found[0].is_none_or(|at| at == ByteSpan::new(start, end)));
        found[0].is_some()
    }

    /// An entry's key or value is placed only where the file, read again,
    /// still holds it at its span: rewritten since (another key or value
    /// there, or the span inside or past another token), it is placed
    /// nowhere, in each format; a key past a BOM is where the BOM leaves
    /// it; and a YAML block scalar's value, at its key, is read from the
    /// document's entries.
    #[test]
    fn an_entry_is_placed_only_where_its_text_still_holds_it() {
        let number = |lexeme| Value::Number(crate::relation::Number::from_lexeme(lexeme).unwrap());
        let cases: [Case; 10] = [
            (
                "{\"a\":\"x\"}",
                &[
                    ("{\"b\":\"x\"}", false),
                    ("{\"a\":\"q\"}", true),
                    ("[\"a\"]", false),
                ],
                key_at(Format::Json, (1, 4), "a"),
            ),
            (
                "\u{feff}{\"a\":\"x\"}",
                &[("\u{feff}{\"a\":\"q\"}", true), ("{\"a\":\"x\"}", false)],
                key_at(Format::Json, (4, 7), "a"),
            ),
            (
                "{\"a\":\"x\"}",
                &[
                    ("{\"a\":\"q\"}", false),
                    ("{\"b\":\"x\"}", true),
                    ("{\"a\":1}", false),
                ],
                value_at(Format::Json, (5, 8), &text("x")),
            ),
            (
                "{\"a\":1.10}",
                &[("{\"a\":1.1 }", false), ("{\"a\":\"1.1\"}", false)],
                value_at(Format::Json, (5, 9), &number("1.10")),
            ),
            (
                "{\"a\":1}",
                &[("{\"a\":12}", false), ("{\"b\":1}", true)],
                value_at(Format::Json, (5, 6), &number("1")),
            ),
            (
                "a: x\n",
                &[("b: x\n", false), ("a: q\n", true), ("a:: x\n", false)],
                key_at(Format::Yaml, (0, 1), "a"),
            ),
            (
                "a: x\n",
                &[
                    ("a: q\n", false),
                    ("b: x\n", true),
                    ("a: x y\n", false),
                    ("a: x\n  y\n", false),
                ],
                value_at(Format::Yaml, (3, 4), &text("x")),
            ),
            (
                "a: |\n  x\n",
                &[
                    ("a: |\n  q\n", false),
                    ("a: >\n  x\n", true),
                    ("b: |\n  x\n", true),
                ],
                value_at(Format::Yaml, (0, 1), &text("x\n")),
            ),
            (
                "a = \"x\"\n",
                &[("b = \"x\"\n", false), ("a = \"q\"\n", true)],
                key_at(Format::Toml, (0, 1), "a"),
            ),
            (
                "a = \"x\"\n",
                &[
                    ("a = \"q\"\n", false),
                    ("a = 'x'\n", true),
                    ("a = \"x\"\n", true),
                ],
                value_at(Format::Toml, (4, 7), &text("x")),
            ),
        ];
        for (was, now, span) in cases {
            let bytes = span.bytes().unwrap();
            let at = (bytes.start, bytes.end);
            assert!(found(was, at, &span), "{span:?} {was:?}");
            for (now, held) in now {
                assert_eq!(found(now, at, &span), *held, "{span:?} {now:?}");
            }
        }
    }

    #[test]
    fn find_spans_with_limits_verifies_toml_array_value_and_honors_custom_limits() {
        let toml = "[[items]]\nname = \"alpha\"\n";
        let start = toml.find("\"alpha\"").unwrap();
        let end = start + "\"alpha\"".len();
        // Both `Holds::Value` (from `bound:` inside a TOML array) and
        // `Holds::RecordValue` must descend TOML arrays when verifying spans.
        let val_span = Span::Bytes(BytesSpan::value(
            ByteSpan::new(start, end),
            Format::Toml,
            false,
            &text("alpha"),
        ));
        let (found, _) = find_spans_with_limits(toml.as_bytes(), [&val_span], &Limits::default());
        assert_eq!(found, vec![Some(ByteSpan::new(start, end))]);

        // Tight custom `RowsPerExtractor` limit prevents verifying past the limit
        // (proving `still_held` honors the passed `limits` rather than hardcoded defaults).
        let many_toml = "a = \"1\"\nb = \"2\"\nc = \"3\"\n";
        let c_start = many_toml.find("\"3\"").unwrap();
        let c_end = c_start + 3;
        let c_span = Span::Bytes(BytesSpan::value(
            ByteSpan::new(c_start, c_end),
            Format::Toml,
            false,
            &text("3"),
        ));
        let tight = Limits::default()
            .with(crate::relation::LimitKind::RowsPerExtractor, 1)
            .unwrap();
        let (found_tight, _) = find_spans_with_limits(many_toml.as_bytes(), [&c_span], &tight);
        assert_eq!(found_tight, vec![None]);
        let (found_default, _) =
            find_spans_with_limits(many_toml.as_bytes(), [&c_span], &Limits::default());
        assert_eq!(found_default, vec![Some(ByteSpan::new(c_start, c_end))]);
    }
}
