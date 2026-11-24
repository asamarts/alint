//! YAML for alint: the entry points every alint crate decodes through, and the
//! value and error types alint's public API uses.
//!
//! Every document is read under the same parser limits: ayaml's defaults,
//! without its caps on events, collection entries, anchors and alias uses, and
//! with the input cap (32 MiB), the depth cap (128) and the alias-expansion
//! budget set from the constants alint's own pre-parse guards use.
//!
//! [`Value`], [`Mapping`], [`Sequence`], [`Error`] and [`Location`] are alint's
//! own types. Each wraps the parser's, which stays private, so a change of
//! parser does not change alint's API (ADR-0038).

use std::fmt;
use std::iter::FusedIterator;

use ref_cast::{RefCastCustom, ref_cast_custom};
use serde::{Deserialize, Serialize};

/// The parser limits every decode runs under: ayaml's defaults, without its
/// caps on events, collection entries, anchors and alias uses. The parser
/// v0.22 used had no cap on events, entries or anchors, and capped alias uses
/// at 100 per event of the document, where the alias-expansion budget bounds
/// what aliases expand to instead; ayaml's default of 1,000,000 events refuses
/// a valid file of a few megabytes, far below the input cap. The input cap
/// ([`crate::MAX_STRUCTURED_BYTES`]), the depth cap and the alias-expansion
/// budget come from the constants of [`crate::yaml_depth`], whose guards bound
/// flow nesting and alias expansion before a parse (counting flow levels only,
/// and charging aliases more strictly), and together they bound what one parse
/// can cost. ayaml's defaults are the same values.
fn limits() -> ayaml::Limits {
    // The parser cannot go deeper than its cap, so the guard's ceiling must not
    // either: a deeper ceiling would let through what the parser refuses.
    const _: () = assert!(crate::yaml_depth::MAX_YAML_FLOW_DEPTH <= ayaml::MAX_DEPTH_CAP);
    use crate::yaml_depth::{
        MAX_YAML_EXPANSION_BYTES, MAX_YAML_FLOW_DEPTH, YAML_EXPANSION_BYTES_PER_INPUT_BYTE,
        YAML_NODE_COST,
    };
    ayaml::Limits::default()
        .with_max_input_bytes(crate::MAX_STRUCTURED_BYTES)
        .with_max_depth(MAX_YAML_FLOW_DEPTH)
        .with_max_alias_expansion_bytes(MAX_YAML_EXPANSION_BYTES)
        .with_alias_bytes_per_input_byte(YAML_EXPANSION_BYTES_PER_INPUT_BYTE)
        .with_node_cost_bytes(YAML_NODE_COST)
        .with_max_events(usize::MAX)
        .with_max_collection_entries(usize::MAX)
        .with_max_anchors(usize::MAX)
        .with_max_alias_dereferences(usize::MAX)
}

/// Compatibility parse options carrying alint's unified parser limits.
pub(crate) fn compat_parse_options() -> ayaml::compat::ParseOptions {
    ayaml::compat::ParseOptions::new().with_limits(limits())
}

/// Decode one YAML document into `T`, under alint's parser limits.
///
/// The parser is ayaml, unless a build with `alint-core`'s `yaml-reference`
/// feature, which is for testing, chooses another with `ALINT_YAML_PARSER`
/// (ADR-0041). The feature is for alint's own tests: a crate that depends on
/// alint-core must not enable it, as the variable then chooses the parser for
/// the whole process.
///
/// An enum's variant without data takes only its name, a string, not the
/// one-entry mapping or the tag that serde's externally tagged form gives a
/// variant with data.
///
/// # Errors
///
/// When the input is not one valid YAML document, exceeds a limit, or does
/// not decode into `T`. A value of the wrong type is named, as ``integer `1` ``,
/// and so is a YAML tag a field cannot read. A parse error that is likely an
/// indentation mistake ends with a hint to check the line's indentation.
pub fn from_str<'de, T: Deserialize<'de>>(input: &'de str) -> Result<T, Error> {
    crate::yaml_backend::from_str(input)
}

/// [`from_str`] through ayaml, whatever parser the build chose, and through
/// alint's own decoding layer (see [`crate::yaml_decode`]).
pub(crate) fn ayaml_from_str<'de, T: Deserialize<'de>>(input: &'de str) -> Result<T, Error> {
    ayaml::compat::from_str_seed_with(
        input,
        &compat_parse_options(),
        crate::yaml_decode::TextSeed::<T>::new(),
    )
    .map_err(|error| Error::from_text(input, crate::yaml_decode::finish(error)))
}

/// Decode one YAML document through `seed` with ayaml, under alint's parser
/// limits, whatever parser the build chose: the guards measure what ayaml
/// reads.
pub(crate) fn ayaml_from_str_seed<'de, S: serde::de::DeserializeSeed<'de>>(
    input: &'de str,
    seed: S,
) -> Result<S::Value, ayaml::Error> {
    ayaml::compat::from_str_seed_with(input, &compat_parse_options(), seed)
}

/// Stream the spanned parser events of `input` lazily under alint's parser
/// limits, using ayaml's compatibility reader.
pub(crate) fn parse_events_stream(input: &str) -> Result<ayaml::EventStream<'_>, ayaml::Error> {
    ayaml::compat::parse_events_with(input, &compat_parse_options())
}

/// Line/column lookup over a YAML source buffer, wrapping the parser's
/// internal line index without exposing parser types in `alint-core`'s API.
#[doc(hidden)]
pub struct LineIndex<'a>(ayaml::LineIndex<'a>);

impl fmt::Debug for LineIndex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LineIndex").finish_non_exhaustive()
    }
}

impl<'a> LineIndex<'a> {
    /// Index the line starts of `text`.
    #[must_use]
    pub fn new(text: &'a str) -> Self {
        Self(ayaml::LineIndex::new(text))
    }

    /// 1-based `(line, column)` counted in Unicode scalar values for `byte_offset`.
    #[must_use]
    pub fn line_col(&self, byte_offset: usize) -> Option<(usize, usize)> {
        let pos = self
            .0
            .to_position(byte_offset, ayaml::ColumnEncoding::Scalar)
            .ok()?;
        let loc = ayaml::Location::from(pos);
        Some((loc.line(), loc.column()))
    }

    /// 0-based `(line, character)` counted in UTF-16 code units for `byte_offset`
    /// (LSP `Position` coordinates).
    #[must_use]
    pub fn utf16_position(&self, byte_offset: usize) -> Option<(u32, u32)> {
        let pos = self
            .0
            .to_position(byte_offset, ayaml::ColumnEncoding::Utf16)
            .ok()?;
        let line = u32::try_from(pos.line).ok()?;
        let column = u32::try_from(pos.column).ok()?;
        Some((line, column))
    }
}

/// One spanned YAML event exposed internally for workspace crates such as
/// `alint-rules` (`pinned_references`) so every YAML parse in the workspace
/// goes through `alint_core::yaml` and respects alint's parser limits.
#[doc(hidden)]
#[derive(Debug, Clone)]
pub enum StreamEvent<'a> {
    StreamStart {
        span: crate::ByteSpan,
    },
    StreamEnd {
        span: crate::ByteSpan,
    },
    DocumentStart {
        span: crate::ByteSpan,
    },
    DocumentEnd {
        span: crate::ByteSpan,
    },
    Directive {
        span: crate::ByteSpan,
    },
    Scalar {
        value: std::borrow::Cow<'a, str>,
        plain: bool,
        anchor: Option<String>,
        explicit_string_tag: bool,
        local_tag: Option<String>,
        span: crate::ByteSpan,
    },
    Alias {
        anchor: String,
        span: crate::ByteSpan,
    },
    SequenceStart {
        anchor: Option<String>,
        local_tag: Option<String>,
        span: crate::ByteSpan,
    },
    SequenceEnd {
        span: crate::ByteSpan,
    },
    MappingStart {
        anchor: Option<String>,
        local_tag: Option<String>,
        span: crate::ByteSpan,
    },
    MappingEnd {
        span: crate::ByteSpan,
    },
    /// Future non-exhaustive parser event variant.
    Other {
        span: crate::ByteSpan,
    },
}

impl StreamEvent<'_> {
    /// Native UTF-8 byte span of this event in the source input.
    #[must_use]
    pub const fn span(&self) -> crate::ByteSpan {
        match self {
            Self::StreamStart { span }
            | Self::StreamEnd { span }
            | Self::DocumentStart { span }
            | Self::DocumentEnd { span }
            | Self::Directive { span }
            | Self::Scalar { span, .. }
            | Self::Alias { span, .. }
            | Self::SequenceStart { span, .. }
            | Self::SequenceEnd { span }
            | Self::MappingStart { span, .. }
            | Self::MappingEnd { span }
            | Self::Other { span } => *span,
        }
    }
}

/// Streaming iterator of [`StreamEvent`]s under alint's unified YAML parser limits.
#[doc(hidden)]
pub struct SpannedEvents<'a>(ayaml::EventStream<'a>);

impl fmt::Debug for SpannedEvents<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpannedEvents").finish_non_exhaustive()
    }
}

impl<'a> Iterator for SpannedEvents<'a> {
    type Item = Result<StreamEvent<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0
            .next()
            .map(|item| item.map(map_stream_event).map_err(Error::ayaml))
    }
}

fn byte_span(span: ayaml::Span) -> crate::ByteSpan {
    crate::ByteSpan {
        start: span.start(),
        end: span.end(),
    }
}

fn local_tag_suffix(tag: Option<&ayaml::Tag>) -> Option<String> {
    let tag = tag?;
    if tag.is_core() {
        return None;
    }
    let rest = tag.handle().strip_prefix('!')?;
    if rest.is_empty() {
        Some(tag.suffix().to_owned())
    } else {
        Some(format!("{rest}{}", tag.suffix()))
    }
}

fn map_stream_event(event: ayaml::Event<'_>) -> StreamEvent<'_> {
    let span = byte_span(event.span());
    match event {
        ayaml::Event::StreamStart { .. } => StreamEvent::StreamStart { span },
        ayaml::Event::StreamEnd { .. } => StreamEvent::StreamEnd { span },
        ayaml::Event::DocumentStart { .. } => StreamEvent::DocumentStart { span },
        ayaml::Event::DocumentEnd { .. } => StreamEvent::DocumentEnd { span },
        ayaml::Event::Directive { .. } => StreamEvent::Directive { span },
        ayaml::Event::Scalar {
            value,
            style,
            anchor,
            tag,
            ..
        } => StreamEvent::Scalar {
            value,
            plain: style == ayaml::ScalarStyle::Plain,
            anchor: anchor.map(|a| a.as_str().to_owned()),
            explicit_string_tag: tag
                .as_ref()
                .is_some_and(|t| t.is_core() && t.suffix() == "str"),
            local_tag: local_tag_suffix(tag.as_ref()),
            span,
        },
        ayaml::Event::Alias { anchor, .. } => StreamEvent::Alias {
            anchor: anchor.as_str().to_owned(),
            span,
        },
        ayaml::Event::SequenceStart { anchor, tag, .. } => StreamEvent::SequenceStart {
            anchor: anchor.map(|a| a.as_str().to_owned()),
            local_tag: local_tag_suffix(tag.as_ref()),
            span,
        },
        ayaml::Event::SequenceEnd { .. } => StreamEvent::SequenceEnd { span },
        ayaml::Event::MappingStart { anchor, tag, .. } => StreamEvent::MappingStart {
            anchor: anchor.map(|a| a.as_str().to_owned()),
            local_tag: local_tag_suffix(tag.as_ref()),
            span,
        },
        ayaml::Event::MappingEnd { .. } => StreamEvent::MappingEnd { span },
        _ => StreamEvent::Other { span },
    }
}

/// Stream spanned YAML events under alint's unified parser limits.
///
/// # Errors
///
/// Returns an [`Error`] when `input` violates a parser limit before streaming
/// starts.
#[doc(hidden)]
pub fn parse_spanned_events(input: &str) -> Result<SpannedEvents<'_>, Error> {
    parse_events_stream(input)
        .map(SpannedEvents)
        .map_err(Error::ayaml)
}

/// One step of a YAML configuration path used to place diagnostics on source
/// spans (#566, #571, #572).
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigPathSegment {
    /// A mapping key (e.g. `rules`, `paths`, `relations`, `at`).
    Key(String),
    /// A 0-based sequence index (e.g. `[3]` in `rules[3].paths`).
    Index(usize),
    /// A mapping element inside a sequence whose `id:` field equals this value
    /// (e.g. a rule or fact named by id).
    RuleId(String),
}

/// Decode a double-quoted Rust/`ayaml` key literal starting at `input[0] == '"'`,
/// handling `\"`, `\\`, `\n`, `\r`, `\t`, `\0`, `\'`, and `\u{HEX}`. Returns
/// `(decoded, bytes_consumed)` including both quotes.
fn unescape_quoted_key(input: &str) -> Option<(String, usize)> {
    let mut chars = input.char_indices();
    if chars.next()?.1 != '"' {
        return None;
    }
    let mut out = String::new();
    while let Some((idx, ch)) = chars.next() {
        match ch {
            '"' => return Some((out, idx + 1)),
            '\\' => {
                let (_, esc) = chars.next()?;
                match esc {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '\'' => out.push('\''),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    '0' => out.push('\0'),
                    'u' => {
                        let (_, open) = chars.next()?;
                        if open != '{' {
                            return None;
                        }
                        let mut hex = String::new();
                        loop {
                            let (_, h) = chars.next()?;
                            if h == '}' {
                                break;
                            }
                            if hex.len() >= 6 || !h.is_ascii_hexdigit() {
                                return None;
                            }
                            hex.push(h);
                        }
                        if hex.is_empty() {
                            return None;
                        }
                        let code = u32::from_str_radix(&hex, 16).ok()?;
                        out.push(char::from_u32(code)?);
                    }
                    _ => return None,
                }
            }
            other => out.push(other),
        }
    }
    None
}

/// Strict parser for dot-and-bracket YAML paths emitted by `ayaml` and `alint`,
/// returning `None` if a bracket segment is malformed or an unquoted
/// non-integer placeholder such as `ayaml`'s `[?]` collection-key marker.
fn parse_config_path_inner(path: &str, strict: bool) -> Option<Vec<ConfigPathSegment>> {
    let mut segments = Vec::new();
    let bytes = path.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'.' {
            i += 1;
            continue;
        }
        if bytes[i] == b'[' {
            let start = i + 1;
            if start >= bytes.len() {
                return if strict { None } else { Some(segments) };
            }
            if bytes[start] == b'"' {
                let Some((decoded, consumed)) = unescape_quoted_key(&path[start..]) else {
                    return if strict { None } else { Some(segments) };
                };
                let close = start + consumed;
                if bytes.get(close) != Some(&b']') {
                    return if strict { None } else { Some(segments) };
                }
                segments.push(ConfigPathSegment::Key(decoded));
                i = close + 1;
                continue;
            }
            if bytes[start] == b'\'' {
                let rest = &path[start + 1..];
                let Some(rel_quote) = rest.find("']") else {
                    return if strict { None } else { Some(segments) };
                };
                segments.push(ConfigPathSegment::Key(rest[..rel_quote].to_owned()));
                i = start + 1 + rel_quote + 2;
                continue;
            }
            let Some(rel_end) = path[start..].find(']') else {
                return if strict { None } else { Some(segments) };
            };
            let inner = &path[start..start + rel_end];
            if let Ok(index) = inner.parse::<usize>() {
                segments.push(ConfigPathSegment::Index(index));
            } else if strict || inner == "?" {
                // Reject `ayaml`'s `[?]` collection-key placeholder and
                // unquoted non-integer brackets so they never alias a literal
                // `"?"` mapping key.
                return if strict { None } else { Some(segments) };
            } else if !inner.is_empty() {
                segments.push(ConfigPathSegment::Key(inner.to_owned()));
            }
            i = start + rel_end + 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i] != b'.' && bytes[i] != b'[' {
            i += 1;
        }
        if i > start {
            segments.push(ConfigPathSegment::Key(path[start..i].to_owned()));
        }
    }
    Some(segments)
}

/// Parse a dot-and-bracket configuration path such as `rules[3].paths`,
/// `relations.r.fields.name.at`, or `extends[0].as` into [`ConfigPathSegment`]s.
#[doc(hidden)]
#[must_use]
pub fn parse_config_path(path: &str) -> Vec<ConfigPathSegment> {
    parse_config_path_inner(path, false).unwrap_or_default()
}

/// Locate the UTF-8 [`crate::ByteSpan`] of the YAML key or sequence item named
/// by `path` in `input` (#566, #571).
///
/// For a path ending in [`ConfigPathSegment::Key`], returns the key token's
/// span; for a path ending in [`ConfigPathSegment::Index`], returns the start
/// token's span of that sequence element; for [`ConfigPathSegment::RuleId`],
/// locates the sequence element whose `id:` scalar equals the id (and continues
/// resolving any trailing segments inside that element).
#[doc(hidden)]
#[must_use]
pub fn locate_config_path_span(input: &str, path: &[ConfigPathSegment]) -> Option<crate::ByteSpan> {
    if path.is_empty() {
        return None;
    }
    if let Some(pos) = path
        .iter()
        .position(|seg| matches!(seg, ConfigPathSegment::RuleId(_)))
    {
        let ConfigPathSegment::RuleId(target_id) = &path[pos] else {
            unreachable!()
        };
        let index = find_sequence_entry_by_id(input, &path[..pos], target_id)?;
        let mut resolved = Vec::with_capacity(path.len());
        resolved.extend_from_slice(&path[..pos]);
        resolved.push(ConfigPathSegment::Index(index));
        if pos + 1 == path.len() {
            // Point directly at the `id:` key of the matching entry when no
            // sub-field was specified.
            resolved.push(ConfigPathSegment::Key("id".to_owned()));
            return locate_concrete_path_span(input, &resolved)
                .or_else(|| locate_concrete_path_span(input, &resolved[..resolved.len() - 1]));
        }
        resolved.extend_from_slice(&path[pos + 1..]);
        return locate_concrete_path_span(input, &resolved).or_else(|| {
            // Sub-field may be inherited from an `extends:` or template; fall
            // back to the entry's own `id:` key.
            let mut fallback = resolved[..=pos].to_vec();
            fallback.push(ConfigPathSegment::Key("id".to_owned()));
            locate_concrete_path_span(input, &fallback)
        });
    }
    locate_concrete_path_span(input, path)
}

/// Locate the UTF-8 [`crate::ByteSpan`] of the `entry_index`-th top-level
/// mapping key in `input` (fallback for non-string top-level keys in #566).
#[doc(hidden)]
#[must_use]
pub fn locate_top_level_key_span(input: &str, entry_index: usize) -> Option<crate::ByteSpan> {
    let events = parse_events_stream(input).ok()?;
    let mut depth = 0_usize;
    let mut in_root_mapping = false;
    let mut at_key = true;
    let mut seen_keys = 0_usize;
    for parsed in events {
        let Ok(event) = parsed else {
            return None;
        };
        match event {
            ayaml::Event::DocumentEnd { .. } | ayaml::Event::StreamEnd { .. } => return None,
            ayaml::Event::MappingStart { span, .. } => {
                if in_root_mapping {
                    if depth == 0 && at_key {
                        if seen_keys == entry_index {
                            return Some(byte_span(span));
                        }
                        seen_keys += 1;
                    }
                    depth += 1;
                } else {
                    in_root_mapping = true;
                }
            }
            ayaml::Event::SequenceStart { span, .. } => {
                if !in_root_mapping {
                    return None;
                }
                if depth == 0 && at_key {
                    if seen_keys == entry_index {
                        return Some(byte_span(span));
                    }
                    seen_keys += 1;
                }
                depth += 1;
            }
            ayaml::Event::MappingEnd { .. } | ayaml::Event::SequenceEnd { .. } => {
                let next_depth = depth.checked_sub(1)?;
                depth = next_depth;
                if depth == 0 {
                    at_key = !at_key;
                }
            }
            ayaml::Event::Scalar { span, .. } | ayaml::Event::Alias { span, .. } => {
                if !in_root_mapping {
                    return None;
                }
                if depth == 0 {
                    if at_key {
                        if seen_keys == entry_index {
                            return Some(byte_span(span));
                        }
                        seen_keys += 1;
                    }
                    at_key = !at_key;
                }
            }
            _ => {}
        }
    }
    None
}

#[derive(Debug)]
enum PathWalkFrame {
    Mapping {
        /// Whether this mapping itself sits at `path[..depth]` (`Some(depth)`).
        matched_depth: Option<usize>,
        at_key: bool,
        /// When `at_key` just transitioned to `false`, the matched depth for the
        /// upcoming value node (`Some(depth + 1)` if the key matched `path[depth]`).
        value_depth: Option<usize>,
    },
    Sequence {
        matched_depth: Option<usize>,
        next_index: usize,
    },
}

#[allow(clippy::too_many_lines)]
fn locate_concrete_path_span(input: &str, path: &[ConfigPathSegment]) -> Option<crate::ByteSpan> {
    let events = parse_events_stream(input).ok()?;
    let mut stack: Vec<PathWalkFrame> = Vec::new();
    let mut scalar_anchors: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for parsed in events {
        let Ok(event) = parsed else {
            return None;
        };
        let next_node_depth = match stack.last() {
            None => Some(0),
            Some(
                PathWalkFrame::Mapping { at_key: true, .. }
                | PathWalkFrame::Sequence {
                    matched_depth: None,
                    ..
                },
            ) => None,
            Some(PathWalkFrame::Mapping {
                at_key: false,
                value_depth,
                ..
            }) => *value_depth,
            Some(PathWalkFrame::Sequence {
                matched_depth: Some(depth),
                next_index,
            }) => match path.get(*depth) {
                Some(ConfigPathSegment::Index(target)) if *target == *next_index => Some(depth + 1),
                _ => None,
            },
        };

        match event {
            ayaml::Event::DocumentEnd { .. } | ayaml::Event::StreamEnd { .. } => return None,
            ayaml::Event::MappingStart { anchor, span, .. } => {
                if let Some(anchor) = anchor {
                    scalar_anchors.remove(anchor.as_str());
                }
                if next_node_depth == Some(path.len()) {
                    return Some(byte_span(span));
                }
                stack.push(PathWalkFrame::Mapping {
                    matched_depth: next_node_depth,
                    at_key: true,
                    value_depth: None,
                });
                continue;
            }
            ayaml::Event::SequenceStart { anchor, span, .. } => {
                if let Some(anchor) = anchor {
                    scalar_anchors.remove(anchor.as_str());
                }
                if next_node_depth == Some(path.len()) {
                    return Some(byte_span(span));
                }
                stack.push(PathWalkFrame::Sequence {
                    matched_depth: next_node_depth,
                    next_index: 0,
                });
                continue;
            }
            ayaml::Event::MappingEnd { .. } | ayaml::Event::SequenceEnd { .. } => {
                stack.pop()?;
            }
            ayaml::Event::Scalar {
                value,
                anchor,
                span,
                ..
            } => {
                if let Some(anchor) = anchor {
                    scalar_anchors.insert(anchor.as_str().to_owned(), value.to_string());
                }
                if next_node_depth == Some(path.len()) {
                    return Some(byte_span(span));
                }
                if let Some(PathWalkFrame::Mapping {
                    matched_depth,
                    at_key: at_key @ true,
                    value_depth,
                }) = stack.last_mut()
                {
                    let matched = matched_depth.and_then(|depth| match path.get(depth) {
                        Some(ConfigPathSegment::Key(target)) if target == value.as_ref() => {
                            Some(depth + 1)
                        }
                        _ => None,
                    });
                    if matched == Some(path.len()) {
                        return Some(byte_span(span));
                    }
                    *value_depth = matched;
                    *at_key = false;
                    continue;
                }
            }
            ayaml::Event::Alias { anchor, span, .. } => {
                if next_node_depth == Some(path.len()) {
                    return Some(byte_span(span));
                }
                if let Some(PathWalkFrame::Mapping {
                    matched_depth,
                    at_key: at_key @ true,
                    value_depth,
                }) = stack.last_mut()
                {
                    let resolved = scalar_anchors.get(anchor.as_str());
                    let matched = matched_depth.and_then(|depth| match path.get(depth) {
                        Some(ConfigPathSegment::Key(target))
                            if resolved.is_some_and(|s| s == target) =>
                        {
                            Some(depth + 1)
                        }
                        _ => None,
                    });
                    if matched == Some(path.len()) {
                        return Some(byte_span(span));
                    }
                    *value_depth = matched;
                    *at_key = false;
                    continue;
                }
            }
            _ => continue,
        }

        // A node completed inside its parent container.
        match stack.last_mut() {
            Some(PathWalkFrame::Mapping {
                at_key,
                value_depth,
                ..
            }) => {
                if *at_key {
                    // Non-scalar key completed; its value cannot match a string key.
                    *value_depth = None;
                    *at_key = false;
                } else {
                    *value_depth = None;
                    *at_key = true;
                }
            }
            Some(PathWalkFrame::Sequence { next_index, .. }) => {
                *next_index += 1;
            }
            None => return None,
        }
    }
    None
}

/// Find the 0-based index of the mapping element inside the sequence at
/// `seq_path` whose `id:` scalar equals `target_id`.
#[allow(clippy::too_many_lines)]
fn find_sequence_entry_by_id(
    input: &str,
    seq_path: &[ConfigPathSegment],
    target_id: &str,
) -> Option<usize> {
    let events = parse_events_stream(input).ok()?;
    let mut stack: Vec<PathWalkFrame> = Vec::new();
    let mut scalar_anchors: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut current_entry_index: Option<usize> = None;
    let mut reading_id_value = false;

    for parsed in events {
        let Ok(event) = parsed else {
            return None;
        };
        let next_node_depth = match stack.last() {
            None => Some(0),
            Some(
                PathWalkFrame::Mapping { at_key: true, .. }
                | PathWalkFrame::Sequence {
                    matched_depth: None,
                    ..
                },
            ) => None,
            Some(PathWalkFrame::Mapping {
                at_key: false,
                value_depth,
                ..
            }) => *value_depth,
            Some(PathWalkFrame::Sequence {
                matched_depth: Some(depth),
                next_index,
            }) => {
                if *depth == seq_path.len() {
                    current_entry_index = Some(*next_index);
                    Some(depth + 1)
                } else {
                    match seq_path.get(*depth) {
                        Some(ConfigPathSegment::Index(target)) if *target == *next_index => {
                            Some(depth + 1)
                        }
                        _ => None,
                    }
                }
            }
        };

        match event {
            ayaml::Event::DocumentEnd { .. } | ayaml::Event::StreamEnd { .. } => return None,
            ayaml::Event::MappingStart { anchor, .. } => {
                if let Some(anchor) = anchor {
                    scalar_anchors.remove(anchor.as_str());
                }
                reading_id_value = false;
                stack.push(PathWalkFrame::Mapping {
                    matched_depth: next_node_depth,
                    at_key: true,
                    value_depth: None,
                });
                continue;
            }
            ayaml::Event::SequenceStart { anchor, .. } => {
                if let Some(anchor) = anchor {
                    scalar_anchors.remove(anchor.as_str());
                }
                reading_id_value = false;
                stack.push(PathWalkFrame::Sequence {
                    matched_depth: next_node_depth,
                    next_index: 0,
                });
                continue;
            }
            ayaml::Event::MappingEnd { .. } | ayaml::Event::SequenceEnd { .. } => {
                reading_id_value = false;
                if matches!(
                    stack.last(),
                    Some(PathWalkFrame::Mapping {
                        matched_depth: Some(d),
                        ..
                    }) if *d == seq_path.len() + 1
                ) {
                    current_entry_index = None;
                }
                stack.pop()?;
            }
            ayaml::Event::Scalar { value, anchor, .. } => {
                if let Some(anchor) = anchor {
                    scalar_anchors.insert(anchor.as_str().to_owned(), value.to_string());
                }
                if reading_id_value {
                    reading_id_value = false;
                    if value.as_ref() == target_id
                        && let Some(idx) = current_entry_index
                    {
                        return Some(idx);
                    }
                } else if let Some(PathWalkFrame::Mapping {
                    matched_depth,
                    at_key: at_key @ true,
                    value_depth,
                }) = stack.last_mut()
                {
                    if *matched_depth == Some(seq_path.len() + 1) {
                        reading_id_value = value.as_ref() == "id";
                        *value_depth = None;
                    } else {
                        *value_depth = matched_depth.and_then(|depth| match seq_path.get(depth) {
                            Some(ConfigPathSegment::Key(target)) if target == value.as_ref() => {
                                Some(depth + 1)
                            }
                            _ => None,
                        });
                    }
                    *at_key = false;
                    continue;
                }
            }
            ayaml::Event::Alias { anchor, .. } => {
                let resolved = scalar_anchors.get(anchor.as_str()).map(String::as_str);
                if reading_id_value {
                    reading_id_value = false;
                    if resolved == Some(target_id)
                        && let Some(idx) = current_entry_index
                    {
                        return Some(idx);
                    }
                } else if let Some(PathWalkFrame::Mapping {
                    matched_depth,
                    at_key: at_key @ true,
                    value_depth,
                }) = stack.last_mut()
                {
                    if *matched_depth == Some(seq_path.len() + 1) {
                        reading_id_value = resolved == Some("id");
                        *value_depth = None;
                    } else {
                        *value_depth = matched_depth.and_then(|depth| match seq_path.get(depth) {
                            Some(ConfigPathSegment::Key(target))
                                if resolved == Some(target.as_str()) =>
                            {
                                Some(depth + 1)
                            }
                            _ => None,
                        });
                    }
                    *at_key = false;
                    continue;
                }
            }
            _ => continue,
        }

        match stack.last_mut() {
            Some(PathWalkFrame::Mapping {
                at_key,
                value_depth,
                ..
            }) => {
                *value_depth = None;
                *at_key = !*at_key;
            }
            Some(PathWalkFrame::Sequence { next_index, .. }) => {
                *next_index += 1;
            }
            None => return None,
        }
    }
    None
}

/// Structured description of the repeated key reported by a duplicate-key or
/// duplicate-field error (#572).
#[derive(Debug, Clone)]
enum DuplicateKeyMatcher {
    /// Serde ``duplicate field `name` `` (untagged or `!!str` string key).
    FieldName(String),
    /// `duplicate entry with key "..."` (string key; `truncated` is true when
    /// `ayaml::value::push_quoted_key` truncated past 63 chars with `'…'`).
    QuotedString { text: String, truncated: bool },
    /// Unquoted typed scalar key (`null`, `true`, `false`, int, float).
    TypedScalar(CoreScalar),
    /// Tagged key such as `!t "a"` or `!t 1`.
    Tagged {
        tag: String,
        payload: Box<DuplicateKeyMatcher>,
    },
}

/// Snapshot of a YAML scalar key for typed/tagged duplicate-key matching.
#[derive(Debug, Clone)]
struct KeySnapshot {
    raw: String,
    tag_text: Option<String>,
    core: Option<CoreScalar>,
    payload: Option<CoreScalar>,
}

impl KeySnapshot {
    fn from_event(value: &str, style: ayaml::ScalarStyle, tag: Option<&ayaml::Tag>) -> Self {
        let non_core_tag = tag
            .filter(|t| !(is_core_scalar_tag(t) || (t.handle() == "!" && t.suffix().is_empty())));
        Self {
            raw: value.to_owned(),
            tag_text: non_core_tag.map(ToString::to_string),
            core: resolve_core_scalar(value, style, tag),
            payload: resolve_scalar_payload(value, style, tag),
        }
    }
}

impl DuplicateKeyMatcher {
    fn parse_key_tail(tail: &str) -> Option<Self> {
        if tail.starts_with('"') {
            let (decoded, consumed) = unescape_quoted_key(tail)?;
            if tail[consumed..].trim().is_empty() {
                let truncated = decoded.ends_with('\u{2026}') && decoded.chars().count() == 64;
                return Some(Self::QuotedString {
                    text: decoded,
                    truncated,
                });
            }
            return None;
        }
        if tail.starts_with('!')
            && let Some((tag, rest)) = tail.split_once(' ')
        {
            let payload = Self::parse_key_tail(rest.trim())?;
            return Some(Self::Tagged {
                tag: tag.to_owned(),
                payload: Box::new(payload),
            });
        }
        let scalar = resolve_core_scalar(tail, ayaml::ScalarStyle::Plain, None)?;
        Some(Self::TypedScalar(scalar))
    }

    fn matches_strict(&self, key: &KeySnapshot) -> bool {
        match self {
            Self::FieldName(name) => key.tag_text.is_none() && key.raw == *name,
            Self::QuotedString { text, truncated } => {
                if key.tag_text.is_some() {
                    return false;
                }
                let Some(CoreScalar::String(actual)) = &key.payload else {
                    return false;
                };
                if *truncated {
                    let prefix = text.strip_suffix('\u{2026}').unwrap_or(text);
                    actual.starts_with(prefix) && actual.chars().count() >= 64
                } else {
                    actual == text
                }
            }
            Self::TypedScalar(expected) => {
                key.tag_text.is_none() && key.core.as_ref() == Some(expected)
            }
            Self::Tagged { tag, payload } => {
                if key.tag_text.as_deref() != Some(tag.as_str()) {
                    return false;
                }
                match payload.as_ref() {
                    Self::QuotedString { text, truncated } => {
                        let Some(CoreScalar::String(actual)) = &key.payload else {
                            return false;
                        };
                        if *truncated {
                            let prefix = text.strip_suffix('\u{2026}').unwrap_or(text);
                            actual.starts_with(prefix) && actual.chars().count() >= 64
                        } else {
                            actual == text
                        }
                    }
                    Self::TypedScalar(expected) => key.payload.as_ref() == Some(expected),
                    _ => false,
                }
            }
        }
    }

    /// Coerced-string fallback for `vars:` maps (`deserialize_unique_string_map`)
    /// where a plain scalar key such as `1` is deserialized as `"1"` and collides
    /// with a quoted `"1"` key.
    fn matches_coerced_string(&self, key: &KeySnapshot) -> bool {
        matches!(self, Self::QuotedString { text, truncated: false } if key.tag_text.is_none() && key.raw == *text)
    }
}

/// Split `message` at the final `": <needle>"` or leading `"<needle>"` so a
/// parent mapping key whose own text contains `needle` cannot hijack matching.
fn split_error_suffix<'a>(message: &'a str, needle: &str) -> Option<(&'a str, &'a str)> {
    if let Some(rest) = message.strip_prefix(needle) {
        return Some(("", rest));
    }
    let marker = format!(": {needle}");
    let at = message.rfind(&marker)?;
    Some((&message[..at], &message[at + marker.len()..]))
}

/// Extract `(parent_path, duplicate_key_matcher)` from a duplicate-key or
/// duplicate-field YAML decode error message (#572).
fn parse_duplicate_key_error(
    message: &str,
) -> Option<(Vec<ConfigPathSegment>, DuplicateKeyMatcher)> {
    if let Some((prefix, rest)) = split_error_suffix(message, "duplicate entry with key ") {
        let tail = rest.trim();
        let matcher = DuplicateKeyMatcher::parse_key_tail(tail)?;
        let parent = if prefix.is_empty() {
            Vec::new()
        } else {
            parse_config_path_inner(prefix, true)?
        };
        return Some((parent, matcher));
    }
    if let Some((prefix, rest)) = split_error_suffix(message, "duplicate field `") {
        let end = rest.find('`')?;
        let key = rest[..end].to_owned();
        let parent = if prefix.is_empty() {
            Vec::new()
        } else {
            parse_config_path_inner(prefix, true)?
        };
        return Some((parent, DuplicateKeyMatcher::FieldName(key)));
    }
    None
}

/// Locate the second occurrence of a duplicate key inside the mapping at
/// `parent_path` so duplicate-entry / duplicate-field errors point at the
/// repeated key rather than the mapping's first key (#572).
#[allow(clippy::too_many_lines)]
fn locate_repeated_key_span_pass(
    input: &str,
    parent_path: &[ConfigPathSegment],
    matcher: &DuplicateKeyMatcher,
    coerced_fallback: bool,
) -> Option<ayaml::Span> {
    let events = parse_events_stream(input).ok()?;
    let mut stack: Vec<PathWalkFrame> = Vec::new();
    let mut scalar_anchors: std::collections::HashMap<String, KeySnapshot> =
        std::collections::HashMap::new();
    let mut seen_target_key = false;

    let key_matches = |snapshot: &KeySnapshot| {
        if coerced_fallback {
            matcher.matches_coerced_string(snapshot)
        } else {
            matcher.matches_strict(snapshot)
        }
    };

    for parsed in events {
        let Ok(event) = parsed else {
            return None;
        };
        let next_node_depth = match stack.last() {
            None => Some(0),
            Some(
                PathWalkFrame::Mapping { at_key: true, .. }
                | PathWalkFrame::Sequence {
                    matched_depth: None,
                    ..
                },
            ) => None,
            Some(PathWalkFrame::Mapping {
                at_key: false,
                value_depth,
                ..
            }) => *value_depth,
            Some(PathWalkFrame::Sequence {
                matched_depth: Some(depth),
                next_index,
            }) => match parent_path.get(*depth) {
                Some(ConfigPathSegment::Index(target)) if *target == *next_index => Some(depth + 1),
                _ => None,
            },
        };

        match event {
            ayaml::Event::DocumentEnd { .. } | ayaml::Event::StreamEnd { .. } => return None,
            ayaml::Event::MappingStart { anchor, .. } => {
                if let Some(anchor) = anchor {
                    scalar_anchors.remove(anchor.as_str());
                }
                if next_node_depth == Some(parent_path.len()) {
                    seen_target_key = false;
                }
                stack.push(PathWalkFrame::Mapping {
                    matched_depth: next_node_depth,
                    at_key: true,
                    value_depth: None,
                });
                continue;
            }
            ayaml::Event::SequenceStart { anchor, .. } => {
                if let Some(anchor) = anchor {
                    scalar_anchors.remove(anchor.as_str());
                }
                stack.push(PathWalkFrame::Sequence {
                    matched_depth: next_node_depth,
                    next_index: 0,
                });
                continue;
            }
            ayaml::Event::MappingEnd { .. } | ayaml::Event::SequenceEnd { .. } => {
                stack.pop()?;
            }
            ayaml::Event::Scalar {
                value,
                style,
                anchor,
                tag,
                span,
                ..
            } => {
                let snapshot = KeySnapshot::from_event(value.as_ref(), style, tag.as_ref());
                if let Some(anchor) = anchor {
                    scalar_anchors.insert(anchor.as_str().to_owned(), snapshot.clone());
                }
                if let Some(PathWalkFrame::Mapping {
                    matched_depth,
                    at_key: at_key @ true,
                    value_depth,
                }) = stack.last_mut()
                {
                    if *matched_depth == Some(parent_path.len()) {
                        if key_matches(&snapshot) {
                            if seen_target_key {
                                return Some(span);
                            }
                            seen_target_key = true;
                        }
                        *value_depth = None;
                    } else {
                        *value_depth =
                            matched_depth.and_then(|depth| match parent_path.get(depth) {
                                Some(ConfigPathSegment::Key(target))
                                    if snapshot.tag_text.is_none()
                                        && target == snapshot.raw.as_str() =>
                                {
                                    Some(depth + 1)
                                }
                                _ => None,
                            });
                    }
                    *at_key = false;
                    continue;
                }
            }
            ayaml::Event::Alias { anchor, span, .. } => {
                if let Some(PathWalkFrame::Mapping {
                    matched_depth,
                    at_key: at_key @ true,
                    value_depth,
                }) = stack.last_mut()
                {
                    let resolved = scalar_anchors.get(anchor.as_str());
                    if *matched_depth == Some(parent_path.len()) {
                        if resolved.is_some_and(&key_matches) {
                            if seen_target_key {
                                return Some(span);
                            }
                            seen_target_key = true;
                        }
                        *value_depth = None;
                    } else {
                        *value_depth =
                            matched_depth.and_then(|depth| match parent_path.get(depth) {
                                Some(ConfigPathSegment::Key(target))
                                    if resolved.is_some_and(|s| {
                                        s.tag_text.is_none() && s.raw == *target
                                    }) =>
                                {
                                    Some(depth + 1)
                                }
                                _ => None,
                            });
                    }
                    *at_key = false;
                    continue;
                }
            }
            _ => continue,
        }

        match stack.last_mut() {
            Some(PathWalkFrame::Mapping {
                at_key,
                value_depth,
                ..
            }) => {
                *value_depth = None;
                *at_key = !*at_key;
            }
            Some(PathWalkFrame::Sequence { next_index, .. }) => {
                *next_index += 1;
            }
            None => return None,
        }
    }
    None
}

fn locate_repeated_key_span(
    input: &str,
    parent_path: &[ConfigPathSegment],
    matcher: &DuplicateKeyMatcher,
) -> Option<ayaml::Span> {
    locate_repeated_key_span_pass(input, parent_path, matcher, false).or_else(|| {
        matches!(
            matcher,
            DuplicateKeyMatcher::QuotedString {
                truncated: false,
                ..
            }
        )
        .then(|| locate_repeated_key_span_pass(input, parent_path, matcher, true))
        .flatten()
    })
}

/// Locate the full UTF-8 [`crate::ByteSpan`] of a repeated key described by a
/// duplicate-key / duplicate-field error message (#571, #572), accepting either
/// raw `Error::message()` text or formatted strings ending with
/// ` at line L column C`.
#[doc(hidden)]
#[must_use]
pub fn locate_duplicate_key_span(input: &str, message: &str) -> Option<crate::ByteSpan> {
    let trimmed = strip_trailing_line_column(message);
    let (parent_path, matcher) = parse_duplicate_key_error(trimmed)?;
    locate_repeated_key_span(input, &parent_path, &matcher).map(byte_span)
}

fn strip_trailing_line_column(message: &str) -> &str {
    let Some(at) = message.rfind(" at line ") else {
        return message;
    };
    let tail = &message[at + " at line ".len()..];
    let Some((line, col)) = tail.split_once(" column ") else {
        return message;
    };
    let line_ok = !line.is_empty() && line.bytes().all(|b| b.is_ascii_digit());
    let col_digits = col.split_whitespace().next().unwrap_or_default();
    let col_ok = !col_digits.is_empty() && col_digits.bytes().all(|b| b.is_ascii_digit());
    if line_ok && col_ok {
        &message[..at]
    } else {
        message
    }
}

/// If `error` is a duplicate-key or duplicate-field error, relocate its span
/// and line/column to the repeated key itself rather than the mapping start
/// (#572).
fn relocate_duplicate_key_error(input: &str, error: ayaml::Error) -> ayaml::Error {
    if !matches!(
        error.kind(),
        ayaml::ErrorKind::Serde | ayaml::ErrorKind::DuplicateKey
    ) {
        return error;
    }
    let Some((parent_path, matcher)) = parse_duplicate_key_error(error.message()) else {
        return error;
    };
    let Some(span) = locate_repeated_key_span(input, &parent_path, &matcher) else {
        return error;
    };
    let line_index = ayaml::LineIndex::new(input);
    let Ok(pos) = line_index.to_position(span.start(), ayaml::ColumnEncoding::Scalar) else {
        return error;
    };
    let location = ayaml::Location::from(pos);
    error.with_span(span).with_location(location)
}

/// Whether the first node of `prefix` (the input up to an error) is a block
/// collection, inspecting events lazily so the full stream is never buffered.
pub(crate) fn root_is_a_block_collection(prefix: &str) -> bool {
    let Ok(events) = parse_events_stream(prefix) else {
        return false;
    };
    for parsed in events {
        let Ok(event) = parsed else {
            return false;
        };
        match event {
            ayaml::Event::StreamStart { .. }
            | ayaml::Event::Directive { .. }
            | ayaml::Event::DocumentStart { .. }
            | ayaml::Event::DocumentEnd { .. } => {}
            ayaml::Event::SequenceStart { style, .. }
            | ayaml::Event::MappingStart { style, .. } => {
                return style == ayaml::CollectionStyle::Block;
            }
            _ => return false,
        }
    }
    false
}

/// The events of every document in `input`, from the compatibility reader
/// [`from_str`] decodes with, under alint's parser limits, whatever parser the
/// build chose. alint decodes one document; the `yaml_depth` tests read a
/// whole stream with this, and `requires:` is checked on the first
/// document's events.
pub(crate) fn stream_events(input: &str) -> Result<Vec<ayaml::Event<'_>>, ayaml::Error> {
    ayaml::compat::parse_events_eager_with(input, &compat_parse_options())
}

/// A scalar resolved under the YAML 1.2 core schema, with canonical integer
/// and float representations suitable for value-equality mapping-key checks.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CoreScalar {
    Null,
    Bool(bool),
    /// Strictly negative integer (`< 0`). Non-negative integers use [`Self::Uint`].
    NegInt(i128),
    /// Non-negative integer (`>= 0`).
    Uint(u128),
    /// Canonical IEEE-754 float bits (`-0.0` normalized to `+0.0`, all NaNs to
    /// `f64::NAN`).
    Float(u64),
    String(String),
}

/// Whether `tag` is one of the five YAML 1.2 core-schema scalar tags.
pub(crate) fn is_core_scalar_tag(tag: &ayaml::Tag) -> bool {
    tag.is_core() && matches!(tag.suffix(), "str" | "int" | "float" | "bool" | "null")
}

/// Whether `tag` is a YAML 1.2 core-schema collection tag (`!!seq`, `!!map`,
/// `!!set`, `!!omap`, `!!pairs`) that leaves a collection plain.
pub(crate) fn is_core_collection_tag(tag: &ayaml::Tag) -> bool {
    tag.is_core() && matches!(tag.suffix(), "seq" | "map" | "set" | "omap" | "pairs")
}

/// Resolve an untagged or core-scalar-tagged scalar event under the YAML 1.2
/// core schema. Returns `None` for any non-core tag (including `!` and
/// unknown `!!` names like `!!binary`) and for a core tag whose text does not
/// fit it (`!!int abc`).
pub(crate) fn resolve_core_scalar(
    text: &str,
    style: ayaml::ScalarStyle,
    tag: Option<&ayaml::Tag>,
) -> Option<CoreScalar> {
    if let Some(tag) = tag {
        if !is_core_scalar_tag(tag) {
            return None;
        }
        return resolve_explicit_core_scalar(text, tag.suffix());
    }
    Some(resolve_untagged_scalar(text, style))
}

/// Resolve a scalar event's payload under the YAML 1.2 core schema, looking
/// through custom tags (`!` forces string; other non-core tags resolve their
/// payload as if untagged), matching `ayaml::Value` accessors. Returns `None`
/// only when an explicit core scalar tag cannot read the text (`!!int abc`).
pub(crate) fn resolve_scalar_payload(
    text: &str,
    style: ayaml::ScalarStyle,
    tag: Option<&ayaml::Tag>,
) -> Option<CoreScalar> {
    if let Some(tag) = tag {
        if is_core_scalar_tag(tag) {
            return resolve_explicit_core_scalar(text, tag.suffix());
        }
        if tag.handle() == "!" && tag.suffix().is_empty() {
            return Some(CoreScalar::String(text.to_owned()));
        }
        if tag.is_core() {
            // Unknown core tags like `!!binary` carry string payloads in ayaml.
            return Some(CoreScalar::String(text.to_owned()));
        }
    }
    Some(resolve_untagged_scalar(text, style))
}

fn resolve_untagged_scalar(text: &str, style: ayaml::ScalarStyle) -> CoreScalar {
    if style != ayaml::ScalarStyle::Plain {
        return CoreScalar::String(text.to_owned());
    }
    if is_core_null(text) {
        return CoreScalar::Null;
    }
    if let Some(flag) = parse_core_bool(text) {
        return CoreScalar::Bool(flag);
    }
    if let Some(int) = parse_core_int(text, false) {
        return int;
    }
    if let Some(float) = parse_core_float(text, false) {
        return float;
    }
    CoreScalar::String(text.to_owned())
}

fn resolve_explicit_core_scalar(text: &str, suffix: &str) -> Option<CoreScalar> {
    match suffix {
        "str" => Some(CoreScalar::String(text.to_owned())),
        "int" => parse_core_int(text, true),
        "float" => parse_core_float(text, true),
        "bool" => parse_core_bool(text).map(CoreScalar::Bool),
        "null" => is_core_null(text).then_some(CoreScalar::Null),
        _ => None,
    }
}

pub(crate) fn is_core_null(text: &str) -> bool {
    matches!(text, "" | "~" | "null" | "Null" | "NULL")
}

fn parse_core_bool(text: &str) -> Option<bool> {
    match text {
        "true" | "True" | "TRUE" => Some(true),
        "false" | "False" | "FALSE" => Some(false),
        _ => None,
    }
}

fn parse_core_int(text: &str, allow_leading_zero: bool) -> Option<CoreScalar> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    if digits.is_empty() || digits.starts_with(['+', '-']) {
        return None;
    }
    let (radix, digits) = if let Some(rest) = digits.strip_prefix("0b") {
        (2, rest)
    } else if let Some(rest) = digits.strip_prefix("0o") {
        (8, rest)
    } else if let Some(rest) = digits.strip_prefix("0x") {
        (16, rest)
    } else {
        (10, digits)
    };
    if digits.is_empty() || digits.starts_with(['+', '-']) {
        return None;
    }
    if !allow_leading_zero && radix == 10 && digits.len() > 1 && digits.starts_with('0') {
        return None;
    }
    let magnitude = u128::from_str_radix(digits, radix).ok()?;
    if magnitude == 0 {
        return Some(CoreScalar::Uint(0));
    }
    if negative {
        0_i128
            .checked_sub_unsigned(magnitude)
            .map(CoreScalar::NegInt)
    } else {
        Some(CoreScalar::Uint(magnitude))
    }
}

fn canonical_float(value: f64) -> CoreScalar {
    let bits = if value.is_nan() {
        f64::NAN.to_bits()
    } else if value == 0.0 {
        0.0_f64.to_bits()
    } else {
        value.to_bits()
    };
    CoreScalar::Float(bits)
}

fn parse_core_float(text: &str, allow_leading_zero: bool) -> Option<CoreScalar> {
    match text {
        ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF" => {
            return Some(canonical_float(f64::INFINITY));
        }
        "-.inf" | "-.Inf" | "-.INF" => return Some(canonical_float(f64::NEG_INFINITY)),
        ".nan" | ".NaN" | ".NAN" => return Some(canonical_float(f64::NAN)),
        _ => {}
    }
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    let decimal_int_like = !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit());
    if decimal_int_like {
        if !allow_leading_zero && digits.len() > 1 && digits.starts_with('0') {
            return None;
        }
    } else if !text.contains(['.', 'e', 'E']) {
        return None;
    }
    let value = text.parse::<f64>().ok()?;
    value.is_finite().then(|| canonical_float(value))
}

/// Decode `value` into `T`, as [`from_str`] decodes a document's value, except
/// that a string target takes only a string (or a tagged one), where a
/// document's scalar is read by its text.
///
/// # Errors
///
/// When `value` does not decode into `T`.
pub fn from_value<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, Error> {
    crate::yaml_decode::from_value(value.0).map_err(Error::ayaml)
}

/// The YAML value of `value`.
///
/// # Errors
///
/// When `value` cannot be represented in YAML.
pub fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<Value, Error> {
    ayaml::to_value(value).map(Value).map_err(Error::ayaml)
}

/// `value` written as a YAML document.
///
/// # Errors
///
/// When `value` cannot be represented in YAML.
pub fn to_string<T: Serialize + ?Sized>(value: &T) -> Result<String, Error> {
    ayaml::to_string(value).map_err(Error::ayaml)
}

/// A YAML value: null, a boolean, a number, a string, a [`Sequence`], a
/// [`Mapping`], or one of those with a tag.
///
/// The accessors look through a tag, so `!Celsius 21` is a number. A value
/// prints with `{:?}` as the parser's value does.
#[derive(Clone, PartialEq, Eq, Hash, RefCastCustom)]
#[repr(transparent)]
pub struct Value(ayaml::Value);

/// What [`Mapping`]'s and [`Value`]'s indexing return for a missing key.
static NULL: Value = Value(ayaml::Value::Null);

impl Value {
    #[ref_cast_custom]
    pub(crate) fn from_parser_ref(value: &ayaml::Value) -> &Self;

    #[ref_cast_custom]
    pub(crate) fn from_parser_mut(value: &mut ayaml::Value) -> &mut Self;

    #[cfg(test)]
    pub(crate) fn from_parser(value: ayaml::Value) -> Self {
        Self(value)
    }

    pub(crate) fn into_parser(self) -> ayaml::Value {
        self.0
    }

    pub(crate) fn parser(&self) -> &ayaml::Value {
        &self.0
    }

    pub(crate) fn parser_mut(&mut self) -> &mut ayaml::Value {
        &mut self.0
    }

    /// Whether this is null.
    #[must_use]
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }

    /// Whether this is a boolean.
    #[must_use]
    pub fn is_bool(&self) -> bool {
        self.0.is_bool()
    }

    /// Whether this is a number.
    #[must_use]
    pub fn is_number(&self) -> bool {
        self.0.is_number()
    }

    /// Whether this is a string.
    #[must_use]
    pub fn is_string(&self) -> bool {
        self.0.is_string()
    }

    /// Whether this is a sequence.
    #[must_use]
    pub fn is_sequence(&self) -> bool {
        self.0.is_sequence()
    }

    /// Whether this is a mapping.
    #[must_use]
    pub fn is_mapping(&self) -> bool {
        self.0.is_mapping()
    }

    /// The tag on this value, or `None` when it has none. A local tag reads as
    /// written (`!celsius`, and `!` for the non-specific tag), a tag in the
    /// YAML namespace in its short form (`!!binary`), and any other global tag
    /// by its full name, however the document wrote it: `!<tag:x,2000:y>`, and
    /// `!e!y` under `%TAG !e! tag:x,2000:`, both read `tag:x,2000:y`.
    ///
    /// A tag that only restates the kind of value is not kept: the core-schema
    /// scalar tags (`!!str`, `!!int`, `!!float`, `!!bool`, `!!null`) are
    /// applied when the document is read, and the collection tags (`!!seq`,
    /// `!!map`, `!!set`, `!!omap`, `!!pairs`) leave a plain sequence or
    /// mapping. The other accessors look through a tag: a tagged string is a
    /// string.
    #[must_use]
    pub fn tag(&self) -> Option<String> {
        match &self.0 {
            ayaml::Value::Tagged(tagged) => Some(tagged.tag.to_string()),
            _ => None,
        }
    }

    /// The boolean, if this is one.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        self.0.as_bool()
    }

    /// The number as an `i64`, if this is an integer in its range.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        self.0.as_i64()
    }

    /// The number as a `u64`, if this is an integer in its range.
    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        self.0.as_u64()
    }

    /// The number as an `f64`, if this is a number.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        self.0.as_f64()
    }

    /// The string, if this is one.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        self.0.as_str()
    }

    /// The sequence, if this is one.
    #[must_use]
    pub fn as_sequence(&self) -> Option<&Sequence> {
        self.0.as_sequence().map(Sequence::from_parser_ref)
    }

    /// The sequence, mutably, if this is one.
    #[must_use]
    pub fn as_sequence_mut(&mut self) -> Option<&mut Sequence> {
        self.0.as_sequence_mut().map(Sequence::from_parser_mut)
    }

    /// The mapping, if this is one.
    #[must_use]
    pub fn as_mapping(&self) -> Option<&Mapping> {
        self.0.as_mapping().map(Mapping::from_parser_ref)
    }

    /// The mapping, mutably, if this is one.
    #[must_use]
    pub fn as_mapping_mut(&mut self) -> Option<&mut Mapping> {
        self.0.as_mapping_mut().map(Mapping::from_parser_mut)
    }

    /// The value under `key`, if this is a mapping that has one.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Self> {
        self.as_mapping().and_then(|mapping| mapping.get(key))
    }

    /// The value under `key`, mutably, if this is a mapping that has one.
    #[must_use]
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Self> {
        self.as_mapping_mut()
            .and_then(|mapping| mapping.get_mut(key))
    }
}

/// Null.
impl Default for Value {
    fn default() -> Self {
        Self(ayaml::Value::Null)
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for Value {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        ayaml::Value::deserialize(deserializer).map(Self)
    }
}

/// `value[key]` is the value under `key`, or null when this is not a mapping
/// or has no such key.
impl std::ops::Index<&str> for Value {
    type Output = Self;

    fn index(&self, key: &str) -> &Self {
        self.get(key).unwrap_or(&NULL)
    }
}

/// `value[index]` is the entry at `index` of a sequence, or the value under the
/// integer key `index` of a mapping, so `value[0]` on `{0: zero}` is `zero`.
/// It is null when there is no such entry or this is neither.
impl std::ops::Index<usize> for Value {
    type Output = Self;

    fn index(&self, index: usize) -> &Self {
        Self::from_parser_ref(&self.0[index])
    }
}

/// `value[&key]` is `value[key.as_str()]`.
impl std::ops::Index<&String> for Value {
    type Output = Self;

    fn index(&self, key: &String) -> &Self {
        &self[key.as_str()]
    }
}

/// `value[key] = ...` sets the value under `key`. A mapping gains a missing
/// key, and a null becomes a mapping first; a tagged null keeps its tag.
///
/// # Panics
///
/// When this is neither a mapping nor null.
impl std::ops::IndexMut<&str> for Value {
    fn index_mut(&mut self, key: &str) -> &mut Self {
        // The parser turns only an untagged null into a mapping.
        let untagged = self.0.untag_mut();
        if untagged.is_null() {
            *untagged = ayaml::Value::Mapping(ayaml::Mapping::new());
        }
        Self::from_parser_mut(&mut self.0[key])
    }
}

/// `value[&key] = ...` is `value[key.as_str()] = ...`.
impl std::ops::IndexMut<&String> for Value {
    fn index_mut(&mut self, key: &String) -> &mut Self {
        &mut self[key.as_str()]
    }
}

/// `value[index] = ...` sets the entry at `index` of a sequence, or the value
/// under the integer key `index` of a mapping, which a mapping gains when it
/// is missing.
///
/// # Panics
///
/// When this is a sequence and `index` is past its end, or this is neither a
/// sequence nor a mapping.
impl std::ops::IndexMut<usize> for Value {
    fn index_mut(&mut self, index: usize) -> &mut Self {
        Self::from_parser_mut(&mut self.0[index])
    }
}

macro_rules! value_from {
    ($($from:ty),* $(,)?) => {$(
        impl From<$from> for Value {
            fn from(value: $from) -> Self {
                Self(ayaml::Value::from(value))
            }
        }
    )*};
}

value_from!(bool, i64, u64, i128, u128, usize, f64, String, &str);

/// `isize` is at most 64 bits on every target Rust supports.
impl From<isize> for Value {
    #[allow(clippy::cast_possible_truncation)]
    fn from(value: isize) -> Self {
        Self(ayaml::Value::from(value as i64))
    }
}

macro_rules! value_from_widened {
    ($wide:ty: $($from:ty),* $(,)?) => {$(
        impl From<$from> for Value {
            fn from(value: $from) -> Self {
                Self(ayaml::Value::from(<$wide>::from(value)))
            }
        }
    )*};
}

value_from_widened!(i64: i8, i16, i32);
value_from_widened!(u64: u8, u16, u32);
value_from_widened!(f64: f32);

impl From<Mapping> for Value {
    fn from(mapping: Mapping) -> Self {
        Self(ayaml::Value::Mapping(mapping.0))
    }
}

impl From<Sequence> for Value {
    fn from(sequence: Sequence) -> Self {
        Self(ayaml::Value::Sequence(sequence.0))
    }
}

impl From<Vec<Value>> for Value {
    fn from(values: Vec<Value>) -> Self {
        Self::from(values.into_iter().collect::<Sequence>())
    }
}

macro_rules! value_eq {
    ($($other:ty),* $(,)?) => {$(
        impl PartialEq<$other> for Value {
            fn eq(&self, other: &$other) -> bool {
                self.0 == *other
            }
        }
    )*};
}

value_eq!(
    str, &str, String, bool, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64,
);

/// A YAML sequence: values in order.
#[derive(Clone, Default, PartialEq, Eq, Hash, RefCastCustom)]
#[repr(transparent)]
pub struct Sequence(Vec<ayaml::Value>);

impl Sequence {
    #[ref_cast_custom]
    fn from_parser_ref(values: &Vec<ayaml::Value>) -> &Self;

    #[ref_cast_custom]
    fn from_parser_mut(values: &mut Vec<ayaml::Value>) -> &mut Self;

    /// An empty sequence.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many values it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether it holds no values.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The value at `index`, if there is one.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Value> {
        self.0.get(index).map(Value::from_parser_ref)
    }

    /// The value at `index`, mutably, if there is one.
    #[must_use]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut Value> {
        self.0.get_mut(index).map(Value::from_parser_mut)
    }

    /// Add `value` at the end.
    pub fn push(&mut self, value: impl Into<Value>) {
        self.0.push(value.into().0);
    }

    /// Remove the last value and return it, if there is one.
    pub fn pop(&mut self) -> Option<Value> {
        self.0.pop().map(Value)
    }

    /// The values, in order.
    pub fn iter(&self) -> SequenceIter<'_> {
        SequenceIter(self.0.iter())
    }

    /// The values, mutably, in order.
    pub fn iter_mut(&mut self) -> SequenceIterMut<'_> {
        SequenceIterMut(self.0.iter_mut())
    }
}

/// `sequence[index]` is the value at `index`.
///
/// # Panics
///
/// When `index` is past the end, as for a `Vec`; [`Sequence::get`] does not.
impl std::ops::Index<usize> for Sequence {
    type Output = Value;

    fn index(&self, index: usize) -> &Value {
        Value::from_parser_ref(&self.0[index])
    }
}

/// `sequence[index] = ...` sets the value at `index`.
///
/// # Panics
///
/// When `index` is past the end, as for a `Vec`.
impl std::ops::IndexMut<usize> for Sequence {
    fn index_mut(&mut self, index: usize) -> &mut Value {
        Value::from_parser_mut(&mut self.0[index])
    }
}

impl From<Vec<Value>> for Sequence {
    fn from(values: Vec<Value>) -> Self {
        values.into_iter().collect()
    }
}

impl Extend<Value> for Sequence {
    fn extend<I: IntoIterator<Item = Value>>(&mut self, values: I) {
        self.0.extend(values.into_iter().map(Value::into_parser));
    }
}

impl fmt::Debug for Sequence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for Sequence {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Sequence {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Vec::<ayaml::Value>::deserialize(deserializer).map(Self)
    }
}

impl FromIterator<Value> for Sequence {
    fn from_iter<I: IntoIterator<Item = Value>>(values: I) -> Self {
        Self(values.into_iter().map(Value::into_parser).collect())
    }
}

impl<'a> IntoIterator for &'a Sequence {
    type Item = &'a Value;
    type IntoIter = SequenceIter<'a>;

    fn into_iter(self) -> SequenceIter<'a> {
        self.iter()
    }
}

impl<'a> IntoIterator for &'a mut Sequence {
    type Item = &'a mut Value;
    type IntoIter = SequenceIterMut<'a>;

    fn into_iter(self) -> SequenceIterMut<'a> {
        self.iter_mut()
    }
}

impl IntoIterator for Sequence {
    type Item = Value;
    type IntoIter = SequenceIntoIter;

    fn into_iter(self) -> SequenceIntoIter {
        SequenceIntoIter(self.0.into_iter())
    }
}

/// The values of a [`Sequence`], in order.
#[derive(Clone, Debug)]
pub struct SequenceIter<'a>(std::slice::Iter<'a, ayaml::Value>);

impl<'a> Iterator for SequenceIter<'a> {
    type Item = &'a Value;

    fn next(&mut self) -> Option<&'a Value> {
        self.0.next().map(Value::from_parser_ref)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl DoubleEndedIterator for SequenceIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0.next_back().map(Value::from_parser_ref)
    }
}

impl ExactSizeIterator for SequenceIter<'_> {}

impl FusedIterator for SequenceIter<'_> {}

/// The values of a [`Sequence`], mutably, in order.
#[derive(Debug)]
pub struct SequenceIterMut<'a>(std::slice::IterMut<'a, ayaml::Value>);

impl<'a> Iterator for SequenceIterMut<'a> {
    type Item = &'a mut Value;

    fn next(&mut self) -> Option<&'a mut Value> {
        self.0.next().map(Value::from_parser_mut)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl DoubleEndedIterator for SequenceIterMut<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0.next_back().map(Value::from_parser_mut)
    }
}

impl ExactSizeIterator for SequenceIterMut<'_> {}

impl FusedIterator for SequenceIterMut<'_> {}

/// The values of a [`Sequence`] it gave up, in order.
#[derive(Clone, Debug)]
pub struct SequenceIntoIter(std::vec::IntoIter<ayaml::Value>);

impl Iterator for SequenceIntoIter {
    type Item = Value;

    fn next(&mut self) -> Option<Value> {
        self.0.next().map(Value)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl DoubleEndedIterator for SequenceIntoIter {
    fn next_back(&mut self) -> Option<Value> {
        self.0.next_back().map(Value)
    }
}

impl ExactSizeIterator for SequenceIntoIter {}

impl FusedIterator for SequenceIntoIter {}

/// A YAML mapping: keys and their values, in the order they were written or
/// inserted.
///
/// Two mappings are equal when they hold the same keys with equal values, in
/// any order. The string-key methods (`get`, `get_mut`, `contains_key`,
/// `remove`, indexing) find untagged string keys only: `get("k")` does not
/// find the key of `!t k: v`. Iterate to reach a tagged key or one of another
/// kind.
#[derive(Clone, Default, PartialEq, Eq, Hash, RefCastCustom)]
#[repr(transparent)]
pub struct Mapping(ayaml::Mapping);

impl Mapping {
    #[ref_cast_custom]
    pub(crate) fn from_parser_ref(mapping: &ayaml::Mapping) -> &Self;

    #[ref_cast_custom]
    pub(crate) fn from_parser_mut(mapping: &mut ayaml::Mapping) -> &mut Self;

    pub(crate) fn from_parser(mapping: ayaml::Mapping) -> Self {
        Self(mapping)
    }

    pub(crate) fn parser(&self) -> &ayaml::Mapping {
        &self.0
    }

    /// An empty mapping.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many keys it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether it holds no keys.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether it holds the string key `key`.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.0.get_str(key).is_some()
    }

    /// The value under the string key `key`, if there is one.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get_str(key).map(Value::from_parser_ref)
    }

    /// The value under the string key `key`, mutably, if there is one.
    #[must_use]
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.0.get_str_mut(key).map(Value::from_parser_mut)
    }

    /// Set `key` to `value`, returning the value it replaced. A new key goes
    /// last; a replaced one keeps its place.
    pub fn insert(&mut self, key: impl Into<Value>, value: impl Into<Value>) -> Option<Value> {
        self.0.insert(key.into().0, value.into().0).map(Value)
    }

    /// Remove the string key `key`, returning its value. The keys after it
    /// keep their order.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.0.shift_remove(key).map(Value)
    }

    /// The keys and their values, in order.
    pub fn iter(&self) -> MappingIter<'_> {
        MappingIter((&self.0).into_iter())
    }

    /// The keys and their values, mutably, in order.
    pub fn iter_mut(&mut self) -> MappingIterMut<'_> {
        MappingIterMut((&mut self.0).into_iter())
    }

    /// The keys, in order.
    pub fn keys(&self) -> MappingKeys<'_> {
        MappingKeys(self.iter())
    }

    /// The values, in order.
    pub fn values(&self) -> MappingValues<'_> {
        MappingValues(self.iter())
    }

    /// The values, mutably, in order.
    pub fn values_mut(&mut self) -> MappingValuesMut<'_> {
        MappingValuesMut(self.iter_mut())
    }
}

impl fmt::Debug for Mapping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for Mapping {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Mapping {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        ayaml::Mapping::deserialize(deserializer).map(Self)
    }
}

/// `mapping[key]` is the value under `key`, or null when there is none.
impl std::ops::Index<&str> for Mapping {
    type Output = Value;

    fn index(&self, key: &str) -> &Value {
        self.get(key).unwrap_or(&NULL)
    }
}

/// `mapping[&key]` is `mapping[key.as_str()]`.
impl std::ops::Index<&String> for Mapping {
    type Output = Value;

    fn index(&self, key: &String) -> &Value {
        &self[key.as_str()]
    }
}

/// `mapping[key] = ...` sets the value under `key`; a missing key is added
/// last, with null until the assignment.
impl std::ops::IndexMut<&str> for Mapping {
    fn index_mut(&mut self, key: &str) -> &mut Value {
        Value::from_parser_mut(
            self.0
                .entry(ayaml::Value::from(key))
                .or_insert(ayaml::Value::Null),
        )
    }
}

/// `mapping[&key] = ...` is `mapping[key.as_str()] = ...`.
impl std::ops::IndexMut<&String> for Mapping {
    fn index_mut(&mut self, key: &String) -> &mut Value {
        &mut self[key.as_str()]
    }
}

impl FromIterator<(Value, Value)> for Mapping {
    fn from_iter<I: IntoIterator<Item = (Value, Value)>>(entries: I) -> Self {
        Self(
            entries
                .into_iter()
                .map(|(key, value)| (key.0, value.0))
                .collect(),
        )
    }
}

impl Extend<(Value, Value)> for Mapping {
    /// Inserts each entry as [`Mapping::insert`] does.
    fn extend<I: IntoIterator<Item = (Value, Value)>>(&mut self, entries: I) {
        self.0
            .extend(entries.into_iter().map(|(key, value)| (key.0, value.0)));
    }
}

impl<'a> IntoIterator for &'a Mapping {
    type Item = (&'a Value, &'a Value);
    type IntoIter = MappingIter<'a>;

    fn into_iter(self) -> MappingIter<'a> {
        self.iter()
    }
}

impl<'a> IntoIterator for &'a mut Mapping {
    type Item = (&'a Value, &'a mut Value);
    type IntoIter = MappingIterMut<'a>;

    fn into_iter(self) -> MappingIterMut<'a> {
        self.iter_mut()
    }
}

impl IntoIterator for Mapping {
    type Item = (Value, Value);
    type IntoIter = MappingIntoIter;

    fn into_iter(self) -> MappingIntoIter {
        MappingIntoIter(self.0.into_iter())
    }
}

/// The keys of a [`Mapping`] and their values, in order.
#[derive(Clone, Debug)]
pub struct MappingIter<'a>(<&'a ayaml::Mapping as IntoIterator>::IntoIter);

impl<'a> Iterator for MappingIter<'a> {
    type Item = (&'a Value, &'a Value);

    fn next(&mut self) -> Option<Self::Item> {
        self.0
            .next()
            .map(|(key, value)| (Value::from_parser_ref(key), Value::from_parser_ref(value)))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl DoubleEndedIterator for MappingIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0
            .next_back()
            .map(|(key, value)| (Value::from_parser_ref(key), Value::from_parser_ref(value)))
    }
}

impl ExactSizeIterator for MappingIter<'_> {}

impl FusedIterator for MappingIter<'_> {}

/// The keys of a [`Mapping`] and their values, mutably, in order.
#[derive(Debug)]
pub struct MappingIterMut<'a>(<&'a mut ayaml::Mapping as IntoIterator>::IntoIter);

impl<'a> Iterator for MappingIterMut<'a> {
    type Item = (&'a Value, &'a mut Value);

    fn next(&mut self) -> Option<Self::Item> {
        self.0
            .next()
            .map(|(key, value)| (Value::from_parser_ref(key), Value::from_parser_mut(value)))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl DoubleEndedIterator for MappingIterMut<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0
            .next_back()
            .map(|(key, value)| (Value::from_parser_ref(key), Value::from_parser_mut(value)))
    }
}

impl ExactSizeIterator for MappingIterMut<'_> {}

impl FusedIterator for MappingIterMut<'_> {}

/// The keys of a [`Mapping`] it gave up and their values, in order.
#[derive(Clone, Debug)]
pub struct MappingIntoIter(<ayaml::Mapping as IntoIterator>::IntoIter);

impl Iterator for MappingIntoIter {
    type Item = (Value, Value);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(key, value)| (Value(key), Value(value)))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl DoubleEndedIterator for MappingIntoIter {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0
            .next_back()
            .map(|(key, value)| (Value(key), Value(value)))
    }
}

impl ExactSizeIterator for MappingIntoIter {}

impl FusedIterator for MappingIntoIter {}

/// The iterator impls of one side of a [`Mapping`]'s entries: `$name` yields `$item`,
/// taken by `$take` from each entry of the iterator it wraps. Each type is written out
/// where it is defined, with its documentation and derives, so the macro builds no
/// attribute (the public-API gate refuses one a macro builds).
macro_rules! mapping_side {
    ($name:ident => $item:ty, $take:expr) => {
        impl<'a> Iterator for $name<'a> {
            type Item = $item;

            fn next(&mut self) -> Option<$item> {
                self.0.next().map($take)
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                self.0.size_hint()
            }
        }

        impl DoubleEndedIterator for $name<'_> {
            fn next_back(&mut self) -> Option<Self::Item> {
                self.0.next_back().map($take)
            }
        }

        impl ExactSizeIterator for $name<'_> {}

        impl FusedIterator for $name<'_> {}
    };
}

/// The keys of a [`Mapping`], in order.
#[derive(Clone, Debug)]
pub struct MappingKeys<'a>(MappingIter<'a>);

mapping_side!(MappingKeys => &'a Value, |(key, _)| key);

/// The values of a [`Mapping`], in order.
#[derive(Clone, Debug)]
pub struct MappingValues<'a>(MappingIter<'a>);

mapping_side!(MappingValues => &'a Value, |(_, value)| value);

/// The values of a [`Mapping`], mutably, in order.
#[derive(Debug)]
pub struct MappingValuesMut<'a>(MappingIterMut<'a>);

mapping_side!(MappingValuesMut => &'a mut Value, |(_, value)| value);

/// Why a YAML document did not parse, or a value did not decode or encode.
///
/// It prints a message and, where there is one, the location.
pub struct Error(ErrorRepr);

/// The error of the parser that failed (see [`crate::yaml_backend`]).
enum ErrorRepr {
    Ayaml(ayaml::Error),
    /// ayaml's, with [`crate::yaml_decode::INDENTATION_HINT`], and its
    /// message with the hint. `Display` puts the hint after the location.
    /// Boxed, so the error is no larger than ayaml's: the loader holds many
    /// on the stack of its `extends:` recursion.
    Hinted(Box<(ayaml::Error, String)>),
    /// libyaml's, through `yaml_serde`, with its message without the
    /// locations its `Display` adds.
    #[cfg(feature = "yaml-reference")]
    Libyaml(yaml_serde::Error, String),
}

impl Error {
    pub(crate) fn ayaml(error: ayaml::Error) -> Self {
        Self(ErrorRepr::Ayaml(error))
    }

    /// ayaml's error from decoding `input`, with duplicate-key errors placed at
    /// the repeated key (#572) and a hint when it is likely an indentation
    /// mistake (see [`crate::yaml_decode::indentation_hint`]).
    pub(crate) fn from_text(input: &str, error: ayaml::Error) -> Self {
        let error = relocate_duplicate_key_error(input, error);
        if crate::yaml_decode::indentation_hint(input, &error) {
            let message = format!(
                "{} {}",
                error.message(),
                crate::yaml_decode::INDENTATION_HINT
            );
            Self(ErrorRepr::Hinted(Box::new((error, message))))
        } else {
            Self::ayaml(error)
        }
    }

    #[cfg(feature = "yaml-reference")]
    pub(crate) fn libyaml(error: yaml_serde::Error) -> Self {
        let message = crate::yaml_backend::without_marks(&error.to_string());
        Self(ErrorRepr::Libyaml(error, message))
    }

    #[cfg(test)]
    pub(crate) fn from_parser(error: ayaml::Error) -> Self {
        Self::ayaml(error)
    }

    /// The kind of limit ayaml's error is about, if it is about one.
    #[cfg(feature = "yaml-reference")]
    pub(crate) fn limit(&self) -> Option<ayaml::LimitKind> {
        let error = match &self.0 {
            ErrorRepr::Ayaml(error) => error,
            ErrorRepr::Hinted(hinted) => &hinted.0,
            ErrorRepr::Libyaml(..) => return None,
        };
        error.limit().map(ayaml::LimitExceeded::kind)
    }

    /// ayaml's error, without a hint alint added; a caller that reads one
    /// decodes with ayaml.
    #[cfg(any(test, feature = "yaml-reference"))]
    pub(crate) fn into_parser(self) -> ayaml::Error {
        match self.0 {
            ErrorRepr::Ayaml(error) => error,
            ErrorRepr::Hinted(hinted) => hinted.0,
            #[cfg(feature = "yaml-reference")]
            ErrorRepr::Libyaml(error, _) => panic!("a libyaml error, not ayaml's: {error}"),
        }
    }

    /// The message, without the error's own location, which `Display` puts
    /// after the problem. The message can still name another place, such as
    /// where the collection that failed began, and it ends with any hint,
    /// which `Display` puts after the location.
    #[must_use]
    pub fn message(&self) -> &str {
        match &self.0 {
            ErrorRepr::Ayaml(error) => error.message(),
            ErrorRepr::Hinted(hinted) => &hinted.1,
            #[cfg(feature = "yaml-reference")]
            ErrorRepr::Libyaml(_, message) => message,
        }
    }

    /// A redacted diagnostic without document text, naming only the error
    /// category, error code (when available), and source location.
    #[must_use]
    pub fn masked_message(&self) -> String {
        match &self.0 {
            ErrorRepr::Ayaml(error) => error.masked().to_string(),
            ErrorRepr::Hinted(hinted) => hinted.0.masked().to_string(),
            #[cfg(feature = "yaml-reference")]
            ErrorRepr::Libyaml(_, _) => match self.location() {
                Some(location) => format!(
                    "YAML parse error at line {} column {}",
                    location.line(),
                    location.column()
                ),
                None => "YAML parse error".to_string(),
            },
        }
    }

    /// Where in the document the error is, if it has a place there.
    #[must_use]
    pub fn location(&self) -> Option<Location> {
        let ayaml = |error: &ayaml::Error| {
            error.location().map(|location| Location {
                line: location.line(),
                column: location.column(),
            })
        };
        match &self.0 {
            ErrorRepr::Ayaml(error) => ayaml(error),
            ErrorRepr::Hinted(hinted) => ayaml(&hinted.0),
            #[cfg(feature = "yaml-reference")]
            ErrorRepr::Libyaml(error, _) => error.location().map(|location| Location {
                line: location.line(),
                column: location.column(),
            }),
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            ErrorRepr::Ayaml(error) => error.fmt(f),
            ErrorRepr::Hinted(hinted) => hinted.0.fmt(f),
            #[cfg(feature = "yaml-reference")]
            ErrorRepr::Libyaml(error, _) => error.fmt(f),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            ErrorRepr::Ayaml(error) => error.fmt(f),
            ErrorRepr::Hinted(hinted) => {
                write!(f, "{} {}", hinted.0, crate::yaml_decode::INDENTATION_HINT)
            }
            #[cfg(feature = "yaml-reference")]
            ErrorRepr::Libyaml(error, _) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.0 {
            ErrorRepr::Ayaml(error) => error.source(),
            ErrorRepr::Hinted(hinted) => hinted.0.source(),
            #[cfg(feature = "yaml-reference")]
            ErrorRepr::Libyaml(error, _) => error.source(),
        }
    }
}

/// An error a [`Deserialize`] or [`Serialize`] implementation reports with
/// this message.
impl serde::de::Error for Error {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self::ayaml(ayaml::Error::new(
            ayaml::ErrorKind::Serde,
            message.to_string(),
        ))
    }
}

impl serde::ser::Error for Error {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self::ayaml(ayaml::Error::new(
            ayaml::ErrorKind::Serde,
            message.to_string(),
        ))
    }
}

/// A place in a YAML document: a line and a column, both counted from one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Location {
    line: usize,
    column: usize,
}

impl Location {
    /// The line, counted from one.
    #[must_use]
    pub fn line(self) -> usize {
        self.line
    }

    /// The column, counted from one.
    #[must_use]
    pub fn column(self) -> usize {
        self.column
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    /// Every limit a decode runs under. The guards in `yaml_depth` and the
    /// parser bound depth and aliases from the same constants, so the depth and
    /// alias limits are alint's constants, and ayaml's defaults are checked
    /// against them so that an ayaml release that changes one is noticed.
    #[test]
    fn the_parser_limits_are_alints() {
        use crate::yaml_depth::{
            MAX_YAML_EXPANSION_BYTES, MAX_YAML_FLOW_DEPTH, YAML_EXPANSION_BYTES_PER_INPUT_BYTE,
            YAML_NODE_COST,
        };
        let limits = limits();
        let pinned = [
            (limits.max_input_bytes, crate::MAX_STRUCTURED_BYTES),
            (limits.max_depth, MAX_YAML_FLOW_DEPTH),
            (limits.max_alias_expansion_bytes, MAX_YAML_EXPANSION_BYTES),
            (
                limits.alias_bytes_per_input_byte,
                YAML_EXPANSION_BYTES_PER_INPUT_BYTE,
            ),
            (limits.node_cost_bytes, YAML_NODE_COST),
            (limits.max_events, usize::MAX),
            (limits.max_collection_entries, usize::MAX),
            (limits.max_anchors, usize::MAX),
            (limits.max_alias_dereferences, usize::MAX),
            // A scalar may be the whole input.
            (limits.max_scalar_bytes, crate::MAX_STRUCTURED_BYTES),
            (limits.max_documents, 10_000),
            (limits.max_output_bytes, 64 * 1024 * 1024),
            (limits.max_tag_expansion_bytes, 64 * 1024 * 1024),
        ];
        for (index, (actual, expected)) in pinned.into_iter().enumerate() {
            assert_eq!(actual, expected, "limit {index}");
        }
        let defaults = ayaml::Limits::default();
        assert_eq!(
            (
                defaults.max_input_bytes,
                defaults.max_depth,
                defaults.max_alias_expansion_bytes,
                defaults.alias_bytes_per_input_byte,
                defaults.node_cost_bytes,
            ),
            (
                crate::MAX_STRUCTURED_BYTES,
                MAX_YAML_FLOW_DEPTH,
                MAX_YAML_EXPANSION_BYTES,
                YAML_EXPANSION_BYTES_PER_INPUT_BYTE,
                YAML_NODE_COST,
            )
        );
    }

    /// A valid document with more events, entries, anchors and alias uses
    /// than ayaml's defaults allow (1,000,000, 1,000,000, 100,000 and
    /// 100,000) decodes, as it did in v0.22. Each count sits just past its
    /// default, so dropping any one override fails here.
    #[test]
    fn a_large_valid_document_decodes() {
        // 1,000,001 entries in one flow sequence: past the entries default
        // and, with the sequence's own events, the events default.
        let mut entries = String::with_capacity(2_100_000);
        entries.push('[');
        for i in 0..1_000_001 {
            if i > 0 {
                entries.push(',');
            }
            entries.push('0');
        }
        entries.push(']');
        let decoded: Vec<u8> = from_str(&entries).unwrap();
        assert_eq!(decoded.len(), 1_000_001);

        // 100,001 anchors, and 100,001 uses of one of them.
        let mut anchors = String::with_capacity(4_000_000);
        anchors.push_str("a: &x 0\nanchored: [");
        for i in 0..100_001 {
            if i > 0 {
                anchors.push(',');
            }
            write!(anchors, "&a{i} 0").unwrap();
        }
        anchors.push_str("]\nused: [");
        for i in 0..100_001 {
            if i > 0 {
                anchors.push(',');
            }
            anchors.push_str("*x");
        }
        anchors.push_str("]\n");
        let decoded: std::collections::BTreeMap<String, ayaml::Value> = from_str(&anchors).unwrap();
        assert_eq!(decoded.len(), 3);
    }

    /// The accessors read each kind of value, and look through a tag.
    #[test]
    fn value_accessors_read_each_kind() {
        let value: Value = from_str(concat!(
            "s: text\nt: true\nb: false\nn: -3\nu: 18446744073709551615\nf: 0.5\nz: ~\n",
            "q: [a, 1]\nm: {k: v}\ntagged: !celsius 21\n",
        ))
        .unwrap();
        assert_eq!(value.get("s").and_then(Value::as_str), Some("text"));
        assert_eq!(value.get("t").and_then(Value::as_bool), Some(true));
        assert_eq!(value.get("b").and_then(Value::as_bool), Some(false));
        assert_eq!(value.get("s").and_then(Value::as_bool), None);
        assert_eq!(value.get("n").and_then(Value::as_i64), Some(-3));
        assert_eq!(value.get("n").and_then(Value::as_u64), None);
        assert_eq!(value.get("u").and_then(Value::as_u64), Some(u64::MAX));
        assert_eq!(value.get("u").and_then(Value::as_i64), None);
        assert_eq!(value.get("f").and_then(Value::as_f64), Some(0.5));
        assert!(value.get("z").is_some_and(Value::is_null));
        assert!(!value["s"].is_null());
        assert_eq!(value["tagged"].as_u64(), Some(21));
        let sequence = value["q"].as_sequence().unwrap();
        assert_eq!(sequence.len(), 2);
        assert!(!sequence.is_empty());
        assert_eq!(sequence.get(0).and_then(Value::as_str), Some("a"));
        assert_eq!(sequence.get(1).and_then(Value::as_u64), Some(1));
        assert!(sequence.get(2).is_none());
        assert_eq!(value["q"][1], 1);
        assert_eq!(value["m"].as_mapping().map(Mapping::len), Some(1));
        assert_eq!(value["m"]["k"], "v");
        // A value of another kind answers `None`, and indexing it is null.
        assert!(value["s"].as_mapping().is_none());
        assert!(value["s"].as_sequence().is_none());
        assert!(value["s"].get("k").is_none());
        assert!(value["s"]["k"].is_null());
        assert!(value["s"][0].is_null());
        assert!(value["q"][2].is_null());
        assert!(value["missing"].is_null());
        assert!(Value::default().is_null());
    }

    /// A mapping keeps its keys in order: a new key goes last, a replaced
    /// one keeps its place, and removing one keeps the others' order.
    #[test]
    fn mapping_edits_keep_key_order() {
        let mut mapping = Mapping::new();
        assert!(mapping.is_empty());
        assert_eq!(mapping.insert("a", 1_u64), None);
        assert_eq!(mapping.insert("b", true), None);
        assert_eq!(mapping.insert("c", "x"), None);
        assert_eq!(mapping.insert("a", 2_u64), Some(Value::from(1_u64)));
        assert_eq!(mapping.len(), 3);
        assert!(!mapping.is_empty());
        let keys = |mapping: &Mapping| -> Vec<String> {
            mapping
                .keys()
                .map(|key| key.as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(keys(&mapping), ["a", "b", "c"]);
        assert_eq!(mapping["a"], 2);
        assert_eq!(mapping.remove("a"), Some(Value::from(2_u64)));
        assert_eq!(mapping.remove("a"), None);
        assert_eq!(keys(&mapping), ["b", "c"]);
        assert!(mapping.contains_key("b"));
        assert!(!mapping.contains_key("a"));
        assert!(mapping["a"].is_null());
        *mapping.get_mut("b").unwrap() = Value::from("y");
        assert_eq!(mapping.get("b").and_then(Value::as_str), Some("y"));
        assert!(mapping.get_mut("missing").is_none());
        mapping
            .iter_mut()
            .for_each(|(_, value)| *value = Value::from(7_i64));
        let values: Vec<_> = mapping.values().cloned().collect();
        assert_eq!(values, [Value::from(7_i64), Value::from(7_i64)]);
        let mut entries = mapping.iter();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries.next_back().map(|(key, _)| key.clone()),
            Some(Value::from("c"))
        );
        assert_eq!((&mapping).into_iter().count(), 2);
        let collected: Mapping = mapping
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        assert_eq!(collected, mapping);
    }

    /// A sequence keeps its values in order, and edits through `iter_mut`
    /// and a mutable view land in the value that holds it.
    #[test]
    fn sequence_edits_land_in_place() {
        let mut sequence: Sequence = [Value::from("a"), Value::from(1_u64)].into_iter().collect();
        sequence.push(false);
        assert_eq!(sequence.len(), 3);
        sequence
            .iter_mut()
            .for_each(|value| *value = Value::from(value.as_str().unwrap_or("other")));
        let mut value = Value::from(sequence);
        value.as_sequence_mut().unwrap().push("d");
        let strings: Vec<_> = value
            .as_sequence()
            .unwrap()
            .iter()
            .rev()
            .map(|value| value.as_str().unwrap())
            .collect();
        assert_eq!(strings, ["d", "other", "other", "a"]);
        assert_eq!((value.as_sequence().unwrap()).into_iter().len(), 4);
        assert_eq!(Value::from(vec![Value::from(1_u64)])[0], 1);
        assert!(Sequence::new().is_empty());
        let mut mapping = Value::from(Mapping::new());
        mapping.as_mapping_mut().unwrap().insert("k", "v");
        assert_eq!(mapping["k"], "v");
        assert!(Value::from("s").as_sequence_mut().is_none());
        assert!(Value::from("s").as_mapping_mut().is_none());
    }

    /// Values compare with plain Rust values of the same kind only.
    #[test]
    fn values_compare_with_plain_values() {
        assert_eq!(Value::from("x"), "x");
        assert_eq!(Value::from("x"), *"x");
        assert_eq!(Value::from("x"), "x".to_string());
        assert_ne!(Value::from("1"), 1);
        assert_eq!(Value::from(true), true);
        assert_ne!(Value::from(true), "true");
        assert_eq!(Value::from(-2_i64), -2_i64);
        assert_eq!(Value::from(-2_i64), -2);
        assert_ne!(Value::from(3_i64), 4);
        assert_eq!(Value::from(3_u64), 3_u64);
        assert_eq!(Value::from(0.25), Value::from(0.25));
        assert_eq!(Value::from("s".to_string()), Value::from("s"));
    }

    /// The types read and write as the parser's do: a flattened mapping, a
    /// typed decode, a round trip, and `{:?}` output (which snapshots hold).
    #[test]
    fn serde_and_debug_match_the_parser() {
        #[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
        struct Options {
            pattern: String,
            limits: Vec<u32>,
        }
        #[derive(serde::Deserialize)]
        struct Spec {
            kind: String,
            #[serde(flatten)]
            extra: Mapping,
        }
        let text = "kind: k\npattern: x\nlimits: [1, 2]\n";
        let spec: Spec = from_str(text).unwrap();
        assert_eq!(spec.kind, "k");
        let parser: ayaml::Mapping =
            ayaml::compat::from_str_with("pattern: x\nlimits: [1, 2]\n", &compat_parse_options())
                .unwrap();
        assert_eq!(format!("{:?}", spec.extra), format!("{parser:?}"));
        assert_eq!(
            format!("{:?}", spec.extra["limits"]),
            format!("{:?}", parser["limits"])
        );
        assert_eq!(
            format!("{:?}", spec.extra["limits"].as_sequence().unwrap()),
            format!("{:?}", parser["limits"].as_sequence().unwrap())
        );
        let options: Options = from_value(Value::from(spec.extra.clone())).unwrap();
        assert_eq!(options.limits, [1, 2]);
        assert_eq!(to_value(&options).unwrap(), Value::from(spec.extra.clone()));
        let written = to_string(&spec.extra).unwrap();
        assert_eq!(from_str::<Mapping>(&written).unwrap(), spec.extra);
        let sequence: Sequence = from_str("[a, b]").unwrap();
        let items: Vec<_> = sequence.iter().map(|item| item.as_str().unwrap()).collect();
        assert_eq!(items, ["a", "b"]);
        assert_eq!(
            from_str::<Sequence>(&to_string(&sequence).unwrap()).unwrap(),
            sequence
        );
        assert_eq!(serde_json::to_value(&spec.extra).unwrap()["limits"][1], 2);
    }

    /// An error gives the parser's message, without and with its location,
    /// and a Serde error carries its message alone.
    #[test]
    fn errors_give_message_and_location() {
        let error = from_str::<Value>("a: [1, 2\n").unwrap_err();
        let location = error.location().unwrap();
        assert_eq!((location.line(), location.column()), (2, 1), "{error}");
        // The message leaves out the location that the error prints.
        assert!(error.to_string().contains("at line 2 column 1"), "{error}");
        assert!(
            !error.message().contains("at line 2 column 1"),
            "{}",
            error.message()
        );
        assert!(
            error.message().starts_with("did not find expected"),
            "{}",
            error.message()
        );
        let inline = from_str::<Value>("a: b: c\n")
            .unwrap_err()
            .location()
            .unwrap();
        assert_eq!((inline.line(), inline.column()), (1, 5));
        let typed = from_str::<u32>("text").unwrap_err();
        assert!(
            typed.message().contains("invalid type"),
            "{}",
            typed.message()
        );
        let custom = <Error as serde::de::Error>::custom("bad option");
        assert_eq!(custom.to_string(), "bad option");
        assert_eq!(custom.message(), "bad option");
        assert!(custom.location().is_none());
        assert!(std::error::Error::source(&custom).is_none());
        let chained = Error::from_parser(
            ayaml::Error::new(ayaml::ErrorKind::Serde, "outer")
                .with_source(std::io::Error::other("cause")),
        );
        let source = std::error::Error::source(&chained).map(ToString::to_string);
        assert_eq!(source.as_deref(), Some("cause"));
        let custom = <Error as serde::ser::Error>::custom("cannot write");
        assert_eq!(custom.to_string(), "cannot write");
        assert_eq!(format!("{custom:?}"), format!("{:?}", custom.into_parser()));
    }

    /// The caps that stay still refuse: depth past 128, and input past the
    /// 32 MiB cap.
    #[test]
    fn the_depth_and_input_caps_still_refuse() {
        // The caps are ayaml's limits; a reference parser has its own.
        let deep = format!("{}{}", "[".repeat(129), "]".repeat(129));
        assert!(ayaml_from_str::<ayaml::Value>(&deep).is_err());
        let large = format!("a: \"{}\"\n", "x".repeat(crate::MAX_STRUCTURED_BYTES));
        assert!(ayaml_from_str::<ayaml::Value>(&large).is_err());
    }

    /// The predicates and the tag accessor read each kind of value; the
    /// predicates, like the other accessors, look through a tag. A global tag
    /// reads by its full name however it was written, a YAML-namespace one in
    /// its short form, and a tag that restates the kind of value is dropped.
    #[test]
    fn predicates_and_tag_read_each_kind() {
        let value: Value = from_str(concat!(
            "b: true\nn: 1.5\ns: text\nq: [a]\nm: {k: v}\nz: ~\n",
            "local: !celsius 21\ncore: !!str 7\n",
            "verbatim: !<tag:x,2000:y> 1\nbinary: !!binary aGk=\nset: !!set {a: ~}\n",
            "omap: !!omap [a: 1]\nnonspecific: ! 21\n",
        ))
        .unwrap();
        assert_eq!(value["verbatim"].tag().as_deref(), Some("tag:x,2000:y"));
        assert_eq!(value["verbatim"].as_u64(), Some(1));
        let expanded: Value = from_str("%TAG !e! tag:x,2000:\n--- !e!y 1\n").unwrap();
        assert_eq!(expanded.tag().as_deref(), Some("tag:x,2000:y"));
        assert_eq!(value["binary"].tag().as_deref(), Some("!!binary"));
        assert_eq!(value["binary"].as_str(), Some("aGk="));
        assert_eq!(value["set"].tag(), None);
        assert!(value["set"].is_mapping());
        assert_eq!(value["omap"].tag(), None);
        assert!(value["omap"].is_sequence());
        assert_eq!(value["nonspecific"].tag().as_deref(), Some("!"));
        assert_eq!(value["nonspecific"].as_str(), Some("21"));
        let kinds = |key: &str| {
            let v = &value[key];
            [
                v.is_bool(),
                v.is_number(),
                v.is_string(),
                v.is_sequence(),
                v.is_mapping(),
                v.is_null(),
            ]
        };
        assert_eq!(kinds("b"), [true, false, false, false, false, false]);
        assert_eq!(kinds("n"), [false, true, false, false, false, false]);
        assert_eq!(kinds("s"), [false, false, true, false, false, false]);
        assert_eq!(kinds("q"), [false, false, false, true, false, false]);
        assert_eq!(kinds("m"), [false, false, false, false, true, false]);
        assert_eq!(kinds("z"), [false, false, false, false, false, true]);
        assert_eq!(kinds("local"), [false, true, false, false, false, false]);
        assert_eq!(value["local"].tag().as_deref(), Some("!celsius"));
        // A core-schema tag is applied, not kept.
        assert_eq!(value["core"].tag(), None);
        assert_eq!(value["core"].as_str(), Some("7"));
        assert_eq!(value["s"].tag(), None);
        assert_eq!(value.tag(), None);
    }

    /// Every integer width and `f32` convert into a value and compare with
    /// one.
    #[test]
    fn numbers_of_every_width_convert_and_compare() {
        assert_eq!(Value::from(-8_i8), -8_i8);
        assert_eq!(Value::from(-16_i16), -16_i16);
        assert_eq!(Value::from(-32_i32), -32_i32);
        assert_eq!(Value::from(-64_i64), -64_i64);
        assert_eq!(Value::from(-128_i128), -128_i64);
        assert_eq!(Value::from(8_u8), 8_u8);
        assert_eq!(Value::from(16_u16), 16_u16);
        assert_eq!(Value::from(32_u32), 32_u32);
        assert_eq!(Value::from(64_u64), 64_u64);
        assert_eq!(Value::from(128_u128), 128_u64);
        assert_eq!(Value::from(7_usize), 7_usize);
        assert_eq!(Value::from(7_usize), 7_isize);
        assert_eq!(Value::from(-7_isize), -7_isize);
        assert_eq!(Value::from(-7_isize).as_i64(), Some(-7));
        assert_eq!(Value::from(0.5_f32), 0.5_f32);
        assert_eq!(Value::from(0.25_f64), 0.25_f64);
        assert_ne!(Value::from(1_i32), 2_i32);
        assert_ne!(Value::from("1"), 1_i32);
        let mut mapping = Mapping::new();
        mapping.insert("n", 3_i32);
        assert_eq!(mapping["n"].as_i64(), Some(3));
    }

    /// Indexing reads and writes: a value's index assignment adds a key, and a
    /// sequence indexes, pops and builds from a `Vec`.
    #[test]
    fn values_and_sequences_index_and_assign() {
        let mut value = Value::default();
        value["k"] = Value::from("v");
        assert_eq!(value["k"], "v");
        value["k"] = Value::from(vec![Value::from(1_i32), Value::from(2_i32)]);
        value["k"][1] = Value::from(5_i32);
        assert_eq!(value["k"][1], 5_i32);

        let mut sequence = Sequence::from(vec![Value::from("a"), Value::from("b")]);
        assert_eq!(sequence[0], "a");
        sequence[0] = Value::from("c");
        *sequence.get_mut(1).unwrap() = Value::from("d");
        assert_eq!(sequence.pop(), Some(Value::from("d")));
        assert_eq!(sequence.len(), 1);
        assert_eq!(sequence[0], "c");
        assert_eq!(Sequence::new().pop(), None);
        assert!(sequence.get_mut(5).is_none());

        // An integer index reads and writes a mapping's integer key, as 0.22 did.
        let mut numbered: Value = from_str("{0: zero, x: 1}").unwrap();
        assert_eq!(numbered[0], "zero");
        assert!(numbered[1].is_null());
        numbered[1] = Value::from("one");
        assert_eq!(numbered[1], "one");
        assert!(Value::from("s")[0].is_null());
        assert!(Value::default()[0].is_null());
    }

    /// Assigning a key on a tagged null makes it a mapping that keeps the tag,
    /// as an untagged null becomes a mapping.
    #[test]
    fn index_assignment_on_a_tagged_null_keeps_the_tag() {
        let mut value: Value = from_str("!t ~").unwrap();
        assert!(value.is_null());
        value["k"] = Value::from("v");
        assert_eq!(value["k"], "v");
        assert!(value.is_mapping());
        assert_eq!(value.tag().as_deref(), Some("!t"));
        let key = String::from("j");
        value[&key] = Value::from(1_i32);
        assert_eq!(value[&key], 1_i32);
        assert_eq!(value["k"], "v");
    }

    /// `Value::get_mut`, indexing by `&String`, and a mapping's index
    /// assignment, which adds a missing key last; reading one adds nothing.
    #[test]
    fn get_mut_string_keys_and_mapping_assignment() {
        let mut value: Value = from_str("a: 1\nb: [x]\n").unwrap();
        *value.get_mut("a").unwrap() = Value::from(2_i32);
        assert_eq!(value["a"], 2_i32);
        assert!(value.get_mut("missing").is_none());
        assert!(Value::from("s").get_mut("a").is_none());
        let key = String::from("b");
        assert_eq!(value[&key][0], "x");
        let mapping = value.as_mapping_mut().unwrap();
        assert_eq!(mapping[&key][0], "x");
        mapping["a"] = Value::from(3_i32);
        mapping[&String::from("c")] = Value::from(true);
        assert!(mapping["missing"].is_null());
        assert_eq!(mapping["a"], 3_i32);
        assert_eq!(mapping["c"], true);
        let keys: Vec<_> = mapping.keys().map(|key| key.as_str().unwrap()).collect();
        assert_eq!(keys, ["a", "b", "c"]);
    }

    /// The string-key lookups find an untagged string key only: `!t k` is a
    /// tagged key, which `get("k")` does not find and iteration does.
    #[test]
    fn string_key_lookups_skip_a_tagged_key() {
        let mut value: Value = from_str("!t k: tagged\nplain: 1\n").unwrap();
        assert!(value.get("k").is_none());
        assert!(value.get_mut("k").is_none());
        assert!(value["k"].is_null());
        let mapping = value.as_mapping_mut().unwrap();
        assert!(mapping.get("k").is_none());
        assert!(mapping.get_mut("k").is_none());
        assert!(!mapping.contains_key("k"));
        assert!(mapping["k"].is_null());
        assert!(mapping.remove("k").is_none());
        assert_eq!(mapping.len(), 2);
        let (key, tagged) = mapping.iter().next().unwrap();
        assert_eq!(
            (key.tag().as_deref(), key.as_str()),
            (Some("!t"), Some("k"))
        );
        assert_eq!(*tagged, "tagged");
        assert_eq!(mapping["plain"], 1_i32);
    }

    /// The owned types' auto traits, pinned: a user may rely on each, so
    /// losing one, as wrapping another parser type could, is a breaking
    /// change. Every type here is `Send`, `Sync` and `Unpin`, and all but
    /// `Error` are also `UnwindSafe` and `RefUnwindSafe`; `Error` is `'static`
    /// and boxes into `Box<dyn std::error::Error + Send + Sync>`.
    ///
    /// Not asserted, since a negative bound needs a dependency: `Error` is not
    /// `UnwindSafe` or `RefUnwindSafe`, because ayaml's error is not, and the
    /// iterators that borrow mutably (`SequenceIterMut`, `MappingIterMut`,
    /// `MappingValuesMut`) are not `UnwindSafe`, as no `&mut` is.
    #[test]
    fn owned_types_keep_their_auto_traits() {
        fn threads<T: Send + Sync + Unpin>() {}
        fn unwind<T: std::panic::UnwindSafe + std::panic::RefUnwindSafe>() {}
        fn ref_unwind<T: std::panic::RefUnwindSafe>() {}
        fn error<T: std::error::Error + Send + Sync + 'static>() {}

        threads::<Value>();
        threads::<Mapping>();
        threads::<Sequence>();
        threads::<Location>();
        threads::<Error>();
        threads::<SequenceIter<'_>>();
        threads::<SequenceIterMut<'_>>();
        threads::<SequenceIntoIter>();
        threads::<MappingIter<'_>>();
        threads::<MappingIterMut<'_>>();
        threads::<MappingIntoIter>();
        threads::<MappingKeys<'_>>();
        threads::<MappingValues<'_>>();
        threads::<MappingValuesMut<'_>>();
        unwind::<Value>();
        unwind::<Mapping>();
        unwind::<Sequence>();
        unwind::<Location>();
        unwind::<SequenceIter<'_>>();
        unwind::<SequenceIntoIter>();
        unwind::<MappingIter<'_>>();
        unwind::<MappingIntoIter>();
        unwind::<MappingKeys<'_>>();
        unwind::<MappingValues<'_>>();
        ref_unwind::<SequenceIterMut<'_>>();
        ref_unwind::<MappingIterMut<'_>>();
        ref_unwind::<MappingValuesMut<'_>>();
        error::<Error>();
    }

    /// An integer index assignment on a scalar panics.
    #[test]
    #[should_panic(expected = "cannot access index 0")]
    fn value_index_assignment_on_a_scalar_panics() {
        let mut value = Value::from("s");
        value[0] = Value::from(1_i32);
    }

    /// `sequence[index]` past the end panics, as a `Vec`'s does.
    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn sequence_index_past_the_end_panics() {
        let _ = &Sequence::new()[0];
    }

    /// Mappings and sequences iterate owned and mutably, in order, and extend.
    #[test]
    fn collections_iterate_owned_and_mutably_and_extend() {
        let mut mapping: Mapping = from_str("a: 1\nb: 2\n").unwrap();
        for (_, value) in &mut mapping {
            *value = Value::from(value.as_i64().unwrap() * 10);
        }
        mapping.extend([(Value::from("c"), Value::from(30_i32))]);
        let owned: Vec<(Value, Value)> = mapping.clone().into_iter().collect();
        assert_eq!(
            owned,
            [
                (Value::from("a"), Value::from(10_i32)),
                (Value::from("b"), Value::from(20_i32)),
                (Value::from("c"), Value::from(30_i32)),
            ]
        );
        assert_eq!(mapping.iter_mut().len(), 3);
        assert_eq!(mapping.into_iter().next_back().unwrap().0, "c");

        let mut sequence: Sequence = from_str("[1, 2]").unwrap();
        for value in &mut sequence {
            *value = Value::from(value.as_i64().unwrap() + 1);
        }
        sequence.extend([Value::from(4_i32)]);
        let owned: Vec<Value> = sequence.clone().into_iter().collect();
        assert_eq!(
            owned,
            [Value::from(2_i32), Value::from(3_i32), Value::from(4_i32)]
        );
        assert_eq!(sequence.iter_mut().next_back().unwrap(), &4_i32);
        assert_eq!(sequence.into_iter().len(), 3);
    }

    /// Each of the nine iterators knows its length, runs from both ends, and
    /// stays done once done. Every collection takes one item more than it
    /// expects, so an iterator that never ends fails here instead of
    /// exhausting memory.
    #[test]
    fn every_iterator_knows_its_length_and_runs_backward() {
        fn sized<I: ExactSizeIterator + DoubleEndedIterator + FusedIterator>(iter: I) -> I {
            iter
        }
        let mut sequence: Sequence = from_str("[1, 2, 3]").unwrap();
        let mut mapping: Mapping = from_str("a: 1\nb: 2\nc: 3\n").unwrap();
        for hint in [
            sized(sequence.iter()).size_hint(),
            sized(sequence.iter_mut()).size_hint(),
            sized(sequence.clone().into_iter()).size_hint(),
            sized(mapping.iter()).size_hint(),
            sized(mapping.iter_mut()).size_hint(),
            sized(mapping.clone().into_iter().clone()).size_hint(),
            sized(mapping.keys().clone()).size_hint(),
            sized(mapping.values().clone()).size_hint(),
            sized(mapping.values_mut()).size_hint(),
        ] {
            assert_eq!(hint, (3, Some(3)));
        }
        let mut keys = mapping.keys();
        assert_eq!(keys.by_ref().count(), 3);
        assert!(keys.next().is_none() && keys.next_back().is_none());

        let ints = [Value::from(3_i32), Value::from(2_i32), Value::from(1_i32)];
        let values: Vec<Value> = mapping.values().rev().take(4).cloned().collect();
        assert_eq!(values, ints);
        let values: Vec<Value> = mapping
            .values_mut()
            .rev()
            .take(4)
            .map(|v| v.clone())
            .collect();
        assert_eq!(values, ints);
        let keys: Vec<&str> = mapping
            .keys()
            .rev()
            .take(4)
            .map(|k| k.as_str().unwrap())
            .collect();
        assert_eq!(keys, ["c", "b", "a"]);
        for value in mapping.values_mut() {
            *value = Value::from(value.as_i64().unwrap() * 2);
        }
        assert_eq!(mapping["c"], 6_i32);

        let backward = [Value::from(3_i32), Value::from(2_i32), Value::from(1_i32)];
        let values: Vec<Value> = sequence.iter().rev().take(4).cloned().collect();
        assert_eq!(values, backward);
        let values: Vec<Value> = sequence
            .iter_mut()
            .rev()
            .take(4)
            .map(|v| v.clone())
            .collect();
        assert_eq!(values, backward);
        let values: Vec<Value> = sequence.clone().into_iter().rev().take(4).collect();
        assert_eq!(values, backward);

        let key = |key: &Value| key.as_str().unwrap().to_string();
        let keys: Vec<String> = mapping.iter().rev().take(4).map(|(k, _)| key(k)).collect();
        assert_eq!(keys, ["c", "b", "a"]);
        let keys: Vec<String> = mapping
            .iter_mut()
            .rev()
            .take(4)
            .map(|(k, _)| key(k))
            .collect();
        assert_eq!(keys, ["c", "b", "a"]);
        let keys: Vec<String> = mapping
            .into_iter()
            .rev()
            .take(4)
            .map(|(k, _)| key(&k))
            .collect();
        assert_eq!(keys, ["c", "b", "a"]);

        // Taking from the front shortens what the back can reach.
        let mut values = sequence.into_iter();
        assert_eq!(values.next().unwrap(), 1_i32);
        assert_eq!(values.size_hint(), (2, Some(2)));
        assert_eq!(values.next_back().unwrap(), 3_i32);
        assert_eq!(values.next_back().unwrap(), 2_i32);
        assert!(values.next_back().is_none());
    }

    /// Two mappings with the same entries are equal in any order.
    #[test]
    fn mapping_equality_ignores_order() {
        let ab: Mapping = from_str("a: 1\nb: 2\n").unwrap();
        let ba: Mapping = from_str("b: 2\na: 1\n").unwrap();
        assert_eq!(ab, ba);
        assert_ne!(ab, from_str::<Mapping>("a: 1\n").unwrap());
    }

    /// What a value prints and serializes to, pinned as text, so a change of
    /// parser that changes either fails here rather than in users' output.
    #[test]
    fn debug_and_serialized_forms_are_pinned() {
        let value: Value =
            from_str("kind: k\nlimits: [1, 2.5]\nflag: true\nnone: ~\nt: !x y\n").unwrap();
        assert_eq!(
            format!("{value:?}"),
            concat!(
                r#"Mapping(Mapping {String("kind"): String("k"), "#,
                r#"String("limits"): Sequence([Number(Number(PosInt(1))), "#,
                r#"Number(Number(Float(2.5)))]), String("flag"): Bool(true), "#,
                r#"String("none"): Null, String("t"): Tagged(TaggedValue { "#,
                r#"tag: Tag { handle: "!", suffix: "x" }, value: String("y") })})"#,
            )
        );
        // A tag serializes as a one-entry map, so JSON keeps it only as a key.
        assert_eq!(
            serde_json::to_string(&value).unwrap(),
            r#"{"kind":"k","limits":[1,2.5],"flag":true,"none":null,"t":{"!x":"y"}}"#
        );
        assert_eq!(
            to_string(&value).unwrap(),
            "kind: k\nlimits:\n- 1\n- 2.5\nflag: true\nnone: null\nt: !x y\n"
        );
    }

    #[test]
    fn masked_message_strips_document_text_and_keeps_location() {
        let error = ayaml_from_str::<u32>("top-secret-canary-token").unwrap_err();
        let masked = error.masked_message();
        assert!(!masked.contains("top-secret-canary-token"), "{masked}");
        assert_ne!(masked, "");

        let syntax = ayaml_from_str::<Value>("secret-key: [unclosed\n").unwrap_err();
        let masked = syntax.masked_message();
        assert!(!masked.contains("secret-key"), "{masked}");
        assert!(!masked.contains("unclosed"), "{masked}");
        assert!(masked.contains("line 2"), "{masked}");
    }

    #[test]
    fn core_scalar_resolution_matches_yaml_1_2_core_schema() {
        use ayaml::{ScalarStyle, Tag};
        assert_eq!(
            resolve_core_scalar("1", ScalarStyle::Plain, None),
            resolve_core_scalar("0x1", ScalarStyle::Plain, None)
        );
        assert_eq!(
            resolve_core_scalar("0", ScalarStyle::Plain, None),
            resolve_core_scalar("-0", ScalarStyle::Plain, None)
        );
        assert_eq!(
            resolve_core_scalar("0123", ScalarStyle::Plain, None),
            Some(CoreScalar::String("0123".into()))
        );
        assert_eq!(
            resolve_core_scalar("0123", ScalarStyle::Plain, Some(&Tag::core("int"))),
            Some(CoreScalar::Uint(123))
        );
        assert_eq!(
            resolve_core_scalar("abc", ScalarStyle::Plain, Some(&Tag::core("int"))),
            None
        );
        assert_eq!(
            resolve_core_scalar("foo", ScalarStyle::Plain, Some(&Tag::local("Ref"))),
            None
        );
        assert_eq!(
            resolve_scalar_payload("1", ScalarStyle::Plain, Some(&Tag::local(""))),
            Some(CoreScalar::String("1".into()))
        );
        assert_eq!(
            resolve_scalar_payload("foo", ScalarStyle::Plain, Some(&Tag::local("Ref"))),
            Some(CoreScalar::String("foo".into()))
        );
        assert_eq!(
            resolve_scalar_payload("abc", ScalarStyle::Plain, Some(&Tag::core("int"))),
            None
        );
    }

    #[test]
    fn duplicate_key_relocation_distinguishes_types_tags_escapes_and_redefined_anchors() {
        let loc = |src: &str| {
            let err = ayaml_from_str::<Value>(src).unwrap_err();
            let loc = err
                .location()
                .unwrap_or_else(|| panic!("no location: {err}"));
            (loc.line(), loc.column())
        };

        // Typed vs quoted keys must not cross-match onto the first occurrence.
        assert_eq!(loc("\"1\": a\n1: b\n1: c\n"), (3, 1));
        assert_eq!(loc("1: a\n\"1\": b\n\"1\": c\n"), (3, 1));
        assert_eq!(loc("\"null\": a\nnull: b\nnull: c\n"), (3, 1));
        assert_eq!(loc("null: a\n\"null\": b\n\"null\": c\n"), (3, 1));
        assert_eq!(loc("\"true\": a\ntrue: b\ntrue: c\n"), (3, 1));
        assert_eq!(loc(": a\n\"\": b\n\"\": c\n"), (3, 1));
        assert_eq!(loc("!t a: 0\na: 1\na: 2\n"), (3, 1));

        // Core-schema equivalents and tagged duplicates relocate to line 2.
        assert_eq!(loc("~: 1\n~: 2\n"), (2, 1));
        assert_eq!(loc("null: 1\n~: 2\n"), (2, 1));
        assert_eq!(loc("true: 1\nTRUE: 2\n"), (2, 1));
        assert_eq!(loc("0x1: 1\n1: 2\n"), (2, 1));
        assert_eq!(loc("1.0: 1\n1e0: 2\n"), (2, 1));
        // Tagged scalar `Event::Scalar` span starts at the scalar `a` (col 4 after `!t `).
        assert_eq!(loc("!t a: 1\n!t a: 2\n"), (2, 4));

        // Keys > 63 chars (truncated with `…` in ayaml error messages).
        let long_key = "k".repeat(80);
        let long_doc = format!("{long_key}: 1\n{long_key}: 2\n");
        assert_eq!(loc(&long_doc), (2, 1));

        // Escaped control chars (`\0`, `\u{7f}`) and special bracket parent keys.
        assert_eq!(loc("\"a\\0b\": 1\n\"a\\0b\": 2\n"), (2, 1));
        assert_eq!(loc("\"a\\x7fb\": 1\n\"a\\x7fb\": 2\n"), (2, 1));
        assert_eq!(loc("\"\":\n  a: 1\n  a: 2\n"), (3, 3));
        assert_eq!(loc("\"a]b\":\n  x: 1\n  x: 2\n"), (3, 3));
        assert_eq!(loc("\"a\\\"b\":\n  x: 1\n  x: 2\n"), (3, 3));
        assert_eq!(
            loc("'duplicate entry with key \"x\"':\n  a: 1\n  a: 2\n"),
            (3, 3)
        );

        // `[?]` collection-key placeholder must NOT relocate into an unrelated `"?"` map.
        let coll_key = "[1, 2]:\n  a: 1\n  a: 2\n\"?\":\n  a: 1\n  a: 2\n";
        let (coll_line, _) = loc(coll_key);
        assert!(
            coll_line <= 3,
            "must not relocate `[?]` duplicate into unrelated `\"?\"` map at line {coll_line}"
        );

        // Redefined collection anchor `&k []` evicts earlier scalar anchor `&k a`.
        let redefined = "x: &k a\ny: &k []\n*k: 1\na: 2\na: 3\n";
        assert_eq!(loc(redefined), (5, 1));

        // Coerced `vars:` key (`1` vs `"1"`) still relocates to the repeated entry.
        let vars_cfg = "version: 1\nvars:\n  1: a\n  \"1\": b\nrules: []\n";
        let err = ayaml_from_str::<crate::Config>(vars_cfg).unwrap_err();
        let vars_loc = err.location().unwrap();
        assert_eq!((vars_loc.line(), vars_loc.column()), (4, 3), "{err}");
    }
}
