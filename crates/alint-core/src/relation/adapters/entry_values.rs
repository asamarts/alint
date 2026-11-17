//! Value-bearing entry rows (ADR-0034 decision 5, design section 5): the
//! members of each object a relation's `entries:` selects, or the leaves
//! below it, each with its value, and the relation's `bound:` values.

use std::collections::HashMap;

use serde_json_path::JsonPath;

use crate::ByteSpan;
use crate::extract::{EntryRowsError, EntryValue, extract_entry_values_bounded};
use crate::relation::adapter::{Adapter, AdapterError, AdapterOutput, check_row_count};
use crate::relation::limits::{LimitExceeded, LimitKind, Limits};
use crate::relation::pointer::{Pointer, Segment};
use crate::relation::resource::Resource;
use crate::relation::row::{Row, RowState};
use crate::structured_format::Format;

use super::toml::TomlRowsAdapter;

/// The label prefix of a row that reports what an `at:` path selected when it
/// is not an object: `at:<JSONPath>`.
pub(crate) const AT_LABEL: &str = "at:";

/// The label prefix of a `bound:` row: `bound:<JSONPath>`.
pub(crate) const BOUND_LABEL: &str = "bound:";

/// What an entry's value holds when it is not a scalar, as an
/// [`RowState::Error`] of an entry row says it: a field that binds
/// `entry.value` (or the `bound:` path) turns it into an analysis failure,
/// and a key-only relation never looks.
pub(crate) const OBJECT: &str = "an object";
pub(crate) const ARRAY: &str = "an array";
pub(crate) const DATETIME: &str = "a TOML date or time";
pub(crate) const UNREADABLE: &str = "a YAML scalar its tag cannot read";
/// A number in an array: no entry row holds it, so its lexeme is unknown.
pub(crate) const ARRAY_NUMBER: &str = "a number in an array";
/// A TOML table in an array of tables, whose members the TOML walk does not
/// emit (arrays are values).
pub(crate) const ARRAY_TABLE: &str = "a table in an array of tables";

/// Emits a relation part's entry rows for one resource, values included.
///
/// For each `at:` path in the order written, and each object it selects in
/// document order, the adapter emits one row per member of that object, or,
/// with `leaves`, one per leaf below it (a key with no key below it, so an
/// empty object is a leaf; arrays are values, so nothing inside one is a key).
/// An entry two paths select is emitted once, where the first selects it. A
/// member row's pointer is its key path, its `span` the key's, its
/// `value_span` the value's (none for a YAML block scalar), and its state the
/// scalar it holds, or an [`RowState::Error`] naming what it holds otherwise
/// ([`OBJECT`], [`ARRAY`], [`DATETIME`], [`UNREADABLE`]).
///
/// A path that selects something other than an object emits one row labelled
/// `at:<path>` at what it selected, whose error state names it (`an array`,
/// `a string`); so does one that selects a TOML table in an array of tables,
/// whose members the TOML walk does not read ([`ARRAY_TABLE`]). Each
/// `bound:` path emits a row labelled `bound:<path>` per match, at most two
/// (one match is what a bound field needs; a second is enough to name both
/// in the failure).
///
/// The id holds the format, the paths and `leaves`, so two relations that
/// read one file differently never share a decode.
#[derive(Debug, Clone)]
pub(crate) struct EntryValuesAdapter {
    format: Format,
    at: Vec<(String, JsonPath)>,
    leaves: bool,
    bound: Vec<(String, JsonPath)>,
    id: String,
}

impl EntryValuesAdapter {
    /// An adapter for JSON, YAML or TOML (`format`), the `at:` paths, `leaves`
    /// and the `bound:` paths a relation's fields read.
    ///
    /// # Errors
    ///
    /// Another format, or a path that does not parse (the loader refuses
    /// both).
    pub fn new(
        format: Format,
        at: &[String],
        leaves: bool,
        bound: &[String],
    ) -> Result<Self, AdapterError> {
        let label = match format {
            Format::Json => "json",
            Format::Yaml => "yaml",
            Format::Toml => "toml",
            other => {
                return Err(AdapterError::Parse(format!(
                    "value-bearing entry rows are not supported for {}",
                    other.label()
                )));
            }
        };
        let parse = |query: &String| {
            JsonPath::parse(query)
                .map_err(|error| AdapterError::Parse(format!("bad JSONPath {query:?}: {error}")))
        };
        let id = format!("entry-values-{label}:{at:?}:{leaves}:{bound:?}");
        Ok(Self {
            format,
            at: at
                .iter()
                .map(|query| Ok((query.clone(), parse(query)?)))
                .collect::<Result<_, AdapterError>>()?,
            leaves,
            bound: bound
                .iter()
                .map(|query| Ok((query.clone(), parse(query)?)))
                .collect::<Result<_, AdapterError>>()?,
            id,
        })
    }

    #[must_use]
    pub fn format(&self) -> Format {
        self.format
    }
}

/// One entry of the document: its key path, spans and value.
struct Entry {
    pointer: Pointer,
    key_span: ByteSpan,
    value_span: Option<ByteSpan>,
    value: EntryValue,
}

impl Adapter for EntryValuesAdapter {
    fn id(&self) -> &str {
        &self.id
    }

    fn extract(
        &self,
        resource: &Resource,
        bytes: &[u8],
        limits: &Limits,
    ) -> Result<AdapterOutput, AdapterError> {
        // Spans index the original bytes, so the text is never lossy.
        let text = std::str::from_utf8(bytes).map_err(|_| {
            AdapterError::Parse(format!(
                "the {} document is not valid UTF-8",
                self.format.label()
            ))
        })?;
        let entries = self.entries(resource, text, limits)?;
        // The `JSONPath`s run over the parsed tree, as every structured
        // extract does; the entries give what the tree loses (each key's
        // span, a number's lexeme, a repeated key).
        let tree = self.format.parse(text).map_err(AdapterError::Parse)?;
        let mut members: HashMap<&[Segment], Vec<usize>> = HashMap::new();
        for (index, entry) in entries.iter().enumerate() {
            let segments = entry.pointer.segments();
            members
                .entry(&segments[..segments.len() - 1])
                .or_default()
                .push(index);
        }
        let mut rows = Vec::new();
        let mut emitted = vec![false; entries.len()];
        for (query, path) in &self.at {
            let label = format!("{AT_LABEL}{query}");
            for node in path.query_located(&tree) {
                let pointer = pointer_of(node.location());
                let kind = non_object(self.format, node.node()).or_else(|| {
                    // Arrays are values, so the TOML walk emits no member of a
                    // table in an array of tables.
                    (self.format == Format::Toml
                        && node.node().as_object().is_some_and(|map| !map.is_empty())
                        && !members.contains_key(pointer.segments()))
                    .then_some(ARRAY_TABLE)
                });
                if let Some(kind) = kind {
                    push(
                        &mut rows,
                        limits,
                        error_row(resource, pointer, &label, kind),
                    )?;
                    continue;
                }
                for index in self.selected(&members, &entries, pointer.segments()) {
                    if std::mem::replace(&mut emitted[index], true) {
                        continue;
                    }
                    push(
                        &mut rows,
                        limits,
                        entry_row(resource, &entries[index], None),
                    )?;
                }
            }
        }
        for (query, path) in &self.bound {
            let label = format!("{BOUND_LABEL}{query}");
            let mut matches = 0;
            for node in path.query_located(&tree) {
                let pointer = pointer_of(node.location());
                let held: Vec<&Entry> = entries
                    .iter()
                    .filter(|entry| entry.pointer == pointer)
                    .take(2)
                    .collect();
                if held.is_empty() {
                    // The root, or an element of an array: no entry holds it.
                    let row = match tree_scalar(self.format, node.node()) {
                        Ok(value) => {
                            Row::new(resource.clone(), Some(pointer), 0, RowState::Value(value))
                                .with_label(&label)
                        }
                        Err(kind) => error_row(resource, pointer, &label, kind),
                    };
                    push(&mut rows, limits, row)?;
                    matches += 1;
                } else {
                    for entry in held {
                        push(&mut rows, limits, entry_row(resource, entry, Some(&label)))?;
                        matches += 1;
                    }
                }
                if matches >= 2 {
                    break;
                }
            }
        }
        for (ordinal, row) in rows.iter_mut().enumerate() {
            row.ordinal = u32::try_from(ordinal).expect("the row limit is far below u32::MAX");
        }
        Ok(AdapterOutput::new(rows))
    }
}

impl EntryValuesAdapter {
    fn entries(
        &self,
        resource: &Resource,
        text: &str,
        limits: &Limits,
    ) -> Result<Vec<Entry>, AdapterError> {
        if self.format == Format::Toml {
            let (rows, values) = TomlRowsAdapter::values(resource, text, limits)?;
            return Ok(rows
                .into_iter()
                .zip(values)
                .map(|(row, value)| Entry {
                    pointer: row.pointer.unwrap_or_default(),
                    key_span: row.span.unwrap_or(ByteSpan::new(0, 0)),
                    value_span: value.span,
                    value: value.value,
                })
                .collect());
        }
        let max = limits.get(LimitKind::RowsPerExtractor);
        let (rows, values) =
            extract_entry_values_bounded(self.format, text, max).map_err(|error| match error {
                EntryRowsError::Limit { limit } => AdapterError::Limit(LimitExceeded {
                    kind: LimitKind::RowsPerExtractor,
                    limit,
                    reached: limit + 1,
                }),
                EntryRowsError::Parse(message) => AdapterError::Parse(message),
                EntryRowsError::UnsupportedFormat(_) => {
                    unreachable!("`new` accepts JSON, YAML and TOML only")
                }
            })?;
        Ok(rows
            .into_iter()
            .zip(values)
            .map(|(row, value)| Entry {
                pointer: Pointer::from(&row),
                key_span: row.key_span,
                value_span: row.value_span,
                value,
            })
            .collect())
    }

    /// The entries an object at `at` contributes: its members, or the leaves
    /// below it, in document order.
    fn selected(
        &self,
        members: &HashMap<&[Segment], Vec<usize>>,
        entries: &[Entry],
        at: &[Segment],
    ) -> Vec<usize> {
        let Some(children) = members.get(at) else {
            return Vec::new();
        };
        if !self.leaves {
            return children.clone();
        }
        // Depth first, with a stack of its own: document order, and nesting
        // cannot exhaust the thread's stack.
        let mut leaves = Vec::new();
        let mut pending: Vec<usize> = children.iter().rev().copied().collect();
        while let Some(index) = pending.pop() {
            match members.get(entries[index].pointer.segments()) {
                Some(below) => pending.extend(below.iter().rev()),
                None => leaves.push(index),
            }
        }
        leaves
    }
}

fn push(rows: &mut Vec<Row>, limits: &Limits, row: Row) -> Result<(), AdapterError> {
    check_row_count(limits, rows.len() + 1)?;
    rows.push(row);
    Ok(())
}

fn entry_row(resource: &Resource, entry: &Entry, label: Option<&str>) -> Row {
    let state = match &entry.value {
        EntryValue::Scalar(value) => RowState::Value(value.clone()),
        EntryValue::Object => RowState::Error(OBJECT.into()),
        EntryValue::Array => RowState::Error(ARRAY.into()),
        EntryValue::Datetime => RowState::Error(DATETIME.into()),
        EntryValue::Unreadable => RowState::Error(UNREADABLE.into()),
    };
    let mut row =
        Row::new(resource.clone(), Some(entry.pointer.clone()), 0, state).with_span(entry.key_span);
    row.value_span = entry.value_span;
    if let Some(label) = label {
        row = row.with_label(label);
    }
    row
}

fn error_row(resource: &Resource, pointer: Pointer, label: &str, kind: &str) -> Row {
    Row::new(
        resource.clone(),
        Some(pointer),
        0,
        RowState::Error(kind.into()),
    )
    .with_label(label)
}

fn pointer_of(path: &serde_json_path::NormalizedPath<'_>) -> Pointer {
    Pointer::from_segments(
        path.iter()
            .map(|element| match element {
                serde_json_path::PathElement::Name(name) => Segment::Key((*name).into()),
                serde_json_path::PathElement::Index(index) => Segment::Index(*index),
            })
            .collect(),
    )
}

/// Whether a TOML tree node is a date or time: the `toml` crate writes one
/// as a map with a single private key.
fn is_toml_datetime(format: Format, node: &serde_json::Value) -> bool {
    format == Format::Toml
        && node
            .as_object()
            .is_some_and(|map| map.len() == 1 && map.contains_key(toml_datetime_key()))
}

const fn toml_datetime_key() -> &'static str {
    "$__toml_private_datetime"
}

/// What an `at:` path selected when it is not an object.
fn non_object(format: Format, node: &serde_json::Value) -> Option<&'static str> {
    use serde_json::Value;
    match node {
        Value::Object(_) if is_toml_datetime(format, node) => Some(DATETIME),
        Value::Object(_) => None,
        Value::Array(_) => Some(ARRAY),
        Value::String(_) => Some("a string"),
        Value::Number(_) => Some("a number"),
        Value::Bool(_) => Some("a boolean"),
        Value::Null => Some("null"),
    }
}

/// A scalar no entry holds (the root, or an array's element), from the
/// tree. A number there has lost its lexeme (`1.10` reads as `1.1`), so it
/// is not a value.
fn tree_scalar(
    format: Format,
    node: &serde_json::Value,
) -> Result<crate::relation::Value, &'static str> {
    use crate::relation::Value as RowValue;
    use serde_json::Value;
    match node {
        Value::Null => Ok(RowValue::Null),
        Value::Bool(flag) => Ok(RowValue::Bool(*flag)),
        Value::String(text) => Ok(RowValue::String(text.as_str().into())),
        Value::Number(_) => Err(ARRAY_NUMBER),
        Value::Object(_) if is_toml_datetime(format, node) => Err(DATETIME),
        Value::Object(_) => Err(OBJECT),
        Value::Array(_) => Err(ARRAY),
    }
}

#[cfg(test)]
#[path = "entry_values_tests.rs"]
mod tests;
