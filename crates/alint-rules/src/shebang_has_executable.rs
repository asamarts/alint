//! `shebang_has_executable` — every file that starts with `#!`
//! must have the Unix `+x` bit set.
//!
//! The inverse of `executable_has_shebang`: catches scripts that
//! were committed with a shebang but where the executable bit
//! was never set (or got clobbered by `git add --chmod=-x`,
//! `cp`, a tarball round-trip, etc.). Running them requires
//! `bash script.sh` instead of `./script.sh`, which is usually
//! not the author's intent.
//!
//! Non-Unix platforms: rule is a no-op. No fix op — `chmod`
//! auto-apply is deferred to a later release (see ROADMAP).

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};

#[derive(Debug)]
pub struct ShebangHasExecutableRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
}

impl Rule for ShebangHasExecutableRule {
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
            let Ok(bytes) = std::fs::read(&full) else {
                continue;
            };
            if !bytes.starts_with(b"#!") {
                continue;
            }
            let Ok(meta) = std::fs::metadata(&full) else {
                continue;
            };
            if meta.permissions().mode() & 0o111 == 0 {
                let msg = self
                    .message
                    .clone()
                    .unwrap_or_else(|| "shebang script is not marked executable".to_string());
                violations.push(Violation::new(msg).with_path(&entry.path));
            }
        }
        Ok(violations)
    }

    #[cfg(not(unix))]
    fn evaluate(&self, _ctx: &Context<'_>) -> Result<Vec<Violation>> {
        Ok(Vec::new())
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(&spec.id, "shebang_has_executable requires a `paths` field")
    })?;
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "shebang_has_executable has no fix op — chmod auto-apply is deferred (see ROADMAP)",
        ));
    }
    Ok(Box::new(ShebangHasExecutableRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
    }))
}
