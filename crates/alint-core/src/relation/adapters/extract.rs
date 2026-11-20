//! The value-bearing wrapper over today's [`Extract`], and where the values
//! it read are written ([`locate`]).

use std::collections::HashMap;

use regex::Regex;

use crate::extract::{
    ByteSpan, EntryValue, Extract, ExtractFailure, JsonSyntax, extract_values_bounded_typed,
    is_non_literal,
};
use crate::relation::adapter::{
    Adapter, AdapterError, AdapterNote, AdapterOutput, check_row_count,
};
use crate::relation::key_paths::{EntryAt, EntryTable, PathId};
use crate::relation::limits::{LimitExceeded, LimitKind, Limits};
use crate::relation::pointer::Segment;
use crate::relation::resource::Resource;
use crate::relation::row::{Row, RowState};
use crate::relation::value::Value;
use crate::structured_format::Format;

use super::entry_values::entry_table;
use crate::relation::eval::fingerprint_text;

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
/// Its rows carry no span: where a relation's `read:` value is written is
/// found only for a value a finding places, by reading its file again
/// ([`find_spans`](crate::relation::eval::find_spans)), so reading costs
/// what it costs `cross_file`.
#[derive(Debug, Clone)]
pub struct ExtractAdapter {
    extract: Extract,
    id: String,
}

impl ExtractAdapter {
    #[must_use]
    pub fn new(extract: Extract) -> Self {
        let id = spec_id(&extract);
        Self { extract, id }
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
        let values =
            extract_values_bounded_typed(&self.extract, &text, max).map_err(
                |error| match error {
                    ExtractFailure::Parse(message) => AdapterError::Parse(message),
                    ExtractFailure::Limit { limit, reached } => {
                        AdapterError::Limit(LimitExceeded {
                            kind: LimitKind::RowsPerExtractor,
                            limit,
                            reached,
                        })
                    }
                },
            )?;

        let mut rows = Vec::new();
        let mut notes = Vec::new();
        let skip_non_literal = !matches!(self.extract, Extract::WholeFile);
        for value in values.strings {
            if skip_non_literal && is_non_literal(&value) {
                notes.push(AdapterNote::NonLiteral { value });
                continue;
            }
            check_row_count(limits, rows.len() + 1)?;
            rows.push(Row::new(
                resource.clone(),
                None,
                u32::try_from(rows.len()).expect("the row limit is far below u32::MAX"),
                RowState::Value(Value::String(value.into())),
            ));
        }
        Ok(AdapterOutput {
            rows,
            non_strings: values.non_strings,
            notes,
        })
    }
}

/// Where the rows `asked` are written that [`ExtractAdapter`] read with
/// `extract` from a file whose text is now `text`: `spans[i]` is the span of
/// the row `asked[i]`, an ordinal (its place among the rows the adapter
/// emitted, which skip non-literal values) and the fingerprint of the value
/// it held, each found in one walk of `text`. A `lines:` value's span is its trimmed line, a
/// `regex:` value's its capture, and a JSON, YAML or TOML value an object
/// member holds, its value's (its key's for a YAML block scalar), as an
/// `entries:` part places it. A row whose value the text no longer holds
/// (the file changed since it was read), a `whole_file` value, a value in an
/// array, the document root, a repeated key's value (the tree holds only one
/// of them), and a value of a document whose entries cannot be walked within
/// `limits` have none.
///
/// Also the most the walk held at once besides `text` (a structured
/// document's tree, and then, the tree let go, the parse its entries are
/// read from and the entries, when a value asked for is an object
/// member's), which the caller takes from the run's bytes: it held them
/// only while it ran.
pub(crate) fn locate(
    extract: &Extract,
    text: &str,
    asked: &[(u32, u32)],
    limits: &Limits,
) -> (Vec<Option<ByteSpan>>, usize) {
    if asked.is_empty() {
        return (Vec::new(), 0);
    }
    WALKS.with(|walks| walks.set(walks.get() + 1));
    let mut rows = Rows {
        asked: asked
            .iter()
            .map(|&(ordinal, held)| (ordinal, (held, None)))
            .collect(),
        next: 0,
        last: asked
            .iter()
            .map(|(ordinal, _)| *ordinal)
            .max()
            .unwrap_or_default(),
    };
    let mut held = 0;
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
                let at = start + (line.len() - line.trim_start().len());
                if !rows.row(trimmed, || Some(ByteSpan::new(at, at + trimmed.len()))) {
                    break;
                }
            }
        }
        Extract::Regex(pattern) => {
            if let Ok(regex) = Regex::new(pattern) {
                for matched in regex
                    .captures_iter(text)
                    .filter_map(|captures| captures.get(1))
                {
                    let span = || Some(ByteSpan::new(matched.start(), matched.end()));
                    if !rows.row(matched.as_str(), span) {
                        break;
                    }
                }
            }
        }
        Extract::Structured(structured) => {
            held = members(structured.format, &structured.path, text, &mut rows, limits);
        }
        Extract::WholeFile => {}
    }
    (
        asked
            .iter()
            .map(|(ordinal, held)| match rows.asked[ordinal] {
                (found, span) if found == *held => span,
                _ => None,
            })
            .collect(),
        held,
    )
}

/// The rows [`locate`] is asked for, by ordinal, each with the fingerprint
/// of its value and its span once it is found, as it numbers the rows the
/// adapter emitted.
struct Rows {
    asked: HashMap<u32, (u32, Option<ByteSpan>)>,
    /// The next row's ordinal.
    next: u32,
    /// The last row asked for.
    last: u32,
}

impl Rows {
    /// Number a string value the reader read, whose span `span` gives, as
    /// the adapter does: a non-literal value is no row. A row asked for is
    /// placed only when it is the value it was ([`fingerprint_text`]).
    /// Whether a later row is asked for.
    fn row(&mut self, value: &str, span: impl FnOnce() -> Option<ByteSpan>) -> bool {
        if is_non_literal(value) {
            return true;
        }
        if let Some((held, slot)) = self.asked.get_mut(&self.next)
            && *held == fingerprint_text(value)
        {
            *slot = span();
        }
        self.next += 1;
        self.next <= self.last
    }
}

std::thread_local! {
    /// How many times [`locate`] has walked a text on this thread.
    static WALKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many times [`locate`] has walked a text to place `read:` values on
/// this thread. Test only.
pub(crate) fn walks() -> usize {
    WALKS.with(std::cell::Cell::get)
}

/// [`locate`] for a structured query over `text` parsed as `format`: each
/// string row asked of `rows` is placed at the one object member that holds
/// it, or nowhere. The most it held at once.
fn members(
    format: Format,
    path: &serde_json_path::JsonPath,
    text: &str,
    rows: &mut Rows,
    limits: &Limits,
) -> usize {
    // What the parse held besides the tree: a JSONC text's stripped copy,
    // TOML's tokens.
    let Ok((tree, parsing)) = format.parse_held(text) else {
        return 0;
    };
    // The string rows asked for, as the adapter numbered them.
    let mut asked: Vec<(u32, &serde_json::Value)> = Vec::new();
    for node in path.query(&tree) {
        let serde_json::Value::String(value) = node else {
            continue;
        };
        let ordinal = rows.next;
        let mut same = false;
        let more = rows.row(value, || {
            same = true;
            None
        });
        if same {
            asked.push((ordinal, node));
        }
        if !more {
            break;
        }
    }
    let (paths, tree_bytes) = key_paths(&tree, &asked);
    let tree_bytes = tree_bytes + parsing;
    let ordinals: Vec<u32> = asked.iter().map(|(ordinal, _)| *ordinal).collect();
    // The tree is let go before the entries are walked: the paths name
    // what it was asked for.
    drop(tree);
    // Only an object member's value is placed, from the document's entries,
    // which only JSON, YAML and TOML give.
    let paths: Vec<Option<Vec<Segment>>> = paths
        .into_iter()
        .map(|path| path.filter(|path| matches!(path.last(), Some(Segment::Key(_)))))
        .collect();
    if paths.iter().all(Option::is_none)
        || !matches!(format, Format::Json | Format::Yaml | Format::Toml)
    {
        return tree_bytes;
    }
    // A JSON value is found in the document's syntax tree, along its path
    // alone, without the entries every member would make: the tree is read
    // without its guards, which the tree's parse passed.
    if format == Format::Json {
        let Ok(syntax) = JsonSyntax::parsed(text) else {
            return tree_bytes;
        };
        #[cfg(test)]
        tests::TABLES.with(|tables| tables.set(tables.get() + 1));
        for (ordinal, span) in ordinals.iter().zip(syntax.member_spans(&paths)) {
            if let Some((_, slot)) = rows.asked.get_mut(ordinal) {
                *slot = span;
            }
        }
        return tree_bytes.max(syntax.held());
    }
    // The entries' spans, without their values (which TOML's walk alone
    // needs for its values' spans).
    let entries = if format == Format::Toml {
        entry_table(format, text, limits).ok()
    } else {
        let max = limits.get(LimitKind::RowsPerExtractor);
        crate::extract::entry_table(format, text, max).ok()
    };
    let Some(entries) = entries else {
        return tree_bytes;
    };
    #[cfg(test)]
    tests::TABLES.with(|tables| tables.set(tables.get() + 1));
    // Each path's entries: how many there are (more than one for a repeated
    // key), and the first.
    let ids: Vec<Option<PathId>> = paths
        .iter()
        .map(|path| {
            path.as_ref()?.iter().try_fold(PathId::ROOT, |at, segment| {
                entries.paths.get(at, segment.clone())
            })
        })
        .collect();
    let mut held: HashMap<PathId, (usize, Option<EntryAt>)> =
        ids.iter().flatten().map(|id| (*id, (0, None))).collect();
    for row in &entries.rows {
        if let Some((count, first)) = held.get_mut(&row.path) {
            *count += 1;
            first.get_or_insert(*row);
        }
    }
    for (ordinal, id) in ordinals.iter().zip(&ids) {
        let span = match id.and_then(|id| held.get(&id)) {
            Some((1, Some(row))) => Some(row.value_span.unwrap_or(row.key_span)),
            _ => None,
        };
        if let Some((_, slot)) = rows.asked.get_mut(ordinal) {
            *slot = span;
        }
    }
    // The parse the entries were read from was held with them.
    tree_bytes.max(entries.parsed + table_bytes(&entries))
}

/// One step of a key path, as a walk of a tree reads it.
#[derive(Clone, Copy)]
enum Step<'t> {
    Key(&'t str),
    Index(usize),
}

/// The key path of each of `asked`'s nodes in `tree` (none for a node it
/// does not hold), found in one walk, and roughly what the tree holds on the
/// heap.
fn key_paths(
    tree: &serde_json::Value,
    asked: &[(u32, &serde_json::Value)],
) -> (Vec<Option<Vec<Segment>>>, usize) {
    let mut paths = vec![None; asked.len()];
    // The nodes asked for, by address, each with its place in `asked`.
    let mut wanted: Vec<(*const serde_json::Value, usize)> = asked
        .iter()
        .enumerate()
        .map(|(index, (_, node))| (std::ptr::from_ref(*node), index))
        .collect();
    wanted.sort_unstable();
    let mut bytes = 0;
    let mut path: Vec<Step<'_>> = Vec::new();
    let mut pending: Vec<(&serde_json::Value, usize, Option<Step<'_>>)> = vec![(tree, 0, None)];
    while let Some((node, depth, step)) = pending.pop() {
        path.truncate(depth.saturating_sub(1));
        path.extend(step);
        let address = std::ptr::from_ref(node);
        let first = wanted.partition_point(|(asked, _)| *asked < address);
        for (_, index) in wanted[first..]
            .iter()
            .take_while(|(asked, _)| *asked == address)
        {
            paths[*index] = Some(
                path.iter()
                    .map(|step| match step {
                        Step::Key(key) => Segment::Key((*key).into()),
                        Step::Index(index) => Segment::Index(*index),
                    })
                    .collect(),
            );
        }
        bytes += std::mem::size_of::<serde_json::Value>();
        match node {
            serde_json::Value::String(value) => bytes += value.capacity(),
            serde_json::Value::Object(map) => {
                for (key, value) in map {
                    bytes += key.capacity() + crate::extract::TREE_MEMBER;
                    pending.push((value, depth + 1, Some(Step::Key(key))));
                }
            }
            serde_json::Value::Array(items) => {
                bytes += items.capacity().saturating_sub(items.len())
                    * std::mem::size_of::<serde_json::Value>();
                for (index, item) in items.iter().enumerate() {
                    pending.push((item, depth + 1, Some(Step::Index(index))));
                }
            }
            serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            }
        }
    }
    (paths, bytes)
}

/// Roughly what `entries`, a document's entries, hold on the heap: their
/// rows, their values and the values' text, and their key paths.
pub(crate) fn table_bytes(entries: &EntryTable) -> usize {
    let values: usize = entries
        .values
        .iter()
        .map(|value| match value {
            EntryValue::Scalar(value) => value.heap_bytes(),
            _ => 0,
        })
        .sum();
    entries.rows.capacity() * std::mem::size_of::<EntryAt>()
        + entries.values.capacity() * std::mem::size_of::<EntryValue>()
        + values
        + entries.paths.heap_bytes()
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

/// Whether `a` and `b` read alike: the same extractor and specification.
pub(crate) fn same_reading(a: &Extract, b: &Extract) -> bool {
    spec_id(a) == spec_id(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    std::thread_local! {
        /// How many entry tables [`members`] has walked on this thread.
        pub(super) static TABLES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    use crate::extract::{LinesOpts, NonStringValueKind};
    use crate::relation::eval::fingerprint;
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

    /// The rows `ordinals` of those [`ExtractAdapter`] reads with `extract`
    /// from `bytes`, as [`locate`] is asked for them: each with the
    /// fingerprint of its value.
    fn asked(extract: &Extract, bytes: &[u8], ordinals: &[u32]) -> Vec<(u32, u32)> {
        let output = ExtractAdapter::new(extract.clone())
            .extract(&resource("a"), bytes, &Limits::default())
            .unwrap();
        ordinals
            .iter()
            .map(|ordinal| {
                let row = &output.rows[*ordinal as usize];
                (*ordinal, fingerprint(row.value().unwrap()))
            })
            .collect()
    }

    /// Where `extract` places each row [`ExtractAdapter`] reads from `bytes`,
    /// asked one at a time.
    fn placed(extract: &Extract, bytes: &[u8]) -> Vec<Option<ByteSpan>> {
        let rows = ExtractAdapter::new(extract.clone())
            .extract(&resource("a"), bytes, &Limits::default())
            .map_or(0, |output| output.rows.len());
        let text = std::str::from_utf8(bytes).unwrap();
        (0..rows)
            .map(|ordinal| {
                let ordinal = u32::try_from(ordinal).unwrap();
                let asked = asked(extract, bytes, &[ordinal]);
                locate(extract, text, &asked, &Limits::default()).0[0]
            })
            .collect()
    }

    /// Where an `entries:` walk of the whole document places each string
    /// row a structured `extract` reads, as `read:` placed them when it
    /// walked every document it read: the oracle [`locate`] is held to.
    fn walked(extract: &Extract, text: &str) -> Vec<Option<ByteSpan>> {
        let Extract::Structured(structured) = extract else {
            unreachable!("a structured extract")
        };
        let tree = structured.format.parse(text).unwrap();
        let entries = entry_table(structured.format, text, &Limits::default()).unwrap();
        let document = super::super::entry_values::Document::new(&entries, &tree);
        structured
            .path
            .query(&tree)
            .iter()
            .filter(|node| node.as_str().is_some_and(|value| !is_non_literal(value)))
            .map(|node| {
                let at = document.path(node).filter(|at| *at != PathId::ROOT)?;
                let mut held = document.rows_at(at);
                let row = &entries.rows[held.next()?];
                held.next()
                    .is_none()
                    .then_some(row.value_span.unwrap_or(row.key_span))
            })
            .collect()
    }

    /// A structured row is placed where a walk of every entry of its
    /// document places it, though only the members asked for are looked
    /// up: arrays, objects in arrays, repeated keys, non-literal values,
    /// a query that matches one node twice, a BOM (JSON's and YAML's), a
    /// YAML block scalar and TOML tables.
    #[test]
    fn a_structured_row_is_placed_where_a_walk_of_its_document_places_it() {
        let structured = |format, query| Extract::structured(format, query).unwrap();
        let json = "{\"a\": \"x\", \"b\": 1, \"c\": [\"y\", {\"d\": \"z\"}], \"e\": \"${E}\",\n \
                    \"f\": {\"g\": \"w\", \"g\": \"v\"}, \"h\": {\"i\": {\"j\": \"u\"}}}";
        let yaml =
            "\u{feff}a: x\nb: |\n  block\nc: [y, {d: z}]\nh:\n  i: {j: u}\nk: '${K}'\nl: \"t\"\n";
        let toml = "a = \"x\"\n[t]\nb = \"y\"\nc = [\"z\"]\n[t.u]\nd = 'w'\n[[v]]\ne = \"s\"\n";
        let marked = format!("\u{feff}{json}");
        for (format, text) in [
            (Format::Json, json),
            (Format::Json, &marked),
            (Format::Yaml, yaml),
            (Format::Toml, toml),
        ] {
            for query in ["$..*", "$.*", "$['a','a']", "$..d", "$.a"] {
                let extract = structured(format, query);
                let spans = placed(&extract, text.as_bytes());
                assert_eq!(spans, walked(&extract, text), "{format:?} {query}");
            }
        }
        // Every member's value is placed in each document, as itself or
        // quoted (a YAML block scalar at its key).
        let extract = structured(Format::Json, "$..*");
        let spans = placed(&extract, json.as_bytes());
        assert_eq!(spans.iter().flatten().count(), 3, "{spans:?}");
    }

    /// Each span [`locate`] finds spells its row's value in the file, as
    /// written or quoted (a YAML block scalar's, its key); a structured
    /// value of a format other than JSON, YAML and TOML has none.
    #[test]
    fn each_span_spells_its_value() {
        let mut found = 0;
        for (extract, bytes) in extracts() {
            let Ok(text) = std::str::from_utf8(bytes) else {
                continue;
            };
            let output = ExtractAdapter::new(extract.clone())
                .extract(&resource("a"), bytes, &Limits::default())
                .unwrap();
            let ordinals: Vec<u32> = (0..output.rows.len())
                .map(|ordinal| u32::try_from(ordinal).unwrap())
                .collect();
            let asked = asked(&extract, bytes, &ordinals);
            let (spans, _) = locate(&extract, text, &asked, &Limits::default());
            assert_eq!(spans.len(), output.rows.len());
            for (row, span) in output.rows.iter().zip(spans) {
                let (Some(span), Some(Value::String(value))) = (span, row.value()) else {
                    continue;
                };
                found += 1;
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
        assert_eq!(found, 10);
    }

    /// Where each reader places its values: a trimmed line, a capture, and
    /// the value an object member holds; not a value in an array, nor a
    /// value of a repeated key, nor a `whole_file` value. A non-literal
    /// value is no row, so the rows after it keep their ordinals.
    #[test]
    fn each_reader_places_its_values() {
        let spans = |extract: Extract, bytes: &[u8]| -> Vec<Option<(usize, usize)>> {
            placed(&extract, bytes)
                .into_iter()
                .map(|span| span.map(|span| (span.start, span.end)))
                .collect()
        };
        assert_eq!(
            spans(
                Extract::Lines(LinesOpts::default()),
                b"# c\r\n  ab \r\n${X}\nc"
            ),
            [Some((7, 9)), Some((17, 18))]
        );
        assert_eq!(
            spans(
                Extract::Regex(r"(?m)^v=(\S+)".into()),
                b"v=ab\nv=${V}\nv=c\n"
            ),
            [Some((2, 4)), Some((14, 15))]
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
        assert_eq!(spans(Extract::WholeFile, b"a\n"), [None]);
        // A file that no longer holds the row, or no longer parses, places
        // nothing.
        let x = fingerprint_text("x");
        let text = "{\"a\": \"x\"}";
        assert_eq!(
            locate(&json("$.a"), text, &[(1, x)], &Limits::default()).0,
            [None]
        );
        assert_eq!(
            locate(&json("$.a"), "{", &[(0, x)], &Limits::default()).0,
            [None]
        );
    }

    /// A row is placed only where the file, read again, still holds its
    /// value: a file rewritten since the row was read, whose row of that
    /// ordinal holds another value now, places it nowhere, for each reader.
    #[test]
    fn a_row_whose_value_changed_is_placed_nowhere() {
        let x = fingerprint_text("x");
        let json = Extract::structured(Format::Json, "$.*").unwrap();
        let yaml = Extract::structured(Format::Yaml, "$.*").unwrap();
        let toml = Extract::structured(Format::Toml, "$.*").unwrap();
        let lines = Extract::Lines(LinesOpts::default());
        let regex = Extract::Regex("v=(.)".into());
        for (extract, was, is) in [
            (&json, "{\"a\": \"x\"}", "{\"a\": \"q\"}"),
            (&yaml, "a: x\n", "a: q\n"),
            (&toml, "a = \"x\"\n", "a = \"q\"\n"),
            (&lines, "x\n", "q\n"),
            (&regex, "v=x\n", "v=q\n"),
        ] {
            let found = |text| locate(extract, text, &[(0, x)], &Limits::default()).0;
            assert!(found(was)[0].is_some(), "{extract:?}");
            assert_eq!(found(is), [None], "{extract:?}");
        }
    }

    /// Placing a JSON value walks its document's entries from the tree it
    /// parsed it to, without the guards parsing the text whole once more:
    /// the tree's parse passed them.
    #[test]
    fn placing_a_json_value_parses_its_text_once() {
        let guards = || crate::extract::tests::JSON_GUARDS.with(std::cell::Cell::get);
        let text = "{\"a\": {\"b\": \"x\"}}";
        let extract = Extract::structured(Format::Json, "$.a.b").unwrap();
        let before = guards();
        assert_eq!(
            placed(&extract, text.as_bytes()),
            [Some(ByteSpan::new(12, 15))]
        );
        assert_eq!(guards() - before, 0);
        // An `entries:` part's read is guarded.
        entry_table(Format::Json, text, &Limits::default()).unwrap();
        assert_eq!(guards() - before, 1);
    }

    /// One walk places every row asked of it, in the order asked, as it
    /// places each alone; it walks the document's entries only for an
    /// object member's value, and what it held counts them.
    #[test]
    fn one_walk_places_every_row_asked() {
        let text = "{\"a\": [\"x\", \"y\"], \"b\": \"z\", \"c\": \"x\"}";
        let extract = Extract::structured(Format::Json, "$..*").unwrap();
        let alone = placed(&extract, text.as_bytes());
        assert_eq!(alone.len(), 4);
        let tables = || TABLES.with(std::cell::Cell::get);
        let (before, tables_before) = (walks(), tables());
        let asked_for = asked(&extract, text.as_bytes(), &[3, 0, 2, 3]);
        let (together, held) = locate(&extract, text, &asked_for, &Limits::default());
        assert_eq!((walks() - before, tables() - tables_before), (1, 1));
        assert_eq!(together, [alone[3], alone[0], alone[2], alone[3]]);
        assert!(held > 3 * text.len(), "{held}");
        // Values in an array alone: the entries are not walked.
        let in_array: Vec<u32> = (0..4)
            .filter(|ordinal| alone[*ordinal as usize].is_none())
            .collect();
        assert_eq!(in_array.len(), 2);
        let in_array = asked(&extract, text.as_bytes(), &in_array);
        let (spans, tree) = locate(&extract, text, &in_array, &Limits::default());
        assert_eq!(spans, [None, None]);
        assert!(tree > 0);
        assert_eq!((walks() - before, tables() - tables_before), (2, 1));
        // Nothing asked: no walk.
        assert_eq!(
            locate(&extract, text, &[], &Limits::default()),
            (Vec::new(), 0)
        );
        assert_eq!(walks() - before, 2);
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
