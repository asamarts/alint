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

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let found = ctx.index.dirs().any(|entry| self.scope.matches(&entry.path));
        if found {
            Ok(Vec::new())
        } else {
            let msg = self.message.clone().unwrap_or_else(|| {
                format!("expected a directory matching [{}]", self.patterns.join(", "))
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
    }))
}

fn patterns_of(spec: &PathsSpec) -> Vec<String> {
    match spec {
        PathsSpec::Single(s) => vec![s.clone()],
        PathsSpec::Many(v) => v.clone(),
        PathsSpec::IncludeExclude { include, .. } => include.clone(),
    }
}
