//! `no_submodules` — flag the presence of a `.gitmodules` file at
//! the repo root.
//!
//! Submodules introduce a second source of truth (the submodule's
//! commit pointer) that drifts silently, complicates CI, and
//! surprises users who `git clone` without `--recurse-submodules`.
//! Many projects have adopted vendoring or package managers and
//! want to keep submodules banned forever.
//!
//! This rule is intentionally *not* configurable by `paths` —
//! `.gitmodules` at the repo root is what actually enables git
//! submodules, and letting users override that would invite
//! footguns. For more general "file X must not exist" checks,
//! use `file_absent` instead.
//!
//! Fixable via `file_remove`, which deletes `.gitmodules`. Note
//! this is the surface-level fix — the user still needs to run
//! `git submodule deinit` and clean up `.git/modules/` themselves.

use alint_core::{
    Context, Error, FixSpec, Fixer, Level, PathsSpec, Result, Rule, RuleSpec, Scope, Violation,
};

use crate::fixers::FileRemoveFixer;

#[derive(Debug)]
pub struct NoSubmodulesRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    fixer: Option<FileRemoveFixer>,
}

impl Rule for NoSubmodulesRule {
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
            let msg = self.message.clone().unwrap_or_else(|| {
                "`.gitmodules` present — git submodules are forbidden".to_string()
            });
            violations.push(Violation::new(msg).with_path(&entry.path));
        }
        Ok(violations)
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    if spec.paths.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "no_submodules does not accept a `paths` field; it always targets `.gitmodules` at \
             the repo root. Use `file_absent` for more general patterns.",
        ));
    }
    let fixer = match &spec.fix {
        Some(FixSpec::FileRemove { .. }) => Some(FileRemoveFixer),
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "fix.{} is not compatible with no_submodules",
                    other.op_name()
                ),
            ));
        }
        None => None,
    };
    let pinned = PathsSpec::Single(".gitmodules".to_string());
    Ok(Box::new(NoSubmodulesRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(&pinned)?,
        fixer,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, index, spec_yaml};
    use std::path::Path;

    #[test]
    fn build_rejects_paths_field() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_submodules\n\
             paths: \"**\"\n\
             level: warning\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(err.contains("paths"), "unexpected: {err}");
    }

    #[test]
    fn build_accepts_minimal_config() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_submodules\n\
             level: warning\n",
        );
        assert!(build(&spec).is_ok());
    }

    #[test]
    fn build_accepts_file_remove_fix() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_submodules\n\
             level: warning\n\
             fix:\n  \
               file_remove: {}\n",
        );
        let rule = build(&spec).unwrap();
        assert!(rule.fixer().is_some());
    }

    #[test]
    fn build_rejects_incompatible_fix() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_submodules\n\
             level: warning\n\
             fix:\n  \
               file_create:\n    \
                 content: \"x\"\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn evaluate_passes_without_gitmodules() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_submodules\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["README.md", ".gitignore"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn evaluate_fires_when_gitmodules_present() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_submodules\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&[".gitmodules", "README.md"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 1);
    }
}
