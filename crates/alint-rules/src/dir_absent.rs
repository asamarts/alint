//! `dir_absent` — no directory matching `paths` may exist.

use alint_core::{Context, Error, Level, PathsSpec, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// If true, only a directory directly at the repository root is forbidden; a
    /// nested match with the same name is allowed.
    #[serde(default)]
    root_only: bool,
    /// Restrict matches to directories that contain at least one git-tracked
    /// file. No effect outside a git repo. Default `false`.
    #[serde(default)]
    git_tracked_only: bool,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct DirAbsentRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    patterns: Vec<String>,
    root_only: bool,
    /// When `true`, only fire on directories that contain at
    /// least one git-tracked file. The canonical use case is
    /// "don't let `target/` be committed" — with this flag set,
    /// a developer's locally-built `target/` (gitignored, no
    /// tracked content) doesn't trigger; a `target/` whose
    /// contents made it into git's index does.
    git_tracked_only: bool,
}

impl Rule for DirAbsentRule {
    // Deliberately NO `path_scope` override (mirrors `dir_exists`): this is a
    // directory rule (`requires_full_index = true`, scope matches dir paths), and
    // a directory scope doesn't intersect a file-path-based `--changed` set, so
    // exposing `path_scope` would let `skip_for_changed` wrongly skip a standing
    // forbidden dir on every `--changed` run. `has_ancestor` still works (it
    // resolves inside `evaluate`); manifest `scope_filter` is rejected loud by the
    // engine's `ensure_manifest_scope_resolvable` guard rather than silently
    // no-op'ing -- manifest-set scope isn't meaningful on a whole-tree dir rule.
    alint_core::rule_common_impl!();
    fn git_tracked_mode(&self) -> alint_core::GitTrackedMode {
        if self.git_tracked_only {
            alint_core::GitTrackedMode::DirAware
        } else {
            alint_core::GitTrackedMode::Off
        }
    }

    fn requires_full_index(&self) -> bool {
        // See `dir_exists::requires_full_index` — directory
        // scopes don't intersect a file-path-based changed-set
        // cleanly, so we always evaluate this rule on the full
        // tree in `--changed` mode. One O(N) scan per rule.
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        // v0.9.11: when `git_tracked_only` is set the engine
        // hands us a pre-filtered `ctx.index` (dir_aware mode);
        // the per-entry `dir_has_tracked_files` check that lived
        // here is now subsumed by the engine narrowing.
        for entry in ctx.index.dirs() {
            if !self.scope.matches(&entry.path, ctx.index) {
                continue;
            }
            // `root_only`: only a directory directly at the repo root is
            // forbidden; a nested directory with the same name is allowed.
            if self.root_only && crate::is_nested(&entry.path) {
                continue;
            }
            let msg = self.message.clone().unwrap_or_else(|| {
                let tracked = if self.git_tracked_only {
                    " and has tracked content"
                } else {
                    ""
                };
                format!(
                    "directory is forbidden (matches [{}]{tracked}): {}",
                    self.patterns.join(", "),
                    entry.path.display()
                )
            });
            violations.push(Violation::new(msg).with_path(entry.path.clone()));
        }
        Ok(violations)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "dir_absent requires a `paths` field",
        ));
    };
    // v0.9.18: dir_absent honours `scope_filter` so the same
    // ancestor-manifest gate that scopes per-file rules can scope
    // dir-iterating rules — required by `hygiene-no-js-build-outputs`
    // (only fire on `dist/`/`build/` whose ancestor chain contains
    // a `package.json`, so polyglot monorepos with non-JS dirs of
    // the same name don't see false positives).
    let opts: Options = spec.deserialize_options()?;
    Ok(Box::new(DirAbsentRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        patterns: patterns_of(paths),
        root_only: opts.root_only,
        git_tracked_only: opts.git_tracked_only,
    }))
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
             kind: dir_absent\n\
             level: error\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(err.contains("paths"), "unexpected: {err}");
    }

    #[test]
    fn evaluate_passes_when_no_matching_dir_present() {
        let spec = spec_yaml(
            "id: t\n\
             kind: dir_absent\n\
             paths: \"target\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index_with_dirs(&[("src", true), ("docs", true)]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert!(v.is_empty(), "unexpected: {v:?}");
    }

    #[test]
    fn evaluate_fires_one_violation_per_forbidden_dir() {
        let spec = spec_yaml(
            "id: t\n\
             kind: dir_absent\n\
             paths: \"**/target\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index_with_dirs(&[("target", true), ("crates/foo/target", true), ("src", true)]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 2, "expected one violation per target dir: {v:?}");
    }

    #[test]
    fn evaluate_ignores_files_with_matching_name() {
        let spec = spec_yaml(
            "id: t\n\
             kind: dir_absent\n\
             paths: \"target\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        // A file named "target" should NOT fire `dir_absent`.
        let idx = index_with_dirs(&[("target", false)]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert!(v.is_empty(), "file named 'target' shouldn't fire");
    }

    #[test]
    fn git_tracked_only_advertises_dir_aware_mode() {
        // v0.9.11: the silent-no-op-outside-git-repo guarantee
        // moved from a per-rule runtime check to an engine-side
        // pre-filtered FileIndex. Calling `evaluate` directly
        // bypasses the engine's filtering, so this unit test
        // can no longer assert the no-op behaviour at the rule
        // level — instead it asserts the rule advertises the
        // correct `GitTrackedMode`, which is what tells the
        // engine to substitute an empty index when the
        // tracked-set is `None`. The end-to-end no-op behaviour
        // is asserted by
        // `crates/alint-e2e/scenarios/check/git/git_tracked_only_outside_git_silently_passes_absent.yml`.
        let spec = spec_yaml(
            "id: t\n\
             kind: dir_absent\n\
             paths: \"target\"\n\
             level: error\n\
             git_tracked_only: true\n",
        );
        let rule = build(&spec).unwrap();
        assert_eq!(
            rule.git_tracked_mode(),
            alint_core::GitTrackedMode::DirAware,
            "git_tracked_only on dir_absent must advertise DirAware mode",
        );
    }

    #[test]
    fn rule_advertises_full_index_requirement() {
        let spec = spec_yaml(
            "id: t\n\
             kind: dir_absent\n\
             paths: \"target\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        assert!(rule.requires_full_index());
    }

    #[test]
    fn build_accepts_scope_filter() {
        // v0.9.18: dir_absent honours `scope_filter` so the
        // ancestor-manifest gate that scopes per-file rules can
        // also scope dir-iterating rules. The build path must
        // accept the field and bundle it into the rule's `Scope`.
        let yaml = r#"
id: t
kind: dir_absent
paths: "**/dist"
level: warning
scope_filter:
  has_ancestor: package.json
"#;
        let spec = spec_yaml(yaml);
        let rule = build(&spec).expect("scope_filter must be accepted on dir_absent");
        assert_eq!(rule.id(), "t");
    }

    #[test]
    fn root_only_forbids_only_a_root_level_directory() {
        let rule = build(&spec_yaml(
            "id: t\nkind: dir_absent\npaths: \"**/target\"\nlevel: error\nroot_only: true\n",
        ))
        .unwrap();
        let idx = index_with_dirs(&[("target", true), ("a/target", true)]);
        assert_eq!(
            rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap().len(),
            1,
            "root_only forbids only the root-level target/",
        );
        // Without root_only, both fire.
        let plain = build(&spec_yaml(
            "id: t\nkind: dir_absent\npaths: \"**/target\"\nlevel: error\n",
        ))
        .unwrap();
        assert_eq!(
            plain
                .evaluate(&ctx(Path::new("/fake"), &idx))
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn build_accepts_root_only_and_rejects_unknown_option() {
        assert!(
            build(&spec_yaml(
                "id: t\nkind: dir_absent\npaths: \"target\"\nlevel: error\nroot_only: true\n",
            ))
            .is_ok()
        );
        assert!(
            build(&spec_yaml(
                "id: t\nkind: dir_absent\npaths: \"target\"\nlevel: error\nbogus: 1\n",
            ))
            .is_err()
        );
    }
}
