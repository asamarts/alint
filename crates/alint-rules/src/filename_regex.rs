//! `filename_regex` — every file in scope must have a basename matching a
//! regex. Anchored with `^...$` automatically; use the full basename
//! (including extension) in your pattern.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Options {
    pattern: String,
    /// Check against the file *stem* (no final extension) instead of the
    /// full basename. Defaults to `false` (full basename is matched).
    #[serde(default)]
    stem: bool,
}

#[derive(Debug)]
pub struct FilenameRegexRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    pattern_src: String,
    pattern: Regex,
    stem: bool,
}

impl Rule for FilenameRegexRule {
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
            let name = if self.stem {
                entry.path.file_stem().and_then(|s| s.to_str())
            } else {
                entry.path.file_name().and_then(|s| s.to_str())
            };
            let Some(name) = name else { continue };
            if !self.pattern.is_match(name) {
                let target = if self.stem { "stem" } else { "basename" };
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "filename {target} {:?} does not match /^{}$/",
                        name, self.pattern_src
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
            "filename_regex requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let anchored = format!("^{}$", opts.pattern);
    let pattern = Regex::new(&anchored)
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid pattern: {e}")))?;
    Ok(Box::new(FilenameRegexRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        pattern_src: opts.pattern,
        pattern,
        stem: opts.stem,
    }))
}
