//! The value-bearing wrapper over today's [`Extract`].

use std::borrow::Cow;

use regex::Regex;

use crate::extract::{
    ByteSpan, Extract, ExtractFailure, ExtractedValues, NonStringValueKind,
    extract_values_bounded_typed, is_non_literal,
};
use crate::relation::adapter::{
    Adapter, AdapterError, AdapterNote, AdapterOutput, check_row_count,
};
use crate::relation::key_paths::{EntryTable, PathId};
use crate::relation::limits::{LimitExceeded, LimitKind, Limits};
use crate::relation::resource::Resource;
use crate::relation::row::{Row, RowState};
use crate::relation::value::Value;
use crate::structured_format::Format;

use super::entry_values::{Document, entry_table};

/// Wraps today's string-only extraction as a relation adapter.
///
/// It reproduces `cross_file`'s decoding contract: lossy UTF-8, capture group 1
/// for `regex:`, trimmed non-comment lines for `lines:`, and the non-literal
/// skip for every extractor but `whole_file`. Non-string matches stay typed in
/// [`AdapterOutput::non_strings`] so the consuming rule can render the existing
/// one-finding summary, and skipped computed values stay in
/// [`AdapterOutput::notes`] so the rule can redact and report them.
///
/// The adapter id includes the extractor's specification, so two different
/// queries over the same file never share a decode cache entry.
///
/// The wrapper is generic. `cross_file` filters a target `regex:`'s captures
/// by syntax (Dockerfile `FROM`, `__version__`, and similar), so its reader
/// takes those captures from the target-syntax filter instead of this adapter.
///
/// [`Self::with_spans`] also places each value ([`Row::span`]), for a
/// relation's `read:`; [`Self::new`] reads as `cross_file` always has.
#[derive(Debug, Clone)]
pub struct ExtractAdapter {
    extract: Extract,
    /// Whether each row carries its value's byte span, where it has one.
    spans: bool,
    id: String,
}

impl ExtractAdapter {
    #[must_use]
    pub fn new(extract: Extract) -> Self {
        let id = spec_id(&extract);
        Self {
            extract,
            spans: false,
            id,
        }
    }

    /// An adapter that reads the values [`Self::new`]'s does, in its order,
    /// and gives each row its value's byte span in the file: a `lines:`
    /// value's trimmed line, a `regex:` value's capture, and a JSON, YAML or
    /// TOML value an object member holds, its value's span (the key's for a
    /// YAML block scalar), as an `entries:` part places it. A `whole_file`
    /// value, a value in an array, and a value of a file that is not UTF-8
    /// (read lossily) have none, and a structured file whose entries cannot be
    /// walked within the rows limit places none of its values. Its id is its
    /// own, so the two never share a decode.
    #[must_use]
    pub fn with_spans(extract: Extract) -> Self {
        let id = format!("{}:spans", spec_id(&extract));
        Self {
            extract,
            spans: true,
            id,
        }
    }

    #[must_use]
    pub fn extractor(&self) -> &Extract {
        &self.extract
    }
}

impl Adapter for ExtractAdapter {
    fn id(&self) -> &str {
        &self.id
    }

    fn redacts_by_default(&self) -> bool {
        self.extract.redacts_by_default()
    }

    fn extract(
        &self,
        resource: &Resource,
        bytes: &[u8],
        limits: &Limits,
    ) -> Result<AdapterOutput, AdapterError> {
        // `cross_file` reads every file through `String::from_utf8_lossy`;
        // the wrapper must stay byte-compatible with it.
        let text = String::from_utf8_lossy(bytes);
        let max = limits.get(LimitKind::RowsPerExtractor);
        // A span indexes the file's bytes, which a lossy text does not.
        let (values, spans) = match (&self.extract, &text) {
            (
                Extract::Lines(_) | Extract::Regex(_) | Extract::Structured(_),
                Cow::Borrowed(text),
            ) if self.spans => located(&self.extract, text, max, limits)?,
            _ => (
                extract_values_bounded_typed(&self.extract, &text, max).map_err(extract_failure)?,
                Vec::new(),
            ),
        };

        let mut rows = Vec::new();
        let mut notes = Vec::new();
        let skip_non_literal = !matches!(self.extract, Extract::WholeFile);
        for (index, value) in values.strings.into_iter().enumerate() {
            if skip_non_literal && is_non_literal(&value) {
                notes.push(AdapterNote::NonLiteral { value });
                continue;
            }
            check_row_count(limits, rows.len() + 1)?;
            let mut row = Row::new(
                resource.clone(),
                None,
                u32::try_from(rows.len()).expect("the row limit is far below u32::MAX"),
                RowState::Value(Value::String(value.into())),
            );
            if let Some(span) = spans.get(index).copied().flatten() {
                row = row.with_span(span);
            }
            rows.push(row);
        }
        Ok(AdapterOutput {
            rows,
            non_strings: values.non_strings,
            notes,
        })
    }
}

fn extract_failure(error: ExtractFailure) -> AdapterError {
    match error {
        ExtractFailure::Parse(message) => AdapterError::Parse(message),
        ExtractFailure::Limit { limit, reached } => AdapterError::Limit(LimitExceeded {
            kind: LimitKind::RowsPerExtractor,
            limit,
            reached,
        }),
    }
}

/// The values a `lines:`, `regex:` or structured extract reads from `text`
/// (UTF-8, so its offsets are the file's), as
/// [`extract_values_bounded_typed`] reads them, each string with its span
/// when it has one (`spans[i]` is `strings[i]`'s); a structured format other
/// than JSON, YAML and TOML places none. At most `max` values, failing as
/// that function fails past it.
fn located(
    extract: &Extract,
    text: &str,
    max: usize,
    limits: &Limits,
) -> Result<(ExtractedValues, Vec<Option<ByteSpan>>), AdapterError> {
    let mut values = ExtractedValues::default();
    let mut spans = Vec::new();
    // Room for one more value, or the limit error that function gives.
    let room = |values: &ExtractedValues| {
        if values.strings.len() + values.non_strings.len() >= max {
            Err(AdapterError::Limit(LimitExceeded {
                kind: LimitKind::RowsPerExtractor,
                limit: max,
                reached: max.saturating_add(1),
            }))
        } else {
            Ok(())
        }
    };
    match extract {
        Extract::Lines(options) => {
            for (start, line) in crate::physical_lines(text) {
                let trimmed = line.trim();
                if trimmed.is_empty()
                    || (!options.comment.is_empty()
                        && trimmed.starts_with(options.comment.as_str()))
                {
                    continue;
                }
                room(&values)?;
                let at = start + (line.len() - line.trim_start().len());
                values.strings.push(trimmed.to_string());
                spans.push(Some(ByteSpan::new(at, at + trimmed.len())));
            }
        }
        Extract::Regex(pattern) => {
            let regex = Regex::new(pattern)
                .map_err(|error| AdapterError::Parse(format!("bad regex: {error}")))?;
            for matched in regex
                .captures_iter(text)
                .filter_map(|captures| captures.get(1))
            {
                room(&values)?;
                values.strings.push(matched.as_str().to_string());
                spans.push(Some(ByteSpan::new(matched.start(), matched.end())));
            }
        }
        Extract::Structured(structured) => {
            let tree = structured.format.parse(text).map_err(AdapterError::Parse)?;
            let nodes = structured.path.query(&tree).all();
            // The entries place each string an object member holds, walked
            // only when a string matched; a file they cannot be walked in,
            // within the rows limit, places none.
            let entries = (nodes.iter().any(|node| node.is_string())
                && matches!(
                    structured.format,
                    Format::Json | Format::Yaml | Format::Toml
                ))
            .then(|| entry_table(structured.format, text, limits).ok())
            .flatten();
            let document = entries
                .as_ref()
                .map(|entries| Document::new(entries, &tree));
            for node in nodes {
                room(&values)?;
                let kind = match node {
                    serde_json::Value::String(value) => {
                        values.strings.push(value.clone());
                        spans.push(
                            document
                                .as_ref()
                                .zip(entries.as_ref())
                                .and_then(|(document, entries)| place(document, entries, node)),
                        );
                        continue;
                    }
                    serde_json::Value::Null => NonStringValueKind::Null,
                    serde_json::Value::Bool(_) => NonStringValueKind::Boolean,
                    serde_json::Value::Number(_) => NonStringValueKind::Number,
                    serde_json::Value::Array(_) => NonStringValueKind::Array,
                    serde_json::Value::Object(_) => NonStringValueKind::Object,
                };
                values.non_strings.push(kind);
            }
        }
        Extract::WholeFile => unreachable!("a whole file is read without a span"),
    }
    Ok((values, spans))
}

/// Where `node`, a value of the tree `document` indexes, is written: the
/// value's span of the one entry that holds it (its key's for a YAML block
/// scalar, as an `entries:` row's), or `None` for a value in an array, the
/// root, and a repeated key's, whose value the tree holds only one of.
fn place(
    document: &Document<'_>,
    entries: &EntryTable,
    node: &serde_json::Value,
) -> Option<ByteSpan> {
    let at = document.path(node).filter(|at| *at != PathId::ROOT)?;
    let mut held = document.rows_at(at);
    let row = &entries.rows[held.next()?];
    held.next()
        .is_none()
        .then_some(row.value_span.unwrap_or(row.key_span))
}

fn spec_id(extract: &Extract) -> String {
    match extract {
        Extract::Structured(extract) => {
            format!("extract:structured:{:?}:{}", extract.format, extract.query)
        }
        Extract::Lines(options) => format!("extract:lines:{}", options.comment),
        Extract::Regex(pattern) => format!("extract:regex:{pattern}"),
        Extract::WholeFile => "extract:whole_file".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::{LinesOpts, NonStringValueKind};
    use crate::structured_format::Format;

    fn resource(path: &str) -> Resource {
        Resource::for_test(0, path)
    }

    fn run(extract: Extract, text: &str) -> Result<AdapterOutput, AdapterError> {
        ExtractAdapter::new(extract).extract(
            &resource("a.json"),
            text.as_bytes(),
            &Limits::default(),
        )
    }

    #[test]
    fn structured_strings_become_rows_and_non_strings_stay_typed() {
        let extract = Extract::structured(Format::Json, "$.items[*]").unwrap();
        let output = run(extract, r#"{"items":["a",1,null]}"#).unwrap();
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0].value(), Some(&Value::String("a".into())));
        assert_eq!(
            output.non_strings,
            [NonStringValueKind::Number, NonStringValueKind::Null]
        );
        assert_eq!(output.notes.len(), 0);
    }

    #[test]
    fn non_literal_values_are_notes_not_rows() {
        let extract = Extract::Regex(r"(?m)^v=(.*)$".to_string());
        let output = run(extract, "v=${VERSION}\nv=1.2.3\n").unwrap();
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0].value(), Some(&Value::String("1.2.3".into())));
        assert_eq!(
            output.notes,
            [AdapterNote::NonLiteral {
                value: "${VERSION}".into()
            }]
        );
    }

    #[test]
    fn whole_file_does_not_skip_non_literal_content() {
        let output = run(Extract::WholeFile, "${VERSION}\n").unwrap();
        assert_eq!(output.rows.len(), 1);
        assert_eq!(
            output.rows[0].value(),
            Some(&Value::String("${VERSION}\n".into()))
        );
        assert_eq!(output.notes.len(), 0);
    }

    #[test]
    fn lines_and_regex_keep_cross_files_value_shape() {
        let lines = Extract::Lines(LinesOpts::default());
        let output = run(lines, "# comment\n a \n\n").unwrap();
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0].value(), Some(&Value::String("a".into())));
    }

    #[test]
    fn parse_failures_and_limits_are_typed() {
        let extract = Extract::structured(Format::Json, "$.a").unwrap();
        assert!(matches!(run(extract, "{"), Err(AdapterError::Parse(_))));
        let limits = Limits::default()
            .with(LimitKind::RowsPerExtractor, 1)
            .unwrap();
        let extract = Extract::Regex(r"(?m)^(.*)$".to_string());
        let err = ExtractAdapter::new(extract)
            .extract(&resource("a.txt"), b"a\nb\n", &limits)
            .unwrap_err();
        assert!(matches!(
            err,
            AdapterError::Limit(LimitExceeded {
                kind: LimitKind::RowsPerExtractor,
                limit: 1,
                reached: 2
            })
        ));
    }

    #[test]
    fn different_specifications_have_different_ids() {
        let a = ExtractAdapter::new(Extract::structured(Format::Json, "$.a").unwrap());
        let b = ExtractAdapter::new(Extract::structured(Format::Json, "$.b").unwrap());
        let c = ExtractAdapter::new(Extract::Regex("a".into()));
        assert_ne!(a.id(), b.id());
        assert_ne!(b.id(), c.id());
    }

    #[test]
    fn sensitive_extracts_ask_the_consumer_to_redact() {
        let extract = Extract::structured(Format::Dotenv, "$.SECRET").unwrap();
        assert!(ExtractAdapter::new(extract).redacts_by_default());
        let extract = Extract::structured(Format::Json, "$.a").unwrap();
        assert!(!ExtractAdapter::new(extract).redacts_by_default());
    }

    /// The extracts a relation's `read:` builds, over texts that exercise
    /// each: comments, blank and CRLF lines, non-literal values, non-strings,
    /// arrays, a repeated key, a BOM and a byte that is not UTF-8.
    fn extracts() -> Vec<(Extract, &'static [u8])> {
        let structured = |format, query| Extract::structured(format, query).unwrap();
        vec![
            (
                Extract::Lines(LinesOpts::default()),
                b"# c\r\n  a  \r\n\r\n${X}\n\tb\n#\n",
            ),
            (
                Extract::Lines(LinesOpts {
                    comment: String::new(),
                }),
                b"# c\n a\n",
            ),
            (
                Extract::Regex(r"(?m)^v=(.*)$".into()),
                b"v=1\nv=${V}\n v=2\nv=3\n",
            ),
            (Extract::Regex(r"(x)?y".into()), b"y xy y"),
            (
                structured(Format::Json, "$.*"),
                b"{\"a\": \"x\", \"b\": 1, \"c\": [\"y\"], \"d\": null, \"e\": \"${E}\", \"a\": \"z\"}",
            ),
            (structured(Format::Json, "$.c[*]"), b"{\"c\": [\"y\", 2]}"),
            (
                structured(Format::Yaml, "$.*"),
                b"\xef\xbb\xbfa: x\nb: |\n  block\nc: [y]\nd: ~\n",
            ),
            (structured(Format::Toml, "$.t.*"), b"[t]\na = \"x\"\nb = 2\n"),
            (structured(Format::Dotenv, "$.A"), b"A=x\n"),
            (Extract::WholeFile, b"all\n"),
            (Extract::Lines(LinesOpts::default()), b"a\xff\nb\n"),
            (structured(Format::Json, "$.a"), b"{\"a\": \"\xff\"}"),
        ]
    }

    /// The values, non-strings and notes a row of `output` holds, without
    /// spans.
    fn read(output: &AdapterOutput) -> (Vec<(Option<Value>, u32)>, AdapterOutput) {
        let rows = output
            .rows
            .iter()
            .map(|row| (row.value().cloned(), row.ordinal))
            .collect();
        let rest = AdapterOutput {
            rows: Vec::new(),
            ..output.clone()
        };
        (rows, rest)
    }

    /// A `read:` reads what `cross_file` reads, and `cross_file`'s adapter
    /// places nothing: spans are only added.
    #[test]
    fn spans_change_no_value_and_cross_file_gets_none() {
        for (extract, bytes) in extracts() {
            let plain = ExtractAdapter::new(extract.clone());
            let placed = ExtractAdapter::with_spans(extract.clone());
            assert_ne!(plain.id(), placed.id());
            let limits = Limits::default();
            let plain = plain.extract(&resource("a"), bytes, &limits);
            let placed = placed.extract(&resource("a"), bytes, &limits);
            match (plain, placed) {
                (Ok(plain), Ok(placed)) => {
                    assert_eq!(read(&plain), read(&placed), "{extract:?}");
                    assert!(plain.rows.iter().all(|row| row.span.is_none()));
                    // Each span spells its value in the original bytes, quoted
                    // or not, or a YAML block scalar's key.
                    for row in &placed.rows {
                        if let (Some(span), Some(Value::String(value))) = (row.span, row.value()) {
                            let spelled = &bytes[span.as_range()];
                            let quoted = format!("\"{value}\"");
                            assert!(
                                [value.as_bytes(), quoted.as_bytes()].contains(&spelled)
                                    || (&**value, spelled) == ("block\n", b"b"),
                                "{extract:?}: {:?} at {span:?}",
                                String::from_utf8_lossy(spelled)
                            );
                        }
                    }
                }
                (plain, placed) => {
                    assert_eq!(
                        plain.map(|output| read(&output)),
                        placed.map(|output| read(&output)),
                        "{extract:?}"
                    );
                }
            }
        }
        // Past the rows limit, both fail alike.
        let limits = Limits::default()
            .with(LimitKind::RowsPerExtractor, 1)
            .unwrap();
        for (extract, bytes) in extracts() {
            let plain =
                ExtractAdapter::new(extract.clone()).extract(&resource("a"), bytes, &limits);
            let placed =
                ExtractAdapter::with_spans(extract.clone()).extract(&resource("a"), bytes, &limits);
            assert_eq!(
                plain.map(|output| read(&output)),
                placed.map(|output| read(&output)),
                "{extract:?}"
            );
        }
    }

    /// Where each reader places its values: a trimmed line, a capture, and
    /// the value an object member holds; not a value in an array, nor a
    /// value of a text that is not UTF-8.
    #[test]
    fn each_reader_places_its_values() {
        let spans = |extract: Extract, bytes: &[u8]| -> Vec<Option<(usize, usize)>> {
            ExtractAdapter::with_spans(extract)
                .extract(&resource("a"), bytes, &Limits::default())
                .unwrap()
                .rows
                .iter()
                .map(|row| row.span.map(|span| (span.start, span.end)))
                .collect()
        };
        assert_eq!(
            spans(Extract::Lines(LinesOpts::default()), b"# c\r\n  ab \r\nc"),
            [Some((7, 9)), Some((12, 13))]
        );
        assert_eq!(
            spans(Extract::Regex(r"(?m)^v=(\S+)".into()), b"v=ab\nw=1\nv=c\n"),
            [Some((2, 4)), Some((11, 12))]
        );
        let json = |query| Extract::structured(Format::Json, query).unwrap();
        assert_eq!(
            spans(json("$.*"), br#"{"a": "x", "b": ["y"]}"#),
            [Some((6, 9))]
        );
        assert_eq!(spans(json("$.b[*]"), br#"{"b": ["y"]}"#), [None]);
        // A repeated key: the tree holds one of its values, so neither is
        // placed.
        assert_eq!(spans(json("$.a"), br#"{"a": "x", "a": "y"}"#), [None]);
        let yaml = Extract::structured(Format::Yaml, "$.a").unwrap();
        assert_eq!(spans(yaml, b"a:  'x'\n"), [Some((4, 7))]);
        let toml = Extract::structured(Format::Toml, "$.t.a").unwrap();
        assert_eq!(spans(toml, b"[t]\na = \"x\"\n"), [Some((8, 11))]);
        assert_eq!(
            spans(Extract::Lines(LinesOpts::default()), b"a\xff\nb\n"),
            [None, None]
        );
    }

    #[test]
    fn lossy_decoding_matches_cross_file() {
        let extract = Extract::WholeFile;
        let output = ExtractAdapter::new(extract)
            .extract(&resource("a.txt"), &[0xff, b'a'], &Limits::default())
            .unwrap();
        assert_eq!(
            output.rows[0].value(),
            Some(&Value::String("\u{fffd}a".into()))
        );
    }
}
