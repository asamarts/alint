//! `dir_exists` — at least one directory matching `paths` must exist.

use alint_core::{Context, Error, Level, PathsSpec, Result, Rule, RuleSpec, Scope, Violation};

#[derive(Debug)]
pub struct DirExistsRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    patterns: Vec<String>,
    /// When `true`, only consider directories that contain at
    /// least one git-tracked file. Outside a git repo the
    /// tracked set is empty, so the rule reports the "missing"
    /// violation as if no matching directory existed.
    git_tracked_only: bool,
}

impl Rule for DirExistsRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        self.level
    }
    fn policy_url(&self) -> Option<&str> {
        self.policy_url.as_deref()
    }
    fn wants_git_tracked(&self) -> bool {
        self.git_tracked_only
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
        let found = ctx.index.dirs().any(|entry| {
            if !self.scope.matches(&entry.path) {
                return false;
            }
            if self.git_tracked_only && !ctx.dir_has_tracked_files(&entry.path) {
                return false;
            }
            true
        });
        if found {
            Ok(Vec::new())
        } else {
            let msg = self.message.clone().unwrap_or_else(|| {
                let tracked = if self.git_tracked_only {
                    " (with tracked content)"
                } else {
                    ""
                };
                format!(
                    "expected a directory matching [{}]{tracked}",
                    self.patterns.join(", ")
                )
            });
            Ok(vec![Violation::new(msg)])
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
    Ok(Box::new(DirExistsRule {
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
    fn git_tracked_only_propagates_to_wants_git_tracked() {
        let spec = spec_yaml(
            "id: t\n\
             kind: dir_exists\n\
             paths: \"src\"\n\
             level: error\n\
             git_tracked_only: true\n",
        );
        let rule = build(&spec).unwrap();
        assert!(rule.wants_git_tracked());
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
}
