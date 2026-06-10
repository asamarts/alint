//! `git_commit_gpg_signed` — assert every commit in scope has a
//! verifying signature (`git verify-commit` exits 0).
//!
//! Demand: kernel maintainers, security-sensitive OSS, anyone using
//! GitHub's "Require signed commits" branch protection. A commit that
//! is unsigned — or signed with a key that doesn't verify against the
//! local keyring — fires one violation.
//!
//! The rule reflects git's own verdict; it deliberately does NOT
//! distinguish "unsigned" from "signed with an untrusted key" —
//! trust is git's GPG config / `.git/allowed_signers`, not this
//! rule's job.
//!
//! Shares the commit-validation family shape (the `commit_range`
//! module): `since:` unset checks HEAD only; `since:` set checks
//! `<since>..HEAD`. Silent outside a git repo; a bad `since:` ref
//! hard-fails with a shallow-clone hint.
//!
//! Check-only — alint can't sign the user's commits.

use alint_core::git::verify_commit;
use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Violation};
use serde::Deserialize;

use crate::commit_range::{collect_commits, format_commit_violation};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Git ref to use as the base of the commit range. When set, validates
    /// every commit in `<since>..HEAD` instead of just HEAD. Accepts anything
    /// `git rev-parse` does. Use the canonical `{{env.X}}` interpolation to
    /// pass a SHA via an env var, e.g.
    /// `since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"`.
    #[serde(default)]
    since: Option<String>,
    /// When validating a range (`since:` set), include merge commits. Has no
    /// effect when `since:` is unset; combining `include_merges: true` with no
    /// `since:` is a load-time error.
    #[serde(default)]
    include_merges: bool,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct GitCommitGpgSignedRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message_override: Option<String>,
    since_raw: Option<String>,
    include_merges: bool,
}

impl Rule for GitCommitGpgSignedRule {
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
            // `Some(false)` = git says the signature doesn't verify
            // (unsigned or untrusted). `None` = git vanished mid-run;
            // don't fire on inability to check (advisory posture).
            if verify_commit(ctx.root, &commit.sha) == Some(false) {
                let msg = self.message_override.clone().unwrap_or_else(|| {
                    format_commit_violation(
                        commit,
                        "is not signed (or its signature did not verify)",
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
            "git_commit_gpg_signed has no fix op",
        ));
    }
    if opts.include_merges && opts.since.is_none() {
        return Err(Error::rule_config(
            &spec.id,
            "`include_merges: true` has no effect without `since:`. Either remove it \
             or set `since:` to enable range mode.",
        ));
    }

    Ok(Box::new(GitCommitGpgSignedRule {
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
            String::from("id = \"signed\"\nkind = \"git_commit_gpg_signed\"\nlevel = \"error\"\n");
        full.push_str(toml);
        toml::from_str(&full).unwrap()
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
