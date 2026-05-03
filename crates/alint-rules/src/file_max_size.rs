//! `file_max_size` — files in scope must be at most `max_bytes` bytes.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Options {
    max_bytes: u64,
}

#[derive(Debug)]
pub struct FileMaxSizeRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    max_bytes: u64,
}

impl Rule for FileMaxSizeRule {
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
            if !self.scope.matches(&entry.path, ctx.index) {
                continue;
            }
            if entry.size > self.max_bytes {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "file exceeds {} byte(s) (actual: {})",
                        self.max_bytes, entry.size
                    )
                });
                violations.push(Violation::new(msg).with_path(entry.path.clone()));
            }
        }
        Ok(violations)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(_paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "file_max_size requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    Ok(Box::new(FileMaxSizeRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        max_bytes: opts.max_bytes,
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
             kind: file_max_size\n\
             max_bytes: 1000\n\
             level: warning\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn build_rejects_missing_max_bytes() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_max_size\n\
             paths: \"**/*\"\n\
             level: warning\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn evaluate_passes_when_size_under_limit() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_max_size\n\
             paths: \"**/*\"\n\
             max_bytes: 100\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let idx = idx_with_size("a.bin", 50);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn evaluate_fires_when_size_over_limit() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_max_size\n\
             paths: \"**/*\"\n\
             max_bytes: 100\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let idx = idx_with_size("big.bin", 1024);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn evaluate_size_at_exact_limit_passes() {
        // Boundary: `entry.size > max_bytes` is strict, so a
        // file at exactly max_bytes is OK.
        let spec = spec_yaml(
            "id: t\n\
             kind: file_max_size\n\
             paths: \"**/*\"\n\
             max_bytes: 100\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let idx = idx_with_size("a.bin", 100);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert!(v.is_empty(), "size == max should pass: {v:?}");
    }

    #[test]
    fn scope_filter_narrows() {
        // Two oversize files; only the one inside a directory
        // with `marker.lock` as ancestor should fire.
        let spec = spec_yaml(
            "id: t\n\
             kind: file_max_size\n\
             paths: \"**/*.bin\"\n\
             max_bytes: 100\n\
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
                path: std::path::Path::new("pkg/big.bin").into(),
                is_dir: false,
                size: 1024,
            },
            FileEntry {
                path: std::path::Path::new("other/big.bin").into(),
                is_dir: false,
                size: 1024,
            },
        ]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 1, "only in-scope file should fire: {v:?}");
        assert_eq!(v[0].path.as_deref(), Some(Path::new("pkg/big.bin")));
    }
}
