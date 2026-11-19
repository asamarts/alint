//! A JSON document's members read from its syntax tree, without building
//! its entries: where placing a finding looks for a `read:` value by its
//! key path, or checks an entry's key or value at its span. The entries of
//! a large document (each key path held, numbered and mapped) held more
//! than the tree they were read from; placing needs only the members it
//! looks at.

use jsonc_parser::ast::{ObjectProp, ObjectPropName, StringLit, Value};
use jsonc_parser::common::Ranged as _;
use jsonc_parser::{CollectOptions, CommentCollectionStrategy, parse_to_ast};

use super::{ByteSpan, EntryRowsError, EntryValue, json_entry_value, json_parse_options};
use crate::relation::Segment;
use crate::structured_format::Format;

/// A JSON text's syntax tree, read as an `entries:` part reads it.
pub(crate) struct JsonSyntax<'t> {
    root: Option<Value<'t>>,
    /// Where the tree's text starts in the file: past a BOM.
    offset: usize,
    /// Roughly what the tree holds on the heap, and, read through the
    /// guards, what the tree they parsed the text to held before it.
    held: usize,
}

impl<'t> JsonSyntax<'t> {
    /// The syntax tree of `text`, which [`Format::parse`] has read just
    /// now, so its guards (size, depth and dialect) passed: they do not
    /// read it whole once more.
    pub(crate) fn parsed(text: &'t str) -> Result<Self, EntryRowsError> {
        Self::read(text, false)
    }

    /// The syntax tree of `text`, read through [`Format::parse`]'s guards
    /// first, as an `entries:` part reads it.
    pub(crate) fn guarded(text: &'t str) -> Result<Self, EntryRowsError> {
        #[cfg(test)]
        super::tests::JSON_GUARDS.with(|guards| guards.set(guards.get() + 1));
        Format::Json.parse(text).map_err(EntryRowsError::Parse)?;
        Self::read(text, true)
    }

    fn read(text: &'t str, guarded: bool) -> Result<Self, EntryRowsError> {
        let stripped = text.trim_start_matches('\u{feff}');
        let offset = text.len() - stripped.len();
        let root = parse_to_ast(
            stripped,
            &CollectOptions {
                comments: CommentCollectionStrategy::Off,
                tokens: false,
            },
            &json_parse_options(),
        )
        .map_err(|error| EntryRowsError::Parse(error.to_string()))?
        .value;
        let mut syntax = Self {
            root,
            offset,
            held: 0,
        };
        let (tree, guards) = syntax.sizes();
        syntax.held = if guarded { tree.max(guards) } else { tree };
        Ok(syntax)
    }

    /// Roughly what the syntax tree holds on the heap (each object's
    /// members, each array's elements, and a string the parser unescaped
    /// into text of its own), and what the guards' `serde_json` tree held
    /// (a value per node, a key and a slot per member, each string's text,
    /// as [`TREE_MEMBER`] counts them).
    fn sizes(&self) -> (usize, usize) {
        let (mut tree, mut guards) = (std::mem::size_of::<Value<'_>>(), 0);
        let mut pending: Vec<&Value<'_>> = self.root.iter().collect();
        while let Some(value) = pending.pop() {
            guards += std::mem::size_of::<serde_json::Value>();
            match value {
                Value::Object(object) => {
                    tree += object.properties.capacity() * std::mem::size_of::<ObjectProp<'_>>();
                    for property in &object.properties {
                        if let ObjectPropName::String(literal) = &property.name {
                            tree += owned(literal);
                        }
                        guards += name(property).len() + TREE_MEMBER;
                        pending.push(&property.value);
                    }
                }
                Value::Array(array) => {
                    tree += array.elements.capacity() * std::mem::size_of::<Value<'_>>();
                    pending.extend(&array.elements);
                }
                Value::StringLit(literal) => {
                    tree += owned(literal);
                    guards += literal.value.len();
                }
                Value::NumberLit(_) | Value::BooleanLit(_) | Value::NullKeyword(_) => {}
            }
        }
        (tree, guards)
    }

    /// Roughly the most reading the tree held at once besides the text.
    pub(crate) fn held(&self) -> usize {
        self.held
    }

    /// The span of the value of the one member at each of `paths` (key
    /// paths ending in a key), as the document's entries place it: none for
    /// no path, and when no member or several (a repeated key, here or
    /// above it) are at the path.
    pub(crate) fn member_spans(&self, paths: &[Option<Vec<Segment>>]) -> Vec<Option<ByteSpan>> {
        paths
            .iter()
            .map(|path| {
                let (Segment::Key(key), above) = path.as_ref()?.split_last()? else {
                    return None;
                };
                let mut holders: Vec<&Value<'_>> = self.root.iter().collect();
                for segment in above {
                    holders = holders
                        .into_iter()
                        .flat_map(|holder| below(holder, segment))
                        .collect();
                }
                let mut members = holders.into_iter().flat_map(|holder| match holder {
                    Value::Object(object) => object
                        .properties
                        .iter()
                        .filter(|property| name(property) == &**key)
                        .collect(),
                    _ => Vec::new(),
                });
                let member = members.next()?;
                members
                    .next()
                    .is_none()
                    .then(|| self.span(property_value_range(member)))
            })
            .collect()
    }

    /// Each member of the document, in no set order: its key's span and
    /// key, and its value's span and what the value holds, made when
    /// `visit` asks.
    pub(crate) fn members(&self, visit: &mut dyn FnMut(Member<'_, '_>)) {
        let mut pending: Vec<&Value<'_>> = self.root.iter().collect();
        while let Some(value) = pending.pop() {
            match value {
                Value::Object(object) => {
                    for property in &object.properties {
                        let key_range = match &property.name {
                            ObjectPropName::String(literal) => literal.range(),
                            ObjectPropName::Word(literal) => literal.range(),
                        };
                        visit(Member {
                            key_span: self.span(key_range),
                            key: name(property),
                            value_span: self.span(property_value_range(property)),
                            value: &property.value,
                        });
                        pending.push(&property.value);
                    }
                }
                Value::Array(array) => pending.extend(&array.elements),
                Value::StringLit(_)
                | Value::NumberLit(_)
                | Value::BooleanLit(_)
                | Value::NullKeyword(_) => {}
            }
        }
    }

    fn span(&self, range: jsonc_parser::common::Range) -> ByteSpan {
        ByteSpan::new(range.start + self.offset, range.end + self.offset)
    }
}

/// Roughly what a `serde_json` tree holds for an object's member besides
/// its key's text and its value: its key and value in the map's node, twice
/// over, as a map's nodes are filled half way when its keys come in order.
pub(crate) const TREE_MEMBER: usize = 2 * std::mem::size_of::<(String, serde_json::Value)>();

/// The bytes of text of its own a parsed string holds: none when it
/// borrows the document's.
fn owned(literal: &StringLit<'_>) -> usize {
    match &literal.value {
        std::borrow::Cow::Borrowed(_) => 0,
        std::borrow::Cow::Owned(text) => text.capacity(),
    }
}

/// One member of a JSON document, as [`JsonSyntax::members`] reads it.
pub(crate) struct Member<'m, 't> {
    pub(crate) key_span: ByteSpan,
    pub(crate) key: &'m str,
    pub(crate) value_span: ByteSpan,
    value: &'m Value<'t>,
}

impl Member<'_, '_> {
    /// What the member's value holds, as an `entries:` part reads it.
    pub(crate) fn value(&self) -> EntryValue {
        json_entry_value(self.value)
    }
}

fn name<'p>(property: &'p ObjectProp<'_>) -> &'p str {
    match &property.name {
        ObjectPropName::String(literal) => literal.value.as_ref(),
        ObjectPropName::Word(literal) => literal.value,
    }
}

fn property_value_range(property: &ObjectProp<'_>) -> jsonc_parser::common::Range {
    property.value.range()
}

/// The values at `segment` below `holder`: an object's members of that key,
/// or an array's element at that index.
fn below<'v, 't>(holder: &'v Value<'t>, segment: &Segment) -> Vec<&'v Value<'t>> {
    match (holder, segment) {
        (Value::Object(object), Segment::Key(key)) => object
            .properties
            .iter()
            .filter(|property| name(property) == &**key)
            .map(|property| &property.value)
            .collect(),
        (Value::Array(array), Segment::Index(index)) => {
            array.elements.get(*index).into_iter().collect()
        }
        _ => Vec::new(),
    }
}
