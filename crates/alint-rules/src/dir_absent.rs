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
    fn wants_git_tracked(&self) -> bool {
        self.git_tracked_only
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
        for entry in ctx.index.dirs() {
            if !self.scope.matches(&entry.path) {
                continue;
            }
            if self.git_tracked_only && !ctx.dir_has_tracked_files(&entry.path) {
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
            violations.push(Violation::new(msg).with_path(&entry.path));
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
