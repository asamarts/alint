//! TOML key rows through the lossless `toml_edit` tree (v0.21 R9).

use toml_edit::{Document, InlineTable, Item, Key, Table, Value};

use crate::ByteSpan;
use crate::extract::EntryValue;
use crate::relation::adapter::{Adapter, AdapterError, AdapterOutput, check_row_count};
use crate::relation::key_paths::{EntryAt, EntryTable, PathId, PathStack};
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
            offset: 0,
            limits,
            values: false,
            descend_arrays: false,
            entries: EntryTable::default(),
            stack: PathStack::default(),
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
            offset: 0,
            limits,
            values: true,
            descend_arrays: false,
            entries: EntryTable::default(),
            stack: PathStack::default(),
        };
        walk.document()?;
        Ok(walk.entries)
    }

    /// Like [`Self::values`], also descending into arrays of tables
    /// (`[[items]]`) and inline tables inside arrays (`items = [{ a = 1 }]`)
    /// with `Segment::Index(i)` paths for `records:` extraction (#592).
    pub(crate) fn values_with_arrays(
        text: &str,
        limits: &Limits,
    ) -> Result<EntryTable, AdapterError> {
        let mut walk = Walk {
            text,
            offset: 0,
            limits,
            values: true,
            descend_arrays: true,
            entries: EntryTable::default(),
            stack: PathStack::default(),
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
/// mode, each member's value beside it. Uses a lazy [`PathStack`] (matching
/// JSON and YAML) so empty tables, empty inline tables, and empty nested
/// arrays inside `[[...]]` / `[...]` allocate zero `KeyPaths` nodes.
struct Walk<'w> {
    text: &'w str,
    offset: usize,
    limits: &'w Limits,
    values: bool,
    descend_arrays: bool,
    entries: EntryTable,
    stack: PathStack,
}

enum WalkStep<'t> {
    Enter(Segment),
    Leave,
    Inline(&'t InlineTable),
    Array(&'t toml_edit::Array),
}

impl Walk<'_> {
    fn document(&mut self) -> Result<(), AdapterError> {
        if self
            .text
            .trim_matches(|c: char| c == '\u{feff}' || c.is_whitespace())
            .is_empty()
        {
            return Ok(());
        }
        let stripped = self.text.trim_start_matches('\u{feff}');
        self.offset = self.text.len() - stripped.len();
        self.text = stripped;
        if stripped.len() > crate::structured_format::MAX_STRUCTURED_BYTES {
            return Err(AdapterError::Parse(format!(
                "TOML input exceeds the maximum supported size ({} bytes)",
                crate::structured_format::MAX_STRUCTURED_BYTES
            )));
        }
        let document = Document::parse(stripped).map_err(|error: toml_edit::TomlError| {
            AdapterError::Parse(parse_error(stripped, &error))
        })?;
        // The lexer's tokens, held while the document was built.
        self.entries.parsed += crate::structured_format::toml_tokens(stripped);
        self.table(document.as_table())
    }

    fn table(&mut self, table: &Table) -> Result<(), AdapterError> {
        for key_text in table.iter().map(|(key, _)| key) {
            let (key, item) = table
                .get_key_value(key_text)
                .unwrap_or_else(|| unreachable!("table.iter yielded a missing key"));
            self.item(key, item)?;
        }
        Ok(())
    }

    fn item(&mut self, key: &Key, item: &Item) -> Result<(), AdapterError> {
        let key_text = key.get();
        if matches!(item, Item::None) {
            return Ok(());
        }
        let span = key
            .span()
            .or_else(|| item.span())
            .ok_or_else(|| AdapterError::Parse(format!("the TOML key {key_text:?} has no span")))?;
        self.stack.enter(Segment::Key(key_text.into()));
        let path = self.stack.path(&mut self.entries.paths);
        let text = self.text;
        let owned = text_of(key_text, item.as_value());
        self.push(path, span, owned, || item_value(item, text))?;
        // When arrays are not descended, charge what the document holds below
        // them up front.
        if !self.descend_arrays {
            self.entries.parsed += match item {
                Item::Value(Value::Array(_)) | Item::ArrayOfTables(_) => below(item),
                Item::None | Item::Value(_) | Item::Table(_) => 0,
            };
        }

        match item {
            Item::Table(table) => self.table(table)?,
            Item::Value(Value::InlineTable(inline)) => {
                self.walk_values(WalkStep::Inline(inline))?;
            }
            Item::ArrayOfTables(tables) if self.descend_arrays => {
                for (index, table) in tables.iter().enumerate() {
                    self.entries.parsed += std::mem::size_of::<Item>();
                    self.stack.enter(Segment::Index(index));
                    self.table(table)?;
                    self.stack.leave();
                }
            }
            Item::Value(Value::Array(array)) if self.descend_arrays => {
                self.walk_values(WalkStep::Array(array))?;
            }
            Item::None | Item::Value(_) | Item::ArrayOfTables(_) => {}
        }
        self.stack.leave();
        Ok(())
    }

    /// Iteratively descend inline tables and (when `descend_arrays` is set)
    /// arrays using the lazy [`PathStack`], preserving source order and zero
    /// `KeyPaths` allocation on empty inline tables or nested arrays.
    fn walk_values<'t>(&mut self, root: WalkStep<'t>) -> Result<(), AdapterError> {
        let mut pending: Vec<WalkStep<'t>> = vec![root];
        while let Some(step) = pending.pop() {
            match step {
                WalkStep::Enter(segment) => self.stack.enter(segment),
                WalkStep::Leave => self.stack.leave(),
                WalkStep::Array(array) => {
                    let mut children: Vec<(Segment, WalkStep<'t>)> = Vec::new();
                    for (index, value) in array.iter().enumerate() {
                        self.entries.parsed += std::mem::size_of::<Item>();
                        match value {
                            Value::InlineTable(table) => {
                                children.push((Segment::Index(index), WalkStep::Inline(table)));
                            }
                            Value::Array(inner) => {
                                children.push((Segment::Index(index), WalkStep::Array(inner)));
                            }
                            Value::String(string) => self.entries.parsed += string.value().len(),
                            _ => {}
                        }
                    }
                    for (segment, child) in children.into_iter().rev() {
                        pending.push(WalkStep::Leave);
                        pending.push(child);
                        pending.push(WalkStep::Enter(segment));
                    }
                }
                WalkStep::Inline(table) => {
                    if self.values {
                        let mut children: Vec<(Segment, WalkStep<'t>)> = Vec::new();
                        self.inline_members(table, &mut children)?;
                        for (segment, child) in children.into_iter().rev() {
                            pending.push(WalkStep::Leave);
                            pending.push(child);
                            pending.push(WalkStep::Enter(segment));
                        }
                    } else {
                        let mut children: Vec<(Vec<Segment>, &'t InlineTable)> = Vec::new();
                        for (path, value) in table.get_values() {
                            let span = path
                                .iter()
                                .filter_map(|key| key.span())
                                .next_back()
                                .ok_or_else(|| {
                                    AdapterError::Parse(
                                        "a TOML inline-table key has no span".to_string(),
                                    )
                                })?;
                            for key in &path {
                                self.stack.enter(Segment::Key(key.get().into()));
                            }
                            let at = self.stack.path(&mut self.entries.paths);
                            for _ in 0..path.len() {
                                self.stack.leave();
                            }
                            let text = self.text;
                            let owned = path.iter().map(|key| key.get().len()).sum::<usize>()
                                + text_of("", Some(value));
                            self.push(at, span, owned, || value_of(value, text))?;
                            if let Value::InlineTable(inner) = value {
                                let segs =
                                    path.iter().map(|k| Segment::Key(k.get().into())).collect();
                                children.push((segs, inner));
                            }
                        }
                        for (segs, inner) in children.into_iter().rev() {
                            for _ in 0..segs.len() {
                                pending.push(WalkStep::Leave);
                            }
                            pending.push(WalkStep::Inline(inner));
                            for seg in segs.into_iter().rev() {
                                pending.push(WalkStep::Enter(seg));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// A row for the member at `path`, whose key (and string value) the
    /// document holds as `owned` bytes of text.
    fn push(
        &mut self,
        path: PathId,
        span: std::ops::Range<usize>,
        owned: usize,
        value: impl FnOnce() -> TomlValue,
    ) -> Result<(), AdapterError> {
        check_row_count(self.limits, self.entries.rows.len() + 1)?;
        // What the document holds for the member.
        self.entries.parsed += MEMBER + owned;
        let offset = self.offset;
        let value_span = if self.values {
            let held = value();
            self.entries.values.push(held.value);
            held.span
                .map(|s| ByteSpan::new(s.start + offset, s.end + offset))
        } else {
            None
        };
        self.entries.rows.push(EntryAt {
            path,
            key_span: ByteSpan::new(span.start + offset, span.end + offset),
            value_span,
        });
        Ok(())
    }
}

/// Roughly what a `toml_edit` document holds on the heap for one member:
/// its key, its item, and its place in its table's map, twice over, as a
/// map that grows by doubling its room may hold twice its members.
const MEMBER: usize = 2 * (std::mem::size_of::<Key>() + std::mem::size_of::<Item>() + 32);

/// The text a document holds of its own for a member: its key, and its
/// value when that is a string.
fn text_of(key: &str, value: Option<&Value>) -> usize {
    key.len()
        + match value {
            Some(Value::String(string)) => string.value().len(),
            _ => 0,
        }
}

/// Roughly what a document holds below `item`, an array or an array of
/// tables: each element, each table's members and their text, and what is
/// below those.
/// An explicit stack, so nesting cannot exhaust the call stack.
fn below(item: &Item) -> usize {
    enum Node<'n> {
        Item(&'n Item),
        Value(&'n Value),
    }
    let mut held = 0;
    let mut pending = vec![Node::Item(item)];
    while let Some(node) = pending.pop() {
        match node {
            Node::Item(Item::Value(value)) => pending.push(Node::Value(value)),
            Node::Item(Item::Table(table)) => {
                for (key, item) in table {
                    held += MEMBER + key.len();
                    pending.push(Node::Item(item));
                }
            }
            Node::Item(Item::ArrayOfTables(tables)) => {
                for table in tables {
                    held += std::mem::size_of::<Item>();
                    for (key, item) in table {
                        held += MEMBER + key.len();
                        pending.push(Node::Item(item));
                    }
                }
            }
            Node::Value(Value::Array(array)) => {
                for value in array {
                    held += std::mem::size_of::<Item>();
                    pending.push(Node::Value(value));
                }
            }
            Node::Value(Value::InlineTable(table)) => {
                for (key, value) in table {
                    held += MEMBER + key.len();
                    pending.push(Node::Value(value));
                }
            }
            Node::Value(Value::String(string)) => held += string.value().len(),
            Node::Item(Item::None) | Node::Value(_) => {}
        }
    }
    held
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
    /// The value mode's rows for one inline table: a row per key it holds,
    /// as [`Self::table`] reads a table, and each inline table or (in
    /// `records:` mode) array among them, in `children` to descend next.
    fn inline_members<'t>(
        &mut self,
        table: &'t InlineTable,
        children: &mut Vec<(Segment, WalkStep<'t>)>,
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
            let segment = Segment::Key(key_text.into());
            self.stack.enter(segment.clone());
            let at = self.stack.path(&mut self.entries.paths);
            self.stack.leave();
            let text = self.text;
            self.push(at, span, text_of(key_text, Some(value)), || {
                value_of(value, text)
            })?;
            if let Value::InlineTable(inner) = value {
                children.push((segment, WalkStep::Inline(inner)));
            } else if self.descend_arrays
                && let Value::Array(array) = value
            {
                children.push((segment, WalkStep::Array(array)));
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

    /// What the parse held counts the text the document keeps of its own:
    /// each key and each string value, in a table, a dotted key, an inline
    /// table, an array and an array of tables. Beside the lexer's tokens,
    /// counted by the text's length, a long key and value add their length.
    #[test]
    fn the_parse_held_counts_each_key_and_string_the_document_keeps() {
        const LONG: usize = 1 << 16;
        for shape in [
            "@K = \"@V\"\n",
            "s.@K = '@V'\n",
            "t = { @K = \"@V\" }\n",
            "a = [\"@V\", { @K = 1 }]\n",
            "[[t]]\n@K = \"@V\"\n",
        ] {
            // The document with `key` and `value`, padded by a comment to
            // one length, so the lexer's tokens count alike.
            let parsed = |key: &str, value: &str| {
                let text = shape.replace("@K", key).replace("@V", value);
                let pad = "-".repeat(2 * LONG - key.len() - value.len());
                let text = format!("{text}# {pad}\n");
                let entries = TomlRowsAdapter::values(&text, &Limits::default()).unwrap();
                (entries.parsed, text.len())
            };
            let (long, length) = parsed(&"k".repeat(LONG), &"v".repeat(LONG));
            let (short, same) = parsed("k", "v");
            assert_eq!(length, same, "{shape}");
            assert_eq!(long - short, 2 * (LONG - 1), "{shape}");
        }
    }

    #[test]
    fn values_with_arrays_descends_tables_and_keeps_empty_elements_lazy() {
        let doc = concat!(
            "[[items]]\n",
            "name = \"api\"\n",
            "port = 1_000\n",
            "inline = [{ tag = \"v1\" }, {}]\n",
            "[[items]]\n",
        );
        let table = TomlRowsAdapter::values_with_arrays(doc, &Limits::default()).unwrap();
        let pointers: Vec<String> = table
            .rows
            .iter()
            .map(|r| table.paths.pointer(r.path).render())
            .collect();
        assert!(
            pointers.contains(&"items[0].name".to_string()),
            "{pointers:?}"
        );
        assert!(
            pointers.contains(&"items[0].port".to_string()),
            "{pointers:?}"
        );
        assert!(
            pointers.contains(&"items[0].inline[0].tag".to_string()),
            "{pointers:?}"
        );
        // Empty `[[items]]` and empty `{}` inline table allocate zero KeyPaths nodes.
        let empty_many = format!(
            "a = [{}]\n",
            std::iter::repeat_n("{}", 1_000)
                .collect::<Vec<_>>()
                .join(",")
        );
        let tight = Limits::default()
            .with(crate::relation::LimitKind::RowsPerExtractor, 5)
            .unwrap();
        let empty_table = TomlRowsAdapter::values_with_arrays(&empty_many, &tight).unwrap();
        assert_eq!(empty_table.rows.len(), 1, "only top-level `a` row");
        assert_eq!(
            empty_table.paths.len(),
            2,
            "lazy PathStack must allocate only ROOT + `a`, not 1,000 empty inline tables"
        );
    }

    #[test]
    fn multi_bom_and_whitespace_only_toml_keep_accurate_spans() {
        let text = "\u{feff}\u{feff}k = \"v\"\n";
        let out = output(text);
        assert_eq!(located(text, &out.rows), ["k @ \"k\""]);
        let vals = TomlRowsAdapter::values(text, &Limits::default()).unwrap();
        assert_eq!(&text[vals.rows[0].value_span.unwrap().as_range()], "\"v\"");
        assert_eq!(output(" \u{feff} \t\n").rows, []);
    }
}
