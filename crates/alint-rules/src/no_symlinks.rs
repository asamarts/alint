//! `no_symlinks` — flag tracked paths that are symbolic links.
//!
//! Symlinks create portability headaches: Windows NTFS needs
//! admin rights to create them, git-for-Windows may turn them
//! into flat files, and CI systems vary. Repos that are
//! checked out across platforms usually want them banned.
//!
//! Fixable via `file_remove`, which deletes the symlink.

use alint_core::{
    Context, Error, FixSpec, Fixer, Level, Result, Rule, RuleSpec, Scope, ScopeFilter, Violation,
};

use crate::fixers::FileRemoveFixer;

#[derive(Debug)]
pub struct NoSymlinksRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    scope_filter: Option<ScopeFilter>,
    fixer: Option<FileRemoveFixer>,
}

impl Rule for NoSymlinksRule {
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
            let full = ctx.root.join(&entry.path);
            let Ok(meta) = std::fs::symlink_metadata(&full) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                let msg = self
                    .message
                    .clone()
                    .unwrap_or_else(|| "path is a symbolic link".to_string());
                violations.push(Violation::new(msg).with_path(entry.path.clone()));
            }
        }
        Ok(violations)
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }

    fn scope_filter(&self) -> Option<&ScopeFilter> {
        self.scope_filter.as_ref()
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let paths = spec
        .paths
        .as_ref()
        .ok_or_else(|| Error::rule_config(&spec.id, "no_symlinks requires a `paths` field"))?;
    let fixer = match &spec.fix {
        Some(FixSpec::FileRemove { .. }) => Some(FileRemoveFixer),
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!("fix.{} is not compatible with no_symlinks", other.op_name()),
            ));
        }
        None => None,
    };
    Ok(Box::new(NoSymlinksRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        scope_filter: spec.parse_scope_filter()?,
        fixer,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, spec_yaml, tempdir_with_files};

    #[test]
    fn build_rejects_missing_paths_field() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_symlinks\n\
             level: warning\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn build_accepts_file_remove_fix() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_symlinks\n\
             paths: \"**/*\"\n\
             level: warning\n\
             fix:\n  \
               file_remove: {}\n",
        );
        let rule = build(&spec).unwrap();
        assert!(rule.fixer().is_some());
    }

    #[test]
    fn build_rejects_incompatible_fix() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_symlinks\n\
             paths: \"**/*\"\n\
             level: warning\n\
             fix:\n  \
               file_create:\n    \
                 content: \"x\"\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn evaluate_passes_on_regular_files() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_symlinks\n\
             paths: \"**/*\"\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("a.txt", b"hi")]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn evaluate_fires_on_symlink() {
        use std::os::unix::fs::symlink;
        let spec = spec_yaml(
            "id: t\n\
             kind: no_symlinks\n\
             paths: \"**/*\"\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, mut idx) = tempdir_with_files(&[("real.txt", b"target")]);
        // Add a symlink pointing to real.txt; index it manually
        // (tempdir_with_files doesn't create symlinks).
        symlink(tmp.path().join("real.txt"), tmp.path().join("link.txt")).unwrap();
        idx.entries.push(alint_core::FileEntry {
            path: std::path::Path::new("link.txt").into(),
            is_dir: false,
            size: 0,
        });
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "symlink should fire: {v:?}");
    }

    #[cfg(unix)]
    #[test]
    fn scope_filter_narrows() {
        use std::os::unix::fs::symlink;
        // Two symlinks; only the one inside a directory with
        // `marker.lock` as ancestor should fire.
        let spec = spec_yaml(
            "id: t\n\
             kind: no_symlinks\n\
             paths: \"**/*\"\n\
             scope_filter:\n  \
               has_ancestor: marker.lock\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, mut idx) = tempdir_with_files(&[
            ("pkg/marker.lock", b""),
            ("pkg/real.txt", b"target"),
            ("other/real.txt", b"target"),
        ]);
        std::fs::create_dir_all(tmp.path().join("pkg")).unwrap();
        std::fs::create_dir_all(tmp.path().join("other")).unwrap();
        symlink(tmp.path().join("pkg/real.txt"), tmp.path().join("pkg/link")).unwrap();
        symlink(
            tmp.path().join("other/real.txt"),
            tmp.path().join("other/link"),
        )
        .unwrap();
        idx.entries.push(alint_core::FileEntry {
            path: std::path::Path::new("pkg/link").into(),
            is_dir: false,
            size: 0,
        });
        idx.entries.push(alint_core::FileEntry {
            path: std::path::Path::new("other/link").into(),
            is_dir: false,
            size: 0,
        });
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "only in-scope symlink should fire: {v:?}");
        assert_eq!(v[0].path.as_deref(), Some(std::path::Path::new("pkg/link")));
    }
}
