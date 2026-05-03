//! `max_files_per_directory` — cap how many files may live
//! directly under any directory in scope (non-recursive).
//!
//! Useful for monorepos that want to force sharding — "no more
//! than 200 files in `packages/`", etc. Reports one violation
//! per overlong directory, with the overflow count.

use std::collections::BTreeMap;
use std::path::PathBuf;

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    max_files: usize,
}

#[derive(Debug)]
pub struct MaxFilesPerDirectoryRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    max_files: usize,
}

impl Rule for MaxFilesPerDirectoryRule {
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
        // Group files by their immediate parent directory.
        let mut counts: BTreeMap<PathBuf, usize> = BTreeMap::new();
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path, ctx.index) {
                continue;
            }
            let parent = entry
                .path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            *counts.entry(parent).or_insert(0) += 1;
        }
        let mut violations = Vec::new();
        for (dir, count) in counts {
            if count > self.max_files {
                let pretty = if dir.as_os_str().is_empty() {
                    "<repo root>".to_string()
                } else {
                    dir.display().to_string()
                };
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!("{pretty} has {count} files; max is {}", self.max_files)
                });
                violations.push(Violation::new(msg).with_path(dir.clone()));
            }
        }
        Ok(violations)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let _paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(
            &spec.id,
            "max_files_per_directory requires a `paths` field (often `\"**\"`)",
        )
    })?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.max_files == 0 {
        return Err(Error::rule_config(
            &spec.id,
            "max_files_per_directory `max_files` must be > 0",
        ));
    }
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "max_files_per_directory has no fix op — file relocation is a human decision",
        ));
    }
    Ok(Box::new(MaxFilesPerDirectoryRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        max_files: opts.max_files,
    }))
}

// `Path::to_path_buf` is required by the grouping above.
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, index, spec_yaml};

    #[test]
    fn build_rejects_missing_paths_field() {
        let spec = spec_yaml(
            "id: t\n\
             kind: max_files_per_directory\n\
             max_files: 100\n\
             level: warning\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn build_rejects_zero_max() {
        let spec = spec_yaml(
            "id: t\n\
             kind: max_files_per_directory\n\
             paths: \"**\"\n\
             max_files: 0\n\
             level: warning\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn build_rejects_fix_block() {
        let spec = spec_yaml(
            "id: t\n\
             kind: max_files_per_directory\n\
             paths: \"**\"\n\
             max_files: 100\n\
             level: warning\n\
             fix:\n  \
               file_remove: {}\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn evaluate_passes_under_limit() {
        let spec = spec_yaml(
            "id: t\n\
             kind: max_files_per_directory\n\
             paths: \"**\"\n\
             max_files: 5\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["a/1.rs", "a/2.rs", "a/3.rs"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn evaluate_fires_on_over_limit_directory() {
        let spec = spec_yaml(
            "id: t\n\
             kind: max_files_per_directory\n\
             paths: \"**\"\n\
             max_files: 2\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["a/1.rs", "a/2.rs", "a/3.rs", "b/1.rs"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 1, "only `a/` exceeds: {v:?}");
    }

    #[test]
    fn evaluate_groups_by_immediate_parent() {
        // Files in `a/` and files in `a/b/` count toward
        // separate directory totals.
        let spec = spec_yaml(
            "id: t\n\
             kind: max_files_per_directory\n\
             paths: \"**\"\n\
             max_files: 2\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["a/1.rs", "a/2.rs", "a/b/1.rs", "a/b/2.rs"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert!(v.is_empty(), "neither dir exceeds: {v:?}");
    }

    #[test]
    fn scope_filter_narrows() {
        // `pkg/` and `other/` each hold 3 files; only `pkg/`
        // has the `marker.lock` ancestor, so its files count
        // toward the cap and `pkg/` fires; `other/` is silently
        // excluded.
        let spec = spec_yaml(
            "id: t\n\
             kind: max_files_per_directory\n\
             paths: \"**/*.rs\"\n\
             max_files: 2\n\
             scope_filter:\n  \
               has_ancestor: marker.lock\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&[
            "pkg/marker.lock",
            "pkg/1.rs",
            "pkg/2.rs",
            "pkg/3.rs",
            "other/1.rs",
            "other/2.rs",
            "other/3.rs",
        ]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 1, "only `pkg/` should fire: {v:?}");
        assert!(
            v[0].path.as_deref().is_some_and(|p| p == Path::new("pkg")),
            "unexpected path: {:?}",
            v[0].path
        );
    }
}
