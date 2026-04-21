//! `no_empty_files` — flag zero-byte files in scope.
//!
//! Empty files usually indicate placeholders forgotten in a
//! branch or generator output that lost its content. Fixable
//! via `file_remove`, which deletes the empty file.

use alint_core::{Context, Error, FixSpec, Fixer, Level, Result, Rule, RuleSpec, Scope, Violation};

use crate::fixers::FileRemoveFixer;

#[derive(Debug)]
pub struct NoEmptyFilesRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    fixer: Option<FileRemoveFixer>,
}

impl Rule for NoEmptyFilesRule {
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
            if entry.size == 0 {
                let msg = self
                    .message
                    .clone()
                    .unwrap_or_else(|| "file is empty".to_string());
                violations.push(Violation::new(msg).with_path(&entry.path));
            }
        }
        Ok(violations)
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let paths = spec
        .paths
        .as_ref()
        .ok_or_else(|| Error::rule_config(&spec.id, "no_empty_files requires a `paths` field"))?;
    let fixer = match &spec.fix {
        Some(FixSpec::FileRemove { .. }) => Some(FileRemoveFixer),
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "fix.{} is not compatible with no_empty_files",
                    other.op_name()
                ),
            ));
        }
        None => None,
    };
    Ok(Box::new(NoEmptyFilesRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        fixer,
    }))
}
