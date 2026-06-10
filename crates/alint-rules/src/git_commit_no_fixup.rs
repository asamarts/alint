//! `git_commit_no_fixup` — fail on residual `fixup!` / `squash!` /
//! `amend!` commits left in the range.
//!
//! `git commit --fixup` / `--squash` / `--fixup=amend:` produce
//! subjects prefixed `fixup! ` / `squash! ` / `amend! `, meant to be
//! collapsed by `git rebase --autosquash` before merging. Forgetting
//! to rebase is the universal case; this rule catches the leftover.
//!
//! Shares the commit-validation family shape (the `commit_range`
//! module): `since:` unset checks HEAD only; `since:` set checks
//! `<since>..HEAD`. Silent outside a git repo; a bad `since:` ref
//! hard-fails with a shallow-clone hint. No configuration knobs — the
//! matched prefixes are exactly what `--autosquash` understands.
//!
//! Check-only — alint can't rebase the user's history.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Violation};
use serde::Deserialize;

use crate::commit_range::{collect_commits, format_commit_violation};

/// Subject prefixes `git rebase --autosquash` recognises.
const FIXUP_PREFIXES: &[&str] = &["fixup! ", "squash! ", "amend! "];

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Git ref to use as the base of the commit range. When set, validates
    /// every commit in `<since>..HEAD` instead of just HEAD. Accepts anything
    /// `git rev-parse` does.
    #[serde(default)]
    since: Option<String>,
    /// When validating a range (`since:` set), include merge commits. Has no
    /// effect when `since:` is unset.
    #[serde(default)]
    include_merges: bool,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct GitCommitNoFixupRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message_override: Option<String>,
    since_raw: Option<String>,
    include_merges: bool,
}

impl Rule for GitCommitNoFixupRule {
    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        let commits = collect_commits(
            ctx,
            self.since_raw.as_deref(),
            self.include_merges,
            &self.id,
        )?;
        for commit in &commits {
            let subject = commit.message.split('\n').next().unwrap_or("");
            if FIXUP_PREFIXES.iter().any(|p| subject.starts_with(p)) {
                let msg = self.message_override.clone().unwrap_or_else(|| {
                    format_commit_violation(
                        commit,
                        "is a fixup/squash/amend commit; rebase with `--autosquash` before merging",
                    )
                });
                violations.push(Violation::new(msg));
            }
        }
        Ok(violations)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "git_commit_no_fixup has no fix op",
        ));
    }
    if opts.include_merges && opts.since.is_none() {
        return Err(Error::rule_config(
            &spec.id,
            "`include_merges: true` has no effect without `since:`. Either remove it \
             or set `since:` to enable range mode.",
        ));
    }

    Ok(Box::new(GitCommitNoFixupRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message_override: spec.message.clone(),
        since_raw: opts.since,
        include_merges: opts.include_merges,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(toml: &str) -> RuleSpec {
        let mut full =
            String::from("id = \"no-fixup\"\nkind = \"git_commit_no_fixup\"\nlevel = \"error\"\n");
        full.push_str(toml);
        toml::from_str(&full).unwrap()
    }

    #[test]
    fn fixup_prefixes_are_recognised() {
        for s in ["fixup! real subject", "squash! x", "amend! y"] {
            assert!(
                FIXUP_PREFIXES.iter().any(|p| s.starts_with(p)),
                "{s:?} should be a fixup-shaped subject"
            );
        }
        // A subject that merely contains the word is not a prefix.
        assert!(
            !FIXUP_PREFIXES
                .iter()
                .any(|p| "fix: a fixup typo".starts_with(p))
        );
    }

    #[test]
    fn build_accepts_minimal_and_rejects_fix() {
        assert!(build(&spec("")).is_ok());
        assert!(build(&spec("fix = { file_create = { content = \"x\" } }\n")).is_err());
    }

    #[test]
    fn build_rejects_include_merges_without_since() {
        let err = build(&spec("include_merges = true\n")).unwrap_err();
        assert!(err.to_string().contains("include_merges"), "{err}");
    }
}
