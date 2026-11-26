//! `no_duplicate_keys` - detect duplicate keys at any depth across YAML, JSON,
//! dotenv, Java `.properties`, INI, TOML, and HCL (ADR-0050 D6, Gap B1).
//!
//! Runs as a per-file rule (`DispatchClass::PerFile`) so it works in both
//! `version: 1` and `version: 2` configs and executes on the LSP `didChange`
//! hot path with exact line/column spans and first-occurrence related
//! locations.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;

use alint_core::duplicate_keys::{DuplicateKey, MAX_DUPLICATES_PER_FILE, find_duplicate_keys};
use alint_core::{
    AnalysisFailureKind, ByteSpan, Context, Error, Format, Level, PerFileRule, RelatedLocation,
    Result, Rule, RuleSpec, Scope, SourceRange, Violation,
};
use serde::Deserialize;

use crate::io::looks_binary;

fn present<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    match Option::<T>::deserialize(deserializer)? {
        Some(value) => Ok(Some(value)),
        None => Err(serde::de::Error::custom(
            "an optional key written as null; give it a value, or leave the key out",
        )),
    }
}

fn no_default(schema: &mut schemars::Schema) {
    schema.remove("default");
}

/// Explicit `format:` override values for `no_duplicate_keys`.
///
/// XML is intentionally excluded: sibling elements legitimately repeat in XML
/// and duplicate XML attributes are already parse errors in `roxmltree`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(rename = "NoDuplicateKeysFormat")]
enum TargetFormat {
    Json,
    Yaml,
    Yml,
    Toml,
    Dotenv,
    Properties,
    Ini,
    Hcl,
}

impl TargetFormat {
    const fn to_format(self) -> Format {
        match self {
            Self::Json => Format::Json,
            Self::Yaml | Self::Yml => Format::Yaml,
            Self::Toml => Format::Toml,
            Self::Dotenv => Format::Dotenv,
            Self::Properties => Format::Properties,
            Self::Ini => Format::Ini,
            Self::Hcl => Format::Hcl,
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend(
    "allOf" = [{
        "if": { "required": ["strict", "format"], "properties": { "format": { "type": "string" } } },
        "then": { "properties": { "format": { "const": "json" } } }
    }]
))]
struct Options {
    /// Override the auto-detected file format. When omitted, inferred per file
    /// from its extension (`.json`, `.yaml`/`.yml`, `.toml`, `.properties`,
    /// `.ini`/`.cfg`/`.editorconfig`, `.hcl`/`.tf`/`.tfvars`/`.nomad`, or
    /// `.env`/`.env.*`). Explicit `format: xml` is rejected at config load.
    #[serde(default)]
    format: Option<TargetFormat>,
    /// When `true`, enforce strict RFC 8259 JSON parsing on JSON files
    /// (rejecting comments, trailing commas, leading UTF-8 BOM, and empty
    /// files; ADR-0050 D7). Rejected at build time when `format:` is explicitly
    /// non-JSON.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "bool", transform = no_default)]
    strict: Option<bool>,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct NoDuplicateKeysRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    format_override: Option<Format>,
    strict: bool,
    explicit_sensitive: bool,
}

impl Rule for NoDuplicateKeysRule {
    alint_core::rule_common_impl!();

    fn path_contents_sensitive_by_default(&self) -> bool {
        self.format_override.is_some_and(Format::redacts_by_default)
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
        let all_sensitive = explicit
            || manifest_sensitive
            || self.explicit_sensitive
            || self.path_contents_sensitive_by_default();
        for entry in self.scope.candidate_files(ctx.index) {
            if !self.scope.matches(&entry.path, ctx.index) {
                continue;
            }
            if all_sensitive
                || (self.format_override.is_none()
                    && Format::detect_from_path(&entry.path)
                        .is_some_and(Format::redacts_by_default))
            {
                visit(&entry.path);
            }
        }
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        alint_core::eval_per_file(self, ctx)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for NoDuplicateKeysRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn enforces_findings_per_rule(&self) -> bool {
        true
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        Ok(self.evaluate_file_inner(path, bytes))
    }
}

impl NoDuplicateKeysRule {
    fn evaluate_file_inner(&self, path: &Path, bytes: &[u8]) -> Vec<Violation> {
        let format = self
            .format_override
            .or_else(|| Format::detect_from_path(path));
        let strict_json = self.strict && format == Some(Format::Json);

        // Under strict RFC 8259 JSON, do NOT bypass binary/NUL bytes (an
        // unescaped NUL or UTF-16 BOM is invalid RFC 8259 JSON and must surface
        // as `resource_parse`). For tolerant formats caught by broad globs,
        // skip genuinely binary files cleanly.
        if !strict_json && looks_binary(bytes) {
            return Vec::new();
        }
        let Some(format) = format else {
            return vec![
                Violation::new(
                    "could not detect format from extension; pass `format:` \
                     (`json` / `yaml` / `toml` / `dotenv` / `properties` / `ini` / `hcl`) on the rule",
                )
                .with_path(Arc::<Path>::from(path))
                .as_analysis_failure(AnalysisFailureKind::ResourceParse),
            ];
        };

        // Require valid UTF-8 so RFC 8259 §8.1 holds under `strict: true` and
        // distinct non-UTF-8 byte keys never collapse onto `U+FFFD` as a false
        // positive duplicate.
        let Ok(text) = std::str::from_utf8(bytes) else {
            let msg = if format.redacts_by_default() || self.explicit_sensitive {
                format!(
                    "not a valid {} document (parse diagnostic redacted)",
                    format.label()
                )
            } else {
                format!(
                    "not a valid {} document: input is not valid UTF-8",
                    format.label()
                )
            };
            return vec![
                Violation::new(msg)
                    .with_path(Arc::<Path>::from(path))
                    .as_analysis_failure(AnalysisFailureKind::ResourceParse),
            ];
        };

        let scan = match find_duplicate_keys(format, text, strict_json) {
            Ok(scan) => scan,
            Err(err) => {
                // Never echo parser diagnostics for secret-bearing formats or
                // explicitly sensitive rules since parse errors may quote values.
                let msg = if format.redacts_by_default() || self.explicit_sensitive {
                    format!(
                        "not a valid {} document (parse diagnostic redacted)",
                        format.label()
                    )
                } else {
                    format!("not a valid {} document: {err}", format.label())
                };
                return vec![
                    Violation::new(msg)
                        .with_path(Arc::<Path>::from(path))
                        .as_analysis_failure(AnalysisFailureKind::ResourceParse),
                ];
            }
        };

        if scan.duplicates.is_empty() && !scan.capped {
            return Vec::new();
        }

        let ranges = resolve_duplicate_ranges(text, &scan.duplicates);
        let mut violations = Vec::with_capacity(scan.duplicates.len() + usize::from(scan.capped));
        let path_arc: Arc<Path> = Arc::from(path);
        // Memoize SHA-256 baseline tokens for large (>128 B) shared Arc<str>
        // keys/sections within this immutable `scan` so 1,000 repeats hash once.
        let mut long_token_cache: HashMap<(usize, usize), String> = HashMap::new();

        for (dup, (dup_range, first_range)) in scan.duplicates.iter().zip(ranges) {
            violations.push(self.build_finding(
                format,
                path,
                &path_arc,
                dup,
                dup_range,
                first_range,
                &mut long_token_cache,
            ));
        }

        if scan.capped {
            violations.push(
                Violation::new(format!(
                    "file exceeded the maximum of {MAX_DUPLICATES_PER_FILE} duplicate keys; \
                     remaining duplicates were omitted"
                ))
                .with_path(path_arc)
                .as_analysis_failure(AnalysisFailureKind::Evaluation),
            );
        }

        violations
    }

    #[allow(clippy::too_many_arguments)]
    fn build_finding(
        &self,
        format: Format,
        path: &Path,
        path_arc: &Arc<Path>,
        dup: &DuplicateKey,
        dup_range: Option<SourceRange>,
        first_range: Option<SourceRange>,
        long_token_cache: &mut HashMap<(usize, usize), String>,
    ) -> Violation {
        let msg = if let Some(custom) = &self.message {
            custom.clone()
        } else if self.explicit_sensitive {
            "duplicate key <redacted>".to_string()
        } else {
            let shown_key = escape_display_truncated(&dup.key, 128);
            let first_hint = first_range
                .as_ref()
                .map(|r| {
                    format!(
                        " (first defined at line {}, column {})",
                        r.start_line(),
                        r.start_column()
                    )
                })
                .unwrap_or_default();
            if let Some(section) = &dup.section {
                let shown_sec = escape_display_truncated(section, 128);
                format!("duplicate key `{shown_key}` in section `[{shown_sec}]`{first_hint}")
            } else {
                format!("duplicate key `{shown_key}`{first_hint}")
            }
        };

        let mut finding = Violation::new(msg).with_path(Arc::clone(path_arc));
        if let Some(range) = dup_range {
            finding = if self.explicit_sensitive {
                finding.with_location(range.start_line(), range.start_column())
            } else {
                finding.with_range(range)
            };
        }
        if !self.explicit_sensitive
            && let Some(range) = first_range
        {
            let rel = RelatedLocation::new(path.to_path_buf(), "first occurrence of key")
                .with_range(range);
            finding = finding.with_related_location(rel);
        }
        let baseline_key_src = dup.canonical_key.as_deref().unwrap_or(dup.key.as_ref());
        let baseline_key_part = cached_bounded_key_token(baseline_key_src, long_token_cache);
        let baseline_sec_part = dup.section.as_deref().map_or_else(
            || std::borrow::Cow::Borrowed(""),
            |sec| cached_bounded_key_token(sec, long_token_cache),
        );
        let baseline = format!(
            "dup\u{0}{}\u{0}{baseline_sec_part}\u{0}{baseline_key_part}",
            format.label(),
        );
        finding = finding.with_baseline_key(baseline);
        if self.explicit_sensitive {
            let locator = format!("{}\0{}\0dup", format.label(), crate::slash(path));
            if self.message.is_some() {
                if let Some(sec) = dup.section.as_deref() {
                    finding.redact_evidence(&self.id, &locator, [dup.key.as_ref(), sec]);
                } else {
                    finding.redact_evidence(&self.id, &locator, [dup.key.as_ref()]);
                }
            } else {
                // Built-in message already renders `"duplicate key <redacted>"`
                // without key or section text; passing raw key/section tokens
                // would scrub fixed template words (e.g. `"e"` or `"key"`),
                // creating a substring oracle.
                finding.redact_evidence(&self.id, &locator, std::iter::empty());
            }
        }
        finding
    }
}

fn escape_display_truncated(raw: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(raw.len().min(max_chars));
    for (count, c) in raw.chars().enumerate() {
        if count >= max_chars {
            out.push_str("...");
            break;
        }
        if c.is_control() || alint_core::invisible::is_deceptive_on_display(c) {
            let _ = write!(out, "\\u{{{:X}}}", c as u32);
        } else {
            out.push(c);
        }
    }
    out
}

fn cached_bounded_key_token<'a>(
    raw: &'a str,
    cache: &mut HashMap<(usize, usize), String>,
) -> std::borrow::Cow<'a, str> {
    if raw.len() <= 128 {
        return std::borrow::Cow::Borrowed(raw);
    }
    let key = (raw.as_ptr() as usize, raw.len());
    let token = cache
        .entry(key)
        .or_insert_with(|| bounded_key_token(raw))
        .clone();
    std::borrow::Cow::Owned(token)
}

fn bounded_key_token(raw: &str) -> String {
    use sha2::{Digest as _, Sha256};

    if raw.len() <= 128 {
        return raw.to_string();
    }
    let digest = Sha256::digest(raw.as_bytes());
    let mut out = String::with_capacity(7 + 64);
    out.push_str("sha256:");
    for b in digest {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Resolve every duplicate and first-occurrence byte span to 1-based
/// [`SourceRange`]s in strictly O(N + K log K) time and O(K) memory (avoiding
/// O(K * N) column rescans and large line-starts tables on 32 MiB files).
fn resolve_duplicate_ranges(
    text: &str,
    duplicates: &[DuplicateKey],
) -> Vec<(Option<SourceRange>, Option<SourceRange>)> {
    if duplicates.is_empty() {
        return Vec::new();
    }
    if text.is_ascii() {
        return resolve_ascii_duplicate_ranges(text.as_bytes(), duplicates);
    }

    // Non-ASCII path: collect and sort all unique valid char-boundary offsets
    // once, then resolve every offset in a single linear forward pass.
    let mut targets: Vec<usize> = Vec::with_capacity(duplicates.len() * 4);
    for d in duplicates {
        for off in [
            d.duplicate.start,
            d.duplicate.end,
            d.first.start,
            d.first.end,
        ] {
            let clamped = off.min(text.len());
            if text.is_char_boundary(clamped) {
                targets.push(clamped);
            }
        }
    }
    targets.sort_unstable();
    targets.dedup();

    let mut positions: HashMap<usize, (usize, usize)> = HashMap::with_capacity(targets.len());
    let bom_prefix_len = text.len() - text.trim_start_matches('\u{feff}').len();
    let mut target_idx = 0usize;
    let mut line = 1usize;
    let mut col = 1usize;
    let mut prev_cr = false;
    let mut byte_idx = 0usize;

    for ch in text.chars() {
        while target_idx < targets.len() && targets[target_idx] == byte_idx {
            positions.insert(byte_idx, (line, col));
            target_idx += 1;
        }
        if target_idx >= targets.len() {
            break;
        }
        let ch_len = ch.len_utf8();
        let in_leading_bom = byte_idx < bom_prefix_len;
        if ch == '\r' {
            line += 1;
            col = 1;
            prev_cr = true;
        } else if ch == '\n' {
            if !prev_cr {
                line += 1;
                col = 1;
            }
            prev_cr = false;
        } else {
            prev_cr = false;
            if !in_leading_bom {
                col += 1;
            }
        }
        byte_idx += ch_len;
    }
    while target_idx < targets.len() && targets[target_idx] == byte_idx {
        positions.insert(byte_idx, (line, col));
        target_idx += 1;
    }

    let lookup = |span: ByteSpan| -> Option<SourceRange> {
        let &(sl, sc) = positions.get(&span.start.min(text.len()))?;
        let &(el, ec) = positions.get(&span.end.min(text.len()))?;
        SourceRange::new(sl, sc, el, ec)
    };
    duplicates
        .iter()
        .map(|d| (lookup(d.duplicate), lookup(d.first)))
        .collect()
}

fn resolve_ascii_duplicate_ranges(
    bytes: &[u8],
    duplicates: &[DuplicateKey],
) -> Vec<(Option<SourceRange>, Option<SourceRange>)> {
    let mut targets: Vec<usize> = Vec::with_capacity(duplicates.len() * 4);
    for d in duplicates {
        for off in [
            d.duplicate.start,
            d.duplicate.end,
            d.first.start,
            d.first.end,
        ] {
            targets.push(off.min(bytes.len()));
        }
    }
    targets.sort_unstable();
    targets.dedup();

    let mut positions: HashMap<usize, (usize, usize)> = HashMap::with_capacity(targets.len());
    let mut target_idx = 0usize;
    let mut line = 1usize;
    let mut col = 1usize;
    let mut prev_cr = false;
    let mut i = 0usize;
    while target_idx < targets.len() {
        let target = targets[target_idx];
        while i < target {
            match bytes[i] {
                b'\r' => {
                    line += 1;
                    col = 1;
                    prev_cr = true;
                }
                b'\n' => {
                    if !prev_cr {
                        line += 1;
                        col = 1;
                    }
                    prev_cr = false;
                }
                _ => {
                    prev_cr = false;
                    col += 1;
                }
            }
            i += 1;
        }
        positions.insert(target, (line, col));
        target_idx += 1;
    }

    let lookup = |span: ByteSpan| -> Option<SourceRange> {
        let &(sl, sc) = positions.get(&span.start.min(bytes.len()))?;
        let &(el, ec) = positions.get(&span.end.min(bytes.len()))?;
        SourceRange::new(sl, sc, el, ec)
    };
    duplicates
        .iter()
        .map(|d| (lookup(d.duplicate), lookup(d.first)))
        .collect()
}

pub static META: alint_core::KindMeta = alint_core::KindMeta {
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
    formats: std::borrow::Cow::Borrowed(&[
        Format::Json,
        Format::Yaml,
        Format::Toml,
        Format::Dotenv,
        Format::Properties,
        Format::Ini,
        Format::Hcl,
    ]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[]),
    expect_matches: true,
    trace: alint_core::TraceMeta::SELECTION,
    nested_rules: false,
};

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let _paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(&spec.id, "no_duplicate_keys requires a `paths` field")
    })?;
    if spec
        .extra
        .get("format")
        .and_then(|v| v.as_str())
        .is_some_and(|f| f.eq_ignore_ascii_case("xml"))
    {
        return Err(Error::rule_config(
            &spec.id,
            "no_duplicate_keys does not support `format: xml` (XML sibling elements legitimately \
             repeat, and duplicate XML attributes are already parse errors)",
        ));
    }
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let format_override = opts.format.map(TargetFormat::to_format);
    if opts.strict.is_some() && format_override.is_some_and(|f| f != Format::Json) {
        return Err(Error::rule_config(
            &spec.id,
            "`strict` is only supported for JSON files; remove `strict` or set `format: json`",
        ));
    }
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "no_duplicate_keys has no fix op - alint cannot know which duplicate entry to keep",
        ));
    }
    Ok(Box::new(NoDuplicateKeysRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        format_override,
        strict: opts.strict.unwrap_or(false),
        explicit_sensitive: spec.sensitive,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, tempdir_with_files};

    fn make_spec(paths: &str, extra: &str) -> RuleSpec {
        let yaml = format!(
            "id: dup-check\nkind: no_duplicate_keys\npaths: {paths:?}\nlevel: error\n{extra}"
        );
        alint_core::yaml::from_str(&yaml).unwrap()
    }

    #[test]
    fn build_rejects_xml_and_strict_on_non_json() {
        let err = build(&make_spec("**/*.xml", "format: xml\n")).unwrap_err();
        assert!(err.to_string().contains("xml"), "{err}");

        let err = build(&make_spec("**/*.yml", "format: yaml\nstrict: true\n")).unwrap_err();
        assert!(err.to_string().contains("strict"), "{err}");

        let err = build(&make_spec("**/*.yml", "format: yaml\nstrict: false\n")).unwrap_err();
        assert!(err.to_string().contains("strict"), "{err}");

        assert!(build(&make_spec("**/*.json", "format: json\nstrict: true\n")).is_ok());
        assert!(build(&make_spec("**/*.json", "strict: true\n")).is_ok());
    }

    #[test]
    fn yaml_and_json_report_exact_spans_and_first_occurrence_related_location() {
        let (dir, index) = tempdir_with_files(&[
            (
                "workflow.yml",
                b"jobs:\n  build:\n    runs-on: ubuntu\njobs:\n  test:\n    runs-on: ubuntu\n",
            ),
            ("steps.yml", b"steps:\n  - name: a\n    name: b\n"),
            ("ok.yml", b"a: 1\n---\na: 2\n"),
            ("bad.json", b"{\n  \"k\": 1,\n  \"k\": 2\n}\n"),
            ("data.xml", b"<r><a/><a/></r>"),
        ]);
        let c = ctx(dir.path(), &index);
        let rule = build(&make_spec("*", "")).unwrap();
        let vs = rule.evaluate(&c).unwrap();
        assert_eq!(vs.len(), 4, "{vs:#?}");

        let wf = vs
            .iter()
            .find(|v| v.path.as_deref() == Some(Path::new("workflow.yml")))
            .unwrap();
        assert_eq!(wf.line, Some(4));
        assert_eq!(wf.column, Some(1));
        assert_eq!(wf.related_locations.len(), 1);
        assert_eq!(
            wf.related_locations[0]
                .range
                .as_ref()
                .map(SourceRange::start_line),
            Some(1)
        );

        let steps = vs
            .iter()
            .find(|v| v.path.as_deref() == Some(Path::new("steps.yml")))
            .unwrap();
        assert_eq!(steps.line, Some(3));
        assert!(steps.message.contains("`name`"));

        let xml = vs
            .iter()
            .find(|v| v.path.as_deref() == Some(Path::new("data.xml")))
            .unwrap();
        assert_eq!(
            xml.analysis_failure_kind(),
            Some(AnalysisFailureKind::ResourceParse)
        );
    }

    #[test]
    fn dotenv_never_quotes_values_and_registers_sensitive_paths() {
        let (dir, index) = tempdir_with_files(&[
            (".env", b"API_KEY=super_secret_1\nAPI_KEY=super_secret_2\n"),
            ("tsconfig.json", b"{\n  // comment\n  \"a\": 1\n}\n"),
            ("bad_utf8.json", b"{\"a\": \"\xff\"}"),
            ("nul_json.json", b"{\"a\": \"\x00\"}"),
        ]);
        let c = ctx(dir.path(), &index);

        let env_rule = build(&make_spec(".env", "")).unwrap();
        let mut sensitive_paths = Vec::new();
        env_rule.visit_sensitive_paths(false, &c, &mut |p| sensitive_paths.push(p.to_path_buf()));
        assert_eq!(sensitive_paths, vec![Path::new(".env").to_path_buf()]);

        let env_format_rule = build(&make_spec("secrets.txt", "format: dotenv\n")).unwrap();
        assert!(env_format_rule.path_contents_sensitive_by_default());

        let env_vs = env_rule.evaluate(&c).unwrap();
        assert_eq!(env_vs.len(), 1);
        assert!(env_vs[0].message.contains("API_KEY"));
        assert!(!env_vs[0].message.contains("super_secret"));

        for strict_target in ["tsconfig.json", "bad_utf8.json", "nul_json.json"] {
            let strict_rule = build(&make_spec(strict_target, "strict: true\n")).unwrap();
            let strict_vs = strict_rule.evaluate(&c).unwrap();
            assert_eq!(strict_vs.len(), 1, "{strict_target}: {strict_vs:#?}");
            assert_eq!(
                strict_vs[0].analysis_failure_kind(),
                Some(AnalysisFailureKind::ResourceParse),
                "{strict_target}: {strict_vs:#?}"
            );
        }
    }

    #[test]
    fn non_ascii_and_multi_bom_ranges_resolve_in_single_pass() {
        let (dir, index) = tempdir_with_files(&[(
            "unicode.json",
            "\u{feff}\u{feff}{\"é\": 1, \"k\": 2, \"k\": 3}\n".as_bytes(),
        )]);
        let c = ctx(dir.path(), &index);
        let rule = build(&make_spec("unicode.json", "")).unwrap();
        let vs = rule.evaluate(&c).unwrap();
        assert_eq!(vs.len(), 1);
        // Leading BOMs occupy no visible column on line 1.
        assert_eq!(vs[0].line, Some(1));
        assert_eq!(vs[0].column, Some(18));
        assert_eq!(
            vs[0].related_locations[0]
                .range
                .as_ref()
                .map(SourceRange::start_column),
            Some(10)
        );
    }

    #[test]
    fn explicit_sensitive_preserves_builtin_template_and_handles_large_keys() {
        let big_key = "k".repeat(256 * 1024);
        let mut big_yaml = format!("holder: &a \"{big_key}\"\n*a: 1\n");
        for _ in 0..200 {
            big_yaml.push_str("*a: 2\n");
        }
        let (dir, index) = tempdir_with_files(&[
            ("short.yml", b"e: 1\ne: 2\nkey: 3\nkey: 4\ndup: 5\ndup: 6\n"),
            ("crlf.ini", b"[sec]\r\na = 1\ra = 2\r\n"),
            ("big.yml", big_yaml.as_bytes()),
        ]);
        let c = ctx(dir.path(), &index);

        // Built-in sensitive message must stay exact `"duplicate key <redacted>"`
        // even when key names (`"e"`, `"key"`, `"dup"`) are substrings of it.
        let short_rule = build(&make_spec("short.yml", "sensitive: true\n")).unwrap();
        let short_vs = short_rule.evaluate(&c).unwrap();
        assert_eq!(short_vs.len(), 3);
        for v in &short_vs {
            assert!(v.is_sensitive());
            assert_eq!(v.message.as_ref(), "duplicate key <redacted>");
        }

        // Custom message under `sensitive: true` still scrubs the duplicated key/section.
        let custom_rule = build(&make_spec(
            "crlf.ini",
            "sensitive: true\nmessage: \"custom sec a duplicate\"\n",
        ))
        .unwrap();
        let custom_vs = custom_rule.evaluate(&c).unwrap();
        assert_eq!(custom_vs.len(), 1);
        assert!(custom_vs[0].is_sensitive());
        assert!(!custom_vs[0].message.contains("sec"));
        // Verify ASCII CRLF + bare CR line/column resolution.
        assert_eq!(custom_vs[0].line, Some(3));
        assert_eq!(custom_vs[0].column, Some(1));

        // Huge aliased key with `sensitive: true` avoids allocating per-finding debug copies.
        let big_rule = build(&make_spec("big.yml", "sensitive: true\n")).unwrap();
        let big_vs = big_rule.evaluate(&c).unwrap();
        assert_eq!(big_vs.len(), 200);
        for v in &big_vs {
            assert!(v.is_sensitive());
            assert_eq!(v.message.as_ref(), "duplicate key <redacted>");
        }
    }
}
