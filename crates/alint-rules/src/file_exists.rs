//! `file_exists` — require that at least one file matching any of the given
//! globs exists in the repository.

use alint_core::{Context, Error, Level, PathsSpec, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    #[serde(default)]
    root_only: bool,
}

#[derive(Debug)]
pub struct FileExistsRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    patterns: Vec<String>,
    root_only: bool,
}

impl FileExistsRule {
    fn describe_patterns(&self) -> String {
        self.patterns.join(", ")
    }
}

impl Rule for FileExistsRule {
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
        let found = ctx.index.files().any(|entry| {
            if self.root_only && entry.path.components().count() != 1 {
                return false;
            }
            self.scope.matches(&entry.path)
        });
        if found {
            Ok(Vec::new())
        } else {
            let message = self.message.clone().unwrap_or_else(|| {
                let scope = if self.root_only {
                    " at the repo root"
                } else {
                    ""
                };
                format!(
                    "expected a file matching [{}]{scope}",
                    self.describe_patterns()
                )
            });
            Ok(vec![Violation::new(message)])
        }
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "file_exists requires a `paths` field",
        ));
    };
    let patterns = patterns_of(paths);
    let scope = Scope::from_paths_spec(paths)?;
    let opts: Options = spec
        .deserialize_options()
        .unwrap_or(Options { root_only: false });
    Ok(Box::new(FileExistsRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope,
        patterns,
        root_only: opts.root_only,
    }))
}

fn patterns_of(spec: &PathsSpec) -> Vec<String> {
    match spec {
        PathsSpec::Single(s) => vec![s.clone()],
        PathsSpec::Many(v) => v.clone(),
        PathsSpec::IncludeExclude { include, .. } => include.clone(),
    }
}
