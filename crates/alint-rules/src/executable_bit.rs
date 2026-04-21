//! `executable_bit` — assert every file in scope either has the
//! Unix `+x` bit set (`require: true`) or does not (`require: false`).
//!
//! Common uses:
//!   - Force every script under `scripts/` to be executable.
//!   - Force `.md`, `.txt`, `.yaml` to *not* be executable
//!     (a frequent accidental commit).
//!
//! Windows has no true executable bit; on non-Unix platforms the
//! rule is a no-op (never produces violations). Document this in
//! the config so platform-specific behaviour isn't a surprise.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// `true` → +x must be set; `false` → +x must NOT be set.
    require: bool,
}

#[derive(Debug)]
pub struct ExecutableBitRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    require_exec: bool,
}

impl Rule for ExecutableBitRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        self.level
    }
    fn policy_url(&self) -> Option<&str> {
        self.policy_url.as_deref()
    }

    #[cfg(unix)]
    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        use std::os::unix::fs::PermissionsExt;

        let mut violations = Vec::new();
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path) {
                continue;
            }
            let full = ctx.root.join(&entry.path);
            let Ok(meta) = std::fs::metadata(&full) else {
                continue;
            };
            let mode = meta.permissions().mode();
            let is_exec = mode & 0o111 != 0;
            let passes = is_exec == self.require_exec;
            if !passes {
                let msg = self.message.clone().unwrap_or_else(|| {
                    if self.require_exec {
                        format!("mode is 0o{mode:o}; +x bit required")
                    } else {
                        format!("mode is 0o{mode:o}; +x bit must not be set")
                    }
                });
                violations.push(Violation::new(msg).with_path(&entry.path));
            }
        }
        Ok(violations)
    }

    #[cfg(not(unix))]
    fn evaluate(&self, _ctx: &Context<'_>) -> Result<Vec<Violation>> {
        // Windows has no true executable bit; treat as always-passing
        // so configs stay portable across platforms.
        Ok(Vec::new())
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let paths = spec
        .paths
        .as_ref()
        .ok_or_else(|| Error::rule_config(&spec.id, "executable_bit requires a `paths` field"))?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "executable_bit has no fix op — chmod auto-apply is deferred (see ROADMAP)",
        ));
    }
    Ok(Box::new(ExecutableBitRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        require_exec: opts.require,
    }))
}
