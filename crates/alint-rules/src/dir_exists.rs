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
