//! Structured-query rule family:
//! `{json,yaml,toml,xml}_path_{equals,matches,absent}`.
//!
//! The eight value-checking kinds (`equals` / `matches`) share a
//! single implementation that varies along two axes:
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
//! ## `{json,yaml,toml,xml}_path_absent`
//!
//! A third op — **existence** — mirrors `file_absent` for a path:
//! the query must select *nothing*, and any match produces exactly
//! one file-level violation (never per-match, so a `$[?…]` filter
//! that fans out over every root key still yields one violation).
//! `equals` / `matches` / `if_present` don't apply. Shipped for all
//! four formats, kept symmetric with `equals`/`matches` by the
//! `structured_family_is_symmetric` test.
//!
//! Unparseable files (bad JSON / YAML / TOML, not-well-formed
//! XML) produce one violation per file. An unparseable file is a
//! documentation problem, not the structured rule's concern —
//! but better to surface it than silently skip.

use std::path::{Path, PathBuf};

use alint_core::{
    Context, Error, Format, Level, PathsSpec, PerFileRule, Result, Rule, RuleSpec, Scope, Violation,
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
    /// Existence assertion: the query must match **nothing**. Any
    /// match produces exactly one file-level violation (no per-match
    /// value check). Resolved early in `evaluate_file`; the per-match
    /// helpers (`check_match`, `match_baseline_key`) are never reached
    /// with this variant.
    Absent,
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

/// Options for the `*_path_absent` kinds. Existence-only: there is no value to
/// compare and no `if_present` (the rule *is* a presence check).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct AbsentOptions {
    /// `JSONPath` expression rooted at `$`. The rule fires one violation per
    /// file if the query matches any node (the path must be absent).
    path: String,
}

/// schemars-derived options schema for the `*_path_absent` kinds.
#[must_use]
pub fn absent_options_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(AbsentOptions))
        .expect("AbsentOptions JSON schema serializes")
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
                // Cap the read so a multi-GB file matched here can't OOM the
                // run; over-cap or unreadable → skip (M3).
                let Ok(bytes) = crate::io::read_capped(&full) else {
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
                // Cap the read (multi-GB OOM guard, M3); permission / race /
                // over-cap → silent skip, like other content rules.
                let Ok(bytes) = crate::io::read_capped(&full) else {
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
        // Existence assertion: the query must select nothing. Any match is
        // exactly one file-level violation -- never per-match, so a `$[?...]`
        // filter that fans out over every root key still yields one violation.
        if matches!(self.op, Op::Absent) {
            if matches.is_empty() {
                return Ok(Vec::new());
            }
            let msg = self.message.clone().unwrap_or_else(|| {
                format!(
                    "JSONPath `{}` matched, but this rule requires it to be absent",
                    self.path_src
                )
            });
            return Ok(vec![
                Violation::new(msg)
                    .with_path(std::sync::Arc::<Path>::from(path))
                    .with_baseline_key(format!("{}\u{0}absent", self.path_src)),
            ]);
        }
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
        // Unreached: `Absent` violations are file-level (built in `evaluate_file`).
        Op::Absent => "absent".to_string(),
    };
    format!("{path_src}\u{0}{op_descr}\u{0}got {m}")
}

/// Return `Some(message)` if the match fails the op; `None` if it passes.
fn check_match(m: &Value, op: &Op) -> Option<String> {
    match op {
        // `Absent` is resolved file-level in `evaluate_file` and never reaches
        // the per-match loop; a stray call is a pass.
        Op::Absent => None,
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
    // Truncate on a char boundary, not a byte index: `raw` is the JSON
    // rendering of an untrusted matched value (serde_json does not escape
    // non-ASCII), so a fixed byte slice `&raw[..80]` can split a multibyte
    // codepoint and panic. With no catch_unwind on the per-file path that
    // aborts the whole parallel `check` run (and the LSP server).
    // `char_indices().nth(80)` yields the byte offset of the 81st char:
    // `None` (≤ 80 chars) returns the string whole; otherwise we slice at
    // that guaranteed-valid boundary.
    match raw.char_indices().nth(80) {
        None => raw,
        Some((boundary, _)) => format!("{}…", &raw[..boundary]),
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

pub fn json_path_absent_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_absent(spec, Format::Json, "json_path_absent")
}

pub fn yaml_path_absent_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_absent(spec, Format::Yaml, "yaml_path_absent")
}

pub fn toml_path_absent_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_absent(spec, Format::Toml, "toml_path_absent")
}

pub fn xml_path_absent_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_absent(spec, Format::Xml, "xml_path_absent")
}

pub fn dotenv_path_equals_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_equals(spec, Format::Dotenv, "dotenv_path_equals")
}

pub fn dotenv_path_matches_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_matches(spec, Format::Dotenv, "dotenv_path_matches")
}

pub fn dotenv_path_absent_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_absent(spec, Format::Dotenv, "dotenv_path_absent")
}

fn build_absent(spec: &RuleSpec, format: Format, kind_label: &str) -> Result<Box<dyn Rule>> {
    let paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(&spec.id, format!("{kind_label} requires a `paths` field"))
    })?;
    let opts: AbsentOptions = spec
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
        op: Op::Absent,
        if_present: false,
    }))
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

    #[test]
    fn short_render_truncates_on_a_char_boundary_without_panicking() {
        // Regression: `short_render` byte-sliced `&raw[..80]`, which panics
        // when an untrusted matched value puts a multibyte codepoint across
        // byte 80 — crashing the whole parallel run (and the LSP). 78 ASCII
        // + `é`s: serde_json quotes the string, so the byte-80 boundary
        // lands mid-`é`. Must truncate (with the ellipsis), not abort.
        let value = Value::String(format!("{}{}", "a".repeat(78), "é".repeat(8)));
        let rendered = short_render(&value);
        assert!(rendered.ends_with('…'), "expected truncation: {rendered}");
        // A short non-ASCII value is returned whole (quoted JSON rendering).
        assert_eq!(short_render(&Value::String("café".to_string())), "\"café\"");
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

    // ─── yaml_path_absent ────────────────────────────────────

    #[test]
    fn yaml_path_absent_passes_when_query_matches_nothing() {
        // The path the rule forbids isn't there -> pass.
        let spec = spec_yaml(
            "id: t\n\
             kind: yaml_path_absent\n\
             paths: \".github/workflows/*.yml\"\n\
             path: \"$.permissions\"\n\
             level: error\n",
        );
        let rule = yaml_path_absent_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[(
            ".github/workflows/ci.yml",
            b"name: CI\non: push\njobs: {}\n",
        )]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "absent path should pass: {v:?}");
    }

    #[test]
    fn yaml_path_absent_fires_once_when_query_matches() {
        // The forbidden path exists -> exactly one file-level violation.
        let spec = spec_yaml(
            "id: t\n\
             kind: yaml_path_absent\n\
             paths: \".github/workflows/*.yml\"\n\
             path: \"$.permissions\"\n\
             level: error\n",
        );
        let rule = yaml_path_absent_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[(
            ".github/workflows/ci.yml",
            b"name: CI\npermissions: write-all\non: push\njobs: {}\n",
        )]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "present path should fire once: {v:?}");
    }

    #[test]
    fn yaml_path_absent_filter_fanout_collapses_to_one_violation() {
        // A root-level filter `$[?…]` selects *every* top-level key when the
        // predicate holds (N nodes). Absent-mode must still yield exactly ONE
        // file-level violation -- this is the whole point of the kind (vs. a
        // `yaml_path_equals` + sentinel filter, which fans out to N warnings).
        let spec = spec_yaml(
            "id: t\n\
             kind: yaml_path_absent\n\
             paths: \"w.yml\"\n\
             path: \"$[?($.permissions == 'write-all')]\"\n\
             level: error\n",
        );
        let rule = yaml_path_absent_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[(
            "w.yml",
            b"name: CI\npermissions: write-all\non: push\njobs: {}\n",
        )]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(
            v.len(),
            1,
            "filter fan-out must collapse to one file-level violation: {v:?}"
        );
    }

    #[test]
    fn yaml_path_absent_rejects_value_and_if_present_options() {
        // `if_present` / `equals` / `matches` are not valid on an absent rule
        // (deny_unknown_fields).
        let spec = spec_yaml(
            "id: t\n\
             kind: yaml_path_absent\n\
             paths: \"w.yml\"\n\
             path: \"$.x\"\n\
             if_present: true\n\
             level: error\n",
        );
        assert!(
            yaml_path_absent_build(&spec).is_err(),
            "if_present must be rejected on yaml_path_absent"
        );
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
        let depth = alint_core::MAX_XML_DEPTH + 50;
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
    fn xml_depth_beyond_parse_recursion_limit_is_rejected_pre_parse_not_aborted() {
        // The MAX_XML_DEPTH guard above is POST-parse; a document deep enough to
        // overflow `roxmltree::Document::parse` itself (tens of thousands of
        // levels) aborts the whole process before that guard runs. The pre-parse
        // `xml_depth_within_limit` scan must reject it as an ordinary parse error.
        // (Without the pre-scan this test would SIGABRT the whole test binary.)
        let depth = 100_000;
        let xml = format!("{}deep{}", "<a>".repeat(depth), "</a>".repeat(depth));
        let spec = spec_yaml(
            "id: t\nkind: xml_path_equals\npaths: \"deep.xml\"\n\
             path: \"$.a\"\nequals: \"x\"\nlevel: error\n",
        );
        let rule = xml_path_equals_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("deep.xml", xml.as_bytes())]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(
            v.len(),
            1,
            "must be one contained parse-error, no abort: {v:?}"
        );
        assert!(v[0].message.contains("depth"), "{}", v[0].message);
    }

    #[test]
    fn yaml_flow_depth_bomb_is_rejected_pre_parse_not_hung() {
        // W2 wiring regression: a `yaml_path_*` rule over a deeply-nested FLOW
        // document (`[[[…`) must be rejected as a contained parse-error, not fed
        // to libyaml (which is super-linear on flow nesting and would hang the
        // run). This exercises the `flow_depth_within_limit` guard *at its
        // structured-query call site* — the yaml_depth unit tests only cover the
        // scanner in isolation, so without this a deleted guard here would pass
        // CI while reopening the DoS.
        let depth = 5000;
        let yaml = format!("x: {}1{}", "[".repeat(depth), "]".repeat(depth));
        let spec = spec_yaml(
            "id: t\nkind: yaml_path_equals\npaths: \"bomb.yml\"\n\
             path: \"$.x\"\nequals: \"1\"\nlevel: error\n",
        );
        let rule = yaml_path_equals_build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("bomb.yml", yaml.as_bytes())]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(
            v.len(),
            1,
            "must be one contained parse-error, no hang: {v:?}"
        );
        assert!(
            v[0].message.contains("depth"),
            "the flow-depth guard message should mention depth: {}",
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
