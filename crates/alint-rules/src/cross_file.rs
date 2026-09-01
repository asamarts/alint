//! `cross_file` — a value (or set of values, or the whole content,
//! or a set of paths) extracted from one authoritative `source`
//! file must hold a `relation:` to one or more `targets` (or the
//! filesystem). The unified cross-file value-relation kind
//! (architecture-synthesis primitive A): one kind, a `relation:`
//! knob, over the shared `crate::extract` + `normalize`. Design +
//! open questions: `docs/design/v0.12/cross_file.md`.
//!
//! `cross_file_value_equals` (v0.10) is a registered **alias** for
//! this kind with `relation` defaulting to `equals`; every existing
//! config is byte-compatible.
//!
//! `relation` groups into three shapes, validated in `build`:
//! - **value** (`equals` | `subset` | `superset` | `set_equals`):
//!   `source.extract` + `targets` (each with `extract`).
//! - **`identical`**: whole-file byte identity — `targets` with NO
//!   `extract`, no `source.extract` (optional `skip_header_lines`).
//! - **`resolves`**: each extracted source path must exist on disk —
//!   `source.extract`, NO `targets` (the forward half of
//!   `registry_paths_resolve`, which keeps its richer ergonomics).
//!
//! ```yaml
//! - id: workspace-versions-coherent
//!   kind: cross_file
//!   source:  { file: Cargo.toml, extract: { toml: "$.workspace.package.version" } }
//!   targets: { files: "crates/*/Cargo.toml", extract: { toml: "$.package.version" } }
//!   relation: equals               # equals (default) | subset | superset | set_equals
//!
//! # identical — the crate README must mirror the workspace README byte-for-byte.
//! - id: readme-mirrors-root
//!   kind: cross_file
//!   source:  { file: README.md }
//!   targets: { files: "crates/*/README.md" }
//!   relation: identical
//! ```

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use alint_core::{
    Context, Error, Extract, ExtractSpec, Level, Result, Rule, RuleSpec, Scope, Violation,
    extract_values, is_non_literal,
};
use serde::Deserialize;

/// The file whose extracted value(s) form the reference side of the relation:
/// a single `{ file, extract }`, or (set relations only) `{ files: <glob>,
/// extract }` whose matches are unioned into one set.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("oneOf" = [{"required": ["file"]}, {"required": ["files"]}]))]
struct SourceSpec {
    /// A single source file.
    #[serde(default)]
    file: Option<String>,
    /// A glob whose matches are read and whose extracted values are UNIONED
    /// into one set, for the set relations only (`subset` / `superset` /
    /// `set_equals`).
    #[serde(default)]
    files: Option<String>,
    /// The extraction to apply. Absent for `identical` (whole-file); required
    /// otherwise.
    #[serde(default)]
    extract: Option<ExtractSpec>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct TargetEntrySpec {
    /// A single target file.
    file: String,
    /// The extraction to apply to this target (absent for `identical`).
    #[serde(default)]
    extract: Option<ExtractSpec>,
}

/// `targets:` is either a `{ files: <glob>, extract: … }` map
/// (form a — one query applied per glob match) or a sequence of
/// `{ file, extract }` (form b — heterogeneous pins). A YAML map
/// vs a sequence are structurally distinct, so an untagged enum
/// decodes them unambiguously. `extract` is absent for `identical`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(
    untagged,
    expecting = "a `{ files, extract }` map, or a list of `{ file, extract }` entries"
)]
enum TargetsSpec {
    /// Form a: one query applied per glob match.
    // The hand-written schema rejected unknown keys in this form; schemars does
    // not emit `additionalProperties: false` for an untagged struct variant, so
    // restore it schema-side (the loader stays lenient, as it was before).
    #[schemars(extend("additionalProperties" = false))]
    Glob {
        files: String,
        #[serde(default)]
        extract: Option<ExtractSpec>,
    },
    /// Form b: a sequence of heterogeneous `{ file, extract }` pins.
    List(Vec<TargetEntrySpec>),
}

/// The relation the source must hold to each target. `equals` is the
/// 1:1 scalar case (the released `cross_file_value_equals`); the set
/// relations compare extracted sets; `identical` compares whole
/// files; `resolves` checks path existence on the filesystem.
#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Relation {
    /// Source extracts exactly one value `v`; every target value
    /// must equal `v` (after normalize).
    #[default]
    Equals,
    /// `S ⊆ T` — every source value appears in the target
    /// (singleton `S` = membership).
    Subset,
    /// `S ⊇ T` — every target value appears in the source.
    Superset,
    /// `S == T` — the sets match exactly.
    SetEquals,
    /// Whole-file byte identity (optional `skip_header_lines`).
    Identical,
    /// Each extracted source path must exist on disk (file or dir).
    Resolves,
}

impl Relation {
    fn is_value(self) -> bool {
        matches!(
            self,
            Self::Equals | Self::Subset | Self::Superset | Self::SetEquals
        )
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(rename = "NormalizeTransform")]
enum Normalize {
    #[default]
    None,
    Trim,
    Lower,
    /// Compare only the leading `MAJOR` token (the dotnet/runtime
    /// SDK-band shape: same feature band, not exact patch).
    SemverMajor,
    /// Compare only the leading `MAJOR.MINOR` band — drops patch and
    /// pre-release and takes the leading digits of each token, so
    /// `4.36-dev`, `4.36.0`, `pnpm@11.3.0` (→ `11.3`) and `>=22.13`
    /// all reconcile to one band (the protobuf / pnpm version-format
    /// case the v0.12 study surfaced).
    SemverMinor,
}

impl Normalize {
    fn apply(self, v: &str) -> String {
        match self {
            Self::None => v.to_string(),
            Self::Trim => v.trim().to_string(),
            Self::Lower => v.trim().to_lowercase(),
            // Leading `.`-token, leading non-digits stripped. The trailing
            // `trim_end` keeps normalisation idempotent: `trim()` runs before
            // the split, so the pre-`.` token can still carry trailing
            // whitespace (e.g. `"0 ."` -> token `"0 "`) that a second pass
            // would strip — found by `normalize_transforms_are_idempotent`.
            // (Trailing non-whitespace like `-dev` is intentionally kept, so
            // released behaviour is unchanged for real version strings.)
            Self::SemverMajor => v
                .trim()
                .split('.')
                .next()
                .unwrap_or("")
                .trim_start_matches(|c: char| !c.is_ascii_digit())
                .trim_end()
                .to_string(),
            Self::SemverMinor => semver_minor(v),
        }
    }
}

/// Leading digits of a semver token after stripping a non-digit
/// prefix (`pnpm@11` → `11`, `>=22` → `22`, `36-dev` → `36`).
fn token_digits(tok: &str) -> String {
    tok.trim_start_matches(|c: char| !c.is_ascii_digit())
        .chars()
        .take_while(char::is_ascii_digit)
        .collect()
}

/// The `MAJOR.MINOR` band of a version string.
fn semver_minor(v: &str) -> String {
    let mut it = v.trim().split('.');
    let major = token_digits(it.next().unwrap_or(""));
    if major.is_empty() {
        return String::new();
    }
    match it.next().map(token_digits).filter(|m| !m.is_empty()) {
        Some(minor) => format!("{major}.{minor}"),
        None => major,
    }
}

/// `normalize:` accepts a single transform (`trim`) or an ordered
/// list (`[trim, semver-minor]`), applied left-to-right. A scalar
/// vs a sequence are structurally distinct, so an untagged enum
/// decodes them unambiguously.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(
    untagged,
    expecting = "a normalize transform (`none`, `trim`, `lower`, `semver-major`, or `semver-minor`), or a list of them"
)]
enum NormalizeSpec {
    One(Normalize),
    Many(Vec<Normalize>),
}

impl Default for NormalizeSpec {
    fn default() -> Self {
        Self::One(Normalize::None)
    }
}

impl NormalizeSpec {
    /// The ordered transform list, with `None` (a no-op marker)
    /// dropped — so an empty list means "no normalization".
    fn into_list(self) -> Vec<Normalize> {
        let raw = match self {
            Self::One(n) => vec![n],
            Self::Many(v) => v,
        };
        raw.into_iter().filter(|n| *n != Normalize::None).collect()
    }
}

/// Apply an ordered list of transforms to a value (left-to-right).
fn apply_normalize(transforms: &[Normalize], v: &str) -> String {
    transforms
        .iter()
        .fold(v.to_string(), |acc, t| t.apply(&acc))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// The file whose extracted value(s) form the reference side of the
    /// relation: a single `{ file, extract }`, or (set relations only)
    /// `{ files: <glob>, extract }` whose matches are unioned into one set.
    source: SourceSpec,
    /// The file(s) compared against the source, one relation check per target.
    /// Absent for `resolves` (the target is the filesystem).
    #[serde(default)]
    targets: Option<TargetsSpec>,
    /// The assertion checked between the source and each target: `equals`
    /// (default), `subset`, `superset`, `set_equals`, `identical` (whole file
    /// byte-for-byte), or `resolves` (each path the source extracts exists on
    /// disk).
    #[serde(default)]
    relation: Relation,
    /// A normalize transform, or an ordered list of transforms applied
    /// left-to-right (`[trim, semver-minor]`). `semver-major` / `semver-minor`
    /// keep only the leading MAJOR / MAJOR.MINOR band.
    #[serde(default)]
    normalize: NormalizeSpec,
    /// When true, an absent target file or a missing extracted target value is
    /// tolerated instead of reported as drift.
    #[serde(default)]
    allow_missing_target: bool,
    /// For the `identical` relation only: drop this many leading lines from
    /// both files before comparison, to ignore a differing license or
    /// generated header.
    #[serde(default)]
    #[schemars(range(min = 0))]
    skip_header_lines: Option<usize>,
}

crate::options_schema_for!(Options);

/// Resolved target shape. `extract` is `None` for `identical`
/// (whole-file), `Some` for the value relations.
#[derive(Debug)]
enum Targets {
    Glob {
        scope: Scope,
        extract: Option<Extract>,
    },
    List(Vec<(String, Option<Extract>)>),
}

/// Per-target callback for `each_target`: receives the target's
/// path, its raw literal values, and the violation sink.
type TargetFn<'a> = dyn FnMut(&Path, &[String], &mut Vec<Violation>) + 'a;

#[derive(Debug)]
pub struct CrossFileRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    /// The source path (single-file mode) or the glob string (used
    /// for display in messages when `source_glob` is set).
    source_file: String,
    /// `Some` ⇒ glob-union source: `source_file` is the glob, and
    /// `source_values` unions the extracted values across every
    /// matching file. Set-relations only.
    source_glob: Option<Scope>,
    /// `Some` for value relations + `resolves`; `None` for `identical`.
    source_extract: Option<Extract>,
    /// `Some` for value relations + `identical`; `None` for `resolves`.
    targets: Option<Targets>,
    relation: Relation,
    /// Ordered transform list (empty = no normalization).
    normalize: Vec<Normalize>,
    allow_missing: bool,
    skip_header_lines: usize,
}

impl Rule for CrossFileRule {
    alint_core::rule_common_impl!();

    fn requires_full_index(&self) -> bool {
        // Cross-file: the source and every target may live
        // anywhere in the tree; never `--changed`-scoped.
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut out = Vec::new();
        match self.relation {
            Relation::Equals => {
                if let Some(source_values) = self.source_values(ctx, &mut out) {
                    self.check_equals(ctx, &source_values, &mut out);
                }
            }
            Relation::Subset | Relation::Superset | Relation::SetEquals => {
                if let Some(source_values) = self.source_values(ctx, &mut out) {
                    let source_set: BTreeSet<String> = source_values
                        .iter()
                        .map(|v| apply_normalize(&self.normalize, v))
                        .collect();
                    self.check_set(ctx, &source_set, &mut out);
                }
            }
            Relation::Identical => self.check_identical(ctx, &mut out),
            Relation::Resolves => self.check_resolves(ctx, &mut out),
        }
        Ok(out)
    }
}

impl CrossFileRule {
    /// Read + extract the source file's literal values (raw, not
    /// normalised — callers normalise as the relation needs).
    /// `None` (with a violation pushed) when the source can't be
    /// read or parsed. Only the value relations + `resolves` call
    /// this; `build` guarantees `source_extract` is `Some` for them.
    fn source_values(&self, ctx: &Context<'_>, out: &mut Vec<Violation>) -> Option<Vec<String>> {
        let extract = self.source_extract.as_ref()?;
        if let Some(scope) = &self.source_glob {
            // Glob-union source: extract from every matching file and
            // union the values into one set (the set relations only).
            let mut all = Vec::new();
            let mut matched = 0usize;
            for entry in ctx.index.files() {
                if scope.matches(&entry.path, ctx.index) {
                    matched += 1;
                    if let Some(vals) = Self::source_values_from(ctx, &entry.path, extract, out) {
                        all.extend(vals);
                    }
                }
            }
            // A glob that matches nothing is a misconfiguration (a
            // typo'd path) — fire, mirroring the target-glob behaviour,
            // rather than silently passing `subset` / yielding a
            // confusing empty-set `set_equals` diff.
            if matched == 0 {
                if !self.allow_missing {
                    out.push(Self::violation(
                        Path::new(&self.source_file),
                        "`source.files` glob matched no files",
                    ));
                }
                return None;
            }
            return Some(all);
        }
        Self::source_values_from(ctx, Path::new(&self.source_file), extract, out)
    }

    /// Read one source file + extract its literal values — the
    /// per-file body shared by the single-`file` and `files:`
    /// glob-union forms.
    fn source_values_from(
        ctx: &Context<'_>,
        src: &Path,
        extract: &Extract,
        out: &mut Vec<Violation>,
    ) -> Option<Vec<String>> {
        let text = match read_rel(ctx, src) {
            Ok(t) => t,
            Err(crate::io::ReadCapError::TooLarge(n)) => {
                out.push(Self::violation(
                    src,
                    &format!(
                        "source file is too large to analyze ({})",
                        crate::io::over_cap(n)
                    ),
                ));
                return None;
            }
            Err(crate::io::ReadCapError::Io(e)) => {
                out.push(Self::violation(
                    src,
                    &format!("source file is unreadable: {e}"),
                ));
                return None;
            }
        };
        let values = match extract_values(extract, &text) {
            Ok(v) => v,
            Err(e) => {
                out.push(Self::violation(src, &format!("source extract failed: {e}")));
                return None;
            }
        };
        // Whole-file content is compared verbatim — the non-literal
        // skip (for interpolated *paths*) does not apply.
        if matches!(extract, Extract::WholeFile) {
            return Some(values);
        }
        let (skipped, literal): (Vec<String>, Vec<String>) =
            values.into_iter().partition(|v| is_non_literal(v));
        for v in &skipped {
            out.push(Self::note(
                src,
                &format!("skipped non-literal source value {v:?}"),
            ));
        }
        Some(literal)
    }

    /// `relation: equals` — the released `cross_file_value_equals`
    /// behaviour: the source must resolve to exactly one value, and
    /// every target value must equal it after normalize.
    fn check_equals(&self, ctx: &Context<'_>, source_values: &[String], out: &mut Vec<Violation>) {
        let source = match source_values {
            [one] => one.clone(),
            [] => {
                out.push(Self::violation(
                    Path::new(&self.source_file),
                    "canonical value not found (the source query matched no literal value)",
                ));
                return;
            }
            _ => {
                out.push(Self::violation(
                    Path::new(&self.source_file),
                    "source must resolve to exactly one value (the query matched several); \
                     use a set relation (subset/superset/set_equals) for multi-value sources",
                ));
                return;
            }
        };
        let source_norm = apply_normalize(&self.normalize, &source);
        self.each_target(ctx, out, &mut |target, values, out| {
            if values.is_empty() {
                if !self.allow_missing {
                    out.push(Self::violation(
                        target,
                        "no literal value to compare (the target query matched nothing)",
                    ));
                }
                return;
            }
            for value in values {
                if apply_normalize(&self.normalize, value) != source_norm {
                    out.push(self.mismatch(target, &source, value));
                }
            }
        });
    }

    /// The set relations — compare the source set `S` to each
    /// target's extracted (normalised) set `T`.
    fn check_set(
        &self,
        ctx: &Context<'_>,
        source_set: &BTreeSet<String>,
        out: &mut Vec<Violation>,
    ) {
        self.each_target(ctx, out, &mut |target, values, out| {
            let target_set: BTreeSet<String> = values
                .iter()
                .map(|v| apply_normalize(&self.normalize, v))
                .collect();
            if let Some(v) = self.set_violation(target, source_set, &target_set) {
                out.push(v);
            }
        });
    }

    fn set_violation(
        &self,
        target: &Path,
        source: &BTreeSet<String>,
        actual: &BTreeSet<String>,
    ) -> Option<Violation> {
        let missing: BTreeSet<&String> = source.difference(actual).collect();
        let extra: BTreeSet<&String> = actual.difference(source).collect();
        let reason = match self.relation {
            Relation::Subset if !missing.is_empty() => Some(format!(
                "is missing value(s) required by {}: {}",
                self.source_file,
                render(&missing)
            )),
            Relation::Superset if !extra.is_empty() => Some(format!(
                "has value(s) not present in {}: {}",
                self.source_file,
                render(&extra)
            )),
            Relation::SetEquals if !missing.is_empty() || !extra.is_empty() => Some(format!(
                "set differs from {} (missing: {}; extra: {})",
                self.source_file,
                render(&missing),
                render(&extra),
            )),
            _ => None,
        }?;
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| format!("{} {reason}", crate::slash(target)));
        Some(Violation::new(msg).with_path(target.to_path_buf()))
    }

    /// `relation: identical` — every target file's bytes (after
    /// dropping `skip_header_lines` leading lines) must equal the
    /// source file's. Binary-accurate; `normalize` does not apply.
    fn check_identical(&self, ctx: &Context<'_>, out: &mut Vec<Violation>) {
        let Some(src) = confined_rel(ctx, Path::new(&self.source_file)) else {
            out.push(Self::violation(
                Path::new(&self.source_file),
                "source file escapes the repo root",
            ));
            return;
        };
        let src = src.as_path();
        let src_bytes = match crate::io::read_capped(&ctx.root.join(src)) {
            Ok(b) => b,
            Err(e) => {
                out.push(Self::violation(src, &read_cap_reason("source file", &e)));
                return;
            }
        };
        let src_cmp = skip_header(&src_bytes, self.skip_header_lines);

        let paths = self.identical_target_paths(ctx);
        if paths.is_empty() {
            if !self.allow_missing {
                out.push(Self::violation(src, "targets matched no files"));
            }
            return;
        }
        for target in &paths {
            let Some(target) = confined_rel(ctx, target) else {
                out.push(Self::violation(target, "target file escapes the repo root"));
                continue;
            };
            let tgt_bytes = match crate::io::read_capped(&ctx.root.join(&target)) {
                Ok(b) => b,
                Err(crate::io::ReadCapError::TooLarge(n)) => {
                    out.push(Self::violation(
                        &target,
                        &format!(
                            "target file is too large to analyze ({})",
                            crate::io::over_cap(n)
                        ),
                    ));
                    continue;
                }
                Err(crate::io::ReadCapError::Io(_)) => {
                    if !self.allow_missing {
                        out.push(Self::violation(
                            &target,
                            "target file is missing or unreadable",
                        ));
                    }
                    continue;
                }
            };
            if skip_header(&tgt_bytes, self.skip_header_lines) != src_cmp {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "{} is not byte-identical to {}",
                        crate::slash(&target),
                        self.source_file,
                    )
                });
                out.push(Violation::new(msg).with_path(target.clone()));
            }
        }
    }

    /// The target paths for `identical` (glob expansion or the
    /// explicit list), ignoring `extract` (which is absent).
    fn identical_target_paths(&self, ctx: &Context<'_>) -> Vec<PathBuf> {
        match &self.targets {
            Some(Targets::Glob { scope, .. }) => ctx
                .index
                .files()
                .filter(|e| scope.matches(&e.path, ctx.index))
                .map(|e| e.path.to_path_buf())
                .collect(),
            Some(Targets::List(list)) => list.iter().map(|(f, _)| PathBuf::from(f)).collect(),
            None => Vec::new(),
        }
    }

    /// `relation: resolves` — each path the source extracts must
    /// exist on disk (file or dir), resolved relative to the source
    /// file's directory. The 1-level forward half of
    /// `registry_paths_resolve`.
    fn check_resolves(&self, ctx: &Context<'_>, out: &mut Vec<Violation>) {
        let Some(paths) = self.source_values(ctx, out) else {
            return;
        };
        let src = Path::new(&self.source_file);
        let base = src.parent().map(Path::to_path_buf).unwrap_or_default();
        let dirs: HashSet<&Path> = ctx.index.dirs().map(|e| &*e.path).collect();
        for entry in &paths {
            // A confined-out (absolute / root-escaping) declared path
            // can never resolve to an in-tree file → treated as unresolved.
            let exists = crate::pathsafe::normalize_confined(&base.join(entry))
                .is_some_and(|r| ctx.index.contains_file(&r) || dirs.contains(r.as_path()));
            if !exists {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "{}: declared path {entry:?} does not resolve to a file or directory",
                        crate::slash(src),
                    )
                });
                out.push(
                    // One source can declare many unresolved paths → key on
                    // the source and the specific declared entry.
                    Violation::new(msg)
                        .with_path(src.to_path_buf())
                        .with_baseline_key(format!(
                            "resolves\u{0}{}\u{0}{entry}",
                            crate::slash(src)
                        )),
                );
            }
        }
    }

    /// Iterate the value-relation targets, calling
    /// `f(target_path, raw_literal_values, out)` for each readable
    /// target. Read/extract errors and a zero-match glob are
    /// reported here, so `f` sees only resolvable targets. `build`
    /// guarantees `targets` is `Some` and every `extract` is `Some`
    /// for the value relations that call this.
    fn each_target(&self, ctx: &Context<'_>, out: &mut Vec<Violation>, f: &mut TargetFn<'_>) {
        match &self.targets {
            Some(Targets::Glob {
                scope,
                extract: Some(extract),
            }) => {
                let mut matched = 0usize;
                for e in ctx.index.files() {
                    if !scope.matches(&e.path, ctx.index) {
                        continue;
                    }
                    matched += 1;
                    if let Some(values) = self.target_values(ctx, &e.path, extract, out) {
                        f(&e.path, &values, out);
                    }
                }
                if matched == 0 && !self.allow_missing {
                    out.push(Self::violation(
                        Path::new(&self.source_file),
                        "targets glob matched no files",
                    ));
                }
            }
            Some(Targets::List(list)) => {
                for (file, extract) in list {
                    let Some(extract) = extract else { continue };
                    let target = Path::new(file);
                    if let Some(values) = self.target_values(ctx, target, extract, out) {
                        f(target, &values, out);
                    }
                }
            }
            _ => {}
        }
    }

    /// Read + extract one target's raw literal values. `None`
    /// (with a violation pushed, unless `allow_missing` for the
    /// missing-file case) when the target can't be read or parsed.
    fn target_values(
        &self,
        ctx: &Context<'_>,
        target: &Path,
        extract: &Extract,
        out: &mut Vec<Violation>,
    ) -> Option<Vec<String>> {
        let text = match read_rel(ctx, target) {
            Ok(t) => t,
            Err(crate::io::ReadCapError::TooLarge(n)) => {
                // A too-large target is always a violation — never
                // suppressed by `allow_missing` (it is present,
                // just unanalysable).
                out.push(Self::violation(
                    target,
                    &format!(
                        "target file is too large to analyze ({})",
                        crate::io::over_cap(n)
                    ),
                ));
                return None;
            }
            Err(crate::io::ReadCapError::Io(_)) => {
                if !self.allow_missing {
                    out.push(Self::violation(
                        target,
                        "target file is missing or unreadable",
                    ));
                }
                return None;
            }
        };
        let values = match extract_values(extract, &text) {
            Ok(v) => v,
            Err(e) => {
                out.push(Self::violation(
                    target,
                    &format!("target extract failed: {e}"),
                ));
                return None;
            }
        };
        // Whole-file content is compared verbatim — the non-literal
        // skip (for interpolated *paths*) does not apply.
        if matches!(extract, Extract::WholeFile) {
            return Some(values);
        }
        let (skipped, literal): (Vec<String>, Vec<String>) =
            values.into_iter().partition(|v| is_non_literal(v));
        for v in &skipped {
            out.push(Self::note(
                target,
                &format!("skipped non-literal target value {v:?}"),
            ));
        }
        Some(literal)
    }

    fn violation(path: &Path, reason: &str) -> Violation {
        Violation::new(format!("{}: {reason}", crate::slash(path))).with_path(path.to_path_buf())
    }

    /// An informational note (non-violation finding) — e.g. a
    /// non-literal value the rule skipped rather than compared.
    fn note(path: &Path, reason: &str) -> Violation {
        Self::violation(path, reason).as_note()
    }

    fn mismatch(&self, target: &Path, source: &str, target_value: &str) -> Violation {
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "{} value {target_value:?} != {} value {source:?}",
                crate::slash(target),
                self.source_file,
            )
        });
        Violation::new(msg)
            .with_path(target.to_path_buf())
            // One query can match several target values → key on the target
            // and the specific failing value so they don't collapse to one
            // fingerprint (which would mask a genuinely new mismatch).
            .with_baseline_key(format!(
                "equals\u{0}{}\u{0}{target_value}",
                crate::slash(target)
            ))
    }
}

/// Render a sorted value set for a violation message.
fn render(set: &BTreeSet<&String>) -> String {
    if set.is_empty() {
        // `set_equals` renders both sides; an empty one reads `none`
        // rather than a dangling `(missing: ; extra: "x")`. The
        // subset/superset paths only call this on a non-empty side.
        return "none".to_string();
    }
    set.iter()
        .map(|v| format!("{v:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Drop the first `n` newline-delimited lines of `bytes` (for
/// `identical`'s `skip_header_lines`). Fewer than `n` lines ⇒ the
/// whole file is header, so the comparison is over the empty slice.
fn skip_header(bytes: &[u8], n: usize) -> &[u8] {
    if n == 0 {
        return bytes;
    }
    let mut seen = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            seen += 1;
            if seen == n {
                return &bytes[i + 1..];
            }
        }
    }
    &[]
}

/// The user-facing reason fragment for a capped-read failure.
fn read_cap_reason(what: &str, e: &crate::io::ReadCapError) -> String {
    match e {
        crate::io::ReadCapError::TooLarge(n) => {
            format!(
                "{what} is too large to analyze ({})",
                crate::io::over_cap(*n)
            )
        }
        crate::io::ReadCapError::Io(e) => format!("{what} is unreadable: {e}"),
    }
}

/// Lexically confine `rel`, then verify it doesn't escape the root through
/// an in-repo symlink once joined — `normalize_confined` is symlink-blind,
/// so `link/secret` (with `link -> /etc`) would otherwise read out of the
/// tree. `None` on either escape; the caller reports "escapes the repo
/// root". Used by every cross-file *read* path.
fn confined_rel(ctx: &Context<'_>, rel: &Path) -> Option<PathBuf> {
    let p = crate::pathsafe::normalize_confined(rel)?;
    crate::pathsafe::resolved_within_root(&ctx.root.join(&p), ctx.root).then_some(p)
}

/// Read a tree-relative path as text (the index stores paths, not
/// contents, so the cross-file rules read the file themselves).
fn read_rel(ctx: &Context<'_>, rel: &Path) -> Result<String, crate::io::ReadCapError> {
    // Confine to the repo root before any read — an absolute or
    // root-escaping (lexically or via symlink) `source.file` /
    // `targets[].file` must never read a file outside the tree.
    let Some(rel) = confined_rel(ctx, rel) else {
        return Err(crate::io::ReadCapError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path escapes the repo root",
        )));
    };
    crate::io::read_capped(&ctx.root.join(rel)).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Resolve a `targets:` spec, validating each glob / list entry.
/// `extract` stays `Option` (absent for `identical`); the
/// relation-shape coupling is checked in `validate_shape`.
fn resolve_targets(ts: TargetsSpec, cfg: &impl Fn(String) -> Error) -> Result<Targets> {
    match ts {
        TargetsSpec::Glob { files, extract } => {
            if files.trim().is_empty() {
                return Err(cfg("`targets.files` must not be empty".into()));
            }
            let scope = Scope::from_patterns(std::slice::from_ref(&files))
                .map_err(|e| cfg(format!("invalid `targets.files` glob: {e}")))?;
            let extract = match extract {
                Some(e) => Some(
                    e.resolve()
                        .map_err(|e| cfg(format!("invalid `targets.extract`: {e}")))?,
                ),
                None => None,
            };
            Ok(Targets::Glob { scope, extract })
        }
        TargetsSpec::List(list) => {
            if list.is_empty() {
                return Err(cfg("`targets` list must not be empty".into()));
            }
            let mut resolved = Vec::with_capacity(list.len());
            for (i, t) in list.into_iter().enumerate() {
                if t.file.trim().is_empty() {
                    return Err(cfg(format!("`targets[{i}].file` must not be empty")));
                }
                let ex = match t.extract {
                    Some(e) => Some(
                        e.resolve()
                            .map_err(|e| cfg(format!("invalid `targets[{i}].extract`: {e}")))?,
                    ),
                    None => None,
                };
                resolved.push((t.file, ex));
            }
            Ok(Targets::List(resolved))
        }
    }
}

/// Enforce the per-relation shape: value relations need
/// `source.extract` + `targets` (with `extract`); `identical` needs
/// `targets` without `extract` and no `source.extract`; `resolves`
/// needs `source.extract` and no `targets`.
fn validate_shape(
    relation: Relation,
    source_extract: Option<&Extract>,
    targets: Option<&Targets>,
    cfg: &impl Fn(String) -> Error,
) -> Result<()> {
    let (any_target_extract, all_target_extract) = match targets {
        Some(Targets::Glob { extract, .. }) => (extract.is_some(), extract.is_some()),
        Some(Targets::List(list)) => (
            list.iter().any(|(_, e)| e.is_some()),
            list.iter().all(|(_, e)| e.is_some()),
        ),
        None => (false, false),
    };
    if relation.is_value() {
        if source_extract.is_none() {
            return Err(cfg(format!(
                "`relation: {relation:?}` (a value relation) needs `source.extract`"
            )));
        }
        if targets.is_none() {
            return Err(cfg("a value relation needs `targets`".into()));
        }
        if !all_target_extract {
            return Err(cfg("a value relation's `targets` need `extract`".into()));
        }
    } else if relation == Relation::Identical {
        if source_extract.is_some() {
            return Err(cfg(
                "`relation: identical` compares whole files; remove `source.extract`".into(),
            ));
        }
        if targets.is_none() {
            return Err(cfg("`relation: identical` needs `targets`".into()));
        }
        if any_target_extract {
            return Err(cfg(
                "`relation: identical` compares whole files; remove `targets.extract`".into(),
            ));
        }
    } else {
        // resolves
        if source_extract.is_none() {
            return Err(cfg(
                "`relation: resolves` needs `source.extract` (the paths to check)".into(),
            ));
        }
        if targets.is_some() {
            return Err(cfg(
                "`relation: resolves` checks the filesystem; remove `targets`".into(),
            ));
        }
    }
    Ok(())
}

/// Reject a malformed regex extract at build time (like `file_graph` /
/// `registry_paths_resolve`), so a bad pattern is a clean config error
/// rather than an error-level eval-time violation.
fn validate_extract_regexes(
    source_extract: Option<&Extract>,
    targets: Option<&Targets>,
    cfg: &impl Fn(String) -> Error,
) -> Result<()> {
    let check = |e: Option<&Extract>| -> Result<()> {
        if let Some(Extract::Regex(p)) = e {
            regex::Regex::new(p).map_err(|err| cfg(format!("invalid `extract.regex`: {err}")))?;
        }
        Ok(())
    };
    check(source_extract)?;
    if let Some(t) = targets {
        match t {
            Targets::Glob { extract, .. } => check(extract.as_ref())?,
            Targets::List(list) => {
                for (_, e) in list {
                    check(e.as_ref())?;
                }
            }
        }
    }
    Ok(())
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    alint_core::reject_scope_filter_on_cross_file(spec, "cross_file")?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let cfg = |msg: String| Error::rule_config(&spec.id, msg);

    let source_extract = match opts.source.extract {
        Some(spec) => Some(
            spec.resolve()
                .map_err(|e| cfg(format!("invalid `source.extract`: {e}")))?,
        ),
        None => None,
    };
    // Source: exactly one of `file` (single) or `files` (glob-union,
    // set relations only). For the glob form `source_file` holds the
    // glob string so violation messages still name a source.
    let (source_file, source_glob) = match (opts.source.file, opts.source.files) {
        (Some(f), None) => {
            if f.trim().is_empty() {
                return Err(cfg("`source.file` must not be empty".into()));
            }
            (f, None)
        }
        (None, Some(g)) => {
            if g.trim().is_empty() {
                return Err(cfg("`source.files` must not be empty".into()));
            }
            if !matches!(
                opts.relation,
                Relation::Subset | Relation::Superset | Relation::SetEquals
            ) {
                return Err(cfg(format!(
                    "`source.files` (glob-union) requires a set relation \
                     (subset / superset / set_equals), not `{:?}`",
                    opts.relation
                )));
            }
            let scope = Scope::from_patterns(std::slice::from_ref(&g))
                .map_err(|e| cfg(format!("invalid `source.files` glob: {e}")))?;
            (g, Some(scope))
        }
        (Some(_), Some(_)) => {
            return Err(cfg(
                "set exactly one of `source.file` (a path) or `source.files` (a glob), not both"
                    .into(),
            ));
        }
        (None, None) => {
            return Err(cfg(
                "`source` needs `file` (a path) or `files` (a glob-union over set relations)"
                    .into(),
            ));
        }
    };
    // Glob-union + `whole_file` would union whole-file CONTENTS as set
    // members — semantically odd and never useful; reject it so the
    // misconfiguration fails loudly at build time.
    if source_glob.is_some() && matches!(source_extract, Some(Extract::WholeFile)) {
        return Err(cfg(
            "`source.files` (glob-union) cannot use a `whole_file` extract \
             (it would union file contents as set members); use a structured / \
             regex / lines extract"
                .into(),
        ));
    }
    let targets = match opts.targets {
        Some(ts) => Some(resolve_targets(ts, &cfg)?),
        None => None,
    };

    // Pre-validate regex extracts at build time (clean config error,
    // not an error-level eval-time violation).
    validate_extract_regexes(source_extract.as_ref(), targets.as_ref(), &cfg)?;

    validate_shape(
        opts.relation,
        source_extract.as_ref(),
        targets.as_ref(),
        &cfg,
    )?;

    // Reject options the chosen relation ignores, so a misconfiguration
    // fails loudly at build rather than silently doing nothing.
    if opts.skip_header_lines.is_some() && opts.relation != Relation::Identical {
        return Err(cfg(
            "`skip_header_lines` only applies to `relation: identical`".into(),
        ));
    }
    let normalize = opts.normalize.into_list();
    if !normalize.is_empty() && matches!(opts.relation, Relation::Identical | Relation::Resolves) {
        return Err(cfg(format!(
            "`normalize` does not apply to `relation: {:?}` \
             (it compares whole files / paths, not extracted values)",
            opts.relation
        )));
    }

    Ok(Box::new(CrossFileRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        source_file,
        source_glob,
        source_extract,
        targets,
        relation: opts.relation,
        normalize,
        allow_missing: opts.allow_missing_target,
        skip_header_lines: opts.skip_header_lines.unwrap_or(0),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{FileEntry, FileIndex, Format};
    use proptest::prelude::*;

    #[test]
    fn untagged_enums_name_accepted_forms_not_internal_enum() {
        // A value matching no variant reports the accepted forms (via
        // `#[serde(expecting)]`), not the internal untagged-enum name.
        let n = serde_yaml_ng::from_str::<NormalizeSpec>("42")
            .unwrap_err()
            .to_string();
        assert!(
            n.contains("a normalize transform") && !n.contains("NormalizeSpec"),
            "NormalizeSpec: {n}"
        );
        let t = serde_yaml_ng::from_str::<TargetsSpec>("42")
            .unwrap_err()
            .to_string();
        assert!(
            t.contains("{ files, extract }") && !t.contains("TargetsSpec"),
            "TargetsSpec: {t}"
        );
    }

    proptest! {
        /// Every `normalize` transform is idempotent: normalising an
        /// already-normalised value is a no-op. A non-idempotent
        /// transform would make `equals` comparisons depend on how many
        /// times normalisation ran — a latent correctness bug. (This is
        /// the behaviour spec; proptest is the partial proof.)
        #[test]
        fn normalize_transforms_are_idempotent(s in r"\PC{0,48}") {
            for t in [
                Normalize::Trim,
                Normalize::Lower,
                Normalize::SemverMajor,
                Normalize::SemverMinor,
            ] {
                let once = t.apply(&s);
                let twice = t.apply(&once);
                prop_assert_eq!(&twice, &once, "{:?} is not idempotent on {:?}", t, s);
            }
        }

        /// `apply_normalize` with a single transform equals that
        /// transform applied directly — the fold has no off-by-one.
        #[test]
        fn apply_normalize_single_equals_transform(s in r"\PC{0,48}") {
            let t = Normalize::SemverMinor;
            prop_assert_eq!(apply_normalize(&[t], &s), t.apply(&s));
        }

        /// `semver_minor` always yields a clean band: empty, or digits
        /// optionally followed by `.` and more digits (`MAJOR` or
        /// `MAJOR.MINOR`). No stray separators or non-digits survive.
        #[test]
        fn semver_minor_yields_a_clean_band(s in r"\PC{0,48}") {
            let band = semver_minor(&s);
            if !band.is_empty() {
                let mut parts = band.split('.');
                let major = parts.next().unwrap();
                prop_assert!(!major.is_empty() && major.bytes().all(|b| b.is_ascii_digit()));
                if let Some(minor) = parts.next() {
                    prop_assert!(!minor.is_empty() && minor.bytes().all(|b| b.is_ascii_digit()));
                }
                prop_assert!(parts.next().is_none(), "at most MAJOR.MINOR");
            }
        }
    }

    fn index(files: &[&str]) -> FileIndex {
        FileIndex::from_entries(
            files
                .iter()
                .map(|p| FileEntry {
                    path: Path::new(p).into(),
                    is_dir: false,
                    size: 1,
                })
                .collect(),
        )
    }

    fn value_rule(
        source_file: &str,
        source: Extract,
        targets: Targets,
        relation: Relation,
        normalize: Normalize,
    ) -> CrossFileRule {
        CrossFileRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            source_file: source_file.into(),
            source_glob: None,
            source_extract: Some(source),
            targets: Some(targets),
            relation,
            normalize: NormalizeSpec::One(normalize).into_list(),
            allow_missing: false,
            skip_header_lines: 0,
        }
    }

    fn eval(r: &CrossFileRule, root: &Path, idx: &FileIndex) -> Vec<Violation> {
        let ctx = Context {
            root,
            index: idx,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
        };
        r.evaluate(&ctx).unwrap()
    }

    // ─── equals (the migrated cross_file_value_equals path) ──────

    #[test]
    fn equals_glob_targets_pass_and_fail_on_version_lockstep() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace.package]\nversion = \"1.4.0\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("crates/a")).unwrap();
        std::fs::create_dir_all(root.join("crates/b")).unwrap();
        std::fs::write(
            root.join("crates/a/Cargo.toml"),
            "[package]\nversion = \"1.4.0\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("crates/b/Cargo.toml"),
            "[package]\nversion = \"1.3.0\"\n",
        )
        .unwrap();
        let idx = index(&["Cargo.toml", "crates/a/Cargo.toml", "crates/b/Cargo.toml"]);
        let r = value_rule(
            "Cargo.toml",
            Extract::Structured(Format::Toml, "$.workspace.package.version".into()),
            Targets::Glob {
                scope: Scope::from_patterns(&["crates/*/Cargo.toml".to_string()]).unwrap(),
                extract: Some(Extract::Structured(
                    Format::Toml,
                    "$.package.version".into(),
                )),
            },
            Relation::Equals,
            Normalize::None,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "only crates/b drifts: {v:?}");
        assert!(v[0].message.contains("crates/b/Cargo.toml"));
        assert!(v[0].message.contains("1.3.0"));
    }

    #[test]
    fn equals_target_query_matching_nothing_fires_unless_allow_missing() {
        // A target file that resolves no value (the query names an
        // absent key) is a drift by default, but `allow_missing_target`
        // makes it a tolerated absence.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("source.toml"), "v = \"1.0\"\n").unwrap();
        std::fs::write(root.join("target.toml"), "other = \"x\"\n").unwrap();
        let idx = index(&["source.toml", "target.toml"]);
        let make = || {
            value_rule(
                "source.toml",
                Extract::Structured(Format::Toml, "$.v".into()),
                Targets::Glob {
                    scope: Scope::from_patterns(&["target.toml".to_string()]).unwrap(),
                    extract: Some(Extract::Structured(Format::Toml, "$.missing".into())),
                },
                Relation::Equals,
                Normalize::None,
            )
        };
        // Default (`allow_missing_target: false`): the empty target
        // query fires.
        let strict = make();
        let v = eval(&strict, root, &idx);
        assert_eq!(v.len(), 1, "missing target value fires: {v:?}");
        assert!(v[0].message.contains("matched nothing"), "{}", v[0].message);
        // `allow_missing_target: true`: the absence is tolerated.
        let mut lax = make();
        lax.allow_missing = true;
        assert!(
            eval(&lax, root, &idx).is_empty(),
            "allow_missing silences the empty target"
        );
    }

    #[test]
    fn whole_file_equals_compares_verbatim_despite_interpolation_markers() {
        // The body carries `${...}`/`{{...}}` — markers the non-literal
        // skip would normally drop. whole_file must compare verbatim.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let body = "Copyright ${YEAR} Acme\nAll rights {{ reserved }}.\n";
        std::fs::write(root.join("LICENSE"), body).unwrap();
        std::fs::write(root.join("LICENSE-MIT"), body).unwrap();
        std::fs::write(root.join("LICENSE-APACHE"), "different text\n").unwrap();
        let idx = index(&["LICENSE", "LICENSE-MIT", "LICENSE-APACHE"]);
        let r = value_rule(
            "LICENSE",
            Extract::WholeFile,
            Targets::Glob {
                scope: Scope::from_patterns(&["LICENSE-*".to_string()]).unwrap(),
                extract: Some(Extract::WholeFile),
            },
            Relation::Equals,
            Normalize::None,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "only LICENSE-APACHE drifts: {v:?}");
        assert!(v[0].message.contains("LICENSE-APACHE"));
    }

    #[test]
    fn equals_multi_value_source_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("m.json"), "{\"v\":[\"1\",\"2\"]}").unwrap();
        let idx = index(&["m.json"]);
        let r = value_rule(
            "m.json",
            Extract::Structured(Format::Json, "$.v[*]".into()),
            Targets::List(vec![(
                "m.json".into(),
                Some(Extract::Structured(Format::Json, "$.v[0]".into())),
            )]),
            Relation::Equals,
            Normalize::None,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("exactly one value"));
    }

    #[test]
    fn semver_major_is_idempotent_on_trailing_space_token() {
        // Regression (found by normalize_transforms_are_idempotent): `trim()`
        // runs before `split('.')`, so the pre-`.` token can carry trailing
        // whitespace (`"0 ."` -> token `"0 "`). Without the trailing `trim_end`
        // the first pass returned `"0 "` and a second pass `"0"` — not stable.
        assert_eq!(Normalize::SemverMajor.apply("0 ."), "0");
        let once = Normalize::SemverMajor.apply("0 .");
        assert_eq!(Normalize::SemverMajor.apply(&once), once);
    }

    #[test]
    fn equals_semver_major_normalize_allows_band() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("global.json"),
            "{\"sdk\":{\"version\":\"8.0.402\"}}",
        )
        .unwrap();
        std::fs::write(root.join("Directory.Build.props"), "8.0.100\n").unwrap();
        let idx = index(&["global.json", "Directory.Build.props"]);
        let r = value_rule(
            "global.json",
            Extract::Structured(Format::Json, "$.sdk.version".into()),
            Targets::List(vec![(
                "Directory.Build.props".into(),
                Some(Extract::Lines(alint_core::LinesOpts::default())),
            )]),
            Relation::Equals,
            Normalize::SemverMajor,
        );
        assert!(eval(&r, root, &idx).is_empty());
    }

    #[test]
    fn semver_minor_reconciles_version_formats() {
        // The protobuf / pnpm version-format cases all collapse to
        // one MAJOR.MINOR band.
        assert_eq!(semver_minor("4.36-dev"), "4.36");
        assert_eq!(semver_minor("4.36.0"), "4.36");
        assert_eq!(semver_minor("pnpm@11.3.0"), "11.3");
        assert_eq!(semver_minor(">=22.13"), "22.13");
        assert_eq!(semver_minor("4"), "4");
        assert_eq!(semver_minor(""), "");
        // SemverMajor keeps its unchanged released behaviour.
        assert_eq!(Normalize::SemverMajor.apply("8.0.402"), "8");
    }

    #[test]
    fn normalize_list_applies_in_order_and_filters_none() {
        assert_eq!(
            apply_normalize(&[Normalize::Trim, Normalize::Lower], "  ABC  "),
            "abc"
        );
        // An empty list is the identity.
        assert_eq!(apply_normalize(&[], "  ABC  "), "  ABC  ");
        // `none` is dropped from the resolved list.
        assert!(NormalizeSpec::One(Normalize::None).into_list().is_empty());
        assert_eq!(
            NormalizeSpec::Many(vec![Normalize::None, Normalize::Trim]).into_list(),
            vec![Normalize::Trim]
        );
    }

    #[test]
    fn equals_semver_minor_reconciles_dev_and_patch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // protobuf: `4.36-dev` (version.json) vs `4.36.0` (the .bzl).
        std::fs::write(root.join("version.json"), "{\"v\":\"4.36-dev\"}").unwrap();
        std::fs::write(root.join("protobuf_version.bzl"), "4.36.0\n").unwrap();
        let idx = index(&["version.json", "protobuf_version.bzl"]);
        let r = value_rule(
            "version.json",
            Extract::Structured(Format::Json, "$.v".into()),
            Targets::List(vec![(
                "protobuf_version.bzl".into(),
                Some(Extract::Lines(alint_core::LinesOpts::default())),
            )]),
            Relation::Equals,
            Normalize::SemverMinor,
        );
        assert!(
            eval(&r, root, &idx).is_empty(),
            "{:?}",
            eval(&r, root, &idx)
        );
    }

    #[test]
    fn build_accepts_scalar_and_list_normalize() {
        use crate::test_support::spec_yaml;
        let base = "id: t\nkind: cross_file\nsource:\n  file: a\n  extract:\n    lines: {}\n\
                    targets:\n  files: \"b/*\"\n  extract:\n    lines: {}\nrelation: equals\n";
        // Scalar (the released form) and a list both build.
        assert!(
            build(&spec_yaml(&format!(
                "{base}normalize: semver-minor\nlevel: error\n"
            )))
            .is_ok()
        );
        assert!(
            build(&spec_yaml(&format!(
                "{base}normalize: [trim, semver-minor]\nlevel: error\n"
            )))
            .is_ok()
        );
    }

    // ─── set relations ──────────────────────────────────────────

    fn set_rule(source: Extract, targets: Targets, relation: Relation) -> CrossFileRule {
        value_rule("src.json", source, targets, relation, Normalize::None)
    }

    fn write_sets(root: &Path, source: &str, target: &str) {
        std::fs::write(root.join("src.json"), source).unwrap();
        std::fs::write(root.join("tgt.json"), target).unwrap();
    }

    fn set_targets() -> Targets {
        Targets::List(vec![(
            "tgt.json".into(),
            Some(Extract::Structured(Format::Json, "$.have[*]".into())),
        )])
    }

    #[test]
    fn subset_fires_when_a_source_value_is_missing_from_target() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // S = {a, b}; T = {a} -> b is missing.
        write_sets(root, "{\"need\":[\"a\",\"b\"]}", "{\"have\":[\"a\",\"c\"]}");
        let idx = index(&["src.json", "tgt.json"]);
        let r = set_rule(
            Extract::Structured(Format::Json, "$.need[*]".into()),
            set_targets(),
            Relation::Subset,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("missing"));
        assert!(v[0].message.contains("\"b\""));
        // `c` is extra in the target but `subset` does not care.
        assert!(!v[0].message.contains("\"c\""));
    }

    #[test]
    fn subset_silent_when_source_is_contained() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_sets(
            root,
            "{\"need\":[\"a\",\"b\"]}",
            "{\"have\":[\"a\",\"b\",\"c\"]}",
        );
        let idx = index(&["src.json", "tgt.json"]);
        let r = set_rule(
            Extract::Structured(Format::Json, "$.need[*]".into()),
            set_targets(),
            Relation::Subset,
        );
        assert!(eval(&r, root, &idx).is_empty());
    }

    #[test]
    fn superset_fires_on_a_target_value_not_in_source() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // S = {a, b}; T = {a, z} -> z is not covered by the source.
        write_sets(root, "{\"need\":[\"a\",\"b\"]}", "{\"have\":[\"a\",\"z\"]}");
        let idx = index(&["src.json", "tgt.json"]);
        let r = set_rule(
            Extract::Structured(Format::Json, "$.need[*]".into()),
            set_targets(),
            Relation::Superset,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("not present"));
        assert!(v[0].message.contains("\"z\""));
    }

    #[test]
    fn set_equals_reports_both_missing_and_extra() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // S = {a, b}; T = {a, z} -> missing b, extra z.
        write_sets(root, "{\"need\":[\"a\",\"b\"]}", "{\"have\":[\"a\",\"z\"]}");
        let idx = index(&["src.json", "tgt.json"]);
        let r = set_rule(
            Extract::Structured(Format::Json, "$.need[*]".into()),
            set_targets(),
            Relation::SetEquals,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("missing"));
        assert!(v[0].message.contains("\"b\""));
        assert!(v[0].message.contains("extra"));
        assert!(v[0].message.contains("\"z\""));
    }

    #[test]
    fn set_equals_silent_on_matching_sets_regardless_of_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_sets(root, "{\"need\":[\"b\",\"a\"]}", "{\"have\":[\"a\",\"b\"]}");
        let idx = index(&["src.json", "tgt.json"]);
        let r = set_rule(
            Extract::Structured(Format::Json, "$.need[*]".into()),
            set_targets(),
            Relation::SetEquals,
        );
        assert!(eval(&r, root, &idx).is_empty());
    }

    #[test]
    fn glob_union_source_set_equals_unions_across_the_glob() {
        // The vim hlgroups shape: the union of `*hl-X*` across every
        // doc file must equal the `default link X` set in one file.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("doc")).unwrap();
        std::fs::write(root.join("doc/a.txt"), "see *hl-Comment* and *hl-String*\n").unwrap();
        std::fs::write(root.join("doc/b.txt"), "also *hl-Number*\n").unwrap();
        std::fs::write(
            root.join("highlight.c"),
            "default link Comment\ndefault link String\ndefault link Number\n",
        )
        .unwrap();
        let idx = index(&["doc/a.txt", "doc/b.txt", "highlight.c"]);
        let r = CrossFileRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            source_file: "doc/*.txt".into(),
            source_glob: Some(Scope::from_patterns(&["doc/*.txt".to_string()]).unwrap()),
            source_extract: Some(Extract::Regex(r"\*hl-(\w+)\*".into())),
            targets: Some(Targets::List(vec![(
                "highlight.c".into(),
                Some(Extract::Regex(r"default link (\w+)".into())),
            )])),
            relation: Relation::SetEquals,
            normalize: vec![],
            allow_missing: false,
            skip_header_lines: 0,
        };
        // union {Comment, String, Number} == highlight.c set → silent.
        assert!(eval(&r, root, &idx).is_empty(), "matched union should pass");
        // A doc declares an extra group the code lacks → set_equals fires.
        std::fs::write(root.join("doc/b.txt"), "also *hl-Number* and *hl-Extra*\n").unwrap();
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("Extra"), "{}", v[0].message);
    }

    #[test]
    fn glob_union_source_matching_no_files_fires() {
        // C2: a `source.files` glob that matches nothing is a
        // misconfiguration — it fires, not a silent `subset` pass.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("highlight.c"), "default link Comment\n").unwrap();
        let idx = index(&["highlight.c"]);
        let r = CrossFileRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            source_file: "doc/*.txt".into(),
            source_glob: Some(Scope::from_patterns(&["doc/*.txt".to_string()]).unwrap()),
            source_extract: Some(Extract::Regex(r"\*hl-(\w+)\*".into())),
            targets: Some(Targets::List(vec![(
                "highlight.c".into(),
                Some(Extract::Regex(r"default link (\w+)".into())),
            )])),
            relation: Relation::Subset,
            normalize: vec![],
            allow_missing: false,
            skip_header_lines: 0,
        };
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].message.contains("matched no files"),
            "{}",
            v[0].message
        );
    }

    #[test]
    fn build_glob_source_requires_a_set_relation() {
        let yaml = "id: t\nkind: cross_file\n\
                    source: { files: \"doc/*.txt\", extract: { regex: 'x(.)' } }\n\
                    targets: [{ file: c, extract: { regex: 'y(.)' } }]\n\
                    relation: equals\nlevel: error\n";
        let err = build(&crate::test_support::spec_yaml(yaml))
            .unwrap_err()
            .to_string();
        assert!(err.contains("set relation"), "{err}");
    }

    #[test]
    fn build_glob_source_rejects_whole_file_extract() {
        // Unioning whole-file CONTENTS as set members is never useful.
        let yaml = "id: t\nkind: cross_file\n\
                    source: { files: \"doc/*.txt\", extract: { whole_file: {} } }\n\
                    targets: [{ file: c, extract: { whole_file: {} } }]\n\
                    relation: set_equals\nlevel: error\n";
        let err = build(&crate::test_support::spec_yaml(yaml))
            .unwrap_err()
            .to_string();
        assert!(err.contains("whole_file"), "{err}");
    }

    #[test]
    fn build_rejects_both_source_file_and_files() {
        let yaml = "id: t\nkind: cross_file\n\
                    source: { file: a, files: \"b/*\", extract: { regex: 'x(.)' } }\n\
                    targets: [{ file: c, extract: { regex: 'y(.)' } }]\n\
                    relation: set_equals\nlevel: error\n";
        let err = build(&crate::test_support::spec_yaml(yaml))
            .unwrap_err()
            .to_string();
        assert!(err.contains("exactly one"), "{err}");
    }

    #[test]
    fn subset_singleton_is_a_membership_check() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // S = {needle}; member -> silent.
        write_sets(
            root,
            "{\"need\":[\"needle\"]}",
            "{\"have\":[\"hay\",\"needle\",\"straw\"]}",
        );
        let idx = index(&["src.json", "tgt.json"]);
        let r = set_rule(
            Extract::Structured(Format::Json, "$.need[*]".into()),
            set_targets(),
            Relation::Subset,
        );
        assert!(eval(&r, root, &idx).is_empty());
    }

    // ─── identical ──────────────────────────────────────────────

    #[test]
    fn build_rejects_invalid_extract_regex() {
        use crate::test_support::spec_yaml;
        let spec = spec_yaml(
            "id: t\n\
             kind: cross_file\n\
             relation: equals\n\
             source: { file: a.txt, extract: { regex: \"(unclosed\" } }\n\
             targets: { files: \"**/*.txt\", extract: { regex: \".*\" } }\n\
             level: error\n",
        );
        let err = build(&spec).unwrap_err();
        assert!(err.to_string().contains("regex"), "{err}");
    }

    #[test]
    fn build_rejects_skip_header_on_non_identical() {
        use crate::test_support::spec_yaml;
        let spec = spec_yaml(
            "id: t\n\
             kind: cross_file\n\
             relation: equals\n\
             source: { file: a.txt, extract: { regex: \"(.*)\" } }\n\
             targets: { files: \"**/*.txt\", extract: { regex: \"(.*)\" } }\n\
             skip_header_lines: 2\n\
             level: error\n",
        );
        let err = build(&spec).unwrap_err();
        assert!(err.to_string().contains("skip_header_lines"), "{err}");
    }

    #[test]
    fn build_rejects_normalize_on_identical() {
        use crate::test_support::spec_yaml;
        let spec = spec_yaml(
            "id: t\n\
             kind: cross_file\n\
             relation: identical\n\
             source: { file: a.txt }\n\
             targets: { files: \"**/*.txt\" }\n\
             normalize: trim\n\
             level: error\n",
        );
        let err = build(&spec).unwrap_err();
        assert!(err.to_string().contains("normalize"), "{err}");
    }

    #[test]
    fn build_surfaces_friendly_untagged_error_through_options_wrapper() {
        // The `expecting` message must reach the user through build()'s
        // `invalid options: {e}` wrapping, not just the isolated enum. Guards a
        // refactor that drops the inner error from the wrapper (audit M3).
        use crate::test_support::spec_yaml;
        let spec = spec_yaml(
            "id: t\n\
             kind: cross_file\n\
             relation: equals\n\
             source: { file: a.txt, extract: { toml: \"$.v\" } }\n\
             normalize: 42\n\
             level: error\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(
            err.contains("a normalize transform"),
            "friendly text lost through the options wrapper: {err}"
        );
        assert!(!err.contains("NormalizeSpec"), "leaks internal enum: {err}");
    }

    fn identical_rule(targets: Targets, skip_header_lines: usize) -> CrossFileRule {
        CrossFileRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            source_file: "README.md".into(),
            source_glob: None,
            source_extract: None,
            targets: Some(targets),
            relation: Relation::Identical,
            normalize: Vec::new(),
            allow_missing: false,
            skip_header_lines,
        }
    }

    #[test]
    fn identical_fires_on_byte_difference_silent_on_match() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("README.md"), "# Project\n\nHello.\n").unwrap();
        std::fs::create_dir_all(root.join("crates/a")).unwrap();
        std::fs::create_dir_all(root.join("crates/b")).unwrap();
        // a mirrors exactly; b drifts by a byte.
        std::fs::write(root.join("crates/a/README.md"), "# Project\n\nHello.\n").unwrap();
        std::fs::write(root.join("crates/b/README.md"), "# Project\n\nHello!\n").unwrap();
        let idx = index(&["README.md", "crates/a/README.md", "crates/b/README.md"]);
        let r = identical_rule(
            Targets::Glob {
                scope: Scope::from_patterns(&["crates/*/README.md".to_string()]).unwrap(),
                extract: None,
            },
            0,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "only crates/b drifts: {v:?}");
        assert!(v[0].message.contains("crates/b/README.md"));
        assert!(v[0].message.contains("not byte-identical"));
    }

    #[test]
    fn identical_root_escape_target_fires_without_reading() {
        // Security regression (v0.12 path-confinement): an absolute
        // targets[].file must produce an "escapes the repo root"
        // violation, never read an out-of-tree file.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("README.md"), "# Project\n").unwrap();
        let idx = index(&["README.md"]);
        let r = identical_rule(Targets::List(vec![("/etc/hostname".into(), None)]), 0);
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].message.contains("escapes the repo root"),
            "{}",
            v[0].message
        );
    }

    #[test]
    fn identical_skip_header_lines_ignores_a_differing_header() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Two leading lines differ; the body is identical.
        std::fs::write(root.join("README.md"), "// 2024 Acme\n// gen\nBODY\n").unwrap();
        std::fs::write(root.join("mirror.md"), "// 2025 Acme\n// gen2\nBODY\n").unwrap();
        let idx = index(&["README.md", "mirror.md"]);
        let mk = |skip| identical_rule(Targets::List(vec![("mirror.md".into(), None)]), skip);
        // skip 2 -> bodies match.
        assert!(eval(&mk(2), root, &idx).is_empty());
        // skip 0 -> headers differ -> fires.
        assert_eq!(eval(&mk(0), root, &idx).len(), 1);
    }

    // ─── resolves ───────────────────────────────────────────────

    fn resolves_rule(source_file: &str, extract: Extract) -> CrossFileRule {
        CrossFileRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            source_file: source_file.into(),
            source_glob: None,
            source_extract: Some(extract),
            targets: None,
            relation: Relation::Resolves,
            normalize: Vec::new(),
            allow_missing: false,
            skip_header_lines: 0,
        }
    }

    fn index_with_dirs(files: &[&str], dirs: &[&str]) -> FileIndex {
        let mut e: Vec<FileEntry> = files
            .iter()
            .map(|p| FileEntry {
                path: Path::new(p).into(),
                is_dir: false,
                size: 1,
            })
            .collect();
        e.extend(dirs.iter().map(|p| FileEntry {
            path: Path::new(p).into(),
            is_dir: true,
            size: 0,
        }));
        FileIndex::from_entries(e)
    }

    #[test]
    fn resolves_fires_on_a_declared_path_that_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/a\", \"crates/gone\"]\n",
        )
        .unwrap();
        // crates/a exists (a dir); crates/gone does not.
        let idx = index_with_dirs(&["Cargo.toml"], &["crates/a"]);
        let r = resolves_rule(
            "Cargo.toml",
            Extract::Structured(Format::Toml, "$.workspace.members[*]".into()),
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("crates/gone"));
        assert!(v[0].message.contains("does not resolve"));
    }

    #[test]
    fn resolves_silent_when_every_path_exists() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("manifest.txt"), "src/a.rs\nsrc/b.rs\n").unwrap();
        let idx = index(&["manifest.txt", "src/a.rs", "src/b.rs"]);
        let r = resolves_rule(
            "manifest.txt",
            Extract::Lines(alint_core::LinesOpts::default()),
        );
        assert!(eval(&r, root, &idx).is_empty());
    }

    // ─── build-time shape validation ────────────────────────────

    #[test]
    fn build_enforces_per_relation_shape() {
        use crate::test_support::spec_yaml;
        // identical with a stray source.extract -> rejected.
        let bad_identical = "id: t\nkind: cross_file\nsource:\n  file: a\n  \
            extract:\n    lines: {}\ntargets:\n  files: \"b/*\"\nrelation: identical\nlevel: error\n";
        assert!(
            build(&spec_yaml(bad_identical)).is_err(),
            "identical must not take source.extract"
        );
        // resolves with targets -> rejected.
        let bad_resolves = "id: t\nkind: cross_file\nsource:\n  file: a\n  \
            extract:\n    lines: {}\ntargets:\n  files: \"b/*\"\n  extract:\n    lines: {}\n\
            relation: resolves\nlevel: error\n";
        assert!(
            build(&spec_yaml(bad_resolves)).is_err(),
            "resolves must not take targets"
        );
        // value relation missing source.extract -> rejected.
        let bad_value = "id: t\nkind: cross_file\nsource:\n  file: a\ntargets:\n  \
            files: \"b/*\"\n  extract:\n    lines: {}\nrelation: subset\nlevel: error\n";
        assert!(
            build(&spec_yaml(bad_value)).is_err(),
            "value relation needs source.extract"
        );
        // valid identical (no extract anywhere) -> builds.
        let ok_identical = "id: t\nkind: cross_file\nsource:\n  file: README.md\n\
            targets:\n  files: \"crates/*/README.md\"\nrelation: identical\nlevel: error\n";
        assert!(
            build(&spec_yaml(ok_identical)).is_ok(),
            "valid identical should build"
        );
        // valid resolves (source extract, no targets) -> builds.
        let ok_resolves = "id: t\nkind: cross_file\nsource:\n  file: Cargo.toml\n  \
            extract:\n    toml: \"$.workspace.members[*]\"\nrelation: resolves\nlevel: error\n";
        assert!(
            build(&spec_yaml(ok_resolves)).is_ok(),
            "valid resolves should build"
        );
    }
}
