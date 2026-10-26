//! TOML key rows through the lossless `toml_edit` tree (v0.21 R9).

use toml_edit::{Document, InlineTable, Item, Key, Table, Value};

use crate::ByteSpan;
use crate::relation::adapter::{Adapter, AdapterError, AdapterOutput, check_row_count};
use crate::relation::limits::Limits;
use crate::relation::pointer::{Pointer, Segment};
use crate::relation::resource::Resource;
use crate::relation::row::{Row, RowState};

/// Emits one key row per TOML table member.
///
/// Dotted and ordinary table keys produce the same leaf paths (`[nav] home`
/// and `nav.home = ...`). Arrays, including arrays of tables, are values and
/// are not descended; an inline table is a record and is descended. Duplicate
/// keys are rejected by TOML itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct TomlRowsAdapter;

impl TomlRowsAdapter {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Adapter for TomlRowsAdapter {
    fn id(&self) -> &'static str {
        "entry-toml"
    }

    fn extract(
        &self,
        resource: &Resource,
        bytes: &[u8],
        limits: &Limits,
    ) -> Result<AdapterOutput, AdapterError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| AdapterError::Parse("the TOML document is not valid UTF-8".to_string()))?;
        let document = Document::parse(text)
            .map_err(|error: toml_edit::TomlError| AdapterError::Parse(error.to_string()))?;
        let mut rows = Vec::new();
        let mut parent = Vec::new();
        collect_table(
            document.as_table(),
            resource,
            &mut parent,
            &mut rows,
            limits,
        )?;
        Ok(AdapterOutput::new(rows))
    }
}

fn collect_table(
    table: &Table,
    resource: &Resource,
    parent: &mut Vec<Segment>,
    rows: &mut Vec<Row>,
    limits: &Limits,
) -> Result<(), AdapterError> {
    for key_text in table.iter().map(|(key, _)| key) {
        let (key, item) = table
            .get_key_value(key_text)
            .unwrap_or_else(|| unreachable!("table.iter yielded a missing key"));
        collect_item(key, item, resource, parent, rows, limits)?;
    }
    Ok(())
}

fn collect_item(
    key: &Key,
    item: &Item,
    resource: &Resource,
    parent: &mut Vec<Segment>,
    rows: &mut Vec<Row>,
    limits: &Limits,
) -> Result<(), AdapterError> {
    let key_text = key.get().to_string();
    if matches!(item, Item::None) {
        return Ok(());
    }
    let span = key
        .span()
        .or_else(|| item.span())
        .ok_or_else(|| AdapterError::Parse(format!("the TOML key {key_text:?} has no span")))?;
    let pointer = pointer_for(parent, &key_text);
    push_row(resource, pointer, span, rows, limits)?;

    match item {
        Item::Table(table) => {
            parent.push(Segment::Key(key_text.into()));
            collect_table(table, resource, parent, rows, limits)?;
            parent.pop();
        }
        Item::Value(Value::InlineTable(inline)) => {
            collect_inline_table(inline, resource, parent, &key_text, rows, limits)?;
        }
        Item::None | Item::Value(_) | Item::ArrayOfTables(_) => {}
    }
    Ok(())
}

fn collect_inline_table(
    table: &InlineTable,
    resource: &Resource,
    parent: &mut Vec<Segment>,
    table_key: &str,
    rows: &mut Vec<Row>,
    limits: &Limits,
) -> Result<(), AdapterError> {
    parent.push(Segment::Key(table_key.into()));
    for (path, _) in table.get_values() {
        let span = path
            .iter()
            .filter_map(|key| key.span())
            .next_back()
            .ok_or_else(|| {
                AdapterError::Parse("a TOML inline-table key has no span".to_string())
            })?;
        let mut segments = parent.clone();
        for key in path {
            let text = key.get().to_string();
            segments.push(Segment::Key(text.into()));
        }
        push_row(
            resource,
            Pointer::from_segments(segments),
            span,
            rows,
            limits,
        )?;
    }
    parent.pop();
    Ok(())
}

fn pointer_for(parent: &[Segment], key: &str) -> Pointer {
    let mut segments = parent.to_vec();
    segments.push(Segment::Key(key.into()));
    Pointer::from_segments(segments)
}

fn push_row(
    resource: &Resource,
    pointer: Pointer,
    span: std::ops::Range<usize>,
    rows: &mut Vec<Row>,
    limits: &Limits,
) -> Result<(), AdapterError> {
    check_row_count(limits, rows.len() + 1)?;
    rows.push(
        Row::new(
            resource.clone(),
            Some(pointer),
            u32::try_from(rows.len()).expect("the row limit is far below u32::MAX"),
            RowState::Key,
        )
        .with_span(ByteSpan::new(span.start, span.end)),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource() -> Resource {
        Resource::for_test(0, "Cargo.toml")
    }

    fn output(text: &str) -> AdapterOutput {
        TomlRowsAdapter::new()
            .extract(&resource(), text.as_bytes(), &Limits::default())
            .expect("TOML extracts")
    }

    fn rendered(output: &AdapterOutput) -> Vec<String> {
        output
            .rows
            .iter()
            .map(|row| row.pointer.as_ref().unwrap().render())
            .collect()
    }

    #[test]
    fn table_and_dotted_keys_have_the_same_leaf_paths() {
        let table = output("[nav]\nhome = \"Home\"\nabout = \"About\"\n");
        let dotted = output("nav.home = \"Home\"\nnav.about = \"About\"\n");
        assert_eq!(rendered(&table), ["nav", "nav.home", "nav.about"]);
        assert_eq!(rendered(&dotted), ["nav", "nav.home", "nav.about"]);
        assert!(table.rows.iter().all(Row::is_key));
        let home = table
            .rows
            .iter()
            .find(|row| row.pointer.as_ref().unwrap().render() == "nav.home")
            .unwrap();
        let text = "[nav]\nhome = \"Home\"\nabout = \"About\"\n";
        assert_eq!(&text[home.span.unwrap().as_range()], "home");
    }

    #[test]
    fn arrays_are_values_but_inline_tables_are_descended() {
        let output =
            output("list = [1, 2]\ninline = { one = 1, two = 2 }\n[[items]]\nname = \"a\"\n");
        let keys = rendered(&output);
        assert_eq!(
            keys,
            ["list", "inline", "inline.one", "inline.two", "items"]
        );
    }

    #[test]
    fn malformed_and_duplicate_toml_fail_loudly() {
        let error = TomlRowsAdapter::new()
            .extract(&resource(), b"a = 1\na = 2\n", &Limits::default())
            .unwrap_err();
        assert!(matches!(error, AdapterError::Parse(_)), "{error}");
    }

    #[test]
    fn the_row_limit_is_enforced() {
        let limits = Limits::default()
            .with(crate::relation::LimitKind::RowsPerExtractor, 1)
            .unwrap();
        let error = TomlRowsAdapter::new()
            .extract(&resource(), b"a = 1\nb = 2\n", &limits)
            .unwrap_err();
        assert!(matches!(error, AdapterError::Limit(_)), "{error}");
    }
}
