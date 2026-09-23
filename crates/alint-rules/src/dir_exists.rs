//! `dir_exists` — at least one directory matching `paths` must exist.

use alint_core::{
    Applicability, Context, Error, FixSpec, Fixer, Level, PathsSpec, Result, Rule, RuleSpec, Scope,
    Violation,
};
use serde::Deserialize;

use crate::fixers::DirCreateFixer;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// If true, only a directory directly at the repository root satisfies the
    /// rule; a nested match does not.
    #[serde(default)]
    root_only: bool,
    /// Restrict matches to directories that contain at least one git-tracked
    /// file. No effect outside a git repo. Default `false`.
    #[serde(default)]
    git_tracked_only: bool,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct DirExistsRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    patterns: Vec<String>,
    root_only: bool,
    /// When `true`, only consider directories that contain at
    /// least one git-tracked file. Outside a git repo the
    /// tracked set is empty, so the rule reports the "missing"
    /// violation as if no matching directory existed.
    git_tracked_only: bool,
    /// The optional `dir_create` fix (creates the missing literal directory).
    fixer: Option<DirCreateFixer>,
}

impl Rule for DirExistsRule {
    alint_core::rule_common_impl!();

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }

    fn git_tracked_mode(&self) -> alint_core::GitTrackedMode {
        if self.git_tracked_only {
            alint_core::GitTrackedMode::DirAware
        } else {
            alint_core::GitTrackedMode::Off
        }
    }

    fn requires_full_index(&self) -> bool {
        // Aggregate verdict over the whole tree. Note we
        // deliberately don't expose `path_scope` here: directory
        // scopes (e.g. `src/foo`) don't naturally intersect a
        // changed-set built from file paths (`src/foo/main.rs`),
        // so the engine evaluates dir-existence rules on every
        // `--changed` run. Cheap (one O(N) scan) and correct.
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        // v0.9.11: when `git_tracked_only` is set the engine
        // hands us a pre-filtered `ctx.index` (dir_aware mode);
        // the per-entry `dir_has_tracked_files` check that lived
        // here is now subsumed by the engine narrowing.
        let found = ctx.index.dirs().any(|entry| {
            // `root_only`: only a directory directly at the repo root counts.
            if self.root_only && crate::is_nested(&entry.path) {
                return false;
            }
            if !self.scope.matches(&entry.path, ctx.index) {
                return false;
            }
            true
        });
        if found {
            Ok(Vec::new())
        } else {
            let msg = self.message.clone().unwrap_or_else(|| {
                let scope = if self.root_only {
                    " at the repo root"
                } else {
                    ""
                };
                let tracked = if self.git_tracked_only {
                    " (with tracked content)"
                } else {
                    ""
                };
                format!(
                    "expected a directory matching [{}]{scope}{tracked}",
                    self.patterns.join(", ")
                )
            });
            // No path (no matching directory exists), so key on the pattern
            // set rather than relying on the volatile message fingerprint.
            Ok(vec![
                Violation::new(msg).with_baseline_key(self.patterns.join(",")),
            ])
        }
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    alint_core::reject_scope_filter_on_cross_file(spec, "dir_exists")?;
    let Some(paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "dir_exists requires a `paths` field",
        ));
    };
    let opts: Options = spec.deserialize_options()?;
    // The only supported fix op is `dir_create`, which creates the required
    // directory -- so `paths` must name ONE literal directory (a glob or multiple
    // patterns is ambiguous: which directory would we create?).
    let fixer = match &spec.fix {
        None => None,
        Some(FixSpec::DirCreate { dir_create }) => {
            let dir = single_literal_dir(paths).ok_or_else(|| {
                Error::rule_config(
                    &spec.id,
                    "dir_create requires `paths` to be a single literal directory \
                     (no glob metacharacters, no `..`)",
                )
            })?;
            // Reject combos an EMPTY directory can never satisfy, else `check` tags
            // the violation fixable and `fix` reports "created directory" but the
            // rule never clears -- a silent non-convergence (audit H1/H2).
            if opts.git_tracked_only {
                return Err(Error::rule_config(
                    &spec.id,
                    "dir_create cannot satisfy `git_tracked_only`: git does not track an \
                     empty directory, so a created directory has no tracked content. Commit \
                     a `.gitkeep` (via a `file_create` fix) instead.",
                ));
            }
            if opts.root_only && crate::is_nested(&dir) {
                return Err(Error::rule_config(
                    &spec.id,
                    format!(
                        "dir_create with `root_only` requires a root-level (single-component) \
                         directory; {} is nested and could never satisfy the rule",
                        dir.display()
                    ),
                ));
            }
            Some(DirCreateFixer::new(
                dir,
                dir_create.applicability.unwrap_or(Applicability::Safe),
            ))
        }
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "fix.{} is not compatible with dir_exists (only `dir_create`)",
                    other.op_name()
                ),
            ));
        }
    };
    Ok(Box::new(DirExistsRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        patterns: patterns_of(paths),
        root_only: opts.root_only,
        git_tracked_only: opts.git_tracked_only,
        fixer,
    }))
}

/// The single, literal directory `paths` names, or `None` if it is a glob,
/// multiple patterns, or contains a `..` component -- any of which makes
/// "the directory to create" ambiguous or out-of-tree, so `dir_create` rejects it.
fn single_literal_dir(paths: &PathsSpec) -> Option<std::path::PathBuf> {
    let PathsSpec::Single(s) = paths else {
        return None;
    };
    if s.contains(['*', '?', '[', ']', '{', '}']) {
        return None;
    }
    let p = std::path::PathBuf::from(s);
    if p.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return None;
    }
    Some(p)
}

fn patterns_of(spec: &PathsSpec) -> Vec<String> {
    match spec {
        PathsSpec::Single(s) => vec![s.clone()],
        PathsSpec::Many(v) => v.clone(),
        PathsSpec::IncludeExclude { include, .. } => include.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, index_with_dirs, spec_yaml};
    use std::path::Path;

    #[test]
    fn build_rejects_missing_paths_field() {
        let spec = spec_yaml(
            "id: t\n\
             kind: dir_exists\n\
             level: error\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(err.contains("paths"), "unexpected: {err}");
    }

    #[test]
    fn evaluate_passes_when_matching_dir_present() {
        let spec = spec_yaml(
            "id: t\n\
             kind: dir_exists\n\
             paths: \"docs\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index_with_dirs(&[("docs", true), ("docs/README.md", false)]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert!(v.is_empty(), "unexpected: {v:?}");
    }

    #[test]
    fn evaluate_fires_when_directory_missing() {
        let spec = spec_yaml(
            "id: t\n\
             kind: dir_exists\n\
             paths: \"docs\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index_with_dirs(&[("README.md", false), ("src", true)]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 1, "missing dir should fire one violation");
    }

    #[test]
    fn evaluate_skips_files_when_dir_glob_only_matches_dirs() {
        // A file named `docs` must not satisfy a `dir_exists`
        // rule — only entries with `is_dir: true` count.
        let spec = spec_yaml(
            "id: t\n\
             kind: dir_exists\n\
             paths: \"docs\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index_with_dirs(&[("docs", false)]); // a file named "docs"
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn rule_advertises_full_index_requirement() {
        let spec = spec_yaml(
            "id: t\n\
             kind: dir_exists\n\
             paths: \"docs\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        assert!(rule.requires_full_index());
    }

    #[test]
    fn git_tracked_only_advertises_dir_aware_mode() {
        let spec = spec_yaml(
            "id: t\n\
             kind: dir_exists\n\
             paths: \"src\"\n\
             level: error\n\
             git_tracked_only: true\n",
        );
        let rule = build(&spec).unwrap();
        assert_eq!(
            rule.git_tracked_mode(),
            alint_core::GitTrackedMode::DirAware,
        );
    }

    #[test]
    fn build_rejects_scope_filter_on_cross_file_rule() {
        // dir_exists is a cross-file rule (requires_full_index =
        // true); scope_filter is per-file-rules-only. The build
        // path must reject it with a clear message pointing at
        // the for_each_dir + when_iter: alternative.
        let yaml = r#"
id: t
kind: dir_exists
paths: "docs"
level: error
scope_filter:
  has_ancestor: Cargo.toml
"#;
        let spec = spec_yaml(yaml);
        let err = build(&spec).unwrap_err().to_string();
        assert!(
            err.contains("scope_filter is supported on per-file rules only"),
            "expected per-file-only message, got: {err}",
        );
        assert!(
            err.contains("dir_exists"),
            "expected message to name the cross-file kind, got: {err}",
        );
    }

    #[test]
    fn root_only_requires_a_root_level_directory() {
        let rule = build(&spec_yaml(
            "id: t\nkind: dir_exists\npaths: \"**/docs\"\nlevel: error\nroot_only: true\n",
        ))
        .unwrap();
        // Only a nested docs/ → fires (no root-level docs/).
        let nested = index_with_dirs(&[("a/docs", true)]);
        assert_eq!(
            rule.evaluate(&ctx(Path::new("/fake"), &nested))
                .unwrap()
                .len(),
            1,
        );
        // A root-level docs/ → satisfied.
        let root = index_with_dirs(&[("docs", true)]);
        assert!(
            rule.evaluate(&ctx(Path::new("/fake"), &root))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn build_accepts_root_only_and_rejects_unknown_option() {
        assert!(
            build(&spec_yaml(
                "id: t\nkind: dir_exists\npaths: \"docs\"\nlevel: error\nroot_only: true\n",
            ))
            .is_ok()
        );
        assert!(
            build(&spec_yaml(
                "id: t\nkind: dir_exists\npaths: \"docs\"\nlevel: error\nbogus: 1\n",
            ))
            .is_err()
        );
    }

    #[test]
    fn build_accepts_dir_create_for_a_single_literal_dir() {
        let rule = build(&spec_yaml(
            "id: t\nkind: dir_exists\npaths: \"docs/adr\"\nlevel: error\nfix:\n  dir_create: {}\n",
        ))
        .expect("dir_create builds for a literal directory");
        assert!(rule.fixer().is_some(), "the dir_create fixer attaches");
    }

    #[test]
    fn build_rejects_dir_create_on_a_glob_or_multiple_paths() {
        // A glob or multiple patterns is ambiguous: which directory would we create?
        for paths in [
            "\"**/generated\"",
            "\"gen*\"",
            "[\"a\", \"b\"]",
            "\"../up\"",
        ] {
            let spec = spec_yaml(&format!(
                "id: t\nkind: dir_exists\npaths: {paths}\nlevel: error\nfix:\n  dir_create: {{}}\n",
            ));
            let err = build(&spec).unwrap_err().to_string();
            assert!(
                err.contains("single literal"),
                "{paths} must be rejected: {err}"
            );
        }
    }

    #[test]
    fn build_rejects_an_incompatible_fix_op() {
        let spec = spec_yaml(
            "id: t\nkind: dir_exists\npaths: \"docs\"\nlevel: error\nfix:\n  file_remove: {}\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(err.contains("file_remove"), "{err}");
        assert!(err.contains("not compatible with dir_exists"), "{err}");
    }

    #[test]
    fn build_rejects_dir_create_with_git_tracked_only() {
        // Audit H1: git never tracks an empty directory, so `git_tracked_only` +
        // dir_create can never converge -- reject at load, not silently.
        let spec = spec_yaml(
            "id: t\nkind: dir_exists\npaths: \"vendored\"\ngit_tracked_only: true\n\
             level: error\nfix:\n  dir_create: {}\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(err.contains("git_tracked_only"), "{err}");
    }

    #[test]
    fn build_rejects_dir_create_with_root_only_and_a_nested_path() {
        // Audit H2: a nested directory can never satisfy `root_only`.
        let nested = spec_yaml(
            "id: t\nkind: dir_exists\npaths: \"a/docs\"\nroot_only: true\n\
             level: error\nfix:\n  dir_create: {}\n",
        );
        let err = build(&nested).unwrap_err().to_string();
        assert!(err.contains("root_only") && err.contains("nested"), "{err}");
        // ...but root_only + a single-component directory is fine.
        let ok = spec_yaml(
            "id: t\nkind: dir_exists\npaths: \"docs\"\nroot_only: true\n\
             level: error\nfix:\n  dir_create: {}\n",
        );
        assert!(
            build(&ok).is_ok(),
            "root_only + a single-component dir_create must build"
        );
    }
}
