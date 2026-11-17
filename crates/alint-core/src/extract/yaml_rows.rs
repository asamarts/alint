//! Duplicate-aware entry rows of a YAML document, walked over the parser's
//! events (see [`super::extract_entry_rows`]), with each row's value when a
//! value-bearing relation asks for it ([`super::extract_entry_values_bounded`]).

use super::{ByteSpan, EntryAt, EntryRowsError, EntryTable, EntryValue, PathStack, entry_capacity};
use crate::relation::Segment;

pub(super) fn yaml_entry_table(
    text: &str,
    reject_yaml_features: bool,
    max_rows: Option<usize>,
    values: bool,
) -> std::result::Result<EntryTable, EntryRowsError> {
    use saphyr_parser::{Event, Parser};

    let stripped = text.trim_start_matches('\u{feff}');
    let offset = text.len() - stripped.len();
    if stripped.len() > crate::structured_format::MAX_STRUCTURED_BYTES {
        return Err(EntryRowsError::Parse(format!(
            "input exceeds the maximum supported size for a structured document ({} bytes)",
            crate::structured_format::MAX_STRUCTURED_BYTES
        )));
    }
    // This input's share of the YAML parse budget, held until the rows are
    // built (`yaml_budget`, ADR-0042).
    let _budget = crate::yaml_budget::reserve(stripped.len());
    // Keep the lexical flow guard ahead of saphyr's scanner, which refuses a
    // 256th level of flow nesting of its own but does not bound block nesting
    // (the event loop below bounds both): this one refuses flow nesting past
    // 128, as the parser alint decodes with does.
    let yaml_guards = crate::yaml_depth::scan_guards(stripped);
    if !yaml_guards.flow_within_limit() {
        return Err(EntryRowsError::Parse(
            "YAML flow nesting exceeds the supported limit".to_string(),
        ));
    }

    // Iterate the parser's non-recursive event interface rather than `load`,
    // whose receiver walk recursively descends collections. This bounds block
    // and flow collections, including compact `- -` / `- key:` forms, before
    // this module performs any recursive traversal.
    let mut events = Vec::new();
    let mut collection_depth = 0usize;
    for parsed in Parser::new_from_str(stripped) {
        let (event, span) = parsed.map_err(|error| EntryRowsError::Parse(error.to_string()))?;
        match &event {
            Event::SequenceStart(..) | Event::MappingStart(..) => {
                collection_depth += 1;
                if collection_depth > crate::yaml_depth::MAX_YAML_FLOW_DEPTH {
                    return Err(EntryRowsError::Parse(
                        "YAML nesting exceeds the supported limit".to_string(),
                    ));
                }
            }
            Event::SequenceEnd | Event::MappingEnd => {
                collection_depth = collection_depth.saturating_sub(1);
            }
            _ => {}
        }
        events.push((event, span));
    }
    let documents = events
        .iter()
        .filter(|(event, _)| matches!(event, Event::DocumentStart(_)))
        .count();
    if documents > 1 {
        return Err(EntryRowsError::Parse(
            "multi-document YAML is not supported for entry extraction".to_string(),
        ));
    }
    // This guard drives a recursive discard-only deserializer when aliases are
    // present, so run it only after collection depth is known safe and before
    // traversing or materializing the parser result. saphyr reads what ayaml
    // may refuse, so a text the guard could not read is refused too.
    match yaml_guards.expansion(stripped) {
        crate::yaml_depth::Expansion::WithinLimit => {}
        crate::yaml_depth::Expansion::OverLimit => {
            return Err(EntryRowsError::Parse(
                "YAML alias expansion exceeds the maximum supported size".to_string(),
            ));
        }
        crate::yaml_depth::Expansion::Unparsed(error) => {
            return Err(EntryRowsError::Parse(format!(
                "the YAML uses aliases, and their expansion cannot be bounded: {error}"
            )));
        }
    }

    let byte_offsets = YamlByteOffsets::new(stripped, &events);
    let mut index = events
        .iter()
        .position(|(event, _)| {
            matches!(
                event,
                Event::Scalar(..)
                    | Event::SequenceStart(..)
                    | Event::MappingStart(..)
                    | Event::Alias(..)
            )
        })
        .unwrap_or(events.len());
    let mut walk = YamlWalk {
        events: &events,
        byte_offsets: &byte_offsets,
        offset,
        text: stripped,
        reject_yaml_features,
        max_rows,
        values,
        table: EntryTable::default(),
        stack: PathStack::default(),
    };
    if index < events.len() {
        walk.node(&mut index, true)?;
    }
    Ok(walk.table)
}

enum YamlByteOffsets {
    Ascii(usize),
    Utf8(Vec<(usize, usize)>),
}

impl YamlByteOffsets {
    fn new(text: &str, events: &[(saphyr_parser::Event<'_>, saphyr_parser::Span)]) -> Self {
        use saphyr_parser::Event;

        // Delay endpoint allocation until the first non-ASCII character. Before
        // that point character and byte indices are identical, so an all-ASCII
        // document needs no mapping allocation and a UTF-8 document is still
        // mapped in one pass over its source.
        let mut endpoints: Option<Vec<(usize, usize)>> = None;
        let mut needed = 0;
        let mut character_len = 0;
        for (character, (byte, value)) in text.char_indices().enumerate() {
            if endpoints.is_none() && !value.is_ascii() {
                let mut discovered = Vec::with_capacity(events.len() * 2);
                for (event, span) in events {
                    match event {
                        Event::Scalar(
                            _,
                            saphyr_parser::ScalarStyle::Literal
                            | saphyr_parser::ScalarStyle::Folded,
                            ..,
                        ) => {}
                        Event::Scalar(..) | Event::Alias(..) => {
                            discovered.push((span.start.index(), usize::MAX));
                            discovered.push((span.end.index(), usize::MAX));
                        }
                        Event::SequenceStart(..) | Event::MappingStart(..) => {
                            discovered.push((span.start.index(), usize::MAX));
                        }
                        Event::SequenceEnd | Event::MappingEnd => {
                            discovered.push((span.end.index(), usize::MAX));
                        }
                        _ => {}
                    }
                }
                discovered.sort_unstable_by_key(|(endpoint, _)| *endpoint);
                discovered.dedup_by_key(|(endpoint, _)| *endpoint);
                while discovered
                    .get(needed)
                    .is_some_and(|(endpoint, _)| *endpoint <= character)
                {
                    discovered[needed].1 = discovered[needed].0;
                    needed += 1;
                }
                endpoints = Some(discovered);
            }
            if let Some(endpoints) = endpoints.as_mut() {
                while endpoints
                    .get(needed)
                    .is_some_and(|(endpoint, _)| *endpoint == character)
                {
                    endpoints[needed].1 = byte;
                    needed += 1;
                }
            }
            character_len = character + 1;
        }
        let Some(mut endpoints) = endpoints else {
            return Self::Ascii(text.len());
        };
        while endpoints
            .get(needed)
            .is_some_and(|(endpoint, _)| *endpoint == character_len)
        {
            endpoints[needed].1 = text.len();
            needed += 1;
        }
        Self::Utf8(endpoints)
    }

    fn byte(&self, character: usize) -> Option<usize> {
        match self {
            Self::Ascii(len) => (character <= *len).then_some(character),
            Self::Utf8(endpoints) => endpoints
                .binary_search_by_key(&character, |(endpoint, _)| *endpoint)
                .ok()
                .and_then(|index| {
                    let byte = endpoints[index].1;
                    (byte != usize::MAX).then_some(byte)
                }),
        }
    }
}

struct YamlNode {
    scalar: Option<String>,
    span: Option<ByteSpan>,
    /// Where the node's event starts: a key's location when it has no span
    /// (a block scalar, or an empty key).
    start: Option<usize>,
    style: Option<saphyr_parser::ScalarStyle>,
    /// What the node holds, when the walk records values.
    value: Option<EntryValue>,
}

/// One walk over a document's events, emitting a row per mapping entry and,
/// when `values` is set, each row's value beside it.
struct YamlWalk<'w, 'e> {
    events: &'w [(saphyr_parser::Event<'e>, saphyr_parser::Span)],
    byte_offsets: &'w YamlByteOffsets,
    offset: usize,
    text: &'w str,
    reject_yaml_features: bool,
    max_rows: Option<usize>,
    values: bool,
    table: EntryTable,
    stack: PathStack,
}

/// What a scalar event holds, typed as saphyr's loader types it under the
/// core schema: a quoted or block scalar is a string, a plain one is null, a
/// boolean, a number (kept as written: `1.10` is not `1.1`) or a string, and a
/// core tag that cannot read its scalar (`!!int abc`) leaves it unreadable.
fn scalar_value(
    text: &str,
    style: saphyr_parser::ScalarStyle,
    tag: Option<&std::borrow::Cow<'_, saphyr_parser::Tag>>,
) -> EntryValue {
    use crate::relation::{Number, Value};
    match saphyr::Scalar::parse_from_cow_and_metadata(std::borrow::Cow::Borrowed(text), style, tag)
    {
        Some(saphyr::Scalar::Null) => EntryValue::Scalar(Value::Null),
        Some(saphyr::Scalar::Boolean(flag)) => EntryValue::Scalar(Value::Bool(flag)),
        Some(saphyr::Scalar::Integer(_) | saphyr::Scalar::FloatingPoint(_)) => {
            Number::from_lexeme(text).map_or_else(
                || EntryValue::Scalar(Value::String(text.into())),
                |number| EntryValue::Scalar(Value::Number(number)),
            )
        }
        Some(saphyr::Scalar::String(string)) => {
            EntryValue::Scalar(Value::String(string.as_ref().into()))
        }
        None => EntryValue::Unreadable,
    }
}

impl YamlWalk<'_, '_> {
    #[allow(clippy::too_many_lines)]
    fn node(
        &mut self,
        index: &mut usize,
        emit: bool,
    ) -> std::result::Result<YamlNode, EntryRowsError> {
        use saphyr_parser::Event;

        let (events, byte_offsets, offset, text) =
            (self.events, self.byte_offsets, self.offset, self.text);
        let reject_yaml_features = self.reject_yaml_features;
        let max_rows = self.max_rows;
        let Some((event, event_span)) = events.get(*index) else {
            return Err(EntryRowsError::Parse(
                "unexpected end of YAML event stream".to_string(),
            ));
        };
        let event_start = byte_offsets
            .byte(event_span.start.index())
            .map(|start| start + offset);
        match event {
            Event::Scalar(value, style, _, tag) => {
                *index += 1;
                Ok(YamlNode {
                    scalar: Some(value.to_string()),
                    span: yaml_scalar_span(*event_span, *style, byte_offsets, offset, text),
                    start: event_start,
                    style: Some(*style),
                    value: self
                        .values
                        .then(|| scalar_value(value, *style, tag.as_ref())),
                })
            }
            Event::Alias(_) => {
                if reject_yaml_features {
                    return Err(EntryRowsError::Parse(
                        "YAML aliases are not supported for key extraction".to_string(),
                    ));
                }
                *index += 1;
                Ok(YamlNode {
                    scalar: None,
                    span: yaml_token_span(*event_span, byte_offsets, offset, text, false),
                    start: event_start,
                    style: None,
                    value: Some(EntryValue::Unreadable),
                })
            }
            Event::SequenceStart(..) => {
                let start = *event_span;
                *index += 1;
                let mut item = 0;
                while !matches!(events.get(*index), Some((Event::SequenceEnd, _))) {
                    self.stack.enter(Segment::Index(item));
                    self.node(index, emit)?;
                    self.stack.leave();
                    item += 1;
                }
                let end = events[*index].1;
                *index += 1;
                Ok(YamlNode {
                    scalar: None,
                    span: yaml_flow_container_span(
                        start,
                        end,
                        byte_offsets,
                        offset,
                        text,
                        b'[',
                        b']',
                    ),
                    start: event_start,
                    style: None,
                    value: Some(EntryValue::Array),
                })
            }
            Event::MappingStart(..) => {
                let start = *event_span;
                *index += 1;
                while !matches!(events.get(*index), Some((Event::MappingEnd, _))) {
                    let key = self.node(index, false)?;
                    if let Some(key_text) = key.scalar {
                        if reject_yaml_features
                            && key_text == "<<"
                            && key.style == Some(saphyr_parser::ScalarStyle::Plain)
                        {
                            return Err(EntryRowsError::Parse(
                                "YAML merge keys are not supported for key extraction".to_string(),
                            ));
                        }
                        let row_index = if emit {
                            entry_capacity(self.table.rows.len(), max_rows)?;
                            // A block scalar key and an empty key have no
                            // token span, so the row sits where the key
                            // starts: a row without an exact span is better
                            // than a key dropped without a word.
                            let key_span = key.span.unwrap_or_else(|| {
                                let start = key.start.unwrap_or(offset);
                                ByteSpan::new(start, start)
                            });
                            let parent = self.stack.path(&mut self.table.paths);
                            let path = self
                                .table
                                .paths
                                .child(parent, Segment::Key(key_text.as_str().into()));
                            self.table.rows.push(EntryAt {
                                path,
                                key_span,
                                value_span: None,
                            });
                            if self.values {
                                self.table.values.push(EntryValue::Unreadable);
                            }
                            self.stack.enter_path(path);
                            Some(self.table.rows.len() - 1)
                        } else {
                            self.stack.enter(Segment::Key(key_text.as_str().into()));
                            None
                        };
                        let value = self.node(index, emit)?;
                        self.stack.leave();
                        if let Some(row_index) = row_index {
                            self.table.rows[row_index].value_span = value.span;
                            if self.values
                                && let Some(held) = value.value
                            {
                                self.table.values[row_index] = held;
                            }
                        }
                    } else if reject_yaml_features {
                        // A sequence or mapping key has no key path; reading its
                        // value under the parent would invent keys there.
                        return Err(EntryRowsError::Parse(
                        "YAML complex (non-scalar) mapping keys are not supported for key extraction"
                            .to_string(),
                    ));
                    } else {
                        self.node(index, emit)?;
                    }
                }
                let end = events[*index].1;
                *index += 1;
                Ok(YamlNode {
                    scalar: None,
                    span: yaml_flow_container_span(
                        start,
                        end,
                        byte_offsets,
                        offset,
                        text,
                        b'{',
                        b'}',
                    ),
                    start: event_start,
                    style: None,
                    value: Some(EntryValue::Object),
                })
            }
            _ => Err(EntryRowsError::Parse(format!(
                "unexpected YAML event {event:?}"
            ))),
        }
    }
}

fn yaml_flow_container_span(
    start: saphyr_parser::Span,
    end: saphyr_parser::Span,
    byte_offsets: &YamlByteOffsets,
    offset: usize,
    text: &str,
    open: u8,
    close: u8,
) -> Option<ByteSpan> {
    let start = byte_offsets.byte(start.start.index())?;
    if text.as_bytes().get(start) != Some(&open) {
        // Block container event spans are token boundaries, not complete node
        // spans (indentless sequences can even start after their first dash).
        // Decline them rather than claiming a partial or whitespace-padded value.
        return None;
    }
    let parser_end = byte_offsets.byte(end.end.index())?;
    let close_at = text
        .as_bytes()
        .get(start..parser_end)?
        .iter()
        .rposition(|byte| *byte == close)?
        + start;
    if !text[close_at + 1..parser_end].trim().is_empty() {
        return None;
    }
    Some(ByteSpan::new(start + offset, close_at + 1 + offset))
}

fn yaml_scalar_span(
    span: saphyr_parser::Span,
    style: saphyr_parser::ScalarStyle,
    byte_offsets: &YamlByteOffsets,
    offset: usize,
    text: &str,
) -> Option<ByteSpan> {
    if matches!(
        style,
        saphyr_parser::ScalarStyle::Literal | saphyr_parser::ScalarStyle::Folded
    ) {
        // Saphyr's scalar event does not span the complete indicator plus body.
        return None;
    }
    yaml_token_span(span, byte_offsets, offset, text, true)
}

fn yaml_token_span(
    span: saphyr_parser::Span,
    byte_offsets: &YamlByteOffsets,
    offset: usize,
    text: &str,
    clamp_quoted: bool,
) -> Option<ByteSpan> {
    let start = byte_offsets.byte(span.start.index())?;
    let mut end = byte_offsets.byte(span.end.index())?;
    if start == end {
        // Implicit empty mapping values are represented by a zero-width scalar
        // event at the `:` boundary; that is not a value token.
        return None;
    }
    if clamp_quoted && let Some(open @ (b'"' | b'\'')) = text.as_bytes().get(start).copied() {
        let mut at = start + 1;
        while at < end {
            match text.as_bytes()[at] {
                b'\\' if open == b'"' => at += 2,
                b'\'' if open == b'\'' && text.as_bytes().get(at + 1) == Some(&b'\'') => at += 2,
                byte if byte == open => {
                    end = at + 1;
                    break;
                }
                _ => at += 1,
            }
        }
    }
    Some(ByteSpan::new(start + offset, end + offset))
}
