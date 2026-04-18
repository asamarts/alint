//! `file_absent` — emit a violation for every file matching `paths`.

use alint_core::{Context, Error, Level, PathsSpec, Result, Rule, RuleSpec, Scope, Violation};

#[derive(Debug)]
pub struct FileAbsentRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    patterns: Vec<String>,
}

impl Rule for FileAbsentRule {
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
            if self.scope.matches(&entry.path) {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "file is forbidden (matches [{}]): {}",
                        self.patterns.join(", "),
                        entry.path.display()
                    )
                });
                violations.push(Violation::new(msg).with_path(&entry.path));
            }
        }
        Ok(violations)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "file_absent requires a `paths` field",
        ));
    };
    Ok(Box::new(FileAbsentRule {
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
