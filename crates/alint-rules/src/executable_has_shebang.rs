//! `executable_has_shebang` — every `+x` file in scope must
//! begin with a shebang line (`#!`).
//!
//! Catches the common bug where a script has been marked
//! executable but its content is something else (a text file,
//! a binary missing the shebang). Running such a file silently
//! invokes the user's login shell — surprising at best, exploit
//! vector at worst.
//!
//! Non-Unix platforms: rule is a no-op (no real +x semantics).
//! No fix op — the correct resolution (add shebang vs. remove +x)
//! is a human judgment call.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};

#[derive(Debug)]
pub struct ExecutableHasShebangRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
}

impl Rule for ExecutableHasShebangRule {
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
            if meta.permissions().mode() & 0o111 == 0 {
                continue;
            }
            let Ok(bytes) = std::fs::read(&full) else {
                continue;
            };
            if !bytes.starts_with(b"#!") {
                let msg = self
                    .message
                    .clone()
                    .unwrap_or_else(|| "executable file has no shebang (#!)".to_string());
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
        Error::rule_config(&spec.id, "executable_has_shebang requires a `paths` field")
    })?;
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "executable_has_shebang has no fix op — add a shebang or clear +x is a human call",
        ));
    }
    Ok(Box::new(ExecutableHasShebangRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
    }))
}
