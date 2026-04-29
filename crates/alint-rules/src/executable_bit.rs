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
// Fields are read only by the `#[cfg(unix)]` evaluate path; on
// Windows the struct is constructed but never inspected, so
// rustc flags `message`/`scope`/`require_exec` as dead code.
#[cfg_attr(not(unix), allow(dead_code))]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::spec_yaml;
    // ctx + tempdir_with_files are only consumed by the
    // `#[cfg(unix)]` evaluate-path tests below; importing them
    // unconditionally trips `unused_imports` on Windows.
    #[cfg(unix)]
    use crate::test_support::{ctx, tempdir_with_files};

    #[test]
    fn build_rejects_missing_paths_field() {
        let spec = spec_yaml(
            "id: t\n\
             kind: executable_bit\n\
             require: true\n\
             level: error\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn build_rejects_missing_require() {
        let spec = spec_yaml(
            "id: t\n\
             kind: executable_bit\n\
             paths: \"scripts/**\"\n\
             level: error\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn build_rejects_fix_block() {
        let spec = spec_yaml(
            "id: t\n\
             kind: executable_bit\n\
             paths: \"scripts/**\"\n\
             require: true\n\
             level: error\n\
             fix:\n  \
               file_remove: {}\n",
        );
        assert!(build(&spec).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn evaluate_fires_when_exec_required_but_missing() {
        use std::os::unix::fs::PermissionsExt;
        let spec = spec_yaml(
            "id: t\n\
             kind: executable_bit\n\
             paths: \"scripts/**\"\n\
             require: true\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("scripts/a.sh", b"#!/bin/sh\n")]);
        // Default mode is 0644 — no +x bit.
        let mut perms = std::fs::metadata(tmp.path().join("scripts/a.sh"))
            .unwrap()
            .permissions();
        perms.set_mode(0o644);
        std::fs::set_permissions(tmp.path().join("scripts/a.sh"), perms).unwrap();
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn evaluate_passes_when_exec_required_and_set() {
        use std::os::unix::fs::PermissionsExt;
        let spec = spec_yaml(
            "id: t\n\
             kind: executable_bit\n\
             paths: \"scripts/**\"\n\
             require: true\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("scripts/a.sh", b"#!/bin/sh\n")]);
        let mut perms = std::fs::metadata(tmp.path().join("scripts/a.sh"))
            .unwrap()
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(tmp.path().join("scripts/a.sh"), perms).unwrap();
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "0755 should pass require=true: {v:?}");
    }

    #[cfg(unix)]
    #[test]
    fn evaluate_fires_when_exec_forbidden_but_set() {
        use std::os::unix::fs::PermissionsExt;
        // require: false → no .md should be executable
        let spec = spec_yaml(
            "id: t\n\
             kind: executable_bit\n\
             paths: \"**/*.md\"\n\
             require: false\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("README.md", b"# title\n")]);
        let mut perms = std::fs::metadata(tmp.path().join("README.md"))
            .unwrap()
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(tmp.path().join("README.md"), perms).unwrap();
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "0755 markdown should fire require=false");
    }
}
