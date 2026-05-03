//! `file_min_size` — files in scope must be at least `min_bytes` bytes.
//!
//! Symmetric counterpart of [`file_max_size`](crate::file_max_size).
//! The primary use case is "my README is more than a stub" /
//! "my LICENSE text is not empty": a non-zero minimum size
//! catches placeholder files that pass existence checks but add
//! no information.
//!
//! Empty files are a likely separate problem; if you want to
//! explicitly forbid those, use
//! [`no_empty_files`](crate::no_empty_files) — `file_min_size:
//! 1` works too but the dedicated rule carries clearer intent.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, ScopeFilter, Violation};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Options {
    min_bytes: u64,
}

#[derive(Debug)]
pub struct FileMinSizeRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    scope_filter: Option<ScopeFilter>,
    min_bytes: u64,
}

impl Rule for FileMinSizeRule {
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
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path) {
                continue;
            }
            if let Some(filter) = &self.scope_filter
                && !filter.matches(&entry.path, ctx.index)
            {
                continue;
            }
            if entry.size < self.min_bytes {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "file below {} byte(s) (actual: {})",
                        self.min_bytes, entry.size,
                    )
                });
                violations.push(Violation::new(msg).with_path(entry.path.clone()));
            }
        }
        Ok(violations)
    }

    fn scope_filter(&self) -> Option<&ScopeFilter> {
        self.scope_filter.as_ref()
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "file_min_size requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    Ok(Box::new(FileMinSizeRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        scope_filter: spec.parse_scope_filter()?,
        min_bytes: opts.min_bytes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, spec_yaml};
    use alint_core::{FileEntry, FileIndex};
    use std::path::Path;

    fn idx_with_size(path: &str, size: u64) -> FileIndex {
        FileIndex::from_entries(vec![FileEntry {
            path: std::path::Path::new(path).into(),
            is_dir: false,
            size,
        }])
    }

    #[test]
    fn build_rejects_missing_paths_field() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_min_size\n\
             min_bytes: 100\n\
             level: warning\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn evaluate_passes_when_size_above_minimum() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_min_size\n\
             paths: \"README.md\"\n\
             min_bytes: 100\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let idx = idx_with_size("README.md", 1024);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn evaluate_fires_when_size_below_minimum() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_min_size\n\
             paths: \"README.md\"\n\
             min_bytes: 100\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let idx = idx_with_size("README.md", 10);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn evaluate_size_at_exact_minimum_passes() {
        // Boundary: `entry.size < min_bytes` is strict, so a
        // file at exactly min_bytes passes.
        let spec = spec_yaml(
            "id: t\n\
             kind: file_min_size\n\
             paths: \"a.bin\"\n\
             min_bytes: 100\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let idx = idx_with_size("a.bin", 100);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert!(v.is_empty(), "size == min should pass: {v:?}");
    }

    #[test]
    fn evaluate_zero_byte_file_fires_when_minimum_positive() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_min_size\n\
             paths: \"empty.txt\"\n\
             min_bytes: 1\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let idx = idx_with_size("empty.txt", 0);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn scope_filter_narrows() {
        // Two undersized files; only the one inside a directory
        // with `marker.lock` as ancestor should fire.
        let spec = spec_yaml(
            "id: t\n\
             kind: file_min_size\n\
             paths: \"**/*.txt\"\n\
             min_bytes: 100\n\
             scope_filter:\n  \
               has_ancestor: marker.lock\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let idx = FileIndex::from_entries(vec![
            FileEntry {
                path: std::path::Path::new("pkg/marker.lock").into(),
                is_dir: false,
                size: 1,
            },
            FileEntry {
                path: std::path::Path::new("pkg/small.txt").into(),
                is_dir: false,
                size: 5,
            },
            FileEntry {
                path: std::path::Path::new("other/small.txt").into(),
                is_dir: false,
                size: 5,
            },
        ]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 1, "only in-scope file should fire: {v:?}");
        assert_eq!(v[0].path.as_deref(), Some(Path::new("pkg/small.txt")));
    }
}
