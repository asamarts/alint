//! `git_commit_signed_off` — assert every commit in scope carries a
//! DCO `Signed-off-by:` trailer.
//!
//! The Developer Certificate of Origin sign-off is required by every
//! CNCF / Linux Foundation / kernel-style project (kubernetes, istio,
//! helm, opentelemetry, prometheus, containerd, runc, etcd, …). A
//! commit lacking the trailer fires one violation.
//!
//! Shares the commit-validation family shape (the `commit_range` module):
//! `since:` unset checks HEAD only (push-trigger / hook); `since:` set
//! checks `<since>..HEAD` (PR-CI). Outside a git repo, with no
//! commits, or when `git` is unavailable, silently no-ops. A bad
//! `since:` ref hard-fails with a shallow-clone hint.
//!
//! Check-only — alint can't rewrite history to add a trailer.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Violation};
use regex::Regex;
use serde::Deserialize;

use crate::commit_range::{collect_commits, format_commit_violation};

/// Default trailer pattern: the canonical DCO sign-off line. `(?m)`
/// so `^…$` matches a trailer line anywhere in the message body, not
/// just the whole message. Matches `Signed-off-by: Name <email>`.
const DEFAULT_PATTERN: &str = r"(?m)^Signed-off-by: .+ <.+@.+>$";

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Trailer pattern each commit message must contain. Defaults to the
    /// canonical DCO sign-off shape `(?m)^Signed-off-by: .+ <.+@.+>$`. Override
    /// to enforce a stricter form (e.g. a corporate-domain email).
    #[serde(default)]
    pattern: Option<String>,
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
pub struct GitCommitSignedOffRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message_override: Option<String>,
    pattern: Regex,
    since_raw: Option<String>,
    include_merges: bool,
}

impl Rule for GitCommitSignedOffRule {
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
            if !self.pattern.is_match(&commit.message) {
                let msg = self.message_override.clone().unwrap_or_else(|| {
                    format_commit_violation(commit, "is missing a `Signed-off-by:` trailer")
                });
                // No path; one finding per commit → key on the commit id.
                violations.push(Violation::new(msg).with_baseline_key(commit.sha.clone()));
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
            "git_commit_signed_off has no fix op",
        ));
    }
    if opts.include_merges && opts.since.is_none() {
        return Err(Error::rule_config(
            &spec.id,
            "`include_merges: true` has no effect without `since:`. Either remove it \
             or set `since:` to enable range mode.",
        ));
    }
    let pattern_src = opts.pattern.as_deref().unwrap_or(DEFAULT_PATTERN);
    let pattern = Regex::new(pattern_src).map_err(|e| {
        Error::rule_config(
            &spec.id,
            format!("invalid `pattern:` regex `{pattern_src}`: {e}"),
        )
    })?;

    Ok(Box::new(GitCommitSignedOffRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message_override: spec.message.clone(),
        pattern,
        since_raw: opts.since,
        include_merges: opts.include_merges,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(toml: &str) -> RuleSpec {
        let mut full =
            String::from("id = \"dco\"\nkind = \"git_commit_signed_off\"\nlevel = \"error\"\n");
        full.push_str(toml);
        toml::from_str(&full).unwrap()
    }

    #[test]
    fn default_pattern_matches_canonical_dco_trailer() {
        let re = Regex::new(DEFAULT_PATTERN).unwrap();
        assert!(re.is_match("feat: thing\n\nSigned-off-by: Jane Doe <jane@example.com>"));
        // No trailer → no match.
        assert!(!re.is_match("feat: thing\n\nbody with no trailer"));
        // Trailer with no email angle brackets → no match.
        assert!(!re.is_match("feat: thing\n\nSigned-off-by: Jane Doe"));
    }

    #[test]
    fn build_accepts_default_and_rejects_fix() {
        assert!(build(&spec("")).is_ok());
        assert!(build(&spec("fix = { file_create = { content = \"x\" } }\n")).is_err());
    }

    #[test]
    fn build_rejects_include_merges_without_since() {
        let err = build(&spec("include_merges = true\n")).unwrap_err();
        assert!(err.to_string().contains("include_merges"), "{err}");
    }

    #[test]
    fn build_rejects_invalid_pattern() {
        let err = build(&spec("pattern = \"[unclosed\"\n")).unwrap_err();
        assert!(err.to_string().contains("regex"), "{err}");
    }
}
