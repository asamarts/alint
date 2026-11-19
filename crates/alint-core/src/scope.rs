use std::path::Path;

use globset::{Candidate, Glob, GlobBuilder, GlobSet, GlobSetBuilder};

use crate::config::{PathsSpec, RuleSpec};
use crate::error::{Error, Result};
use crate::scope_filter::ScopeFilter;
use crate::walker::{FileEntry, FileIndex};

/// Compiled include/exclude matcher built from a [`PathsSpec`] or raw pattern list,
/// optionally bundled with a [`ScopeFilter`] ancestor-manifest gate.
///
/// Patterns prefixed with `!` are treated as excludes when passed as a flat list.
/// Paths are matched relative to the repository root. Globs are compiled with
/// `literal_separator(true)` — i.e., Git-style semantics where `*` never
/// crosses a path separator. `**` is required to descend into subdirectories.
///
/// The optional `scope_filter` is the v0.9.6 [`ScopeFilter`] gate (e.g.
/// `has_ancestor: Cargo.toml`). v0.9.10 moved it into `Scope` so
/// `matches(&Path, &FileIndex)` honours it on every call automatically —
/// the v0.9.6/v0.9.7/v0.9.9 silent-no-op bug class can no longer recur.
///
/// **Empty / exclude-only patterns are match-all (fail-open).** With no
/// *include* pattern, the include check is skipped, so every non-excluded
/// path matches. This is by design for the exclude-only idiom
/// (`paths: ["!vendor/**"]` = "everything except vendor"), but it also means
/// an explicit `paths: []` (no includes *and* no excludes) applies the rule to
/// the **whole tree**, not to nothing — a foot-gun if `[]` was meant as "off."
/// Prefer omitting `paths` for match-all, `level: off` to disable a rule, and
/// real patterns otherwise. (L14: a load-time warning for the truly-empty case
/// is a tracked follow-up — it needs an absent-vs-`[]` signal at the spec layer
/// and a load-warning channel the loader doesn't yet expose, and must not
/// false-warn the intentional exclude-only idiom.)
#[derive(Debug, Clone)]
pub struct Scope {
    include: GlobSet,
    exclude: GlobSet,
    has_include: bool,
    /// A sole literal include pattern, matched by comparing the path's bytes
    /// instead of through `include` (see [`Scope::from_patterns`]).
    literal: Option<Box<[u8]>>,
    /// The directories every path the include patterns can match lies under,
    /// one per include ([`literal_dir_prefix`]): only the entries at and below
    /// them can match ([`Scope::candidates`]). `None` when an include starts
    /// with no literal directory, or there is no include.
    within: Option<Box<[Box<Path>]>>,
    // The field name `scope_filter` is intentional — the public
    // accessor and the spec field share it, so renaming to
    // `filter` would cost more clarity than it saves.
    #[allow(clippy::struct_field_names)]
    scope_filter: Option<ScopeFilter>,
}

fn compile(pattern: &str) -> Result<Glob> {
    GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .map_err(|source| Error::Glob {
            pattern: pattern.to_string(),
            source,
        })
}

impl Scope {
    pub fn from_patterns(patterns: &[String]) -> Result<Self> {
        // A single literal path (what a nested rule's `{path}/README.md` renders
        // to, once per matched entry) needs no `GlobSet`: compiling one was most
        // of a nested rule's build. globset matches a literal by exact equality
        // on the candidate path (separators normalized to `/`), which
        // `literal_matches` reproduces. The literal test is the shared one the
        // rules' own literal fast paths use.
        if let [only] = patterns
            && let Some(literal) = literal_pattern(only, std::path::is_separator('\\'))
        {
            return Ok(Self {
                include: GlobSet::empty(),
                exclude: GlobSet::empty(),
                has_include: true,
                literal: Some(literal.into_bytes().into_boxed_slice()),
                within: include_dirs(patterns),
                scope_filter: None,
            });
        }
        let mut include = GlobSetBuilder::new();
        let mut exclude = GlobSetBuilder::new();
        let mut has_include = false;
        for pattern in patterns {
            if let Some(rest) = pattern.strip_prefix('!') {
                exclude.add(compile(rest)?);
            } else {
                include.add(compile(pattern)?);
                has_include = true;
            }
        }
        Ok(Self {
            include: include.build().map_err(|source| Error::Glob {
                pattern: patterns.join(","),
                source,
            })?,
            exclude: exclude.build().map_err(|source| Error::Glob {
                pattern: patterns.join(","),
                source,
            })?,
            has_include,
            literal: None,
            within: include_dirs(patterns),
            scope_filter: None,
        })
    }

    pub fn from_paths_spec(spec: &PathsSpec) -> Result<Self> {
        match spec {
            PathsSpec::Single(s) => Self::from_patterns(std::slice::from_ref(s)),
            PathsSpec::Many(v) => Self::from_patterns(v),
            PathsSpec::IncludeExclude { include, exclude } => {
                let mut combined = include.clone();
                for e in exclude {
                    combined.push(format!("!{e}"));
                }
                Self::from_patterns(&combined)
            }
        }
    }

    /// Build a `Scope` from a [`RuleSpec`] — bundles the rule's
    /// `paths:` (or match-all if absent) AND its `scope_filter:`
    /// into a single value. This is the canonical constructor
    /// for rule builders since v0.9.10; preferring it over
    /// `from_paths_spec` is what compile-enforces every rule to
    /// honour `scope_filter` (the v0.9.6/.7/.9 silent-no-op
    /// bug class).
    pub fn from_spec(spec: &RuleSpec) -> Result<Self> {
        let mut scope = match &spec.paths {
            Some(p) => Self::from_paths_spec(p)?,
            None => Self::match_all(),
        };
        scope.scope_filter = spec.parse_scope_filter()?;
        Ok(scope)
    }

    /// Match-all scope (used when no `paths` is configured on a rule).
    pub fn match_all() -> Self {
        let mut include = GlobSetBuilder::new();
        include.add(compile("**").expect("`**` must compile"));
        Self {
            include: include.build().expect("`**` GlobSet must build"),
            exclude: GlobSet::empty(),
            has_include: true,
            literal: None,
            within: None,
            scope_filter: None,
        }
    }

    /// This scope with `filter` as its `scope_filter:`: a relation's selector
    /// compiles its `paths:` and `scope_filter:` into one scope, as a rule's
    /// [`from_spec`](Self::from_spec) does.
    pub(crate) fn with_scope_filter(mut self, filter: ScopeFilter) -> Self {
        self.scope_filter = Some(filter);
        self
    }

    /// Borrow the optional [`ScopeFilter`] this scope carries.
    /// Used by dispatch sites (e.g. `for_each_dir`'s literal-
    /// path bypass) that already have a `&Scope` in hand and
    /// want to consult the filter without going through
    /// [`matches`](Self::matches).
    pub fn scope_filter(&self) -> Option<&ScopeFilter> {
        self.scope_filter.as_ref()
    }

    /// Secret-bearing manifest files read by this scope's membership predicates.
    /// Used by staged-fix redaction so a passing predicate cannot leak unchanged
    /// manifest bytes through an unrelated composed edit.
    #[doc(hidden)]
    pub fn sensitive_manifest_sources(&self) -> impl Iterator<Item = &Path> {
        self.scope_filter
            .iter()
            .flat_map(ScopeFilter::sensitive_manifest_sources)
    }

    /// Returns `true` iff `path` is in scope:
    /// 1. Excluded patterns reject (dominant).
    /// 2. Include patterns must match — **skipped if there are no includes**,
    ///    so empty / exclude-only `paths` is match-all (see the type-level note).
    /// 3. `scope_filter` (if any) must match.
    ///
    /// The `index` argument is the engine's [`FileIndex`] —
    /// required because `scope_filter` may need to walk
    /// ancestors looking for a manifest (e.g.
    /// `has_ancestor: Cargo.toml`). Callers that don't have a
    /// `scope_filter` on this scope still pass it; the cost is
    /// a single `Option::is_none` branch.
    ///
    /// `#[inline]` is load-bearing — this method runs on every
    /// (rule, file) pair in the per-file dispatch hot loop.
    /// Without it, cross-crate calls from `alint-rules` rules'
    /// `evaluate` bodies don't inline through `thin` LTO and the
    /// `Option<ScopeFilter>` None-branch becomes a non-inlined
    /// function call (~40 % slowdown on S6 10k vs v0.9.9
    /// without the hint).
    #[inline]
    pub fn matches(&self, path: &Path, index: &FileIndex) -> bool {
        #[cfg(debug_assertions)]
        MATCH_TESTS.with(|count| count.set(count.get() + 1));
        if !self.path_matches(path) {
            return false;
        }
        if let Some(filter) = &self.scope_filter
            && !filter.matches(path, index)
        {
            return false;
        }
        true
    }

    /// Match path globs and every non-manifest filter predicate.
    ///
    /// This is a changed-mode failure-routing primitive, not a second policy
    /// matcher: callers use it only when the manifest set is unavailable.
    #[inline]
    pub(crate) fn matches_without_manifest(&self, path: &Path, index: &FileIndex) -> bool {
        if !self.path_matches(path) {
            return false;
        }
        self.scope_filter
            .as_ref()
            .is_none_or(|filter| filter.matches_without_manifest(path, index))
    }

    /// Whether a path at or below the directory `dir` (root-relative, empty
    /// for the root) can match this scope's path patterns, judged from the
    /// patterns alone: a sole literal include only when it lies below `dir`, and
    /// includes that each start with a literal directory
    /// ([`literal_dir_prefix`]) only when `dir` and one of those lie one below
    /// the other. `true` when the patterns cannot tell. The `scope_filter`
    /// is not consulted.
    pub(crate) fn may_match_below(&self, dir: &Path) -> bool {
        if let Some(literal) = &self.literal {
            return std::str::from_utf8(literal)
                .map_or(true, |literal| Path::new(literal).starts_with(dir));
        }
        self.within.as_ref().is_none_or(|dirs| {
            dirs.iter()
                .any(|within| within.starts_with(dir) || dir.starts_with(within))
        })
    }

    /// The entries of `index`, directories and files, that this scope can
    /// match, in index order: those at and below the directories its include
    /// patterns start with (`locales/*.json`, or a nested rule's
    /// `{path}/*.md` once rendered), or every entry when a pattern starts with
    /// no literal directory or the index is not in path order. The caller
    /// still tests each one with [`Self::matches`]: an entry left out can never
    /// match, so the two select what testing every entry selects, in the same
    /// order. A rule built once per iteration of a nested `require:` block
    /// then reads its own directory rather than the whole tree (an O(D x N)
    /// run over D iterations and N entries otherwise).
    pub fn candidates<'i>(
        &self,
        index: &'i FileIndex,
    ) -> impl Iterator<Item = &'i FileEntry> + use<'i> {
        self.candidate_ranges(index)
            .into_iter()
            .flat_map(move |range| &index.entries[range])
    }

    /// The files among [`Self::candidates`].
    pub fn candidate_files<'i>(
        &self,
        index: &'i FileIndex,
    ) -> impl Iterator<Item = &'i FileEntry> + use<'i> {
        self.candidates(index).filter(|entry| !entry.is_dir)
    }

    /// The directories among [`Self::candidates`].
    pub fn candidate_dirs<'i>(
        &self,
        index: &'i FileIndex,
    ) -> impl Iterator<Item = &'i FileEntry> + use<'i> {
        self.candidates(index).filter(|entry| entry.is_dir)
    }

    /// The positions in [`FileIndex::entries`] of [`Self::candidates`]:
    /// ascending, disjoint ranges (one directory may lie below another).
    pub(crate) fn candidate_ranges(&self, index: &FileIndex) -> Vec<std::ops::Range<usize>> {
        let every_entry = || std::iter::once(0..index.entries.len()).collect();
        let Some(dirs) = &self.within else {
            return every_entry();
        };
        let mut ranges = Vec::with_capacity(dirs.len());
        for dir in dirs {
            let Some(range) = index.subtree(dir) else {
                return every_entry();
            };
            if !range.is_empty() {
                ranges.push(range);
            }
        }
        ranges.sort_unstable_by_key(|range| range.start);
        let mut merged: Vec<std::ops::Range<usize>> = Vec::with_capacity(ranges.len());
        for range in ranges {
            match merged.last_mut() {
                Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
                _ => merged.push(range),
            }
        }
        merged
    }
}

#[cfg(debug_assertions)]
std::thread_local! {
    /// How many paths [`Scope::matches`] tested on this thread.
    static MATCH_TESTS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How many paths [`Scope::matches`] has tested on the calling thread, in a
/// build with debug assertions (`None` without them, where nothing counts).
/// Test only: a rule that tests every entry of the index once per iteration
/// of a nested `require:` block gives the same findings as one that tests
/// only its candidates ([`Scope::candidates`]), and only this count tells the
/// two apart.
#[doc(hidden)]
#[must_use]
pub fn scope_match_count() -> Option<u64> {
    #[cfg(debug_assertions)]
    {
        Some(MATCH_TESTS.with(std::cell::Cell::get))
    }
    #[cfg(not(debug_assertions))]
    {
        None
    }
}

/// The directory each include pattern of `patterns` starts with
/// ([`literal_dir_prefix`]), or `None` when one starts with none or every
/// pattern is an exclude (a match-all scope).
fn include_dirs(patterns: &[String]) -> Option<Box<[Box<Path>]>> {
    let mut dirs = Vec::new();
    for pattern in patterns.iter().filter(|pattern| !pattern.starts_with('!')) {
        dirs.push(Box::from(Path::new(&literal_dir_prefix(pattern)?)));
    }
    (!dirs.is_empty()).then(|| dirs.into_boxed_slice())
}

impl Scope {
    /// The include/exclude pattern check alone.
    ///
    /// Both sets are checked against one prepared [`Candidate`]:
    /// `GlobSet::is_match` prepares a fresh one (path normalization, basename
    /// and extension) on every call, even for an empty `exclude`. Calling the
    /// out-of-line `is_match_candidate` also keeps this body small enough that
    /// callers inline it whole, which matters in the per-file dispatch loop.
    #[inline]
    fn path_matches(&self, path: &Path) -> bool {
        if let Some(literal) = &self.literal {
            return literal_matches(literal, path);
        }
        let candidate = Candidate::new(path);
        !self.exclude.is_match_candidate(&candidate)
            && (!self.has_include || self.include.is_match_candidate(&candidate))
    }
}

/// The literal path `pattern` matches, as globset would compile it, or `None`
/// to use a `GlobSet`. Where `\\` is a path separator (Windows) globset rewrites
/// every `\\` in a pattern to `/`, except inside a character class, so the
/// literal is normalized the same way and a pattern with a class is left to
/// the `GlobSet`.
fn literal_pattern(pattern: &str, backslash_is_separator: bool) -> Option<String> {
    if backslash_is_separator && pattern.contains('[') {
        return None;
    }
    let literal = crate::template::literal_glob_path(pattern)?;
    Some(if backslash_is_separator {
        literal.replace('\\', "/")
    } else {
        literal
    })
}

/// The directory every path `pattern` can match lies under, as globset
/// compiles the pattern: the segments before its last `/` for as long as each
/// is a literal name ([`literal_pattern`]), `a/b` for `a/b/*.json` or
/// `a/b/**`, `app/[slug]` for `app/[[]slug[]]/*.tsx`. A match spells these
/// names first, so it is at or below them, component by component. `None`
/// when the first segment is not a literal name (`**/x`, `*/x`, `{a,b}/x`,
/// `[ab]/x`), for a pattern with no `/` or one that starts with `/` or `.`,
/// and for an exclude (`!x`). A segment that names nothing on its own (`.`,
/// `..`, empty) ends the prefix, since globset compares it as text. Where `\\`
/// is a path separator (Windows) it separates segments as `/` does, so a
/// pattern built from a `Path` (`pkg\\d000\\m*`) narrows to `pkg/d000`.
pub(crate) fn literal_dir_prefix(pattern: &str) -> Option<String> {
    literal_dir_prefix_with(pattern, std::path::is_separator('\\'))
}

fn literal_dir_prefix_with(pattern: &str, backslash_is_separator: bool) -> Option<String> {
    if pattern.starts_with('!') {
        return None;
    }
    let separates = |c: char| c == '/' || (backslash_is_separator && c == '\\');
    let (dirs, _) = pattern.rsplit_once(separates)?;
    let mut prefix = String::new();
    for segment in dirs.split(separates) {
        let Some(name) = literal_pattern(segment, backslash_is_separator) else {
            break;
        };
        // A one-character class can spell a separator (`[/]`).
        if name.split('/').any(|part| matches!(part, "" | "." | "..")) {
            break;
        }
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(&name);
    }
    (!prefix.is_empty()).then_some(prefix)
}

/// globset's literal match: the path's bytes (lossy UTF-8 with every separator
/// normalized to `/` off Unix) equal the literal.
#[inline]
fn literal_matches(literal: &[u8], path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        path.as_os_str().as_bytes() == literal
    }
    #[cfg(not(unix))]
    {
        let lossy = path.to_string_lossy();
        lossy.len() == literal.len()
            && lossy
                .bytes()
                .map(|b| {
                    if std::path::is_separator(char::from(b)) {
                        b'/'
                    } else {
                        b
                    }
                })
                .eq(literal.iter().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_pattern_normalizes_like_globset_where_backslash_separates() {
        // Where `\\` separates, globset keeps it literal inside a class: such a
        // pattern is not a literal path.
        assert_eq!(literal_pattern("a[\\]b", true), None);
        assert_eq!(literal_pattern("app/[[]slug[]]", true), None);
        assert_eq!(literal_pattern("src/*.rs", false), None);
        if std::path::is_separator('\\') {
            // A nested `{path}/README.md` rendered on Windows.
            assert_eq!(
                literal_pattern("packages\\a/README.md", true).as_deref(),
                Some("packages/a/README.md")
            );
        } else {
            // Unix: `\\` escapes and a one-character class is a literal.
            assert_eq!(literal_pattern("a\\*b", false).as_deref(), Some("a*b"));
            assert_eq!(
                literal_pattern("app/[[]slug[]]", false).as_deref(),
                Some("app/[slug]")
            );
        }
    }

    /// Where `\\` separates, a pattern built from a `Path` on Windows (a
    /// `registry_paths_resolve` entry joined to its registry's directory)
    /// narrows as its `/` spelling does; elsewhere `\\` escapes.
    #[test]
    fn a_backslash_separates_dir_prefix_segments_where_it_is_a_separator() {
        for (pattern, prefix) in [
            ("pkg\\d000\\m*", Some("pkg/d000")),
            ("pkg\\d000/m*.txt", Some("pkg/d000")),
            ("pkg/d000\\m*", Some("pkg/d000")),
            ("pkg\\*\\m", Some("pkg")),
            ("pkg/x[\\]y", Some("pkg")),
            ("pkg\\..\\m*", Some("pkg")),
            ("\\pkg\\m*", None),
            ("a[\\]b/c", None),
            ("m*", None),
        ] {
            assert_eq!(
                literal_dir_prefix_with(pattern, true).as_deref(),
                prefix,
                "{pattern}"
            );
        }
        assert_eq!(literal_dir_prefix_with("pkg\\d000\\m*", false), None);
        assert_eq!(
            literal_dir_prefix_with("pkg/d000\\m*", false).as_deref(),
            Some("pkg")
        );
    }

    /// Every pattern up to length 3 over the glob alphabet, against every
    /// candidate path up to length 3: a scope must match exactly what a
    /// `GlobSet` of the same pattern matches (and fail to build exactly when
    /// the glob does).
    #[test]
    fn literal_fast_path_matches_like_a_globset() {
        let patterns = short_strings(&['a', '/', '.', '*', '?', '[', ']', '{', '}', '\\', '!'], 3);
        let paths = short_strings(&['a', '/', '.', '\\', '*', '['], 3);
        let index = FileIndex::default();
        let mut literal_scopes = 0;
        for pattern in &patterns {
            let reference = compile(pattern).and_then(|glob| {
                let mut set = GlobSetBuilder::new();
                set.add(glob);
                set.build().map_err(|source| Error::Glob {
                    pattern: pattern.clone(),
                    source,
                })
            });
            let scope = Scope::from_patterns(std::slice::from_ref(pattern));
            if pattern.starts_with('!') {
                continue; // an exclude-only scope, not the single-literal shape
            }
            match (&scope, &reference) {
                (Ok(scope), Ok(set)) => {
                    literal_scopes += usize::from(scope.literal.is_some());
                    for path in &paths {
                        assert_eq!(
                            scope.matches(Path::new(path), &index),
                            set.is_match(Path::new(path)),
                            "pattern {pattern:?} path {path:?}"
                        );
                    }
                }
                (Err(_), Err(_)) => {}
                _ => panic!("pattern {pattern:?}: build outcome differs"),
            }
        }
        assert!(literal_scopes > 100, "{literal_scopes}");
    }

    /// Every string up to `max` characters over `alphabet`, the empty one
    /// included.
    fn short_strings(alphabet: &[char], max: usize) -> Vec<String> {
        let mut out = vec![String::new()];
        let mut frontier = vec![String::new()];
        for _ in 0..max {
            let next: Vec<String> = frontier
                .iter()
                .flat_map(|s| alphabet.iter().map(move |c| format!("{s}{c}")))
                .collect();
            out.extend(next.iter().cloned());
            frontier = next;
        }
        out
    }

    /// Include and exclude sets checked against one shared candidate match
    /// exactly what separate `GlobSet::is_match` calls on the path match, for
    /// every pair of globs up to length 2 over every path up to length 3.
    #[test]
    fn shared_candidate_matches_like_separate_glob_sets() {
        let set = |pattern: &str| {
            let mut set = GlobSetBuilder::new();
            set.add(compile(pattern).ok()?);
            set.build().ok()
        };
        let globs: Vec<(String, GlobSet)> = short_strings(&['a', '/', '.', '*', '?'], 2)
            .into_iter()
            .filter_map(|glob| set(&glob).map(|set| (glob, set)))
            .collect();
        let paths = short_strings(&['a', '/', '.', 'b'], 3);
        let index = FileIndex::default();
        let mut checked = 0;
        for (include, include_set) in &globs {
            for (exclude, exclude_set) in &globs {
                let scope = Scope::from_patterns(&[include.clone(), format!("!{exclude}")])
                    .expect("both globs compile");
                for path in &paths {
                    let path = Path::new(path);
                    assert_eq!(
                        scope.matches(path, &index),
                        include_set.is_match(path) && !exclude_set.is_match(path),
                        "include {include:?} exclude {exclude:?} path {path:?}"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 50_000, "{checked}");
    }

    fn s(patterns: &[&str]) -> Scope {
        Scope::from_patterns(
            &patterns
                .iter()
                .map(|p| (*p).to_string())
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }

    /// Empty file index — sufficient for path-glob-only tests
    /// where no `scope_filter` ancestor walk happens.
    fn empty_index() -> FileIndex {
        FileIndex::from_entries(Vec::new())
    }

    #[test]
    fn star_does_not_cross_path_separator() {
        // Git-style semantics — `*` never matches `/`.
        let scope = s(&["src/*.rs"]);
        let idx = empty_index();
        assert!(scope.matches(Path::new("src/main.rs"), &idx));
        assert!(!scope.matches(Path::new("src/sub/main.rs"), &idx));
    }

    #[test]
    fn double_star_descends_into_subdirectories() {
        let scope = s(&["src/**/*.rs"]);
        let idx = empty_index();
        assert!(scope.matches(Path::new("src/main.rs"), &idx));
        assert!(scope.matches(Path::new("src/sub/main.rs"), &idx));
        assert!(scope.matches(Path::new("src/a/b/c/d.rs"), &idx));
    }

    #[test]
    fn excludes_apply_before_includes() {
        // A path matched by both include and exclude is
        // excluded — exclusion is the dominant operation.
        let scope = s(&["src/**/*.rs", "!src/**/test_*.rs"]);
        let idx = empty_index();
        assert!(scope.matches(Path::new("src/main.rs"), &idx));
        assert!(!scope.matches(Path::new("src/test_widget.rs"), &idx));
        assert!(!scope.matches(Path::new("src/sub/test_thing.rs"), &idx));
    }

    #[test]
    fn empty_pattern_list_matches_nothing() {
        // No includes and no excludes → has_include is false
        // (empty GlobSet) and exclude is empty. `matches` falls
        // through to `has_include` → true (match-all). Caller
        // is expected to use Scope::match_all() explicitly.
        // Verifying actual behaviour rather than asserting an
        // implicit assumption.
        let scope = Scope::from_patterns(&[]).unwrap();
        let idx = empty_index();
        assert!(
            scope.matches(Path::new("anything"), &idx),
            "empty pattern list yields match-all (no excludes, no includes → has_include=false → matches)",
        );
    }

    #[test]
    fn match_all_helper_matches_every_path() {
        let scope = Scope::match_all();
        let idx = empty_index();
        assert!(scope.matches(Path::new("a"), &idx));
        assert!(scope.matches(Path::new("a/b/c.rs"), &idx));
        assert!(scope.matches(Path::new("deeply/nested/path/with.ext"), &idx));
    }

    #[test]
    fn from_paths_spec_handles_single_string() {
        let scope = Scope::from_paths_spec(&PathsSpec::Single("src/**/*.rs".into())).unwrap();
        let idx = empty_index();
        assert!(scope.matches(Path::new("src/main.rs"), &idx));
        assert!(!scope.matches(Path::new("docs/intro.md"), &idx));
    }

    #[test]
    fn from_paths_spec_handles_many_strings() {
        let scope = Scope::from_paths_spec(&PathsSpec::Many(vec![
            "src/**/*.rs".into(),
            "Cargo.toml".into(),
        ]))
        .unwrap();
        let idx = empty_index();
        assert!(scope.matches(Path::new("src/main.rs"), &idx));
        assert!(scope.matches(Path::new("Cargo.toml"), &idx));
        assert!(!scope.matches(Path::new("docs/intro.md"), &idx));
    }

    #[test]
    fn from_paths_spec_handles_include_exclude_form() {
        let scope = Scope::from_paths_spec(&PathsSpec::IncludeExclude {
            include: vec!["src/**/*.rs".into()],
            exclude: vec!["src/**/test_*.rs".into()],
        })
        .unwrap();
        let idx = empty_index();
        assert!(scope.matches(Path::new("src/main.rs"), &idx));
        assert!(!scope.matches(Path::new("src/test_x.rs"), &idx));
    }

    #[test]
    fn invalid_glob_surfaces_clear_error() {
        let err = Scope::from_patterns(&["[unterminated".into()]).unwrap_err();
        let s = err.to_string();
        assert!(s.contains("[unterminated"), "missing pattern: {s}");
    }

    #[test]
    fn brace_expansion_works() {
        let scope = s(&["src/**/*.{rs,toml}"]);
        let idx = empty_index();
        assert!(scope.matches(Path::new("src/main.rs"), &idx));
        assert!(scope.matches(Path::new("src/Cargo.toml"), &idx));
        assert!(!scope.matches(Path::new("src/README.md"), &idx));
    }

    /// A scope's candidates are the entries under each include's literal
    /// directory, merged in index order (a directory below another, a
    /// pattern given twice), and every entry when an include starts with no
    /// literal directory, when only excludes are given, for the match-all
    /// scope, and for an index out of path order. Testing only them selects
    /// what testing every entry selects (ENG-4 of the second post-v0.22.1
    /// audit: a nested rule's glob scanned the whole index per iteration).
    #[test]
    fn candidates_cover_every_include_and_select_what_a_full_scan_selects() {
        use crate::walker::FileEntry;
        const ALL: &[&str] = &[
            "a", "a/b", "a/b/y.md", "a/c.md", "a/x.md", "b", "b/z.md", "c.md", "d", "d/w.md",
        ];
        let index = |sorted: bool| {
            let mut paths = ALL.to_vec();
            if !sorted {
                paths.reverse();
            }
            FileIndex::from_entries(
                paths
                    .into_iter()
                    .map(|path| FileEntry {
                        path: Path::new(path).into(),
                        is_dir: !path.contains('.'),
                        size: 1,
                    })
                    .collect(),
            )
        };
        let a: &[&str] = &["a", "a/b", "a/b/y.md", "a/c.md", "a/x.md"];
        for (patterns, candidates) in [
            (&["a/*.md"][..], a),
            (&["a/b/*.md", "a/*.md"][..], a),
            (&["a/*.md", "a/*.md"][..], a),
            (
                &["d/*.md", "a/b/*.md"][..],
                &["a/b", "a/b/y.md", "d", "d/w.md"][..],
            ),
            (&["a/*.md", "!a/c.md"][..], a),
            (&["a/c.md"][..], a),
            (&["missing/*.md"][..], &[][..]),
            (&["a/*.md", "*.md"][..], ALL),
            (&["a/*.md", "**/*.md"][..], ALL),
            (&["!a/*.md"][..], ALL),
            (&[][..], ALL),
        ] {
            let scope = s(patterns);
            for sorted in [true, false] {
                let index = index(sorted);
                let tested: Vec<&str> = scope
                    .candidates(&index)
                    .map(|entry| entry.path.to_str().unwrap())
                    .collect();
                let mut expected = if sorted {
                    candidates.to_vec()
                } else {
                    ALL.to_vec()
                };
                if !sorted {
                    expected.reverse();
                }
                assert_eq!(tested, expected, "{patterns:?} sorted: {sorted}");
                let matching = |entries: &mut dyn Iterator<Item = &FileEntry>| -> Vec<String> {
                    entries
                        .filter(|entry| scope.matches(&entry.path, &index))
                        .map(|entry| entry.path.display().to_string())
                        .collect()
                };
                assert_eq!(
                    matching(&mut scope.candidates(&index)),
                    matching(&mut index.entries.iter()),
                    "{patterns:?} sorted: {sorted}"
                );
                assert!(scope.candidate_files(&index).all(|entry| !entry.is_dir));
                assert!(scope.candidate_dirs(&index).all(|entry| entry.is_dir));
                assert_eq!(
                    scope.candidate_files(&index).count() + scope.candidate_dirs(&index).count(),
                    tested.len()
                );
            }
        }
        assert_eq!(
            Scope::match_all().candidates(&index(true)).count(),
            ALL.len()
        );
    }

    #[test]
    fn from_spec_bundles_paths_and_scope_filter() {
        // Synthesise a RuleSpec carrying both paths and scope_filter.
        // Index has marker.lock at pkg/ — only files under pkg/
        // satisfy the ancestor predicate.
        use crate::walker::FileEntry;
        let yaml = "id: t\nkind: filename_case\npaths: \"**/*.rs\"\n\
                    scope_filter:\n  has_ancestor: marker.lock\n\
                    case: snake\nlevel: error\n";
        let spec: RuleSpec = crate::yaml::from_str(yaml).unwrap();
        let scope = Scope::from_spec(&spec).unwrap();
        let entries = vec![
            FileEntry {
                path: Path::new("pkg/marker.lock").into(),
                is_dir: false,
                size: 1,
            },
            FileEntry {
                path: Path::new("pkg/in_scope.rs").into(),
                is_dir: false,
                size: 1,
            },
            FileEntry {
                path: Path::new("other/out_of_scope.rs").into(),
                is_dir: false,
                size: 1,
            },
        ];
        let idx = FileIndex::from_entries(entries);
        // Path glob matches both .rs files; scope_filter narrows
        // to only the one under pkg/ (marker.lock ancestor).
        assert!(scope.matches(Path::new("pkg/in_scope.rs"), &idx));
        assert!(!scope.matches(Path::new("other/out_of_scope.rs"), &idx));
        assert!(scope.scope_filter().is_some(), "filter should be wired");
    }
}
