//! `pair` — for every file matching `primary`, require a file matching the
//! `partner` template to exist somewhere in the tree.
//!
//! The partner template is a path string with `{dir}`, `{stem}`, `{ext}`,
//! `{basename}`, `{path}`, `{parent_name}` substitutions derived from the
//! primary match. The resolved partner path is looked up in the engine's
//! `FileIndex`; a missing match emits a violation anchored on the primary.
//!
//! Example (every `.c` needs a same-directory `.h`):
//!
//! ```yaml
//! - id: c-requires-h
//!   kind: pair
//!   primary: "**/*.c"
//!   partner: "{dir}/{stem}.h"
//!   level: error
//!   message: "{{ctx.primary}} has no header at {{ctx.partner}}"
//! ```

use std::path::{Path, PathBuf};

use alint_core::template::{PathTokens, render_message, render_path};
use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    primary: String,
    partner: String,
}

#[derive(Debug)]
pub struct PairRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    primary_scope: Scope,
    partner_template: String,
}

impl Rule for PairRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        self.level
    }
    fn policy_url(&self) -> Option<&str> {
        self.policy_url.as_deref()
    }

    fn requires_full_index(&self) -> bool {
        // Cross-file: a verdict on a primary file depends on
        // whether its partner exists *anywhere* in the tree, not
        // just in the diff. Roadmap §"Monorepo & scale" defines
        // this rule as opting out of `--changed` filtering. We
        // also leave `path_scope` as `None` so the engine doesn't
        // skip-by-intersection — pair always evaluates.
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        for entry in ctx.index.files() {
            if !self.primary_scope.matches(&entry.path) {
                continue;
            }
            let tokens = PathTokens::from_path(&entry.path);
            let partner_rel = render_path(&self.partner_template, &tokens);
            if partner_rel.is_empty() {
                violations.push(
                    Violation::new(format!(
                        "partner template {:?} resolved to an empty path for {}",
                        self.partner_template,
                        entry.path.display(),
                    ))
                    .with_path(entry.path.clone()),
                );
                continue;
            }
            let partner_path = PathBuf::from(&partner_rel);
            if resolves_to_self(&partner_path, &entry.path) {
                violations.push(
                    Violation::new(format!(
                        "partner template {:?} resolves to the primary file itself ({}) — \
                         check that the template differs from the primary",
                        self.partner_template,
                        entry.path.display(),
                    ))
                    .with_path(entry.path.clone()),
                );
                continue;
            }
            if ctx.index.find_file(&partner_path).is_some() {
                continue;
            }
            let message = self.format_message(&entry.path, &partner_path);
            violations.push(Violation::new(message).with_path(entry.path.clone()));
        }
        Ok(violations)
    }
}

fn resolves_to_self(partner: &Path, primary: &Path) -> bool {
    partner == primary
}

impl PairRule {
    fn format_message(&self, primary: &Path, partner: &Path) -> String {
        let primary_str = primary.display().to_string();
        let partner_str = partner.display().to_string();
        if let Some(user_msg) = self.message.as_deref() {
            return render_message(user_msg, |ns, key| match (ns, key) {
                ("ctx", "primary") => Some(primary_str.clone()),
                ("ctx", "partner") => Some(partner_str.clone()),
                _ => None,
            });
        }
        format!(
            "{} has no matching partner at {} (template: {})",
            primary_str, partner_str, self.partner_template,
        )
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.partner.trim().is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "pair `partner` template must not be empty",
        ));
    }
    let primary_patterns = vec![opts.primary.clone()];
    let primary_scope = Scope::from_patterns(&primary_patterns)?;
    Ok(Box::new(PairRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        primary_scope,
        partner_template: opts.partner,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{FileEntry, FileIndex};
    use std::path::Path;

    fn idx(paths: &[&str]) -> FileIndex {
        FileIndex {
            entries: paths
                .iter()
                .map(|p| FileEntry {
                    path: std::path::Path::new(p).into(),
                    is_dir: false,
                    size: 1,
                })
                .collect(),
        }
    }

    fn rule(primary: &str, partner: &str, message: Option<&str>) -> PairRule {
        PairRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: message.map(ToString::to_string),
            primary_scope: Scope::from_patterns(&[primary.to_string()]).unwrap(),
            partner_template: partner.into(),
        }
    }

    fn eval(rule: &PairRule, files: &[&str]) -> Vec<Violation> {
        let index = idx(files);
        let ctx = Context {
            root: Path::new("/"),
            index: &index,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
        };
        rule.evaluate(&ctx).unwrap()
    }

    #[test]
    fn passes_when_partner_exists() {
        let r = rule("**/*.c", "{dir}/{stem}.h", None);
        let v = eval(&r, &["src/mod/foo.c", "src/mod/foo.h"]);
        assert!(v.is_empty(), "unexpected: {v:?}");
    }

    #[test]
    fn violates_when_partner_missing() {
        let r = rule("**/*.c", "{dir}/{stem}.h", None);
        let v = eval(&r, &["src/mod/foo.c"]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].path.as_deref(), Some(Path::new("src/mod/foo.c")));
        assert!(v[0].message.contains("src/mod/foo.h"));
    }

    #[test]
    fn violates_per_missing_primary() {
        let r = rule("**/*.c", "{dir}/{stem}.h", None);
        let v = eval(
            &r,
            &[
                "src/mod/foo.c",
                "src/mod/foo.h", // has partner — OK
                "src/mod/bar.c", // no bar.h
                "src/mod/baz.c", // no baz.h
            ],
        );
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn no_primary_matches_means_no_violation() {
        let r = rule("**/*.c", "{dir}/{stem}.h", None);
        let v = eval(&r, &["README.md", "src/mod/other.rs"]);
        assert!(v.is_empty());
    }

    #[test]
    fn user_message_with_ctx_substitution() {
        let r = rule(
            "**/*.c",
            "{dir}/{stem}.h",
            Some("missing header {{ctx.partner}} for {{ctx.primary}}"),
        );
        let v = eval(&r, &["src/foo.c"]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].message, "missing header src/foo.h for src/foo.c");
    }

    #[test]
    fn rejects_partner_resolving_to_self() {
        // Partner template evaluates to the same file as the primary — caught
        // as a config/authorship mistake rather than silently passing.
        let r = rule("**/*.c", "{path}", None);
        let v = eval(&r, &["src/foo.c"]);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("primary file itself"));
    }

    #[test]
    fn empty_partner_after_substitution_is_a_violation() {
        // Template yields "" (only unknown-stripped content would). This
        // exercises the empty-partner guard.
        let r = rule("**/*.c", "", None);
        // Guard is in build(); direct construction bypasses it, so the runtime
        // guard in evaluate() catches this.
        let v = eval(&r, &["src/foo.c"]);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("empty path"));
    }
}
