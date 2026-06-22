//! Structured-query rule family:
//! `{json,yaml,toml,xml}_path_{equals,matches}`.
//!
//! Eight rule kinds share a single implementation that varies
//! along two axes:
//!
//! - **Format** — `Json`, `Yaml`, `Toml`, or `Xml`. The file is
//!   parsed into a `serde_json::Value` tree regardless (YAML and
//!   TOML coerce through serde; XML maps via the xmltodict-style
//!   convention in `xml_to_value` — `@attr` / `#text` /
//!   repeated-element→array, leaf elements collapse to their
//!   text string, namespaces flatten to local names, every leaf
//!   is a string), so the `JSONPath` engine only has to reason
//!   about one tree shape. XML design + open-question
//!   resolutions: `docs/design/v0.10/xml_path.md`.
//! - **Op** — `Equals(value)` for exact equality or
//!   `Matches(regex)` for regex on string values.
//!
//! All rule kinds require:
//!
//! - `paths` — which files to scan.
//! - `path` — a `JSONPath` expression (RFC 9535) pointing at the
//!   values to check.
//! - Either `equals` (arbitrary YAML value) or `matches`
//!   (regex string), according to the rule kind.
//!
//! ## Semantics
//!
//! `JSONPath` can return multiple matches (`$.deps[*].version`).
//! Every match must satisfy the op; any single mismatch
//! produces a violation at that match's location. If the query
//! returns zero matches, that's one "path not found" violation
//! — the option the user is enforcing doesn't exist.
//!
//! The optional **`if_present: true`** flag flips the zero-match
//! case: under it, zero matches are silently OK, and only
//! actual matches that fail the op produce violations. Useful
//! for predicates that only apply when a field is present —
//! e.g. "every `uses:` in a GitHub Actions workflow must be
//! pinned to a commit SHA" (a workflow with only `run:` steps
//! has no `uses:` at all and shouldn't be flagged).
//!
//! Unparseable files (bad JSON / YAML / TOML, not-well-formed
//! XML) produce one violation per file. An unparseable file is a
//! documentation problem, not the structured rule's concern —
//! but better to surface it than silently skip.

use std::path::{Path, PathBuf};

use alint_core::{
    Context, Error, Level, PathsSpec, PerFileRule, Result, Rule, RuleSpec, Scope, Violation,
};
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use serde_json_path::JsonPath;

/// True when `pattern` is a plain relative-path literal — no
/// glob metacharacters, no `!` exclude prefix. Mirrors
/// `file_exists::is_literal_path`; kept local to dodge a
/// crate-wide pub-helper module just for two rules.
fn is_literal_path(pattern: &str) -> bool {
    !pattern.starts_with('!')
        && !pattern
            .chars()
            .any(|c| matches!(c, '*' | '?' | '[' | ']' | '{' | '}'))
}

/// Collect every literal pattern from `spec` IFF every entry is
/// a literal AND the spec carries no excludes. Returns `None`
/// when any pattern is a glob or there are excludes — the slow
/// path is still correct in those cases.
fn extract_literal_paths(spec: &PathsSpec) -> Option<Vec<PathBuf>> {
    let patterns: Vec<&str> = match spec {
        PathsSpec::Single(s) => vec![s.as_str()],
        PathsSpec::Many(v) => v.iter().map(String::as_str).collect(),
        PathsSpec::IncludeExclude { include, exclude } if exclude.is_empty() => {
            include.iter().map(String::as_str).collect()
        }
        PathsSpec::IncludeExclude { .. } => return None,
    };
    if patterns.iter().all(|p| is_literal_path(p)) {
        Some(patterns.iter().map(PathBuf::from).collect())
    } else {
        None
    }
}

/// Which YAML-flavoured parser to use on the target file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Yaml,
    Toml,
    Xml,
}

impl Format {
    pub(crate) fn parse(self, text: &str) -> std::result::Result<Value, String> {
        match self {
            // Try strict JSON first (the common, fast path — plain
            // JSON is byte-for-byte unchanged). Only on failure retry
            // tolerating JSONC: `//` + `/* */` comments and trailing
            // commas, which the JS/TS ecosystem uses pervasively in
            // `.json` files (tsconfig.json, `.vscode/*.json`). If the
            // tolerant retry also fails, surface the *original* strict
            // error so genuinely-broken JSON reports accurately.
            Self::Json => serde_json::from_str(text).or_else(|strict_err| {
                serde_json::from_str(&strip_jsonc(text)).map_err(|_| strict_err.to_string())
            }),
            Self::Yaml => serde_yaml_ng::from_str(text).map_err(|e| e.to_string()),
            Self::Toml => toml::from_str(text).map_err(|e| e.to_string()),
            Self::Xml => xml_to_value(text),
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Yaml => "YAML",
            Self::Toml => "TOML",
            Self::Xml => "XML",
        }
    }

    /// Detect the format from a path's extension. Returns `None`
    /// for unknown extensions; callers decide how to fall back
    /// (require an explicit `format:` override, default to JSON,
    /// emit a per-file violation, etc).
    pub(crate) fn detect_from_path(path: &std::path::Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "json" => Some(Self::Json),
            "yaml" | "yml" => Some(Self::Yaml),
            "toml" => Some(Self::Toml),
            "xml" | "csproj" | "props" | "targets" | "vbproj" | "fsproj" | "nuspec" => {
                Some(Self::Xml)
            }
            _ => None,
        }
    }
}

/// Make a JSONC document parseable as strict JSON: drop `//` and
/// `/* … */` comments and trailing commas (a `,` immediately before a
/// `]` / `}`). String-aware — markers inside `"…"` (with `\` escapes)
/// are preserved, so a `"https://…"` URL or a `","` literal is
/// untouched. Only invoked when strict parsing already failed, so
/// plain JSON never pays for it.
fn strip_jsonc(src: &str) -> String {
    // Pass 1: remove comments.
    let mut decommented = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            decommented.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    decommented.push(n);
                }
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                decommented.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        decommented.push('\n');
                        break;
                    }
                }
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
            }
            _ => decommented.push(c),
        }
    }
    // Pass 2: drop trailing commas (`,` then whitespace then `]`/`}`).
    let cs: Vec<char> = decommented.chars().collect();
    let mut out = String::with_capacity(cs.len());
    let mut in_string = false;
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if in_string {
            out.push(c);
            if c == '\\' {
                i += 1;
                if i < cs.len() {
                    out.push(cs[i]);
                }
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == ',' {
            let mut j = i + 1;
            while j < cs.len() && cs[j].is_whitespace() {
                j += 1;
            }
            if j < cs.len() && (cs[j] == ']' || cs[j] == '}') {
                // Drop the comma; keep the intervening whitespace.
                out.extend(&cs[i + 1..j]);
                i = j;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Comparison op — keeps the rule builders thin.
#[derive(Debug)]
pub enum Op {
    /// Value at `path` must serialize-compare equal to this
    /// literal. Any JSON-representable value works (bool,
    /// number, string, array, object, null).
    Equals(Value),
    /// Value at `path` must be a string that the regex matches.
    /// A non-string match produces a violation with a clear
    /// `expected string, got <kind>` message.
    Matches(Regex),
}

// ---------------------------------------------------------------
// Options — deserialized from the rule spec's `extra` map.
// ---------------------------------------------------------------

/// Options shared by every `*_path_equals` rule kind.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct EqualsOptions {
    /// `JSONPath` expression rooted at `$`. Supports dot-access (`$.foo.bar`),
    /// array index (`$.deps[0]`), wildcards (`$.deps[*]`), filters, and every
    /// other RFC 9535 construct.
    path: String,
    /// Expected value. Any JSON type (string, number, boolean, null, array, object).
    equals: Value,
    /// When true, a query returning zero matches is silently OK - only real
    /// matches that fail the op produce violations.
    #[serde(default)]
    if_present: bool,
}

/// Options shared by every `*_path_matches` rule kind.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct MatchesOptions {
    /// `JSONPath` expression rooted at `$`.
    path: String,
    /// Rust-regex pattern to match against the value at `path`.
    matches: String,
    /// When true, a query returning zero matches is silently OK - only real
    /// matches that fail the op produce violations.
    #[serde(default)]
    if_present: bool,
}

/// schemars-derived options schema for the four `*_path_equals` kinds; composed
/// into their `$defs` branches by `xtask gen-schema`. See
/// [`crate::migrated_option_schemas`].
#[must_use]
pub fn equals_options_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(EqualsOptions))
        .expect("EqualsOptions JSON schema serializes")
}

/// schemars-derived options schema for the four `*_path_matches` kinds.
#[must_use]
pub fn matches_options_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(MatchesOptions))
        .expect("MatchesOptions JSON schema serializes")
}

// ---------------------------------------------------------------
// Rule
// ---------------------------------------------------------------

#[derive(Debug)]
pub struct StructuredPathRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    /// `Some(paths)` when every `paths:` entry is a plain
    /// literal (no glob metacharacters, no `!` excludes). The
    /// fast path uses these to short-circuit through the
    /// index's hash-set and skip the O(N) `scope.matches`
    /// scan — same shape as `file_exists`'s fast path. Driven
    /// by the bundled `monorepo/cargo-workspace@v1`'s
    /// `cargo-workspace-member-declares-name` rule, which
    /// `for_each_dir` instantiates with `paths:
    /// "{path}/Cargo.toml"` (purely literal after token
    /// substitution) for every `crates/*` directory; without
    /// the fast path this is the dominant 1M-scale bottleneck.
    literal_paths: Option<Vec<PathBuf>>,
    format: Format,
    path_expr: JsonPath,
    path_src: String,
    op: Op,
    /// When `true`, a `JSONPath` query that produces zero matches
    /// is silently OK. When `false` (default), a zero-match query
    /// is reported as a single violation — the "value being
    /// enforced doesn't exist" case. Use `true` for predicates
    /// that are conditional on the field being present (e.g.
    /// "every `uses:` in a workflow must be SHA-pinned" — a
    /// workflow with no `uses:` at all shouldn't be flagged).
    if_present: bool,
}

impl Rule for StructuredPathRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        self.level
    }
    fn policy_url(&self) -> Option<&str> {
        self.policy_url.as_deref()
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        if let Some(literals) = self.literal_paths.as_ref() {
            // Fast path: each `paths:` entry is a literal
            // relative path; we don't need to touch the entry
            // list at all. `contains_file` is the cheap
            // membership check; the absolute path comes from
            // joining `root` with the literal directly.
            // (`find_file` would re-scan the entries list to
            // hand back a `&FileEntry`, which we don't need
            // here — only the bytes — and which would
            // re-introduce the O(N) work this fast path
            // exists to avoid.)
            for literal in literals {
                if !ctx.index.contains_file(literal) {
                    continue;
                }
                let full = ctx.root.join(literal);
                let Ok(bytes) = std::fs::read(&full) else {
                    continue;
                };
                violations.extend(self.evaluate_file(ctx, literal, &bytes)?);
            }
        } else {
            for entry in ctx.index.files() {
                if !self.scope.matches(&entry.path, ctx.index) {
                    continue;
                }
                let full = ctx.root.join(&entry.path);
                let Ok(bytes) = std::fs::read(&full) else {
                    // permission / race — silent skip, like other
                    // content rules
                    continue;
                };
                violations.extend(self.evaluate_file(ctx, &entry.path, &bytes)?);
            }
        }
        Ok(violations)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for StructuredPathRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Ok(Vec::new());
        };
        let root_value = match self.format.parse(text) {
            Ok(v) => v,
            Err(err) => {
                return Ok(vec![
                    Violation::new(format!(
                        "not a valid {} document: {err}",
                        self.format.label()
                    ))
                    .with_path(std::sync::Arc::<Path>::from(path)),
                ]);
            }
        };
        let matches = self.path_expr.query(&root_value);
        if matches.is_empty() {
            if self.if_present {
                return Ok(Vec::new());
            }
            let msg = self
                .message
                .clone()
                .unwrap_or_else(|| format!("JSONPath `{}` produced no match", self.path_src));
            return Ok(vec![
                Violation::new(msg).with_path(std::sync::Arc::<Path>::from(path)),
            ]);
        }
        let mut violations = Vec::new();
        for m in matches.iter() {
            if let Some(v) = check_match(m, &self.op) {
                let base = self.message.clone().unwrap_or(v);
                // Baseline identity: the query + operator + the specific
                // matched value. One JSONPath can match N nodes (e.g.
                // `$.scripts[*]`), so without a key the N path-only
                // violations would collapse to one fingerprint and mask a
                // genuinely new failing node; and a reworded `message:`
                // must not churn the baseline. (v3 audit, §2.4.)
                violations.push(
                    Violation::new(base)
                        .with_path(std::sync::Arc::<Path>::from(path))
                        .with_baseline_key(match_baseline_key(&self.path_src, &self.op, m)),
                );
            }
        }
        Ok(violations)
    }
}

/// A reword-proof baseline identity for one failing match: the query source,
/// the operator (with its expected value / regex), and the specific matched
/// value. Distinct failing nodes from one query get distinct keys (no
/// masking); two nodes that fail with the *same* value collapse to a count
/// (legitimate). Independent of the rendered `message`, so a reword is inert.
///
/// The value is rendered in **full** (not `short_render`, which truncates at
/// 80 chars for human messages): a truncated value would collapse two distinct
/// long values sharing an 80-char prefix into one fingerprint — a masking bug.
/// `Value`'s `Display` is compact JSON with control characters escaped, so a
/// value can never contain a literal `\0` and forge the NUL separators.
fn match_baseline_key(path_src: &str, op: &Op, m: &Value) -> String {
    let op_descr = match op {
        Op::Equals(expected) => format!("== {expected}"),
        Op::Matches(re) => format!("=~ {}", re.as_str()),
    };
    format!("{path_src}\u{0}{op_descr}\u{0}got {m}")
}

/// Return `Some(message)` if the match fails the op; `None` if it passes.
fn check_match(m: &Value, op: &Op) -> Option<String> {
    match op {
        Op::Equals(expected) => {
            if m == expected {
                None
            } else {
                Some(format!(
                    "value at path does not equal expected: expected {}, got {}",
                    short_render(expected),
                    short_render(m),
                ))
            }
        }
        Op::Matches(re) => {
            let Some(s) = m.as_str() else {
                return Some(format!(
                    "value at path is not a string (got {}), can't apply regex",
                    kind_name(m)
                ));
            };
            if re.is_match(s) {
                None
            } else {
                Some(format!(
                    "value at path {} does not match regex {}",
                    short_render(m),
                    re.as_str(),
                ))
            }
        }
    }
}

/// A stable, short rendering for error messages. Avoids
/// dumping a whole object when the mismatch is on a sub-key.
fn short_render(v: &Value) -> String {
    let raw = v.to_string();
    if raw.len() <= 80 {
        raw
    } else {
        format!("{}…", &raw[..80])
    }
}

fn kind_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// ---------------------------------------------------------------
// XML → serde_json::Value
//
// xmltodict-style convention so the JSONPath a user writes reads
// like the XML they see. Full rationale + false-positive surface:
// `docs/design/v0.10/xml_path.md`.
// ---------------------------------------------------------------

/// Maximum XML element-nesting depth `xml_to_value` will
/// descend. Real config/manifest XML (`.csproj`, `pom.xml`, …)
/// is a handful of levels deep; 256 is far beyond any real
/// manifest yet far below the recursion depth that would
/// overflow the stack. A document nested deeper is rejected as a
/// parse error (one per-file violation via the existing
/// parse-error path) rather than recursed into — a crafted or
/// accidental deeply-nested file must never abort the run. The
/// other formats' parsers carry their own internal recursion
/// limits; this is the XML arm's equivalent.
const MAX_XML_DEPTH: usize = 256;

/// Parse XML into the same `serde_json::Value` tree the rest of
/// the family queries. The document maps to
/// `{ <root-element-name>: <root value> }` so the root element is
/// the first `JSONPath` segment (`$.Project…`, `$.project…`).
fn xml_to_value(text: &str) -> std::result::Result<Value, String> {
    let doc = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
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

// ---------------------------------------------------------------
// Builders
//
// Eight thin wrappers per (Format, Op) combination. Each consumes
// the spec, validates the structured-query options, and
// constructs the shared `StructuredPathRule`.
// ---------------------------------------------------------------

pub fn json_path_equals_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_equals(spec, Format::Json, "json_path_equals")
}

pub fn json_path_matches_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_matches(spec, Format::Json, "json_path_matches")
}

pub fn yaml_path_equals_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_equals(spec, Format::Yaml, "yaml_path_equals")
}

pub fn yaml_path_matches_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_matches(spec, Format::Yaml, "yaml_path_matches")
}

pub fn toml_path_equals_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_equals(spec, Format::Toml, "toml_path_equals")
}

pub fn toml_path_matches_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_matches(spec, Format::Toml, "toml_path_matches")
}

pub fn xml_path_equals_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_equals(spec, Format::Xml, "xml_path_equals")
}

pub fn xml_path_matches_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_matches(spec, Format::Xml, "xml_path_matches")
}

fn build_equals(spec: &RuleSpec, format: Format, kind_label: &str) -> Result<Box<dyn Rule>> {
    let paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(&spec.id, format!("{kind_label} requires a `paths` field"))
    })?;
    let opts: EqualsOptions = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let path_expr = JsonPath::parse(&opts.path).map_err(|e| {
        Error::rule_config(
            &spec.id,
            alint_core::jsonpath_diagnostics::format_parse_error(&opts.path, e),
        )
    })?;
    Ok(Box::new(StructuredPathRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        literal_paths: extract_literal_paths(paths),
        format,
        path_expr,
        path_src: opts.path,
        op: Op::Equals(opts.equals),
        if_present: opts.if_present,
    }))
}

fn build_matches(spec: &RuleSpec, format: Format, kind_label: &str) -> Result<Box<dyn Rule>> {
    let paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(&spec.id, format!("{kind_label} requires a `paths` field"))
    })?;
    let opts: MatchesOptions = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let path_expr = JsonPath::parse(&opts.path).map_err(|e| {
        Error::rule_config(
            &spec.id,
            alint_core::jsonpath_diagnostics::format_parse_error(&opts.path, e),
        )
    })?;
    let re = Regex::new(&opts.matches).map_err(|e| {
        Error::rule_config(&spec.id, format!("invalid regex {:?}: {e}", opts.matches))
    })?;
    Ok(Box::new(StructuredPathRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        literal_paths: extract_literal_paths(paths),
        format,
        path_expr,
        path_src: opts.path,
        op: Op::Matches(re),
        if_present: opts.if_present,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, spec_yaml, tempdir_with_files};

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

    // ─── build-path errors ────────────────────────────────────

    #[test]
    fn build_rejects_missing_paths() {
        let spec = spec_yaml(
            "id: t\n\
             kind: json_path_equals\n\
             path: \"$.name\"\n\
             equals: \"x\"\n\
             level: error\n",
        );
        assert!(json_path_equals_build(&spec).is_err());
    }

    #[test]
    fn build_rejects_invalid_jsonpath() {
        let spec = spec_yaml(
            "id: t\n\
             kind: json_path_equals\n\
             paths: \"package.json\"\n\
             path: \"$..[invalid\"\n\
             equals: \"x\"\n\
             level: error\n",
        );
        assert!(json_path_equals_build(&spec).is_err());
    }

    #[test]
    fn build_rejects_invalid_regex_in_matches() {
        let spec = spec_yaml(
            "id: t\n\
             kind: json_path_matches\n\
             paths: \"package.json\"\n\
             path: \"$.version\"\n\
             matches: \"[unterminated\"\n\
             level: error\n",
        );
        // Must fail in the regex-compile path (not via
        // deny_unknown_fields on a typo'd `pattern:` key — the
        // latent bug this previously had).
        let e = json_path_matches_build(&spec).unwrap_err().to_string();
        assert!(e.contains("regex"), "expected a regex error, got: {e}");
    }

    // ─── json_path_equals ─────────────────────────────────────

    #[test]
    fn json_path_equals_passes_when_value_matches() {
        let spec = spec_yaml(
            "id: t\n\
             kind: json_path_equals\n\
             paths: \"package.json\"\n\
             path: \"$.name\"\n\
             equals: \"demo\"\n\
             level: error\n",
        );
        let rule = json_path_equals_build(&spec).unwrap();
        let (tmp, idx) =
            tempdir_with_files(&[("package.json", br#"{"name":"demo","version":"1.0.0"}"#)]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "matching value should pass: {v:?}");
    }

    #[test]
    fn json_path_equals_fires_on_mismatch() {
        let spec = spec_yaml(
            "id: t\n\
             kind: json_path_equals\n\
             paths: \"package.json\"\n\
             path: \"$.name\"\n\
             equals: \"demo\"\n\
             level: error\n",
        );
        let rule = json_path_equals_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("package.json", br#"{"name":"other"}"#)]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn json_path_equals_fires_on_missing_path() {
        let spec = spec_yaml(
            "id: t\n\
             kind: json_path_equals\n\
             paths: \"package.json\"\n\
             path: \"$.name\"\n\
             equals: \"demo\"\n\
             level: error\n",
        );
        let rule = json_path_equals_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("package.json", br#"{"version":"1.0"}"#)]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "missing path should fire");
    }

    #[test]
    fn json_path_if_present_silent_on_missing() {
        // `if_present: true` → missing path is silent.
        let spec = spec_yaml(
            "id: t\n\
             kind: json_path_equals\n\
             paths: \"package.json\"\n\
             path: \"$.name\"\n\
             equals: \"demo\"\n\
             if_present: true\n\
             level: error\n",
        );
        let rule = json_path_equals_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("package.json", br#"{"version":"1.0"}"#)]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "if_present should silence: {v:?}");
    }

    // ─── json_path_matches ────────────────────────────────────

    #[test]
    fn json_path_matches_passes_on_pattern_hit() {
        let spec = spec_yaml(
            "id: t\n\
             kind: json_path_matches\n\
             paths: \"package.json\"\n\
             path: \"$.version\"\n\
             matches: \"^\\\\d+\\\\.\\\\d+\\\\.\\\\d+$\"\n\
             level: error\n",
        );
        let rule = json_path_matches_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("package.json", br#"{"version":"1.2.3"}"#)]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "matching version should pass: {v:?}");
    }

    #[test]
    fn json_path_matches_fires_on_pattern_miss() {
        let spec = spec_yaml(
            "id: t\n\
             kind: json_path_matches\n\
             paths: \"package.json\"\n\
             path: \"$.version\"\n\
             matches: \"^\\\\d+\\\\.\\\\d+\\\\.\\\\d+$\"\n\
             level: error\n",
        );
        let rule = json_path_matches_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("package.json", br#"{"version":"v1.x"}"#)]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1);
    }

    // ─── yaml_path_* ─────────────────────────────────────────

    #[test]
    fn yaml_path_equals_passes_when_value_matches() {
        let spec = spec_yaml(
            "id: t\n\
             kind: yaml_path_equals\n\
             paths: \".github/workflows/*.yml\"\n\
             path: \"$.name\"\n\
             equals: \"CI\"\n\
             level: error\n",
        );
        let rule = yaml_path_equals_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[(
            ".github/workflows/ci.yml",
            b"name: CI\non: push\njobs: {}\n",
        )]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "matching name should pass: {v:?}");
    }

    #[test]
    fn yaml_path_matches_uses_bracket_notation_for_dashed_keys() {
        // Per the memory note: dashed YAML keys need bracket
        // notation (`$.foo['dashed-key']`) because the JSONPath
        // dot-form can't parse them.
        let spec = spec_yaml(
            "id: t\n\
             kind: yaml_path_matches\n\
             paths: \"action.yml\"\n\
             path: \"$.runs['using']\"\n\
             matches: \"^node\\\\d+$\"\n\
             level: error\n",
        );
        let rule = yaml_path_matches_build(&spec).unwrap();
        let (tmp, idx) =
            tempdir_with_files(&[("action.yml", b"runs:\n  using: node20\n  main: index.js\n")]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "bracket notation should match: {v:?}");
    }

    // ─── toml_path_* ─────────────────────────────────────────

    #[test]
    fn toml_path_equals_passes_when_value_matches() {
        let spec = spec_yaml(
            "id: t\n\
             kind: toml_path_equals\n\
             paths: \"Cargo.toml\"\n\
             path: \"$.package.edition\"\n\
             equals: \"2024\"\n\
             level: error\n",
        );
        let rule = toml_path_equals_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[(
            "Cargo.toml",
            b"[package]\nname = \"x\"\nedition = \"2024\"\n",
        )]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "matching edition should pass: {v:?}");
    }

    #[test]
    fn toml_path_matches_fires_on_floating_version() {
        // Common policy: deps must be tilde-pinned, not bare.
        let spec = spec_yaml(
            "id: t\n\
             kind: toml_path_matches\n\
             paths: \"Cargo.toml\"\n\
             path: \"$.dependencies.serde\"\n\
             matches: \"^[~=]\"\n\
             level: error\n",
        );
        let rule = toml_path_matches_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[(
            "Cargo.toml",
            b"[package]\nname = \"x\"\n[dependencies]\nserde = \"1\"\n",
        )]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "floating `serde = \"1\"` should fire");
    }

    // ─── xml_path_* ──────────────────────────────────────────

    #[test]
    fn xml_path_equals_passes_on_csproj_leaf() {
        let spec = spec_yaml(
            "id: t\n\
             kind: xml_path_equals\n\
             paths: \"App.csproj\"\n\
             path: \"$.Project.PropertyGroup.TargetFramework\"\n\
             equals: \"net8.0\"\n\
             level: error\n",
        );
        let rule = xml_path_equals_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[(
            "App.csproj",
            br#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup></Project>"#,
        )]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "leaf element should match: {v:?}");
    }

    #[test]
    fn xml_path_equals_fires_on_csproj_mismatch() {
        let spec = spec_yaml(
            "id: t\n\
             kind: xml_path_equals\n\
             paths: \"App.csproj\"\n\
             path: \"$.Project.PropertyGroup.TargetFramework\"\n\
             equals: \"net8.0\"\n\
             level: error\n",
        );
        let rule = xml_path_equals_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[(
            "App.csproj",
            br"<Project><PropertyGroup><TargetFramework>net6.0</TargetFramework></PropertyGroup></Project>",
        )]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn xml_path_matches_on_packageref_attribute_array() {
        // Repeated <PackageReference> → array; `@Version`
        // attribute reached via bracket notation; every match
        // must be a non-empty version-ish string.
        let spec = spec_yaml(
            "id: t\n\
             kind: xml_path_matches\n\
             paths: \"App.csproj\"\n\
             path: \"$.Project.ItemGroup.PackageReference[*]['@Version']\"\n\
             matches: \"^\\\\d\"\n\
             level: error\n",
        );
        let rule = xml_path_matches_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[(
            "App.csproj",
            br#"<Project><ItemGroup><PackageReference Include="A" Version="1.2.3"/><PackageReference Include="B" Version="4.0.0"/></ItemGroup></Project>"#,
        )]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "both @Version attrs should match: {v:?}");
    }

    #[test]
    fn xml_pom_namespace_flattened_and_repeated_dependency_array() {
        // Maven default namespace must not leak into the query;
        // repeated <dependency> must be an array.
        let pom = br#"<project xmlns="http://maven.apache.org/POM/4.0.0"><modelVersion>4.0.0</modelVersion><dependencies><dependency><artifactId>guava</artifactId></dependency><dependency><artifactId>junit</artifactId></dependency></dependencies></project>"#;
        let eq = spec_yaml(
            "id: t\n\
             kind: xml_path_equals\n\
             paths: \"pom.xml\"\n\
             path: \"$.project.modelVersion\"\n\
             equals: \"4.0.0\"\n\
             level: error\n",
        );
        let (tmp, idx) = tempdir_with_files(&[("pom.xml", pom)]);
        assert!(
            xml_path_equals_build(&eq)
                .unwrap()
                .evaluate(&ctx(tmp.path(), &idx))
                .unwrap()
                .is_empty(),
            "namespace-flattened modelVersion should match"
        );
        let m = spec_yaml(
            "id: t\n\
             kind: xml_path_matches\n\
             paths: \"pom.xml\"\n\
             path: \"$.project.dependencies.dependency[*].artifactId\"\n\
             matches: \"^[a-z]+$\"\n\
             level: error\n",
        );
        let v = xml_path_matches_build(&m)
            .unwrap()
            .evaluate(&ctx(tmp.path(), &idx))
            .unwrap();
        assert!(v.is_empty(), "both deps' artifactId should match: {v:?}");
    }

    #[test]
    fn xml_path_if_present_silences_missing() {
        let spec = spec_yaml(
            "id: t\n\
             kind: xml_path_equals\n\
             paths: \"App.csproj\"\n\
             path: \"$.Project.PropertyGroup.Nullable\"\n\
             equals: \"enable\"\n\
             if_present: true\n\
             level: error\n",
        );
        let rule = xml_path_equals_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[(
            "App.csproj",
            br"<Project><PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup></Project>",
        )]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "if_present should silence missing: {v:?}");
    }

    #[test]
    fn xml_malformed_fires_one_violation() {
        let spec = spec_yaml(
            "id: t\n\
             kind: xml_path_equals\n\
             paths: \"App.csproj\"\n\
             path: \"$.Project\"\n\
             equals: \"x\"\n\
             level: error\n",
        );
        let rule = xml_path_equals_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("App.csproj", b"<Project><Unclosed></Project>")]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "not-well-formed XML should fire once");
        assert!(v[0].message.contains("XML"), "{:?}", v[0].message);
    }

    #[test]
    fn xml_deeply_nested_is_a_parse_error_not_an_abort() {
        // P1 regression: unbounded recursion would `abort()` the
        // whole process. The `MAX_XML_DEPTH` guard must instead
        // yield exactly one ordinary parse-error violation for
        // the file (no panic, no abort, per-file contained).
        let depth = MAX_XML_DEPTH + 50;
        let xml = format!("{}deep{}", "<a>".repeat(depth), "</a>".repeat(depth));
        let spec = spec_yaml(
            "id: t\n\
             kind: xml_path_equals\n\
             paths: \"deep.xml\"\n\
             path: \"$.a\"\n\
             equals: \"x\"\n\
             level: error\n",
        );
        let rule = xml_path_equals_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("deep.xml", xml.as_bytes())]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(
            v.len(),
            1,
            "deeply-nested XML must yield exactly one parse-error violation: {v:?}"
        );
        assert!(
            v[0].message.contains("not a valid XML") && v[0].message.contains("depth"),
            "expected a depth parse-error message, got: {}",
            v[0].message
        );
    }

    #[test]
    fn xml_leaf_values_are_string_typed() {
        // Documented gotcha: every XML leaf is a string. A
        // quoted `equals: "8"` matches; a bare `equals: 8`
        // (a YAML integer) does not.
        let xml: &[u8] = b"<Config><n>8</n></Config>";
        let as_str = spec_yaml(
            "id: t\n\
             kind: xml_path_equals\n\
             paths: \"c.xml\"\n\
             path: \"$.Config.n\"\n\
             equals: \"8\"\n\
             level: error\n",
        );
        let (tmp, idx) = tempdir_with_files(&[("c.xml", xml)]);
        assert!(
            xml_path_equals_build(&as_str)
                .unwrap()
                .evaluate(&ctx(tmp.path(), &idx))
                .unwrap()
                .is_empty(),
            "string 8 should match the string-typed leaf"
        );
        let as_int = spec_yaml(
            "id: t\n\
             kind: xml_path_equals\n\
             paths: \"c.xml\"\n\
             path: \"$.Config.n\"\n\
             equals: 8\n\
             level: error\n",
        );
        let v = xml_path_equals_build(&as_int)
            .unwrap()
            .evaluate(&ctx(tmp.path(), &idx))
            .unwrap();
        assert_eq!(v.len(), 1, "integer 8 must NOT equal string \"8\"");
    }

    #[test]
    fn xml_empty_element_is_null() {
        // Design-doc promise (was untested): an empty element
        // maps to JSON null — `equals: null` matches; `equals:
        // ""` does not (it is null, not an empty string).
        let xml: &[u8] = b"<Config><empty/></Config>";
        let (tmp, idx) = tempdir_with_files(&[("c.xml", xml)]);
        let as_null = spec_yaml(
            "id: t\nkind: xml_path_equals\npaths: \"c.xml\"\n\
             path: \"$.Config.empty\"\nequals: null\nlevel: error\n",
        );
        assert!(
            xml_path_equals_build(&as_null)
                .unwrap()
                .evaluate(&ctx(tmp.path(), &idx))
                .unwrap()
                .is_empty(),
            "an empty element must equal null"
        );
        let as_empty_str = spec_yaml(
            "id: t\nkind: xml_path_equals\npaths: \"c.xml\"\n\
             path: \"$.Config.empty\"\nequals: \"\"\nlevel: error\n",
        );
        assert_eq!(
            xml_path_equals_build(&as_empty_str)
                .unwrap()
                .evaluate(&ctx(tmp.path(), &idx))
                .unwrap()
                .len(),
            1,
            "null must NOT equal the empty string"
        );
    }

    // ─── parse error path ─────────────────────────────────────

    #[test]
    fn evaluate_fires_on_malformed_input() {
        let spec = spec_yaml(
            "id: t\n\
             kind: json_path_equals\n\
             paths: \"package.json\"\n\
             path: \"$.name\"\n\
             equals: \"x\"\n\
             level: error\n",
        );
        let rule = json_path_equals_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("package.json", b"{not valid json")]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "malformed JSON should fire one violation");
    }
}
