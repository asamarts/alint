//! `no_symlinks` — flag tracked paths that are symbolic links.
//!
//! Symlinks create portability headaches: Windows NTFS needs
//! admin rights to create them, git-for-Windows may turn them
//! into flat files, and CI systems vary. Repos that are
//! checked out across platforms usually want them banned.
//!
//! Fixable via `file_remove`, which deletes the symlink.

use alint_core::{Context, Error, FixSpec, Fixer, Level, Result, Rule, RuleSpec, Scope, Violation};

use crate::fixers::FileRemoveFixer;

#[derive(Debug)]
pub struct NoSymlinksRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    fixer: Option<FileRemoveFixer>,
}

impl Rule for NoSymlinksRule {
    /// Expose the per-file scope so the engine resolves this rule's
    /// `scope_filter` (manifest sets, `changed_since:`) before dispatch and
    /// can `--changed`-skip it (see `Rule::path_scope`).
    fn path_scope(&self) -> Option<&Scope> {
        Some(&self.scope)
    }

    fn scope_matches_any(&self, index: &alint_core::FileIndex) -> bool {
        self.scope
            .candidates(index)
            .map(|entry| entry.path.as_ref())
            .chain(index.escaping_symlinks().iter().map(AsRef::as_ref))
            .any(|path| self.scope.matches(path, index))
    }

    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        // Iterate ALL indexed entries, not just `files()`: a symlink whose
        // target is a *directory* is indexed as a dir entry (the walk follows
        // it), so a `files()`-only scan silently missed dir symlinks (M4). The
        // per-entry `symlink_metadata` re-stat below is what actually decides —
        // a regular directory is never flagged.
        for entry in self.scope.candidates(ctx.index) {
            if !self.scope.matches(&entry.path, ctx.index) {
                continue;
            }
            let full = ctx.root.join(&entry.path);
            let meta = match std::fs::symlink_metadata(&full) {
                Ok(m) => m,
                Err(e) => {
                    violations.push(crate::io::io_error_violation(&entry.path, &e));
                    continue;
                }
            };
            if meta.file_type().is_symlink() {
                let msg = self
                    .message
                    .clone()
                    .unwrap_or_else(|| "path is a symbolic link".to_string());
                violations.push(Violation::new(msg).with_path(entry.path.clone()));
            }
        }
        // Symlinks whose target ESCAPES the repo root are pruned from the index
        // by the walker (so no content rule can read out-of-tree through them),
        // but recorded on the side-list — flag them here too (M4). They are
        // symlinks by construction, so no re-stat is needed.
        for link in ctx.index.escaping_symlinks() {
            if !self.scope.matches(link, ctx.index) {
                continue;
            }
            let msg = self
                .message
                .clone()
                .unwrap_or_else(|| "path is a symbolic link".to_string());
            violations.push(Violation::new(msg).with_path(link.clone()));
        }
        Ok(violations)
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }
}

pub static META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::RuleMajorScoped,
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::Metadata,
    formats: std::borrow::Cow::Borrowed(&[]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[std::borrow::Cow::Borrowed("file_remove")]),
    expect_matches: true,
    trace: alint_core::TraceMeta::SELECTION,
    nested_rules: false,
};

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let _paths = spec
        .paths
        .as_ref()
        .ok_or_else(|| Error::rule_config(&spec.id, "no_symlinks requires a `paths` field"))?;
    let fixer = match &spec.fix {
        Some(FixSpec::FileRemove { file_remove }) => Some(FileRemoveFixer::new(
            file_remove
                .applicability
                .unwrap_or(alint_core::Applicability::Unsafe),
        )),
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
        scope: Scope::from_spec(spec)?,
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
    fn evaluate_fires_on_directory_symlink_via_real_walk() {
        // M4: a symlink whose target is a *directory* is indexed as a dir
        // entry by the real walk, so a `files()`-only scan missed it. Uses the
        // real walker (not a hand-built index) to prove the dir symlink is both
        // indexed and flagged.
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join("realdir")).unwrap();
        std::fs::write(root.join("realdir/f.txt"), b"hi").unwrap();
        symlink(root.join("realdir"), root.join("linkdir")).unwrap();

        let idx = alint_core::walk(root, &crate::test_support::walk_options()).unwrap();
        assert!(
            idx.entries
                .iter()
                .any(|e| &*e.path == std::path::Path::new("linkdir")),
            "the dir symlink must be indexed as an entry"
        );

        let spec = spec_yaml(
            "id: t\n\
             kind: no_symlinks\n\
             paths: \"**/*\"\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let v = rule.evaluate(&ctx(root, &idx)).unwrap();
        // Exactly one violation, on the symlink itself — proving BOTH that the
        // dir symlink fires AND that the regular `realdir` and the descended
        // `linkdir/f.txt` (also in the index) are NOT flagged.
        assert_eq!(v.len(), 1, "only the dir symlink should fire: {v:?}");
        assert_eq!(
            v[0].path.as_deref(),
            Some(std::path::Path::new("linkdir")),
            "the flagged path must be the symlink"
        );
    }

    #[cfg(unix)]
    #[test]
    fn evaluate_fires_on_escaping_symlink_via_side_list() {
        // M4: a symlink whose target ESCAPES the repo root is pruned from the
        // index (so no content rule reads through it) but recorded on the
        // side-list — the real walk + no_symlinks must still flag it.
        use std::os::unix::fs::symlink;
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), b"TOPSECRET").unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("real.txt"), b"in-tree").unwrap();
        symlink(outside.path().join("secret.txt"), root.join("escape")).unwrap();

        let idx = alint_core::walk(root, &crate::test_support::walk_options()).unwrap();
        assert!(
            !idx.entries
                .iter()
                .any(|e| &*e.path == std::path::Path::new("escape")),
            "the escaping symlink must be PRUNED from entries (confinement)"
        );

        let spec = spec_yaml(
            "id: t\n\
             kind: no_symlinks\n\
             paths: \"**/*\"\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let v = rule.evaluate(&ctx(root, &idx)).unwrap();
        // Only the escaping symlink fires: real.txt isn't a symlink and `escape`
        // isn't in entries — so the single fire must come from the side-list.
        assert_eq!(v.len(), 1, "only the escaping symlink should fire: {v:?}");
        assert_eq!(
            v[0].path.as_deref(),
            Some(std::path::Path::new("escape")),
            "the flagged path must be the escaping symlink"
        );
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

    #[cfg(unix)]
    #[test]
    fn changed_mode_flags_escaping_dangling_and_cyclic_symlinks() {
        use std::collections::HashSet;
        use std::os::unix::fs::symlink;
        use std::path::PathBuf;

        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), b"x").unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("ok.txt"), b"ok").unwrap();
        symlink(outside.path().join("secret.txt"), root.join("esc")).unwrap();
        symlink("nonexistent", root.join("dangling")).unwrap();
        symlink("self_loop", root.join("self_loop")).unwrap();

        let full = alint_core::walk(root, &crate::test_support::walk_options()).unwrap();
        let changed: HashSet<PathBuf> = [
            PathBuf::from("esc"),
            PathBuf::from("dangling"),
            PathBuf::from("self_loop"),
        ]
        .into_iter()
        .collect();
        let spec = std::sync::Arc::new(spec_yaml(
            "id: t\nkind: no_symlinks\npaths: \"**/*\"\nlevel: error\n",
        ));
        let registry = crate::builtin_registry();
        let rule = registry.build(&spec).unwrap();
        let engine = alint_core::Engine::from_entries(
            vec![alint_core::RuleEntry::new(rule).with_spec(spec)],
            registry,
        )
        .with_changed_paths(changed);
        let report = engine.run(root, &full).unwrap();
        let mut flagged: Vec<_> = report.results[0]
            .violations
            .iter()
            .filter_map(|v| v.path.as_deref())
            .collect();
        flagged.sort();
        assert_eq!(
            flagged,
            vec![
                std::path::Path::new("dangling"),
                std::path::Path::new("esc"),
                std::path::Path::new("self_loop"),
            ],
            "{report:?}"
        );
    }
}
