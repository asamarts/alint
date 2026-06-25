//! `git_commit_message` — assert commit messages match a shape
//! (regex, max subject length, body required).
//!
//! Two modes:
//!
//! - **HEAD-only** (default, `since:` omitted): validate the tip
//!   commit. Right shape for push-trigger CI and post-commit hooks.
//! - **Range** (`since:` set): validate every commit reachable from
//!   `HEAD` but not from `since`. Right shape for pull-request CI,
//!   where `actions/checkout` checks out a synthetic merge commit
//!   that the rule should never see. Set `since:` to the PR base
//!   SHA (typically via `${ALINT_BASE_SHA}` env interpolation, with
//!   the env var sourced from `github.event.pull_request.base.sha`
//!   in the workflow). Merge commits in the range are skipped by
//!   default (`include_merges: false`); set `include_merges: true`
//!   to lint them too.
//!
//! Use cases: enforce Conventional Commits / Angular-style
//! prefixes, cap the subject at a screen-friendly width (50–72),
//! require commits that fix issues to include a body linking the
//! issue.
//!
//! Outside a git repo, with no commits yet, or when `git` isn't on
//! PATH, the rule silently no-ops. Same advisory posture as
//! `git_tracked_only` and `git_no_denied_paths`: a rule about git
//! only fires when there's git to inspect.
//!
//! Bad `since:` refs (typo, or a shallow-clone gotcha that left
//! the ref out of local objects) hard-fail with a hint to widen
//! the checkout. The user asked for a range; silently falling back
//! to HEAD-only would mask the misconfiguration.
//!
//! Check-only — alint can't rewrite the user's commit message, and
//! `git commit --amend` is a sensitive operation we don't automate.

use alint_core::git::{
    CommitRangeError, CommitRecord, commit_messages_in_range, head_commit_message,
};
use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Violation};
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Rust-regex pattern the full message (subject + body, joined with
    /// newlines) must match. Use `(?s)` to make `.` match newlines.
    #[serde(default)]
    pattern: Option<String>,
    /// Maximum number of characters allowed in the subject line. Common
    /// values: 50 (Tim Pope's recommendation), 72 (GitHub PR-title cutoff).
    #[serde(default)]
    #[schemars(range(min = 1))]
    subject_max_length: Option<usize>,
    /// When true, the message must have a non-empty body, that is, at least
    /// one line of content after the subject's blank-line separator.
    #[serde(default)]
    requires_body: bool,
    /// Git ref to use as the base of the commit range. When set, validates
    /// every commit in `<since>..HEAD` instead of just HEAD. Accepts anything
    /// `git rev-parse` does: SHA (full or abbreviated), branch (`origin/main`),
    /// tag (`v1.2.3`), or relative ref (`HEAD~5`). Supports POSIX `${VAR}` and
    /// `${VAR:-default}` env-var interpolation so CI can pass a SHA via an env
    /// var (e.g. `since: ${ALINT_BASE_SHA:-origin/main}` with `ALINT_BASE_SHA`
    /// exported in a workflow step from `github.event.pull_request.base.sha`).
    /// The GitHub Actions double-brace template syntax `${{ ... }}` is NOT
    /// interpolated by alint.
    #[serde(default)]
    since: Option<String>,
    /// When validating a range (`since:` set), include merge commits. Defaults
    /// to `false` because merge commits in PR contexts are typically the
    /// synthetic merge `actions/checkout` produces (with an auto-generated
    /// subject the rule would always flag) or maintainer-resolved merges from
    /// the base branch. Has no effect when `since:` is unset; combining
    /// `include_merges: true` with no `since:` is a load-time error.
    #[serde(default)]
    include_merges: bool,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct GitCommitMessageRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message_override: Option<String>,
    pattern: Option<Regex>,
    subject_max_length: Option<usize>,
    requires_body: bool,
    /// `since:` value as written in the config, with `${VAR}`
    /// interpolation NOT yet performed. Resolution is deferred to
    /// evaluate-time so env vars exported by CI steps after rule
    /// load are picked up correctly.
    since_raw: Option<String>,
    include_merges: bool,
}

impl Rule for GitCommitMessageRule {
    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();

        // Resolve `since:` once at evaluate-time so `${VAR}` env
        // expansions reflect the current process environment.
        let since = match &self.since_raw {
            None => None,
            Some(raw) => match expand_env(raw, env_lookup) {
                Ok(resolved) => Some(resolved),
                Err(missing) => {
                    return Err(Error::rule_config(
                        &self.id,
                        format!(
                            "`since:` references undefined env var `{missing}` \
                             and has no default. Either set the env var (for \
                             example, `ALINT_BASE_SHA` from \
                             `github.event.pull_request.base.sha` in a GitHub \
                             Actions workflow) or use the `${{VAR:-default}}` \
                             default-value syntax."
                        ),
                    ));
                }
            },
        };

        let commits = match &since {
            None => match head_commit_message(ctx.root) {
                Some(message) => vec![CommitRecord {
                    sha: "HEAD".to_string(),
                    message,
                    author_name: String::new(),
                    author_email: String::new(),
                }],
                None => return Ok(violations), // silent no-op
            },
            Some(since) => {
                match commit_messages_in_range(ctx.root, since, self.include_merges) {
                    Ok(None) => return Ok(violations), // not a git repo: silent
                    Ok(Some(records)) => records,
                    Err(CommitRangeError::BadRange { stderr }) => {
                        return Err(Error::rule_config(
                            &self.id,
                            format!(
                                "could not resolve commit range `{since}..HEAD`: {stderr}. \
                                 Common cause: shallow clone. In a GitHub Actions PR \
                                 workflow, use `actions/checkout@v4` with \
                                 `fetch-depth: 0` so the base ref is reachable."
                            ),
                        ));
                    }
                }
            }
        };

        for commit in &commits {
            self.check_one(commit, &mut violations);
        }

        Ok(violations)
    }
}

impl GitCommitMessageRule {
    fn check_one(&self, commit: &CommitRecord, violations: &mut Vec<Violation>) {
        let (subject, body) = split_subject_body(&commit.message);

        if let Some(re) = &self.pattern
            && !re.is_match(&commit.message)
        {
            violations.push(self.make_violation(
                format_msg(
                    commit,
                    subject,
                    &format!("commit message does not match pattern `{}`", re.as_str()),
                ),
                format!("{}\u{0}pattern", commit.sha),
            ));
        }

        if let Some(max) = self.subject_max_length
            && subject.chars().count() > max
        {
            violations.push(self.make_violation(
                format_msg(
                    commit,
                    subject,
                    &format!(
                        "commit subject is {} chars; max allowed is {max}",
                        subject.chars().count(),
                    ),
                ),
                format!("{}\u{0}subject_max_length", commit.sha),
            ));
        }

        if self.requires_body && body.trim().is_empty() {
            violations.push(self.make_violation(
                format_msg(
                    commit,
                    subject,
                    "commit message has no body; this rule requires one",
                ),
                format!("{}\u{0}requires_body", commit.sha),
            ));
        }
    }

    fn make_violation(&self, default_msg: String, key: String) -> Violation {
        // A single commit can trip several checks (pattern / length / body),
        // and there is no path, so the baseline key combines the commit's
        // stable id with the check kind (and never the volatile message).
        Violation::new(self.message_override.clone().unwrap_or(default_msg)).with_baseline_key(key)
    }
}

/// Render a violation message with the commit SHA + a trimmed
/// subject snippet for context. SHA is "HEAD" in single-commit
/// mode (the helper synthesises it) and an abbreviated SHA in
/// range mode. Subject is truncated to ~60 chars so violation
/// output stays readable.
fn format_msg(commit: &CommitRecord, subject: &str, what: &str) -> String {
    const SUBJECT_PREVIEW_MAX: usize = 60;
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

/// Split a commit message into (subject, body). The subject is the
/// first line; the body is everything after the first blank line
/// that follows it. Messages with no blank-line separator have an
/// empty body. Trailing whitespace on the subject is preserved
/// as-is — the length check counts it.
fn split_subject_body(message: &str) -> (&str, &str) {
    let (subject, rest) = message.split_once('\n').unwrap_or((message, ""));
    // Skip exactly one trailing blank-line separator if present
    // (the canonical "subject\n\nbody" shape). Multiple blank
    // lines fall through into the body — they're unusual but we
    // don't want to silently swallow user content.
    let body = rest.strip_prefix('\n').unwrap_or(rest);
    (subject, body)
}

/// Expand POSIX-style env-var references in a `since:` value. The
/// syntax is intentionally narrow:
///
/// - `${VAR}` — substitute the value of `VAR`. If unset, returns
///   `Err(missing-var-name)` so the rule can hard-fail with a
///   CI-friendly hint.
/// - `${VAR:-default}` — substitute `VAR`, or `default` when
///   `VAR` is unset or empty. `default` may not itself contain
///   `${`, `}` or `:-` — keep the surface small.
/// - Bare text — left as-is. So `since: origin/main` works
///   unchanged.
///
/// Multiple `${...}` references in one value are supported. The
/// double-brace GitHub Actions syntax (`${{ ... }}`) is NOT
/// interpolated by alint; it has to be rendered by Actions before
/// the YAML is read, which only works in workflow files, not in
/// `.alint.yml`. The single-brace form is the alint convention.
///
/// `lookup` is an explicit env-var resolver (typically
/// `|name| std::env::var(name).ok()`); injecting it keeps the
/// pure-string parsing testable without `set_var` calls (the
/// crate forbids unsafe code, and Rust 2024 marks `set_var` /
/// `remove_var` unsafe).
fn expand_env<F>(input: &str, lookup: F) -> std::result::Result<String, String>
where
    F: Fn(&str) -> Option<String>,
{
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        let Some(end) = after_open.find('}') else {
            // Unclosed `${...`. Treat as literal, don't error —
            // the caller's value is probably a literal SHA that
            // happens to contain `${`.
            out.push_str("${");
            rest = after_open;
            continue;
        };
        let inner = &after_open[..end];
        let (name, default) = match inner.split_once(":-") {
            Some((n, d)) => (n, Some(d)),
            None => (inner, None),
        };
        match lookup(name) {
            Some(v) if !v.is_empty() => out.push_str(&v),
            _ => match default {
                Some(d) => out.push_str(d),
                None => return Err(name.to_string()),
            },
        }
        rest = &after_open[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Production lookup: read from the process environment.
fn env_lookup(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// Rewrite POSIX `${VAR}` / `${VAR:-default}` occurrences into the
/// canonical v0.11 `{{env.VAR}}` / `{{env.VAR | default('default')}}`
/// form, for the deprecation warning's actionable suggestion. Mirrors
/// [`expand_env`]'s grammar; non-`${...}` text passes through.
fn posix_to_env_template(input: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        let Some(end) = after_open.find('}') else {
            out.push_str("${");
            rest = after_open;
            continue;
        };
        let inner = &after_open[..end];
        match inner.split_once(":-") {
            Some((name, def)) => {
                let _ = write!(out, "{{{{env.{name} | default('{def}')}}}}");
            }
            None => {
                let _ = write!(out, "{{{{env.{inner}}}}}");
            }
        }
        rest = &after_open[end + 1..];
    }
    out.push_str(rest);
    out
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;

    if opts.pattern.is_none() && opts.subject_max_length.is_none() && !opts.requires_body {
        return Err(Error::rule_config(
            &spec.id,
            "git_commit_message needs at least one of `pattern:`, `subject_max_length:`, \
             or `requires_body: true`",
        ));
    }
    // `subject_max_length: 0` would fail every commit (a 0-char subject is
    // impossible); the JSON schema declares `minimum: 1`, so enforce it at load.
    if opts.subject_max_length == Some(0) {
        return Err(Error::rule_config(
            &spec.id,
            "git_commit_message `subject_max_length` must be >= 1 (0 rejects every commit)",
        ));
    }
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "git_commit_message has no fix op",
        ));
    }
    if opts.include_merges && opts.since.is_none() {
        return Err(Error::rule_config(
            &spec.id,
            "`include_merges: true` has no effect without `since:`. Either remove it \
             or set `since:` to enable range mode.",
        ));
    }

    // Deprecation: v0.9.21 shipped POSIX `${VAR}` interpolation on
    // `since:` only. v0.11 generalises env interpolation to the
    // canonical `{{env.X}}` form (resolved at config load by
    // `alint-dsl`), so a `since:` value still written as `${VAR}`
    // is a legacy syntax. It keeps working this one minor (expanded
    // at evaluate time by `expand_env`) but warns; v1.0 removes it.
    // Scoped to `since:` because `${VAR}` was never interpolated in
    // any other field — a literal `${` elsewhere is just a literal.
    if let Some(raw) = &opts.since {
        if raw.contains("${") {
            eprintln!(
                "alint: warning: rule {:?}: `since: {raw}` uses the deprecated v0.9.21 \
                 `${{VAR}}` interpolation syntax. The canonical v0.11+ form is `{}`; \
                 the `${{VAR}}` form will be removed in v1.0. \
                 See https://alint.org/docs/configuration/#variable-interpolation.",
                spec.id,
                posix_to_env_template(raw),
            );
        }
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
        since_raw: opts.since,
        include_merges: opts.include_merges,
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
        // git-style messages should have a blank line, but tools
        // like `git commit -m "first\nsecond"` produce bodies
        // without one. Treat the second line on as body even
        // without a separator.
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

    // ----- format_msg formatting --------------------------------

    #[test]
    fn format_msg_renders_sha_and_subject() {
        let commit = CommitRecord {
            sha: "a1b2c3d".to_string(),
            message: "fix: thing".to_string(),
            author_name: String::new(),
            author_email: String::new(),
        };
        let s = format_msg(&commit, "fix: thing", "subject too long");
        assert!(s.contains("commit a1b2c3d"));
        assert!(s.contains("fix: thing"));
        assert!(s.contains("subject too long"));
    }

    #[test]
    fn format_msg_truncates_long_subjects() {
        let long_subject = "x".repeat(120);
        let commit = CommitRecord {
            sha: "abc1234".to_string(),
            message: long_subject.clone(),
            author_name: String::new(),
            author_email: String::new(),
        };
        let s = format_msg(&commit, &long_subject, "too long");
        // Subject preview is capped at 60 chars + ellipsis.
        assert!(s.contains(&"x".repeat(60)));
        assert!(s.contains('…'));
        assert!(!s.contains(&"x".repeat(61)));
    }

    // ----- env-var interpolation --------------------------------

    /// Build a fake env lookup from a list of (name, value) pairs.
    /// Anything not in the list is treated as unset.
    fn fake_env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_string())
        }
    }

    #[test]
    fn expand_env_passthrough_for_bare_string() {
        let env = fake_env(&[]);
        assert_eq!(expand_env("origin/main", &env).unwrap(), "origin/main");
        assert_eq!(expand_env("v0.9.20", &env).unwrap(), "v0.9.20");
        assert_eq!(
            expand_env("abc1234567890abcdef1234567890abcdef12345678", &env,).unwrap(),
            "abc1234567890abcdef1234567890abcdef12345678"
        );
    }

    #[test]
    fn expand_env_substitutes_simple_var() {
        let env = fake_env(&[("ALINT_BASE_SHA", "deadbeef")]);
        assert_eq!(expand_env("${ALINT_BASE_SHA}", &env).unwrap(), "deadbeef");
    }

    #[test]
    fn expand_env_default_used_when_var_unset() {
        let env = fake_env(&[]);
        assert_eq!(
            expand_env("${MISSING:-origin/main}", &env).unwrap(),
            "origin/main"
        );
    }

    #[test]
    fn expand_env_default_used_when_var_empty() {
        let env = fake_env(&[("EMPTY", "")]);
        assert_eq!(
            expand_env("${EMPTY:-origin/main}", &env).unwrap(),
            "origin/main"
        );
    }

    #[test]
    fn expand_env_errors_when_var_unset_and_no_default() {
        let env = fake_env(&[]);
        let err = expand_env("${NOPE}", &env).unwrap_err();
        assert_eq!(err, "NOPE");
    }

    #[test]
    fn expand_env_handles_multiple_references() {
        let env = fake_env(&[("A", "foo"), ("B", "bar")]);
        assert_eq!(expand_env("${A}-${B}", &env).unwrap(), "foo-bar");
    }

    #[test]
    fn expand_env_handles_text_around_var() {
        let env = fake_env(&[("SHA", "abc1234")]);
        assert_eq!(
            expand_env("refs/${SHA}/head", &env).unwrap(),
            "refs/abc1234/head"
        );
    }

    #[test]
    fn expand_env_ignores_unclosed_brace() {
        // Don't crash on a value that contains a literal `${` —
        // could be a base64 SHA or something weirder. Treat as
        // literal.
        let env = fake_env(&[]);
        assert_eq!(expand_env("foo${unclosed", &env).unwrap(), "foo${unclosed");
    }

    // ----- build() validation -----------------------------------

    fn spec(toml: &str) -> RuleSpec {
        let mut full =
            String::from("id = \"test-rule\"\nkind = \"git_commit_message\"\nlevel = \"error\"\n");
        full.push_str(toml);
        toml::from_str(&full).unwrap()
    }

    #[test]
    fn build_requires_at_least_one_assertion() {
        // No pattern, no subject_max_length, no requires_body. The
        // rule has nothing to check; build() rejects.
        let s = spec("");
        let err = build(&s).unwrap_err();
        assert!(err.to_string().contains("at least one of"));
    }

    #[test]
    fn build_rejects_include_merges_without_since() {
        // include_merges only makes sense alongside since:.
        // Surfacing this at config-load time prevents silent
        // no-ops.
        let s = spec("requires_body = true\ninclude_merges = true\n");
        let err = build(&s).unwrap_err();
        assert!(
            err.to_string().contains("include_merges"),
            "expected include_merges hint, got: {err}"
        );
    }

    #[test]
    fn posix_to_env_template_converts_simple_and_default() {
        assert_eq!(
            posix_to_env_template("${ALINT_BASE_SHA}"),
            "{{env.ALINT_BASE_SHA}}"
        );
        assert_eq!(
            posix_to_env_template("${BASE:-origin/main}"),
            "{{env.BASE | default('origin/main')}}"
        );
        // Bare text + embedded form pass through / convert in place.
        assert_eq!(posix_to_env_template("origin/main"), "origin/main");
        assert_eq!(posix_to_env_template("refs/${REF}"), "refs/{{env.REF}}");
    }

    #[test]
    fn build_accepts_legacy_posix_since_with_deprecation() {
        // `${VAR}` still builds (deprecation is a stderr warning, not
        // an error) so existing v0.9.21 configs keep loading. The
        // value is still expanded at evaluate time by `expand_env`.
        let s = spec("requires_body = true\nsince = \"${ALINT_BASE_SHA}\"\n");
        assert!(build(&s).is_ok());
    }

    #[test]
    fn build_accepts_canonical_template_since() {
        // The canonical `{{env.X}}` form is resolved upstream by the
        // DSL interp pass before the rule sees it; here (no interp)
        // it simply arrives as a literal ref and builds fine without
        // triggering the `${VAR}` deprecation path.
        let s = spec("requires_body = true\nsince = \"{{env.ALINT_BASE_SHA}}\"\n");
        assert!(build(&s).is_ok());
    }

    #[test]
    fn build_accepts_since_with_other_options() {
        let s = spec("pattern = \"^feat: \"\nsince = \"origin/main\"\n");
        assert!(build(&s).is_ok());
    }

    #[test]
    fn build_accepts_since_with_include_merges() {
        let s = spec("subject_max_length = 50\nsince = \"origin/main\"\ninclude_merges = true\n");
        assert!(build(&s).is_ok());
    }
}
