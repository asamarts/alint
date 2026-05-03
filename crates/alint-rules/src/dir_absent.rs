//! `dir_absent` — no directory matching `paths` may exist.

use alint_core::{Context, Error, Level, PathsSpec, Result, Rule, RuleSpec, Scope, Violation};

#[derive(Debug)]
pub struct DirAbsentRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    patterns: Vec<String>,
    /// When `true`, only fire on directories that contain at
    /// least one git-tracked file. The canonical use case is
    /// "don't let `target/` be committed" — with this flag set,
    /// a developer's locally-built `target/` (gitignored, no
    /// tracked content) doesn't trigger; a `target/` whose
    /// contents made it into git's index does.
    git_tracked_only: bool,
}

impl Rule for DirAbsentRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        self.level
    }
    fn policy_url(&self) -> Option<&str> {
        self.policy_url.as_deref()
    }
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
    alint_core::reject_scope_filter_on_cross_file(spec, "dir_absent")?;
    let Some(paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "dir_absent requires a `paths` field",
        ));
    };
    Ok(Box::new(DirAbsentRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        patterns: patterns_of(paths),
        git_tracked_only: spec.git_tracked_only,
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
    fn build_rejects_scope_filter_on_cross_file_rule() {
        // dir_absent is a cross-file rule (requires_full_index =
        // true); scope_filter is per-file-rules-only. The build
        // path must reject it with a clear message pointing at
        // the for_each_dir + when_iter: alternative.
        let yaml = r#"
id: t
kind: dir_absent
paths: "target"
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
            err.contains("dir_absent"),
            "expected message to name the cross-file kind, got: {err}",
        );
    }
}
