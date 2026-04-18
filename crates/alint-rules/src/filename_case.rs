//! `filename_case` — every file in scope must have a basename whose stem
//! matches a case convention.
//!
//! The check runs on the *stem* (filename with the final extension removed),
//! matching ls-lint precedent. For files with compound extensions like
//! `foo.spec.ts`, the stem is `foo.spec`, which will fail most case checks —
//! use `filename_regex` for finer control in those situations.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

use crate::case::CaseConvention;

#[derive(Debug, Deserialize)]
struct Options {
    case: CaseConvention,
}

#[derive(Debug)]
pub struct FilenameCaseRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    case: CaseConvention,
}

impl Rule for FilenameCaseRule {
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
            if !self.scope.matches(&entry.path) {
                continue;
            }
            let Some(stem) = entry.path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !self.case.check(stem) {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "filename stem {:?} is not {}",
                        stem,
                        self.case.display_name()
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
            "filename_case requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    Ok(Box::new(FilenameCaseRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        case: opts.case,
    }))
}
