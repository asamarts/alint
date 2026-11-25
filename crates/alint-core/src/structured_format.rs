//! Structured-document parsing shared by the structured-query rule
//! family (`{json,yaml,toml,xml,dotenv,properties,ini,hcl}_path_*`) and core-side predicates
//! that need to read a config / manifest into a `serde_json::Value`
//! tree.
//!
//! [`Format`] parses JSON / YAML / TOML / XML / dotenv / properties / INI / HCL
//! into one uniform `serde_json::Value` shape (YAML and TOML coerce through
//! serde; XML maps via the xmltodict-style convention in `xml_to_value` —
//! `@attr` / `#text` / repeated-element→array, leaf elements collapse to their
//! text string, and namespaces flatten to local names; dotenv is a flat map
//! whose assigned values are literal strings and whose bare declarations are
//! `null`; Java `.properties` is a flat literal-string map; INI is a 2-level
//! `{ section: { key: value } }` map of the same, with pre-section keys hoisted
//! to the top level; HCL maps via `hcl-rs`, JSON-native — blocks nest by type +
//! labels, values keep their type, and an unevaluated expression is an opaque
//! string), so a single `JSONPath`
//! engine only ever has to reason about one tree shape. XML design +
//! open-question resolutions: `docs/design/v0.10/xml_path.md`.

use serde::Serialize;
use serde_json::Value;

mod hcl;
mod xml_scan;
mod yaml_value;

use hcl::hcl_to_value;
pub(crate) use hcl::parse_edit_body as parse_hcl_edit_body;
#[cfg(test)]
use xml_scan::MAX_XML_ATTRS_PER_ELEMENT;
pub use xml_scan::MAX_XML_DEPTH;
pub(crate) use xml_scan::xml_within_parse_limits;
pub(crate) use yaml_value::json_seed;

/// Maximum INPUT size any structured format will parse into a value tree. The read
/// cap [`crate::walker::MAX_ANALYZE_BYTES`] (256 MiB) bounds bytes, but a parse
/// costs far more RSS than its input: measured up to ~30x the input for
/// attribute-heavy XML (roxmltree builds its own arena alongside the
/// `serde_json::Value` tree), ~6-10x for JSON, and ~24-48x for YAML with
/// ayaml 0.2.2's streaming reader. At this cap one YAML parse peaked at 1.54 GiB
/// for a `[1,1,...]` flow sequence, 764 MiB for `kN: 1` lines and 401 MiB for a
/// 16 MiB flow mapping (down from 6.2 GB, 4.5 GB and 5.0 GB on ayaml 0.1.2,
/// ADR-0042). Lossless dotenv keeps
/// tighter byte and physical-line budgets because retained items and decoded
/// escape maps amplify input. Capping the structured-parse INPUT at 32 MiB, far
/// above any real config / manifest (they are KB-MB; even a large `OpenAPI` spec
/// or SBOM is well under 32 MiB) yet 8x tighter than the read cap, bounds one
/// parse to under ~1.6 GiB. An oversize structured file is one per-file
/// parse-error violation. Both HCL (`MAX_HCL_BYTES`) and dotenv keep a 4 MiB
/// lossless-CST cap (plus dotenv's physical-line budget and HCL's per-scope
/// width and block-label bounds); this is the ceiling for the rest. The
/// per-file rule dispatch is parallel, so several files parse at
/// once: YAML inputs over 1 MiB therefore share a process-wide budget of this
/// many in-flight input bytes, so at most one maximal YAML file parses at a time
/// (`yaml_budget`). For the other formats, aggregate concurrent parse
/// memory (this cap times the `par_iter` fan-out) is a coarser bound tracked in
/// `docs/design/format-coverage.md`.
pub const MAX_STRUCTURED_BYTES: usize = 32 * 1024 * 1024;

/// The structured-parse cap must stay meaningfully below the read cap, or it does
/// nothing (a file that passes the read cap would also pass this and still balloon).
const _: () = assert!((MAX_STRUCTURED_BYTES as u64) < crate::walker::MAX_ANALYZE_BYTES / 2);

/// Which config format the target file is parsed as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Json,
    Yaml,
    Toml,
    Xml,
    Dotenv,
    Properties,
    Ini,
    Hcl,
}

/// A parsed structured document shared across rules via `Arc`.
///
/// For alias-amplified YAML documents whose alias-replayed expansion exceeds
/// the YAML parse budget threshold, `SharedValue` also retains the tree's share
/// of the YAML parse budget until the last clone drops so parallel workers
/// cannot hold unbounded alias-expanded trees simultaneously. Sharing across
/// threads via `Clone` on the receiving thread or [`crate::Context::parse_structured`]
/// tracks each receiving thread's handle affinity; moving a `SharedValue` to
/// another thread without cloning retains its affinity on the creating thread
/// until dropped, so a receiver should drop a moved handle before starting
/// another budget-sized YAML parse.
pub struct SharedValue {
    // Field order matters: drop `inner` (deallocating the `Value` heap and, on
    // the final handle, releasing `RetainedLease` bytes) before releasing the
    // per-handle thread affinity token.
    inner: std::sync::Arc<SharedValueInner>,
    _holder: Option<crate::yaml_budget::LeaseHolder>,
}

impl Clone for SharedValue {
    fn clone(&self) -> Self {
        let holder = self
            .inner
            .lease
            .as_ref()
            .map(crate::yaml_budget::RetainedLease::register_holder);
        Self {
            inner: std::sync::Arc::clone(&self.inner),
            _holder: holder,
        }
    }
}

pub(crate) struct SharedValueInner {
    value: Value,
    lease: Option<crate::yaml_budget::RetainedLease>,
}

impl std::fmt::Debug for SharedValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedValue")
            .field("value", &self.inner.value)
            .field("retained_lease", &self.inner.lease.is_some())
            .finish_non_exhaustive()
    }
}

impl std::ops::Deref for SharedValue {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        &self.inner.value
    }
}

impl AsRef<Value> for SharedValue {
    fn as_ref(&self) -> &Value {
        &self.inner.value
    }
}

impl SharedValue {
    /// Wrap an already-parsed value with no retained budget lease.
    #[must_use]
    pub fn new(value: Value) -> Self {
        Self::with_lease(value, None)
    }

    pub(crate) fn with_lease(
        value: Value,
        lease: Option<crate::yaml_budget::RetainedLease>,
    ) -> Self {
        let holder = lease
            .as_ref()
            .map(crate::yaml_budget::RetainedLease::register_holder);
        Self {
            inner: std::sync::Arc::new(SharedValueInner { value, lease }),
            _holder: holder,
        }
    }

    pub(crate) fn from_inner(inner: std::sync::Arc<SharedValueInner>) -> Self {
        let holder = inner
            .lease
            .as_ref()
            .map(crate::yaml_budget::RetainedLease::register_holder);
        Self {
            inner,
            _holder: holder,
        }
    }

    pub(crate) fn downgrade(&self) -> std::sync::Weak<SharedValueInner> {
        std::sync::Arc::downgrade(&self.inner)
    }

    /// Whether this tree retains a YAML alias-expansion budget lease.
    pub(crate) fn has_retained_lease(&self) -> bool {
        self.inner.lease.is_some()
    }

    /// Whether two handles point to the exact same shared allocation.
    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.inner, &other.inner)
    }
}

/// Iterative upper-bound estimate of heap bytes retained by a `serde_json::Value`
/// tree, used to bound the per-run structured-tree cache.
pub(crate) fn estimate_value_bytes(root: &Value) -> usize {
    let mut total = std::mem::size_of::<SharedValueInner>();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        match node {
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
            Value::String(text) => {
                total = total.saturating_add(text.capacity());
            }
            Value::Array(items) => {
                total = total.saturating_add(
                    items
                        .capacity()
                        .saturating_mul(std::mem::size_of::<Value>()),
                );
                for item in items {
                    stack.push(item);
                }
            }
            Value::Object(entries) => {
                // Account for the map's BTree/IndexMap node overhead plus each
                // owned key string and value slot.
                const PER_ENTRY_OVERHEAD: usize =
                    std::mem::size_of::<String>() + std::mem::size_of::<Value>() + 48;
                total = total.saturating_add(entries.len().saturating_mul(PER_ENTRY_OVERHEAD));
                for (key, value) in entries {
                    total = total.saturating_add(key.capacity());
                    stack.push(value);
                }
            }
        }
    }
    total
}

impl Format {
    /// The canonical inventory of config formats and the single source of truth
    /// for the config-format axis. Format-specific surfaces (`extract`, the
    /// structured-query kinds, `json_schema_passes`, the `did_you_mean` hints)
    /// should cover every variant; a per-surface parity test asserts that as each
    /// surface is leveled (today `extract_spec_covers_every_format`; the rest land
    /// with the format-coverage rollout). `format_all_is_complete` guards this
    /// list itself. See `docs/design/format-coverage.md`.
    pub const ALL: &'static [Format] = &[
        Format::Json,
        Format::Yaml,
        Format::Toml,
        Format::Xml,
        Format::Dotenv,
        Format::Properties,
        Format::Ini,
        Format::Hcl,
    ];

    /// Whether values read from this format are secret-bearing by default.
    /// Every extraction and structured-query surface delegates to this single
    /// classification so a future format change cannot redact on one path and
    /// leak on another.
    #[must_use]
    pub fn redacts_by_default(self) -> bool {
        matches!(self, Self::Dotenv | Self::Properties)
    }

    pub fn parse(self, text: &str) -> std::result::Result<Value, String> {
        self.parse_with_strict_json(text, false)
    }

    /// Like [`Format::parse`], enforcing strict RFC 8259 JSON semantics when
    /// `self == Format::Json && strict_json` (rejecting comments, trailing
    /// commas, leading UTF-8 BOMs, and empty/whitespace-only documents).
    ///
    /// # Errors
    ///
    /// Returns the same format-specific parse or limit error message as
    /// [`Format::parse`].
    pub fn parse_with_strict_json(
        self,
        text: &str,
        strict_json: bool,
    ) -> std::result::Result<Value, String> {
        self.parse_holding(text, &mut 0, false, strict_json)
            .map(|(value, _)| value)
    }

    /// Parse `text` into a [`SharedValue`] that can be shared across rules via
    /// `Arc` and retains the YAML alias-expansion budget lease for as long as
    /// an alias-amplified tree stays alive.
    ///
    /// # Errors
    ///
    /// Returns the same format-specific parse or limit error message as
    /// [`Format::parse`].
    pub fn parse_shared(self, text: &str) -> std::result::Result<SharedValue, String> {
        self.parse_shared_with_strict_json(text, false)
    }

    /// Like [`Format::parse_shared`], enforcing strict RFC 8259 JSON semantics
    /// when `self == Format::Json && strict_json` (ADR-0050 D7).
    ///
    /// # Errors
    ///
    /// Returns the same format-specific parse or limit error message as
    /// [`Format::parse_with_strict_json`].
    pub fn parse_shared_with_strict_json(
        self,
        text: &str,
        strict_json: bool,
    ) -> std::result::Result<SharedValue, String> {
        let (value, lease) = self.parse_holding(text, &mut 0, true, strict_json)?;
        Ok(SharedValue::with_lease(value, lease))
    }

    /// [`Format::parse_shared`] with optional strict RFC 8259 JSON mode, and
    /// roughly the most the parse held at once besides `text` and the tree it
    /// returns: a JSONC text's copy with its comments and trailing commas
    /// dropped (none for strict JSON), and the tokens the TOML lexer reserves
    /// room for. Retains the YAML alias-tree lease until the returned
    /// [`SharedValue`] drops.
    pub(crate) fn parse_held_with_strict_json(
        self,
        text: &str,
        strict_json: bool,
    ) -> std::result::Result<(SharedValue, usize), String> {
        let mut held = 0;
        let (value, lease) = self.parse_holding(text, &mut held, true, strict_json)?;
        Ok((SharedValue::with_lease(value, lease), held))
    }

    fn parse_holding(
        self,
        text: &str,
        held: &mut usize,
        retain_lease: bool,
        strict_json: bool,
    ) -> std::result::Result<(Value, Option<crate::yaml_budget::RetainedLease>), String> {
        let raw_text = text;
        if self == Self::Json && strict_json {
            // Bound input size BEFORE scanning/parsing so oversize inputs hit
            // the size cap immediately.
            if text.len() > MAX_STRUCTURED_BYTES {
                return Err(format!(
                    "input exceeds the maximum supported size for a structured document ({MAX_STRUCTURED_BYTES} bytes)"
                ));
            }
            // RFC 8259 section 8.1 and Node.js `JSON.parse` reject a leading
            // UTF-8 BOM; strict mode does not strip it and does not short-
            // circuit empty/whitespace files to `{}` or fall back to JSONC.
            if text.starts_with('\u{feff}') {
                return Err(
                    "strict JSON rejects a leading UTF-8 BOM (RFC 8259 section 8.1)".to_string(),
                );
            }
            return serde_json::from_str(text)
                .map(|v| (v, None))
                .map_err(|e| e.to_string());
        }
        // Strip any leading UTF-8 BOM(s) uniformly. Some libraries (serde_json,
        // hcl-rs) reject a `\u{feff}` prefix as a syntax error, so a BOM-prefixed
        // `package.json` / `config.hcl` (common from Windows editors) would
        // false-fire the structured rules; the dotenv/ini/properties parsers
        // already skip it. `trim_start_matches` (not `strip_prefix`) so
        // consecutive leading BOMs are all removed. Flagging a BOM is the `no_bom`
        // rule's job.
        let text = text.trim_start_matches('\u{feff}');
        // Bound the parsed-tree memory FIRST: a parse costs up to ~30x the input in
        // RSS for XML and ~24-48x for YAML (0.75-1.54 GiB at the 32 MiB cap,
        // ADR-0042), so an unbounded file would balloon to multiple GB (see
        // `MAX_STRUCTURED_BYTES`). Reject before
        // building it -- and before the whitespace-emptiness scan below, so an oversize
        // all-whitespace file is a fast size rejection rather than an O(n) trim.
        // (HCL and dotenv enforce their tighter 4 MiB lossless-CST caps below.)
        if text.len() > MAX_STRUCTURED_BYTES {
            return Err(format!(
                "input exceeds the maximum supported size for a structured document ({MAX_STRUCTURED_BYTES} bytes)"
            ));
        }
        // An empty file (nothing but BOMs and whitespace) is "no config", not a
        // broken document: return an empty OBJECT (an empty document) so
        // `*_path_absent` is satisfied, `if_present` stays silent, AND
        // `json_schema_passes` validates it as `{}` -- which satisfies
        // `{"type":"object"}`, since an empty config file IS a valid empty table.
        // (Returning `Value::Null` here made `json_schema_passes` false-fire "null
        // is not of type object" on an empty `.toml`/`.ini`/`.env`/`.properties`/
        // `.hcl`, while a comment-only file of the same format parsed to `{}` and
        // passed -- two "no config" files disagreeing.) Otherwise the JSON / XML
        // libraries reject empty input as a syntax error -- a false positive for the
        // absent family. The emptiness test also strips a stray BOM interleaved with
        // whitespace (e.g. `\u{feff} \u{feff}`), which `trim_start_matches` alone
        // leaves and which YAML/properties/INI would otherwise mis-read as a
        // one-character scalar. Flagging an empty file is the `no_empty_files` rule's
        // job. (TOML / dotenv / INI / properties already parse empty to `{}`; YAML's
        // native empty is `null`, but one uniform empty document is less surprising
        // than per-format null-vs-object.)
        if text
            .trim_matches(|c: char| c == '\u{feff}' || c.is_whitespace())
            .is_empty()
        {
            return Ok((Value::Object(serde_json::Map::new()), None));
        }
        match self {
            // Try strict JSON first (the common, fast path — plain
            // JSON is byte-for-byte unchanged). Only on failure retry
            // tolerating JSONC: `//` + `/* */` comments and trailing
            // commas, which the JS/TS ecosystem uses pervasively in
            // `.json` files (tsconfig.json, `.vscode/*.json`). If the
            // tolerant retry also fails, surface the *original* strict
            // error so genuinely-broken JSON reports accurately.
            Self::Json => serde_json::from_str(text)
                .or_else(|strict_err| {
                    let stripped = strip_jsonc(text);
                    *held = stripped.capacity();
                    serde_json::from_str(&stripped).map_err(|_| strict_err.to_string())
                })
                .map(|v| (v, None)),
            Self::Yaml => {
                // Wait for this input's share of the YAML parse budget, held
                // until the parse below ends, the guards' passes included
                // (`yaml_budget`, ADR-0042).
                let budget = crate::yaml_budget::reserve(text.len());
                // Bound flow-nesting before any parser reads it (the parser v0.22
                // used hung on a crafted `[[[…` file; ayaml stops at the same 128
                // levels itself, counting block levels too) -- the YAML analogue
                // of the `xml_within_parse_limits` guard below. JSON (serde_json,
                // 128-deep) and TOML (toml, 80-deep) carry their own limits.
                let yaml_guards = crate::yaml_depth::scan_guards(text);
                if !yaml_guards.flow_within_limit() {
                    return Err(format!(
                        "YAML flow nesting exceeds the maximum supported depth ({})",
                        crate::yaml_depth::MAX_YAML_FLOW_DEPTH
                    ));
                }
                // Bound ALIAS expansion: a single anchor referenced many times
                // (`*a` x N -> N x anchor-size nodes) balloons a small file into
                // millions of nodes. Written for the parser v0.22 used, whose
                // limits missed it; ayaml charges an alias budget itself, which
                // this guard's is stricter than, and this guard stays as defense
                // in depth (`yaml_depth`). Cheap discard-only pre-count;
                // alias-free text short-circuits for free.
                let (expansion, expanded, alias_replayed) =
                    yaml_guards.expansion_and_alias_sizes(text);
                if !expansion.within_limit_for_ayaml() {
                    return Err(
                        "YAML alias expansion exceeds the maximum supported size".to_string()
                    );
                }
                let retained = if retain_lease {
                    crate::yaml_budget::retained_alias_cost(alias_replayed)
                } else {
                    0
                };
                // The value copies what each alias names, so a small file of
                // many aliases costs what its expanded tree does: its share of
                // the budget grows to that before the parse.
                let budget = crate::yaml_budget::reserve_at_least(
                    budget,
                    crate::yaml_budget::parse_cost(text.len(), expanded).max(retained),
                );
                let value = yaml_value::yaml_to_value(text)?;
                // An alias-amplified tree stays roughly as big as its parse
                // peak, so transition the reservation atomically into a
                // `Send + Sync` lease held with the returned tree (#582). If a
                // re-entrant parse ran without an in-flight reservation, still
                // charge the retained lease so the tree never escapes the
                // budget or enters an idle LRU cache.
                let lease = if retained == 0 {
                    None
                } else if let Some(res) = budget {
                    res.into_retained(retained)
                } else {
                    crate::yaml_budget::retain_unreserved(retained)
                };
                Ok((value, lease))
            }
            Self::Toml => {
                *held = toml_tokens(text);
                toml::from_str(text)
                    .map(|v| (v, None))
                    .map_err(|e| e.to_string())
            }
            Self::Xml => xml_to_value(text).map(|v| (v, None)),
            Self::Dotenv => crate::dotenv::parse(raw_text).map(|v| (v, None)),
            Self::Properties => properties_to_value(raw_text).map(|v| (v, None)),
            Self::Ini => crate::ini::parse(text).map(|v| (v, None)),
            Self::Hcl => hcl_to_value(text).map(|v| (v, None)),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Yaml => "YAML",
            Self::Toml => "TOML",
            Self::Xml => "XML",
            Self::Dotenv => "dotenv",
            Self::Properties => "properties",
            Self::Ini => "INI",
            Self::Hcl => "HCL",
        }
    }

    /// Detect the format from a path's extension. Returns `None`
    /// for unknown extensions; callers decide how to fall back
    /// (require an explicit `format:` override, default to JSON,
    /// emit a per-file violation, etc).
    pub fn detect_from_path(path: &std::path::Path) -> Option<Self> {
        // Extension detection first, case-INSENSITIVELY: `.JSON` / `.CSPROJ` /
        // `.XML` (common on Windows and case-insensitive filesystems, or from Java
        // tooling) are valid and must not yield a false "could not detect format".
        // A KNOWN format extension also wins over the `.env` family below, so a
        // `.env.json` / `.env.yaml` is parsed by its real format, not shadowed as
        // dotenv.
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            match ext.to_ascii_lowercase().as_str() {
                "json" => return Some(Self::Json),
                "yaml" | "yml" => return Some(Self::Yaml),
                "toml" => return Some(Self::Toml),
                "properties" => return Some(Self::Properties),
                "ini" | "cfg" => return Some(Self::Ini),
                "hcl" | "tf" | "tfvars" | "nomad" => return Some(Self::Hcl),
                "xml" | "csproj" | "props" | "targets" | "vbproj" | "fsproj" | "nuspec" => {
                    return Some(Self::Xml);
                }
                _ => {}
            }
        }
        // Well-known config files identified by NAME, not extension: extension-less
        // (`.editorconfig`, `Pipfile`) or a `.config` that is specifically .NET XML
        // (a bare `.config` extension is not universally XML, so match exact names).
        // Only UNAMBIGUOUS names are mapped -- `.eslintrc`/`.prettierrc` are skipped
        // because they may be JSON OR YAML, so they still need an explicit `format:`.
        // dotenv is likewise filename-based: a bare `.env` has no extension, and
        // `.env.local` / `.env.production` carry the environment where an extension
        // would be. The `.env` family matches by name but only for a suffix that
        // ISN'T a known format extension (handled above), so `.env.json` is JSON.
        // All case-folded (`.ENV`, `WEB.CONFIG`, ...).
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            let lname = name.to_ascii_lowercase();
            match lname.as_str() {
                ".editorconfig" => return Some(Self::Ini),
                "pipfile" => return Some(Self::Toml),
                "web.config" | "app.config" => return Some(Self::Xml),
                _ => {}
            }
            if lname == ".env" || lname.starts_with(".env.") {
                return Some(Self::Dotenv);
            }
        }
        None
    }
}

/// Make a JSONC document parseable as strict JSON: drop `//` and
/// `/* … */` comments and trailing commas (a `,` immediately before a
/// `]` / `}`). String-aware — markers inside `"…"` (with `\` escapes)
/// are preserved, so a `"https://…"` URL or a `","` literal is
/// untouched. Only invoked when strict parsing already failed, so
/// plain JSON never pays for it.
///
/// One pass into one copy of the text: a comma outside a string waits
/// while only whitespace follows it (a comment drops out as it goes, a
/// line comment leaving its line break), and a `]` or `}` next drops it,
/// keeping the whitespace.
fn strip_jsonc(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    // Where the comma waiting for a `]` or `}` sits in `out`.
    let mut comma: Option<usize> = None;
    let mut chars = src.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '/' if chars.peek() == Some(&'/') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                        break;
                    }
                }
                continue;
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
                continue;
            }
            ',' => {
                comma = Some(out.len());
                out.push(c);
                continue;
            }
            ']' | '}' => {
                if let Some(at) = comma.take() {
                    out.remove(at);
                }
            }
            c if c.is_whitespace() => {
                out.push(c);
                continue;
            }
            '"' => {
                in_string = true;
                comma = None;
            }
            _ => comma = None,
        }
        out.push(c);
    }
    out
}

/// Roughly what the TOML lexer holds while a document of `text` is parsed:
/// room for two tokens per seven bytes of text, which it reserves up front,
/// each token a kind and a byte span (three words).
pub(crate) fn toml_tokens(text: &str) -> usize {
    (text.len() / 7) * 2 * 3 * std::mem::size_of::<usize>()
}

// ---------------------------------------------------------------
// XML → serde_json::Value
//
// xmltodict-style convention so the JSONPath a user writes reads
// like the XML they see. Full rationale + false-positive surface:
// `docs/design/v0.10/xml_path.md`.
// ---------------------------------------------------------------

/// Compute the exact quote-delimited byte range of `attr`'s value inside
/// `text`, without relying on `roxmltree::Attribute::range_value()`'s
/// saturating `u16` qname length and `u8` `=`-padding length.
pub(crate) fn xml_attr_value_span(
    text: &str,
    attr: roxmltree::Attribute<'_, '_>,
) -> Option<std::ops::Range<usize>> {
    let full = attr.range();
    let slice = text.get(full.clone())?;
    let eq = slice.find('=')?;
    let after_eq = &slice[eq + 1..];
    let q_rel = after_eq.find(['"', '\''])?;
    let quote = after_eq.as_bytes()[q_rel];
    let start = full.start + eq + 1 + q_rel + 1;
    let end = full.end.checked_sub(1)?;
    if slice.as_bytes().last() != Some(&quote)
        || start > end
        || !text.is_char_boundary(start)
        || !text.is_char_boundary(end)
    {
        return None;
    }
    Some(start..end)
}

/// Parse XML into the same `serde_json::Value` tree the rest of
/// the family queries. The document maps to
/// `{ <root-element-name>: <root value> }` so the root element is
/// the first `JSONPath` segment (`$.Project…`, `$.project…`).
fn xml_to_value(text: &str) -> std::result::Result<Value, String> {
    // Reject over-deep, over-wide or namespace-heavy XML upfront before
    // `Document::parse` builds a large DOM (depth) or hits O(n^2) attribute
    // validation (width) or O(P^2) namespace resolution.
    xml_within_parse_limits(text)?;
    let doc = roxmltree::Document::parse(text).map_err(|e| {
        let msg = e.to_string();
        // roxmltree runs with DTD processing disabled (a billion-laughs / XXE
        // safety limit). Make the wholesale rejection actionable instead of the
        // terse upstream "XML with DTD detected".
        if msg.contains("DTD") {
            format!(
                "{msg}: alint parses XML with DTDs disabled for safety, so \
                 DTD-bearing files (Checkstyle, Spring, Ant, log4j) cannot be linted"
            )
        } else {
            msg
        }
    })?;
    let root = doc.root_element();
    let mut obj = serde_json::Map::new();
    obj.insert(
        root.tag_name().name().to_owned(),
        element_to_value(root, 0)?,
    );
    Ok(Value::Object(obj))
}

/// One element → its `Value`. Attributes become `@name` keys;
/// repeated child elements of the same (local) name become a JSON
/// array in document order; loose text becomes `#text` when the
/// element also has attributes/children, or *is* the value when
/// the element is a pure leaf. Empty element → `null`. Namespaces
/// are flattened to the local name (Open question 1 in the design
/// doc). `depth` bounds recursion at `MAX_XML_DEPTH`: past the
/// bound it returns `Err` (surfaced as one parse-error violation
/// via the caller) instead of recursing into a stack abort.
fn element_to_value(node: roxmltree::Node, depth: usize) -> std::result::Result<Value, String> {
    if depth >= MAX_XML_DEPTH {
        return Err(format!(
            "XML nesting exceeds the maximum supported depth ({MAX_XML_DEPTH})"
        ));
    }
    let mut obj = serde_json::Map::new();
    for attr in node.attributes() {
        obj.insert(
            format!("@{}", attr.name()),
            Value::String(attr.value().to_owned()),
        );
    }
    let mut has_child_elem = false;
    for child in node.children().filter(roxmltree::Node::is_element) {
        has_child_elem = true;
        let name = child.tag_name().name().to_owned();
        let val = element_to_value(child, depth + 1)?;
        match obj.get_mut(&name) {
            Some(Value::Array(arr)) => arr.push(val),
            Some(slot) => {
                let prev = slot.take();
                *slot = Value::Array(vec![prev, val]);
            }
            None => {
                obj.insert(name, val);
            }
        }
    }
    let text: String = node
        .children()
        .filter(roxmltree::Node::is_text)
        .filter_map(|n| n.text())
        .collect();
    let text = text.trim();
    if obj.is_empty() && !has_child_elem {
        return Ok(if text.is_empty() {
            Value::Null
        } else {
            Value::String(text.to_owned())
        });
    }
    if !text.is_empty() {
        obj.insert("#text".to_owned(), Value::String(text.to_owned()));
    }
    Ok(Value::Object(obj))
}

/// Java `.properties` -> a flat `{ key: "value" }` object of literal strings
/// (via [`crate::properties`], decoded as UTF-8; handles `=`/`:`/space
/// separators, `#`/`!` comments, backslash line-continuations including EOF,
/// and `\uXXXX` escapes including UTF-16 surrogate pairs). Dotted keys
/// (`a.b.c`) stay ONE opaque key, faithful to Java -- query with `$['a.b.c']`.
/// Values are literal: `${...}` placeholders are resolved by the application,
/// not the file, so they are kept verbatim. Duplicate keys: last wins.
fn properties_to_value(text: &str) -> std::result::Result<Value, String> {
    crate::properties::parse(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ─── JSONC tolerance ──────────────────────────────────────

    #[test]
    fn json_parse_tolerates_jsonc() {
        // tsconfig.json-style: `//` + `/* */` comments and trailing
        // commas. Strict parse fails, the tolerant retry succeeds.
        let jsonc = "{\n  // line comment\n  \"a\": 1, /* block */\n  \"b\": [1, 2,],\n}\n";
        let v = Format::Json.parse(jsonc).expect("JSONC should parse");
        assert_eq!(v["a"], serde_json::json!(1));
        assert_eq!(v["b"], serde_json::json!([1, 2]));
    }

    #[test]
    fn json_parse_preserves_comment_markers_inside_strings() {
        // `//` and `,` inside string values must NOT be stripped.
        let s = "{ \"url\": \"https://x/y\", \"note\": \"a,b\" }";
        let v = Format::Json.parse(s).expect("plain JSON");
        assert_eq!(v["url"], serde_json::json!("https://x/y"));
        assert_eq!(v["note"], serde_json::json!("a,b"));
    }

    #[test]
    fn broken_json_keeps_the_strict_error() {
        // A genuinely-malformed document (not JSONC) must still fail,
        // and report the *strict* parser's message.
        let err = Format::Json.parse("{ \"x\": 1, \"y\" }").unwrap_err();
        assert!(err.contains("expected"), "strict error preserved: {err}");
    }

    #[test]
    fn xml_scan_bounds_attribute_width_to_prevent_quadratic_parse() {
        // `roxmltree` 0.21.2 validates per-element attribute uniqueness in O(n^2), so
        // one element with tens of thousands of attributes is a wall-clock DoS the
        // depth guard can't see. The pre-scan must reject an over-wide element as
        // an ordinary parse error -- NOT hand it to roxmltree to grind on.
        use std::fmt::Write as _;
        let mut wide = String::from("<r");
        for i in 0..(MAX_XML_ATTRS_PER_ELEMENT + 10) {
            write!(wide, " a{i}=\"v\"").unwrap();
        }
        wide.push_str("/>");
        let err = xml_within_parse_limits(&wide).unwrap_err();
        assert!(
            err.contains("attributes"),
            "over-wide element rejected with an attribute message: {err}"
        );
        // And the full parse surfaces it as one parse-error (no hang), not a panic.
        assert!(Format::Xml.parse(&wide).is_err());
        // A normal, even generously-attributed, element is unaffected.
        let mut ok = String::from("<Reference");
        for i in 0..64 {
            write!(ok, " p{i}=\"v\"").unwrap();
        }
        ok.push_str("/>");
        assert!(xml_within_parse_limits(&ok).is_ok());
        // `=` inside a quoted value is NOT an attribute and must not be counted:
        // a single attribute whose value is packed with `=` stays one attribute.
        let equals_in_value = format!("<r a=\"{}\"/>", "=".repeat(5000));
        assert!(xml_within_parse_limits(&equals_in_value).is_ok());
        // Exact boundary: `MAX_XML_ATTRS_PER_ELEMENT` is accepted, one more is not.
        let at_cap: String = format!(
            "<r{}/>",
            (0..MAX_XML_ATTRS_PER_ELEMENT).fold(String::new(), |mut s, i| {
                write!(s, " a{i}=\"v\"").unwrap();
                s
            })
        );
        assert!(
            xml_within_parse_limits(&at_cap).is_ok(),
            "exactly the cap is accepted"
        );
        let over_cap = at_cap.replace("/>", " extra=\"v\"/>");
        assert!(
            xml_within_parse_limits(&over_cap).is_err(),
            "one over the cap is rejected"
        );
    }

    #[test]
    fn xml_dtd_is_rejected_with_an_actionable_message() {
        // roxmltree runs with DTD processing disabled (billion-laughs / XXE
        // safety); the wholesale rejection must say WHY, not just the terse
        // upstream "XML with DTD detected".
        let err = Format::Xml
            .parse("<!DOCTYPE note SYSTEM \"note.dtd\"><note/>")
            .unwrap_err();
        assert!(err.contains("DTD"), "names the cause: {err}");
        assert!(
            err.contains("disabled for safety"),
            "explains the limit: {err}"
        );
    }

    #[test]
    fn xml_to_value_maps_attributes_text_arrays_and_empty() {
        // Pins the xmltodict-style contract (module docs): the root element wraps
        // the tree, attributes are `@name`, a leaf collapses to its text, repeated
        // siblings become an array, loose text alongside attributes/children is
        // `#text`, and an empty element is null. A silent drift here changes what
        // EVERY XML-consuming surface sees (extract, json_schema_passes, xml_path_*).
        let xml = r#"
            <Project Sdk="Microsoft.NET.Sdk">
              <PropertyGroup>
                <TargetFramework>net8.0</TargetFramework>
              </PropertyGroup>
              <ItemGroup>
                <Ref Include="a" />
                <Ref Include="b" />
              </ItemGroup>
              <Empty/>
              <Mixed Attr="x">hello</Mixed>
            </Project>
        "#;
        let v = Format::Xml.parse(xml).expect("parse xml");
        // Root element wraps the tree; attributes become `@name` keys.
        assert_eq!(v["Project"]["@Sdk"], json!("Microsoft.NET.Sdk"));
        // A leaf element collapses to its text string.
        assert_eq!(
            v["Project"]["PropertyGroup"]["TargetFramework"],
            json!("net8.0")
        );
        // Repeated siblings become a document-order array of objects.
        assert_eq!(
            v["Project"]["ItemGroup"]["Ref"],
            json!([{"@Include": "a"}, {"@Include": "b"}])
        );
        // An empty element is null (NOT an empty object/string).
        assert_eq!(v["Project"]["Empty"], Value::Null);
        // Text alongside an attribute is `#text` -- the element is an object, not
        // the bare string "hello" (the `Condition=`-attribute reshaping footgun).
        assert_eq!(
            v["Project"]["Mixed"],
            json!({"@Attr": "x", "#text": "hello"})
        );
    }

    #[test]
    fn dotenv_files_detect_by_filename() {
        use std::path::Path;
        // dotenv detection is filename-based (a bare `.env` has no extension), the
        // one piece of new control flow in the dotenv work. Positive cases:
        for p in [".env", ".env.local", ".env.production", ".env.example"] {
            assert_eq!(
                Format::detect_from_path(Path::new(p)),
                Some(Format::Dotenv),
                "{p} should detect as dotenv"
            );
        }
        // Near-misses must NOT match (the `.env.` boundary matters: a regression to
        // `contains(\".env\")` would wrongly claim `.environment` / `foo.env`).
        for p in [".environment", "env.local", "foo.env", "envrc", ".envision"] {
            assert_ne!(
                Format::detect_from_path(Path::new(p)),
                Some(Format::Dotenv),
                "{p} must NOT detect as dotenv"
            );
        }
        // A KNOWN format extension after `.env.` wins over the dotenv family: a
        // `.env.json` is JSON-format env config, NOT a flat dotenv map (parsing it
        // with the dotenv parser would silently produce a wrong tree).
        for (p, want) in [
            (".env.json", Format::Json),
            (".env.yaml", Format::Yaml),
            (".env.toml", Format::Toml),
        ] {
            assert_eq!(
                Format::detect_from_path(Path::new(p)),
                Some(want),
                "{p} should detect by its real extension, not be shadowed as dotenv"
            );
        }
    }

    #[test]
    fn format_detection_is_case_insensitive() {
        use std::path::Path;
        // An uppercase extension (Windows / case-insensitive FS / Java tooling) is
        // valid and must detect, not yield a false "could not detect format".
        for (p, want) in [
            ("Config.JSON", Format::Json),
            ("data.YAML", Format::Yaml),
            ("app.YML", Format::Yaml),
            ("Cargo.TOML", Format::Toml),
            ("App.CSPROJ", Format::Xml),
            ("build.XML", Format::Xml),
            ("main.TF", Format::Hcl),
            ("settings.INI", Format::Ini),
            ("db.PROPERTIES", Format::Properties),
        ] {
            assert_eq!(
                Format::detect_from_path(Path::new(p)),
                Some(want),
                "{p} should detect case-insensitively"
            );
        }
        // The `.env` family is case-folded too.
        assert_eq!(
            Format::detect_from_path(Path::new(".ENV")),
            Some(Format::Dotenv),
            ".ENV should detect as dotenv"
        );
    }

    #[test]
    fn well_known_config_files_detect_by_name() {
        use std::path::Path;
        // Extension-less / name-identified config files auto-detect by filename.
        for (p, want) in [
            (".editorconfig", Format::Ini),
            ("Pipfile", Format::Toml),
            ("web.config", Format::Xml),
            ("app.config", Format::Xml),
            ("WEB.CONFIG", Format::Xml), // case-folded
        ] {
            assert_eq!(
                Format::detect_from_path(Path::new(p)),
                Some(want),
                "{p} should detect by name"
            );
        }
        // `Pipfile.lock` is JSON, NOT TOML -- the exact-name match must not catch it
        // (and we don't auto-detect it, so it stays None rather than mis-detecting).
        assert_eq!(Format::detect_from_path(Path::new("Pipfile.lock")), None);
        // Ambiguous dotfiles (JSON or YAML) are deliberately NOT auto-detected.
        assert_eq!(Format::detect_from_path(Path::new(".eslintrc")), None);
        assert_eq!(Format::detect_from_path(Path::new(".prettierrc")), None);
    }

    #[test]
    fn properties_is_a_flat_literal_map() {
        // `.properties` -> a flat object of literal strings: all three Java
        // separators (`=` / `:` / space), dotted keys stay ONE key, and `${...}`
        // placeholders are kept verbatim (resolved by the app, not the file).
        let text = "# comment\n\
                    db.host = localhost\n\
                    db.port : 5432\n\
                    app.name value with spaces\n\
                    url=${NOT_EXPANDED}/x\n";
        let v = Format::Properties.parse(text).expect("parse properties");
        assert_eq!(
            v["db.host"],
            json!("localhost"),
            "`=` separator, dotted key flat"
        );
        assert_eq!(v["db.port"], json!("5432"), "`:` separator");
        assert_eq!(v["app.name"], json!("value with spaces"), "space separator");
        assert_eq!(
            v["url"],
            json!("${NOT_EXPANDED}/x"),
            "placeholders are literal"
        );
    }

    #[test]
    fn properties_non_ascii_is_utf8_not_latin1_with_bom() {
        // Encoding regression guard: UTF-8 values must round-trip, not mojibake
        // through Windows-1252 (`café` -> `cafÃ©`), and a leading BOM is stripped.
        let v = Format::Properties
            .parse("\u{feff}name=café\ngreeting=\u{65e5}\u{672c}\u{8a9e}\n")
            .expect("parse utf-8 properties");
        assert_eq!(
            v["name"],
            json!("café"),
            "UTF-8 value + BOM stripped from key"
        );
        assert_eq!(v["greeting"], json!("日本語"), "CJK round-trips");
    }

    #[test]
    fn properties_escapes_continuations_dups_and_comments() {
        let v = Format::Properties
            .parse(
                "# comment\n\
                 ! also a comment\n\
                 unicode=caf\\u00e9\n\
                 cont=line1\\\n  line2\n\
                 dup=first\n\
                 dup=second\n",
            )
            .expect("parse");
        assert_eq!(v["unicode"], json!("café"), "\\uXXXX escape decoded");
        assert_eq!(v["cont"], json!("line1line2"), "line-continuation joins");
        assert_eq!(v["dup"], json!("second"), "duplicate keys: last wins");
        let obj = v.as_object().unwrap();
        assert!(!obj.contains_key("# comment"), "`#` comment excluded");
        assert!(
            !obj.contains_key("! also a comment"),
            "`!` comment excluded"
        );
        assert_eq!(obj.len(), 3, "only unicode / cont / dup");
    }

    #[test]
    fn properties_malformed_unicode_escape_is_an_error() {
        assert!(Format::Properties.parse("k=\\uZZZZ\n").is_err());
        assert!(Format::Properties.parse("k=\\uD83D\n").is_err());
    }

    #[test]
    fn properties_surrogate_pairs_and_trailing_eof_backslash() {
        let v = Format::Properties
            .parse("emoji=\\uD83D\\uDE00\neof=kept\\")
            .expect("parse surrogate pair and EOF trailing backslash");
        assert_eq!(v["emoji"], json!("😀"));
        assert_eq!(v["eof"], json!("kept"));
    }

    #[test]
    fn properties_and_props_extensions_are_distinct() {
        use std::path::Path;
        // `.properties` (Java config) detects as Properties; `.props` (an MSBuild
        // XML file) detects as XML -- distinct extensions kept separate by the
        // match arms, so neither is misparsed as the other.
        assert_eq!(
            Format::detect_from_path(Path::new("application.properties")),
            Some(Format::Properties)
        );
        assert_eq!(
            Format::detect_from_path(Path::new("Directory.Build.props")),
            Some(Format::Xml)
        );
    }

    #[test]
    fn ini_dispatches_to_a_two_level_section_map() {
        // The `Format::Ini` arm reaches the hand-rolled parser: pre-section keys
        // hoist to the top level, a `[section]` is a nested object, and both
        // `=`/`:` separate. Full parser coverage lives in `crate::ini`.
        let v = Format::Ini
            .parse("root = true\n[server]\nhost = localhost\nport : 8080\n")
            .expect("parse ini");
        assert_eq!(v["root"], json!("true"), "global key hoisted to top level");
        assert_eq!(v["server"]["host"], json!("localhost"));
        assert_eq!(v["server"]["port"], json!("8080"), "`:` separator");
    }

    #[test]
    fn ini_and_cfg_extensions_detect_as_ini() {
        use std::path::Path;
        for p in ["tox.ini", "pytest.ini", "setup.cfg", "app.cfg"] {
            assert_eq!(
                Format::detect_from_path(Path::new(p)),
                Some(Format::Ini),
                "{p} should detect as INI"
            );
        }
    }

    #[test]
    fn hcl_dispatches_to_a_json_native_value() {
        // hcl-rs maps HCL directly: a block nests by type + labels, values keep their
        // HCL type (a number stays a number, unlike assigned dotenv/INI values), and
        // an unevaluated expression is an opaque string.
        let v = Format::Hcl
            .parse(
                "region = \"us-east-1\"\n\
                 resource \"aws_instance\" \"web\" {\n  ami = \"ami-1\"\n  count = 2\n}\n\
                 locals {\n  name = var.env\n}\n",
            )
            .expect("parse hcl");
        assert_eq!(v["region"], json!("us-east-1"));
        assert_eq!(
            v["resource"]["aws_instance"]["web"]["ami"],
            json!("ami-1"),
            "block nests by type -> label1 -> label2"
        );
        assert_eq!(
            v["resource"]["aws_instance"]["web"]["count"],
            json!(2),
            "a number stays a JSON number"
        );
        assert_eq!(
            v["locals"]["name"],
            json!("${var.env}"),
            "an unevaluated expression is an opaque string"
        );
    }

    #[test]
    fn hcl_repeated_block_is_an_array_single_is_an_object() {
        // The cardinality footgun (same as XML repeated elements): a block type
        // appearing once is an object, repeated is a document-order array.
        assert_eq!(
            Format::Hcl.parse("s {\n  a = 1\n}\n").expect("one")["s"],
            json!({"a": 1})
        );
        assert_eq!(
            Format::Hcl
                .parse("s {\n  a = 1\n}\ns {\n  a = 2\n}\n")
                .expect("many")["s"],
            json!([{"a": 1}, {"a": 2}])
        );
    }

    #[test]
    fn hcl_extensions_detect_as_hcl() {
        use std::path::Path;
        for p in ["main.tf", "variables.tfvars", "config.hcl", "job.nomad"] {
            assert_eq!(
                Format::detect_from_path(Path::new(p)),
                Some(Format::Hcl),
                "{p} should detect as HCL"
            );
        }
    }

    #[test]
    fn xml_at_max_depth_parses_on_a_worker_sized_stack() {
        // The depth REJECTION path is tested elsewhere; this gates the other half
        // -- that a document AT `MAX_XML_DEPTH` actually PARSES without overflowing
        // on both a constrained 1 MiB stack and a ~2 MiB rayon worker stack, not
        // just on the test harness's larger stack. `roxmltree` 0.21.2 parses
        // element nesting iteratively, while `element_to_value` and
        // `serde_json::Value::drop` still recurse ~1 frame/element.
        let deep = format!(
            "{}x{}",
            "<a>".repeat(MAX_XML_DEPTH),
            "</a>".repeat(MAX_XML_DEPTH)
        );
        for stack_bytes in [1024 * 1024, 2 * 1024 * 1024] {
            let input = deep.clone();
            let ok = std::thread::Builder::new()
                .stack_size(stack_bytes)
                .spawn(move || Format::Xml.parse(&input).is_ok())
                .expect("spawn worker-sized thread")
                .join()
                .expect("the parse thread must return, not abort the process");
            assert!(
                ok,
                "an at-limit XML document must parse on a {stack_bytes}-byte stack"
            );
        }
    }

    #[test]
    fn roxmltree_parses_deep_xml_iteratively_on_1mib_stack() {
        // Guard the `roxmltree` 0.21.2 iterative-parsing fix: even a 10,000-deep
        // document parsed directly by `roxmltree` must not stack-overflow on a
        // constrained 1 MiB stack, and `Format::Xml.parse` still rejects it
        // cleanly via the pre-scan depth limit.
        let deep = format!("{}x{}", "<a>".repeat(10_000), "</a>".repeat(10_000));
        let input = deep.clone();
        let raw_ok = std::thread::Builder::new()
            .stack_size(1024 * 1024)
            .spawn(move || roxmltree::Document::parse(&input).is_ok())
            .expect("spawn 1 MiB thread")
            .join()
            .expect("roxmltree 0.21.2 must not stack-overflow");
        assert!(
            raw_ok,
            "roxmltree 0.21.2 parses 10,000-deep XML iteratively"
        );
        let err = Format::Xml.parse(&deep).unwrap_err();
        assert!(
            err.contains("depth"),
            "Format::Xml rejects over-deep XML: {err}"
        );
    }

    #[test]
    fn dotenv_budget_counts_original_bom_bytes() {
        let cap = crate::dotenv::MAX_DOTENV_BYTES;
        for prelude in ["", "\u{feff}", "\u{feff}\u{feff}"] {
            let size = cap - prelude.len() - 2;
            let mut source = format!("{prelude}A={}", "x".repeat(size - 1));
            assert_eq!(
                Format::Dotenv.parse(&source).unwrap()["A"]
                    .as_str()
                    .unwrap()
                    .len(),
                size - 1
            );
            source.push('x');
            assert_eq!(
                Format::Dotenv.parse(&source).unwrap()["A"]
                    .as_str()
                    .unwrap()
                    .len(),
                size
            );
            source.push('x');
            assert!(Format::Dotenv.parse(&source).is_err());
        }
    }

    #[test]
    fn dotenv_no_config_fastpath_remains_independent_of_lossless_budgets() {
        let source = format!(
            "\u{feff}{}",
            " \r\n".repeat(crate::dotenv::MAX_DOTENV_BYTES / 3)
        );
        assert!(source.len() > crate::dotenv::MAX_DOTENV_BYTES);
        assert_eq!(
            Format::Dotenv.parse(&source).unwrap(),
            serde_json::json!({})
        );
    }

    #[test]
    fn every_format_tolerates_bom_and_empty_input() {
        // GATE for the uniform BOM-strip + empty->{} handling at the top of
        // `Format::parse`. A BOM-prefixed file must parse IDENTICALLY to its
        // un-BOM'd form (JSON / HCL previously rejected a BOM as a syntax error),
        // and an empty / whitespace-only file must parse to an empty OBJECT for
        // EVERY format -- so `*_path_absent` is satisfied and `json_schema_passes`
        // validates it as `{}` (which passes `{"type":"object"}`), NOT the
        // false-firing `null` the first cut returned. Iterates `Format::ALL`, so a
        // newly-added format cannot silently skip this contract.
        let empty_doc = Value::Object(serde_json::Map::new());
        for &f in Format::ALL {
            assert_eq!(
                f.parse(""),
                Ok(empty_doc.clone()),
                "{} empty input must be an empty object",
                f.label()
            );
            assert_eq!(
                f.parse("   \n\t  "),
                Ok(empty_doc.clone()),
                "{} whitespace-only input must be an empty object",
                f.label()
            );
            // A BOM interleaved with whitespace (a stray BOM that `trim_start_matches`
            // alone leaves behind) on otherwise-empty input must ALSO be an empty
            // object, not a one-char scalar (which YAML/properties/INI would produce).
            assert_eq!(
                f.parse("\u{feff} \u{feff}\t"),
                Ok(empty_doc.clone()),
                "{} BOM-and-whitespace-only input must be an empty object",
                f.label()
            );
        }
        // A minimal valid document per format; a single OR double BOM prefix must
        // not change the parse. The sample table MUST cover every `Format::ALL`
        // variant (asserted), so a new format forces a BOM sample here too.
        let samples: &[(Format, &str)] = &[
            (Format::Json, "{\"k\":\"v\"}"),
            (Format::Yaml, "k: v"),
            (Format::Toml, "k = \"v\""),
            (Format::Xml, "<r><k>v</k></r>"),
            (Format::Dotenv, "k=v"),
            (Format::Properties, "k=v"),
            (Format::Ini, "[s]\nk=v"),
            (Format::Hcl, "k = \"v\""),
        ];
        let covered: std::collections::BTreeSet<&str> =
            samples.iter().map(|(f, _)| f.label()).collect();
        let all: std::collections::BTreeSet<&str> = Format::ALL.iter().map(|f| f.label()).collect();
        assert_eq!(
            covered, all,
            "the BOM sample table must cover every Format::ALL variant"
        );
        for &(f, doc) in samples {
            let plain = f.parse(doc);
            assert!(plain.is_ok(), "{} sample must parse: {plain:?}", f.label());
            assert_eq!(
                f.parse(&format!("\u{feff}{doc}")),
                plain,
                "{} single-BOM must parse identically",
                f.label()
            );
            assert_eq!(
                f.parse(&format!("\u{feff}\u{feff}{doc}")),
                plain,
                "{} double-BOM must parse identically (no residual mis-parse)",
                f.label()
            );
        }
    }

    #[test]
    fn hcl_malformed_and_duplicate_attr_are_parse_errors() {
        assert!(
            Format::Hcl.parse("resource \"x\" {\n  a =\n").is_err(),
            "an incomplete block is a parse error"
        );
        assert!(
            Format::Hcl.parse("a = 1\na = 2\n").is_err(),
            "HCL rejects a duplicate attribute"
        );
    }

    #[test]
    fn duplicate_keys_diverge_by_format_as_documented() {
        // GATE for the "Duplicate keys differ" caveat in docs/rules.md: a key
        // repeated within one scope resolves three incompatible ways, and a rule
        // author needs to know which. Pinning it here keeps the doc honest and
        // catches a parser change that changes the behavior (e.g. a YAML parser
        // that starts rejecting dup keys would move YAML from keep-last to reject).
        //
        // Keep-LAST (earlier value invisible):
        assert_eq!(
            Format::Json.parse("{\"a\":1,\"a\":2}").unwrap()["a"],
            serde_json::json!(2),
            "JSON keeps the last duplicate key"
        );
        assert_eq!(
            Format::Yaml.parse("a: 1\na: 2\n").unwrap()["a"],
            serde_json::json!(2),
            "YAML keeps the last duplicate key"
        );
        assert_eq!(
            Format::Dotenv.parse("a=1\na=2\n").unwrap()["a"],
            serde_json::json!("2"),
            "dotenv keeps the last duplicate key"
        );
        assert_eq!(
            Format::Properties.parse("a=1\na=2\n").unwrap()["a"],
            serde_json::json!("2"),
            "properties keeps the last duplicate key"
        );
        // REJECT (parse error):
        assert!(
            Format::Toml.parse("a = 1\na = 2\n").is_err(),
            "TOML rejects a duplicate key"
        );
        assert!(
            Format::Hcl.parse("a = 1\na = 2\n").is_err(),
            "HCL rejects a duplicate attribute"
        );
        // Document-order ARRAY (both values queryable):
        assert_eq!(
            Format::Xml.parse("<r><a>1</a><a>2</a></r>").unwrap()["r"]["a"],
            serde_json::json!(["1", "2"]),
            "XML collects repeated elements into a document-order array"
        );
        assert_eq!(
            Format::Ini.parse("[s]\na=1\na=2\n").unwrap()["s"]["a"],
            serde_json::json!(["1", "2"]),
            "INI collects a repeated key into a document-order array"
        );
    }

    #[test]
    fn value_mapping_caveats_are_accurate() {
        // GATE for the "Value-mapping caveats" block in docs/rules.md. That block
        // has drifted from the real parser twice (a linter that documents a footgun
        // must document the RIGHT footgun), so pin every claim here: a doc/parser
        // mismatch now fails CI. (Duplicate-key divergence has its own test above.)
        use serde_json::json;

        // Assigned leaves in stringly-typed formats store even a numeric literal
        // as a STRING -- the root cause of numeric/boolean filter comparators
        // no-oping on XML/dotenv/properties/INI. A typed format keeps the number.
        assert_eq!(
            Format::Dotenv.parse("PORT=8080").unwrap()["PORT"],
            json!("8080")
        );
        assert_eq!(
            Format::Json.parse("{\"PORT\":8080}").unwrap()["PORT"],
            json!(8080)
        );

        // inf/nan: TOML (`inf`/`nan`) and YAML (`.inf`/`.nan`) -> null; HCL -> opaque
        // expression string (NOT an error); JSON has no such literal. NB YAML's bare
        // `nan` (no dot) is a plain string -- only the dotted form is a float.
        assert_eq!(Format::Toml.parse("a = inf").unwrap()["a"], json!(null));
        assert_eq!(Format::Toml.parse("a = nan").unwrap()["a"], json!(null));
        assert_eq!(Format::Yaml.parse("a: .inf").unwrap()["a"], json!(null));
        assert_eq!(Format::Yaml.parse("a: .nan").unwrap()["a"], json!(null));
        assert_eq!(Format::Yaml.parse("a: nan").unwrap()["a"], json!("nan"));
        assert_eq!(Format::Hcl.parse("a = inf").unwrap()["a"], json!("${inf}"));
        assert_eq!(Format::Hcl.parse("a = nan").unwrap()["a"], json!("${nan}"));

        // Empty assigned value: null in XML/YAML, "" in
        // json/toml/dotenv/ini/properties. A dotenv bare declaration is distinct
        // from an empty assignment and maps to null.
        assert_eq!(
            Format::Xml.parse("<r><a/></r>").unwrap()["r"]["a"],
            json!(null)
        );
        assert_eq!(Format::Yaml.parse("a:").unwrap()["a"], json!(null));
        assert_eq!(Format::Json.parse("{\"a\":\"\"}").unwrap()["a"], json!(""));
        assert_eq!(Format::Toml.parse("a = \"\"").unwrap()["a"], json!(""));
        assert_eq!(Format::Dotenv.parse("a=").unwrap()["a"], json!(""));
        assert_eq!(Format::Dotenv.parse("a\n").unwrap()["a"], json!(null));
        assert_eq!(Format::Ini.parse("[s]\nk=").unwrap()["s"]["k"], json!(""));
        assert_eq!(Format::Properties.parse("k=").unwrap()["k"], json!(""));

        // A TOML date/time is a magic-key OBJECT, not a string; YAML's is a string.
        assert_eq!(
            Format::Toml.parse("a = 1979-05-27").unwrap()["a"],
            json!({ "$__toml_private_datetime": "1979-05-27" })
        );
        assert_eq!(
            Format::Yaml.parse("a: 2002-12-14").unwrap()["a"],
            json!("2002-12-14")
        );

        // Based / underscored numbers: TOML parses hex/octal/binary + underscores;
        // YAML parses the base prefixes but keeps an underscored decimal as a string;
        // HCL REJECTS non-decimal literals; the stringly formats keep them as strings.
        assert_eq!(Format::Toml.parse("a = 0x1F").unwrap()["a"], json!(31));
        assert_eq!(Format::Toml.parse("a = 0o17").unwrap()["a"], json!(15));
        assert_eq!(Format::Toml.parse("a = 0b101").unwrap()["a"], json!(5));
        assert_eq!(Format::Toml.parse("a = 1_000").unwrap()["a"], json!(1000));
        assert_eq!(Format::Yaml.parse("a: 0x1F").unwrap()["a"], json!(31));
        assert_eq!(Format::Yaml.parse("a: 0o17").unwrap()["a"], json!(15));
        assert_eq!(Format::Yaml.parse("a: 0b101").unwrap()["a"], json!(5));
        assert_eq!(Format::Yaml.parse("a: 1_000").unwrap()["a"], json!("1_000"));
        assert!(Format::Hcl.parse("a = 0x1F").is_err());
        assert!(Format::Hcl.parse("a = 1_000").is_err());
        assert_eq!(
            Format::Ini.parse("[s]\nport=8080").unwrap()["s"]["port"],
            json!("8080")
        );
        assert_eq!(
            Format::Xml.parse("<r><n>42</n></r>").unwrap()["r"]["n"],
            json!("42")
        );

        // Big integer beyond i64: lossy float in JSON/HCL (hcl-edit 0.10), parse error in YAML/TOML.
        assert!(Format::Json.parse("{\"a\":100000000000000000000}").unwrap()["a"].is_f64());
        assert!(Format::Hcl.parse("a = 100000000000000000000").unwrap()["a"].is_f64());
        assert!(Format::Yaml.parse("a: 100000000000000000000").is_err());
        assert!(Format::Toml.parse("a = 100000000000000000000").is_err());
    }

    #[test]
    fn oversize_structured_input_is_rejected_before_building_a_tree() {
        // A structured tree costs up to ~30x the input (XML), so an input over
        // MAX_STRUCTURED_BYTES is rejected at the size check (no tree, no balloon). The
        // rejection is on `.len()` alone, so no parse runs for the oversize input.
        // Iterate `Format::ALL` so the invariant is asserted for EVERY format (a
        // per-arm refactor that skipped the check for one would be caught here); the
        // "maximum supported size" substring matches both the structured cap message
        // and HCL's own tighter cap message.
        let over = "x".repeat(MAX_STRUCTURED_BYTES + 1);
        for &f in Format::ALL {
            let err = f.parse(&over).unwrap_err();
            assert!(
                err.contains("maximum supported size"),
                "{} oversize input rejected: {err}",
                f.label()
            );
        }
    }

    #[test]
    fn format_all_is_complete() {
        // Every parity gate iterates `Format::ALL`, so a variant missing from ALL
        // silently bypasses them. The real guard here is the exhaustive `match`
        // below: a NEW `Format` variant is a COMPILE error until a reviewer adds an
        // arm, so no variant reaches the codebase without a human editing this
        // test with `Format::ALL` in view; the asserts then require ALL and
        // `variants` to list the same set. Fully deriving ALL from the enum would
        // need a proc-macro (`strum`), which this crate deliberately avoids -- so
        // the compile error, not the assert, is what stops a half-wired variant.
        let variants = [
            Format::Json,
            Format::Yaml,
            Format::Toml,
            Format::Xml,
            Format::Dotenv,
            Format::Properties,
            Format::Ini,
            Format::Hcl,
        ];
        for f in variants {
            // Distinct arms (clippy-clean) keep the match exhaustive, so a new
            // Format variant is a compile error here until it is added.
            let _label = match f {
                Format::Json => "json",
                Format::Yaml => "yaml",
                Format::Toml => "toml",
                Format::Xml => "xml",
                Format::Dotenv => "dotenv",
                Format::Properties => "properties",
                Format::Ini => "ini",
                Format::Hcl => "hcl",
            };
        }
        assert_eq!(
            Format::ALL.len(),
            variants.len(),
            "Format::ALL is out of sync with the Format variants; update both"
        );
        for f in variants {
            assert!(
                Format::ALL.contains(&f),
                "Format::{f:?} is missing from Format::ALL"
            );
        }
    }
}

#[cfg(test)]
#[path = "structured_format_hcl_tests.rs"]
mod hcl_tests;

#[cfg(test)]
#[path = "structured_format_jsonc_tests.rs"]
mod jsonc_tests;
