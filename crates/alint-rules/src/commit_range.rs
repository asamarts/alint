//! Shared infrastructure for the commit-validation rule family
//! (`git_commit_signed_off`, `git_commit_no_fixup`, …).
//!
//! Every rule in the family takes `since:` + `include_merges:`, emits
//! one violation per failing commit (abbreviated SHA + subject
//! snippet), silently no-ops outside a git repo, and hard-fails on an
//! unresolvable `since:` ref with a shallow-clone hint. This module
//! centralises the head-or-range fetch and the per-commit violation
//! formatting so each rule body stays thin.
//!
//! `since:` arrives already resolved: the canonical v0.11 `{{env.X}}`
//! interpolation is performed at config load by `alint-dsl`, so these
//! rules — unlike the legacy `git_commit_message` — carry no
//! `${VAR}` expansion of their own.

use alint_core::git::{
    CommitRangeError, CommitRecord, commit_messages_in_range, head_commit_record,
};
use alint_core::{Context, Error, Result};

/// Resolve the commits a commit-validation rule should check.
///
/// - `since` `None` → HEAD only (the push-trigger / commit-hook
///   shape). Synthesises a single record with the SHA `"HEAD"`.
/// - `since` `Some` → the `<since>..HEAD` range (the PR-CI shape),
///   oldest first, merge commits excluded unless `include_merges`.
///
/// Returns an empty vec (silent no-op) outside a git repo, with no
/// commits, or when `git` is unavailable. A `since:` ref that fails
/// to resolve is an [`Error::rule_config`] with the shallow-clone
/// hint, so the user sees the misconfiguration instead of a silently
/// empty range.
pub(crate) fn collect_commits(
    ctx: &Context<'_>,
    since: Option<&str>,
    include_merges: bool,
    rule_id: &str,
) -> Result<Vec<CommitRecord>> {
    match since {
        None => Ok(head_commit_record(ctx.root)
            .map(|record| vec![record])
            .unwrap_or_default()),
        Some(since) => match commit_messages_in_range(ctx.root, since, include_merges) {
            Ok(None) => Ok(Vec::new()),
            Ok(Some(records)) => Ok(records),
            Err(CommitRangeError::BadRange { stderr }) => Err(Error::rule_config(
                rule_id,
                format!(
                    "could not resolve commit range `{since}..HEAD`: {stderr}. Common cause: \
                     shallow clone. In a GitHub Actions PR workflow, use `actions/checkout@v4` \
                     with `fetch-depth: 0` so the base ref is reachable."
                ),
            )),
        },
    }
}

/// Render a per-commit violation message: the commit SHA plus a
/// trimmed subject snippet for context. The SHA is `"HEAD"` in
/// single-commit mode and an abbreviated SHA in range mode.
pub(crate) fn format_commit_violation(commit: &CommitRecord, what: &str) -> String {
    const SUBJECT_PREVIEW_MAX: usize = 60;
    let subject = commit.message.split('\n').next().unwrap_or("");
    let preview: String = subject.chars().take(SUBJECT_PREVIEW_MAX).collect();
    let ellipsis = if subject.chars().count() > SUBJECT_PREVIEW_MAX {
        "…"
    } else {
        ""
    };
    format!(
        "commit {}: {what} (subject: \"{preview}{ellipsis}\")",
        commit.sha
    )
}

/// The shared "could not resolve `<since>...HEAD`" diff-range error
/// (with the shallow-clone hint) for the changeset rules
/// (`changeset_requires_path` and `pair_changed_together`) whose
/// `since:` drives a `git diff` rather than a commit walk.
pub(crate) fn bad_diff_range(rule_id: &str, since: &str, stderr: &str) -> Error {
    Error::rule_config(
        rule_id,
        format!(
            "could not resolve diff range `{since}...HEAD`: {stderr}. Common cause: shallow \
             clone. In a GitHub Actions PR workflow, use `actions/checkout@v4` with \
             `fetch-depth: 0` so the base ref is reachable."
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(sha: &str, message: &str) -> CommitRecord {
        CommitRecord {
            sha: sha.to_string(),
            message: message.to_string(),
            author_name: "Test".to_string(),
            author_email: "test@example.com".to_string(),
        }
    }

    #[test]
    fn format_includes_sha_and_subject() {
        let c = record("abc1234", "fix: a thing\n\nbody");
        let msg = format_commit_violation(&c, "is bad");
        assert!(msg.contains("commit abc1234"), "{msg}");
        assert!(msg.contains("is bad"), "{msg}");
        assert!(msg.contains("fix: a thing"), "{msg}");
        // Only the subject appears, not the body.
        assert!(!msg.contains("body"), "{msg}");
    }

    #[test]
    fn format_truncates_long_subject() {
        let long = "x".repeat(100);
        let c = record("d34db33f", &long);
        let msg = format_commit_violation(&c, "too long");
        assert!(msg.contains('…'), "expected ellipsis: {msg}");
    }
}
