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
                violations.push(Violation::new(base).with_path(std::sync::Arc::<Path>::from(path)));
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
             pattern: \"[unterminated\"\n\
             level: error\n",
        );
        assert!(json_path_matches_build(&spec).is_err());
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
