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
