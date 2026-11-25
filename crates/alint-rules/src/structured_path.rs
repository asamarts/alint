//! Structured-query rule family:
//! `{json,yaml,toml,xml,dotenv,properties,ini,hcl}_path_{equals,matches,absent}`.
//!
//! The sixteen value-checking kinds (`equals` / `matches`) share a
//! single implementation that varies along two axes:
//!
//! - **Format** — `Json`, `Yaml`, `Toml`, `Xml`, `Dotenv`, `Properties`, `Ini`, or `Hcl`. The file is
//!   parsed into a `serde_json::Value` tree regardless (YAML and
//!   TOML coerce through serde -- YAML with its `<<` merge keys
//!   applied and custom tags such as `!Ref` dropped in favour of the
//!   tagged value; XML maps via the xmltodict-style
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
//! ## `{json,yaml,toml,xml,dotenv,properties,ini,hcl}_path_absent`
//!
//! A third op — **existence** — mirrors `file_absent` for a path:
//! the query must select *nothing*, and any match produces exactly
//! one file-level violation (never per-match, so a `$[?…]` filter
//! that fans out over every root key still yields one violation).
//! `equals` / `matches` / `if_present` don't apply. Shipped for all
//! eight formats, kept symmetric with `equals`/`matches` by the
//! `structured_family_is_symmetric` test.
//!
//! Unparseable files (bad JSON / YAML / TOML / dotenv / properties / INI / HCL, not-well-formed
//! XML) produce one violation per file. An unparseable file is a
//! documentation problem, not the structured rule's concern —
//! but better to surface it than silently skip.

use std::path::{Path, PathBuf};

use alint_core::{
    AnalysisFailureKind, Applicability, Context, Error, FixSpec, Fixer, Format, Level, PathsSpec,
    PerFileRule, Result, Rule, RuleSpec, Scope, Violation,
};

use crate::fixers::StructuredFixer;
use crate::io::looks_binary;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use serde_json_path::JsonPath;

/// Collect every literal pattern from `spec` IFF every entry is
/// a literal AND the spec carries no excludes. Returns `None`
/// when any pattern is a glob or there are excludes - the slow
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
    // `literal_glob_path` resolves escapes (a rendered `a\\b` names the
    // directory `a\b`); a raw metacharacter scan looked such a path up verbatim
    // and silently found nothing.
    patterns
        .iter()
        .map(|p| alint_core::template::literal_glob_path(p).map(PathBuf::from))
        .collect()
}

/// Comparison op - keeps the rule builders thin.
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

/// Options shared by non-JSON `*_path_equals` rule kinds.
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

/// Options for `json_path_equals` (adds strict RFC 8259 JSON parsing).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct JsonEqualsOptions {
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
    /// When true, enforce strict RFC 8259 JSON parsing (rejecting comments,
    /// trailing commas, leading UTF-8 BOM, and empty files; ADR-0050 D7).
    #[serde(default)]
    strict: bool,
}

/// Options shared by non-JSON `*_path_matches` rule kinds.
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

/// Options for `json_path_matches` (adds strict RFC 8259 JSON parsing).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct JsonMatchesOptions {
    /// `JSONPath` expression rooted at `$`.
    path: String,
    /// Rust-regex pattern to match against the value at `path`.
    matches: String,
    /// When true, a query returning zero matches is silently OK - only real
    /// matches that fail the op produce violations.
    #[serde(default)]
    if_present: bool,
    /// When true, enforce strict RFC 8259 JSON parsing (rejecting comments,
    /// trailing commas, leading UTF-8 BOM, and empty files; ADR-0050 D7).
    #[serde(default)]
    strict: bool,
}

/// schemars-derived options schema for the non-JSON `*_path_equals` kinds; composed
/// into their `$defs` branches by `xtask gen-schema`. See
/// [`crate::rule_option_schemas`].
#[must_use]
pub fn equals_options_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(EqualsOptions))
        .expect("EqualsOptions JSON schema serializes")
}

/// schemars-derived options schema for `json_path_equals`.
#[must_use]
pub fn json_equals_options_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(JsonEqualsOptions))
        .expect("JsonEqualsOptions JSON schema serializes")
}

/// schemars-derived options schema for the non-JSON `*_path_matches` kinds.
#[must_use]
pub fn matches_options_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(MatchesOptions))
        .expect("MatchesOptions JSON schema serializes")
}

/// schemars-derived options schema for `json_path_matches`.
#[must_use]
pub fn json_matches_options_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(JsonMatchesOptions))
        .expect("JsonMatchesOptions JSON schema serializes")
}

/// Options for the non-JSON `*_path_absent` kinds. Existence-only: there is no value to
/// compare and no `if_present` (the rule *is* a presence check).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct AbsentOptions {
    /// `JSONPath` expression rooted at `$`. The rule fires one violation per
    /// file if the query matches any node (the path must be absent).
    path: String,
}

/// Options for `json_path_absent` (adds strict RFC 8259 JSON parsing).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct JsonAbsentOptions {
    /// `JSONPath` expression rooted at `$`. The rule fires one violation per
    /// file if the query matches any node (the path must be absent).
    path: String,
    /// When true, enforce strict RFC 8259 JSON parsing (rejecting comments,
    /// trailing commas, leading UTF-8 BOM, and empty files; ADR-0050 D7).
    #[serde(default)]
    strict: bool,
}

/// schemars-derived options schema for the non-JSON `*_path_absent` kinds.
#[must_use]
pub fn absent_options_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(AbsentOptions))
        .expect("AbsentOptions JSON schema serializes")
}

/// schemars-derived options schema for `json_path_absent`.
#[must_use]
pub fn json_absent_options_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(JsonAbsentOptions))
        .expect("JsonAbsentOptions JSON schema serializes")
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
    /// scan - same shape as `file_exists`'s fast path. Driven
    /// by the bundled `monorepo/cargo-workspace@v1`'s
    /// `cargo-workspace-member-declares-name` rule, which
    /// `for_each_dir` instantiates with `paths:
    /// "{path}/Cargo.toml"` (purely literal after token
    /// substitution) for every `crates/*` directory; without
    /// the fast path this is the dominant 1M-scale bottleneck.
    literal_paths: Option<Vec<PathBuf>>,
    format: Format,
    /// When `true` (only valid for `Format::Json`), enforce strict RFC 8259
    /// JSON parsing (ADR-0050 D7).
    strict: bool,
    path_expr: JsonPath,
    path_src: String,
    op: Op,
    /// When `true`, a `JSONPath` query that produces zero matches
    /// is silently OK. When `false` (default), a zero-match query
    /// is reported as a single violation - the "value being
    /// enforced doesn't exist" case. Use `true` for predicates
    /// that are conditional on the field being present (e.g.
    /// "every `uses:` in a workflow must be SHA-pinned" - a
    /// workflow with no `uses:` at all shouldn't be flagged).
    if_present: bool,
    /// The located structured fixer, when the rule declares a compatible
    /// `fix:` (`set_value` on `*_path_equals`, `remove_value` on
    /// `*_path_absent`). `None` for `*_path_matches` and unfixed rules.
    fixer: Option<StructuredFixer>,
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

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }

    fn path_contents_sensitive_by_default(&self) -> bool {
        self.format.redacts_by_default()
    }

    fn visit_sensitive_paths(
        &self,
        explicit: bool,
        ctx: &Context<'_>,
        visit: &mut dyn FnMut(&Path),
    ) {
        let manifest_sensitive = self.scope.sensitive_manifest_sources().next().is_some();
        for path in self.scope.sensitive_manifest_sources() {
            visit(path);
        }
        if !explicit && !manifest_sensitive && !self.format.redacts_by_default() {
            return;
        }
        if let Some(literals) = &self.literal_paths {
            for path in literals {
                if ctx.index.contains_file(path) && self.scope.matches(path, ctx.index) {
                    visit(path);
                }
            }
            return;
        }
        for entry in self.scope.candidate_files(ctx.index) {
            if self.scope.matches(&entry.path, ctx.index) {
                visit(&entry.path);
            }
        }
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
                // The scope still applies: a literal `paths:` entry satisfies the
                // include globs trivially, but `scope_filter` can exclude it. Skipping
                // this check let `alint fix` (which evaluates rule-by-rule) edit a file
                // that `check`'s file-major dispatch had excluded.
                if !ctx.index.contains_file(literal) || !self.scope.matches(literal, ctx.index) {
                    continue;
                }
                let full = ctx.root.join(literal);
                // Cap the read so a multi-GB file matched here cannot OOM the
                // run. Every failure class becomes a typed `resource_read`
                // analysis failure, matching `check`'s file-major dispatch.
                let bytes = match crate::io::read_capped(&full) {
                    Ok(b) => b,
                    Err(e) => {
                        violations.push(crate::io::read_cap_error_violation(literal, &e));
                        continue;
                    }
                };
                violations.extend(self.evaluate_file(ctx, literal, &bytes)?);
            }
        } else {
            for entry in self.scope.candidate_files(ctx.index) {
                if !self.scope.matches(&entry.path, ctx.index) {
                    continue;
                }
                let full = ctx.root.join(&entry.path);
                // Cap the read so a multi-GB file matched here cannot OOM the
                // run. Every failure class becomes a typed `resource_read`
                // analysis failure, matching `check`'s file-major dispatch.
                let bytes = match crate::io::read_capped(&full) {
                    Ok(b) => b,
                    Err(e) => {
                        violations.push(crate::io::read_cap_error_violation(&entry.path, &e));
                        continue;
                    }
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
        ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        Ok(self.evaluate_file_inner::<false>(ctx, path, bytes))
    }

    fn evaluate_file_traced(
        &self,
        ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        Ok(self.evaluate_file_inner::<true>(ctx, path, bytes))
    }
}

impl StructuredPathRule {
    fn parse_file_document<const TRACE: bool>(
        &self,
        ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
        strict_json: bool,
        sensitive: bool,
    ) -> std::result::Result<alint_core::SharedValue, Vec<Violation>> {
        let text = if strict_json {
            let Ok(s) = std::str::from_utf8(bytes) else {
                if TRACE {
                    ctx.record_trace_extraction(alint_core::TraceExtractionObservation {
                        inputs: 1,
                        parse_failures: 1,
                        ..alint_core::TraceExtractionObservation::default()
                    });
                }
                return Err(vec![
                    Violation::new("not a valid JSON document: input is not valid UTF-8")
                        .with_path(std::sync::Arc::<Path>::from(path))
                        .as_analysis_failure(AnalysisFailureKind::ResourceParse),
                ]);
            };
            std::borrow::Cow::Borrowed(s)
        } else {
            String::from_utf8_lossy(bytes)
        };
        ctx.parse_structured_strict(path, self.format, &text, self.strict)
            .map_err(|err| {
                if TRACE {
                    ctx.record_trace_extraction(alint_core::TraceExtractionObservation {
                        inputs: 1,
                        parse_failures: 1,
                        ..alint_core::TraceExtractionObservation::default()
                    });
                }
                let mut finding = Violation::new(format!(
                    "not a valid {} document: {err}",
                    self.format.label()
                ))
                .with_path(std::sync::Arc::<Path>::from(path))
                .as_analysis_failure(AnalysisFailureKind::ResourceParse);
                if sensitive {
                    finding.redact_message(&self.id);
                }
                vec![finding]
            })
    }

    fn evaluate_file_inner<const TRACE: bool>(
        &self,
        ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Vec<Violation> {
        // A NUL-bearing / genuinely-binary file caught by a broad glob is skipped
        // (no violation), matching the hygiene fixers' invariant and keeping `check`
        // and `fix` in agreement -- otherwise a PERMISSIVE-format rule (`.properties`
        // / YAML) flags a fixable "value mismatch" on binary junk that `set_value`
        // then cannot resolve, so the fix loops to a false exit-2 (audit M2).
        // `looks_binary` triggers on NUL bytes, NOT high bytes, so a legitimate
        // Latin-1 `.properties` (below) is unaffected.
        let strict_json = self.strict && self.format == Format::Json;
        if !strict_json && looks_binary(bytes) {
            return Vec::new();
        }
        let sensitive = self.format.redacts_by_default();
        let redact = |finding: &mut Violation, category: &str, evidence: &[String]| {
            if sensitive {
                let locator = format!(
                    "{}\0{}\0{}\0{category}",
                    self.format.label(),
                    crate::slash(path),
                    self.path_src
                );
                finding.redact_evidence(&self.id, &locator, evidence.iter().map(String::as_str));
            }
        };
        let root_value =
            match self.parse_file_document::<TRACE>(ctx, path, bytes, strict_json, sensitive) {
                Ok(v) => v,
                Err(violations) => return violations,
            };
        let matches = self.path_expr.query(&root_value);
        if TRACE {
            let cardinality_mismatch = if matches!(self.op, Op::Absent) {
                !matches.is_empty()
            } else {
                matches.is_empty() && !self.if_present
            };
            ctx.record_trace_extraction(alint_core::TraceExtractionObservation {
                inputs: 1,
                values: u64::try_from(matches.len()).expect("query result length fits u64"),
                cardinality_checks: 1,
                cardinality_mismatches: u64::from(cardinality_mismatch),
                ..alint_core::TraceExtractionObservation::default()
            });
            record_structured_trace_preview(ctx, path, &matches, sensitive);
        }
        // Existence assertion: the query must select nothing. Any match is
        // exactly one file-level violation -- never per-match, so a `$[?...]`
        // filter that fans out over every root key still yields one violation.
        if matches!(self.op, Op::Absent) {
            if matches.is_empty() {
                return Vec::new();
            }
            let msg = self.message.clone().unwrap_or_else(|| {
                format!(
                    "JSONPath `{}` matched, but this rule requires it to be absent",
                    self.path_src
                )
            });
            let mut finding = Violation::new(msg)
                .with_path(std::sync::Arc::<Path>::from(path))
                .with_baseline_key(format!("{}\u{0}absent", self.path_src));
            // The matched value drives this finding even though the default
            // message is value-free; mark secret-bearing formats so baseline
            // identity and remediation output stay protected.
            redact(&mut finding, "absent", &[]);
            return vec![finding];
        }
        if matches.is_empty() {
            if self.if_present {
                return Vec::new();
            }
            let msg = self
                .message
                .clone()
                .unwrap_or_else(|| format!("JSONPath `{}` produced no match", self.path_src));
            let finding = Violation::new(msg).with_path(std::sync::Arc::<Path>::from(path));
            return vec![finding];
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
                let mut finding = Violation::new(base)
                    .with_path(std::sync::Arc::<Path>::from(path))
                    .with_baseline_key(match_baseline_key(&self.path_src, &self.op, m));
                let evidence = sensitive.then(|| vec![m.to_string(), short_render(m)]);
                redact(&mut finding, "mismatch", evidence.as_deref().unwrap_or(&[]));
                violations.push(finding);
            }
        }
        violations
    }
}
fn record_structured_trace_preview(
    ctx: &Context<'_>,
    path: &Path,
    matches: &serde_json_path::NodeList<'_>,
    sensitive: bool,
) {
    if !ctx.trace_preview_enabled() {
        return;
    }
    let sensitivity = ctx.trace_evidence_sensitivity(sensitive);
    for (occurrence, value) in matches.iter().enumerate() {
        let occurrence = u64::try_from(occurrence).expect("query result index fits u64");
        ctx.record_trace_preview(alint_core::TracePreviewRow::new(
            path,
            alint_core::TracePathOrigin::RepositorySource,
            None,
            Some(format!("row:{occurrence}")),
            occurrence,
            alint_core::TraceProvenance::Structured,
            sensitivity,
            trace_value_type(value),
            &value.to_string(),
        ));
    }
}

fn trace_value_type(value: &Value) -> alint_core::TraceValueType {
    match value {
        Value::String(_) => alint_core::TraceValueType::String,
        Value::Number(_) => alint_core::TraceValueType::Number,
        Value::Bool(_) => alint_core::TraceValueType::Boolean,
        Value::Null => alint_core::TraceValueType::Null,
        Value::Array(_) => alint_core::TraceValueType::Array,
        Value::Object(_) => alint_core::TraceValueType::Object,
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
/// long values sharing an 80-char prefix into one fingerprint - a masking bug.
/// `Value`'s `Display` is compact JSON with control characters escaped, so a
/// value can never contain a literal `\0` and forge the NUL separators.
fn match_baseline_key(path_src: &str, op: &Op, m: &Value) -> String {
    let op_descr = match op {
        Op::Equals(expected) => format!("== {expected}"),
        // The `matches:` key is shared with the located `replace` fixer -- route
        // it through the single source so the two can never drift.
        Op::Matches(re) => return matches_baseline_key(path_src, re.as_str(), m),
        // Unreached: `Absent` violations are file-level (built in `evaluate_file`).
        Op::Absent => "absent".to_string(),
    };
    format!("{path_src}\u{0}{op_descr}\u{0}got {m}")
}

/// The baseline identity of one failing `matches:` node -- the SINGLE definition
/// of that key, shared with the located `replace` fixer
/// ([`crate::fixers::StructuredFixer::collect_edits`]). `fix --baseline` drops
/// the grandfathered violations upstream and hands the fixer only the LIVE set;
/// the fixer re-derives every node from the file, so it must correlate each
/// candidate back to a live violation by this exact key. If the check side and
/// the fixer ever computed it differently, a grandfathered node would be
/// silently re-fixed (an under-suppression trust bug) -- routing both callers
/// through this fn makes that drift impossible. Output is byte-identical to the
/// former inline `=~ {re}` branch, so recorded baselines stay valid.
pub(crate) fn matches_baseline_key(path_src: &str, matches_regex_src: &str, m: &Value) -> String {
    format!("{path_src}\u{0}=~ {matches_regex_src}\u{0}got {m}")
}

/// Deep equality for `equals:` with numbers compared BY VALUE: `1` equals `1.0`
/// (and `1e2` equals `100`), recursively through arrays and objects. Plain
/// `serde_json::Value` equality distinguishes an integer from a float, so
/// `equals: 1` failed against a document's `1.0` -- and which one a document
/// yields is a parser detail (TOML / YAML keep `1.0` a float, `hcl-rs`
/// normalizes it to `1`). Value equality is the same in every format. Two
/// integers compare exactly (no float rounding of large `i64` / `u64`), and so
/// does an integer against a float: equal only when the float is finite,
/// integral and in the integer's range, and then exactly (`9007199254740993`
/// does NOT equal `9007199254740992.0`, though both round to the same `f64`).
/// Two floats compare as `f64`. No other coercion: `"1"` never equals `1`.
fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => numbers_equal(x, y),
        (Value::Array(xs), Value::Array(ys)) => {
            xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| values_equal(x, y))
        }
        (Value::Object(xs), Value::Object(ys)) => {
            xs.len() == ys.len()
                && xs
                    .iter()
                    .all(|(k, x)| ys.get(k).is_some_and(|y| values_equal(x, y)))
        }
        _ => a == b,
    }
}

/// Exact numeric equality (see [`values_equal`]).
fn numbers_equal(x: &serde_json::Number, y: &serde_json::Number) -> bool {
    match (
        x.as_f64().filter(|_| x.is_f64()),
        y.as_f64().filter(|_| y.is_f64()),
    ) {
        #[allow(clippy::float_cmp)] // exact value equality is the point
        (Some(f), Some(g)) => f == g,
        (Some(f), None) => int_equals_float(y, f),
        (None, Some(g)) => int_equals_float(x, g),
        (None, None) => {
            if let (Some(i), Some(j)) = (x.as_i64(), y.as_i64()) {
                i == j
            } else if let (Some(i), Some(j)) = (x.as_u64(), y.as_u64()) {
                i == j
            } else {
                // One negative i64 and one u64 beyond i64::MAX: never equal.
                false
            }
        }
    }
}

/// Whether integer `n` equals float `f` EXACTLY: `f` must be finite, integral
/// and within the integer type's range; only then is the cast to it lossless.
fn int_equals_float(n: &serde_json::Number, f: f64) -> bool {
    // 2^63 and 2^64 are exactly representable as f64.
    const TWO_63: f64 = 9_223_372_036_854_775_808.0;
    const TWO_64: f64 = 18_446_744_073_709_551_616.0;
    if !f.is_finite() || f.fract() != 0.0 {
        return false;
    }
    if let Some(i) = n.as_i64() {
        #[allow(clippy::cast_possible_truncation)] // range-checked: exact
        return (-TWO_63..TWO_63).contains(&f) && f as i64 == i;
    }
    if let Some(u) = n.as_u64() {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // range-checked
        return (0.0..TWO_64).contains(&f) && f as u64 == u;
    }
    false
}

/// Return `Some(message)` if the match fails the op; `None` if it passes.
fn check_match(m: &Value, op: &Op) -> Option<String> {
    match op {
        // `Absent` is resolved file-level in `evaluate_file` and never reaches
        // the per-match loop; a stray call is a pass.
        Op::Absent => None,
        Op::Equals(expected) => {
            if values_equal(m, expected) {
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

pub static JSON_PATH_EQUALS_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Json]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("set_value")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static JSON_PATH_MATCHES_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Json]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("replace")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static YAML_PATH_EQUALS_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Yaml]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("set_value")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static YAML_PATH_MATCHES_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Yaml]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("replace")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static TOML_PATH_EQUALS_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Toml]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("set_value")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static TOML_PATH_MATCHES_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Toml]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("replace")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static XML_PATH_EQUALS_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Xml]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("set_value")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static XML_PATH_MATCHES_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Xml]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("replace")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static DOTENV_PATH_EQUALS_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Dotenv]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("set_value")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static DOTENV_PATH_MATCHES_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Dotenv]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("replace")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static PROPERTIES_PATH_EQUALS_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Properties]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("set_value")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static PROPERTIES_PATH_MATCHES_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Properties]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("replace")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static INI_PATH_EQUALS_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Ini]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("set_value")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static INI_PATH_MATCHES_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Ini]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("replace")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static HCL_PATH_EQUALS_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Hcl]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("set_value")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static HCL_PATH_MATCHES_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Hcl]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("replace")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static JSON_PATH_ABSENT_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Json]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("remove_value")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static YAML_PATH_ABSENT_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Yaml]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("remove_value")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static TOML_PATH_ABSENT_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Toml]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("remove_value")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static XML_PATH_ABSENT_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Xml]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("remove_value")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static DOTENV_PATH_ABSENT_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Dotenv]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("remove_value")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static PROPERTIES_PATH_ABSENT_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Properties]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("remove_value")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static INI_PATH_ABSENT_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Ini]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("remove_value")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub static HCL_PATH_ABSENT_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[alint_core::Format::Hcl]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("remove_value")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

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

pub fn properties_path_equals_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_equals(spec, Format::Properties, "properties_path_equals")
}

pub fn properties_path_matches_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_matches(spec, Format::Properties, "properties_path_matches")
}

pub fn properties_path_absent_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_absent(spec, Format::Properties, "properties_path_absent")
}

pub fn ini_path_equals_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_equals(spec, Format::Ini, "ini_path_equals")
}

pub fn ini_path_matches_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_matches(spec, Format::Ini, "ini_path_matches")
}

pub fn ini_path_absent_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_absent(spec, Format::Ini, "ini_path_absent")
}

pub fn hcl_path_equals_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_equals(spec, Format::Hcl, "hcl_path_equals")
}

pub fn hcl_path_matches_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_matches(spec, Format::Hcl, "hcl_path_matches")
}

pub fn hcl_path_absent_build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    build_absent(spec, Format::Hcl, "hcl_path_absent")
}

fn build_absent(spec: &RuleSpec, format: Format, kind_label: &str) -> Result<Box<dyn Rule>> {
    let paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(&spec.id, format!("{kind_label} requires a `paths` field"))
    })?;
    let (path, strict) = if format == Format::Json {
        let opts: JsonAbsentOptions = spec
            .deserialize_options()
            .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
        (opts.path, opts.strict)
    } else {
        let opts: AbsentOptions = spec
            .deserialize_options()
            .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
        (opts.path, false)
    };
    let path_expr = JsonPath::parse(&path).map_err(|e| {
        Error::rule_config(
            &spec.id,
            alint_core::jsonpath_diagnostics::format_parse_error(&path, e),
        )
    })?;
    // Only `remove_value` is compatible with a `*_path_absent` kind (it deletes
    // the matched node the rule requires to be absent). Any other op is a config
    // error rather than a silently-ignored `fix:`. Unsafe by default.
    let fixer = match &spec.fix {
        Some(FixSpec::RemoveValue { remove_value }) => Some(
            StructuredFixer::remove(
                format,
                path_expr.clone(),
                path.clone(),
                remove_value.applicability.unwrap_or(Applicability::Unsafe),
            )
            .with_strict(strict),
        ),
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "fix.{} is not compatible with {kind_label} (only `remove_value` is)",
                    other.op_name()
                ),
            ));
        }
        None => None,
    };
    Ok(Box::new(StructuredPathRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        literal_paths: extract_literal_paths(paths),
        format,
        strict,
        path_expr,
        path_src: path,
        op: Op::Absent,
        if_present: false,
        fixer,
    }))
}

fn build_equals(spec: &RuleSpec, format: Format, kind_label: &str) -> Result<Box<dyn Rule>> {
    let paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(&spec.id, format!("{kind_label} requires a `paths` field"))
    })?;
    let (path, equals, if_present, strict) = if format == Format::Json {
        let opts: JsonEqualsOptions = spec
            .deserialize_options()
            .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
        (opts.path, opts.equals, opts.if_present, opts.strict)
    } else {
        let opts: EqualsOptions = spec
            .deserialize_options()
            .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
        (opts.path, opts.equals, opts.if_present, false)
    };
    let path_expr = JsonPath::parse(&path).map_err(|e| {
        Error::rule_config(
            &spec.id,
            alint_core::jsonpath_diagnostics::format_parse_error(&path, e),
        )
    })?;
    // Only `set_value` is compatible with a `*_path_equals` kind (it overwrites
    // the matched node with the rule's `equals` value). Safe by default, but the
    // fixer only emits at tier for a scalar replacing an existing scalar
    // (`StructuredFixer::collect_edits`); anything else declines.
    let fixer = match &spec.fix {
        Some(FixSpec::SetValue { set_value }) => Some(
            StructuredFixer::set(
                format,
                path_expr.clone(),
                path.clone(),
                equals.clone(),
                set_value.applicability.unwrap_or(Applicability::Safe),
            )
            .with_strict(strict),
        ),
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "fix.{} is not compatible with {kind_label} (only `set_value` is)",
                    other.op_name()
                ),
            ));
        }
        None => None,
    };
    Ok(Box::new(StructuredPathRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        literal_paths: extract_literal_paths(paths),
        format,
        strict,
        path_expr,
        path_src: path,
        op: Op::Equals(equals),
        if_present,
        fixer,
    }))
}

fn build_matches(spec: &RuleSpec, format: Format, kind_label: &str) -> Result<Box<dyn Rule>> {
    let paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(&spec.id, format!("{kind_label} requires a `paths` field"))
    })?;
    let (path, matches, if_present, strict) = if format == Format::Json {
        let opts: JsonMatchesOptions = spec
            .deserialize_options()
            .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
        (opts.path, opts.matches, opts.if_present, opts.strict)
    } else {
        let opts: MatchesOptions = spec
            .deserialize_options()
            .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
        (opts.path, opts.matches, opts.if_present, false)
    };
    let path_expr = JsonPath::parse(&path).map_err(|e| {
        Error::rule_config(
            &spec.id,
            alint_core::jsonpath_diagnostics::format_parse_error(&path, e),
        )
    })?;
    let re = Regex::new(&matches)
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid regex {matches:?}: {e}")))?;
    // `*_path_matches` accepts ONLY the `replace` op: the value at `path` is
    // rewritten (by the fix's OWN `pattern:` -> `replacement:`) so it satisfies the
    // rule's `matches:` -- which stays the check + re-verify target. Any other op is
    // a config error. Unsafe by default (a regex rewrite is not behavior-preserving).
    let fixer = match &spec.fix {
        Some(FixSpec::Replace { replace }) => {
            let Some(pattern) = &replace.pattern else {
                return Err(Error::rule_config(
                    &spec.id,
                    format!(
                        "fix.replace on {kind_label} requires a `pattern:` (the search \
                         regex applied to the value at `path`); the rule's `matches:` is \
                         the check target, not the search"
                    ),
                ));
            };
            let search = Regex::new(pattern).map_err(|e| {
                Error::rule_config(
                    &spec.id,
                    format!("invalid fix.replace.pattern {pattern:?}: {e}"),
                )
            })?;
            Some(
                StructuredFixer::replace(
                    format,
                    path_expr.clone(),
                    path.clone(),
                    search,
                    replace.replacement.clone(),
                    matches.clone(),
                    replace.applicability.unwrap_or(Applicability::Unsafe),
                )
                .with_strict(strict),
            )
        }
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "fix.{} is not compatible with {kind_label} (only `replace` is)",
                    other.op_name()
                ),
            ));
        }
        None => None,
    };
    Ok(Box::new(StructuredPathRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        literal_paths: extract_literal_paths(paths),
        format,
        strict,
        path_expr,
        path_src: path,
        op: Op::Matches(re),
        if_present,
        fixer,
    }))
}

#[cfg(test)]
#[path = "structured_path_tests.rs"]
mod tests;
