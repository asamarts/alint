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
//! Non-Unix platforms: rule is a no-op. Fix op: `chmod` (`fix: { chmod: {} }`),
//! which sets the executable bit; Safe by default, applied only on Unix.

use alint_core::{
    Applicability, Context, Error, FixSpec, Fixer, Level, Result, Rule, RuleSpec, Scope, Violation,
};

use crate::fixers::ChmodFixer;

#[cfg(unix)]
use crate::io::read_prefix_n;

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
    fixer: Option<ChmodFixer>,
}

impl Rule for ShebangHasExecutableRule {
    /// Expose the per-file scope so the engine resolves this rule's
    /// `scope_filter` (manifest sets, `changed_since:`) before dispatch and
    /// can `--changed`-skip it (see `Rule::path_scope`).
    fn path_scope(&self) -> Option<&Scope> {
        Some(&self.scope)
    }

    alint_core::rule_common_impl!();

    #[cfg(unix)]
    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        use std::os::unix::fs::PermissionsExt;

        let mut violations = Vec::new();
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path, ctx.index) {
                continue;
            }
            // Bounded read: only the first 2 bytes (`#!`)
            // matter to short-circuit non-shebang files; the
            // metadata check happens after, only on actual
            // shebang files.
            let full = ctx.root.join(&entry.path);
            let Ok(bytes) = read_prefix_n(&full, 2) else {
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
                violations.push(Violation::new(msg).with_path(entry.path.clone()));
            }
        }
        Ok(violations)
    }

    #[cfg(not(unix))]
    fn evaluate(&self, _ctx: &Context<'_>) -> Result<Vec<Violation>> {
        Ok(Vec::new())
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let _paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(&spec.id, "shebang_has_executable requires a `paths` field")
    })?;
    // The only supported fix op is `chmod`, which sets +x (a shebang script must be
    // executable).
    let fixer = match &spec.fix {
        None => None,
        Some(FixSpec::Chmod { chmod }) => Some(ChmodFixer::new(
            /* desired_exec */ true,
            chmod.applicability.unwrap_or(Applicability::Safe),
        )),
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "fix.{} is not compatible with shebang_has_executable (only `chmod`)",
                    other.op_name()
                ),
            ));
        }
    };
    Ok(Box::new(ShebangHasExecutableRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        fixer,
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

    #[test]
    fn build_accepts_a_chmod_fix() {
        let spec = spec_yaml(
            "id: t\n\
             kind: shebang_has_executable\n\
             paths: \"scripts/**\"\n\
             level: error\n\
             fix: { chmod: {} }\n",
        );
        let rule = build(&spec).expect("chmod fix builds");
        assert!(rule.fixer().is_some(), "the rule exposes a chmod fixer");
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

    #[cfg(unix)]
    #[test]
    fn scope_filter_narrows() {
        use std::os::unix::fs::PermissionsExt;
        // Two scripts with shebang but no +x; only the one
        // inside a dir with `marker.lock` as ancestor fires.
        let spec = spec_yaml(
            "id: t\n\
             kind: shebang_has_executable\n\
             paths: \"**/*.sh\"\n\
             scope_filter:\n  \
               has_ancestor: marker.lock\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[
            ("pkg/marker.lock", b""),
            ("pkg/a.sh", b"#!/bin/sh\necho hi\n"),
            ("other/a.sh", b"#!/bin/sh\necho hi\n"),
        ]);
        for rel in ["pkg/a.sh", "other/a.sh"] {
            let mut perms = std::fs::metadata(tmp.path().join(rel))
                .unwrap()
                .permissions();
            perms.set_mode(0o644);
            std::fs::set_permissions(tmp.path().join(rel), perms).unwrap();
        }
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "only in-scope file should fire: {v:?}");
        assert_eq!(v[0].path.as_deref(), Some(std::path::Path::new("pkg/a.sh")));
    }
}
