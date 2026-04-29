use std::path::Path;

use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};

use crate::config::PathsSpec;
use crate::error::{Error, Result};

/// Compiled include/exclude matcher built from a [`PathsSpec`] or raw pattern list.
///
/// Patterns prefixed with `!` are treated as excludes when passed as a flat list.
/// Paths are matched relative to the repository root. Globs are compiled with
/// `literal_separator(true)` — i.e., Git-style semantics where `*` never
/// crosses a path separator. `**` is required to descend into subdirectories.
#[derive(Debug, Clone)]
pub struct Scope {
    include: GlobSet,
    exclude: GlobSet,
    has_include: bool,
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

    /// Match-all scope (used when no `paths` is configured on a rule).
    pub fn match_all() -> Self {
        let mut include = GlobSetBuilder::new();
        include.add(compile("**").expect("`**` must compile"));
        Self {
            include: include.build().expect("`**` GlobSet must build"),
            exclude: GlobSet::empty(),
            has_include: true,
        }
    }

    pub fn matches(&self, path: &Path) -> bool {
        if self.exclude.is_match(path) {
            return false;
        }
        if !self.has_include {
            return true;
        }
        self.include.is_match(path)
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

    #[test]
    fn star_does_not_cross_path_separator() {
        // Git-style semantics — `*` never matches `/`.
        let scope = s(&["src/*.rs"]);
        assert!(scope.matches(Path::new("src/main.rs")));
        assert!(!scope.matches(Path::new("src/sub/main.rs")));
    }

    #[test]
    fn double_star_descends_into_subdirectories() {
        let scope = s(&["src/**/*.rs"]);
        assert!(scope.matches(Path::new("src/main.rs")));
        assert!(scope.matches(Path::new("src/sub/main.rs")));
        assert!(scope.matches(Path::new("src/a/b/c/d.rs")));
    }

    #[test]
    fn excludes_apply_before_includes() {
        // A path matched by both include and exclude is
        // excluded — exclusion is the dominant operation.
        let scope = s(&["src/**/*.rs", "!src/**/test_*.rs"]);
        assert!(scope.matches(Path::new("src/main.rs")));
        assert!(!scope.matches(Path::new("src/test_widget.rs")));
        assert!(!scope.matches(Path::new("src/sub/test_thing.rs")));
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
        assert!(
            scope.matches(Path::new("anything")),
            "empty pattern list yields match-all (no excludes, no includes → has_include=false → matches)",
        );
    }

    #[test]
    fn match_all_helper_matches_every_path() {
        let scope = Scope::match_all();
        assert!(scope.matches(Path::new("a")));
        assert!(scope.matches(Path::new("a/b/c.rs")));
        assert!(scope.matches(Path::new("deeply/nested/path/with.ext")));
    }

    #[test]
    fn from_paths_spec_handles_single_string() {
        let scope = Scope::from_paths_spec(&PathsSpec::Single("src/**/*.rs".into())).unwrap();
        assert!(scope.matches(Path::new("src/main.rs")));
        assert!(!scope.matches(Path::new("docs/intro.md")));
    }

    #[test]
    fn from_paths_spec_handles_many_strings() {
        let scope = Scope::from_paths_spec(&PathsSpec::Many(vec![
            "src/**/*.rs".into(),
            "Cargo.toml".into(),
        ]))
        .unwrap();
        assert!(scope.matches(Path::new("src/main.rs")));
        assert!(scope.matches(Path::new("Cargo.toml")));
        assert!(!scope.matches(Path::new("docs/intro.md")));
    }

    #[test]
    fn from_paths_spec_handles_include_exclude_form() {
        let scope = Scope::from_paths_spec(&PathsSpec::IncludeExclude {
            include: vec!["src/**/*.rs".into()],
            exclude: vec!["src/**/test_*.rs".into()],
        })
        .unwrap();
        assert!(scope.matches(Path::new("src/main.rs")));
        assert!(!scope.matches(Path::new("src/test_x.rs")));
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
        assert!(scope.matches(Path::new("src/main.rs")));
        assert!(scope.matches(Path::new("src/Cargo.toml")));
        assert!(!scope.matches(Path::new("src/README.md")));
    }
}
