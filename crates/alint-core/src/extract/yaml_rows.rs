//! Duplicate-aware entry rows of a YAML document, walked over `ayaml`'s
//! spanned events (see [`super::extract_entry_rows`]), with each row's value
//! when a value-bearing relation asks for it
//! ([`super::extract_entry_values_bounded`]).

use super::{ByteSpan, EntryAt, EntryRowsError, EntryTable, EntryValue, PathStack, entry_capacity};
use crate::relation::Segment;
use crate::yaml::{CoreScalar, is_core_null, resolve_scalar_payload};
use ayaml::{CollectionStyle, Event, ScalarStyle, Span};

pub(super) fn yaml_entry_table(
    text: &str,
    reject_yaml_features: bool,
    max_rows: Option<usize>,
    values: bool,
) -> std::result::Result<EntryTable, EntryRowsError> {
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
    // Keep the lexical flow guard ahead of the event stream so deeply nested
    // flow collections fail before allocating parser state.
    let yaml_guards = crate::yaml_depth::scan_guards(stripped);
    if !yaml_guards.flow_within_limit() {
        return Err(EntryRowsError::Parse(
            "YAML flow nesting exceeds the supported limit".to_string(),
        ));
    }

    // Iterate `ayaml`'s lazy event stream, bounding block and flow collection
    // nesting before performing any recursive walk over the buffered events.
    let (events, owned_bytes) = collect_yaml_events(stripped)?;
    // Bound alias expansion before walking the document.
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

    let mut index = events
        .iter()
        .position(|event| {
            matches!(
                event,
                Event::Scalar { .. }
                    | Event::SequenceStart { .. }
                    | Event::MappingStart { .. }
                    | Event::Alias { .. }
            )
        })
        .unwrap_or(events.len());
    let mut walk = YamlWalk {
        events: &events,
        offset,
        text: stripped,
        reject_yaml_features,
        max_rows,
        values,
        table: EntryTable::default(),
        stack: PathStack::default(),
    };
    // The parse the table is read from: the events and any owned text they hold.
    walk.table.parsed = events.capacity() * std::mem::size_of::<Event<'_>>() + owned_bytes;
    if index < events.len() {
        walk.node(&mut index, true)?;
    }
    Ok(walk.table)
}

fn collect_yaml_events(
    stripped: &str,
) -> std::result::Result<(Vec<Event<'_>>, usize), EntryRowsError> {
    let mut events = Vec::new();
    let mut collection_depth = 0usize;
    let mut documents = 0usize;
    let mut owned_bytes = 0usize;
    let map_ayaml_err = |error: ayaml::Error| {
        if error
            .limit()
            .is_some_and(|limit| limit.kind() == ayaml::LimitKind::Depth)
        {
            EntryRowsError::Parse(format!("YAML nesting exceeds the supported limit: {error}"))
        } else {
            EntryRowsError::Parse(error.to_string())
        }
    };
    let stream = crate::yaml::parse_events_stream(stripped).map_err(map_ayaml_err)?;
    for parsed in stream {
        let event = parsed.map_err(map_ayaml_err)?;
        match &event {
            Event::DocumentStart { .. } => {
                documents += 1;
                if documents > 1 {
                    return Err(EntryRowsError::Parse(
                        "multi-document YAML is not supported for entry extraction".to_string(),
                    ));
                }
            }
            Event::Scalar {
                value,
                raw,
                tag,
                anchor,
                ..
            } => {
                if let std::borrow::Cow::Owned(text) = value {
                    owned_bytes += text.capacity();
                }
                if let std::borrow::Cow::Owned(text) = raw {
                    owned_bytes += text.capacity();
                }
                if let Some(tag) = tag {
                    owned_bytes += tag.handle().len() + tag.suffix().len();
                }
                if let Some(anchor) = anchor {
                    owned_bytes += anchor.as_str().len();
                }
            }
            Event::SequenceStart { tag, anchor, .. } | Event::MappingStart { tag, anchor, .. } => {
                collection_depth += 1;
                if collection_depth > crate::yaml_depth::MAX_YAML_FLOW_DEPTH {
                    return Err(EntryRowsError::Parse(
                        "YAML nesting exceeds the supported limit".to_string(),
                    ));
                }
                if let Some(tag) = tag {
                    owned_bytes += tag.handle().len() + tag.suffix().len();
                }
                if let Some(anchor) = anchor {
                    owned_bytes += anchor.as_str().len();
                }
            }
            Event::SequenceEnd { .. } | Event::MappingEnd { .. } => {
                collection_depth = collection_depth.saturating_sub(1);
            }
            _ => {}
        }
        events.push(event);
    }
    Ok((events, owned_bytes))
}

struct YamlNode {
    scalar: Option<String>,
    span: Option<ByteSpan>,
    /// Where the node's event starts: a key's location when it has no span
    /// (a block scalar, or an empty key).
    start: Option<usize>,
    style: Option<ScalarStyle>,
    /// What the node holds, when the walk records values.
    value: Option<EntryValue>,
}

/// One walk over a document's events, emitting a row per mapping entry and,
/// when `values` is set, each row's value beside it.
struct YamlWalk<'w, 'e> {
    events: &'w [Event<'e>],
    offset: usize,
    text: &'w str,
    reject_yaml_features: bool,
    max_rows: Option<usize>,
    values: bool,
    table: EntryTable,
    stack: PathStack,
}

/// What a scalar event holds, typed under the YAML 1.2 core schema: a quoted
/// or block scalar is a string, a plain one is null, a boolean, a number (kept
/// as written: `1.10` is not `1.1`) or a string, and a core tag that cannot
/// read its scalar (`!!int abc`) leaves it unreadable.
fn scalar_value(text: &str, style: ScalarStyle, tag: Option<&ayaml::Tag>) -> EntryValue {
    use crate::relation::{Number, Value};
    match resolve_scalar_payload(text, style, tag) {
        Some(CoreScalar::Null) => EntryValue::Scalar(Value::Null),
        Some(CoreScalar::Bool(flag)) => EntryValue::Scalar(Value::Bool(flag)),
        Some(CoreScalar::NegInt(_) | CoreScalar::Uint(_) | CoreScalar::Float(_)) => {
            Number::from_lexeme(text).map_or_else(
                || EntryValue::Scalar(Value::String(text.into())),
                |number| EntryValue::Scalar(Value::Number(number)),
            )
        }
        Some(CoreScalar::String(string)) => EntryValue::Scalar(Value::String(string.into())),
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
        let (events, offset, text) = (self.events, self.offset, self.text);
        let reject_yaml_features = self.reject_yaml_features;
        let max_rows = self.max_rows;
        let Some(event) = events.get(*index) else {
            return Err(EntryRowsError::Parse(
                "unexpected end of YAML event stream".to_string(),
            ));
        };
        let event_span = event.span();
        let event_start = Some(event_span.start() + offset);
        match event {
            Event::Scalar {
                value, style, tag, ..
            } => {
                *index += 1;
                Ok(YamlNode {
                    scalar: Some(value.to_string()),
                    span: yaml_scalar_span(event_span, *style, offset),
                    start: event_start,
                    style: Some(*style),
                    value: self
                        .values
                        .then(|| scalar_value(value, *style, tag.as_ref())),
                })
            }
            Event::Alias { .. } => {
                if reject_yaml_features {
                    return Err(EntryRowsError::Parse(
                        "YAML aliases are not supported for key extraction".to_string(),
                    ));
                }
                *index += 1;
                Ok(YamlNode {
                    scalar: None,
                    span: yaml_token_span(event_span, offset),
                    start: event_start,
                    style: None,
                    value: Some(EntryValue::Unreadable),
                })
            }
            Event::SequenceStart { style, .. } => {
                let start = event_span;
                let flow = *style == CollectionStyle::Flow;
                *index += 1;
                let mut item = 0;
                while !matches!(events.get(*index), Some(Event::SequenceEnd { .. })) {
                    self.stack.enter(Segment::Index(item));
                    self.node(index, emit)?;
                    self.stack.leave();
                    item += 1;
                }
                let end = events[*index].span();
                *index += 1;
                Ok(YamlNode {
                    scalar: None,
                    span: flow
                        .then(|| yaml_flow_container_span(start, end, offset, text, b'[', b']'))
                        .flatten(),
                    start: event_start,
                    style: None,
                    value: Some(EntryValue::Array),
                })
            }
            Event::MappingStart { style, .. } => {
                let start = event_span;
                let flow = *style == CollectionStyle::Flow;
                *index += 1;
                while !matches!(events.get(*index), Some(Event::MappingEnd { .. })) {
                    let key = self.node(index, false)?;
                    if let Some(key_text) = key.scalar {
                        if reject_yaml_features
                            && key_text == "<<"
                            && key.style == Some(ScalarStyle::Plain)
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
                let end = events[*index].span();
                *index += 1;
                Ok(YamlNode {
                    scalar: None,
                    span: flow
                        .then(|| yaml_flow_container_span(start, end, offset, text, b'{', b'}'))
                        .flatten(),
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
    start: Span,
    end: Span,
    offset: usize,
    text: &str,
    open: u8,
    close: u8,
) -> Option<ByteSpan> {
    let start = start.start();
    if text.as_bytes().get(start) != Some(&open) {
        return None;
    }
    let parser_end = end.end();
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

fn yaml_scalar_span(span: Span, style: ScalarStyle, offset: usize) -> Option<ByteSpan> {
    if matches!(style, ScalarStyle::Literal | ScalarStyle::Folded) {
        // Keep block scalars unspanned so placement and replacement stay
        // consistent with single-token spans.
        return None;
    }
    yaml_token_span(span, offset)
}

fn yaml_token_span(span: Span, offset: usize) -> Option<ByteSpan> {
    if span.is_empty() {
        // Implicit empty mapping values have a zero-width scalar span; that is
        // not a value token.
        return None;
    }
    Some(ByteSpan::new(span.start() + offset, span.end() + offset))
}

/// Inspect a YAML document's root shape in O(1) memory by stopping the event
/// stream at the first root value (or first single-key mapping value), never
/// materializing a value tree. Used by `key_parity`'s `strip_root` gate (#597).
pub(super) fn yaml_strip_root_shape(text: &str) -> super::StripRootShape {
    use super::StripRootShape;

    fn is_core_null_scalar(value: &str, style: ScalarStyle, tag: Option<&ayaml::Tag>) -> bool {
        if let Some(tag) = tag {
            return tag.is_core() && tag.suffix() == "null";
        }
        style == ScalarStyle::Plain && is_core_null(value)
    }

    let stripped = text.trim_start_matches('\u{feff}');
    let Ok(mut stream) = crate::yaml::parse_events_stream(stripped) else {
        return StripRootShape::EmptyDocument;
    };
    let mut next_event = || loop {
        match stream.next()? {
            Ok(
                Event::StreamStart { .. } | Event::Directive { .. } | Event::DocumentStart { .. },
            ) => {}
            Ok(event) => return Some(Ok(event)),
            Err(err) => return Some(Err(err)),
        }
    };

    let Some(Ok(root)) = next_event() else {
        return StripRootShape::EmptyDocument;
    };
    match root {
        Event::DocumentEnd { .. } | Event::StreamEnd { .. } => StripRootShape::EmptyDocument,
        Event::SequenceStart { .. } => StripRootShape::TopLevelArray,
        Event::Scalar {
            value, style, tag, ..
        } => {
            if style == ScalarStyle::Plain && tag.is_none() && value.is_empty() {
                StripRootShape::EmptyDocument
            } else if is_core_null_scalar(&value, style, tag.as_ref()) {
                StripRootShape::TopLevelNull
            } else {
                StripRootShape::TopLevelValue
            }
        }
        Event::MappingStart { .. } => {
            // Inspect the first key in the top-level mapping.
            let Some(Ok(key_ev)) = next_event() else {
                return StripRootShape::EmptyObject;
            };
            if matches!(key_ev, Event::MappingEnd { .. }) {
                return StripRootShape::EmptyObject;
            }
            // Skip any nested collection inside a complex key so we land on
            // the key's value event.
            let mut key_depth = usize::from(matches!(
                key_ev,
                Event::MappingStart { .. } | Event::SequenceStart { .. }
            ));
            while key_depth > 0 {
                let Some(Ok(ev)) = next_event() else {
                    return StripRootShape::SingleRootValue;
                };
                match ev {
                    Event::MappingStart { .. } | Event::SequenceStart { .. } => key_depth += 1,
                    Event::MappingEnd { .. } | Event::SequenceEnd { .. } => key_depth -= 1,
                    _ => {}
                }
            }
            // Now inspect the value event of that single top-level key.
            let Some(Ok(val_ev)) = next_event() else {
                return StripRootShape::SingleRootNull;
            };
            match val_ev {
                Event::MappingStart { .. } => match next_event() {
                    Some(Ok(Event::MappingEnd { .. })) => StripRootShape::SingleRootEmptyObject,
                    _ => StripRootShape::Object,
                },
                Event::SequenceStart { .. } => StripRootShape::SingleRootArray,
                Event::Scalar {
                    value, style, tag, ..
                } => {
                    if is_core_null_scalar(&value, style, tag.as_ref()) {
                        StripRootShape::SingleRootNull
                    } else {
                        StripRootShape::SingleRootValue
                    }
                }
                _ => StripRootShape::SingleRootValue,
            }
        }
        _ => StripRootShape::TopLevelValue,
    }
}
