//! `ScopeFilter` — per-file rule gate that scopes rule
//! application to files whose ancestor directories contain a
//! named manifest. The "closest-ancestor manifest" pattern, in
//! practical terms.
//!
//! Composes with the existing per-file `paths:` glob and the
//! tree-level `when:` gate as an AND. A file matches a rule
//! iff every gate it has accepts the file.
//!
//! ## Why
//!
//! Bundled ecosystem rulesets (`rust@v1`, `node@v1`, …) need
//! to scope per-file rules to only files inside a package of
//! the matching ecosystem. A `**/*.rs` glob alone is too
//! broad: in a polyglot monorepo, `services/web/scripts/
//! migrate.rs` shouldn't be governed by Rust hygiene rules
//! just because it has the `.rs` extension. With
//! `scope_filter: { has_ancestor: Cargo.toml }`, the rule
//! only fires on files that have a `Cargo.toml` somewhere in
//! their ancestor chain — i.e., files inside an actual Rust
//! package.
//!
//! ## Semantics
//!
//! For a file at `crates/api/src/main.rs`, `has_ancestor:
//! Cargo.toml` walks the ancestor chain `crates/api/src/`,
//! `crates/api/`, `crates/`, root, and returns true on the
//! first match. The walk includes the file's own directory:
//! `crates/api/Cargo.toml` itself matches because
//! `crates/api/` (the file's parent) contains a `Cargo.toml`.
//!
//! See `docs/design/v0.9/scope-filter.md` for full design,
//! pinned decisions, and the bundled-ruleset migration plan.
//!
//! ## Performance
//!
//! Each `has_ancestor` check walks `Path::parent()` upward
//! and consults [`FileIndex::contains_file`] (the v0.9.5
//! path-index) at each step. Both operations are O(1)
//! hashlookups; per-file overhead is `O(depth × M)` where
//! `M` is the number of names in the `has_ancestor` list.
//! Typical: 5 levels × 1 manifest = 150 ns / file. At 1M
//! files × 5 rules with `scope_filter`, total overhead is
//! ~750 ms — and that's before the file-read savings the
//! filter unlocks.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Deserializer};

use crate::error::{Error, Result};
use crate::extract::{Extract, ExtractSpec, extract_values, is_non_literal};
use crate::pathsafe::{derive_target, normalize_confined};
use crate::walker::FileIndex;

/// Per-file rule gate. Today's only primitive is
/// `has_ancestor`; the type is an enum-shape struct so future
/// primitives (`closest_ancestor_with_content`, etc.) can land
/// without breaking the public surface.
///
/// Build with [`ScopeFilter::from_spec`] to get the
/// build-time validation (rejects globs, separators, empty
/// lists). Direct construction is allowed for tests via
/// [`ScopeFilter::has_ancestor_unchecked`].
#[derive(Debug, Clone)]
pub struct ScopeFilter {
    has_ancestor: Vec<PathBuf>,
    /// `changed_since: <ref>` — when set, the file must also be in the
    /// `<ref>...HEAD` diff (resolved once per run, cached on the
    /// [`FileIndex`]). AND-composes with `has_ancestor`. Empty
    /// `has_ancestor` + `Some` `changed_since` is a diff-only filter.
    changed_since: Option<String>,
    /// `include_manifest_paths:` / `exclude_manifest_paths:` — gate each file by
    /// membership in a path set extracted from a manifest (ADR-0010). Each
    /// predicate's set is resolved once per run and cached on the [`FileIndex`],
    /// exactly like `changed_since`. AND-composes with the others.
    manifest_predicates: Vec<ManifestPredicate>,
}

/// YAML-level shape of `scope_filter:`. Deserialised by
/// [`RuleSpec`](crate::config::RuleSpec) and validated into
/// the runtime [`ScopeFilter`] via
/// [`ScopeFilter::from_spec`].
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeFilterSpec {
    /// Single literal filename or non-empty list of literal
    /// filenames. Each must be a basename (no path separator,
    /// no glob metacharacters). Optional since v0.11 — a
    /// `scope_filter:` with only `changed_since:` is valid.
    #[serde(default, deserialize_with = "deserialize_opt_string_or_list")]
    pub has_ancestor: Option<Vec<String>>,
    /// `changed_since: <git-ref>` — narrow the rule to files in the
    /// `<ref>...HEAD` diff. Accepts the `{{env.X}}` interpolation
    /// (resolved at config load). At least one of `has_ancestor:` /
    /// `changed_since:` / `include_manifest_paths:` / `exclude_manifest_paths:`
    /// must be present.
    #[serde(default)]
    pub changed_since: Option<String>,
    /// `include_manifest_paths:` — keep only files whose path is in the
    /// manifest-derived set (ADR-0010). A file the base `paths:` glob matched
    /// but the manifest does not declare is dropped.
    #[serde(default)]
    pub include_manifest_paths: Option<ManifestPathSpec>,
    /// `exclude_manifest_paths:` — drop files whose path is in the
    /// manifest-derived set. The complement of `include_manifest_paths:`.
    #[serde(default)]
    pub exclude_manifest_paths: Option<ManifestPathSpec>,
}

/// YAML shape of a manifest-derived path set, shared by
/// `include_manifest_paths:` / `exclude_manifest_paths:`. The set is extracted
/// from `source` once per run, optionally mapped by `derive_target`, resolved
/// relative to the manifest's own directory, and confined to the repo root. See
/// ADR-0010 and `docs/design/v0.15/manifest-derived-scope.md`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestPathSpec {
    /// The manifest file, always repo-root-confined (a manifest a rule scopes
    /// by lives in the repo; an escaping `source` is a build-time error). Its
    /// declared paths resolve relative to its own directory.
    pub source: String,
    /// The `{toml|json|yaml|lines|regex}` extractor shared with
    /// `registry_paths_resolve` / `file_graph`. Non-literal / interpolated
    /// entries are dropped, not failed.
    pub extract: ExtractSpec,
    /// Optional `{from, to}` regex mapping applied to each extracted path — e.g.
    /// map a `package.json` `bin` output (`dist/cli.js`) back to its source
    /// (`src/cli.ts`). An entry that does not match `from` is dropped, exactly
    /// as `file_graph`'s `derive_target` drops a non-matching node.
    #[serde(default)]
    pub derive_target: Option<ManifestDeriveTarget>,
    /// `include_manifest_paths:` only — warn when the extracted set is empty
    /// (default `true`). An empty include set silently no-ops the whole rule, a
    /// footgun; set `false` for a rule that legitimately tolerates it.
    #[serde(default)]
    pub expect_nonempty: Option<bool>,
}

/// `{from, to}` output-to-source mapping (the same shape `file_graph` uses).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestDeriveTarget {
    /// A regex matched against each extracted path; `$1`-style capture groups
    /// substitute into `to`.
    pub from: String,
    /// The replacement template producing the mapped (source) path.
    pub to: String,
}

fn deserialize_opt_string_or_list<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged, expecting = "a string, or a list of strings")]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(s) => Ok(Some(vec![s])),
        OneOrMany::Many(v) => Ok(Some(v)),
    }
}

/// Whether a manifest predicate keeps or drops files in its set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManifestSense {
    /// `include_manifest_paths:` — keep only files IN the set.
    Include,
    /// `exclude_manifest_paths:` — drop files IN the set.
    Exclude,
}

/// The validated, resolve-ready form of one `*_manifest_paths:` predicate. The
/// path SET it gates by is resolved once per run by the engine and cached on the
/// [`FileIndex`] under [`Self::cache_key`]; this struct holds only the config
/// needed to key that cache and resolve the set from the manifest text.
#[derive(Debug, Clone)]
pub(crate) struct ManifestPredicate {
    /// Canonical `(source, extract, derive_target)` key. Two predicates with an
    /// identical config share one resolved set — resolved and cached once.
    cache_key: String,
    /// The manifest path, relative to the repo root (confined at build time).
    source: PathBuf,
    /// The resolved extractor.
    extract: Extract,
    /// The compiled `derive_target`, if any: `(from regex, to template)`.
    derive_target: Option<(Regex, String)>,
    /// Include vs exclude.
    sense: ManifestSense,
    /// Warn on an empty extracted set (include predicates only).
    expect_nonempty: bool,
}

impl ManifestPredicate {
    /// Build + validate from the config half: confine `source`, resolve the
    /// extractor, compile the `derive_target` regex, and compute the cache key.
    fn from_spec(rule_id: &str, spec: ManifestPathSpec, sense: ManifestSense) -> Result<Self> {
        let source = normalize_confined(Path::new(&spec.source)).ok_or_else(|| {
            Error::rule_config(
                rule_id,
                format!(
                    "scope_filter manifest `source:` must be a repo-root-relative path, got {:?}",
                    spec.source
                ),
            )
        })?;
        let extract = spec.extract.resolve().map_err(|e| {
            Error::rule_config(rule_id, format!("scope_filter manifest `extract:` {e}"))
        })?;
        let derive_target = match spec.derive_target {
            Some(dt) => {
                let re = Regex::new(&dt.from).map_err(|e| {
                    Error::rule_config(
                        rule_id,
                        format!(
                            "scope_filter manifest `derive_target.from:` is not a valid regex: {e}"
                        ),
                    )
                })?;
                Some((re, dt.to))
            }
            None => None,
        };
        let dt_key = derive_target
            .as_ref()
            .map(|(from, to)| format!("{}=>{to}", from.as_str()))
            .unwrap_or_default();
        // `Extract`'s Debug is a stable, injective rendering of the resolved
        // extractor — enough to co-cache two rules with an identical config.
        let cache_key = format!("{}\u{0}{extract:?}\u{0}{dt_key}", source.display());
        Ok(Self {
            cache_key,
            source,
            extract,
            derive_target,
            sense,
            expect_nonempty: spec.expect_nonempty.unwrap_or(true),
        })
    }

    /// The cache key the engine resolves under and [`ScopeFilter::matches`]
    /// looks the resolved set up by.
    pub(crate) fn cache_key(&self) -> &str {
        &self.cache_key
    }

    /// The manifest path (repo-root-relative) the engine reads.
    pub(crate) fn source(&self) -> &Path {
        &self.source
    }

    /// True for an `include_manifest_paths:` predicate that wants a non-empty
    /// set — the engine warns when its resolved set is empty.
    pub(crate) fn warns_on_empty(&self) -> bool {
        self.sense == ManifestSense::Include && self.expect_nonempty
    }

    /// Resolve the declared path set from the manifest's text: extract, drop
    /// non-literal entries, optionally `derive_target`-map, resolve relative to
    /// the manifest's directory, and confine each result to the repo root. Pure
    /// — the engine does the file read and caches the result on the
    /// [`FileIndex`]. An unparseable manifest / bad extract yields the empty set
    /// (the engine warns); the predicate then contributes nothing.
    pub(crate) fn resolve_set(&self, text: &str) -> HashSet<PathBuf> {
        let base = self.source.parent().unwrap_or_else(|| Path::new(""));
        let Ok(raw) = extract_values(&self.extract, text) else {
            return HashSet::new();
        };
        raw.into_iter()
            .filter(|e| !is_non_literal(e))
            .filter_map(|entry| {
                let mapped = match &self.derive_target {
                    // A non-matching entry is not a mapped output — dropped,
                    // matching file_graph's derive_target.
                    Some((from, to)) => derive_target(from, to, &entry)?,
                    None => PathBuf::from(entry),
                };
                normalize_confined(&base.join(mapped))
            })
            .collect()
    }
}

/// One manifest predicate's resolved path set, for `alint explain` to surface
/// what the manifest scope resolves to (the design's legibility mitigation).
#[derive(Debug, Clone)]
pub struct ResolvedManifestScope {
    /// `true` for `include_manifest_paths`, `false` for `exclude_manifest_paths`.
    pub include: bool,
    /// The manifest source (repo-root-relative).
    pub source: PathBuf,
    /// The resolved, confined declared paths, sorted.
    pub paths: Vec<PathBuf>,
}

/// Read a manifest for `explain`'s resolved-set display, which (unlike the
/// engine) has no `FileIndex` to read through: confine against a `source` that
/// resolves — via a symlink — outside `root` (the canonical target must stay
/// under the canonical root, ADR-0004), and cap the read at the analysis limit.
/// Returns "" if the manifest is absent, escapes the root, or is too large; the
/// predicate then shows the empty set.
fn read_manifest_confined(root: &Path, rel: &Path) -> String {
    let abs = root.join(rel);
    let (Ok(croot), Ok(cabs)) = (root.canonicalize(), abs.canonicalize()) else {
        return String::new();
    };
    if !cabs.starts_with(&croot) {
        return String::new(); // a symlinked source that escapes the root
    }
    let Ok(meta) = std::fs::metadata(&cabs) else {
        return String::new();
    };
    // Only a regular file is a manifest: a FIFO / device / socket would block or
    // hang the read (the walker applies the same is_file filter for the engine).
    if !meta.is_file() {
        return String::new();
    }
    crate::walker::read_capped_or_skip(&cabs, meta.len())
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

impl ScopeFilter {
    /// Build from the deserialised spec, validating every
    /// `has_ancestor` entry. Returns `Error::rule_config` on
    /// any of:
    ///
    /// - empty list
    /// - empty string
    /// - string contains a path separator (`/` or `\`)
    /// - string contains a glob metacharacter
    ///   (`* ? [ ] { } !`)
    pub fn from_spec(rule_id: &str, spec: ScopeFilterSpec) -> Result<Self> {
        let has_ancestor = match spec.has_ancestor {
            Some(names) => {
                if names.is_empty() {
                    return Err(Error::rule_config(
                        rule_id,
                        "scope_filter.has_ancestor must be a non-empty list",
                    ));
                }
                let mut paths = Vec::with_capacity(names.len());
                for name in names {
                    validate_manifest_name(rule_id, &name)?;
                    paths.push(PathBuf::from(name));
                }
                paths
            }
            None => Vec::new(),
        };
        let mut manifest_predicates = Vec::new();
        if let Some(inc) = spec.include_manifest_paths {
            manifest_predicates.push(ManifestPredicate::from_spec(
                rule_id,
                inc,
                ManifestSense::Include,
            )?);
        }
        if let Some(exc) = spec.exclude_manifest_paths {
            manifest_predicates.push(ManifestPredicate::from_spec(
                rule_id,
                exc,
                ManifestSense::Exclude,
            )?);
        }
        if has_ancestor.is_empty() && spec.changed_since.is_none() && manifest_predicates.is_empty()
        {
            return Err(Error::rule_config(
                rule_id,
                "scope_filter must set at least one of `has_ancestor:`, `changed_since:`, \
                 `include_manifest_paths:`, or `exclude_manifest_paths:`",
            ));
        }
        Ok(Self {
            has_ancestor,
            changed_since: spec.changed_since,
            manifest_predicates,
        })
    }

    /// Direct construction without validation. Tests only.
    #[doc(hidden)]
    pub fn has_ancestor_unchecked(names: Vec<&str>) -> Self {
        Self {
            has_ancestor: names.into_iter().map(PathBuf::from).collect(),
            changed_since: None,
            manifest_predicates: Vec::new(),
        }
    }

    /// Direct construction of a diff-only filter. Tests only.
    #[doc(hidden)]
    pub fn changed_since_unchecked(since: &str) -> Self {
        Self {
            has_ancestor: Vec::new(),
            changed_since: Some(since.to_string()),
            manifest_predicates: Vec::new(),
        }
    }

    /// The configured `changed_since:` ref, if any. The engine reads
    /// this from every per-file rule to know which diffs to resolve.
    #[must_use]
    pub fn changed_since(&self) -> Option<&str> {
        self.changed_since.as_deref()
    }

    /// True iff at least one of the configured ancestor
    /// names exists as a file in some ancestor directory of
    /// `file` — including the file's own directory.
    ///
    /// Walks `Path::parent()` upward from the file, joins the
    /// candidate ancestor name to each directory, and consults
    /// `index.contains_file(...)`. First match wins; the
    /// matching ancestor's path is not exposed (this is a
    /// boolean filter).
    pub fn matches(&self, file: &Path, index: &FileIndex) -> bool {
        if !self.has_ancestor.is_empty() && !self.ancestor_matches(file, index) {
            return false;
        }
        if let Some(since) = &self.changed_since {
            // The diff set is resolved once per run and cached on the
            // index; a missing entry (ref the engine didn't resolve, or
            // a no-git repo) matches nothing — the documented silent
            // no-op.
            let in_diff = index
                .changed_paths(since)
                .is_some_and(|set| set.contains(file));
            if !in_diff {
                return false;
            }
        }
        for pred in &self.manifest_predicates {
            // The manifest set is resolved once per run and cached on the index
            // (like `changed_since`); a missing entry (manifest absent /
            // unresolved) is the empty set. Membership is component-wise prefix:
            // a declared FILE (`bin` -> `src/cli.ts`) matches itself, and a
            // declared DIRECTORY (`Cargo.toml` `workspace.members` -> `crates/a`)
            // matches every file under it. `Path::starts_with` respects component
            // boundaries, so `crates/a` does not match `crates/ab/x.rs`. `include`
            // keeps files IN the set; `exclude` drops files IN it.
            let in_set = index
                .manifest_paths(pred.cache_key())
                .is_some_and(|set| set.iter().any(|declared| file.starts_with(declared)));
            let keep = match pred.sense {
                ManifestSense::Include => in_set,
                ManifestSense::Exclude => !in_set,
            };
            if !keep {
                return false;
            }
        }
        true
    }

    /// The manifest predicates configured on this filter. The engine reads these
    /// from every per-file rule to resolve each declared path set once per run.
    pub(crate) fn manifest_predicates(&self) -> &[ManifestPredicate] {
        &self.manifest_predicates
    }

    /// Resolve each manifest predicate's declared path set against `root`, for
    /// display by `alint explain`. Reads each manifest (absent → empty set).
    /// Not the hot path — the engine caches these on the [`FileIndex`] for
    /// [`matches`](Self::matches).
    #[must_use]
    pub fn resolved_manifest_sets(&self, root: &Path) -> Vec<ResolvedManifestScope> {
        self.manifest_predicates
            .iter()
            .map(|p| {
                let text = read_manifest_confined(root, &p.source);
                let mut paths: Vec<PathBuf> = p.resolve_set(&text).into_iter().collect();
                paths.sort();
                ResolvedManifestScope {
                    include: p.sense == ManifestSense::Include,
                    source: p.source.clone(),
                    paths,
                }
            })
            .collect()
    }

    /// The `has_ancestor` walk, factored out so [`matches`](Self::matches)
    /// can AND it with `changed_since`.
    fn ancestor_matches(&self, file: &Path, index: &FileIndex) -> bool {
        let mut cur = file.parent();
        loop {
            let dir = cur.unwrap_or_else(|| Path::new(""));
            for name in &self.has_ancestor {
                let candidate = dir.join(name);
                if index.contains_file(&candidate) {
                    return true;
                }
            }
            match cur {
                Some(p) if p.as_os_str().is_empty() => return false,
                Some(p) => cur = p.parent(),
                None => return false,
            }
        }
    }

    /// The configured ancestor names, for diagnostics and
    /// audits (e.g.
    /// `coverage_audit_scope_filter.rs`).
    pub fn has_ancestor_names(&self) -> &[PathBuf] {
        &self.has_ancestor
    }
}

/// Build-time guard for cross-file rule builders. Cross-file
/// rules express ancestor scoping through `for_each_dir +
/// when_iter:` instead of `scope_filter:`; the engine consults
/// the per-file dispatch path's `Scope::matches` (which folds
/// in `scope_filter` since v0.9.10), so a cross-file rule with
/// `scope_filter:` set would silently ignore the field. This
/// helper produces a clear build-time error so the
/// misconfiguration surfaces at config-load time rather than
/// as a confused-rule-doesn't-fire bug.
///
/// Usage in a cross-file rule builder:
///
/// ```ignore
/// pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
///     reject_scope_filter_on_cross_file(spec, "for_each_dir")?;
///     // …
/// }
/// ```
pub fn reject_scope_filter_on_cross_file(
    spec: &crate::config::RuleSpec,
    cross_file_kind_label: &str,
) -> Result<()> {
    if spec.scope_filter.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            format!(
                "scope_filter is supported on per-file rules only; {cross_file_kind_label} is a \
                 cross-file rule. Express ancestor scoping via `for_each_dir + when_iter:` \
                 instead — see docs/design/v0.9/scope-filter.md for the pattern."
            ),
        ));
    }
    Ok(())
}

/// Build-time guard for rules whose evaluation target is fixed
/// (a hardcoded path or a tree-level invariant), making
/// `scope_filter:` semantically meaningless. Sister helper to
/// [`reject_scope_filter_on_cross_file`]; used by rules like
/// `no_submodules` (hardcoded to `.gitmodules` at the repo
/// root) where the user-supplied filter has nothing to scope.
///
/// `reason` is the user-facing why-can't-I-use-it: it gets
/// inlined into the error message after `"...scope_filter is not
/// supported on <rule_kind>; "`. Keep it as a single sentence
/// fragment that completes that lead. Example: `"this rule is
/// hardcoded to check `.gitmodules` at the repository root"`.
///
/// Usage in a rule builder:
///
/// ```ignore
/// pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
///     reject_scope_filter_with_reason(
///         spec,
///         "no_submodules",
///         "this rule is hardcoded to check `.gitmodules` at the repository root",
///     )?;
///     // …
/// }
/// ```
pub fn reject_scope_filter_with_reason(
    spec: &crate::config::RuleSpec,
    rule_kind: &str,
    reason: &str,
) -> Result<()> {
    if spec.scope_filter.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            format!("scope_filter is not supported on {rule_kind}; {reason}"),
        ));
    }
    Ok(())
}

fn validate_manifest_name(rule_id: &str, name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::rule_config(
            rule_id,
            "scope_filter.has_ancestor names must not be empty",
        ));
    }
    if name.contains('/') || name.contains('\\') {
        // Pitfall #11 in `docs/development/CONFIG-AUTHORING.md`: the
        // most common adopter mistake is reaching for `has_ancestor`
        // to scope by directory (e.g. `airflow-core/pyproject.toml`),
        // when the right answer is a `paths:` glob on the rule's
        // main scope. Surface that distinction in the error message.
        let basename = name.rsplit(['/', '\\']).next().unwrap_or(name);
        return Err(Error::rule_config(
            rule_id,
            format!(
                "scope_filter.has_ancestor name {name:?} must be a basename — no path separators.\n  \
                 hint: to match files inside a specific subtree, use `paths:` on the rule's main \
                 scope (e.g. `paths: \"airflow-core/**/*.py\"`); to match files in any subtree \
                 that has this manifest, use the basename only (e.g. `has_ancestor: {basename:?}`)."
            ),
        ));
    }
    if name
        .chars()
        .any(|c| matches!(c, '*' | '?' | '[' | ']' | '{' | '}' | '!'))
    {
        return Err(Error::rule_config(
            rule_id,
            format!(
                "scope_filter.has_ancestor name {name:?} must be a literal — no glob \
                 metacharacters allowed (use `Cargo.toml`, not `*.toml`)"
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walker::{FileEntry, FileIndex};
    use std::path::Path;
    use std::sync::Arc;

    fn idx(paths: &[&str]) -> FileIndex {
        FileIndex::from_entries(
            paths
                .iter()
                .map(|p| FileEntry {
                    path: Arc::<Path>::from(Path::new(p)),
                    is_dir: false,
                    size: 0,
                })
                .collect(),
        )
    }

    fn filter(names: Vec<&str>) -> ScopeFilter {
        ScopeFilter::has_ancestor_unchecked(names)
    }

    #[test]
    fn root_manifest_matches_root_file() {
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&["Cargo.toml", "lib.rs"]);
        assert!(f.matches(Path::new("lib.rs"), &i));
    }

    #[test]
    fn root_manifest_matches_nested_file() {
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&["Cargo.toml", "src/lib.rs"]);
        assert!(f.matches(Path::new("src/lib.rs"), &i));
    }

    #[test]
    fn nested_manifest_matches_own_dir() {
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&["crates/api/Cargo.toml", "crates/api/src/main.rs"]);
        // Manifest at crates/api/ — main.rs's ancestor.
        assert!(f.matches(Path::new("crates/api/src/main.rs"), &i));
    }

    #[test]
    fn manifest_at_files_own_dir_matches_the_manifest_itself() {
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&["crates/api/Cargo.toml"]);
        // `Cargo.toml` is in the file's own dir → match.
        assert!(f.matches(Path::new("crates/api/Cargo.toml"), &i));
    }

    #[test]
    fn root_cargo_toml_matches_itself() {
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&["Cargo.toml"]);
        assert!(f.matches(Path::new("Cargo.toml"), &i));
    }

    #[test]
    fn no_manifest_in_any_ancestor_returns_false() {
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&["src/lib.rs"]);
        assert!(!f.matches(Path::new("src/lib.rs"), &i));
    }

    #[test]
    fn sibling_manifest_does_not_match() {
        let f = filter(vec!["Cargo.toml"]);
        // Sibling has Cargo.toml, but our file is in a different subtree.
        let i = idx(&["crates/api/Cargo.toml", "services/web/src/index.ts"]);
        assert!(!f.matches(Path::new("services/web/src/index.ts"), &i));
    }

    #[test]
    fn two_name_list_matches_if_either_found() {
        let f = filter(vec!["pyproject.toml", "setup.py"]);
        let i = idx(&["app/setup.py", "app/main.py"]);
        assert!(f.matches(Path::new("app/main.py"), &i));
    }

    #[test]
    fn closest_ancestor_among_multiple() {
        // Both root and crates/api have Cargo.toml. Either match.
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&[
            "Cargo.toml",
            "crates/api/Cargo.toml",
            "crates/api/src/main.rs",
        ]);
        assert!(f.matches(Path::new("crates/api/src/main.rs"), &i));
    }

    // ── from_spec validation ──────────────────────────────────

    #[test]
    fn from_spec_rejects_empty_list() {
        let err = ScopeFilter::from_spec(
            "r",
            ScopeFilterSpec {
                has_ancestor: Some(vec![]),
                changed_since: None,
                include_manifest_paths: None,
                exclude_manifest_paths: None,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("non-empty"), "msg: {err}");
    }

    #[test]
    fn from_spec_rejects_empty_string() {
        let err = ScopeFilter::from_spec(
            "r",
            ScopeFilterSpec {
                has_ancestor: Some(vec![String::new()]),
                changed_since: None,
                include_manifest_paths: None,
                exclude_manifest_paths: None,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("not be empty"), "msg: {err}");
    }

    #[test]
    fn from_spec_rejects_path_separator() {
        let err = ScopeFilter::from_spec(
            "r",
            ScopeFilterSpec {
                has_ancestor: Some(vec!["foo/bar".into()]),
                changed_since: None,
                include_manifest_paths: None,
                exclude_manifest_paths: None,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("path separators"), "msg: {err}");
    }

    #[test]
    fn from_spec_rejects_glob_metacharacters() {
        for bad in &["*.toml", "Cargo?", "[abc]", "{a,b}", "!Cargo"] {
            let err = ScopeFilter::from_spec(
                "r",
                ScopeFilterSpec {
                    has_ancestor: Some(vec![(*bad).into()]),
                    changed_since: None,
                    include_manifest_paths: None,
                    exclude_manifest_paths: None,
                },
            )
            .unwrap_err();
            assert!(err.to_string().contains("glob"), "msg for {bad:?}: {err}");
        }
    }

    #[test]
    fn from_spec_accepts_canonical_manifests() {
        for good in &[
            "Cargo.toml",
            "package.json",
            "pyproject.toml",
            "setup.py",
            "go.mod",
            "pom.xml",
            "build.gradle",
            "build.gradle.kts",
        ] {
            ScopeFilter::from_spec(
                "r",
                ScopeFilterSpec {
                    has_ancestor: Some(vec![(*good).into()]),
                    changed_since: None,
                    include_manifest_paths: None,
                    exclude_manifest_paths: None,
                },
            )
            .unwrap_or_else(|e| panic!("{good:?} should be valid; got {e}"));
        }
    }

    // ── deserialise OneOrMany ─────────────────────────────────

    #[test]
    fn deserialize_single_string_form() {
        let yaml = "has_ancestor: Cargo.toml\n";
        let spec: ScopeFilterSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(spec.has_ancestor, Some(vec!["Cargo.toml".to_string()]));
        assert_eq!(spec.changed_since, None);
    }

    #[test]
    fn deserialize_changed_since_only_form() {
        let yaml = "changed_since: origin/main\n";
        let spec: ScopeFilterSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(spec.has_ancestor, None);
        assert_eq!(spec.changed_since.as_deref(), Some("origin/main"));
    }

    #[test]
    fn deserialize_list_form() {
        let yaml = "has_ancestor:\n  - pom.xml\n  - build.gradle\n";
        let spec: ScopeFilterSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(
            spec.has_ancestor,
            Some(vec!["pom.xml".to_string(), "build.gradle".to_string()]),
        );
    }

    #[test]
    fn deserialize_rejects_unknown_field() {
        let yaml = "has_ancestor: Cargo.toml\nunknown: x\n";
        assert!(serde_yaml_ng::from_str::<ScopeFilterSpec>(yaml).is_err());
    }

    // ── changed_since ─────────────────────────────────────────

    fn idx_with_diff(paths: &[&str], since: &str, diff: &[&str]) -> FileIndex {
        let i = idx(paths);
        let mut map = std::collections::HashMap::new();
        map.insert(since.to_string(), diff.iter().map(PathBuf::from).collect());
        i.set_changed_paths(map);
        i
    }

    #[test]
    fn changed_since_matches_only_files_in_diff() {
        let f = ScopeFilter::changed_since_unchecked("origin/main");
        let i = idx_with_diff(&["src/a.rs", "src/b.rs"], "origin/main", &["src/a.rs"]);
        assert!(f.matches(Path::new("src/a.rs"), &i), "in-diff file matches");
        assert!(
            !f.matches(Path::new("src/b.rs"), &i),
            "out-of-diff file skipped"
        );
    }

    #[test]
    fn changed_since_with_unpopulated_cache_matches_nothing() {
        // No git / unresolved ref → empty/absent cache → silent no-op.
        let f = ScopeFilter::changed_since_unchecked("origin/main");
        let i = idx(&["src/a.rs"]);
        assert!(!f.matches(Path::new("src/a.rs"), &i));
    }

    #[test]
    fn changed_since_and_composes_with_has_ancestor() {
        // Both gates must hold. a.rs is in the diff AND under a
        // Cargo.toml; b.rs is in the diff but has no ancestor manifest.
        let f = ScopeFilter {
            has_ancestor: vec![PathBuf::from("Cargo.toml")],
            changed_since: Some("origin/main".to_string()),
            manifest_predicates: Vec::new(),
        };
        let i = idx_with_diff(
            &["crates/x/Cargo.toml", "crates/x/a.rs", "loose/b.rs"],
            "origin/main",
            &["crates/x/a.rs", "loose/b.rs"],
        );
        assert!(f.matches(Path::new("crates/x/a.rs"), &i));
        assert!(
            !f.matches(Path::new("loose/b.rs"), &i),
            "no ancestor manifest"
        );
    }

    #[test]
    fn from_spec_rejects_neither_field() {
        let err = ScopeFilter::from_spec(
            "r",
            ScopeFilterSpec {
                has_ancestor: None,
                changed_since: None,
                include_manifest_paths: None,
                exclude_manifest_paths: None,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("at least one"), "msg: {err}");
    }

    #[test]
    fn from_spec_accepts_changed_since_only() {
        let f = ScopeFilter::from_spec(
            "r",
            ScopeFilterSpec {
                has_ancestor: None,
                changed_since: Some("origin/main".into()),
                include_manifest_paths: None,
                exclude_manifest_paths: None,
            },
        )
        .unwrap();
        assert_eq!(f.changed_since(), Some("origin/main"));
        assert!(f.has_ancestor_names().is_empty());
    }

    // ── manifest-derived path scope (ADR-0010) ────────────────

    /// Build a filter from `scope_filter:` YAML (the real deserialise + validate
    /// path), so the manifest predicate is constructed exactly as production.
    fn manifest_filter(yaml: &str) -> ScopeFilter {
        let spec: ScopeFilterSpec = serde_yaml_ng::from_str(yaml).expect("scope_filter parses");
        ScopeFilter::from_spec("r", spec).expect("from_spec ok")
    }

    fn idx_with_manifest(paths: &[&str], key: &str, set: &[&str]) -> FileIndex {
        let i = idx(paths);
        let mut map = std::collections::HashMap::new();
        map.insert(key.to_string(), set.iter().map(PathBuf::from).collect());
        i.set_manifest_paths(map);
        i
    }

    #[test]
    fn exclude_manifest_paths_drops_files_in_set() {
        let f = manifest_filter(
            "exclude_manifest_paths:\n  source: package.json\n  extract: { json: \"$.bin.*\" }",
        );
        let key = f.manifest_predicates()[0].cache_key();
        let i = idx_with_manifest(&["src/cli.ts", "src/lib.ts"], key, &["src/cli.ts"]);
        assert!(
            !f.matches(Path::new("src/cli.ts"), &i),
            "in-set file dropped"
        );
        assert!(
            f.matches(Path::new("src/lib.ts"), &i),
            "out-of-set file kept"
        );
    }

    #[test]
    fn include_manifest_paths_keeps_only_files_in_set() {
        let f = manifest_filter(
            "include_manifest_paths:\n  source: Cargo.toml\n  extract: { toml: \"$.workspace.members[*]\" }",
        );
        let key = f.manifest_predicates()[0].cache_key();
        let i = idx_with_manifest(
            &["crates/a/lib.rs", "vendor/x.rs"],
            key,
            &["crates/a/lib.rs"],
        );
        assert!(
            f.matches(Path::new("crates/a/lib.rs"), &i),
            "in-set file kept"
        );
        assert!(
            !f.matches(Path::new("vendor/x.rs"), &i),
            "out-of-set file dropped"
        );
    }

    #[test]
    fn manifest_membership_is_directory_aware_prefix() {
        // A declared DIRECTORY (`workspace.members` -> `crates/a`) matches files
        // UNDER it; a shared-prefix sibling (`crates/ab`) does NOT match, because
        // membership is component-wise, not string-prefix.
        let f = manifest_filter(
            "include_manifest_paths:\n  source: Cargo.toml\n  extract: { toml: \"$.workspace.members[*]\" }",
        );
        let key = f.manifest_predicates()[0].cache_key();
        let i = idx_with_manifest(
            &["crates/a/lib.rs", "crates/ab/x.rs", "vendor/y.rs"],
            key,
            &["crates/a"],
        );
        assert!(
            f.matches(Path::new("crates/a/lib.rs"), &i),
            "file under declared dir kept"
        );
        assert!(
            !f.matches(Path::new("crates/ab/x.rs"), &i),
            "shared-prefix sibling dir not matched (component boundary)"
        );
        assert!(
            !f.matches(Path::new("vendor/y.rs"), &i),
            "file outside members dropped"
        );
    }

    #[test]
    fn manifest_absent_include_scopes_nothing_exclude_full() {
        // No cache entry (manifest unresolved) → include drops all, exclude keeps
        // all — consistent with has_ancestor's silent behaviour.
        let inc = manifest_filter(
            "include_manifest_paths:\n  source: p.json\n  extract: { json: \"$.x[*]\" }",
        );
        let exc = manifest_filter(
            "exclude_manifest_paths:\n  source: p.json\n  extract: { json: \"$.x[*]\" }",
        );
        let i = idx(&["a.rs"]); // no set_manifest_paths
        assert!(
            !inc.matches(Path::new("a.rs"), &i),
            "include + absent → nothing"
        );
        assert!(
            exc.matches(Path::new("a.rs"), &i),
            "exclude + absent → full scope"
        );
    }

    #[test]
    fn manifest_and_composes_with_has_ancestor() {
        let f = manifest_filter(
            "has_ancestor: Cargo.toml\nexclude_manifest_paths:\n  source: Cargo.toml\n  extract: { toml: \"$.bin[*].path\" }",
        );
        let key = f.manifest_predicates()[0].cache_key().to_string();
        let i = idx(&[
            "crates/x/Cargo.toml",
            "crates/x/a.rs",
            "crates/x/b.rs",
            "loose/c.rs",
        ]);
        let mut map = std::collections::HashMap::new();
        map.insert(key, [PathBuf::from("crates/x/b.rs")].into_iter().collect());
        i.set_manifest_paths(map);
        assert!(
            f.matches(Path::new("crates/x/a.rs"), &i),
            "under manifest, not excluded"
        );
        assert!(
            !f.matches(Path::new("crates/x/b.rs"), &i),
            "excluded by manifest"
        );
        assert!(
            !f.matches(Path::new("loose/c.rs"), &i),
            "no ancestor manifest"
        );
    }

    #[test]
    fn resolve_set_extracts_maps_and_confines() {
        let f = manifest_filter(
            "exclude_manifest_paths:\n  source: package.json\n  extract: { json: \"$.bin.*\" }\n  derive_target: { from: '^dist/(.*)\\.js$', to: 'src/$1.ts' }",
        );
        let pred = &f.manifest_predicates()[0];
        let text = r#"{ "bin": { "cli": "dist/cli.js", "helper": "dist/sub/helper.js" } }"#;
        let set = pred.resolve_set(text);
        assert!(
            set.contains(Path::new("src/cli.ts")),
            "mapped bin → src: {set:?}"
        );
        assert!(
            set.contains(Path::new("src/sub/helper.ts")),
            "nested mapped: {set:?}"
        );
    }

    #[test]
    fn resolve_set_drops_non_matching_derive_target_entries() {
        let f = manifest_filter(
            "exclude_manifest_paths:\n  source: package.json\n  extract: { json: \"$.bin.*\" }\n  derive_target: { from: '^dist/(.*)\\.js$', to: 'src/$1.ts' }",
        );
        let text = r#"{ "bin": { "sh": "scripts/run.sh" } }"#;
        assert!(
            f.manifest_predicates()[0].resolve_set(text).is_empty(),
            "an entry that doesn't match `from` is dropped"
        );
    }

    #[test]
    fn resolve_set_is_relative_to_manifest_dir() {
        let f = manifest_filter(
            "include_manifest_paths:\n  source: packages/foo/package.json\n  extract: { json: \"$.files[*]\" }",
        );
        let text = r#"{ "files": ["src/index.ts"] }"#;
        let set = f.manifest_predicates()[0].resolve_set(text);
        assert!(
            set.contains(Path::new("packages/foo/src/index.ts")),
            "declared path resolves under the manifest's own dir: {set:?}"
        );
    }

    #[test]
    fn resolve_set_confines_escaping_declared_paths() {
        let f = manifest_filter(
            "exclude_manifest_paths:\n  source: packages/foo/package.json\n  extract: { json: \"$.files[*]\" }",
        );
        // `../../etc/x` escapes the repo root once resolved under packages/foo/.
        let text = r#"{ "files": ["../../etc/x", "src/ok.ts"] }"#;
        let set = f.manifest_predicates()[0].resolve_set(text);
        assert!(set.contains(Path::new("packages/foo/src/ok.ts")));
        assert!(
            !set.iter().any(|p| p.starts_with("..")),
            "root-escaping declared path dropped: {set:?}"
        );
    }

    #[test]
    fn from_spec_accepts_manifest_only() {
        let f = manifest_filter(
            "include_manifest_paths:\n  source: package.json\n  extract: { json: \"$.bin.*\" }",
        );
        assert_eq!(f.manifest_predicates().len(), 1);
    }

    #[test]
    fn from_spec_rejects_source_escaping_root() {
        let spec: ScopeFilterSpec = serde_yaml_ng::from_str(
            "exclude_manifest_paths:\n  source: ../outside.json\n  extract: { json: \"$.x[*]\" }",
        )
        .unwrap();
        let err = ScopeFilter::from_spec("r", spec).unwrap_err();
        assert!(err.to_string().contains("repo-root-relative"), "msg: {err}");
    }

    #[test]
    fn shared_config_produces_one_cache_key() {
        // Two predicates with an identical (source, extract) config share a key,
        // so the engine resolves the manifest once.
        let a = manifest_filter(
            "exclude_manifest_paths:\n  source: package.json\n  extract: { json: \"$.bin.*\" }",
        );
        let b = manifest_filter(
            "exclude_manifest_paths:\n  source: package.json\n  extract: { json: \"$.bin.*\" }",
        );
        assert_eq!(
            a.manifest_predicates()[0].cache_key(),
            b.manifest_predicates()[0].cache_key()
        );
    }

    #[test]
    fn read_manifest_confined_reads_in_root_and_refuses_escape() {
        let root_dir = tempfile::tempdir().unwrap();
        let root = root_dir.path();
        std::fs::write(root.join("m.json"), r#"{"x":1}"#).unwrap();
        // A regular in-root manifest reads.
        assert_eq!(
            read_manifest_confined(root, Path::new("m.json")),
            r#"{"x":1}"#
        );
        // Absent -> "".
        assert!(read_manifest_confined(root, Path::new("missing.json")).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn read_manifest_confined_refuses_escaping_symlink() {
        let root_dir = tempfile::tempdir().unwrap();
        let outside_dir = tempfile::tempdir().unwrap(); // a different tree, not under root
        let root = root_dir.path();
        std::fs::write(outside_dir.path().join("secret.json"), "SECRET").unwrap();
        std::os::unix::fs::symlink(
            outside_dir.path().join("secret.json"),
            root.join("link.json"),
        )
        .unwrap();
        // The symlink resolves outside root -> refused (empty), no out-of-tree read.
        assert!(
            read_manifest_confined(root, Path::new("link.json")).is_empty(),
            "an escaping-symlink source must not be read"
        );
    }

    #[test]
    fn manifest_set_copies_onto_the_filtered_changed_index() {
        // The `--changed` fix: the engine resolves manifests against the full
        // index (where the manifest is reachable), then copies the resolved map
        // onto the fresh filtered index that per-file `matches` reads. Guards
        // that copy so an `include`/`exclude` predicate isn't silently empty
        // under `--changed`.
        let full = idx_with_manifest(&["a.rs"], "KEY", &["crates/a"]);
        let filtered = idx(&["a.rs"]); // a fresh filtered index, empty cache
        assert!(
            filtered.manifest_paths("KEY").is_none(),
            "filtered index starts with no manifest cache"
        );
        filtered.set_manifest_paths(full.manifest_paths_map().unwrap().clone());
        assert!(
            filtered
                .manifest_paths("KEY")
                .unwrap()
                .contains(Path::new("crates/a")),
            "the resolved set is visible on the filtered index after the copy"
        );
    }
}
