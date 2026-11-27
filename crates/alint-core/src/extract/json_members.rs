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
    pub(crate) fn parsed_with_strict(text: &'t str, strict: bool) -> Result<Self, EntryRowsError> {
        Self::read(text, None, strict)
    }

    /// The syntax tree of `text`, read through [`Format::parse`]'s guards
    /// first, as an `entries:` part reads it.
    pub(crate) fn guarded_with_strict(text: &'t str, strict: bool) -> Result<Self, EntryRowsError> {
        #[cfg(test)]
        super::tests::JSON_GUARDS.with(|guards| guards.set(guards.get() + 1));
        let (_, copied) = Format::Json
            .parse_held_with_strict_json(text, strict)
            .map_err(EntryRowsError::Parse)?;
        Self::read(text, Some(copied), strict)
    }

    /// The tree of `text`; `guarded` with what the guards held besides
    /// their tree (a JSONC text's stripped copy) when they read it first.
    fn read(text: &'t str, guarded: Option<usize>, strict: bool) -> Result<Self, EntryRowsError> {
        let stripped = if strict {
            text
        } else {
            if text
                .trim_matches(|c: char| c == '\u{feff}' || c.is_whitespace())
                .is_empty()
            {
                return Ok(Self {
                    root: None,
                    offset: text.len(),
                    held: guarded.unwrap_or(0),
                });
            }
            text.trim_start_matches('\u{feff}')
        };
        let offset = text.len() - stripped.len();
        let root = parse_to_ast(
            stripped,
            &CollectOptions {
                comments: CommentCollectionStrategy::Off,
                tokens: false,
            },
            &json_parse_options(strict),
        )
        .map_err(|error| EntryRowsError::Parse(error.to_string()))?
        .value;
        let mut syntax = Self {
            root,
            offset,
            held: 0,
        };
        let (tree, guards) = syntax.sizes();
        syntax.held = guarded.map_or(tree, |copied| tree.max(guards + copied));
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
    ///
    /// Built as a prefix trie over `paths` and resolved in a single traversal
    /// of `self.root` (`O(N + M * avg_depth)` instead of `O(M * N)`).
    pub(crate) fn member_spans(&self, paths: &[Option<Vec<Segment>>]) -> Vec<Option<ByteSpan>> {
        use std::collections::HashMap;

        #[derive(Default)]
        struct TrieNode<'a> {
            keys: HashMap<&'a str, usize>,
            indices: HashMap<usize, usize>,
            queries: Vec<usize>,
        }

        #[derive(Clone, Copy)]
        enum Hit {
            None,
            One(ByteSpan),
            Many,
        }

        let Some(root) = self.root.as_ref() else {
            return vec![None; paths.len()];
        };
        let mut nodes: Vec<TrieNode<'_>> = vec![TrieNode::default()];
        for (qi, path) in paths.iter().enumerate() {
            let Some(segments) = path.as_deref() else {
                continue;
            };
            if !matches!(segments.last(), Some(Segment::Key(_))) {
                continue;
            }
            let mut cur = 0usize;
            for seg in segments {
                cur = match seg {
                    Segment::Key(key) => {
                        if let Some(&next) = nodes[cur].keys.get(&**key) {
                            next
                        } else {
                            let next = nodes.len();
                            nodes.push(TrieNode::default());
                            nodes[cur].keys.insert(key, next);
                            next
                        }
                    }
                    Segment::Index(idx) => {
                        if let Some(&next) = nodes[cur].indices.get(idx) {
                            next
                        } else {
                            let next = nodes.len();
                            nodes.push(TrieNode::default());
                            nodes[cur].indices.insert(*idx, next);
                            next
                        }
                    }
                };
            }
            nodes[cur].queries.push(qi);
        }
        if nodes[0].keys.is_empty() && nodes[0].indices.is_empty() {
            return vec![None; paths.len()];
        }

        let mut hits = vec![Hit::None; paths.len()];
        let mut stack: Vec<(usize, &Value<'_>)> = vec![(0, root)];
        while let Some((node_idx, value)) = stack.pop() {
            match value {
                Value::Object(object) if !nodes[node_idx].keys.is_empty() => {
                    for property in &object.properties {
                        if let Some(&child) = nodes[node_idx].keys.get(name(property)) {
                            if !nodes[child].queries.is_empty() {
                                let span = self.span(property_value_range(property));
                                for &qi in &nodes[child].queries {
                                    hits[qi] = match hits[qi] {
                                        Hit::None => Hit::One(span),
                                        Hit::One(_) | Hit::Many => Hit::Many,
                                    };
                                }
                            }
                            if !nodes[child].keys.is_empty() || !nodes[child].indices.is_empty() {
                                stack.push((child, &property.value));
                            }
                        }
                    }
                }
                Value::Array(array) if !nodes[node_idx].indices.is_empty() => {
                    for (&idx, &child) in &nodes[node_idx].indices {
                        if let Some(elem) = array.elements.get(idx) {
                            stack.push((child, elem));
                        }
                    }
                }
                _ => {}
            }
        }
        hits.into_iter()
            .map(|hit| match hit {
                Hit::One(span) => Some(span),
                Hit::None | Hit::Many => None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relation::Segment;

    #[test]
    fn member_spans_handles_wide_objects_duplicates_and_interleaved_bom_whitespace() {
        use std::fmt::Write as _;
        let mut text = String::from("{");
        let mut paths: Vec<Option<Vec<Segment>>> = Vec::new();
        for i in 0..20_000 {
            if i > 0 {
                text.push(',');
            }
            let _ = write!(text, "\"k{i}\":{i}");
            if i % 20 == 0 {
                paths.push(Some(vec![Segment::Key(format!("k{i}").into())]));
            }
        }
        text.push('}');
        let syntax = JsonSyntax::read(&text, None, false).unwrap();
        let spans = syntax.member_spans(&paths);
        assert_eq!(spans.len(), paths.len());
        assert_eq!(&text[spans[0].unwrap().as_range()], "0");
        assert_eq!(&text[spans[1].unwrap().as_range()], "20");

        // Duplicate key yields None (ambiguous span).
        let dup_syntax = JsonSyntax::read(r#"{"a": 1, "a": 2}"#, None, false).unwrap();
        let dup_ptr = [Some(vec![Segment::Key("a".into())])];
        let dup_spans = dup_syntax.member_spans(&dup_ptr);
        assert_eq!(dup_spans, [None]);

        // Interleaved BOM + whitespace parses as empty document cleanly.
        let empty = JsonSyntax::read(" \u{feff} \t ", None, false).unwrap();
        let mut count = 0;
        empty.members(&mut |_| count += 1);
        assert_eq!(count, 0);
    }
}
