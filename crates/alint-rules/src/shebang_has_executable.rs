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
// Fields are read only by the `#[cfg(unix)]` evaluate path; on
// Windows the struct is constructed but never inspected.
#[cfg_attr(not(unix), allow(dead_code))]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::spec_yaml;
    #[cfg(unix)]
    use crate::test_support::{ctx, tempdir_with_files};

    #[test]
    fn build_rejects_missing_paths_field() {
        let spec = spec_yaml(
            "id: t\n\
             kind: shebang_has_executable\n\
             level: warning\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn build_rejects_fix_block() {
        let spec = spec_yaml(
            "id: t\n\
             kind: shebang_has_executable\n\
             paths: \"scripts/**\"\n\
             level: warning\n\
             fix:\n  \
               file_remove: {}\n",
        );
        assert!(build(&spec).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn evaluate_fires_when_shebang_lacks_exec_bit() {
        use std::os::unix::fs::PermissionsExt;
        let spec = spec_yaml(
            "id: t\n\
             kind: shebang_has_executable\n\
             paths: \"scripts/**\"\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("scripts/a.sh", b"#!/bin/sh\necho hi\n")]);
        let mut perms = std::fs::metadata(tmp.path().join("scripts/a.sh"))
            .unwrap()
            .permissions();
        perms.set_mode(0o644);
        std::fs::set_permissions(tmp.path().join("scripts/a.sh"), perms).unwrap();
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "shebang without +x must fire");
    }

    #[cfg(unix)]
    #[test]
    fn evaluate_passes_when_shebang_has_exec_bit() {
        use std::os::unix::fs::PermissionsExt;
        let spec = spec_yaml(
            "id: t\n\
             kind: shebang_has_executable\n\
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
        assert!(v.is_empty(), "shebang with +x should pass: {v:?}");
    }

    #[cfg(unix)]
    #[test]
    fn evaluate_silent_on_non_shebang_files() {
        use std::os::unix::fs::PermissionsExt;
        let spec = spec_yaml(
            "id: t\n\
             kind: shebang_has_executable\n\
             paths: \"**/*\"\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("a.txt", b"plain text")]);
        let mut perms = std::fs::metadata(tmp.path().join("a.txt"))
            .unwrap()
            .permissions();
        perms.set_mode(0o644);
        std::fs::set_permissions(tmp.path().join("a.txt"), perms).unwrap();
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "no shebang means rule no-ops: {v:?}");
    }
}
