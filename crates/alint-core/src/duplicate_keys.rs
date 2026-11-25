//! General duplicate-key detection across YAML, JSON, dotenv, properties, INI,
//! TOML, and HCL (ADR-0050 D6, Gap B1).
//!
//! Runs directly on each format's lossless/event parser in `alint-core` so
//! `alint-rules` adds zero direct parser dependencies and per-file evaluation
//! stays streaming and linear in memory.

use std::collections::HashMap;
use std::sync::Arc;

use crate::extract::{ByteSpan, json_parse_options};
use crate::structured_format::{Format, MAX_STRUCTURED_BYTES};

/// Maximum duplicate-key findings retained per file before reporting a
/// non-baselinable analysis failure on breach (ADR-0050 D6).
pub const MAX_DUPLICATES_PER_FILE: usize = 1_000;

/// One duplicate key occurrence found in a structured document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateKey {
    /// Key text as displayed (for secret-bearing formats, only the key name is
    /// ever captured - never any value). Stored as `Arc<str>` so aliased YAML
    /// keys share one allocation instead of multiplying memory across repeats.
    pub key: Arc<str>,
    /// Optional INI section name enclosing the duplicate key.
    pub section: Option<Arc<str>>,
    /// Byte span of the repeated key in the input text.
    pub duplicate: ByteSpan,
    /// Byte span of the first occurrence of this key in the same scope.
    pub first: ByteSpan,
}

/// Outcome of scanning one document for duplicate keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DuplicateScan {
    pub duplicates: Vec<DuplicateKey>,
    /// True when the document exceeded [`MAX_DUPLICATES_PER_FILE`].
    pub capped: bool,
}

impl DuplicateScan {
    fn push(&mut self, dup: DuplicateKey) {
        if self.duplicates.len() >= MAX_DUPLICATES_PER_FILE {
            self.capped = true;
        } else {
            self.duplicates.push(dup);
        }
    }
}

/// Scan `text` parsed as `format` for duplicate keys within the same mapping /
/// object / section / flat-file scope.
///
/// - `yaml`: streams `ayaml` events with YAML 1.2 Core Schema typed scalar keys
///   (reset per `---` document), catches duplicates at every depth including
///   non-leaf keys and mappings inside sequences, skips complex keys cleanly,
///   and allows `<<: *base` merge overrides while flagging two literal `<<:`
///   keys in one mapping.
/// - `json`: walks `jsonc-parser` CST at every depth (honoring `strict_json`).
/// - `dotenv`: walks `adotenv` entries quoting key names only.
/// - `properties`: walks `.properties` entries quoting key names only.
/// - `ini`: walks spanned INI entries merging repeated `[section]` headers into
///   the same section scope.
/// - `toml` / `hcl`: delegates to the format parsers, which already reject
///   duplicate keys/tables/attributes at parse time (`Err(...)`).
/// - `xml`: unsupported (returns `Err`).
///
/// # Errors
///
/// Returns a descriptive parse or limit error string when `text` is malformed,
/// exceeds parser limits, or uses an unsupported format (`Format::Xml`).
pub fn find_duplicate_keys(
    format: Format,
    text: &str,
    strict_json: bool,
) -> Result<DuplicateScan, String> {
    match format {
        Format::Yaml => scan_yaml(text),
        Format::Json => scan_json(text, strict_json),
        Format::Dotenv => scan_dotenv(text),
        Format::Properties => scan_properties(text),
        Format::Ini => scan_ini(text),
        Format::Toml => {
            Format::Toml.parse(text)?;
            Ok(DuplicateScan::default())
        }
        Format::Hcl => {
            Format::Hcl.parse(text)?;
            Ok(DuplicateScan::default())
        }
        Format::Xml => Err(
            "duplicate-key detection is not supported for XML (sibling elements legitimately \
             repeat, and duplicate XML attributes are already parse errors)"
                .to_string(),
        ),
    }
}

// ---------------------------------------------------------------------------
// YAML
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum YamlKey {
    Null,
    Bool(bool),
    Uint(u128),
    NegInt(i128),
    Float(u64),
    String(String),
    Tagged(String, String),
}

fn canonical_float_bits(bits: u64) -> u64 {
    let f = f64::from_bits(bits);
    if f.is_nan() {
        f64::NAN.to_bits()
    } else if f == 0.0 {
        0.0_f64.to_bits()
    } else {
        bits
    }
}

fn yaml_typed_key(
    value: &str,
    style: ayaml::ScalarStyle,
    tag: Option<&ayaml::Tag>,
) -> (YamlKey, Arc<str>) {
    use crate::yaml::{CoreScalar, resolve_scalar_payload};

    let key = match resolve_scalar_payload(value, style, tag) {
        Some(CoreScalar::Null) => YamlKey::Null,
        Some(CoreScalar::Bool(b)) => YamlKey::Bool(b),
        Some(CoreScalar::Uint(u)) => YamlKey::Uint(u),
        Some(CoreScalar::NegInt(i)) => YamlKey::NegInt(i),
        Some(CoreScalar::Float(bits)) => YamlKey::Float(canonical_float_bits(bits)),
        Some(CoreScalar::String(s)) => YamlKey::String(s),
        None => {
            let tag_str = tag.map_or_else(String::new, |t| format!("{}{}", t.handle(), t.suffix()));
            YamlKey::Tagged(tag_str, value.to_string())
        }
    };
    let display: Arc<str> = if value.is_empty() && matches!(key, YamlKey::Null) {
        Arc::from("null")
    } else {
        Arc::from(value)
    };
    (key, display)
}

enum YamlFrame {
    Mapping {
        expecting_key: bool,
        seen: HashMap<YamlKey, ByteSpan>,
    },
    Sequence,
}

struct YamlWalk {
    offset: usize,
    scan: DuplicateScan,
    stack: Vec<YamlFrame>,
    anchors: HashMap<String, Option<(YamlKey, Arc<str>)>>,
    collection_depth: usize,
}

impl YamlWalk {
    fn new(offset: usize) -> Self {
        Self {
            offset,
            scan: DuplicateScan::default(),
            stack: Vec::new(),
            anchors: HashMap::new(),
            collection_depth: 0,
        }
    }

    fn record_key(&mut self, key_id: YamlKey, display: Arc<str>, key_span: ByteSpan) {
        if let Some(YamlFrame::Mapping {
            expecting_key,
            seen,
        }) = self.stack.last_mut()
            && *expecting_key
        {
            if let Some(&first) = seen.get(&key_id) {
                self.scan.push(DuplicateKey {
                    key: display,
                    section: None,
                    duplicate: key_span,
                    first,
                });
            } else {
                seen.insert(key_id, key_span);
            }
            *expecting_key = false;
        }
    }

    fn handle_event(&mut self, event: ayaml::Event) -> Result<(), String> {
        use ayaml::Event;

        match event {
            Event::DocumentStart { .. } | Event::DocumentEnd { .. } => {
                self.stack.clear();
                self.anchors.clear();
                self.collection_depth = 0;
            }
            Event::Scalar {
                value,
                style,
                tag,
                anchor,
                span,
                ..
            } => {
                let typed = yaml_typed_key(&value, style, tag.as_ref());
                if let Some(a) = anchor {
                    self.anchors
                        .insert(a.as_str().to_string(), Some(typed.clone()));
                }
                if let Some(YamlFrame::Mapping { expecting_key, .. }) = self.stack.last_mut() {
                    if *expecting_key {
                        let key_span =
                            ByteSpan::new(span.start() + self.offset, span.end() + self.offset);
                        self.record_key(typed.0, typed.1, key_span);
                    } else {
                        *expecting_key = true;
                    }
                }
            }
            Event::Alias { anchor, span } => {
                if let Some(YamlFrame::Mapping { expecting_key, .. }) = self.stack.last_mut() {
                    if *expecting_key {
                        let resolved = self
                            .anchors
                            .get(anchor.as_str())
                            .and_then(|v| v.as_ref())
                            .map(|(k, d)| (k.clone(), Arc::clone(d)));
                        if let Some((key_id, display)) = resolved {
                            let key_span =
                                ByteSpan::new(span.start() + self.offset, span.end() + self.offset);
                            self.record_key(key_id, display, key_span);
                        } else if let Some(YamlFrame::Mapping { expecting_key, .. }) =
                            self.stack.last_mut()
                        {
                            *expecting_key = false;
                        }
                    } else {
                        *expecting_key = true;
                    }
                }
            }
            Event::MappingStart { anchor, .. } => {
                self.enter_collection(anchor, true)?;
            }
            Event::SequenceStart { anchor, .. } => {
                self.enter_collection(anchor, false)?;
            }
            Event::MappingEnd { .. } | Event::SequenceEnd { .. } => {
                self.collection_depth = self.collection_depth.saturating_sub(1);
                self.stack.pop();
            }
            _ => {}
        }
        Ok(())
    }

    fn enter_collection(
        &mut self,
        anchor: Option<ayaml::Anchor>,
        is_mapping: bool,
    ) -> Result<(), String> {
        self.collection_depth += 1;
        if self.collection_depth > crate::yaml_depth::MAX_YAML_FLOW_DEPTH {
            return Err("YAML nesting exceeds the supported limit".to_string());
        }
        if let Some(a) = anchor {
            self.anchors.insert(a.as_str().to_string(), None);
        }
        if let Some(YamlFrame::Mapping { expecting_key, .. }) = self.stack.last_mut() {
            *expecting_key = !*expecting_key;
        }
        self.stack.push(if is_mapping {
            YamlFrame::Mapping {
                expecting_key: true,
                seen: HashMap::new(),
            }
        } else {
            YamlFrame::Sequence
        });
        Ok(())
    }
}

fn scan_yaml(text: &str) -> Result<DuplicateScan, String> {
    let stripped = text.trim_start_matches('\u{feff}');
    let offset = text.len() - stripped.len();
    if stripped.len() > MAX_STRUCTURED_BYTES {
        return Err(format!(
            "input exceeds the maximum supported size for a structured document ({MAX_STRUCTURED_BYTES} bytes)"
        ));
    }
    let _budget = crate::yaml_budget::reserve(stripped.len());
    // Stream raw ayaml events without materializing a tree or expanding
    // aliases, so multi-document streams with anchors/aliases remain O(N) and
    // do not go through the single-document alias-expansion prepass.
    let guards = crate::yaml_depth::scan_guards(stripped);
    if !guards.flow_within_limit() {
        return Err(format!(
            "YAML flow nesting exceeds the maximum supported depth ({})",
            crate::yaml_depth::MAX_YAML_FLOW_DEPTH
        ));
    }

    let map_ayaml_err = |error: ayaml::Error| {
        if error
            .limit()
            .is_some_and(|limit| limit.kind() == ayaml::LimitKind::Depth)
        {
            format!("YAML nesting exceeds the supported limit: {error}")
        } else {
            error.to_string()
        }
    };

    let stream = crate::yaml::parse_events_stream(stripped).map_err(map_ayaml_err)?;
    let mut walk = YamlWalk::new(offset);
    for parsed in stream {
        walk.handle_event(parsed.map_err(map_ayaml_err)?)?;
    }
    Ok(walk.scan)
}

// ---------------------------------------------------------------------------
// JSON / JSONC
// ---------------------------------------------------------------------------

fn scan_json(text: &str, strict: bool) -> Result<DuplicateScan, String> {
    use jsonc_parser::ast::{ObjectPropName, Value};
    use jsonc_parser::common::Ranged as _;
    use jsonc_parser::{CollectOptions, CommentCollectionStrategy, parse_to_ast};

    // Enforce size, nesting depth, BOM, empty-file, and strict-vs-JSONC guards.
    Format::Json.parse_with_strict_json(text, strict)?;

    let stripped = if strict {
        text
    } else {
        text.trim_start_matches('\u{feff}')
    };
    let offset = text.len() - stripped.len();
    let ast = parse_to_ast(
        stripped,
        &CollectOptions {
            comments: CommentCollectionStrategy::Off,
            tokens: false,
        },
        &json_parse_options(strict),
    )
    .map_err(|e| e.to_string())?;

    let mut scan = DuplicateScan::default();
    let mut pending: Vec<&Value<'_>> = ast.value.iter().collect();
    while let Some(node) = pending.pop() {
        match node {
            Value::Object(object) => {
                let mut seen: HashMap<&str, ByteSpan> =
                    HashMap::with_capacity(object.properties.len());
                for prop in &object.properties {
                    let (key, range) = match &prop.name {
                        ObjectPropName::String(lit) => (lit.value.as_ref(), lit.range()),
                        ObjectPropName::Word(lit) => (lit.value, lit.range()),
                    };
                    let key_span = ByteSpan::new(range.start + offset, range.end + offset);
                    if let Some(&first) = seen.get(key) {
                        scan.push(DuplicateKey {
                            key: Arc::from(key),
                            section: None,
                            duplicate: key_span,
                            first,
                        });
                    } else {
                        seen.insert(key, key_span);
                    }
                }
                for prop in object.properties.iter().rev() {
                    pending.push(&prop.value);
                }
            }
            Value::Array(array) => {
                for elem in array.elements.iter().rev() {
                    pending.push(elem);
                }
            }
            Value::StringLit(_)
            | Value::NumberLit(_)
            | Value::BooleanLit(_)
            | Value::NullKeyword(_) => {}
        }
    }
    Ok(scan)
}

// ---------------------------------------------------------------------------
// Dotenv
// ---------------------------------------------------------------------------

fn scan_dotenv(text: &str) -> Result<DuplicateScan, String> {
    let document = crate::dotenv::parse_document(text)?;
    let mut scan = DuplicateScan::default();
    let mut seen: HashMap<&str, ByteSpan> = HashMap::new();
    for entry in document.entries() {
        let key = entry.key();
        let raw = entry.key_span();
        let key_span = ByteSpan::new(raw.start, raw.end);
        if let Some(&first) = seen.get(key) {
            scan.push(DuplicateKey {
                key: Arc::from(key),
                section: None,
                duplicate: key_span,
                first,
            });
        } else {
            seen.insert(key, key_span);
        }
    }
    Ok(scan)
}

// ---------------------------------------------------------------------------
// Java .properties
// ---------------------------------------------------------------------------

fn scan_properties(text: &str) -> Result<DuplicateScan, String> {
    let mut scan = DuplicateScan::default();
    let mut seen: HashMap<String, ByteSpan> = HashMap::new();
    crate::properties::for_each_entry(text, None, false, |entry| {
        let key_span = ByteSpan::new(entry.key_span.start, entry.key_span.end);
        if let Some(&first) = seen.get(&entry.key) {
            scan.push(DuplicateKey {
                key: Arc::from(entry.key),
                section: None,
                duplicate: key_span,
                first,
            });
        } else {
            seen.insert(entry.key, key_span);
        }
    })
    .map_err(|err| match err {
        crate::properties::PropertiesError::Parse(msg) => msg,
        crate::properties::PropertiesError::Limit { limit } => {
            format!("properties file exceeded {limit} entries")
        }
    })?;
    Ok(scan)
}

// ---------------------------------------------------------------------------
// INI
// ---------------------------------------------------------------------------

fn scan_ini(text: &str) -> Result<DuplicateScan, String> {
    let mut scan = DuplicateScan::default();
    let mut seen: HashMap<(Option<String>, String), ByteSpan> = HashMap::new();
    crate::ini::for_each_entry(text, |entry| {
        let scope = (entry.section.map(str::to_string), entry.key.to_string());
        if let Some(&first) = seen.get(&scope) {
            scan.push(DuplicateKey {
                key: Arc::from(entry.key),
                section: entry.section.map(Arc::from),
                duplicate: entry.key_span,
                first,
            });
        } else {
            seen.insert(scope, entry.key_span);
        }
    })?;
    Ok(scan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_catches_nested_sequence_and_non_leaf_duplicates_with_core_schema_types() {
        let yaml = "\
jobs:
  a: 1
jobs:
  b: 2
steps:
  - name: first
    name: second
types:
  1: int
  \"1\": str
  true: bool
  \"true\": str
  foo: plain
  \"foo\": quoted
";
        let scan = find_duplicate_keys(Format::Yaml, yaml, false).unwrap();
        assert!(!scan.capped);
        let keys: Vec<&str> = scan.duplicates.iter().map(|d| d.key.as_ref()).collect();
        assert_eq!(keys, ["jobs", "name", "foo"]);
        // Verify exact byte spans on both duplicate and first occurrence.
        assert_eq!(&yaml[scan.duplicates[0].first.as_range()], "jobs");
        assert_eq!(&yaml[scan.duplicates[0].duplicate.as_range()], "jobs");
        assert!(scan.duplicates[0].duplicate.start > scan.duplicates[0].first.start);
    }

    #[test]
    fn yaml_multi_doc_and_merge_key_semantics() {
        // Multi-document YAML resets seen-key sets at `---`.
        let multi = "apiVersion: v1\nkind: ConfigMap\n---\napiVersion: v1\nkind: Secret\n";
        assert_eq!(
            find_duplicate_keys(Format::Yaml, multi, false)
                .unwrap()
                .duplicates,
            []
        );
        // Merge override is NOT a duplicate.
        let merge_ok = "base: &b\n  foo: 1\nchild:\n  <<: *b\n  foo: 2\n";
        assert_eq!(
            find_duplicate_keys(Format::Yaml, merge_ok, false)
                .unwrap()
                .duplicates,
            []
        );
        // Two literal `<<:` keys in one mapping ARE flagged.
        let merge_dup = "a: &a\n  x: 1\nb: &b\n  y: 2\nchild:\n  <<: *a\n  <<: *b\n";
        let scan = find_duplicate_keys(Format::Yaml, merge_dup, false).unwrap();
        assert_eq!(scan.duplicates.len(), 1);
        assert_eq!(scan.duplicates[0].key.as_ref(), "<<");
    }

    #[test]
    fn json_nested_jsonc_and_strict_mode() {
        let jsonc =
            "{\n  // comment\n  \"a\": 1,\n  \"arr\": [{\"x\": 1, \"x\": 2,}],\n  \"a\": 3,\n}\n";
        let tolerant = find_duplicate_keys(Format::Json, jsonc, false).unwrap();
        let keys: Vec<&str> = tolerant.duplicates.iter().map(|d| d.key.as_ref()).collect();
        assert_eq!(keys, ["a", "x"]);
        // Strict mode rejects JSONC comments / trailing commas / BOM / empty.
        assert!(find_duplicate_keys(Format::Json, jsonc, true).is_err());
        assert!(find_duplicate_keys(Format::Json, "\u{feff}{\"a\":1}", true).is_err());
        assert!(find_duplicate_keys(Format::Json, "   ", true).is_err());
        let strict_dup = "{\"a\": 1, \"a\": 2}";
        let scan = find_duplicate_keys(Format::Json, strict_dup, true).unwrap();
        assert_eq!(scan.duplicates.len(), 1);
        assert_eq!(scan.duplicates[0].key.as_ref(), "a");
    }

    #[test]
    fn dotenv_properties_and_ini_duplicates() {
        let env = "FOO=secret1\nBAR=ok\nFOO=secret2\n";
        let scan = find_duplicate_keys(Format::Dotenv, env, false).unwrap();
        assert_eq!(scan.duplicates.len(), 1);
        assert_eq!(scan.duplicates[0].key.as_ref(), "FOO");
        assert_eq!(&env[scan.duplicates[0].duplicate.as_range()], "FOO");

        let props = "db.host = a\ndb.port = 1\ndb.host = b\n";
        let scan = find_duplicate_keys(Format::Properties, props, false).unwrap();
        assert_eq!(scan.duplicates.len(), 1);
        assert_eq!(scan.duplicates[0].key.as_ref(), "db.host");

        // INI merges repeated `[section]` headers into the same scope.
        let ini = "root = 1\n[a]\nk = 1\n[b]\nk = 2\n[a]\nj = 3\nk = 4\n";
        let scan = find_duplicate_keys(Format::Ini, ini, false).unwrap();
        assert_eq!(scan.duplicates.len(), 1);
        assert_eq!(scan.duplicates[0].section.as_deref(), Some("a"));
        assert_eq!(scan.duplicates[0].key.as_ref(), "k");
    }

    #[test]
    fn toml_hcl_parse_errors_and_xml_rejected() {
        assert!(find_duplicate_keys(Format::Toml, "a = 1\na = 2\n", false).is_err());
        assert_eq!(
            find_duplicate_keys(Format::Toml, "a = 1\nb = 2\n", false)
                .unwrap()
                .duplicates,
            []
        );
        assert!(find_duplicate_keys(Format::Hcl, "a = 1\na = 2\n", false).is_err());
        assert!(find_duplicate_keys(Format::Xml, "<r><a/><a/></r>", false).is_err());
    }

    #[test]
    fn per_file_cap_triggers_without_unbounded_growth() {
        use std::fmt::Write as _;
        let mut yaml = String::new();
        for i in 0..=(MAX_DUPLICATES_PER_FILE + 10) {
            let _ = writeln!(yaml, "k: {i}");
        }
        let scan = find_duplicate_keys(Format::Yaml, &yaml, false).unwrap();
        assert_eq!(scan.duplicates.len(), MAX_DUPLICATES_PER_FILE);
        assert!(scan.capped);
    }
}
