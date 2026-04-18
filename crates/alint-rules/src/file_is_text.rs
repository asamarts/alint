//! `file_is_text` — every file in scope must be detected as text (not binary).
//!
//! Detection uses `content_inspector` on the first 8 KiB of each file
//! (magic-byte + heuristic analysis). UTF-8, UTF-16 (with BOM), and plain
//! 7-bit ASCII are treated as text.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};

use crate::io::{Classification, classify_bytes, read_prefix};

#[derive(Debug)]
pub struct FileIsTextRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
}

impl Rule for FileIsTextRule {
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
                // Empty files are text by convention.
                continue;
            }
            let full = ctx.root.join(&entry.path);
            let bytes = match read_prefix(&full) {
                Ok(b) => b,
                Err(e) => {
                    violations.push(
                        Violation::new(format!("could not read file: {e}"))
                            .with_path(&entry.path),
                    );
                    continue;
                }
            };
            if classify_bytes(&bytes) == Classification::Binary {
                let msg = self.message.clone().unwrap_or_else(|| {
                    "file is detected as binary; text is required here".to_string()
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
            "file_is_text requires a `paths` field",
        ));
    };
    Ok(Box::new(FileIsTextRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
    }))
}
