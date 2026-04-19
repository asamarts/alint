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
