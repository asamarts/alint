//! `git_commit_author_allowlist` — assert every commit author matches
//! an allowed email and/or name pattern.
//!
//! Demand: enterprise repos enforcing contributor identity against a
//! corporate domain; OSS projects catching commits from sock-puppet
//! or compromised accounts. At least one of `email_pattern:` /
//! `name_pattern:` is required; specifying both means BOTH must match
//! (AND). A commit whose author fails any specified pattern fires one
//! violation.
//!
//! Shares the commit-validation family shape (the `commit_range`
//! module): `since:` unset checks HEAD only; `since:` set checks
//! `<since>..HEAD`. Silent outside a git repo; a bad `since:` ref
//! hard-fails with a shallow-clone hint.
//!
//! Check-only — alint can't rewrite commit authorship.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Violation};
use regex::Regex;
use serde::Deserialize;

use crate::commit_range::{collect_commits, format_commit_violation};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Rust-regex the author email (`git log %ae`) must match, e.g.
    /// `^.+@example\.com$`.
    #[serde(default)]
    email_pattern: Option<String>,
    /// Rust-regex the author name (`git log %an`) must match.
    #[serde(default)]
    name_pattern: Option<String>,
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
pub struct GitCommitAuthorAllowlistRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message_override: Option<String>,
    email_pattern: Option<Regex>,
    name_pattern: Option<Regex>,
    since_raw: Option<String>,
    include_merges: bool,
}

impl Rule for GitCommitAuthorAllowlistRule {
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
            let email_bad = self
                .email_pattern
                .as_ref()
                .is_some_and(|re| !re.is_match(&commit.author_email));
            let name_bad = self
                .name_pattern
                .as_ref()
                .is_some_and(|re| !re.is_match(&commit.author_name));
            if email_bad || name_bad {
                let msg = self.message_override.clone().unwrap_or_else(|| {
                    format_commit_violation(
                        commit,
                        &format!(
                            "author \"{} <{}>\" is not in the allowlist",
                            commit.author_name, commit.author_email
                        ),
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
            "git_commit_author_allowlist has no fix op",
        ));
    }
    if opts.email_pattern.is_none() && opts.name_pattern.is_none() {
        return Err(Error::rule_config(
            &spec.id,
            "git_commit_author_allowlist needs at least one of `email_pattern:` or `name_pattern:`",
        ));
    }
    if opts.include_merges && opts.since.is_none() {
        return Err(Error::rule_config(
            &spec.id,
            "`include_merges: true` has no effect without `since:`. Either remove it \
             or set `since:` to enable range mode.",
        ));
    }
    let compile = |src: Option<String>, field: &str| -> Result<Option<Regex>> {
        src.map(|p| {
            Regex::new(&p).map_err(|e| {
                Error::rule_config(&spec.id, format!("invalid `{field}` regex `{p}`: {e}"))
            })
        })
        .transpose()
    };

    Ok(Box::new(GitCommitAuthorAllowlistRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message_override: spec.message.clone(),
        email_pattern: compile(opts.email_pattern, "email_pattern")?,
        name_pattern: compile(opts.name_pattern, "name_pattern")?,
        since_raw: opts.since,
        include_merges: opts.include_merges,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(toml: &str) -> RuleSpec {
        let mut full = String::from(
            "id = \"authors\"\nkind = \"git_commit_author_allowlist\"\nlevel = \"error\"\n",
        );
        full.push_str(toml);
        toml::from_str(&full).unwrap()
    }

    #[test]
    fn build_requires_at_least_one_pattern() {
        let err = build(&spec("")).unwrap_err();
        assert!(err.to_string().contains("at least one"), "{err}");
    }

    #[test]
    fn build_accepts_email_pattern_and_rejects_fix() {
        assert!(build(&spec("email_pattern = '^.+@example\\.com$'\n")).is_ok());
        assert!(build(&spec("name_pattern = '.+'\n")).is_ok());
        assert!(
            build(&spec(
                "email_pattern = '.+'\nfix = { file_create = { content = \"x\" } }\n"
            ))
            .is_err()
        );
    }

    #[test]
    fn build_rejects_invalid_regex() {
        let err = build(&spec("email_pattern = '[unclosed'\n")).unwrap_err();
        assert!(err.to_string().contains("regex"), "{err}");
    }

    #[test]
    fn build_rejects_include_merges_without_since() {
        let err = build(&spec("email_pattern = '.+'\ninclude_merges = true\n")).unwrap_err();
        assert!(err.to_string().contains("include_merges"), "{err}");
    }
}
