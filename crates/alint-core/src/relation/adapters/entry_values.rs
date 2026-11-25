//! Value-bearing entry rows (ADR-0034 decision 5, design section 5): the
//! members of each object a relation's `entries:` selects, or the leaves
//! below it, each with its value, and the relation's `bound:` values.

use std::collections::HashMap;

use serde_json_path::JsonPath;

use crate::extract::{EntryRowsError, EntryValue, extract_entry_values_bounded};
use crate::relation::adapter::{Adapter, AdapterError, AdapterOutput, check_row_count};
use crate::relation::key_paths::{EntryTable, PathBudget, PathId};
use crate::relation::limits::{LimitExceeded, LimitKind, Limits, RunBudget};
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
/// An object or array with nothing in it, told apart from one with members
/// or elements: under `strip-root`, a top-level key that holds one (or null)
/// is a file with no keys ([`holds_nothing`]).
pub(crate) const EMPTY_OBJECT: &str = "an empty object";
pub(crate) const EMPTY_ARRAY: &str = "an empty array";
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
/// ([`OBJECT`], [`ARRAY`], [`EMPTY_OBJECT`], [`EMPTY_ARRAY`], [`DATETIME`],
/// [`UNREADABLE`]).
///
/// A path that selects something other than an object emits one row labelled
/// `at:<path>` at what it selected, whose error state names it (`an array`,
/// `a string`); so does one that selects a TOML table in an array of tables,
/// whose members the TOML walk does not read ([`ARRAY_TABLE`]). A null
/// document (YAML that holds only comments, `---`, `~` or `null`; JSON
/// `null`) is read as an empty one is: a path that selects its root emits
/// nothing, as `key_parity` reads such a file as a peer with no keys. Each
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
        self.extract_in_context(None, resource, bytes, limits, &RunBudget::bytes(limits))
    }

    fn extract_within(
        &self,
        resource: &Resource,
        bytes: &[u8],
        limits: &Limits,
        budget: &RunBudget,
    ) -> Result<AdapterOutput, AdapterError> {
        self.extract_in_context(None, resource, bytes, limits, budget)
    }

    fn extract_in_context(
        &self,
        ctx: Option<&crate::Context<'_>>,
        resource: &Resource,
        bytes: &[u8],
        limits: &Limits,
        budget: &RunBudget,
    ) -> Result<AdapterOutput, AdapterError> {
        // Spans index the original bytes, so the text is never lossy.
        let text = std::str::from_utf8(bytes).map_err(|_| {
            AdapterError::Parse(format!(
                "the {} document is not valid UTF-8",
                self.format.label()
            ))
        })?;
        let entries = entry_table(self.format, text, limits)?;
        // The `JSONPath`s run over the parsed tree, as every structured
        // extract does; the entries give what the tree loses (each key's
        // span, a number's lexeme, a repeated key, the members' order).
        let tree = match ctx {
            Some(ctx) => ctx.parse_structured(resource.path().as_path(), self.format, text),
            None => self.format.parse_shared(text),
        }
        .map_err(AdapterError::Parse)?;
        let document = Document::new(&entries, &tree);
        let mut out = Output {
            resource,
            limits,
            entries: &entries,
            budget: PathBudget::for_key_rows(budget, limits),
            rows: Vec::new(),
            emitted: vec![false; entries.rows.len()],
            walked: vec![false; entries.paths.len()],
        };
        for (query, path) in &self.at {
            let label = format!("{AT_LABEL}{query}");
            let mut selected = path.query(&tree).all();
            // The tree holds an object's members in key order; the rows
            // follow the document's.
            selected.sort_by_key(|node| document.rank(node));
            let mut reported = false;
            for node in selected {
                let at = document.path(node);
                if at == Some(PathId::ROOT) && node.is_null() {
                    // A null document (a YAML file of only comments, `---`
                    // or `~`) has no entries, as an empty file has none.
                    continue;
                }
                let kind = non_object(self.format, node).or_else(|| {
                    // Arrays are values, so the TOML walk emits no member of a
                    // table in an array of tables.
                    (self.format == Format::Toml
                        && node.as_object().is_some_and(|map| !map.is_empty())
                        && !at.is_some_and(|at| document.has_members(at)))
                    .then_some(ARRAY_TABLE)
                });
                if let Some(kind) = kind {
                    // The relation fails at the first: one row says why.
                    if !std::mem::replace(&mut reported, true) {
                        let pointer = out.locate(path, &tree, node, at)?;
                        out.push(error_row(resource, pointer, &label, kind))?;
                    }
                    continue;
                }
                // An object no entry is below (an empty one) has no members.
                let Some(at) = at else {
                    continue;
                };
                for index in document.selected(at, self.leaves, &mut out.walked) {
                    if std::mem::replace(&mut out.emitted[index], true) {
                        continue;
                    }
                    let row = out.entry_row(&document, index, None)?;
                    out.push(row)?;
                }
            }
        }
        for (query, path) in &self.bound {
            let label = format!("{BOUND_LABEL}{query}");
            let mut matches = 0;
            let mut found = path.query(&tree).all();
            found.sort_by_key(|node| document.rank(node));
            for node in found {
                let at = document.path(node);
                let held: Vec<usize> = at
                    .filter(|at| *at != PathId::ROOT)
                    .map(|at| document.rows_at(at).take(2).collect())
                    .unwrap_or_default();
                if held.is_empty() {
                    // The root, or an element of an array: no entry holds it.
                    let pointer = out.locate(path, &tree, node, at)?;
                    let row = match tree_scalar(self.format, node) {
                        Ok(value) => {
                            Row::new(resource.clone(), Some(pointer), 0, RowState::Value(value))
                                .with_label(&label)
                        }
                        Err(kind) => error_row(resource, pointer, &label, kind),
                    };
                    out.push(row)?;
                    matches += 1;
                } else {
                    for index in held {
                        let row = out.entry_row(&document, index, Some(&label))?;
                        out.push(row)?;
                        matches += 1;
                    }
                }
                if matches >= 2 {
                    break;
                }
            }
        }
        let mut rows = out.rows;
        for (ordinal, row) in rows.iter_mut().enumerate() {
            row.ordinal = u32::try_from(ordinal).expect("the row limit is far below u32::MAX");
        }
        Ok(AdapterOutput::new(rows))
    }
}

/// A JSON, YAML or TOML (`format`) document's entries with their values and
/// spans, at most the rows `limits` allows: what an `entries:` part reads,
/// and what places the structured `read:` values a finding asks about
/// (`extract::locate`).
pub(crate) fn entry_table(
    format: Format,
    text: &str,
    limits: &Limits,
) -> Result<EntryTable, AdapterError> {
    if format == Format::Toml {
        return TomlRowsAdapter::values(text, limits);
    }
    extract_entry_table_non_toml(format, text, limits)
}

/// Like [`entry_table`], also descending into TOML arrays of tables and inline
/// tables inside arrays (JSON and YAML already descend arrays). Used by
/// `records:` extraction and span verification (#592).
pub(crate) fn entry_table_with_arrays(
    format: Format,
    text: &str,
    limits: &Limits,
) -> Result<EntryTable, AdapterError> {
    if format == Format::Toml {
        return TomlRowsAdapter::values_with_arrays(text, limits);
    }
    extract_entry_table_non_toml(format, text, limits)
}

fn extract_entry_table_non_toml(
    format: Format,
    text: &str,
    limits: &Limits,
) -> Result<EntryTable, AdapterError> {
    let max = limits.get(LimitKind::RowsPerExtractor);
    extract_entry_values_bounded(format, text, max).map_err(|error| match error {
        EntryRowsError::Limit { limit } => AdapterError::Limit(LimitExceeded {
            kind: LimitKind::RowsPerExtractor,
            limit,
            reached: limit + 1,
        }),
        EntryRowsError::Parse(message) => AdapterError::Parse(message),
        EntryRowsError::UnsupportedFormat(format) => AdapterError::Parse(format!(
            "value-bearing entry rows are not supported for {}",
            format.label()
        )),
    })
}

/// A row-free index of a document: each tree node's place in document order
/// and key path, and each object's members.
pub(super) struct Document<'d> {
    entries: &'d EntryTable,
    /// Each node of the tree, by address: its place in a walk of the tree
    /// in document order, and its key path when an entry is at or below it.
    nodes: HashMap<*const serde_json::Value, (usize, Option<PathId>)>,
    /// By key path, its first member's row; by row, the next member of the
    /// same object: each object's members in document order.
    first: Vec<u32>,
    next: Vec<u32>,
    /// By key path, whether the tree holds an empty object or an empty array
    /// there.
    empty: Vec<bool>,
}

/// No row, in [`Document::first`] and [`Document::next`].
const NONE: u32 = u32::MAX;

impl<'d> Document<'d> {
    pub(super) fn new(entries: &'d EntryTable, tree: &serde_json::Value) -> Self {
        let mut first = vec![NONE; entries.paths.len()];
        let mut next = vec![NONE; entries.rows.len()];
        for (index, row) in entries.rows.iter().enumerate().rev() {
            let holder = entries.paths.parent(row.path).index();
            next[index] = first[holder];
            first[holder] = u32::try_from(index).expect("the row limit is far below u32::MAX");
        }
        // Each tree node once, with its own stack: each object's members in
        // the order the walk reached their entries, which is the document's,
        // and a member no entry names (a TOML date's private key) after them.
        let mut nodes = HashMap::new();
        let mut empty = vec![false; entries.paths.len()];
        let mut pending: Vec<(&serde_json::Value, Option<PathId>)> =
            vec![(tree, Some(PathId::ROOT))];
        while let Some((node, at)) = pending.pop() {
            let rank = nodes.len();
            nodes.insert(std::ptr::from_ref(node), (rank, at));
            if let Some(at) = at
                && match node {
                    serde_json::Value::Object(map) => map.is_empty(),
                    serde_json::Value::Array(items) => items.is_empty(),
                    _ => false,
                }
            {
                empty[at.index()] = true;
            }
            match node {
                serde_json::Value::Object(map) => {
                    let mut members: Vec<(Option<PathId>, &serde_json::Value)> = map
                        .iter()
                        .map(|(key, value)| {
                            let path = at.and_then(|at| {
                                entries.paths.get(at, Segment::Key(key.as_str().into()))
                            });
                            (path, value)
                        })
                        .collect();
                    members.sort_by_key(|(path, _)| (path.is_none(), *path));
                    pending.extend(members.into_iter().rev().map(|(path, value)| (value, path)));
                }
                serde_json::Value::Array(items) => {
                    pending.extend(items.iter().enumerate().rev().map(|(index, item)| {
                        let path = at.and_then(|at| entries.paths.get(at, Segment::Index(index)));
                        (item, path)
                    }));
                }
                _ => {}
            }
        }
        Self {
            entries,
            nodes,
            first,
            next,
            empty,
        }
    }

    /// Whether the tree holds an empty object or an empty array at `at`.
    pub(super) fn is_empty(&self, at: PathId) -> bool {
        self.empty[at.index()]
    }

    /// `node`'s place in document order.
    pub(super) fn rank(&self, node: &serde_json::Value) -> usize {
        self.nodes
            .get(&std::ptr::from_ref(node))
            .map_or(usize::MAX, |(rank, _)| *rank)
    }

    /// `node`'s key path, when an entry is at or below it.
    pub(super) fn path(&self, node: &serde_json::Value) -> Option<PathId> {
        self.nodes
            .get(&std::ptr::from_ref(node))
            .and_then(|(_, path)| *path)
    }

    fn has_members(&self, at: PathId) -> bool {
        self.first[at.index()] != NONE
    }

    /// The rows of the members of the object at `at`, in document order.
    fn members(&self, at: PathId) -> impl Iterator<Item = usize> + '_ {
        let mut at = self.first[at.index()];
        std::iter::from_fn(move || {
            (at != NONE).then(|| {
                let index = at as usize;
                at = self.next[index];
                #[cfg(test)]
                tests::MEMBER_STEPS.with(|steps| steps.set(steps.get() + 1));
                index
            })
        })
    }

    /// The rows whose key path is `at` (more than one for a repeated key).
    pub(super) fn rows_at(&self, at: PathId) -> impl Iterator<Item = usize> + '_ {
        self.members(self.entries.paths.parent(at))
            .filter(move |index| self.entries.rows[*index].path == at)
    }

    /// The entries the object at `at` contributes: its members, or the
    /// leaves below it, in document order.
    ///
    /// The leaves of an object a `leaves: true` walk has been through were
    /// all emitted then, so a later walk that reaches it (an `at:` path that
    /// selects it, or one below it, after one above it) skips it: each
    /// object is walked once, however many paths select it or what holds
    /// it.
    fn selected(&self, at: PathId, leaves: bool, walked: &mut [bool]) -> Vec<usize> {
        if !leaves {
            return self.members(at).collect();
        }
        if std::mem::replace(&mut walked[at.index()], true) {
            return Vec::new();
        }
        // Depth first, with a stack of its own: document order, and nesting
        // cannot exhaust the thread's stack.
        let mut found = Vec::new();
        let mut pending: Vec<usize> = self.members(at).collect();
        pending.reverse();
        while let Some(index) = pending.pop() {
            let below = self.entries.rows[index].path;
            if !self.has_members(below) {
                found.push(index);
            } else if !std::mem::replace(&mut walked[below.index()], true) {
                let start = pending.len();
                pending.extend(self.members(below));
                pending[start..].reverse();
            }
        }
        found
    }
}

/// What one decode builds: its rows, and what it has emitted and walked.
struct Output<'o> {
    resource: &'o Resource,
    limits: &'o Limits,
    entries: &'o EntryTable,
    /// The key-path bytes the rows hold, taken from the run's as each is
    /// built.
    budget: PathBudget<'o>,
    rows: Vec<Row>,
    /// By row of the table: emitted already, where the first path selected
    /// it.
    emitted: Vec<bool>,
    /// By key path: its object's leaves were emitted ([`Document::selected`]).
    walked: Vec<bool>,
}

impl Output<'_> {
    fn push(&mut self, row: Row) -> Result<(), AdapterError> {
        check_row_count(self.limits, self.rows.len() + 1)?;
        self.rows.push(row);
        Ok(())
    }

    /// The row of the table's entry `index` in `document`, its key path built
    /// and charged.
    fn entry_row(
        &mut self,
        document: &Document<'_>,
        index: usize,
        label: Option<&str>,
    ) -> Result<Row, AdapterError> {
        let entry = &self.entries.rows[index];
        let empty = document.is_empty(entry.path);
        let state = match &self.entries.values[index] {
            EntryValue::Scalar(value) => RowState::Value(value.clone()),
            EntryValue::Object if empty => RowState::Error(EMPTY_OBJECT.into()),
            EntryValue::Object => RowState::Error(OBJECT.into()),
            EntryValue::Array if empty => RowState::Error(EMPTY_ARRAY.into()),
            EntryValue::Array => RowState::Error(ARRAY.into()),
            EntryValue::Datetime => RowState::Error(DATETIME.into()),
            EntryValue::Unreadable => RowState::Error(UNREADABLE.into()),
        };
        let pointer = self.budget.pointer(&self.entries.paths, entry.path)?;
        let mut row =
            Row::new(self.resource.clone(), Some(pointer), 0, state).with_span(entry.key_span);
        row.value_span = entry.value_span;
        if let Some(label) = label {
            row = row.with_label(label);
        }
        Ok(row)
    }

    /// Where `node`, which `path` selected, sits, its bytes charged: its key
    /// path, or, for an element of an array no entry is below, where the
    /// query says it is.
    fn locate(
        &mut self,
        path: &JsonPath,
        tree: &serde_json::Value,
        node: &serde_json::Value,
        at: Option<PathId>,
    ) -> Result<Pointer, AdapterError> {
        if let Some(at) = at {
            return Ok(self.budget.pointer(&self.entries.paths, at)?);
        }
        let pointer = path
            .query_located(tree)
            .into_iter()
            .find(|found| std::ptr::eq(found.node(), node))
            .map_or_else(Pointer::root, |found| pointer_of(found.location()));
        self.budget.take(pointer.heap_bytes())?;
        Ok(pointer)
    }
}

/// Whether an entry row's state is null, an empty object or an empty array:
/// a value with nothing in it.
pub(crate) fn holds_nothing(state: &RowState) -> bool {
    match state {
        RowState::Value(value) => *value == crate::relation::Value::Null,
        RowState::Error(kind) => [EMPTY_OBJECT, EMPTY_ARRAY].contains(&kind.as_ref()),
        RowState::Key | RowState::Missing => false,
    }
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

pub(super) fn pointer_of(path: &serde_json_path::NormalizedPath<'_>) -> Pointer {
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
pub(super) fn non_object(format: Format, node: &serde_json::Value) -> Option<&'static str> {
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
pub(super) fn tree_scalar(
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
