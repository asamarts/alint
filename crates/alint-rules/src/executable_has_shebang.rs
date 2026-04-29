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
// Fields are read only by the `#[cfg(unix)]` evaluate path; on
// Windows the struct is constructed but never inspected.
#[cfg_attr(not(unix), allow(dead_code))]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, spec_yaml, tempdir_with_files};

    #[test]
    fn build_rejects_missing_paths_field() {
        let spec = spec_yaml(
            "id: t\n\
             kind: executable_has_shebang\n\
             level: warning\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn build_rejects_fix_block() {
        let spec = spec_yaml(
            "id: t\n\
             kind: executable_has_shebang\n\
             paths: \"scripts/**\"\n\
             level: warning\n\
             fix:\n  \
               file_remove: {}\n",
        );
        assert!(build(&spec).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn evaluate_fires_when_exec_lacks_shebang() {
        use std::os::unix::fs::PermissionsExt;
        let spec = spec_yaml(
            "id: t\n\
             kind: executable_has_shebang\n\
             paths: \"scripts/**\"\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("scripts/a.sh", b"echo hi\n")]);
        let mut perms = std::fs::metadata(tmp.path().join("scripts/a.sh"))
            .unwrap()
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(tmp.path().join("scripts/a.sh"), perms).unwrap();
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "exec without shebang must fire");
    }

    #[cfg(unix)]
    #[test]
    fn evaluate_passes_when_exec_has_shebang() {
        use std::os::unix::fs::PermissionsExt;
        let spec = spec_yaml(
            "id: t\n\
             kind: executable_has_shebang\n\
             paths: \"scripts/**\"\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("scripts/a.sh", b"#!/bin/sh\necho hi\n")]);
        let mut perms = std::fs::metadata(tmp.path().join("scripts/a.sh"))
            .unwrap()
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(tmp.path().join("scripts/a.sh"), perms).unwrap();
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "exec with shebang should pass: {v:?}");
    }

    #[cfg(unix)]
    #[test]
    fn evaluate_silent_on_non_exec_files() {
        use std::os::unix::fs::PermissionsExt;
        let spec = spec_yaml(
            "id: t\n\
             kind: executable_has_shebang\n\
             paths: \"**/*\"\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("README.md", b"# title\n")]);
        let mut perms = std::fs::metadata(tmp.path().join("README.md"))
            .unwrap()
            .permissions();
        perms.set_mode(0o644);
        std::fs::set_permissions(tmp.path().join("README.md"), perms).unwrap();
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "non-exec doesn't need shebang: {v:?}");
    }
}
