//! `git_commit_subject_matches` — each commit's subject line (the
//! first line of its message) must match a regex.
//!
//! The subject-grammar member of the commit-validation family
//! (`git_commit_signed_off`, `git_commit_no_fixup`, …): enforces a
//! prefix + shape convention like `pkg/path: lowercase summary`
//! (go / Gerrit), `subsystem: description` (node), or
//! conventional-commit types. Unlike `git_commit_message`'s
//! `pattern:` (which matches the whole subject + body), `matches:`
//! is anchored to the **subject alone**, so `^…$` describes the
//! first line exactly. For a subject-length cap use
//! `git_commit_message`'s `subject_max_length:`.
//!
//! Shares the family shape (the `commit_range` module): `since:`
//! unset checks HEAD only; `since:` set checks `<since>..HEAD`,
//! oldest-first, merge commits excluded unless `include_merges:`.
//! Silent outside a git repo / with no commits; a bad `since:` ref
//! hard-fails with a shallow-clone hint. `since:`'s `{{env.X}}`
//! interpolation is resolved at config load by `alint-dsl`.
//!
//! Check-only — alint can't rewrite the user's commit history.

use alint_core::git::CommitRecord;
use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Violation};
use regex::Regex;
use serde::Deserialize;

use crate::commit_range::{collect_commits, format_commit_violation};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Rust-regex the commit subject (first line of the message) must match.
    matches: String,
    /// Git ref to use as the base of the commit range. When set, validates
    /// every commit in `<since>..HEAD` instead of just HEAD.
    #[serde(default)]
    since: Option<String>,
    /// When validating a range (`since:` set), include merge commits. Has no
    /// effect when `since:` is unset.
    #[serde(default)]
    include_merges: bool,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct GitCommitSubjectMatchesRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message_override: Option<String>,
    matches: Regex,
    since_raw: Option<String>,
    include_merges: bool,
}

impl Rule for GitCommitSubjectMatchesRule {
    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let commits = collect_commits(
            ctx,
            self.since_raw.as_deref(),
            self.include_merges,
            &self.id,
        )?;
        Ok(commits.iter().filter_map(|c| self.check_one(c)).collect())
    }
}

impl GitCommitSubjectMatchesRule {
    /// Match a single commit's **subject** (first line of the
    /// message) against `matches:`. The body is never consulted, so a
    /// passing subject with a non-conforming body line is clean and a
    /// failing subject is not rescued by a conforming body line.
    fn check_one(&self, commit: &CommitRecord) -> Option<Violation> {
        let subject = commit.message.split('\n').next().unwrap_or("");
        if self.matches.is_match(subject) {
            return None;
        }
        let msg = self.message_override.clone().unwrap_or_else(|| {
            format_commit_violation(
                commit,
                &format!("subject does not match `{}`", self.matches.as_str()),
            )
        });
        Some(Violation::new(msg))
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "git_commit_subject_matches has no fix op",
        ));
    }
    if opts.include_merges && opts.since.is_none() {
        return Err(Error::rule_config(
            &spec.id,
            "`include_merges: true` has no effect without `since:`. Either remove it \
             or set `since:` to enable range mode.",
        ));
    }
    let matches = Regex::new(&opts.matches).map_err(|e| {
        Error::rule_config(
            &spec.id,
            format!("invalid `matches:` regex `{}`: {e}", opts.matches),
        )
    })?;

    Ok(Box::new(GitCommitSubjectMatchesRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message_override: spec.message.clone(),
        matches,
        since_raw: opts.since,
        include_merges: opts.include_merges,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(toml: &str) -> RuleSpec {
        let mut full = String::from(
            "id = \"subject-grammar\"\nkind = \"git_commit_subject_matches\"\nlevel = \"error\"\n",
        );
        full.push_str(toml);
        toml::from_str(&full).unwrap()
    }

    /// Construct the concrete rule directly so tests can drive
    /// `check_one` — the real per-commit path — without a git repo.
    fn match_rule(re: &str) -> GitCommitSubjectMatchesRule {
        GitCommitSubjectMatchesRule {
            id: "subject-grammar".into(),
            level: Level::Error,
            policy_url: None,
            message_override: None,
            matches: Regex::new(re).unwrap(),
            since_raw: None,
            include_merges: false,
        }
    }

    fn record(message: &str) -> CommitRecord {
        CommitRecord {
            sha: "abc1234".into(),
            message: message.into(),
            author_name: String::new(),
            author_email: String::new(),
        }
    }

    #[test]
    fn build_accepts_minimal_and_rejects_fix() {
        assert!(build(&spec("matches = \"^[a-z]+: \"\n")).is_ok());
        assert!(
            build(&spec(
                "matches = \"^x\"\nfix = { file_create = { content = \"x\" } }\n"
            ))
            .is_err()
        );
    }

    #[test]
    fn build_requires_matches() {
        // `matches:` is the one required field.
        assert!(build(&spec("")).is_err());
    }

    #[test]
    fn build_rejects_invalid_regex() {
        let err = build(&spec("matches = \"(unclosed\"\n")).unwrap_err();
        assert!(err.to_string().contains("regex"), "{err}");
    }

    #[test]
    fn build_rejects_include_merges_without_since() {
        let err = build(&spec("matches = \"^x\"\ninclude_merges = true\n")).unwrap_err();
        assert!(err.to_string().contains("include_merges"), "{err}");
    }

    #[test]
    fn subject_is_matched_against_the_first_line_only() {
        // Exercises `check_one` (the body of `evaluate`'s per-commit
        // loop), so it locks the rule's real anchoring behaviour
        // rather than a regex constructed inline by the test.
        let rule = match_rule(r"^[a-z0-9_/.-]+: [a-z].{0,70}$");

        // Subject conforms; a body line that does NOT conform must
        // be ignored — the regex is anchored to the subject alone.
        let ok = record("pkg/net: add a thing\n\nWIP: Capitalised body that fails the grammar");
        assert!(
            rule.check_one(&ok).is_none(),
            "a conforming subject is clean regardless of the body"
        );

        // Subject does NOT conform; a later body line that WOULD
        // conform must not rescue it — we never scan the body.
        let bad = record("WIP messy subject\n\nfeat: tidy body line");
        let v = rule
            .check_one(&bad)
            .expect("a non-conforming subject fires");
        assert!(
            v.message.contains("does not match"),
            "message names the mismatch: {}",
            v.message
        );
    }
}
