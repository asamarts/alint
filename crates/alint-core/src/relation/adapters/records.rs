//! Array-of-objects extraction (`records:` reader, ADR-0050 D8 / #592):
//! selects object elements with `at:` `JSONPath` queries and extracts sibling scalar
//! subfields per element with `record: <subpath>` `JSONPath` queries, preserving exact
//! CST/spanned-event source spans and number lexemes across JSON, YAML, and
//! TOML.

use std::collections::HashSet;

use serde_json_path::JsonPath;

use crate::extract::EntryValue;
use crate::relation::adapter::{Adapter, AdapterError, AdapterOutput, check_row_count};
use crate::relation::key_paths::{EntryTable, PathBudget, PathId};
use crate::relation::limits::{Limits, RunBudget};
use crate::relation::pointer::Pointer;
use crate::relation::resource::Resource;
use crate::relation::row::{Row, RowState};
use crate::structured_format::Format;

use super::entry_values::{
    self, AT_LABEL, BOUND_LABEL, Document, entry_table_with_arrays, non_object, pointer_of,
    tree_scalar,
};

/// Label of a record header row emitted for each matched object element.
pub(crate) const REC_LABEL: &str = "rec:";

/// Label prefix of a `record:` field match row: `record:<subpath>`.
pub(crate) const RECORD_LABEL: &str = "record:";

/// Emits record header rows, `record:` subfield rows, `bound:` rows, and
/// non-object `at:` error rows for one structured document.
#[derive(Debug, Clone)]
pub(crate) struct RecordsAdapter {
    format: Format,
    at: Vec<(String, JsonPath)>,
    fields: Vec<(String, JsonPath)>,
    bound: Vec<(String, JsonPath)>,
    id: String,
}

impl RecordsAdapter {
    pub fn new(
        format: Format,
        at: &[String],
        fields: &[String],
        bound: &[String],
    ) -> Result<Self, AdapterError> {
        let label = match format {
            Format::Json => "json",
            Format::Yaml => "yaml",
            Format::Toml => "toml",
            other => {
                return Err(AdapterError::Parse(format!(
                    "`records:` is not supported for {}",
                    other.label()
                )));
            }
        };
        let parse_all = |queries: &[String]| -> Result<Vec<(String, JsonPath)>, AdapterError> {
            queries
                .iter()
                .map(|query| {
                    let parsed = JsonPath::parse(query).map_err(|error| {
                        AdapterError::Parse(format!("bad JSONPath {query:?}: {error}"))
                    })?;
                    Ok((query.clone(), parsed))
                })
                .collect()
        };
        let id = format!("records-{label}:{at:?}:{fields:?}:{bound:?}");
        Ok(Self {
            format,
            at: parse_all(at)?,
            fields: parse_all(fields)?,
            bound: parse_all(bound)?,
            id,
        })
    }

    #[must_use]
    pub fn format(&self) -> Format {
        self.format
    }
}

impl Adapter for RecordsAdapter {
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
        let text = std::str::from_utf8(bytes).map_err(|_| {
            AdapterError::Parse(format!(
                "the {} document is not valid UTF-8",
                self.format.label()
            ))
        })?;
        let entries = entry_table_with_arrays(self.format, text, limits)?;
        let tree = match ctx {
            Some(ctx) => ctx.parse_structured(resource.path().as_path(), self.format, text),
            None => self.format.parse_shared(text),
        }
        .map_err(AdapterError::Parse)?;
        let document = Document::new(&entries, &tree);
        let mut out = Output {
            format: self.format,
            resource,
            limits,
            entries: &entries,
            budget: PathBudget::for_key_rows(budget, limits),
            rows: Vec::new(),
        };
        let mut seen_nodes: HashSet<*const serde_json::Value> = HashSet::new();
        let mut rec_index = 0u32;
        for (query, path) in &self.at {
            out.extract_at(
                query,
                path,
                &tree,
                &document,
                &self.fields,
                &mut seen_nodes,
                &mut rec_index,
            )?;
        }
        for (query, path) in &self.bound {
            out.extract_bound(query, path, &tree, &document)?;
        }
        Ok(AdapterOutput::new(out.rows))
    }
}

struct Output<'o> {
    format: Format,
    resource: &'o Resource,
    limits: &'o Limits,
    entries: &'o EntryTable,
    budget: PathBudget<'o>,
    rows: Vec<Row>,
}

impl Output<'_> {
    fn push(&mut self, row: Row) -> Result<(), AdapterError> {
        check_row_count(self.limits, self.rows.len() + 1)?;
        self.rows.push(row);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn extract_at(
        &mut self,
        query: &str,
        path: &JsonPath,
        tree: &serde_json::Value,
        document: &Document<'_>,
        fields: &[(String, JsonPath)],
        seen_nodes: &mut HashSet<*const serde_json::Value>,
        rec_index: &mut u32,
    ) -> Result<(), AdapterError> {
        let at_label = format!("{AT_LABEL}{query}");
        let mut selected = path.query(tree).all();
        selected.sort_by_key(|node| document.rank(node));
        let mut reported = false;
        for node in selected {
            let at = document.path(node);
            if at == Some(PathId::ROOT) && node.is_null() {
                continue;
            }
            if let Some(kind) = non_object(self.format, node) {
                if !std::mem::replace(&mut reported, true) {
                    let pointer = self.locate(path, tree, node, at)?;
                    self.push(
                        Row::new(
                            self.resource.clone(),
                            Some(pointer),
                            0,
                            RowState::Error(kind.into()),
                        )
                        .with_label(&at_label),
                    )?;
                }
                continue;
            }
            if !seen_nodes.insert(std::ptr::from_ref(node)) {
                continue;
            }
            let elem_pointer = self.locate(path, tree, node, at)?;
            let ordinal = *rec_index;
            *rec_index = rec_index.saturating_add(1);
            self.push(
                Row::new(
                    self.resource.clone(),
                    Some(elem_pointer.clone()),
                    ordinal,
                    RowState::Key,
                )
                .with_label(REC_LABEL),
            )?;
            for (sub_query, sub_path) in fields {
                self.extract_record_field(
                    sub_query,
                    sub_path,
                    node,
                    &elem_pointer,
                    ordinal,
                    document,
                )?;
            }
        }
        Ok(())
    }

    fn extract_record_field(
        &mut self,
        sub_query: &str,
        sub_path: &JsonPath,
        elem_node: &serde_json::Value,
        elem_pointer: &Pointer,
        ordinal: u32,
        document: &Document<'_>,
    ) -> Result<(), AdapterError> {
        let label = format!("{RECORD_LABEL}{sub_query}");
        let mut matches = 0usize;
        let mut found = sub_path.query(elem_node).all();
        found.sort_by_key(|node| document.rank(node));
        for sub_node in found {
            let at = document.path(sub_node);
            let held: Vec<usize> = at
                .filter(|at| *at != PathId::ROOT)
                .map(|at| document.rows_at(at).take(2).collect())
                .unwrap_or_default();
            if held.is_empty() {
                let pointer = self.locate_below(sub_path, elem_node, elem_pointer, sub_node, at)?;
                let row = match tree_scalar(self.format, sub_node) {
                    Ok(value) => Row::new(
                        self.resource.clone(),
                        Some(pointer),
                        ordinal,
                        RowState::Value(value),
                    )
                    .with_label(&label),
                    Err(kind) => Row::new(
                        self.resource.clone(),
                        Some(pointer),
                        ordinal,
                        RowState::Error(kind.into()),
                    )
                    .with_label(&label),
                };
                self.push(row)?;
                matches += 1;
            } else {
                for index in held {
                    let row = self.entry_row(document, index, &label, ordinal)?;
                    self.push(row)?;
                    matches += 1;
                    if matches >= 2 {
                        break;
                    }
                }
            }
            if matches >= 2 {
                break;
            }
        }
        Ok(())
    }

    fn extract_bound(
        &mut self,
        query: &str,
        path: &JsonPath,
        tree: &serde_json::Value,
        document: &Document<'_>,
    ) -> Result<(), AdapterError> {
        let label = format!("{BOUND_LABEL}{query}");
        let mut matches = 0usize;
        let mut found = path.query(tree).all();
        found.sort_by_key(|node| document.rank(node));
        for node in found {
            let at = document.path(node);
            let held: Vec<usize> = at
                .filter(|at| *at != PathId::ROOT)
                .map(|at| document.rows_at(at).take(2).collect())
                .unwrap_or_default();
            if held.is_empty() {
                let pointer = self.locate(path, tree, node, at)?;
                let row = match tree_scalar(self.format, node) {
                    Ok(value) => Row::new(
                        self.resource.clone(),
                        Some(pointer),
                        0,
                        RowState::Value(value),
                    )
                    .with_label(&label),
                    Err(kind) => Row::new(
                        self.resource.clone(),
                        Some(pointer),
                        0,
                        RowState::Error(kind.into()),
                    )
                    .with_label(&label),
                };
                self.push(row)?;
                matches += 1;
            } else {
                for index in held {
                    let row = self.entry_row(document, index, &label, 0)?;
                    self.push(row)?;
                    matches += 1;
                    if matches >= 2 {
                        break;
                    }
                }
            }
            if matches >= 2 {
                break;
            }
        }
        Ok(())
    }

    fn entry_row(
        &mut self,
        document: &Document<'_>,
        index: usize,
        label: &str,
        ordinal: u32,
    ) -> Result<Row, AdapterError> {
        let entry = &self.entries.rows[index];
        let empty = document.is_empty(entry.path);
        let state = match &self.entries.values[index] {
            EntryValue::Scalar(value) => RowState::Value(value.clone()),
            EntryValue::Object if empty => RowState::Error(entry_values::EMPTY_OBJECT.into()),
            EntryValue::Object => RowState::Error(entry_values::OBJECT.into()),
            EntryValue::Array if empty => RowState::Error(entry_values::EMPTY_ARRAY.into()),
            EntryValue::Array => RowState::Error(entry_values::ARRAY.into()),
            EntryValue::Datetime => RowState::Error(entry_values::DATETIME.into()),
            EntryValue::Unreadable => RowState::Error(entry_values::UNREADABLE.into()),
        };
        let pointer = self.budget.pointer(&self.entries.paths, entry.path)?;
        let mut row = Row::new(self.resource.clone(), Some(pointer), ordinal, state)
            .with_span(entry.key_span)
            .with_label(label);
        row.value_span = entry.value_span;
        Ok(row)
    }

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

    fn locate_below(
        &mut self,
        sub_path: &JsonPath,
        elem_node: &serde_json::Value,
        elem_pointer: &Pointer,
        sub_node: &serde_json::Value,
        at: Option<PathId>,
    ) -> Result<Pointer, AdapterError> {
        if let Some(at) = at {
            return Ok(self.budget.pointer(&self.entries.paths, at)?);
        }
        let mut segments = elem_pointer.segments().to_vec();
        if let Some(found) = sub_path
            .query_located(elem_node)
            .into_iter()
            .find(|found| std::ptr::eq(found.node(), sub_node))
        {
            segments.extend(pointer_of(found.location()).segments().iter().cloned());
        }
        let pointer = Pointer::from_segments(segments);
        self.budget.take(pointer.heap_bytes())?;
        Ok(pointer)
    }
}
