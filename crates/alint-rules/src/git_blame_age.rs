//! `git_blame_age` — fire on lines matching a regex whose
//! `git blame` author-time is older than a configured threshold.
//!
//! Same regex match shape as `file_content_forbidden`, but with
//! a per-line age gate: a TODO added yesterday passes silently;
//! a TODO that has sat in tree for 18 months fires. Closes the
//! gap between `level: warning` on every TODO (too noisy) and
//! `level: off` (accepts unbounded debt accumulation).
//!
//! Outside a git repo, on untracked files, or when the blame
//! invocation otherwise fails, the rule silently no-ops per
//! file — same advisory posture as `git_no_denied_paths` and
//! `git_commit_message`. Heuristic notes (formatting passes
//! reset blame age, vendored code carries the import commit's
//! timestamp, squash merges collapse to one date) are
//! documented in `docs/rules.md` and `docs/design/v0.7/git_blame_age.md`.
//!
//! Check-only — auto-removing TODO markers is destructive and
//! pinning a line's content as "do nothing" doesn't help.

use std::time::{Duration, SystemTime};

use alint_core::template::render_message;
use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Regex applied to each blame line's content. Same flavour
    /// as `file_content_forbidden`'s pattern. Captured groups
    /// are exposed as `{{ctx.match}}` in the message template
    /// (defaults to the full match when no capture group is
    /// present).
    pattern: String,
    /// Minimum line age (in days) for a matching line to fire as
    /// a violation. Lines younger than this pass silently.
    /// Required.
    max_age_days: u64,
}

#[derive(Debug)]
pub struct GitBlameAgeRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    pattern: Regex,
    max_age: Duration,
}

impl Rule for GitBlameAgeRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        self.level
    }
    fn policy_url(&self) -> Option<&str> {
        self.policy_url.as_deref()
    }
    fn wants_git_blame(&self) -> bool {
        true
    }
    fn path_scope(&self) -> Option<&Scope> {
        Some(&self.scope)
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        let Some(blame_cache) = ctx.git_blame else {
            // Non-git workspace, or no rule asked for the cache
            // — silent no-op, by design. Same posture as the
            // rest of the git module.
            return Ok(violations);
        };
        let now = SystemTime::now();
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path) {
                continue;
            }
            let Some(blame) = blame_cache.get(&entry.path) else {
                // Untracked file or per-file blame failure —
                // skip silently.
                continue;
            };
            for line in blame.iter() {
                let Some(captures) = self.pattern.captures(&line.content) else {
                    continue;
                };
                let age = now
                    .duration_since(line.author_time)
                    .unwrap_or(Duration::ZERO);
                if age <= self.max_age {
                    continue;
                }
                let matched = captures
                    .get(1)
                    .or_else(|| captures.get(0))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
                let age_days = age.as_secs() / 86_400;
                let default_msg = format!(
                    "`{}` matched line is {} days old (>{} days)",
                    matched,
                    age_days,
                    self.max_age.as_secs() / 86_400,
                );
                let user_msg = self.message.as_deref().unwrap_or(&default_msg);
                let rendered = render_message(user_msg, |ns, key| match (ns, key) {
                    ("ctx", "match") => Some(matched.clone()),
                    _ => None,
                });
                violations.push(
                    Violation::new(rendered)
                        .with_path(&entry.path)
                        .with_location(line.line_number, 1),
                );
            }
        }
        Ok(violations)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "git_blame_age requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.max_age_days == 0 {
        return Err(Error::rule_config(
            &spec.id,
            "git_blame_age `max_age_days` must be ≥ 1",
        ));
    }
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "git_blame_age has no fix op — auto-removing matched lines is destructive",
        ));
    }
    let pattern = Regex::new(&opts.pattern)
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid pattern: {e}")))?;
    Ok(Box::new(GitBlameAgeRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        pattern,
        max_age: Duration::from_secs(opts.max_age_days.saturating_mul(86_400)),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::git::BlameCache;
    use alint_core::{FileEntry, FileIndex, PathsSpec};
    use std::path::{Path, PathBuf};

    fn rule(pattern: &str, max_age_days: u64, message: Option<&str>) -> GitBlameAgeRule {
        GitBlameAgeRule {
            id: "test".into(),
            level: Level::Warning,
            policy_url: None,
            message: message.map(str::to_string),
            scope: Scope::from_paths_spec(&PathsSpec::Single("**/*.rs".into())).unwrap(),
            pattern: Regex::new(pattern).unwrap(),
            max_age: Duration::from_secs(max_age_days * 86_400),
        }
    }

    fn index(paths: &[&str]) -> FileIndex {
        FileIndex {
            entries: paths
                .iter()
                .map(|p| FileEntry {
                    path: PathBuf::from(p),
                    is_dir: false,
                    size: 0,
                })
                .collect(),
        }
    }

    #[test]
    fn no_op_when_blame_cache_absent() {
        // Outside a git repo (or no rule wants blame), the
        // engine doesn't build a cache and the rule must
        // silently produce no violations.
        let r = rule(r"\bTODO\b", 30, None);
        let idx = index(&["src/main.rs"]);
        let ctx = Context {
            root: Path::new("/"),
            index: &idx,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
        };
        let v = r.evaluate(&ctx).unwrap();
        assert!(v.is_empty(), "expected silent no-op, got {v:?}");
    }

    #[test]
    fn no_op_when_blame_lookup_fails() {
        // Cache exists but every lookup fails (no real git repo
        // at the cache's root). Rule must silently skip.
        let r = rule(r"\bTODO\b", 30, None);
        let idx = index(&["src/main.rs"]);
        let tmp = tempfile::tempdir().unwrap();
        let cache = BlameCache::new(tmp.path().to_path_buf());
        let ctx = Context {
            root: tmp.path(),
            index: &idx,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: Some(&cache),
        };
        let v = r.evaluate(&ctx).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn rejects_zero_max_age_days() {
        let yaml = "\
id: t
kind: git_blame_age
paths: \"**/*.rs\"
pattern: 'TODO'
max_age_days: 0
level: warning
";
        let spec: RuleSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let err = build(&spec).unwrap_err();
        assert!(
            err.to_string().contains("max_age_days"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn rejects_invalid_regex() {
        let yaml = "\
id: t
kind: git_blame_age
paths: \"**/*.rs\"
pattern: '[unterminated'
max_age_days: 7
level: warning
";
        let spec: RuleSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert!(build(&spec).is_err());
    }

    #[test]
    fn rejects_fix_block() {
        let yaml = "\
id: t
kind: git_blame_age
paths: \"**/*.rs\"
pattern: 'TODO'
max_age_days: 7
level: warning
fix:
  file_remove: {}
";
        let spec: RuleSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let err = build(&spec).unwrap_err();
        assert!(err.to_string().contains("no fix"), "unexpected: {err}");
    }

    #[test]
    fn requires_paths_field() {
        let yaml = "\
id: t
kind: git_blame_age
pattern: 'TODO'
max_age_days: 7
level: warning
";
        let spec: RuleSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let err = build(&spec).unwrap_err();
        assert!(err.to_string().contains("paths"), "unexpected: {err}");
    }
}
