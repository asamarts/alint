//! `no_case_conflicts` — flag two paths that differ only by
//! case (e.g. `README.md` + `readme.md`). Such pairs cannot
//! coexist on case-insensitive filesystems (macOS HFS+/APFS
//! default, Windows NTFS in its default mode), so committing
//! them breaks checkouts for those developers.
//!
//! Check-only — renaming which one to keep is a human decision.

use std::collections::BTreeMap;
use std::path::PathBuf;

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};

#[derive(Debug)]
pub struct NoCaseConflictsRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
}

impl Rule for NoCaseConflictsRule {
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
        // Group paths by their lowercased form.
        let mut groups: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path) {
                continue;
            }
            let Some(as_str) = entry.path.to_str() else {
                continue;
            };
            groups
                .entry(as_str.to_ascii_lowercase())
                .or_default()
                .push(entry.path.clone());
        }
        let mut violations = Vec::new();
        for (_lower, paths) in groups {
            if paths.len() < 2 {
                continue;
            }
            let names: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
            for p in &paths {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "case-insensitive collision: {} (collides with: {})",
                        p.display(),
                        names
                            .iter()
                            .filter(|n| *n != &p.display().to_string())
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                });
                violations.push(Violation::new(msg).with_path(p));
            }
        }
        Ok(violations)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(
            &spec.id,
            "no_case_conflicts requires a `paths` field (often `\"**\"`)",
        )
    })?;
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "no_case_conflicts has no fix op — renaming which path to keep is a human decision",
        ));
    }
    Ok(Box::new(NoCaseConflictsRule {
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
    use crate::test_support::{ctx, index, spec_yaml};
    use std::path::Path;

    #[test]
    fn build_rejects_missing_paths_field() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_case_conflicts\n\
             level: warning\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn build_rejects_fix_block() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_case_conflicts\n\
             paths: \"**\"\n\
             level: warning\n\
             fix:\n  \
               file_remove: {}\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn evaluate_passes_when_paths_unique_after_lowercase() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_case_conflicts\n\
             paths: \"**\"\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let i = index(&["README.md", "src/main.rs", "Cargo.toml"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &i)).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn evaluate_fires_one_violation_per_collision_member() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_case_conflicts\n\
             paths: \"**\"\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        // README.md and readme.md collide → both emitted.
        let i = index(&["README.md", "readme.md", "Cargo.toml"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &i)).unwrap();
        assert_eq!(v.len(), 2, "two collision members should fire");
    }

    #[test]
    fn evaluate_fires_on_three_way_collision() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_case_conflicts\n\
             paths: \"**\"\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let i = index(&["README.md", "readme.md", "ReadMe.md"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &i)).unwrap();
        assert_eq!(v.len(), 3, "three collision members should fire");
    }
}
