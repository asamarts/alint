//! `file_ends_with` — every file in scope must end with the
//! configured suffix (byte-level).
//!
//! Useful for required trailing banners ("<!-- end-of-file -->"),
//! closing magic bytes, or enforcing a generated-file sentinel.
//! For the narrower "file must end with a newline" check, prefer
//! `final_newline` — it has a dedicated fixer and integrates with
//! the text-hygiene family.
//!
//! Check-only: the correct fix is `file_append` with matching
//! content. Auto-appending implicitly could silently duplicate a
//! near-matching tail.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// The required suffix. Matched byte-for-byte.
    suffix: String,
}

#[derive(Debug)]
pub struct FileEndsWithRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    suffix: Vec<u8>,
}

impl Rule for FileEndsWithRule {
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
            let Ok(bytes) = std::fs::read(&full) else {
                continue;
            };
            if !bytes.ends_with(&self.suffix) {
                let msg = self
                    .message
                    .clone()
                    .unwrap_or_else(|| "file does not end with the required suffix".to_string());
                violations.push(Violation::new(msg).with_path(&entry.path));
            }
        }
        Ok(violations)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let paths = spec
        .paths
        .as_ref()
        .ok_or_else(|| Error::rule_config(&spec.id, "file_ends_with requires a `paths` field"))?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.suffix.is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "file_ends_with.suffix must not be empty",
        ));
    }
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "file_ends_with has no fix op — pair with an explicit `file_append` rule if you \
             want auto-append (avoids silently duplicating near-matching suffixes).",
        ));
    }
    Ok(Box::new(FileEndsWithRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        suffix: opts.suffix.into_bytes(),
    }))
}
