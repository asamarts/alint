//! `file_absent` — emit a violation for every file matching `paths`.

use alint_core::{
    Context, Error, FixSpec, Fixer, Level, PathsSpec, Result, Rule, RuleSpec, Scope, Violation,
};

use crate::fixers::FileRemoveFixer;

#[derive(Debug)]
pub struct FileAbsentRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    patterns: Vec<String>,
    /// When `true`, only fire on entries that are also tracked
    /// in git's index. Outside a git repo or with no rules
    /// opting in, the tracked-set is `None` and every entry
    /// reads as "untracked," so the rule becomes a no-op —
    /// which is the right default for "don't let X be
    /// committed" semantics.
    git_tracked_only: bool,
    fixer: Option<FileRemoveFixer>,
}

impl Rule for FileAbsentRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        self.level
    }
    fn policy_url(&self) -> Option<&str> {
        self.policy_url.as_deref()
    }
    fn wants_git_tracked(&self) -> bool {
        self.git_tracked_only
    }

    fn requires_full_index(&self) -> bool {
        // The verdict on "is X forbidden?" is over the whole tree —
        // an unchanged-but-already-committed `.env` should still
        // be visible. The engine skips this rule entirely when its
        // scope doesn't intersect the diff, which is the usual
        // user expectation in `--changed` mode.
        true
    }

    fn path_scope(&self) -> Option<&Scope> {
        Some(&self.scope)
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path, ctx.index) {
                continue;
            }
            if self.git_tracked_only && !ctx.is_git_tracked(&entry.path) {
                continue;
            }
            let msg = self.message.clone().unwrap_or_else(|| {
                let tracked = if self.git_tracked_only {
                    " and tracked in git"
                } else {
                    ""
                };
                format!(
                    "file is forbidden (matches [{}]{tracked}): {}",
                    self.patterns.join(", "),
                    entry.path.display()
                )
            });
            violations.push(Violation::new(msg).with_path(entry.path.clone()));
        }
        Ok(violations)
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    alint_core::reject_scope_filter_on_cross_file(spec, "file_absent")?;
    let Some(paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "file_absent requires a `paths` field",
        ));
    };
    let fixer = match &spec.fix {
        Some(FixSpec::FileRemove { .. }) => Some(FileRemoveFixer),
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!("fix.{} is not compatible with file_absent", other.op_name()),
            ));
        }
        None => None,
    };
    Ok(Box::new(FileAbsentRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        patterns: patterns_of(paths),
        git_tracked_only: spec.git_tracked_only,
        fixer,
    }))
}

fn patterns_of(spec: &PathsSpec) -> Vec<String> {
    match spec {
        PathsSpec::Single(s) => vec![s.clone()],
        PathsSpec::Many(v) => v.clone(),
        PathsSpec::IncludeExclude { include, .. } => include.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, index, spec_yaml};
    use std::path::Path;

    #[test]
    fn build_rejects_missing_paths_field() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_absent\n\
             level: error\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(err.contains("paths"), "unexpected: {err}");
    }

    #[test]
    fn build_rejects_incompatible_fix_op() {
        // file_absent supports `file_remove` only; any other
        // op surfaces a config error so a typo doesn't silently
        // disable the fix path.
        let spec = spec_yaml(
            "id: t\n\
             kind: file_absent\n\
             paths: \"*.bak\"\n\
             level: error\n\
             fix:\n  \
               file_create:\n    \
                 content: \"\"\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(err.contains("file_create"), "unexpected: {err}");
    }

    #[test]
    fn build_accepts_file_remove_fix() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_absent\n\
             paths: \"*.bak\"\n\
             level: error\n\
             fix:\n  \
               file_remove: {}\n",
        );
        let rule = build(&spec).expect("valid file_remove fix");
        assert!(rule.fixer().is_some(), "fixer should be present");
    }

    #[test]
    fn evaluate_passes_when_no_match_present() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_absent\n\
             paths: \"*.bak\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["src/main.rs", "README.md"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert!(v.is_empty(), "unexpected: {v:?}");
    }

    #[test]
    fn evaluate_fires_one_violation_per_match() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_absent\n\
             paths: \"**/*.bak\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["a.bak", "src/b.bak", "ok.txt"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 2, "expected one violation per .bak: {v:?}");
    }

    #[test]
    fn evaluate_silent_when_git_tracked_only_outside_repo() {
        // git_tracked_only requires `ctx.git_tracked` to be
        // populated; when it's None (no rule asked for it / no
        // git repo), every path reads as "untracked" and the
        // rule no-ops — the right default for "don't let X be
        // committed" semantics.
        let spec = spec_yaml(
            "id: t\n\
             kind: file_absent\n\
             paths: \"*.bak\"\n\
             level: error\n\
             git_tracked_only: true\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["a.bak"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert!(
            v.is_empty(),
            "git_tracked_only without ctx.git_tracked must no-op: {v:?}",
        );
    }

    #[test]
    fn rule_advertises_full_index_requirement() {
        // Existence-axis rules opt out of changed-mode
        // filtering — an unchanged-but-already-committed `.env`
        // should still fire.
        let spec = spec_yaml(
            "id: t\n\
             kind: file_absent\n\
             paths: \".env\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        assert!(rule.requires_full_index());
    }

    #[test]
    fn build_rejects_scope_filter_on_cross_file_rule() {
        // file_absent is a cross-file rule (requires_full_index =
        // true); scope_filter is per-file-rules-only. The build
        // path must reject it with a clear message pointing at
        // the for_each_dir + when_iter: alternative.
        let yaml = r#"
id: t
kind: file_absent
paths: "*.bak"
level: error
scope_filter:
  has_ancestor: Cargo.toml
"#;
        let spec = spec_yaml(yaml);
        let err = build(&spec).unwrap_err().to_string();
        assert!(
            err.contains("scope_filter is supported on per-file rules only"),
            "expected per-file-only message, got: {err}",
        );
        assert!(
            err.contains("file_absent"),
            "expected message to name the cross-file kind, got: {err}",
        );
    }
}
