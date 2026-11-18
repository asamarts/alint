//! TOML key rows through the lossless `toml_edit` tree (v0.21 R9).

use toml_edit::{Document, InlineTable, Item, Key, Table, Value};

use crate::ByteSpan;
use crate::extract::EntryValue;
use crate::relation::adapter::{Adapter, AdapterError, AdapterOutput, check_row_count};
use crate::relation::key_paths::{EntryAt, EntryTable, PathId};
use crate::relation::limits::{Limits, RunBudget};
use crate::relation::pointer::Segment;
use crate::relation::resource::Resource;

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
        self.extract_within(resource, bytes, limits, &RunBudget::bytes(limits))
    }

    fn extract_within(
        &self,
        resource: &Resource,
        bytes: &[u8],
        limits: &Limits,
        budget: &RunBudget,
    ) -> Result<AdapterOutput, AdapterError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| AdapterError::Parse("the TOML document is not valid UTF-8".to_string()))?;
        let mut walk = Walk {
            text,
            limits,
            values: false,
            entries: EntryTable::default(),
        };
        walk.document()?;
        Ok(AdapterOutput::new(super::entry::key_rows(
            &walk.entries,
            resource,
            limits,
            budget,
        )?))
    }
}

impl TomlRowsAdapter {
    /// The value mode (ADR-0034 decision 5): the members
    /// [`extract`](Adapter::extract) emits rows for, each with what its value
    /// holds beside it (`values[i]` is `rows[i]`'s) and the value's byte
    /// range. An integer or a float keeps its lexeme as written (`0x1F`,
    /// `1_000`, `+inf`); a date or time is [`EntryValue::Datetime`].
    pub(crate) fn values(text: &str, limits: &Limits) -> Result<EntryTable, AdapterError> {
        let mut walk = Walk {
            text,
            limits,
            values: true,
            entries: EntryTable::default(),
        };
        walk.document()?;
        Ok(walk.entries)
    }
}

/// One TOML entry's value: what it holds, and its byte range.
struct TomlValue {
    value: EntryValue,
    span: Option<ByteSpan>,
}

/// One walk over a document, a member per table member and, in the value
/// mode, each member's value beside it.
struct Walk<'w> {
    text: &'w str,
    limits: &'w Limits,
    values: bool,
    entries: EntryTable,
}

impl Walk<'_> {
    fn document(&mut self) -> Result<(), AdapterError> {
        let document = Document::parse(self.text).map_err(|error: toml_edit::TomlError| {
            AdapterError::Parse(parse_error(self.text, &error))
        })?;
        self.table(document.as_table(), PathId::ROOT)
    }

    fn table(&mut self, table: &Table, parent: PathId) -> Result<(), AdapterError> {
        for key_text in table.iter().map(|(key, _)| key) {
            let (key, item) = table
                .get_key_value(key_text)
                .unwrap_or_else(|| unreachable!("table.iter yielded a missing key"));
            self.item(key, item, parent)?;
        }
        Ok(())
    }

    fn item(&mut self, key: &Key, item: &Item, parent: PathId) -> Result<(), AdapterError> {
        let key_text = key.get();
        if matches!(item, Item::None) {
            return Ok(());
        }
        let span = key
            .span()
            .or_else(|| item.span())
            .ok_or_else(|| AdapterError::Parse(format!("the TOML key {key_text:?} has no span")))?;
        let path = self
            .entries
            .paths
            .child(parent, Segment::Key(key_text.into()));
        let text = self.text;
        self.push(path, span, || item_value(item, text))?;

        match item {
            Item::Table(table) => self.table(table, path)?,
            Item::Value(Value::InlineTable(inline)) => self.inline_table(inline, path)?,
            Item::None | Item::Value(_) | Item::ArrayOfTables(_) => {}
        }
        Ok(())
    }

    fn push(
        &mut self,
        path: PathId,
        span: std::ops::Range<usize>,
        value: impl FnOnce() -> TomlValue,
    ) -> Result<(), AdapterError> {
        check_row_count(self.limits, self.entries.rows.len() + 1)?;
        let value_span = if self.values {
            let held = value();
            self.entries.values.push(held.value);
            held.span
        } else {
            None
        };
        self.entries.rows.push(EntryAt {
            path,
            key_span: ByteSpan::new(span.start, span.end),
            value_span,
        });
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
    ///
    /// The key rows (`key_parity`'s) keep what they have always held: a
    /// dotted key inside an inline table (`x = { a.b = 1 }`) gives the row
    /// `x.a.b` and none for `x.a`. The value mode reads such a key as the
    /// table walk reads a dotted key in a table (`[x]` then `a.b = 1`), with
    /// a row for `x.a` that holds an object, so its rows are those of the
    /// same document written as JSON ([`Self::inline_members`]).
    fn inline_table(&mut self, table: &InlineTable, at: PathId) -> Result<(), AdapterError> {
        let mut pending: Vec<(PathId, &InlineTable)> = vec![(at, table)];
        while let Some((prefix, table)) = pending.pop() {
            let mut nested = Vec::new();
            if self.values {
                self.inline_members(prefix, table, &mut nested)?;
                pending.extend(nested.into_iter().rev());
                continue;
            }
            for (path, value) in table.get_values() {
                let span = path
                    .iter()
                    .filter_map(|key| key.span())
                    .next_back()
                    .ok_or_else(|| {
                        AdapterError::Parse("a TOML inline-table key has no span".to_string())
                    })?;
                let at = path.iter().fold(prefix, |at, key| {
                    self.entries.paths.child(at, Segment::Key(key.get().into()))
                });
                let text = self.text;
                self.push(at, span, || value_of(value, text))?;
                if let Value::InlineTable(inner) = value {
                    nested.push((at, inner));
                }
            }
            // Reverse so the stack visits nested tables in source order.
            pending.extend(nested.into_iter().rev());
        }
        Ok(())
    }

    /// The value mode's rows for one inline table: a row per key it holds,
    /// as [`Self::table`] reads a table, and each inline table among them,
    /// a dotted key's included, in `nested` to descend next.
    fn inline_members<'t>(
        &mut self,
        prefix: PathId,
        table: &'t InlineTable,
        nested: &mut Vec<(PathId, &'t InlineTable)>,
    ) -> Result<(), AdapterError> {
        for (key_text, _) in table {
            let (key, item) = table
                .get_key_value(key_text)
                .unwrap_or_else(|| unreachable!("table.iter yielded a missing key"));
            let Item::Value(value) = item else {
                unreachable!("an inline table holds values")
            };
            let span = key.span().or_else(|| value.span()).ok_or_else(|| {
                AdapterError::Parse(format!("the TOML key {key_text:?} has no span"))
            })?;
            let at = self
                .entries
                .paths
                .child(prefix, Segment::Key(key_text.into()));
            let text = self.text;
            self.push(at, span, || value_of(value, text))?;
            if let Value::InlineTable(inner) = value {
                nested.push((at, inner));
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relation::row::Row;

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

    /// Each row as `pointer @ "the text its span covers"`.
    fn located(text: &str, rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|row| {
                format!(
                    "{} @ {:?}",
                    row.pointer.as_ref().unwrap().render(),
                    &text[row.span.unwrap().as_range()]
                )
            })
            .collect()
    }

    /// `key_parity` reads these key rows: a dotted key in an inline table
    /// gives no row for its middle key, and one in a table does. The value
    /// mode's rows below must never change them.
    #[test]
    fn dotted_keys_keep_their_key_rows_in_inline_tables_and_tables() {
        let inline = "[en]\nnav = { home.title = \"Home\", about = \"About\" }\n";
        let output = output(inline);
        assert!(output.rows.iter().all(Row::is_key));
        assert_eq!(
            located(inline, &output.rows),
            [
                "en @ \"en\"",
                "en.nav @ \"nav\"",
                "en.nav.home.title @ \"title\"",
                "en.nav.about @ \"about\"",
            ]
        );
        let ordinals: Vec<u32> = output.rows.iter().map(|row| row.ordinal).collect();
        assert_eq!(ordinals, [0, 1, 2, 3]);
        let table = "[en.nav]\nhome.title = \"Home\"\nabout = \"About\"\n";
        assert_eq!(
            located(table, &self::output(table).rows),
            [
                "en @ \"en\"",
                "en.nav @ \"nav\"",
                "en.nav.home @ \"home\"",
                "en.nav.home.title @ \"title\"",
                "en.nav.about @ \"about\"",
            ]
        );
    }

    /// The value mode reads a dotted key in an inline table as JSON's nested
    /// object, with a row for the middle key holding an object.
    #[test]
    fn the_value_mode_gives_a_dotted_inline_key_its_middle_row() {
        let text = "[en]\nnav = { home.title = \"Home\", home.sub = 1, about = \"About\" }\n";
        let table = TomlRowsAdapter::values(text, &Limits::default()).unwrap();
        let limits = Limits::default();
        let rows =
            super::super::entry::key_rows(&table, &resource(), &limits, &RunBudget::bytes(&limits))
                .unwrap();
        assert_eq!(
            located(text, &rows),
            [
                "en @ \"en\"",
                "en.nav @ \"nav\"",
                "en.nav.home @ \"home\"",
                "en.nav.about @ \"about\"",
                "en.nav.home.title @ \"title\"",
                "en.nav.home.sub @ \"sub\"",
            ]
        );
        let held: Vec<String> = table
            .values
            .iter()
            .map(|value| match value {
                EntryValue::Scalar(scalar) => format!("{scalar:?}"),
                EntryValue::Object => "object".into(),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(
            held,
            [
                "object",
                "object",
                "object",
                "String(\"About\")",
                "String(\"Home\")",
                "Number(1)"
            ]
        );
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
