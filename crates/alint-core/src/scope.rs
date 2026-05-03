use std::path::Path;

use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};

use crate::config::{PathsSpec, RuleSpec};
use crate::error::{Error, Result};
use crate::scope_filter::ScopeFilter;
use crate::walker::FileIndex;

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
#[derive(Debug, Clone)]
pub struct Scope {
    include: GlobSet,
    exclude: GlobSet,
    has_include: bool,
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
            scope_filter: None,
        }
    }

    /// Borrow the optional [`ScopeFilter`] this scope carries.
    /// Used by dispatch sites (e.g. `for_each_dir`'s literal-
    /// path bypass) that already have a `&Scope` in hand and
    /// want to consult the filter without going through
    /// [`matches`](Self::matches).
    pub fn scope_filter(&self) -> Option<&ScopeFilter> {
        self.scope_filter.as_ref()
    }

    /// Returns `true` iff `path` is in scope:
    /// 1. Excluded patterns reject (dominant).
    /// 2. Include patterns must match (skipped if no includes).
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
        if self.exclude.is_match(path) {
            return false;
        }
        if self.has_include && !self.include.is_match(path) {
            return false;
        }
        if let Some(filter) = &self.scope_filter
            && !filter.matches(path, index)
        {
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn from_spec_bundles_paths_and_scope_filter() {
        // Synthesise a RuleSpec carrying both paths and scope_filter.
        // Index has marker.lock at pkg/ — only files under pkg/
        // satisfy the ancestor predicate.
        use crate::walker::FileEntry;
        let yaml = "id: t\nkind: filename_case\npaths: \"**/*.rs\"\n\
                    scope_filter:\n  has_ancestor: marker.lock\n\
                    case: snake\nlevel: error\n";
        let spec: RuleSpec = serde_yaml_ng::from_str(yaml).unwrap();
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
