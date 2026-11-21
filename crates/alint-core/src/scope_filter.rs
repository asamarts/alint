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

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use regex::Regex;
use serde::{Deserialize, Deserializer};

use crate::error::{Error, Result};
use crate::extract::{Extract, ExtractSpec, NonStringValueKind, extract_values, is_non_literal};
use crate::pathsafe::{derive_target, normalize_confined};
use crate::rule::AnalysisFailureKind;
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
    /// The `{toml|json|yaml|xml|lines|regex}` extractor shared with
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
    /// footgun; set `false` for a rule that legitimately tolerates it. Ignored on
    /// `exclude_manifest_paths:` — an empty exclude set is safe (full scope,
    /// visible over-firing), so there is nothing to warn about.
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

/// A manifest-derived scope that could not produce a trustworthy membership
/// set. Cached beside the empty set so every active owning rule receives the
/// same typed analysis failure without rereading or reparsing the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManifestFailure {
    pub kind: AnalysisFailureKind,
    pub message: String,
}

impl ManifestFailure {
    fn read(message: impl Into<String>) -> Self {
        Self {
            kind: AnalysisFailureKind::ResourceRead,
            message: message.into(),
        }
    }

    fn parse(message: impl Into<String>) -> Self {
        Self {
            kind: AnalysisFailureKind::ResourceParse,
            message: message.into(),
        }
    }
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
        // The cache key NUL-delimits its components; a NUL byte in any of them
        // would break its injectivity (a double-quoted `"a\0b"` YAML scalar
        // decodes to a literal NUL). A NUL in a path / regex / template is
        // meaningless anyway — reject it so the delimiter stays sound.
        for (field, val) in [
            ("source", spec.source.as_str()),
            (
                "derive_target.from",
                spec.derive_target.as_ref().map_or("", |d| d.from.as_str()),
            ),
            (
                "derive_target.to",
                spec.derive_target.as_ref().map_or("", |d| d.to.as_str()),
            ),
        ] {
            if val.contains('\u{0}') {
                return Err(Error::rule_config(
                    rule_id,
                    format!("scope_filter manifest `{field}:` must not contain a NUL byte"),
                ));
            }
        }
        let source = normalize_confined(Path::new(&spec.source)).ok_or_else(|| {
            Error::rule_config(
                rule_id,
                format!(
                    "scope_filter manifest `source:` must be a repo-root-relative path, got {:?}",
                    spec.source
                ),
            )
        })?;
        if crate::pathsafe::path_names_git_directory(Path::new(&spec.source)) {
            return Err(Error::rule_config(
                rule_id,
                format!(
                    "scope_filter manifest `source` {}",
                    crate::pathsafe::git_directory_read_refusal(Path::new(&spec.source))
                ),
            ));
        }
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
            .map(|(from, to)| format!("{}\u{0}{to}", from.as_str()))
            .unwrap_or_default();
        // `Extract`'s Debug is a stable, injective rendering of the resolved
        // extractor. NUL (`\0`) delimits every component and is rejected above in
        // `source`/`from`/`to`, so the key is injective: two rules co-cache iff
        // their `(source, extract, derive_target)` is identical. (A plain `=>`
        // between `from`/`to` would let `{from:"a", to:"b=>c"}` and
        // `{from:"a=>b", to:"c"}` forge one key and share a wrong set.)
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

    pub(crate) fn redacts_by_default(&self) -> bool {
        self.extract.redacts_by_default()
            || crate::Format::detect_from_path(&self.source)
                .is_some_and(crate::Format::redacts_by_default)
    }

    /// Whether a file this predicate's set does (`in_set`) or does not hold
    /// stays in scope: an include keeps the set's files, an exclude drops
    /// them.
    pub(crate) fn keeps(&self, in_set: bool) -> bool {
        match self.sense {
            ManifestSense::Include => in_set,
            ManifestSense::Exclude => !in_set,
        }
    }

    /// True for an `include_manifest_paths:` predicate that wants a non-empty
    /// set — the engine warns when its resolved set is empty.
    pub(crate) fn warns_on_empty(&self) -> bool {
        self.sense == ManifestSense::Include && self.expect_nonempty
    }

    /// Resolve the declared path set from the manifest's text: extract, reject
    /// non-string values, drop non-literal strings, optionally
    /// `derive_target`-map, resolve relative to the manifest's directory, and
    /// confine each result to the repo root. Pure — the engine does the file read
    /// and caches either this set or the structural failure on the [`FileIndex`].
    /// The result is in DECLARED order: a workspace
    /// entry prefixed `!` (`!packages/internal` in a pnpm / npm `workspaces`
    /// list) is an EXCLUDE, resolved like any other entry and returned with a
    /// leading `!` (e.g. `!packages/internal`). [`ManifestSet::from_paths`]
    /// evaluates the entries in order, last match wins. Before, the `!` entry
    /// was resolved as a literal path that matched nothing, so the excluded
    /// package silently stayed in scope (audit 2026-10 finding 8).
    ///
    /// A repeated entry keeps only its LAST occurrence. Under last-match-wins an
    /// earlier copy can never decide (the later identical entry matches the
    /// same files and comes after it), so dropping it is exact; keeping the
    /// FIRST occurrence instead dropped a later re-include, and
    /// `["packages/*", "!packages/internal", "packages/*"]` wrongly excluded
    /// `packages/internal`.
    pub(crate) fn resolve_set(
        &self,
        text: &str,
    ) -> std::result::Result<Vec<PathBuf>, ManifestFailure> {
        let base = self.source.parent().unwrap_or_else(|| Path::new(""));
        let raw = extract_values(&self.extract, text).map_err(|error| {
            ManifestFailure::parse(format!("scope manifest extract failed: {error}"))
        })?;
        if !raw.non_strings.is_empty() {
            let count = raw.non_strings.len();
            let mut kinds = raw.non_strings;
            kinds.sort_unstable();
            kinds.dedup();
            let kinds = kinds
                .into_iter()
                .map(NonStringValueKind::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ManifestFailure::parse(format!(
                "scope manifest extract matched {count} non-string value(s) \
                 (types: {kinds}); path entries must be strings"
            )));
        }
        let resolved: Vec<PathBuf> = raw
            .strings
            .into_iter()
            .filter(|e| !is_non_literal(e))
            .filter_map(|entry| {
                let (negated, entry) = match entry.strip_prefix('!') {
                    Some(rest) => (true, rest.to_string()),
                    None => (false, entry),
                };
                // A non-matching derive_target entry is not a mapped output —
                // dropped, matching file_graph's derive_target.
                let mapped = if let Some((from, to)) = &self.derive_target {
                    derive_target(from, to, &entry)?
                } else {
                    PathBuf::from(entry)
                };
                let resolved = normalize_confined(&base.join(mapped))?;
                // Drop an entry that resolves to EXACTLY the manifest's own
                // directory (``, `.`, `x/..` — `base.join` collapses to `base`):
                // it would silently scope the manifest's WHOLE subtree, the footgun
                // the empty-set safety guards. A sibling (`../shared` ->
                // `packages/shared`) or a child resolves elsewhere and is kept — a
                // nested `tsconfig` referencing a sibling package via `../` is a
                // documented supported case. (At the repo root `base` is empty and
                // vacuous entries already normalize to `None` above, a no-op here.)
                if resolved.as_path() == base {
                    return None;
                }
                Some(if negated {
                    let mut s = std::ffi::OsString::from("!");
                    s.push(resolved.as_os_str());
                    PathBuf::from(s)
                } else {
                    resolved
                })
            })
            .collect();
        // Keep each entry's LAST occurrence (see above): walk backwards, then
        // restore declared order.
        let mut seen: HashSet<&PathBuf> = HashSet::new();
        let mut keep: Vec<&PathBuf> = resolved.iter().rev().filter(|p| seen.insert(*p)).collect();
        keep.reverse();
        Ok(keep.into_iter().cloned().collect())
    }
}

/// A manifest predicate's resolved membership set, cached on the [`FileIndex`]
/// (resolved once per run). Splits the declared entries into literal paths and
/// glob-pattern members, compiling the globs once:
///
/// - A **literal** entry (`crates/app`, `bin` -> `src/cli.ts`) uses the
///   directory-aware `starts_with` rule: it matches itself and every file under
///   it, respecting component boundaries.
/// - A **glob** entry (`crates/*`, the form a real `Cargo.toml` /
///   `package.json` workspace declares) matches a file when some component-prefix
///   of the file matches the glob — the glob analog of the literal rule, so
///   `members = ["crates/*"]` scopes every file under each matching `crates/<x>`.
///
/// A member prefixed `!` (from a negated workspace entry, see
/// [`ManifestPathSpec::resolve_set`]) EXCLUDES what it matches. When any member
/// is negated the members are evaluated in declared order and the LAST one that
/// matches the file decides (gitignore-style), so `["packages/*",
/// "!packages/internal"]` scopes every package except `packages/internal`.
#[derive(Debug, Clone, Default)]
pub struct ManifestSet {
    literals: HashSet<PathBuf>,
    globs: GlobSet,
    glob_patterns: Vec<String>,
    /// Set when any member is negated (see [`OrderedMembers`]). `None` keeps
    /// the unordered fast path.
    ordered: Option<OrderedMembers>,
}

/// The negation-aware form, indexed so a lookup costs O(path depth), not
/// O(members): a literal member matches a file iff it EQUALS one of the file's
/// component-prefixes (directory-aware `starts_with`), so literals live in a
/// hash map; glob members are one [`GlobSet`] queried per prefix. Each member
/// carries its declared position; the highest-positioned match decides (last
/// match wins). A linear scan over members was files x members x depth: a
/// 5,000-entry workspace with one `!` took ~27 s for 100k files (now ~45 ms).
///
/// A short list (the usual `["packages/*", "!packages/internal"]`) instead
/// scans its members from LAST to first and stops at the first match, which
/// is cheaper than probing every prefix of the file.
#[derive(Debug, Clone, Default)]
struct OrderedMembers {
    /// Set for a list of at most [`SMALL_ORDERED`] members: `(negated, member)`
    /// in declared order, scanned in reverse.
    small: Option<Vec<(bool, ManifestMember)>>,
    /// Literal member -> (declared position, negated). A repeated literal keeps
    /// its last position (the one that can decide).
    literals: HashMap<PathBuf, (usize, bool)>,
    /// Every glob member, in declared order.
    globs: GlobSet,
    /// Per `globs` index: (declared position, negated).
    glob_meta: Vec<(usize, bool)>,
}

/// Member count up to which [`OrderedMembers`] scans linearly.
const SMALL_ORDERED: usize = 8;

/// One compiled member of a short ordered list.
#[derive(Debug, Clone)]
enum ManifestMember {
    Literal(PathBuf),
    Glob(globset::GlobMatcher),
}

impl ManifestMember {
    /// Directory-aware match: a literal matches itself and everything under it;
    /// a glob matches when some component-prefix of `file` matches it.
    fn matches(&self, file: &Path) -> bool {
        match self {
            Self::Literal(d) => file.starts_with(d),
            Self::Glob(g) => {
                let mut prefix = PathBuf::new();
                file.components().any(|comp| {
                    prefix.push(comp);
                    g.is_match(&prefix)
                })
            }
        }
    }
}

impl OrderedMembers {
    fn contains_file(&self, file: &Path) -> bool {
        if let Some(small) = &self.small {
            // Last match wins: the first match scanning backwards decides.
            return small
                .iter()
                .rev()
                .find(|(_, m)| m.matches(file))
                .is_some_and(|(negated, _)| !negated);
        }
        let mut last: Option<(usize, bool)> = None;
        let mut consider = |m: (usize, bool)| {
            if last.is_none_or(|(pos, _)| m.0 > pos) {
                last = Some(m);
            }
        };
        let mut prefix = PathBuf::new();
        let mut hits: Vec<usize> = Vec::new();
        for comp in file.components() {
            prefix.push(comp);
            if let Some(&m) = self.literals.get(&prefix) {
                consider(m);
            }
            if !self.glob_meta.is_empty() {
                let candidate = globset::Candidate::new(&prefix);
                if self.globs.is_match_candidate(&candidate) {
                    self.globs.matches_candidate_into(&candidate, &mut hits);
                    for &i in &hits {
                        consider(self.glob_meta[i]);
                    }
                }
            }
        }
        last.is_some_and(|(_, negated)| !negated)
    }
}

/// An entry with a glob metacharacter (`*`, `?`, `[`, `{`) is a glob member.
fn is_glob_member(s: &str) -> bool {
    s.contains(['*', '?', '[', '{'])
}

fn compile_member(s: &str) -> Option<globset::Glob> {
    GlobBuilder::new(s).literal_separator(true).build().ok()
}

impl ManifestSet {
    /// Split a resolved path set (from [`ManifestPathSpec::resolve_set`]) into
    /// literals and glob members, compiling the globs once. An entry with a glob
    /// metacharacter (`*`, `?`, `[`, `{`) is a glob; any other entry is a literal
    /// path. `{` matters: `packages/{app,lib}` (a common npm/pnpm workspace form)
    /// is brace-alternation, which `globset` expands but a literal never would.
    /// `literal_separator(true)` keeps `*` within one component, like the rule
    /// `paths:` globs and Cargo's own member semantics. A malformed glob is
    /// dropped (the same fail-soft as an unparseable extract). A `!`-prefixed
    /// entry is a negated member (see the type docs); `paths` must then be in
    /// declared order.
    #[must_use]
    pub(crate) fn from_paths(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        let paths: Vec<PathBuf> = paths.into_iter().collect();
        if paths.iter().any(|p| p.to_string_lossy().starts_with('!')) {
            return Self::ordered(paths);
        }
        let mut literals = HashSet::new();
        let mut builder = GlobSetBuilder::new();
        let mut glob_patterns = Vec::new();
        for p in paths {
            let s = p.to_string_lossy();
            if is_glob_member(&s) {
                if let Some(glob) = compile_member(&s) {
                    builder.add(glob);
                    glob_patterns.push(s.into_owned());
                }
            } else {
                literals.insert(p);
            }
        }
        let globs = builder.build().unwrap_or_else(|_| GlobSet::empty());
        Self {
            literals,
            globs,
            glob_patterns,
            ordered: None,
        }
    }

    /// The negation-aware form: members indexed by declared position.
    fn ordered(paths: Vec<PathBuf>) -> Self {
        let mut ordered = OrderedMembers::default();
        let mut small = (paths.len() <= SMALL_ORDERED).then(Vec::new);
        let mut builder = GlobSetBuilder::new();
        let mut glob_patterns = Vec::new();
        let mut literals = HashSet::new();
        for (pos, p) in paths.into_iter().enumerate() {
            let s = p.to_string_lossy().into_owned();
            let (negated, body) = match s.strip_prefix('!') {
                Some(rest) => (true, rest.to_string()),
                None => (false, s),
            };
            if is_glob_member(&body) {
                let Some(glob) = compile_member(&body) else {
                    continue;
                };
                if let Some(small) = &mut small {
                    small.push((negated, ManifestMember::Glob(glob.compile_matcher())));
                }
                builder.add(glob);
                ordered.glob_meta.push((pos, negated));
                if !negated {
                    glob_patterns.push(body);
                }
            } else {
                if !negated {
                    literals.insert(PathBuf::from(&body));
                }
                if let Some(small) = &mut small {
                    small.push((negated, ManifestMember::Literal(PathBuf::from(&body))));
                }
                ordered.literals.insert(PathBuf::from(body), (pos, negated));
            }
        }
        ordered.small = small;
        ordered.globs = builder.build().unwrap_or_else(|_| {
            ordered.glob_meta.clear();
            GlobSet::empty()
        });
        // `literals` / `glob_patterns` keep only the POSITIVE members, so
        // `is_empty` stays "declares nothing to include"; matching uses `ordered`.
        Self {
            literals,
            globs: GlobSet::empty(),
            glob_patterns,
            ordered: Some(ordered),
        }
    }

    /// Empty when it holds neither a literal path nor a glob member — the state
    /// the `expect_nonempty` warning guards. A glob member counts as non-empty
    /// even before it matches any file (it is a real, resolvable scope). Negated
    /// members alone include nothing, so they do not count.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.literals.is_empty() && self.glob_patterns.is_empty()
    }

    /// True if `file` is in the declared set: under some literal declared path, or
    /// with a component-prefix matching some glob member (directory-aware). With
    /// negated members, the last matching member in declared order decides.
    #[must_use]
    pub(crate) fn contains_file(&self, file: &Path) -> bool {
        if let Some(ordered) = &self.ordered {
            return ordered.contains_file(file);
        }
        // A literal matches iff it equals one of `file`'s component-prefixes
        // (`starts_with`): a hash probe per prefix, not a scan of the literals.
        let mut prefix = PathBuf::new();
        for comp in file.components() {
            prefix.push(comp);
            if self.literals.contains(&prefix)
                || (!self.glob_patterns.is_empty() && self.globs.is_match(&prefix))
            {
                return true;
            }
        }
        false
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
    /// The resolved, confined declared paths: sorted, or in declared order when a
    /// negated (`!`-prefixed) member is present (order is then the semantics).
    pub paths: Vec<PathBuf>,
}

/// Read one manifest-derived scope source while preserving why it could not be
/// read. The canonical target must remain under the canonical lint root
/// (ADR-0004), only regular files are accepted, and the normal analysis cap is
/// enforced by the bounded reader.
pub(crate) fn read_manifest_confined(
    root: &Path,
    rel: &Path,
) -> std::result::Result<String, ManifestFailure> {
    let abs = root.join(rel);
    let croot = root.canonicalize().map_err(|error| {
        ManifestFailure::read(format!("lint root could not be resolved: {error}"))
    })?;
    let cabs = abs.canonicalize().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ManifestFailure::read("scope manifest is missing")
        } else {
            ManifestFailure::read(format!("scope manifest could not be resolved: {error}"))
        }
    })?;
    if !cabs.starts_with(&croot) {
        return Err(ManifestFailure::read(
            "scope manifest resolves outside the lint root",
        ));
    }
    // A `source` naming a Git directory is refused at load; this one reaches
    // one through a symlink, and a file there can hold credentials. Both paths
    // are resolved already.
    if crate::pathsafe::path_names_git_directory(rel)
        || crate::pathsafe::in_git_directory(&croot, &cabs)
    {
        return Err(ManifestFailure::read(
            "scope manifest is in a Git directory; rules never read there",
        ));
    }
    let meta = std::fs::metadata(&cabs).map_err(|error| {
        ManifestFailure::read(format!(
            "scope manifest metadata could not be read: {error}"
        ))
    })?;
    if !meta.is_file() {
        return Err(ManifestFailure::read(
            "scope manifest is not a regular file",
        ));
    }
    let bytes = match crate::walker::read_for_analysis(&cabs, meta.len()) {
        crate::walker::AnalysisRead::Bytes(bytes) => bytes,
        crate::walker::AnalysisRead::Missing => {
            return Err(ManifestFailure::read("scope manifest is missing"));
        }
        crate::walker::AnalysisRead::TooLarge(size) => {
            return Err(ManifestFailure::read(format!(
                "scope manifest is too large to analyze ({size} bytes)"
            )));
        }
        crate::walker::AnalysisRead::Unreadable(error) => {
            return Err(ManifestFailure::read(format!(
                "scope manifest could not be read: {error}"
            )));
        }
    };
    String::from_utf8(bytes)
        .map_err(|_| ManifestFailure::parse("scope manifest is not valid UTF-8"))
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

    pub(crate) fn sensitive_manifest_sources(&self) -> impl Iterator<Item = &Path> {
        self.manifest_predicates
            .iter()
            .filter(|predicate| predicate.redacts_by_default())
            .map(ManifestPredicate::source)
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
        if !self.matches_without_manifest(file, index) {
            return false;
        }
        for pred in &self.manifest_predicates {
            // The engine resolves the set ONCE and stores it on the index.
            // A missing cache entry behaves as empty only defensively; active
            // engine runs route a cached resolution failure before matching.
            let in_set = index
                .manifest_paths(pred.cache_key())
                .is_some_and(|set| set.contains_file(file));
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

    /// Match every filter predicate except manifest-derived membership.
    ///
    /// Changed-mode dispatch uses this only when manifest resolution failed: a
    /// path rejected here proves the rule is invocation-inactive without
    /// consulting the unavailable set; a path accepted here keeps the rule
    /// active so the engine can surface its cached analysis failure.
    #[inline]
    pub(crate) fn matches_without_manifest(&self, file: &Path, index: &FileIndex) -> bool {
        if !self.has_ancestor.is_empty() && !self.ancestor_matches(file, index) {
            return false;
        }
        if let Some(since) = &self.changed_since {
            let Some(changed) = index.changed_paths(since) else {
                return false;
            };
            if !changed.contains(file) {
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
    /// display by `alint explain`. A failed source or extract is displayed as an
    /// empty set; `check` and `fix` surface the corresponding structural failure.
    /// Not the hot path — the engine caches these on the [`FileIndex`] for
    /// [`matches`](Self::matches).
    #[must_use]
    pub fn resolved_manifest_sets(&self, root: &Path) -> Vec<ResolvedManifestScope> {
        self.manifest_predicates
            .iter()
            .map(|p| {
                let mut paths = read_manifest_confined(root, &p.source)
                    .and_then(|text| p.resolve_set(&text))
                    .unwrap_or_default();
                // Sorted for a stable display, unless a negated (`!`) member is
                // present: then declared order IS the semantics (last match
                // wins), so it is shown as declared.
                if !paths.iter().any(|p| p.to_string_lossy().starts_with('!')) {
                    paths.sort();
                }
                if p.redacts_by_default() {
                    let source = p.source.to_string_lossy().replace('\\', "/");
                    paths = paths
                        .into_iter()
                        .enumerate()
                        .map(|(index, _)| {
                            PathBuf::from(crate::redaction_marker(&format!(
                                "manifest_scope\0{source}\0{:?}\0{index}",
                                p.extract
                            )))
                        })
                        .collect();
                }
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
                // The marker is about the repository, not the iterated files: on a
                // `--changed`-filtered index an unchanged `Cargo.toml` must still
                // count, or the rule never fires in changed mode (v0.18.3).
                if index.contains_file_in_tree(&candidate) {
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
                 instead - see docs/design/v0.9/scope-filter.md for the pattern."
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
                "scope_filter.has_ancestor name {name:?} must be a basename - no path separators.\n  \
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
                "scope_filter.has_ancestor name {name:?} must be a literal - no glob \
                 metacharacters allowed (use `Cargo.toml`, not `*.toml`)"
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "scope_filter_tests.rs"]
mod tests;
