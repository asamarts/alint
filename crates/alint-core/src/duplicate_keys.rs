//! General duplicate-key detection across YAML, JSON, dotenv, properties, INI,
//! TOML, and HCL (ADR-0050 D6, Gap B1).
//!
//! Runs directly on each format's lossless/event parser in `alint-core` so
//! `alint-rules` adds zero direct parser dependencies and per-file evaluation
//! stays streaming and linear in memory.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher as _, Hash, Hasher};
use std::rc::Rc;
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
    /// Optional canonical key identity used for stable, type-aware baseline
    /// keys (set for YAML Core Schema typed keys so `1` and `"1"` stay distinct
    /// while `true`/`True` and `1`/`0x1` share one baseline key). Keys whose
    /// canonical token exceeds 128 bytes are hashed once (`sha256:...`) so
    /// aliased multi-megabyte YAML keys never amplify memory or CPU across
    /// repeats.
    pub canonical_key: Option<Arc<str>>,
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
///   non-leaf keys and mappings inside sequences, validates explicit core scalar
///   tags and anchor/alias references (rejecting unknown and cyclic aliases),
///   preserves custom tags as distinct key types, skips complex keys cleanly,
///   and allows `<<: *base` merge overrides while flagging two literal `<<:`
///   keys in one mapping.
/// - `json`: walks `jsonc-parser` CST at every depth in source order (honoring
///   `strict_json`).
/// - `dotenv`: walks `adotenv` entries quoting key names only.
/// - `properties`: walks `.properties` entries quoting key names only.
/// - `ini`: walks spanned INI entries merging repeated `[section]` headers into
///   the same section scope.
/// - `toml`: delegates to the TOML parser, which rejects duplicate keys and
///   tables at parse time (`Err(...)`).
/// - `hcl`: validates with `Format::Hcl.parse` (rejecting duplicate attributes
///   and identical AST object keys) and additionally scans object literals for
///   mixed identifier/quoted/parenthesized duplicate static keys (`{ a = 1,
///   "a" = 2 }`).
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
        Format::Hcl => scan_hcl(text),
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
    String(Arc<str>),
    Tagged(Arc<str>, Box<Self>),
}

/// Maximum raw canonical-token byte length retained inline before switching to
/// a 71-byte `sha256:<hex>` digest (matches `alint-rules`'s `bounded_key_token`).
const MAX_INLINE_CANONICAL_TOKEN_BYTES: usize = 128;

enum BoundedTokenSink {
    Inline(String),
    Hashed(sha2::Sha256),
}

impl BoundedTokenSink {
    fn new() -> Self {
        Self::Inline(String::new())
    }

    fn push_str(&mut self, chunk: &str) {
        use sha2::Digest as _;
        match self {
            Self::Inline(buf) => {
                if buf.len().saturating_add(chunk.len()) <= MAX_INLINE_CANONICAL_TOKEN_BYTES {
                    buf.push_str(chunk);
                } else {
                    let mut hasher = sha2::Sha256::new();
                    hasher.update(buf.as_bytes());
                    hasher.update(chunk.as_bytes());
                    *self = Self::Hashed(hasher);
                }
            }
            Self::Hashed(hasher) => hasher.update(chunk.as_bytes()),
        }
    }

    fn finish(self) -> Arc<str> {
        use sha2::Digest as _;
        use std::fmt::Write as _;
        match self {
            Self::Inline(buf) => Arc::from(buf),
            Self::Hashed(hasher) => {
                let digest = hasher.finalize();
                let mut out = String::with_capacity(7 + 64);
                out.push_str("sha256:");
                for b in digest {
                    let _ = write!(out, "{b:02x}");
                }
                Arc::from(out)
            }
        }
    }
}

impl YamlKey {
    fn from_core_scalar(scalar: crate::yaml::CoreScalar) -> Self {
        use crate::yaml::CoreScalar;
        match scalar {
            CoreScalar::Null => Self::Null,
            CoreScalar::Bool(b) => Self::Bool(b),
            CoreScalar::Uint(u) => Self::Uint(u),
            CoreScalar::NegInt(i) => Self::NegInt(i),
            CoreScalar::Float(bits) => Self::Float(canonical_float_bits(bits)),
            CoreScalar::String(s) => Self::String(Arc::from(s)),
        }
    }

    fn write_canonical_token(&self, sink: &mut BoundedTokenSink) {
        match self {
            Self::Null => sink.push_str("null"),
            Self::Bool(b) => sink.push_str(if *b { "bool:true" } else { "bool:false" }),
            Self::Uint(u) => sink.push_str(&format!("uint:{u}")),
            Self::NegInt(i) => sink.push_str(&format!("neg:{i}")),
            Self::Float(bits) => sink.push_str(&format!("float:{bits:016x}")),
            Self::String(s) => {
                sink.push_str("str:");
                sink.push_str(s);
            }
            Self::Tagged(tag, inner) => {
                sink.push_str("tag:");
                sink.push_str(tag);
                sink.push_str(":");
                inner.write_canonical_token(sink);
            }
        }
    }

    /// Return the canonical baseline token for this key, streaming directly
    /// into SHA-256 without materializing an intermediate buffer when the
    /// canonical form exceeds [`MAX_INLINE_CANONICAL_TOKEN_BYTES`].
    fn bounded_canonical_token(&self) -> Arc<str> {
        let mut sink = BoundedTokenSink::new();
        self.write_canonical_token(&mut sink);
        sink.finish()
    }
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

/// Validate explicit YAML 1.2 core scalar tags on any scalar (key or value),
/// matching `ayaml`'s resolver so invalid `!!int abc` / `!!bool maybe` never
/// pass silently.
fn validate_yaml_core_tag(
    value: &str,
    style: ayaml::ScalarStyle,
    tag: Option<&ayaml::Tag>,
) -> Result<(), String> {
    if let Some(tag) = tag
        && crate::yaml::is_core_scalar_tag(tag)
        && crate::yaml::resolve_core_scalar(value, style, Some(tag)).is_none()
    {
        return Err(format!(
            "invalid `{}{}` scalar `{value}`",
            tag.handle(),
            tag.suffix()
        ));
    }
    Ok(())
}

struct InternedYamlKey {
    hash: u64,
    key: YamlKey,
    canonical: OnceCell<Arc<str>>,
}

/// O(1)-cloneable, pre-hashed YAML key handle shared across anchors, aliases,
/// and mappings within one `scan_yaml` walk.
///
/// Computing the `SipHash` once per scalar and short-circuiting equality with
/// `Rc::ptr_eq` prevents multi-megabyte anchored keys from being re-hashed or
/// byte-compared on every `*alias` lookup, and `OnceCell` ensures the bounded
/// canonical token (`<= 128 B` or `sha256:...`) is computed at most once per
/// document even across thousands of distinct mappings.
#[derive(Clone)]
struct SharedYamlKey(Rc<InternedYamlKey>);

impl SharedYamlKey {
    fn new(key: YamlKey, hasher_state: &RandomState) -> Self {
        let hash = hasher_state.hash_one(&key);
        Self(Rc::new(InternedYamlKey {
            hash,
            key,
            canonical: OnceCell::new(),
        }))
    }

    fn bounded_canonical_token(&self) -> Arc<str> {
        Arc::clone(
            self.0
                .canonical
                .get_or_init(|| self.0.key.bounded_canonical_token()),
        )
    }
}

impl PartialEq for SharedYamlKey {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0) || (self.0.hash == other.0.hash && self.0.key == other.0.key)
    }
}

impl Eq for SharedYamlKey {}

impl Hash for SharedYamlKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash.hash(state);
    }
}

/// Resolve a YAML mapping key or anchored scalar into a shared, O(1)-cloneable
/// typed key + display label, preserving custom and unknown core tags as
/// distinct key types (matching `ayaml-0.2.3/src/resolver.rs`).
fn resolve_yaml_key(
    value: &str,
    style: ayaml::ScalarStyle,
    tag: Option<&ayaml::Tag>,
    hasher_state: &RandomState,
) -> Result<(SharedYamlKey, Arc<str>), String> {
    let key = match tag {
        Some(t) if crate::yaml::is_core_scalar_tag(t) => {
            let scalar =
                crate::yaml::resolve_core_scalar(value, style, Some(t)).ok_or_else(|| {
                    format!("invalid `{}{}` scalar `{value}`", t.handle(), t.suffix())
                })?;
            YamlKey::from_core_scalar(scalar)
        }
        Some(t) if t.handle() == "!" && t.suffix().is_empty() => YamlKey::String(Arc::from(value)),
        Some(t) if t.is_core() => {
            // Unknown core tags like `!!binary` or `!!timestamp` keep their tag
            // identity and string payload.
            let full_tag: Arc<str> = Arc::from(format!("{}{}", t.handle(), t.suffix()));
            YamlKey::Tagged(full_tag, Box::new(YamlKey::String(Arc::from(value))))
        }
        Some(t) => {
            let full_tag: Arc<str> = Arc::from(format!("{}{}", t.handle(), t.suffix()));
            let payload = crate::yaml::resolve_core_scalar(value, style, None).map_or_else(
                || YamlKey::String(Arc::from(value)),
                YamlKey::from_core_scalar,
            );
            YamlKey::Tagged(full_tag, Box::new(payload))
        }
        None => crate::yaml::resolve_core_scalar(value, style, None).map_or_else(
            || YamlKey::String(Arc::from(value)),
            YamlKey::from_core_scalar,
        ),
    };
    let display: Arc<str> = match &key {
        YamlKey::Null if value.is_empty() => Arc::from("null"),
        YamlKey::String(s) => Arc::clone(s),
        YamlKey::Tagged(_, inner) => match inner.as_ref() {
            YamlKey::String(s) => Arc::clone(s),
            _ => Arc::from(value),
        },
        _ => Arc::from(value),
    };
    Ok((SharedYamlKey::new(key, hasher_state), display))
}

#[derive(Clone)]
enum AnchorState {
    /// Collection currently open on the stack (referencing it is cyclic).
    Open,
    /// Finished mapping or sequence anchor (valid alias, skipped as complex key).
    Collection,
    /// Scalar anchor resolved once into O(1)-cloneable handles.
    Scalar(SharedYamlKey, Arc<str>),
}

enum YamlFrame {
    Mapping {
        anchor: Option<String>,
        expecting_key: bool,
        seen: HashMap<SharedYamlKey, ByteSpan>,
    },
    Sequence {
        anchor: Option<String>,
    },
}

struct YamlWalk {
    offset: usize,
    scan: DuplicateScan,
    stack: Vec<YamlFrame>,
    anchors: HashMap<String, AnchorState>,
    collection_depth: usize,
    hasher_state: RandomState,
}

impl YamlWalk {
    fn new(offset: usize) -> Self {
        Self {
            offset,
            scan: DuplicateScan::default(),
            stack: Vec::new(),
            anchors: HashMap::new(),
            collection_depth: 0,
            hasher_state: RandomState::new(),
        }
    }

    fn expecting_mapping_key(&self) -> bool {
        matches!(
            self.stack.last(),
            Some(YamlFrame::Mapping {
                expecting_key: true,
                ..
            })
        )
    }

    fn record_key(&mut self, key_id: SharedYamlKey, display: Arc<str>, key_span: ByteSpan) {
        if let Some(YamlFrame::Mapping {
            expecting_key,
            seen,
            ..
        }) = self.stack.last_mut()
            && *expecting_key
        {
            if !self.scan.capped {
                if let Some(&first) = seen.get(&key_id) {
                    let canonical = key_id.bounded_canonical_token();
                    self.scan.push(DuplicateKey {
                        key: display,
                        canonical_key: Some(canonical),
                        section: None,
                        duplicate: key_span,
                        first,
                    });
                } else {
                    seen.insert(key_id, key_span);
                }
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
                let is_key = self.expecting_mapping_key();
                if is_key || anchor.is_some() {
                    let (key_id, display) =
                        resolve_yaml_key(&value, style, tag.as_ref(), &self.hasher_state)?;
                    if let Some(a) = anchor {
                        self.anchors.insert(
                            a.as_str().to_string(),
                            AnchorState::Scalar(key_id.clone(), Arc::clone(&display)),
                        );
                    }
                    if is_key {
                        let key_span =
                            ByteSpan::new(span.start() + self.offset, span.end() + self.offset);
                        self.record_key(key_id, display, key_span);
                    } else if let Some(YamlFrame::Mapping { expecting_key, .. }) =
                        self.stack.last_mut()
                    {
                        *expecting_key = true;
                    }
                } else {
                    validate_yaml_core_tag(&value, style, tag.as_ref())?;
                    if let Some(YamlFrame::Mapping { expecting_key, .. }) = self.stack.last_mut() {
                        *expecting_key = true;
                    }
                }
            }
            Event::Alias { anchor, span } => {
                let state = match self.anchors.get(anchor.as_str()) {
                    None => return Err(format!("unknown YAML alias `*{}`", anchor.as_str())),
                    Some(AnchorState::Open) => {
                        return Err(format!("cyclic YAML alias `*{}`", anchor.as_str()));
                    }
                    Some(AnchorState::Collection) => None,
                    Some(AnchorState::Scalar(k, d)) => Some((k.clone(), Arc::clone(d))),
                };
                if let Some(YamlFrame::Mapping { expecting_key, .. }) = self.stack.last_mut() {
                    if *expecting_key {
                        if let Some((key_id, display)) = state {
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
                if let Some(frame) = self.stack.pop() {
                    let frame_anchor = match frame {
                        YamlFrame::Mapping { anchor, .. } | YamlFrame::Sequence { anchor } => {
                            anchor
                        }
                    };
                    if let Some(name) = frame_anchor
                        && matches!(self.anchors.get(&name), Some(AnchorState::Open))
                    {
                        self.anchors.insert(name, AnchorState::Collection);
                    }
                }
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
        let anchor_name = anchor.map(|a| {
            let name = a.as_str().to_string();
            self.anchors.insert(name.clone(), AnchorState::Open);
            name
        });
        if let Some(YamlFrame::Mapping { expecting_key, .. }) = self.stack.last_mut() {
            *expecting_key = !*expecting_key;
        }
        self.stack.push(if is_mapping {
            YamlFrame::Mapping {
                anchor: anchor_name,
                expecting_key: true,
                seen: HashMap::new(),
            }
        } else {
            YamlFrame::Sequence {
                anchor: anchor_name,
            }
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

enum JsonFrame<'a> {
    Object {
        props: &'a [jsonc_parser::ast::ObjectProp<'a>],
        idx: usize,
        seen: HashMap<&'a str, ByteSpan>,
    },
    Array {
        elems: &'a [jsonc_parser::ast::Value<'a>],
        idx: usize,
    },
}

fn push_json_value<'a>(stack: &mut Vec<JsonFrame<'a>>, value: &'a jsonc_parser::ast::Value<'a>) {
    use jsonc_parser::ast::Value;

    match value {
        Value::Object(object) => stack.push(JsonFrame::Object {
            props: &object.properties,
            idx: 0,
            seen: HashMap::with_capacity(object.properties.len()),
        }),
        Value::Array(array) => stack.push(JsonFrame::Array {
            elems: &array.elements,
            idx: 0,
        }),
        Value::StringLit(_)
        | Value::NumberLit(_)
        | Value::BooleanLit(_)
        | Value::NullKeyword(_) => {}
    }
}

fn scan_json(text: &str, strict: bool) -> Result<DuplicateScan, String> {
    use jsonc_parser::ast::ObjectPropName;
    use jsonc_parser::common::Ranged as _;
    use jsonc_parser::{CollectOptions, CommentCollectionStrategy, parse_to_ast};

    // Enforce size, nesting depth, BOM, empty-file, and strict-vs-JSONC guards.
    Format::Json.parse_with_strict_json(text, strict)?;
    if !strict
        && text
            .trim_matches(|c: char| c == '\u{feff}' || c.is_whitespace())
            .is_empty()
    {
        return Ok(DuplicateScan::default());
    }

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
    let mut stack: Vec<JsonFrame<'_>> = Vec::new();
    if let Some(root) = &ast.value {
        push_json_value(&mut stack, root);
    }

    while let Some(frame) = stack.last_mut() {
        match frame {
            JsonFrame::Object { props, idx, seen } => {
                if *idx >= props.len() {
                    stack.pop();
                    continue;
                }
                let prop = &props[*idx];
                *idx += 1;
                let (key, range) = match &prop.name {
                    ObjectPropName::String(lit) => (lit.value.as_ref(), lit.range()),
                    ObjectPropName::Word(lit) => (lit.value, lit.range()),
                };
                let key_span = ByteSpan::new(range.start + offset, range.end + offset);
                if let Some(&first) = seen.get(key) {
                    if !scan.capped {
                        scan.push(DuplicateKey {
                            key: Arc::from(key),
                            canonical_key: None,
                            section: None,
                            duplicate: key_span,
                            first,
                        });
                    }
                } else {
                    seen.insert(key, key_span);
                }
                let child = &prop.value;
                push_json_value(&mut stack, child);
            }
            JsonFrame::Array { elems, idx } => {
                if *idx >= elems.len() {
                    stack.pop();
                    continue;
                }
                let child = &elems[*idx];
                *idx += 1;
                push_json_value(&mut stack, child);
            }
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
            if !scan.capped {
                scan.push(DuplicateKey {
                    key: Arc::from(key),
                    canonical_key: None,
                    section: None,
                    duplicate: key_span,
                    first,
                });
            }
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
            if !scan.capped {
                scan.push(DuplicateKey {
                    key: Arc::from(entry.key),
                    canonical_key: None,
                    section: None,
                    duplicate: key_span,
                    first,
                });
            }
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
    type SectionBucket<'a> = (
        Option<Arc<str>>,
        HashMap<&'a str, (ByteSpan, Option<Arc<str>>)>,
    );
    let mut scan = DuplicateScan::default();
    let mut buckets: Vec<SectionBucket<'_>> = Vec::new();
    let mut section_ids: HashMap<Option<&str>, usize> = HashMap::new();
    // 1-slot fast path for consecutive keys inside the same `[section]` header
    // (keyed by slice pointer + len since all sections borrow from `text`),
    // avoiding O(N * section_len) SipHash and memcmp on large section headers.
    let mut active_section: Option<(Option<(usize, usize)>, usize)> = None;
    crate::ini::for_each_entry(text, |entry| {
        if scan.capped {
            return;
        }
        let sec_ptr = entry.section.map(|s| (s.as_ptr() as usize, s.len()));
        let bucket_idx = match active_section {
            Some((cached_ptr, idx)) if cached_ptr == sec_ptr => idx,
            _ => {
                let idx = *section_ids.entry(entry.section).or_insert_with(|| {
                    let next = buckets.len();
                    buckets.push((None, HashMap::new()));
                    next
                });
                active_section = Some((sec_ptr, idx));
                idx
            }
        };
        let (section_arc, keys) = &mut buckets[bucket_idx];
        if let Some((first, key_arc)) = keys.get_mut(entry.key) {
            let shared_section = entry
                .section
                .map(|s| Arc::clone(section_arc.get_or_insert_with(|| Arc::from(s))));
            let shared_key = Arc::clone(key_arc.get_or_insert_with(|| Arc::from(entry.key)));
            scan.push(DuplicateKey {
                key: shared_key,
                canonical_key: None,
                section: shared_section,
                duplicate: entry.key_span,
                first: *first,
            });
        } else {
            keys.insert(entry.key, (entry.key_span, None));
        }
    })?;
    Ok(scan)
}

// ---------------------------------------------------------------------------
// HCL
// ---------------------------------------------------------------------------

fn hcl_static_object_key(key: &hcl::edit::expr::ObjectKey) -> Option<&str> {
    match key {
        hcl::edit::expr::ObjectKey::Ident(ident) => Some(ident.as_str()),
        hcl::edit::expr::ObjectKey::Expression(expr) => hcl_static_key_expr(expr),
    }
}

fn hcl_static_key_expr(mut expr: &hcl::edit::expr::Expression) -> Option<&str> {
    loop {
        match expr {
            hcl::edit::expr::Expression::String(s) => return Some(s.as_str()),
            hcl::edit::expr::Expression::Parenthesis(p) => expr = p.inner(),
            _ => return None,
        }
    }
}

fn scan_hcl_expr(expr: &hcl::edit::expr::Expression, offset: usize, scan: &mut DuplicateScan) {
    use hcl::edit::Span as _;
    use hcl::edit::expr::Expression as E;

    let mut pending: Vec<&E> = vec![expr];
    while let Some(cur) = pending.pop() {
        match cur {
            E::Object(obj) => {
                let mut seen: HashMap<&str, ByteSpan> = HashMap::with_capacity(obj.len());
                let mut children: Vec<&E> = Vec::with_capacity(obj.len() * 2);
                for (key, value) in obj {
                    if let Some(name) = hcl_static_object_key(key) {
                        let key_span = key.span().map_or_else(
                            || ByteSpan::new(offset, offset),
                            |r| ByteSpan::new(r.start + offset, r.end + offset),
                        );
                        if let Some(&first) = seen.get(name) {
                            if !scan.capped {
                                scan.push(DuplicateKey {
                                    key: Arc::from(name),
                                    canonical_key: None,
                                    section: None,
                                    duplicate: key_span,
                                    first,
                                });
                            }
                        } else {
                            seen.insert(name, key_span);
                        }
                    } else if let hcl::edit::expr::ObjectKey::Expression(kexpr) = key {
                        children.push(kexpr);
                    }
                    children.push(value.expr());
                }
                for child in children.into_iter().rev() {
                    pending.push(child);
                }
            }
            E::Array(arr) => {
                let elems: Vec<&E> = arr.iter().collect();
                for elem in elems.into_iter().rev() {
                    pending.push(elem);
                }
            }
            E::Parenthesis(p) => pending.push(p.inner()),
            E::Conditional(c) => {
                pending.push(&c.false_expr);
                pending.push(&c.true_expr);
                pending.push(&c.cond_expr);
            }
            E::UnaryOp(u) => pending.push(&u.expr),
            E::BinaryOp(b) => {
                pending.push(&b.rhs_expr);
                pending.push(&b.lhs_expr);
            }
            E::FuncCall(f) => {
                let args: Vec<&E> = f.args.iter().collect();
                for arg in args.into_iter().rev() {
                    pending.push(arg);
                }
            }
            E::ForExpr(f) => {
                if let Some(cond) = &f.cond {
                    pending.push(&cond.expr);
                }
                pending.push(&f.value_expr);
                if let Some(kexpr) = &f.key_expr {
                    pending.push(kexpr);
                }
                pending.push(&f.intro.collection_expr);
            }
            E::Traversal(t) => {
                for op in t.operators.iter().rev() {
                    if let hcl::edit::expr::TraversalOperator::Index(idx_expr) = op.value() {
                        pending.push(idx_expr);
                    }
                }
                pending.push(&t.expr);
            }
            E::StringTemplate(t) => push_hcl_template_exprs(t, &mut pending),
            E::HeredocTemplate(t) => push_hcl_template_exprs(&t.template, &mut pending),
            E::Null(_) | E::Bool(_) | E::Number(_) | E::String(_) | E::Variable(_) => {}
        }
    }
}

fn push_hcl_template_exprs<'a>(
    template: &'a hcl::edit::template::Template,
    pending: &mut Vec<&'a hcl::edit::expr::Expression>,
) {
    use hcl::edit::template::{Directive, Element};

    enum Work<'a> {
        Template(&'a hcl::edit::template::Template),
        Elem(&'a Element),
        Expr(&'a hcl::edit::expr::Expression),
    }

    let mut work = vec![Work::Template(template)];
    let mut exprs: Vec<&'a hcl::edit::expr::Expression> = Vec::new();
    while let Some(item) = work.pop() {
        match item {
            Work::Template(tpl) => {
                let elems: Vec<&Element> = tpl.into_iter().collect();
                for elem in elems.into_iter().rev() {
                    work.push(Work::Elem(elem));
                }
            }
            Work::Elem(Element::Literal(_)) => {}
            Work::Elem(Element::Interpolation(interp)) => exprs.push(&interp.expr),
            Work::Elem(Element::Directive(dir)) => match dir.as_ref() {
                Directive::If(if_dir) => {
                    if let Some(else_expr) = &if_dir.else_expr {
                        work.push(Work::Template(&else_expr.template));
                    }
                    work.push(Work::Template(&if_dir.if_expr.template));
                    work.push(Work::Expr(&if_dir.if_expr.cond_expr));
                }
                Directive::For(for_dir) => {
                    work.push(Work::Template(&for_dir.for_expr.template));
                    work.push(Work::Expr(&for_dir.for_expr.collection_expr));
                }
            },
            Work::Expr(expr) => exprs.push(expr),
        }
    }
    for expr in exprs.into_iter().rev() {
        pending.push(expr);
    }
}

fn scan_hcl(text: &str) -> Result<DuplicateScan, String> {
    Format::Hcl.parse(text)?;
    if text
        .trim_matches(|c: char| c == '\u{feff}' || c.is_whitespace())
        .is_empty()
    {
        return Ok(DuplicateScan::default());
    }
    let stripped = text.trim_start_matches('\u{feff}');
    let offset = text.len() - stripped.len();
    let body = crate::structured_format::parse_hcl_edit_body(stripped)?;
    let mut scan = DuplicateScan::default();
    let mut bodies: Vec<&hcl::edit::structure::Body> = vec![&body];
    while let Some(cur) = bodies.pop() {
        let mut child_bodies = Vec::new();
        for item in cur {
            match item {
                hcl::edit::structure::Structure::Attribute(attr) => {
                    scan_hcl_expr(&attr.value, offset, &mut scan);
                }
                hcl::edit::structure::Structure::Block(block) => {
                    child_bodies.push(&block.body);
                }
            }
        }
        for child in child_bodies.into_iter().rev() {
            bodies.push(child);
        }
    }
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
    fn yaml_custom_tags_stay_distinct_and_invalid_core_tags_and_bad_aliases_fail() {
        // Distinct custom tags and unknown core tags must not collide with each
        // other or with untagged keys.
        let tagged = "\
!foo bar: 1
!baz bar: 2
bar: 3
!!binary abc: 4
abc: 5
!Tag 1: 6
!Tag \"1\": 7
";
        assert_eq!(
            find_duplicate_keys(Format::Yaml, tagged, false)
                .unwrap()
                .duplicates,
            []
        );
        // Same custom tag + equivalent untagged payload DOES collide.
        let dup_tagged = "!Tag 1: a\n!Tag 0x1: b\n";
        let scan = find_duplicate_keys(Format::Yaml, dup_tagged, false).unwrap();
        assert_eq!(scan.duplicates.len(), 1);

        // Invalid explicit core tags fail in both key and value positions.
        assert!(find_duplicate_keys(Format::Yaml, "!!int abc: 1\n", false).is_err());
        assert!(find_duplicate_keys(Format::Yaml, "a: !!bool maybe\n", false).is_err());
        assert!(find_duplicate_keys(Format::Yaml, "b: !!null nope\n", false).is_err());
        assert!(find_duplicate_keys(Format::Yaml, "c: !!float xyz\n", false).is_err());

        // Undefined and cyclic aliases fail in all positions.
        assert!(find_duplicate_keys(Format::Yaml, "a: *missing\n", false).is_err());
        assert!(find_duplicate_keys(Format::Yaml, "*missing: 1\n", false).is_err());
        assert!(find_duplicate_keys(Format::Yaml, "- *missing\n", false).is_err());
        assert!(find_duplicate_keys(Format::Yaml, "a: &k foo\n---\n*k: 1\n", false).is_err());
        assert!(find_duplicate_keys(Format::Yaml, "a: &loop [*loop]\n", false).is_err());
        assert!(find_duplicate_keys(Format::Yaml, "a: &loop { k: *loop }\n", false).is_err());
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
    fn json_nested_jsonc_source_order_interleaved_bom_and_strict_mode() {
        let jsonc =
            "{\n  // comment\n  \"a\": 1,\n  \"arr\": [{\"x\": 1, \"x\": 2,}],\n  \"a\": 3,\n}\n";
        let tolerant = find_duplicate_keys(Format::Json, jsonc, false).unwrap();
        // Source order: line 4 (`"x"`) precedes line 5 (`"a"`).
        let keys: Vec<&str> = tolerant.duplicates.iter().map(|d| d.key.as_ref()).collect();
        assert_eq!(keys, ["x", "a"]);
        assert!(tolerant.duplicates[0].duplicate.start < tolerant.duplicates[1].duplicate.start);
        // Tolerant mode agrees with Format::Json on whitespace + BOM empty files.
        assert_eq!(
            find_duplicate_keys(Format::Json, " \u{feff}", false)
                .unwrap()
                .duplicates,
            []
        );
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
    fn dotenv_properties_and_ini_duplicates_and_bom_safety() {
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

        // Whitespace + BOM INI must not panic.
        assert_eq!(
            find_duplicate_keys(Format::Ini, " \u{feff}", false)
                .unwrap()
                .duplicates,
            []
        );
        assert_eq!(
            find_duplicate_keys(Format::Ini, "\u{feff} \u{feff}\n", false)
                .unwrap()
                .duplicates,
            []
        );
    }

    #[test]
    fn toml_hcl_mixed_object_keys_and_xml_rejected() {
        assert!(find_duplicate_keys(Format::Toml, "a = 1\na = 2\n", false).is_err());
        assert_eq!(
            find_duplicate_keys(Format::Toml, "a = 1\nb = 2\n", false)
                .unwrap()
                .duplicates,
            []
        );
        assert!(find_duplicate_keys(Format::Hcl, "a = 1\na = 2\n", false).is_err());
        // Mixed ident / quoted / parenthesized HCL object keys are caught with spans.
        let hcl_mixed = "x = {\n  foo = 1\n  \"foo\" = 2\n  (\"foo\") = 3\n}\n";
        let scan = find_duplicate_keys(Format::Hcl, hcl_mixed, false).unwrap();
        assert_eq!(scan.duplicates.len(), 2);
        assert_eq!(scan.duplicates[0].key.as_ref(), "foo");
        assert_eq!(scan.duplicates[1].key.as_ref(), "foo");
        assert!(scan.duplicates[0].duplicate.start > scan.duplicates[0].first.start);
        // Mixed duplicate object keys inside HCL template interpolations,
        // directives, heredocs, and traversal index expressions are caught too.
        let hcl_nested = concat!(
            "s = \"${jsonencode({ a = 1, \"a\" = 2 })}\"\n",
            "t = a[{ b = 1, \"b\" = 2 }]\n",
            "h = <<EOF\n%{if true}${jsonencode({ c = 1, \"c\" = 2 })}%{endif}\nEOF\n",
        );
        let nested_scan = find_duplicate_keys(Format::Hcl, hcl_nested, false).unwrap();
        let nested_keys: Vec<&str> = nested_scan
            .duplicates
            .iter()
            .map(|d| d.key.as_ref())
            .collect();
        assert_eq!(nested_keys, ["a", "b", "c"]);
        assert!(find_duplicate_keys(Format::Xml, "<r><a/><a/></r>", false).is_err());
    }

    #[test]
    fn large_ini_section_and_aliased_yaml_keys_share_allocations_and_hash_once() {
        use std::fmt::Write as _;

        // Large INI section with thousands of unique + repeated keys must borrow
        // from input and share one Arc<str> per duplicated section/key.
        let big_sec = "s".repeat(64 * 1024);
        let mut ini = format!("[{big_sec}]\n");
        for i in 0..2_000 {
            let _ = writeln!(ini, "k{i} = 1");
        }
        for _ in 0..200 {
            ini.push_str("k0 = 2\n");
        }
        let ini_scan = find_duplicate_keys(Format::Ini, &ini, false).unwrap();
        assert_eq!(ini_scan.duplicates.len(), 200);
        let first_sec = ini_scan.duplicates[0].section.as_ref().unwrap();
        let first_key = &ini_scan.duplicates[0].key;
        for dup in &ini_scan.duplicates[1..] {
            assert!(Arc::ptr_eq(first_sec, dup.section.as_ref().unwrap()));
            assert!(Arc::ptr_eq(first_key, &dup.key));
        }

        // Large anchored YAML scalar aliased as a mapping key 1,050 times must
        // share one pre-hashed bounded canonical_key (71-byte sha256:...)
        // instead of allocating 1,000 copies of the raw key.
        let big_key = "k".repeat(256 * 1024);
        let mut yaml = format!("anchor_holder: &a \"{big_key}\"\n*a: 1\n");
        for _ in 0..(MAX_DUPLICATES_PER_FILE + 50) {
            yaml.push_str("*a: 1\n");
        }
        let yaml_scan = find_duplicate_keys(Format::Yaml, &yaml, false).unwrap();
        assert_eq!(yaml_scan.duplicates.len(), MAX_DUPLICATES_PER_FILE);
        assert!(yaml_scan.capped);
        let first_canon = yaml_scan.duplicates[0].canonical_key.as_ref().unwrap();
        assert!(
            first_canon.starts_with("sha256:") && first_canon.len() == 71,
            "expected bounded sha256 canonical key, got len={}",
            first_canon.len()
        );
        for dup in &yaml_scan.duplicates[1..] {
            assert!(Arc::ptr_eq(
                first_canon,
                dup.canonical_key.as_ref().unwrap()
            ));
            assert!(Arc::ptr_eq(&yaml_scan.duplicates[0].key, &dup.key));
        }

        // Large anchored YAML scalar aliased across many distinct mappings
        // (both zero-duplicate and duplicate cases) must pre-hash once and
        // share a single document-wide bounded canonical token via Arc::ptr_eq.
        let mut multi_map_clean = format!("anchor_holder: &a \"{big_key}\"\nitems:\n");
        for _ in 0..2_000 {
            multi_map_clean.push_str("  - *a: 1\n");
        }
        let clean_scan = find_duplicate_keys(Format::Yaml, &multi_map_clean, false).unwrap();
        assert_eq!(clean_scan.duplicates, []);

        let mut multi_map_dup = format!("anchor_holder: &a \"{big_key}\"\nitems:\n");
        for _ in 0..500 {
            multi_map_dup.push_str("  - *a: 1\n    *a: 2\n");
        }
        let multi_dup_scan = find_duplicate_keys(Format::Yaml, &multi_map_dup, false).unwrap();
        assert_eq!(multi_dup_scan.duplicates.len(), 500);
        let shared_canon = multi_dup_scan.duplicates[0].canonical_key.as_ref().unwrap();
        for dup in &multi_dup_scan.duplicates[1..] {
            assert!(Arc::ptr_eq(
                shared_canon,
                dup.canonical_key.as_ref().unwrap()
            ));
        }

        // Large INI section (512 KiB) with 5,000 keys hits the 1-slot
        // active-section fast path without re-hashing the section per line.
        let huge_sec = "z".repeat(512 * 1024);
        let mut big_ini = format!("[{huge_sec}]\n");
        for i in 0..5_000 {
            let _ = writeln!(big_ini, "key_{i} = 1");
        }
        let big_ini_scan = find_duplicate_keys(Format::Ini, &big_ini, false).unwrap();
        assert_eq!(big_ini_scan.duplicates, []);
    }

    #[test]
    fn hcl_template_directives_report_duplicates_in_true_source_order() {
        let hcl = concat!(
            "s = <<EOF\n",
            "%{if length({ c1 = 1, \"c1\" = 2 }) > 0}${jsonencode({ d1 = 1, \"d1\" = 2 })}%{else}${jsonencode({ e1 = 1, \"e1\" = 2 })}%{endif}",
            "${jsonencode({ after = 1, \"after\" = 2 })}",
            "%{for x in [jsonencode({ f1 = 1, \"f1\" = 2 })]}${jsonencode({ body = 1, \"body\" = 2 })}%{endfor}\n",
            "EOF\n",
        );
        let scan = find_duplicate_keys(Format::Hcl, hcl, false).unwrap();
        let keys: Vec<&str> = scan.duplicates.iter().map(|d| d.key.as_ref()).collect();
        assert_eq!(keys, ["c1", "d1", "e1", "after", "f1", "body"]);
        for pair in scan.duplicates.windows(2) {
            assert!(
                pair[0].duplicate.start < pair[1].duplicate.start,
                "expected strictly ascending source spans: {pair:?}"
            );
        }
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
