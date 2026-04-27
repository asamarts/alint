//! `git_commit_message` — assert HEAD's commit message matches a
//! shape (regex, max subject length, body required).
//!
//! Use cases: enforce Conventional Commits / Angular-style
//! prefixes, cap the subject at a screen-friendly width
//! (50–72), require commits that fix issues to include a body
//! linking the issue. CI integration: run `alint check
//! --changed` (or `alint check`) on every PR; alint reads the
//! tip commit and fires if the shape is off.
//!
//! Outside a git repo, with no commits yet, or when `git` isn't
//! on PATH, the rule silently no-ops. This is the same advisory
//! posture as `git_tracked_only` and `git_no_denied_paths`: a
//! rule about git only fires when there's git to inspect.
//!
//! Check-only — alint can't rewrite the user's commit
//! message, and `git commit --amend` is a sensitive operation
//! we don't automate.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Violation};
use alint_core::git::head_commit_message;
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Regex the full commit message (subject + body, joined
    /// with newlines) must match. When omitted, no regex check
    /// is applied. Use `(?s)` to make `.` match newlines if you
    /// want to assert about content past the subject.
    #[serde(default)]
    pattern: Option<String>,
    /// Maximum length of the subject line (the message's first
    /// line, before any body). When omitted, no length cap.
    /// Common values: 50 (Tim Pope's recommendation), 72
    /// (GitHub's PR-title cutoff).
    #[serde(default)]
    subject_max_length: Option<usize>,
    /// When `true`, the message must have a non-empty body —
    /// at least one line of content after the subject's
    /// trailing blank line. Useful for mandating an
    /// explanation on `fix:` commits etc.
    #[serde(default)]
    requires_body: bool,
}

#[derive(Debug)]
pub struct GitCommitMessageRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message_override: Option<String>,
    pattern: Option<Regex>,
    subject_max_length: Option<usize>,
    requires_body: bool,
}

impl Rule for GitCommitMessageRule {
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
        let mut violations = Vec::new();
        let Some(message) = head_commit_message(ctx.root) else {
            // No git, no commits, or git not on PATH — silent
            // no-op. This rule only makes sense when there's a
            // commit to inspect.
            return Ok(violations);
        };

        let (subject, body) = split_subject_body(&message);

        if let Some(re) = &self.pattern
            && !re.is_match(&message)
        {
            violations.push(self.make_violation(format!(
                "HEAD commit message does not match pattern `{}`",
                re.as_str(),
            )));
        }

        if let Some(max) = self.subject_max_length
            && subject.chars().count() > max
        {
            violations.push(self.make_violation(format!(
                "HEAD commit subject is {} chars; max allowed is {max}",
                subject.chars().count(),
            )));
        }

        if self.requires_body && body.trim().is_empty() {
            violations.push(self.make_violation(
                "HEAD commit message has no body; this rule requires one".to_string(),
            ));
        }

        Ok(violations)
    }
}

impl GitCommitMessageRule {
    fn make_violation(&self, default_msg: String) -> Violation {
        Violation::new(self.message_override.clone().unwrap_or(default_msg))
    }
}

/// Split a commit message into (subject, body). The subject is
/// the first line; the body is everything after the first
/// blank line that follows it. Messages with no blank-line
/// separator have an empty body. Trailing whitespace on the
/// subject is preserved as-is — the length check counts it.
fn split_subject_body(message: &str) -> (&str, &str) {
    let (subject, rest) = message.split_once('\n').unwrap_or((message, ""));
    // Skip exactly one trailing blank-line separator if present
    // (the canonical "subject\n\nbody" shape). Multiple blank
    // lines fall through into the body — they're unusual but
    // we don't want to silently swallow user content.
    let body = rest.strip_prefix('\n').unwrap_or(rest);
    (subject, body)
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;

    if opts.pattern.is_none()
        && opts.subject_max_length.is_none()
        && !opts.requires_body
    {
        return Err(Error::rule_config(
            &spec.id,
            "git_commit_message needs at least one of `pattern:`, `subject_max_length:`, \
             or `requires_body: true`",
        ));
    }
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "git_commit_message has no fix op",
        ));
    }

    let pattern = opts
        .pattern
        .as_deref()
        .map(|p| {
            Regex::new(p).map_err(|e| {
                Error::rule_config(&spec.id, format!("invalid `pattern:` regex `{p}`: {e}"))
            })
        })
        .transpose()?;

    Ok(Box::new(GitCommitMessageRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message_override: spec.message.clone(),
        pattern,
        subject_max_length: opts.subject_max_length,
        requires_body: opts.requires_body,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_one_line_message() {
        let (subj, body) = split_subject_body("just a subject");
        assert_eq!(subj, "just a subject");
        assert_eq!(body, "");
    }

    #[test]
    fn split_subject_body_with_canonical_blank_line() {
        let (subj, body) = split_subject_body("Add feature\n\nLong description here.\nMore.");
        assert_eq!(subj, "Add feature");
        assert_eq!(body, "Long description here.\nMore.");
    }

    #[test]
    fn split_subject_no_blank_separator() {
        // git-style messages should have a blank line, but
        // tools like `git commit -m "first\nsecond"` produce
        // bodies without one. Treat the second line on as body
        // even without a separator.
        let (subj, body) = split_subject_body("subject\nrest of content");
        assert_eq!(subj, "subject");
        assert_eq!(body, "rest of content");
    }

    #[test]
    fn pattern_rejects_unrelated_subject() {
        let re = Regex::new(r"^(feat|fix|chore): ").unwrap();
        assert!(!re.is_match("WIP changes"));
        assert!(re.is_match("feat: add markdown formatter"));
    }

    #[test]
    fn subject_length_uses_chars_not_bytes() {
        // Multi-byte unicode in the subject should count by
        // grapheme-ish chars, not bytes — a 50-char subject of
        // emoji should be 50 chars, not 200 bytes.
        let subj = "🚀".repeat(50);
        assert_eq!(subj.chars().count(), 50);
        assert_eq!(subj.len(), 50 * 4); // bytes
    }

    #[test]
    fn requires_body_detects_subject_only() {
        let (_, body) = split_subject_body("just a subject");
        assert!(body.trim().is_empty());
    }

    #[test]
    fn requires_body_accepts_canonical_form() {
        let (_, body) = split_subject_body("subject\n\nbody content");
        assert!(!body.trim().is_empty());
    }
}
