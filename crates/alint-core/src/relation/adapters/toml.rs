//! TOML key rows through the lossless `toml_edit` tree (v0.21 R9).

use toml_edit::{Document, InlineTable, Item, Key, Table, Value};

use crate::ByteSpan;
use crate::extract::EntryValue;
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
        let mut walk = Walk {
            text,
            resource,
            limits,
            rows: Vec::new(),
            values: None,
        };
        walk.document()?;
        Ok(AdapterOutput::new(walk.rows))
    }
}

impl TomlRowsAdapter {
    /// The value mode (ADR-0034 decision 5): the key rows
    /// [`extract`](Adapter::extract) emits, each with what its value holds
    /// beside it (`values[i]` is `rows[i]`'s, with the value's byte range).
    /// An integer or a float keeps its lexeme as written (`0x1F`, `1_000`,
    /// `+inf`); a date or time is [`EntryValue::Datetime`].
    pub(crate) fn values(
        resource: &Resource,
        text: &str,
        limits: &Limits,
    ) -> Result<(Vec<Row>, Vec<TomlValue>), AdapterError> {
        let mut walk = Walk {
            text,
            resource,
            limits,
            rows: Vec::new(),
            values: Some(Vec::new()),
        };
        walk.document()?;
        Ok((walk.rows, walk.values.unwrap_or_default()))
    }
}

/// One TOML entry's value: what it holds, and its byte range.
pub(crate) struct TomlValue {
    pub(crate) value: EntryValue,
    pub(crate) span: Option<ByteSpan>,
}

/// One walk over a document, emitting a key row per table member and, in the
/// value mode, each member's value beside it.
struct Walk<'w> {
    text: &'w str,
    resource: &'w Resource,
    limits: &'w Limits,
    rows: Vec<Row>,
    values: Option<Vec<TomlValue>>,
}

impl Walk<'_> {
    fn document(&mut self) -> Result<(), AdapterError> {
        let document = Document::parse(self.text).map_err(|error: toml_edit::TomlError| {
            AdapterError::Parse(parse_error(self.text, &error))
        })?;
        let mut parent = Vec::new();
        self.table(document.as_table(), &mut parent)
    }

    fn table(&mut self, table: &Table, parent: &mut Vec<Segment>) -> Result<(), AdapterError> {
        for key_text in table.iter().map(|(key, _)| key) {
            let (key, item) = table
                .get_key_value(key_text)
                .unwrap_or_else(|| unreachable!("table.iter yielded a missing key"));
            self.item(key, item, parent)?;
        }
        Ok(())
    }

    fn item(
        &mut self,
        key: &Key,
        item: &Item,
        parent: &mut Vec<Segment>,
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
        self.push(pointer, span, || item_value(item, self.text))?;

        match item {
            Item::Table(table) => {
                parent.push(Segment::Key(key_text.into()));
                self.table(table, parent)?;
                parent.pop();
            }
            Item::Value(Value::InlineTable(inline)) => {
                self.inline_table(inline, parent, &key_text)?;
            }
            Item::None | Item::Value(_) | Item::ArrayOfTables(_) => {}
        }
        Ok(())
    }

    fn push(
        &mut self,
        pointer: Pointer,
        span: std::ops::Range<usize>,
        value: impl FnOnce() -> TomlValue,
    ) -> Result<(), AdapterError> {
        push_row(self.resource, pointer, span, &mut self.rows, self.limits)?;
        if let Some(values) = &mut self.values {
            values.push(value());
        }
        Ok(())
    }
}

/// What a table member's item holds.
fn item_value(item: &Item, text: &str) -> TomlValue {
    match item {
        Item::Value(value) => value_of(value, text),
        Item::Table(_) => TomlValue {
            value: EntryValue::Object,
            span: None,
        },
        Item::ArrayOfTables(_) => TomlValue {
            value: EntryValue::Array,
            span: None,
        },
        Item::None => unreachable!("a missing item has no row"),
    }
}

/// What a TOML value holds; a number's lexeme is its source text.
fn value_of(value: &Value, text: &str) -> TomlValue {
    use crate::relation::{Number, Value as RowValue};

    let span = value.span();
    let lexeme = || span.clone().and_then(|span| text.get(span));
    let held = match value {
        Value::String(string) => {
            EntryValue::Scalar(RowValue::String(string.value().as_str().into()))
        }
        Value::Integer(_) | Value::Float(_) => lexeme()
            .and_then(Number::from_lexeme)
            .map_or(EntryValue::Unreadable, |number| {
                EntryValue::Scalar(RowValue::Number(number))
            }),
        Value::Boolean(flag) => EntryValue::Scalar(RowValue::Bool(*flag.value())),
        Value::Datetime(_) => EntryValue::Datetime,
        Value::Array(_) => EntryValue::Array,
        Value::InlineTable(_) => EntryValue::Object,
    };
    TomlValue {
        value: held,
        span: span.map(|span| ByteSpan::new(span.start, span.end)),
    }
}

impl Walk<'_> {
    /// Descend an inline table and every inline table nested in it, so
    /// `x = { a = { b = 1 } }` yields the same leaf `x.a.b` as `[x.a] b = 1`.
    /// The walk uses an explicit stack: nesting depth cannot exhaust the call
    /// stack, and the row limit bounds the work.
    fn inline_table(
        &mut self,
        table: &InlineTable,
        parent: &[Segment],
        table_key: &str,
    ) -> Result<(), AdapterError> {
        let mut root = parent.to_vec();
        root.push(Segment::Key(table_key.into()));
        let mut pending: Vec<(Vec<Segment>, &InlineTable)> = vec![(root, table)];
        while let Some((prefix, table)) = pending.pop() {
            let mut nested = Vec::new();
            for (path, value) in table.get_values() {
                let span = path
                    .iter()
                    .filter_map(|key| key.span())
                    .next_back()
                    .ok_or_else(|| {
                        AdapterError::Parse("a TOML inline-table key has no span".to_string())
                    })?;
                let mut segments = prefix.clone();
                for key in path {
                    segments.push(Segment::Key(key.get().into()));
                }
                let text = self.text;
                self.push(Pointer::from_segments(segments.clone()), span, || {
                    value_of(value, text)
                })?;
                if let Value::InlineTable(inner) = value {
                    nested.push((segments, inner));
                }
            }
            // Reverse so the stack visits nested tables in source order.
            pending.extend(nested.into_iter().rev());
        }
        Ok(())
    }
}

/// A TOML parse error as its position and reason, without the source line
/// `toml_edit`'s `Display` quotes: the line may hold a value, and key-only
/// consumers must never echo one.
fn parse_error(text: &str, error: &toml_edit::TomlError) -> String {
    let reason = error.message().trim();
    let Some(offset) = error.span().map(|span| span.start.min(text.len())) else {
        return format!("TOML parse error: {reason}");
    };
    let before = text.get(..offset).unwrap_or(text);
    let line = before.matches('\n').count() + 1;
    // A byte-order mark is not a column.
    let column = before
        .rsplit('\n')
        .next()
        .map(|tail| {
            if line == 1 {
                tail.trim_start_matches('\u{feff}')
            } else {
                tail
            }
        })
        .map_or(1, |tail| tail.chars().count() + 1);
    format!("TOML parse error at line {line}, column {column}: {reason}")
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
    fn nested_inline_tables_are_descended_to_their_leaves() {
        let inline = output("x = { a = { b = 1, c = { d = 2 } } }\n");
        assert_eq!(rendered(&inline), ["x", "x.a", "x.a.b", "x.a.c", "x.a.c.d"]);
        let tables = output("[x.a]\nb = 1\n[x.a.c]\nd = 2\n");
        let leaves = |keys: Vec<String>| {
            keys.iter()
                .filter(|key| {
                    !keys
                        .iter()
                        .any(|other| other.starts_with(&format!("{key}.")))
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(leaves(rendered(&inline)), leaves(rendered(&tables)));
        let deep = format!("x = {}1{}\n", "{ a = ".repeat(40), " }".repeat(40));
        assert_eq!(output(&deep).rows.len(), 41);
    }

    #[test]
    fn parse_errors_report_a_position_without_echoing_the_source() {
        let AdapterError::Parse(message) = TomlRowsAdapter::new()
            .extract(
                &resource(),
                b"ok = 1\nsecret = \"hunter2\n",
                &Limits::default(),
            )
            .unwrap_err()
        else {
            panic!("expected a parse error");
        };
        assert!(message.contains("line 2"), "{message}");
        assert!(!message.contains("hunter2"), "{message}");

        // A byte-order mark does not shift the first line's columns.
        let column = |text: &str| {
            let AdapterError::Parse(message) = TomlRowsAdapter::new()
                .extract(&resource(), text.as_bytes(), &Limits::default())
                .unwrap_err()
            else {
                panic!("expected a parse error");
            };
            message
        };
        assert_eq!(column("a = = 1\n"), column("\u{feff}a = = 1\n"));
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
