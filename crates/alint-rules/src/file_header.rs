//! `file_header` — first N lines of each file in scope must match a pattern.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Options {
    pattern: String,
    #[serde(default = "default_lines")]
    lines: usize,
}

fn default_lines() -> usize {
    20
}

#[derive(Debug)]
pub struct FileHeaderRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    pattern_src: String,
    pattern: Regex,
    lines: usize,
}

impl Rule for FileHeaderRule {
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
                violations.push(
                    Violation::new("file is not valid UTF-8; cannot match header")
                        .with_path(&entry.path),
                );
                continue;
            };
            let header: String = text
                .split_inclusive('\n')
                .take(self.lines)
                .collect();
            if !self.pattern.is_match(&header) {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "first {} line(s) do not match required header /{}/",
                        self.lines, self.pattern_src
                    )
                });
                violations.push(
                    Violation::new(msg)
                        .with_path(&entry.path)
                        .with_location(1, 1),
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
            "file_header requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.lines == 0 {
        return Err(Error::rule_config(
            &spec.id,
            "file_header `lines` must be > 0",
        ));
    }
    let pattern = Regex::new(&opts.pattern)
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid pattern: {e}")))?;
    Ok(Box::new(FileHeaderRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        pattern_src: opts.pattern,
        pattern,
        lines: opts.lines,
    }))
}
