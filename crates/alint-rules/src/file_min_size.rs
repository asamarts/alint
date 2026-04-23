//! `file_min_size` — files in scope must be at least `min_bytes` bytes.
//!
//! Symmetric counterpart of [`file_max_size`](crate::file_max_size).
//! The primary use case is "my README is more than a stub" /
//! "my LICENSE text is not empty": a non-zero minimum size
//! catches placeholder files that pass existence checks but add
//! no information.
//!
//! Empty files are a likely separate problem; if you want to
//! explicitly forbid those, use
//! [`no_empty_files`](crate::no_empty_files) — `file_min_size:
//! 1` works too but the dedicated rule carries clearer intent.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Options {
    min_bytes: u64,
}

#[derive(Debug)]
pub struct FileMinSizeRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    min_bytes: u64,
}

impl Rule for FileMinSizeRule {
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
            if entry.size < self.min_bytes {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "file below {} byte(s) (actual: {})",
                        self.min_bytes, entry.size,
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
            "file_min_size requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    Ok(Box::new(FileMinSizeRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        min_bytes: opts.min_bytes,
    }))
}
