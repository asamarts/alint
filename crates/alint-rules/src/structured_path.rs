//! Structured-query rule family: `{json,yaml,toml}_path_{equals,matches}`.
//!
//! Six rule kinds share a single implementation that varies
//! along two axes:
//!
//! - **Format** — `Json`, `Yaml`, or `Toml`. The file is parsed
//!   into a `serde_json::Value` tree regardless (YAML and TOML
//!   values coerce through serde), so the `JSONPath` engine only
//!   has to reason about one tree shape.
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
//! Unparseable files (bad JSON / YAML / TOML) produce one
//! violation per file. An unparseable file is a documentation
//! problem, not the structured rule's concern — but better to
//! surface it than silently skip.

use std::io::Read;

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use serde_json_path::JsonPath;

/// Which YAML-flavoured parser to use on the target file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Yaml,
    Toml,
}

impl Format {
    pub(crate) fn parse(self, text: &str) -> std::result::Result<Value, String> {
        match self {
            Self::Json => serde_json::from_str(text).map_err(|e| e.to_string()),
            Self::Yaml => serde_yaml_ng::from_str(text).map_err(|e| e.to_string()),
            Self::Toml => toml::from_str(text).map_err(|e| e.to_string()),
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Yaml => "YAML",
            Self::Toml => "TOML",
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
            _ => None,
        }
    }
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
#[derive(Debug, Deserialize)]
struct EqualsOptions {
    path: String,
    equals: Value,
    #[serde(default)]
    if_present: bool,
}

/// Options shared by every `*_path_matches` rule kind.
#[derive(Debug, Deserialize)]
struct MatchesOptions {
    path: String,
    matches: String,
    #[serde(default)]
    if_present: bool,
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
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path) {
                continue;
            }
            let full = ctx.root.join(&entry.path);
            let Ok(text) = read_to_string(&full) else {
                // permission / race — silent skip, like other
                // content rules
                continue;
            };
            let root_value = match self.format.parse(&text) {
                Ok(v) => v,
                Err(err) => {
                    violations.push(
                        Violation::new(format!(
                            "not a valid {} document: {err}",
                            self.format.label()
                        ))
                        .with_path(&entry.path),
                    );
                    continue;
                }
            };
            let matches = self.path_expr.query(&root_value);
            if matches.is_empty() {
                if self.if_present {
                    continue;
                }
                let msg = self
                    .message
                    .clone()
                    .unwrap_or_else(|| format!("JSONPath `{}` produced no match", self.path_src));
                violations.push(Violation::new(msg).with_path(&entry.path));
                continue;
            }
            for m in matches.iter() {
                if let Some(v) = check_match(m, &self.op) {
                    let base = self.message.clone().unwrap_or(v);
                    violations.push(Violation::new(base).with_path(&entry.path));
                }
            }
        }
        Ok(violations)
    }
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

fn read_to_string(path: &std::path::Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut s = String::new();
    f.read_to_string(&mut s)?;
    Ok(s)
}

// ---------------------------------------------------------------
// Builders
//
// Six thin wrappers per (Format, Op) combination. Each consumes
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

fn build_equals(spec: &RuleSpec, format: Format, kind_label: &str) -> Result<Box<dyn Rule>> {
    let paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(&spec.id, format!("{kind_label} requires a `paths` field"))
    })?;
    let opts: EqualsOptions = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let path_expr = JsonPath::parse(&opts.path).map_err(|e| {
        Error::rule_config(&spec.id, format!("invalid JSONPath {:?}: {e}", opts.path))
    })?;
    Ok(Box::new(StructuredPathRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
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
        Error::rule_config(&spec.id, format!("invalid JSONPath {:?}: {e}", opts.path))
    })?;
    let re = Regex::new(&opts.matches).map_err(|e| {
        Error::rule_config(&spec.id, format!("invalid regex {:?}: {e}", opts.matches))
    })?;
    Ok(Box::new(StructuredPathRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        format,
        path_expr,
        path_src: opts.path,
        op: Op::Matches(re),
        if_present: opts.if_present,
    }))
}
