//! `file_content_forbidden` — files in scope must NOT match a regex.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Options {
    pattern: String,
}

#[derive(Debug)]
pub struct FileContentForbiddenRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    pattern_src: String,
    pattern: Regex,
}

impl Rule for FileContentForbiddenRule {
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
            let full = ctx.root.join(&entry.path);
            let bytes = match std::fs::read(&full) {
                Ok(b) => b,
                Err(e) => {
                    violations.push(
                        Violation::new(format!("could not read file: {e}"))
                            .with_path(&entry.path),
                    );
                    continue;
                }
            };
            let Ok(text) = std::str::from_utf8(&bytes) else {
                // Non-UTF-8 files are silently skipped; they can't contain a
                // text regex match. Use `file_is_text` to flag binaries.
                continue;
            };
            if let Some(m) = self.pattern.find(text) {
                let line = text[..m.start()].matches('\n').count() + 1;
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!("forbidden pattern /{}/ found", self.pattern_src)
                });
                violations.push(
                    Violation::new(msg)
                        .with_path(&entry.path)
                        .with_location(line, 1),
                );
            }
        }
        Ok(violations)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "file_content_forbidden requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let pattern = Regex::new(&opts.pattern)
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid pattern: {e}")))?;
    Ok(Box::new(FileContentForbiddenRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        pattern_src: opts.pattern,
        pattern,
    }))
}
