//! Shared structured / line / regex extraction for the
//! manifest-driven cross-file rules (`registry_paths_resolve`,
//! `cross_file`, `file_graph`) and core-side predicates. One place so
//! the one-of decode (`serde_yaml` can't decode an externally-tagged
//! enum from a `{ key: value }` map; an untagged enum can't tell the
//! structured `JSONPath` string variants apart) and the non-literal skip
//! can't drift between consumers.

use std::fmt;
use std::ops::Range;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json_path::JsonPath;

use crate::structured_format::Format;

/// Runtime extraction mode, resolved from [`ExtractSpec`].
#[derive(Clone)]
pub enum Extract {
    /// Structured-query (RFC 9535 `JSONPath` over the parsed tree) against any
    /// [`Format`]. The query is compiled once at config load, not once per file.
    Structured(StructuredExtract),
    /// One path per non-blank, non-comment physical line (LF, CRLF, or CR).
    Lines(LinesOpts),
    /// Capture group 1 of each match is the value.
    Regex(String),
    /// The whole file content as a single value.
    WholeFile,
}

#[derive(Clone)]
pub struct StructuredExtract {
    pub format: Format,
    pub query: String,
    pub path: JsonPath,
}

impl fmt::Debug for Extract {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Structured(extract) => f
                .debug_tuple("Structured")
                .field(&extract.format)
                .field(&extract.query)
                .finish(),
            Self::Lines(options) => f.debug_tuple("Lines").field(options).finish(),
            Self::Regex(pattern) => f.debug_tuple("Regex").field(pattern).finish(),
            Self::WholeFile => f.write_str("WholeFile"),
        }
    }
}

impl fmt::Debug for StructuredExtract {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StructuredExtract")
            .field("format", &self.format)
            .field("query", &self.query)
            .finish_non_exhaustive()
    }
}

impl Extract {
    pub fn structured(
        format: Format,
        query: impl Into<String>,
    ) -> std::result::Result<Self, String> {
        let query = query.into();
        let path = JsonPath::parse(&query).map_err(|e| format!("bad JSONPath {query:?}: {e}"))?;
        Ok(Self::Structured(StructuredExtract {
            format,
            query,
            path,
        }))
    }

    /// Whether values produced by this extractor come from a format that is
    /// secret-bearing by default.
    #[must_use]
    pub fn redacts_by_default(&self) -> bool {
        matches!(self, Self::Structured(extract) if extract.format.redacts_by_default())
    }
}

/// Exactly one of: toml/json/yaml/xml/dotenv/properties/ini/hcl (RFC 9535 `JSONPath` string), lines (object;
/// optional `comment` prefix, default `#`), regex (string; capture group 1 is
/// the value), `whole_file` (object `{}`; the entire file content as one value,
/// for byte-level `cross_file` comparison; the non-literal skip does not apply).
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "extract_spec", extend("minProperties" = 1, "maxProperties" = 1))]
pub struct ExtractSpec {
    #[serde(default)]
    toml: Option<String>,
    #[serde(default)]
    json: Option<String>,
    #[serde(default)]
    yaml: Option<String>,
    #[serde(default)]
    xml: Option<String>,
    #[serde(default)]
    dotenv: Option<String>,
    #[serde(default)]
    properties: Option<String>,
    #[serde(default)]
    ini: Option<String>,
    #[serde(default)]
    hcl: Option<String>,
    #[serde(default)]
    lines: Option<LinesOpts>,
    #[serde(default)]
    regex: Option<String>,
    #[serde(default)]
    whole_file: Option<WholeFileOpts>,
}

impl ExtractSpec {
    pub fn resolve(self) -> std::result::Result<Extract, String> {
        let set: Vec<&str> = [
            ("toml", self.toml.is_some()),
            ("json", self.json.is_some()),
            ("yaml", self.yaml.is_some()),
            ("xml", self.xml.is_some()),
            ("dotenv", self.dotenv.is_some()),
            ("properties", self.properties.is_some()),
            ("ini", self.ini.is_some()),
            ("hcl", self.hcl.is_some()),
            ("lines", self.lines.is_some()),
            ("regex", self.regex.is_some()),
            ("whole_file", self.whole_file.is_some()),
        ]
        .into_iter()
        .filter_map(|(n, on)| on.then_some(n))
        .collect();
        match set.as_slice() {
            [] => Err(
                "`extract` must set exactly one of toml/json/yaml/xml/dotenv/properties/ini/hcl/lines/regex/whole_file (none set)"
                    .to_string(),
            ),
            [_] => {
                if let Some(q) = self.toml {
                    Extract::structured(Format::Toml, q)
                } else if let Some(q) = self.json {
                    Extract::structured(Format::Json, q)
                } else if let Some(q) = self.yaml {
                    Extract::structured(Format::Yaml, q)
                } else if let Some(q) = self.xml {
                    Extract::structured(Format::Xml, q)
                } else if let Some(q) = self.dotenv {
                    Extract::structured(Format::Dotenv, q)
                } else if let Some(q) = self.properties {
                    Extract::structured(Format::Properties, q)
                } else if let Some(q) = self.ini {
                    Extract::structured(Format::Ini, q)
                } else if let Some(q) = self.hcl {
                    Extract::structured(Format::Hcl, q)
                } else if let Some(o) = self.lines {
                    Ok(Extract::Lines(o))
                } else if let Some(q) = self.regex {
                    Ok(Extract::Regex(q))
                } else {
                    Ok(Extract::WholeFile)
                }
            }
            many => Err(format!(
                "`extract` must set exactly one of toml/json/yaml/xml/dotenv/properties/ini/hcl/lines/regex/whole_file (got {})",
                many.join(", ")
            )),
        }
    }
}

impl From<Extract> for ExtractSpec {
    fn from(e: Extract) -> Self {
        let mut s = ExtractSpec::default();
        match e {
            Extract::Structured(extract) => match extract.format {
                Format::Toml => s.toml = Some(extract.query),
                Format::Json => s.json = Some(extract.query),
                Format::Yaml => s.yaml = Some(extract.query),
                Format::Xml => s.xml = Some(extract.query),
                Format::Dotenv => s.dotenv = Some(extract.query),
                Format::Properties => s.properties = Some(extract.query),
                Format::Ini => s.ini = Some(extract.query),
                Format::Hcl => s.hcl = Some(extract.query),
            },
            Extract::Lines(o) => s.lines = Some(o),
            Extract::Regex(q) => s.regex = Some(q),
            Extract::WholeFile => s.whole_file = Some(WholeFileOpts::default()),
        }
        s
    }
}

/// `whole_file:` carries no options today (an empty `{}` map, like a
/// marker); kept as a struct so options can be added without a
/// breaking change.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WholeFileOpts {}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LinesOpts {
    /// Lines starting with this (after trim) are skipped.
    #[serde(default = "default_comment")]
    pub(crate) comment: String,
}

fn default_comment() -> String {
    "#".to_string()
}

// `#[serde(default = "default_comment")]` only fires on the
// deserialize path; `LinesOpts::default()` (used by the
// `Lines(#[serde(default)] …)` variant and tests) needs the
// same `#` default, so derive can't be used here.
impl Default for LinesOpts {
    fn default() -> Self {
        Self {
            comment: default_comment(),
        }
    }
}

/// True when `entry` is a *computed* value (interpolation /
/// concatenation), which the caller skips rather than checks.
/// Genuine markers only: shell/Nix `${var}` and `$(cmd)`,
/// mustache/jinja `{{ … }}`, string concatenation `"a" + b`.
/// A bare `$`, backtick, or `(.` is legal in a real filename, so
/// it is **not** treated as non-literal - over-matching those
/// silently dropped real literal paths (a false negative; v0.10
/// post-audit P2). The skip never fails the rule and is
/// intentionally silent; visibly surfacing skipped entries is a
/// tracked v0.11 item (`alint check` has no `--explain` /
/// informational-finding channel).
pub fn is_non_literal(entry: &str) -> bool {
    entry.contains("${") || entry.contains("$(") || entry.contains("{{") || entry.contains("+ ")
}

/// A structured extraction result that could not satisfy a string-only
/// consumer. Only the JSON type is retained; repository-controlled values are
/// never copied into diagnostics or baseline identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NonStringValueKind {
    Null,
    Boolean,
    Number,
    Array,
    Object,
}

impl NonStringValueKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Boolean => "boolean",
            Self::Number => "number",
            Self::Array => "array",
            Self::Object => "object",
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ExtractedValues {
    pub strings: Vec<String>,
    pub non_strings: Vec<NonStringValueKind>,
}

/// One component in the path of an object containing a structured entry.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum EntryPathSegment {
    Key(String),
    Index(usize),
}

/// A half-open byte range in the original UTF-8 source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ByteSpan {
    pub start: usize,
    pub end: usize,
}

impl ByteSpan {
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    #[must_use]
    pub const fn len(self) -> usize {
        self.end - self.start
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    #[must_use]
    pub fn as_range(self) -> Range<usize> {
        self.start..self.end
    }
}

/// One lexical object member, retained in source order. Duplicate keys are
/// distinct rows rather than entries in a last-wins map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StructuredEntryRow {
    pub parent: Vec<EntryPathSegment>,
    pub key: String,
    pub key_span: ByteSpan,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_span: Option<ByteSpan>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EntryRowsError {
    #[error("duplicate-aware entry extraction is not supported for {0:?}")]
    UnsupportedFormat(Format),
    #[error("structured entry extraction failed: {0}")]
    Parse(String),
    #[error("entry extraction exceeded the {limit}-row safety limit")]
    Limit { limit: usize },
}

/// Extract source-ordered object-member rows without collapsing duplicate keys.
///
/// TOML, XML, and HCL deliberately return [`EntryRowsError::UnsupportedFormat`]
/// until they have a lossless traversal. Returning an empty successful result
/// would falsely claim that no duplicate or out-of-order entry exists.
pub fn extract_entry_rows(
    format: Format,
    text: &str,
) -> std::result::Result<Vec<StructuredEntryRow>, EntryRowsError> {
    extract_entry_rows_with(format, text, false, None)
}

/// Like [`extract_entry_rows`], stopping before a row would exceed
/// `max_rows`. The limit is enforced while the extractor builds its vector, so
/// a crafted document cannot allocate an unbounded row set before the caller
/// checks it.
pub fn extract_entry_rows_bounded(
    format: Format,
    text: &str,
    max_rows: usize,
) -> std::result::Result<Vec<StructuredEntryRow>, EntryRowsError> {
    extract_entry_rows_with(format, text, false, Some(max_rows))
}

/// Like [`extract_entry_rows`], but a YAML alias or plain merge key is a parse
/// failure instead of being dropped or emitted as a literal `<<` key.
///
/// Key parity cannot compare unexpanded aliases: treating the alias as a
/// missing value would report false drift. The relation adapter uses this
/// strict form.
pub fn extract_entry_rows_strict_yaml(
    format: Format,
    text: &str,
) -> std::result::Result<Vec<StructuredEntryRow>, EntryRowsError> {
    extract_entry_rows_with(format, text, true, None)
}

/// [`extract_entry_rows_strict_yaml`] with a bound enforced during extraction.
pub fn extract_entry_rows_strict_yaml_bounded(
    format: Format,
    text: &str,
    max_rows: usize,
) -> std::result::Result<Vec<StructuredEntryRow>, EntryRowsError> {
    extract_entry_rows_with(format, text, true, Some(max_rows))
}

fn extract_entry_rows_with(
    format: Format,
    text: &str,
    reject_yaml_features: bool,
    max_rows: Option<usize>,
) -> std::result::Result<Vec<StructuredEntryRow>, EntryRowsError> {
    match format {
        Format::Json => json_entry_rows(text, max_rows),
        Format::Yaml => yaml_entry_rows(text, reject_yaml_features, max_rows),
        Format::Dotenv => dotenv_entry_rows(text, max_rows),
        Format::Ini => ini_entry_rows(text, max_rows),
        Format::Properties => properties_entry_rows(text, max_rows),
        Format::Toml | Format::Xml | Format::Hcl => Err(EntryRowsError::UnsupportedFormat(format)),
    }
}

pub(crate) fn json_parse_options() -> jsonc_parser::ParseOptions {
    jsonc_parser::ParseOptions {
        allow_comments: true,
        allow_trailing_commas: true,
        allow_loose_object_property_names: false,
        allow_missing_commas: false,
        allow_single_quoted_strings: false,
        allow_hexadecimal_numbers: false,
        allow_unary_plus_numbers: false,
        allow_bare_decimal_point_numbers: false,
        allow_non_finite_numbers: false,
        allow_extended_string_escapes: false,
    }
}

fn entry_capacity(
    rows: &[StructuredEntryRow],
    max_rows: Option<usize>,
) -> std::result::Result<(), EntryRowsError> {
    if let Some(limit) = max_rows
        && rows.len() >= limit
    {
        return Err(EntryRowsError::Limit { limit });
    }
    Ok(())
}

fn json_entry_rows(
    text: &str,
    max_rows: Option<usize>,
) -> std::result::Result<Vec<StructuredEntryRow>, EntryRowsError> {
    use jsonc_parser::{CollectOptions, CommentCollectionStrategy, parse_to_ast};

    // Reuse the check-side size/depth/dialect guards before the lossless pass.
    Format::Json.parse(text).map_err(EntryRowsError::Parse)?;
    let stripped = text.trim_start_matches('\u{feff}');
    let offset = text.len() - stripped.len();
    let ast = parse_to_ast(
        stripped,
        &CollectOptions {
            comments: CommentCollectionStrategy::Off,
            tokens: false,
        },
        &json_parse_options(),
    )
    .map_err(|error| EntryRowsError::Parse(error.to_string()))?;
    let mut rows = Vec::new();
    if let Some(value) = ast.value.as_ref() {
        collect_json_rows(value, &mut Vec::new(), offset, &mut rows, max_rows)?;
    }
    Ok(rows)
}

fn collect_json_rows(
    value: &jsonc_parser::ast::Value<'_>,
    parent: &mut Vec<EntryPathSegment>,
    offset: usize,
    rows: &mut Vec<StructuredEntryRow>,
    max_rows: Option<usize>,
) -> std::result::Result<(), EntryRowsError> {
    use jsonc_parser::ast::{ObjectPropName, Value};
    use jsonc_parser::common::Ranged as _;

    match value {
        Value::Object(object) => {
            for property in &object.properties {
                let (key, range) = match &property.name {
                    ObjectPropName::String(literal) => (literal.value.to_string(), literal.range()),
                    ObjectPropName::Word(literal) => (literal.value.to_string(), literal.range()),
                };
                entry_capacity(rows, max_rows)?;
                let value_range = property.value.range();
                rows.push(StructuredEntryRow {
                    parent: parent.clone(),
                    key: key.clone(),
                    key_span: ByteSpan::new(range.start + offset, range.end + offset),
                    value_span: Some(ByteSpan::new(
                        value_range.start + offset,
                        value_range.end + offset,
                    )),
                });
                parent.push(EntryPathSegment::Key(key));
                collect_json_rows(&property.value, parent, offset, rows, max_rows)?;
                parent.pop();
            }
        }
        Value::Array(array) => {
            for (index, element) in array.elements.iter().enumerate() {
                parent.push(EntryPathSegment::Index(index));
                collect_json_rows(element, parent, offset, rows, max_rows)?;
                parent.pop();
            }
        }
        Value::StringLit(_)
        | Value::NumberLit(_)
        | Value::BooleanLit(_)
        | Value::NullKeyword(_) => {}
    }
    Ok(())
}

fn dotenv_entry_rows(
    text: &str,
    max_rows: Option<usize>,
) -> std::result::Result<Vec<StructuredEntryRow>, EntryRowsError> {
    let (document, offset) = crate::dotenv::parse_document(text).map_err(EntryRowsError::Parse)?;
    let mut rows = Vec::new();
    for entry in document.entries() {
        entry_capacity(&rows, max_rows)?;
        let key = entry.key().to_string();
        let key_span = entry.key_span();
        rows.push(StructuredEntryRow {
            parent: Vec::new(),
            key,
            key_span: ByteSpan::new(key_span.start + offset, key_span.end + offset),
            value_span: entry
                .value_span()
                .map(|span| ByteSpan::new(span.start + offset, span.end + offset)),
        });
    }
    Ok(rows)
}

const fn next_line_start(newline: usize, len: usize) -> usize {
    if newline < len { newline + 1 } else { len }
}

fn ini_entry_rows(
    text: &str,
    max_rows: Option<usize>,
) -> std::result::Result<Vec<StructuredEntryRow>, EntryRowsError> {
    Format::Ini.parse(text).map_err(EntryRowsError::Parse)?;
    let stripped = text.trim_start_matches('\u{feff}');
    let base = text.len() - stripped.len();
    let bytes = stripped.as_bytes();
    let mut rows: Vec<StructuredEntryRow> = Vec::new();
    let mut section: Option<String> = None;
    let mut current_row: Option<usize> = None;
    let mut current_indent = 0;
    let mut at = 0;
    while at < bytes.len() {
        let newline = bytes[at..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |index| at + index);
        let content_end = if newline > at && bytes[newline - 1] == b'\r' {
            newline - 1
        } else {
            newline
        };
        let raw = &stripped[at..content_end];
        let indent = raw
            .bytes()
            .take_while(|byte| matches!(byte, b' ' | b'\t'))
            .count();
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
            at = next_line_start(newline, bytes.len());
            continue;
        }
        if let Some(row) = current_row
            && indent > current_indent
        {
            // A continued INI value is discontiguous after trimming indentation;
            // retaining no span is safer than claiming a partial value token.
            rows[row].value_span = None;
            at = next_line_start(newline, bytes.len());
            continue;
        }
        let trim_start = raw.len() - raw.trim_start().len();
        if let Some(rest) = trimmed.strip_prefix('[') {
            let name = rest
                .strip_suffix(']')
                .expect("validated INI section")
                .trim();
            section = Some(name.to_string());
            current_row = None;
        } else {
            let separator = trimmed.find(['=', ':']).expect("validated INI assignment");
            let before = &trimmed[..separator];
            let key_lead = before.len() - before.trim_start().len();
            let key = before.trim();
            let after = &trimmed[separator + 1..];
            let value_lead = after.len() - after.trim_start().len();
            let value = after.trim();
            let key_start = base + at + trim_start + key_lead;
            let value_start = base + at + trim_start + separator + 1 + value_lead;
            entry_capacity(&rows, max_rows)?;
            rows.push(StructuredEntryRow {
                parent: section.iter().cloned().map(EntryPathSegment::Key).collect(),
                key: key.to_string(),
                key_span: ByteSpan::new(key_start, key_start + key.len()),
                value_span: Some(ByteSpan::new(value_start, value_start + value.len())),
            });
            current_row = Some(rows.len() - 1);
            current_indent = indent;
        }
        at = next_line_start(newline, bytes.len());
    }
    Ok(rows)
}

fn properties_entry_rows(
    text: &str,
    max_rows: Option<usize>,
) -> std::result::Result<Vec<StructuredEntryRow>, EntryRowsError> {
    let stripped = text.trim_start_matches('\u{feff}');
    let base = text.len() - stripped.len();
    let decoded_keys = properties_decoded_keys(stripped, max_rows)?;
    if decoded_keys.is_empty() {
        return Ok(Vec::new());
    }
    let expected_rows = decoded_keys.len();

    let bytes = stripped.as_bytes();
    let mut rows = Vec::with_capacity(expected_rows);
    let mut decoded = decoded_keys.into_iter();
    let mut at = 0;
    while at < bytes.len() {
        let (physical_end, physical_next) = properties_physical_line(bytes, at);
        let first_content = bytes[at..physical_end]
            .iter()
            .position(|byte| !matches!(byte, b' ' | b'\t' | 0x0c))
            .map(|indent| at + indent);
        if first_content.is_none_or(|cursor| matches!(bytes[cursor], b'#' | b'!')) {
            // Java-properties comments are always one physical line. In
            // particular, a trailing backslash in a comment is not a
            // continuation marker.
            at = physical_next;
            continue;
        }
        let (logical_end, next) = properties_logical_line(bytes, at);
        let Some(logical_end) = logical_end else {
            // The dependency discards a logical line whose final physical line
            // ends in an unpaired continuation backslash.
            at = next;
            continue;
        };
        let mut cursor = at;
        while cursor < logical_end && matches!(bytes[cursor], b' ' | b'\t' | 0x0c) {
            cursor += 1;
        }
        let key_start = cursor;
        while cursor < logical_end {
            match bytes[cursor] {
                b'\\' if cursor + 1 < logical_end => {
                    cursor += 1;
                    match bytes[cursor] {
                        b'\r' => {
                            cursor += 1;
                            if cursor < logical_end && bytes[cursor] == b'\n' {
                                cursor += 1;
                            }
                            while cursor < logical_end
                                && matches!(bytes[cursor], b' ' | b'\t' | 0x0c)
                            {
                                cursor += 1;
                            }
                        }
                        b'\n' => {
                            cursor += 1;
                            while cursor < logical_end
                                && matches!(bytes[cursor], b' ' | b'\t' | 0x0c)
                            {
                                cursor += 1;
                            }
                        }
                        _ => cursor += 1,
                    }
                }
                b'=' | b':' | b' ' | b'\t' | 0x0c => break,
                _ => cursor += 1,
            }
        }
        let key_end = cursor;
        while cursor < logical_end && matches!(bytes[cursor], b' ' | b'\t' | 0x0c) {
            cursor += 1;
        }
        if cursor < logical_end && matches!(bytes[cursor], b'=' | b':') {
            cursor += 1;
        }
        while cursor < logical_end && matches!(bytes[cursor], b' ' | b'\t' | 0x0c) {
            cursor += 1;
        }
        let key = decoded.next().ok_or_else(|| {
            EntryRowsError::Parse("properties entry/span count diverged".to_string())
        })?;
        entry_capacity(&rows, max_rows)?;
        rows.push(StructuredEntryRow {
            parent: Vec::new(),
            key,
            key_span: ByteSpan::new(base + key_start, base + key_end),
            value_span: Some(ByteSpan::new(base + cursor, base + logical_end)),
        });
        at = next;
    }
    if decoded.next().is_some() || rows.len() != expected_rows {
        return Err(EntryRowsError::Parse(
            "properties entry/span count diverged".to_string(),
        ));
    }
    Ok(rows)
}
fn properties_decoded_keys(
    text: &str,
    max_rows: Option<usize>,
) -> std::result::Result<Vec<String>, EntryRowsError> {
    if text.len() > crate::structured_format::MAX_STRUCTURED_BYTES {
        return Err(EntryRowsError::Parse(format!(
            "input exceeds the maximum supported size for a structured document ({} bytes)",
            crate::structured_format::MAX_STRUCTURED_BYTES
        )));
    }
    let mut keys = Vec::new();
    let mut limit_hit = false;
    java_properties::PropertiesIter::new_with_encoding(text.as_bytes(), encoding_rs::UTF_8)
        .read_into(|key, _| {
            if let Some(limit) = max_rows
                && keys.len() >= limit
            {
                limit_hit = true;
                return;
            }
            keys.push(key);
        })
        .map_err(|error| EntryRowsError::Parse(error.to_string()))?;
    if limit_hit {
        return Err(EntryRowsError::Limit {
            limit: max_rows.unwrap_or_default(),
        });
    }
    Ok(keys)
}

fn properties_logical_line(bytes: &[u8], start: usize) -> (Option<usize>, usize) {
    let mut line_start = start;
    loop {
        let (content_end, next) = properties_physical_line(bytes, line_start);
        let trailing_backslashes = bytes[line_start..content_end]
            .iter()
            .rev()
            .take_while(|byte| **byte == b'\\')
            .count();
        if trailing_backslashes % 2 == 0 {
            return (Some(content_end), next);
        }
        if next == bytes.len() {
            // `java-properties` synthesizes one final empty physical line
            // after a real line terminator, so only an unterminated odd
            // backslash is an incomplete logical line.
            return if content_end < bytes.len() {
                (Some(content_end), next)
            } else {
                (None, next)
            };
        }
        line_start = next;
    }
}

fn properties_physical_line(bytes: &[u8], start: usize) -> (usize, usize) {
    let mut end = start;
    while end < bytes.len() && !matches!(bytes[end], b'\r' | b'\n') {
        end += 1;
    }
    let next = if bytes.get(end) == Some(&b'\r') && bytes.get(end + 1) == Some(&b'\n') {
        end + 2
    } else if end < bytes.len() {
        end + 1
    } else {
        end
    };
    (end, next)
}

fn yaml_entry_rows(
    text: &str,
    reject_yaml_features: bool,
    max_rows: Option<usize>,
) -> std::result::Result<Vec<StructuredEntryRow>, EntryRowsError> {
    use saphyr_parser::{Event, Parser};

    let stripped = text.trim_start_matches('\u{feff}');
    let offset = text.len() - stripped.len();
    if stripped.len() > crate::structured_format::MAX_STRUCTURED_BYTES {
        return Err(EntryRowsError::Parse(format!(
            "input exceeds the maximum supported size for a structured document ({} bytes)",
            crate::structured_format::MAX_STRUCTURED_BYTES
        )));
    }
    // Keep the lexical flow guard ahead of saphyr's scanner: deeply nested flow
    // collections are super-linear before the parser can emit their starts.
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
    // traversing or materializing the parser result.
    if !yaml_guards.expansion_within_limit(stripped) {
        return Err(EntryRowsError::Parse(
            "YAML alias expansion exceeds the maximum supported size".to_string(),
        ));
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
    let mut rows = Vec::new();
    let mut parent = Vec::new();
    if index < events.len() {
        parse_yaml_node(
            &events,
            &mut index,
            &byte_offsets,
            offset,
            &mut parent,
            &mut rows,
            true,
            stripped,
            reject_yaml_features,
            max_rows,
        )?;
    }
    Ok(rows)
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
    style: Option<saphyr_parser::ScalarStyle>,
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn parse_yaml_node(
    events: &[(saphyr_parser::Event<'_>, saphyr_parser::Span)],
    index: &mut usize,
    byte_offsets: &YamlByteOffsets,
    offset: usize,
    parent: &mut Vec<EntryPathSegment>,
    rows: &mut Vec<StructuredEntryRow>,
    emit: bool,
    text: &str,
    reject_yaml_features: bool,
    max_rows: Option<usize>,
) -> std::result::Result<YamlNode, EntryRowsError> {
    use saphyr_parser::Event;

    let Some((event, event_span)) = events.get(*index) else {
        return Err(EntryRowsError::Parse(
            "unexpected end of YAML event stream".to_string(),
        ));
    };
    match event {
        Event::Scalar(value, style, ..) => {
            *index += 1;
            Ok(YamlNode {
                scalar: Some(value.to_string()),
                span: yaml_scalar_span(*event_span, *style, byte_offsets, offset, text),
                style: Some(*style),
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
                style: None,
            })
        }
        Event::SequenceStart(..) => {
            let start = *event_span;
            *index += 1;
            let mut item = 0;
            while !matches!(events.get(*index), Some((Event::SequenceEnd, _))) {
                parent.push(EntryPathSegment::Index(item));
                parse_yaml_node(
                    events,
                    index,
                    byte_offsets,
                    offset,
                    parent,
                    rows,
                    emit,
                    text,
                    reject_yaml_features,
                    max_rows,
                )?;
                parent.pop();
                item += 1;
            }
            let end = events[*index].1;
            *index += 1;
            Ok(YamlNode {
                scalar: None,
                span: yaml_flow_container_span(start, end, byte_offsets, offset, text, b'[', b']'),
                style: None,
            })
        }
        Event::MappingStart(..) => {
            let start = *event_span;
            *index += 1;
            while !matches!(events.get(*index), Some((Event::MappingEnd, _))) {
                let key = parse_yaml_node(
                    events,
                    index,
                    byte_offsets,
                    offset,
                    parent,
                    rows,
                    false,
                    text,
                    reject_yaml_features,
                    max_rows,
                )?;
                if let Some(key_text) = key.scalar {
                    if reject_yaml_features
                        && key_text == "<<"
                        && key.style == Some(saphyr_parser::ScalarStyle::Plain)
                    {
                        return Err(EntryRowsError::Parse(
                            "YAML merge keys are not supported for key extraction".to_string(),
                        ));
                    }
                    if emit {
                        entry_capacity(rows, max_rows)?;
                    }
                    let row_index = if emit {
                        key.span.map(|key_span| {
                            rows.push(StructuredEntryRow {
                                parent: parent.clone(),
                                key: key_text.clone(),
                                key_span,
                                value_span: None,
                            });
                            rows.len() - 1
                        })
                    } else {
                        None
                    };
                    parent.push(EntryPathSegment::Key(key_text));
                    let value = parse_yaml_node(
                        events,
                        index,
                        byte_offsets,
                        offset,
                        parent,
                        rows,
                        emit,
                        text,
                        reject_yaml_features,
                        max_rows,
                    )?;
                    parent.pop();
                    if let Some(row_index) = row_index {
                        rows[row_index].value_span = value.span;
                    }
                } else {
                    parse_yaml_node(
                        events,
                        index,
                        byte_offsets,
                        offset,
                        parent,
                        rows,
                        emit,
                        text,
                        reject_yaml_features,
                        max_rows,
                    )?;
                }
            }
            let end = events[*index].1;
            *index += 1;
            Ok(YamlNode {
                scalar: None,
                span: yaml_flow_container_span(start, end, byte_offsets, offset, text, b'{', b'}'),
                style: None,
            })
        }
        _ => Err(EntryRowsError::Parse(format!(
            "unexpected YAML event {event:?}"
        ))),
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

/// Extract values over `text`. Structured modes retain non-string match types
/// separately so callers can report a baselinable policy mismatch instead of
/// silently dropping it or misclassifying it as a parse failure.
pub fn extract_values(
    extract: &Extract,
    text: &str,
) -> std::result::Result<ExtractedValues, String> {
    extract_values_with_limit(extract, text, None).map_err(|error| error.to_string())
}

/// Extract at most `max_values` values, failing before allocating the value
/// that would exceed the bound. Callers that turn every extracted value into a
/// finding use this to keep a bounded input from amplifying into unbounded
/// report state.
pub fn extract_values_bounded(
    extract: &Extract,
    text: &str,
    max_values: usize,
) -> std::result::Result<ExtractedValues, String> {
    extract_values_with_limit(extract, text, Some(max_values)).map_err(|error| error.to_string())
}

/// [`extract_values_bounded`] with the failure class preserved, for callers
/// that must distinguish a parse failure from a limit hit (the relation
/// adapters do; `AdapterError` carries both).
pub(crate) fn extract_values_bounded_typed(
    extract: &Extract,
    text: &str,
    max_values: usize,
) -> std::result::Result<ExtractedValues, ExtractFailure> {
    extract_values_with_limit(extract, text, Some(max_values))
}

/// Why a bounded extraction could not produce values.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ExtractFailure {
    #[error("{0}")]
    Parse(String),
    #[error("extract selected more than the {limit}-value safety limit")]
    Limit { limit: usize, reached: usize },
}

fn extract_values_with_limit(
    extract: &Extract,
    text: &str,
    max_values: Option<usize>,
) -> std::result::Result<ExtractedValues, ExtractFailure> {
    let values = match extract {
        Extract::Structured(extract) => structured(extract, text, max_values)?,
        Extract::Lines(opts) => {
            let mut values = ExtractedValues::default();
            for line in crate::physical_lines(text)
                .map(|(_, line)| line.trim())
                .filter(|line| {
                    !line.is_empty()
                        && (opts.comment.is_empty() || !line.starts_with(opts.comment.as_str()))
                })
            {
                ensure_extract_capacity(&values, max_values)?;
                values.strings.push(line.to_string());
            }
            values
        }
        Extract::Regex(pattern) => {
            let regex = Regex::new(pattern)
                .map_err(|error| ExtractFailure::Parse(format!("bad regex: {error}")))?;
            let mut values = ExtractedValues::default();
            for matched in regex
                .captures_iter(text)
                .filter_map(|captures| captures.get(1))
            {
                ensure_extract_capacity(&values, max_values)?;
                values.strings.push(matched.as_str().to_string());
            }
            values
        }
        Extract::WholeFile => {
            if max_values == Some(0) {
                return Err(extract_limit_error(0));
            }
            ExtractedValues {
                strings: vec![text.to_string()],
                non_strings: Vec::new(),
            }
        }
    };
    if max_values.is_some_and(|max| values.strings.len() + values.non_strings.len() > max) {
        return Err(extract_limit_error(max_values.unwrap_or_default()));
    }
    Ok(values)
}

fn ensure_extract_capacity(
    values: &ExtractedValues,
    max_values: Option<usize>,
) -> std::result::Result<(), ExtractFailure> {
    if max_values.is_some_and(|max| values.strings.len() + values.non_strings.len() >= max) {
        return Err(extract_limit_error(max_values.unwrap_or_default()));
    }
    Ok(())
}

fn extract_limit_error(max_values: usize) -> ExtractFailure {
    ExtractFailure::Limit {
        limit: max_values,
        reached: max_values.saturating_add(1),
    }
}

fn structured(
    extract: &StructuredExtract,
    text: &str,
    max_values: Option<usize>,
) -> std::result::Result<ExtractedValues, ExtractFailure> {
    let value = extract.format.parse(text).map_err(ExtractFailure::Parse)?;
    let mut values = ExtractedValues::default();
    for value in extract.path.query(&value).iter() {
        ensure_extract_capacity(&values, max_values)?;
        match value {
            serde_json::Value::String(value) => values.strings.push(value.clone()),
            serde_json::Value::Null => values.non_strings.push(NonStringValueKind::Null),
            serde_json::Value::Bool(_) => values.non_strings.push(NonStringValueKind::Boolean),
            serde_json::Value::Number(_) => values.non_strings.push(NonStringValueKind::Number),
            serde_json::Value::Array(_) => values.non_strings.push(NonStringValueKind::Array),
            serde_json::Value::Object(_) => values.non_strings.push(NonStringValueKind::Object),
        }
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::{
        EntryPathSegment, EntryRowsError, Extract, extract_entry_rows, extract_entry_rows_bounded,
        extract_entry_rows_strict_yaml, extract_entry_rows_strict_yaml_bounded,
        extract_values_bounded, is_non_literal,
    };
    use crate::structured_format::Format;

    #[test]
    fn bounded_extraction_fails_before_returning_excess_values() {
        let regex = Extract::Regex(r"(?m)^v=(.*)$".to_string());
        let error = extract_values_bounded(&regex, "v=1\nv=2\nv=3\n", 2).unwrap_err();
        assert!(error.contains("2-value safety limit"), "{error}");

        let structured = Extract::structured(Format::Json, "$.values[*]").unwrap();
        let error =
            extract_values_bounded(&structured, r#"{"values":["1","2","3"]}"#, 2).unwrap_err();
        assert!(error.contains("2-value safety limit"), "{error}");
    }

    #[test]
    fn genuine_interpolation_is_non_literal() {
        for e in [
            "${pkgs.foo}/bin",
            "$(date +%s)/x",
            "{{ pkg }}/lib",
            "crates/a + crates/b",
        ] {
            assert!(is_non_literal(e), "{e:?} must be non-literal");
        }
    }

    #[test]
    fn bare_dollar_backtick_dotparen_are_literal() {
        // v0.10 post-audit P2 regression: all legal in real
        // filenames — must be CHECKED, not silently skipped.
        for e in [
            "foo$bar.rs",
            "weird`name`.txt",
            "a/b (.c)/d",
            "./relative/path",
            "pkg-1.0",
            "crates/serde_json",
        ] {
            assert!(!is_non_literal(e), "{e:?} must be literal");
        }
    }

    #[test]
    fn whole_file_resolves_and_yields_full_text() {
        let spec: super::ExtractSpec =
            yaml_serde::from_str("whole_file: {}").expect("parse whole_file spec");
        let extract = spec.resolve().expect("resolve whole_file");
        assert!(matches!(extract, super::Extract::WholeFile));
        let text = "line one\nline two\n";
        let got = super::extract_values(&extract, text).expect("extract whole file");
        assert_eq!(got.strings, vec![text.to_string()]);
    }

    #[test]
    fn lines_extractor_recognizes_all_physical_line_endings() {
        let extract = super::Extract::Lines(super::LinesOpts::default());
        let got = super::extract_values(&extract, "alpha\r# ignored\r\n beta \ngamma")
            .expect("extract physical lines");
        assert_eq!(got.strings, ["alpha", "beta", "gamma"]);
    }

    #[test]
    fn whole_file_conflicts_with_another_source() {
        let spec: super::ExtractSpec =
            yaml_serde::from_str("whole_file: {}\nregex: '(x)'").expect("parse spec");
        assert!(spec.resolve().is_err(), "two sources must be rejected");
    }

    #[test]
    fn resolve_and_extract_over_xml() {
        // The `xml:` extract mode reuses `Format::Xml.parse` -- an XML leaf comes
        // back as its string value, exactly like the other structured modes.
        let spec: super::ExtractSpec =
            yaml_serde::from_str("xml: \"$.Project.PropertyGroup.Version\"")
                .expect("parse xml spec");
        let extract = spec.resolve().expect("resolve xml");
        assert!(matches!(
            extract,
            super::Extract::Structured(super::StructuredExtract {
                format: crate::structured_format::Format::Xml,
                ..
            })
        ));
        let xml = "<Project><PropertyGroup><Version>1.2.3</Version></PropertyGroup></Project>";
        assert_eq!(
            super::extract_values(&extract, xml)
                .expect("extract xml")
                .strings,
            vec!["1.2.3".to_string()]
        );
    }

    #[test]
    fn extract_spec_covers_every_format() {
        // Parity gate: every `Format::ALL` variant must have a structured extract
        // field, so a format wired into the rule family / schema can't be forgotten
        // here. The field names are lowercase; `label()` is uppercase, so case-fold.
        let schema = serde_json::to_value(schemars::schema_for!(super::ExtractSpec))
            .expect("ExtractSpec schema serializes");
        // schemars may inline the root or emit a $ref to $defs/extract_spec; read
        // properties from whichever carries them, so a schemars upgrade doesn't
        // silently turn this into a false RED.
        let props = schema
            .get("properties")
            .or_else(|| schema.pointer("/$defs/extract_spec/properties"))
            .and_then(|v| v.as_object())
            .expect("ExtractSpec schema exposes a properties object");
        for fmt in crate::structured_format::Format::ALL {
            let name = fmt.label().to_lowercase();
            assert!(
                props.contains_key(&name),
                "ExtractSpec is missing a `{name}` field for Format::{fmt:?} (format parity)"
            );
        }
    }

    #[test]
    fn xml_and_toml_both_set_is_rejected() {
        // The one-of guard still holds with the new `xml` field in the mix.
        let spec: super::ExtractSpec =
            yaml_serde::from_str("xml: \"$.a\"\ntoml: \"$.b\"").expect("parse two-set spec");
        assert!(
            spec.resolve().is_err(),
            "two structured modes must be rejected"
        );
    }

    #[test]
    fn structured_extract_partitions_every_json_value_type_in_one_pass() {
        let extract =
            super::Extract::structured(crate::structured_format::Format::Json, "$[*]").unwrap();
        let got = super::extract_values(
            &extract,
            r#"[null, true, 7, [1], {"secret":"do-not-report"}, "visible"]"#,
        )
        .expect("extract");

        assert_eq!(got.strings, ["visible"]);
        assert_eq!(
            got.non_strings,
            [
                super::NonStringValueKind::Null,
                super::NonStringValueKind::Boolean,
                super::NonStringValueKind::Number,
                super::NonStringValueKind::Array,
                super::NonStringValueKind::Object,
            ]
        );
    }

    #[test]
    fn invalid_structured_query_fails_when_the_extract_is_built() {
        let error = super::Extract::structured(crate::structured_format::Format::Json, "$[")
            .expect_err("invalid JSONPath must fail at configuration build time");
        assert!(error.contains("bad JSONPath"), "{error}");
    }

    #[test]
    fn xml_extract_classifies_non_string_nodes() {
        let extract = super::Extract::structured(
            crate::structured_format::Format::Xml,
            "$.Project.PropertyGroup",
        )
        .unwrap();
        let xml = "<Project><PropertyGroup><Version>1.0</Version></PropertyGroup></Project>";
        let got = super::extract_values(&extract, xml).expect("extract");
        assert_eq!(got.strings, Vec::<String>::new());
        assert_eq!(
            got.non_strings,
            vec![super::NonStringValueKind::Object],
            "an object node is retained as a typed non-string result"
        );
    }

    #[test]
    fn xml_extract_malformed_propagates_error() {
        // A malformed XML target surfaces as an extract error (the consumer turns
        // it into a per-file violation), same as the other structured modes.
        let extract =
            super::Extract::structured(crate::structured_format::Format::Xml, "$.a").unwrap();
        assert!(
            super::extract_values(&extract, "<a><unclosed>").is_err(),
            "malformed XML must surface as an extract error"
        );
    }

    #[test]
    fn xml_element_cardinality_is_data_dependent() {
        // XML has no array type: a single <Ref> is an object, but repeated <Ref>
        // become an array. So a `[*]` wildcard does NOT normalize cardinality --
        // the same query yields different results for one vs many, a documented
        // footgun (docs/rules.md #xml-mapping). Recursive descent (`$..`) is the
        // cardinality-independent idiom a rule should use when it must handle both.
        use crate::structured_format::Format;
        let xv = |q: &str, xml: &str| {
            super::extract_values(&super::Extract::structured(Format::Xml, q).unwrap(), xml)
                .unwrap()
                .strings
        };
        let one = r#"<Project><ItemGroup><Ref Include="x.csproj"/></ItemGroup></Project>"#;
        let many =
            r#"<Project><ItemGroup><Ref Include="a"/><Ref Include="b"/></ItemGroup></Project>"#;

        // The `[*]['@Include']` idiom (as used in the csproj scenarios) silently
        // extracts NOTHING for a single element -- the false-negative footgun ...
        assert!(
            xv("$.Project.ItemGroup.Ref[*]['@Include']", one).is_empty(),
            "single element + [*] is the documented silent-miss footgun"
        );
        // ... but works for two or more.
        assert_eq!(
            xv("$.Project.ItemGroup.Ref[*]['@Include']", many),
            vec!["a", "b"]
        );

        // Recursive descent from the element handles BOTH cardinalities -- the
        // idiom the docs recommend for cardinality-independent XML extraction.
        assert_eq!(
            xv("$.Project.ItemGroup.Ref..['@Include']", one),
            vec!["x.csproj"]
        );
        assert_eq!(
            xv("$.Project.ItemGroup.Ref..['@Include']", many),
            vec!["a", "b"]
        );
    }

    fn span_text(text: &str, span: super::ByteSpan) -> &str {
        &text[span.as_range()]
    }

    #[test]
    fn json_entry_rows_preserve_duplicate_keys_source_order_and_parents() {
        let text = "\u{feff}{\"a\": 1, \"a\": {\"b\": 2}, \"list\": [{\"x\": 3}]}";
        let rows = extract_entry_rows(Format::Json, text).unwrap();

        assert_eq!(
            rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(),
            ["a", "a", "b", "list", "x"]
        );
        assert_eq!(rows[0].parent, []);
        assert_eq!(rows[1].parent, []);
        assert_eq!(rows[2].parent, [EntryPathSegment::Key("a".into())]);
        assert_eq!(
            rows[4].parent,
            [
                EntryPathSegment::Key("list".into()),
                EntryPathSegment::Index(0)
            ]
        );
        assert_eq!(span_text(text, rows[0].key_span), "\"a\"");
        assert_eq!(span_text(text, rows[0].value_span.unwrap()), "1");
        assert_eq!(span_text(text, rows[2].value_span.unwrap()), "2");
    }

    #[test]
    fn yaml_entry_rows_preserve_duplicates_sequences_and_scalar_spans() {
        let text = "\u{feff}a: one\na: two\nlist:\n  - x: three\n";
        let rows = extract_entry_rows(Format::Yaml, text).unwrap();

        assert_eq!(
            rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(),
            ["a", "a", "list", "x"]
        );
        assert_eq!(
            rows[3].parent,
            [
                EntryPathSegment::Key("list".into()),
                EntryPathSegment::Index(0)
            ]
        );
        assert_eq!(span_text(text, rows[0].key_span), "a");
        assert_eq!(span_text(text, rows[0].value_span.unwrap()), "one");
        assert_eq!(span_text(text, rows[1].value_span.unwrap()), "two");
    }

    #[test]
    fn dotenv_entry_rows_preserve_duplicates_and_raw_value_spans() {
        let text = "\u{feff}export A=\"one\"\nA=two # comment\nEMPTY=\nBARE\n";
        let rows = extract_entry_rows(Format::Dotenv, text).unwrap();

        assert_eq!(
            rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(),
            ["A", "A", "EMPTY", "BARE"]
        );
        assert!(rows.iter().all(|row| row.parent.is_empty()));
        assert_eq!(span_text(text, rows[0].key_span), "A");
        assert_eq!(span_text(text, rows[0].value_span.unwrap()), "\"one\"");
        assert_eq!(span_text(text, rows[1].value_span.unwrap()), "two");
        assert_eq!(span_text(text, rows[2].value_span.unwrap()), "");
        assert!(rows[3].value_span.is_none());
    }

    #[test]
    fn dotenv_entry_rows_use_lossless_budgets_and_repeated_bom_offsets() {
        let text = "\u{feff}\u{feff}KEY=🚀\n";
        let rows = extract_entry_rows(Format::Dotenv, text).unwrap();
        assert_eq!(span_text(text, rows[0].key_span), "KEY");
        assert_eq!(span_text(text, rows[0].value_span.unwrap()), "🚀");

        let too_many_lines = format!(
            "{}KEY=value",
            "#\n".repeat(crate::dotenv::MAX_PHYSICAL_LINES)
        );
        let error = extract_entry_rows(Format::Dotenv, &too_many_lines).unwrap_err();
        assert!(error.to_string().contains("physical line count"), "{error}");

        let oversized = format!("KEY={}", "x".repeat(crate::dotenv::MAX_DOTENV_BYTES));
        let error = extract_entry_rows(Format::Dotenv, &oversized).unwrap_err();
        assert!(
            error.to_string().contains("maximum supported size"),
            "{error}"
        );
    }

    #[test]
    fn ini_entry_rows_preserve_sections_duplicates_and_decline_continuations() {
        let text = "\u{feff}root = one\n[section]\nkey=two\nkey = three\n  continuation\n";
        let rows = extract_entry_rows(Format::Ini, text).unwrap();

        assert_eq!(
            rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(),
            ["root", "key", "key"]
        );
        assert_eq!(rows[0].parent, []);
        assert_eq!(rows[1].parent, [EntryPathSegment::Key("section".into())]);
        assert_eq!(rows[2].parent, rows[1].parent);
        assert_eq!(span_text(text, rows[0].value_span.unwrap()), "one");
        assert_eq!(span_text(text, rows[1].value_span.unwrap()), "two");
        assert!(rows[2].value_span.is_none());
    }

    #[test]
    fn properties_entry_rows_keep_decoded_keys_and_physical_value_regions() {
        let text = "\u{feff}escaped\\ key=one\nescaped\\ key:two\ncontinued = first\\\n  second\n";
        let rows = extract_entry_rows(Format::Properties, text).unwrap();

        assert_eq!(
            rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(),
            ["escaped key", "escaped key", "continued"]
        );
        assert!(rows.iter().all(|row| row.parent.is_empty()));
        assert_eq!(span_text(text, rows[0].key_span), "escaped\\ key");
        assert_eq!(span_text(text, rows[0].value_span.unwrap()), "one");
        assert_eq!(span_text(text, rows[1].value_span.unwrap()), "two");
        assert_eq!(
            span_text(text, rows[2].value_span.unwrap()),
            "first\\\n  second"
        );
    }

    #[test]
    fn flat_entry_rows_restore_offsets_after_repeated_boms() {
        let text = "\u{feff}\u{feff}k=1\nk=2\n";
        for format in [Format::Dotenv, Format::Ini, Format::Properties] {
            let rows = extract_entry_rows(format, text).unwrap();
            assert_eq!(rows.len(), 2, "{format:?}");
            assert_eq!(rows[0].key, "k", "{format:?}");
            assert_eq!(span_text(text, rows[0].key_span), "k", "{format:?}");
            assert_eq!(span_text(text, rows[0].value_span.unwrap()), "1");
        }
    }

    #[test]
    fn properties_entry_rows_follow_cr_crlf_and_continuation_line_endings() {
        let text = "a=one\rb=two\r\ncontinued=first\\\r\n  second\n";
        let rows = extract_entry_rows(Format::Properties, text).unwrap();
        assert_eq!(
            rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(),
            ["a", "b", "continued"]
        );
        assert_eq!(span_text(text, rows[0].value_span.unwrap()), "one");
        assert_eq!(span_text(text, rows[1].value_span.unwrap()), "two");
        assert_eq!(
            span_text(text, rows[2].value_span.unwrap()),
            "first\\\r\n  second"
        );
    }

    #[test]
    fn properties_comments_never_continue_and_dangling_logical_lines_are_dropped() {
        let text = "# comment\\\nreal=one\n  ! another comment\\\nnext=two\n";
        let rows = extract_entry_rows(Format::Properties, text).unwrap();
        assert_eq!(
            rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(),
            ["real", "next"]
        );
        assert_eq!(span_text(text, rows[0].value_span.unwrap()), "one");
        assert_eq!(span_text(text, rows[1].value_span.unwrap()), "two");

        for dangling in [
            "kept=ok\ndropped=value\\",
            "kept=ok\ndropped=first\\\n  second\\",
        ] {
            let rows = extract_entry_rows(Format::Properties, dangling).unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].key, "kept");
        }

        let terminated_continuation =
            extract_entry_rows(Format::Properties, "kept=ok\ncontinued=value\\\n").unwrap();
        assert_eq!(
            terminated_continuation
                .iter()
                .map(|row| row.key.as_str())
                .collect::<Vec<_>>(),
            ["kept", "continued"]
        );
    }

    #[test]
    fn supported_entry_adapters_return_distinct_exact_duplicate_spans() {
        let cases = [
            (Format::Json, "{\"k\":1,\"k\":2}", "\"k\""),
            (Format::Yaml, "k: 1\nk: 2\n", "k"),
            (Format::Dotenv, "k=1\nk=2\n", "k"),
            (Format::Ini, "k=1\nk=2\n", "k"),
            (Format::Properties, "k=1\nk=2\n", "k"),
        ];

        for (format, text, key_token) in cases {
            let rows = extract_entry_rows(format, text).unwrap();
            assert_eq!(rows.len(), 2, "{format:?}");
            assert_eq!(rows[0].key, "k", "{format:?}");
            assert_eq!(rows[1].key, "k", "{format:?}");
            assert_eq!(span_text(text, rows[0].key_span), key_token, "{format:?}");
            assert_eq!(span_text(text, rows[1].key_span), key_token, "{format:?}");
            assert_ne!(rows[0].key_span, rows[1].key_span, "{format:?}");
            assert_eq!(span_text(text, rows[0].value_span.unwrap()), "1");
            assert_eq!(span_text(text, rows[1].value_span.unwrap()), "2");
            assert_eq!(
                rows,
                extract_entry_rows(format, text).unwrap(),
                "{format:?}"
            );
        }
    }

    #[test]
    fn jsonc_entry_rows_keep_escaped_keys_comments_and_trailing_commas() {
        let text = "{\n  \"a\\u0062\": 1, // first\n  \"ab\": 2,\n}\n";
        let rows = extract_entry_rows(Format::Json, text).unwrap();

        assert_eq!(
            rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(),
            ["ab", "ab"]
        );
        assert_eq!(span_text(text, rows[0].key_span), "\"a\\u0062\"");
        assert_eq!(span_text(text, rows[1].key_span), "\"ab\"");
        assert_eq!(span_text(text, rows[0].value_span.unwrap()), "1");
        assert_eq!(span_text(text, rows[1].value_span.unwrap()), "2");
    }

    #[test]
    fn bounded_entry_extraction_fails_while_building_the_rows() {
        for format in [
            Format::Json,
            Format::Yaml,
            Format::Dotenv,
            Format::Ini,
            Format::Properties,
        ] {
            let text = match format {
                Format::Json => r#"{"a":1,"b":2}"#,
                Format::Yaml => "a: 1\nb: 2\n",
                Format::Dotenv => "A=1\nB=2\n",
                Format::Ini => "[s]\na=1\nb=2\n",
                Format::Properties => "a=1\nb=2\n",
                _ => unreachable!(),
            };
            let error = extract_entry_rows_bounded(format, text, 1).unwrap_err();
            assert_eq!(error, EntryRowsError::Limit { limit: 1 }, "{format:?}");
            let error = extract_entry_rows_bounded(format, text, 0).unwrap_err();
            assert_eq!(error, EntryRowsError::Limit { limit: 0 }, "{format:?}");
        }
        let error =
            extract_entry_rows_strict_yaml_bounded(Format::Yaml, "a: 1\nb: 2\n", 1).unwrap_err();
        assert_eq!(error, EntryRowsError::Limit { limit: 1 });
    }

    #[test]
    fn strict_yaml_entry_rows_reject_aliases_and_merge_keys() {
        let alias = "base: &base\n  nested: one\ncopy: *base\n";
        let error = extract_entry_rows_strict_yaml(Format::Yaml, alias).unwrap_err();
        assert!(error.to_string().contains("aliases"), "{error}");

        let merge = "base: {nested: one}\ncopy:\n  <<: {nested: two}\n";
        let error = extract_entry_rows_strict_yaml(Format::Yaml, merge).unwrap_err();
        assert!(error.to_string().contains("merge keys"), "{error}");

        // A quoted "<<" is a normal key, not a merge key.
        let rows = extract_entry_rows_strict_yaml(Format::Yaml, "\"<<\": 1\n").unwrap();
        assert_eq!(
            rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(),
            ["<<"]
        );
    }

    #[test]
    fn yaml_entry_rows_keep_quoted_keys_without_expanding_aliases() {
        let text = "\"a b\": &base\n  nested: one\ncopy: *base\n";
        let rows = extract_entry_rows(Format::Yaml, text).unwrap();

        assert_eq!(
            rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(),
            ["a b", "nested", "copy"]
        );
        assert_eq!(span_text(text, rows[0].key_span), "\"a b\"");
        assert_eq!(rows[1].parent, [EntryPathSegment::Key("a b".into())]);
        assert_eq!(
            rows.iter().filter(|row| row.key == "nested").count(),
            1,
            "the alias must not invent a second nested row"
        );
    }

    #[test]
    fn yaml_entry_rows_only_report_complete_exact_value_tokens() {
        let text = r"'it''s': 'can''t'
café: 🚀
items:
- α
literal: |
  first
folded: >
  second
empty:
quoted_empty: ''
flow: [é, 🚀]
";
        let rows = extract_entry_rows(Format::Yaml, text).unwrap();
        let row = |key: &str| rows.iter().find(|row| row.key == key).unwrap();

        assert_eq!(span_text(text, row("it's").key_span), "'it''s'");
        assert_eq!(span_text(text, row("it's").value_span.unwrap()), "'can''t'");
        assert_eq!(span_text(text, row("café").key_span), "café");
        assert_eq!(span_text(text, row("café").value_span.unwrap()), "🚀");
        assert!(row("items").value_span.is_none());
        assert!(row("literal").value_span.is_none());
        assert!(row("folded").value_span.is_none());
        assert!(row("empty").value_span.is_none());
        assert_eq!(
            span_text(text, row("quoted_empty").value_span.unwrap()),
            "''"
        );
        assert_eq!(span_text(text, row("flow").value_span.unwrap()), "[é, 🚀]");
    }

    #[test]
    fn yaml_entry_rows_apply_size_after_boms_and_reject_alias_amplification() {
        let bom_count = crate::structured_format::MAX_STRUCTURED_BYTES / '\u{feff}'.len_utf8() + 1;
        let text = format!("{}key: value\n", "\u{feff}".repeat(bom_count));
        let rows = extract_entry_rows(Format::Yaml, &text).unwrap();
        assert_eq!(span_text(&text, rows[0].key_span), "key");
        assert_eq!(span_text(&text, rows[0].value_span.unwrap()), "value");

        let oversized = format!(
            "key: {}",
            "x".repeat(crate::structured_format::MAX_STRUCTURED_BYTES)
        );
        let error = extract_entry_rows(Format::Yaml, &oversized).unwrap_err();
        assert!(
            error.to_string().contains("maximum supported size"),
            "{error}"
        );

        let anchor = "x".repeat(100_000);
        let aliases = format!("{}*base", "*base,".repeat(2_999));
        let amplified = format!("base: &base \"{anchor}\"\ncopies: [{aliases}]\n");
        let error = extract_entry_rows(Format::Yaml, &amplified).unwrap_err();
        assert!(error.to_string().contains("alias expansion"), "{error}");
    }

    #[test]
    fn yaml_entry_rows_bound_compact_collection_depth_at_parser_boundary() {
        let max = crate::yaml_depth::MAX_YAML_FLOW_DEPTH;
        let accepted = format!("{}key: value\n", "- ".repeat(max - 1));
        let rows = extract_entry_rows(Format::Yaml, &accepted).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key, "key");
        assert_eq!(span_text(&accepted, rows[0].value_span.unwrap()), "value");

        let rejected = format!("{}key: value\n", "- ".repeat(max));
        let error = extract_entry_rows(Format::Yaml, &rejected).unwrap_err();
        assert!(
            error.to_string().contains("YAML nesting exceeds"),
            "{error}"
        );
    }

    #[test]
    fn entry_rows_fail_loudly_for_unsupported_and_malformed_formats() {
        for format in [Format::Toml, Format::Xml, Format::Hcl] {
            assert_eq!(
                extract_entry_rows(format, ""),
                Err(EntryRowsError::UnsupportedFormat(format))
            );
        }
        assert!(matches!(
            extract_entry_rows(Format::Json, "{"),
            Err(EntryRowsError::Parse(_))
        ));
        assert!(matches!(
            extract_entry_rows(Format::Yaml, "---\na: 1\n---\nb: 2\n"),
            Err(EntryRowsError::Parse(_))
        ));
        assert!(matches!(
            extract_entry_rows(Format::Yaml, "before: visible\0after: hidden\n"),
            Err(EntryRowsError::Parse(_))
        ));
    }
}
