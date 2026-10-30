//! `git_blame_age` — fire on lines matching a regex whose
//! `git blame` author-time is older than a configured threshold.
//!
//! Same regex match shape as `file_content_forbidden`, but with
//! a per-line age gate: a TODO added yesterday passes silently;
//! a TODO that has sat in tree for 18 months fires. Closes the
//! gap between `level: warning` on every TODO (too noisy) and
//! `level: off` (accepts unbounded debt accumulation).
//!
//! An unavailable Git executable or repository is an analysis failure even
//! when an explicit reference date avoids the HEAD-time lookup. Untracked
//! files and individual blame failures still no-op. Heuristic notes (formatting
//! passes reset blame age, vendored code carries the import commit's timestamp,
//! squash merges collapse to one date) are documented in `docs/rules.md` and
//! `docs/design/v0.7/git_blame_age.md`.
//!
//! Check-only — auto-removing TODO markers is destructive and
//! pinning a line's content as "do nothing" doesn't help.

use std::fmt::Write as _;
use std::time::Duration;

use alint_core::template::render_message;
use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Rust-regex pattern applied to each blame line's content. Capture group 1
    /// (when present) is exposed as `{{ctx.match}}` in the message template;
    /// without a capture group, `{{ctx.match}}` substitutes the full match.
    pattern: String,
    /// Minimum line age (in days) for a matching line to fire as a violation.
    /// Lines younger than this pass silently. Common values: 90 (one quarter),
    /// 180 (half a year), 365 (a full year of debt).
    #[schemars(range(min = 1))]
    max_age_days: u64,
}

crate::options_schema_for!(Options);

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
    alint_core::rule_common_impl!();
    fn wants_git_blame(&self) -> bool {
        true
    }
    fn wants_analysis_time(&self) -> bool {
        true
    }
    fn decorate_expect_matches_violation(
        &self,
        violation: &mut Violation,
        ctx: &Context<'_>,
    ) -> Result<()> {
        let analysis_time = ctx.analysis_time()?;
        write!(violation.message.to_mut(), " (as of {analysis_time})")
            .expect("writing to a String cannot fail");
        Ok(())
    }
    fn path_scope(&self) -> Option<&Scope> {
        Some(&self.scope)
    }
    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let analysis_time = ctx.analysis_time()?;
        let mut violations = Vec::new();
        let Some(blame_cache) = ctx.git_blame else {
            return Ok(violations);
        };
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path, ctx.index) {
                continue;
            }
            // Apply scope_filter BEFORE the blame lookup so we
            // don't pay the per-file blame cost on out-of-scope
            // files. (Blame is the expensive bit here.)
            let Some(blame) = blame_cache.get(&entry.path) else {
                // Untracked file or per-file blame failure —
                // skip silently.
                continue;
            };
            for line in blame.iter() {
                let Some(captures) = self.pattern.captures(&line.content) else {
                    continue;
                };
                let age = analysis_time.elapsed_since(line.author_time);
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
                    Violation::new(format!("{rendered} (as of {analysis_time})"))
                        .with_path(entry.path.clone())
                        .with_location(line.line_number, 1),
                );
            }
        }
        Ok(violations)
    }
}

pub static META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::RuleMajorScoped,
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[
        alint_core::CapabilityMeta {
            capability: alint_core::Capability::Git {
                operations: std::borrow::Cow::Borrowed(&[
                    alint_core::GitOperation::TrackedSet,
                    alint_core::GitOperation::Blame,
                ]),
            },
            condition: alint_core::CapabilityCondition::Always,
        },
        alint_core::CapabilityMeta {
            capability: alint_core::Capability::AnalysisTime,
            condition: alint_core::CapabilityCondition::Always,
        },
    ]),
    content: alint_core::ContentAccess::Blame,
    formats: std::borrow::Cow::Borrowed(&[]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[]),
    expect_matches: true,
    trace: alint_core::TraceMeta::SELECTION,
    nested_rules: false,
};

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(_paths) = &spec.paths else {
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
            "git_blame_age has no fix op - auto-removing matched lines is destructive",
        ));
    }
    let pattern = Regex::new(&opts.pattern)
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid pattern: {e}")))?;
    Ok(Box::new(GitBlameAgeRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        pattern,
        max_age: Duration::from_secs(opts.max_age_days.saturating_mul(86_400)),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::git::BlameCache;
    use alint_core::{FileEntry, FileIndex, PathsSpec};
    use std::path::Path;

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
        FileIndex::from_entries(
            paths
                .iter()
                .map(|p| FileEntry {
                    path: std::path::Path::new(p).into(),
                    is_dir: false,
                    size: 0,
                })
                .collect(),
        )
    }

    fn analysis_time() -> alint_core::AnalysisTimeResolution {
        alint_core::AnalysisTimeResolution::Available {
            time: "2026-10-16".parse().unwrap(),
            source: alint_core::AnalysisTimeSource::Explicit,
        }
    }

    #[test]
    fn no_op_when_blame_cache_absent() {
        // Direct rule evaluation remains total without a blame cache. Engine
        // capability routing reports repository unavailability before this
        // rule dispatches in normal execution.
        let r = rule(r"\bTODO\b", 30, None);
        let idx = index(&["src/main.rs"]);
        let analysis_time = analysis_time();
        let ctx = Context {
            root: Path::new("/"),
            index: &idx,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
            analysis_time: Some(&analysis_time),
            process_environment: None,
            trace: None,
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
        let analysis_time = analysis_time();
        let ctx = Context {
            root: tmp.path(),
            index: &idx,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: Some(&cache),
            analysis_time: Some(&analysis_time),
            process_environment: None,
            trace: None,
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
        let spec: RuleSpec = ayaml::from_str(yaml).unwrap();
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
        let spec: RuleSpec = ayaml::from_str(yaml).unwrap();
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
        let spec: RuleSpec = ayaml::from_str(yaml).unwrap();
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
        let spec: RuleSpec = ayaml::from_str(yaml).unwrap();
        let err = build(&spec).unwrap_err();
        assert!(err.to_string().contains("paths"), "unexpected: {err}");
    }

    #[test]
    fn scope_filter_narrows() {
        // YAML build wires up the scope_filter; runtime check
        // gates files BEFORE the blame lookup. With a stub
        // cache backed by a non-repo dir, every blame lookup
        // returns None, so we can't observe per-file violation
        // differences — but we CAN observe that:
        // (a) `Rule::scope_filter()` reports the filter
        //     (proves build wired it up), and
        // (b) the rule's silent-no-op path still holds when
        //     the filter is set.
        let yaml = "\
id: t
kind: git_blame_age
paths: \"**/*.rs\"
pattern: 'TODO'
max_age_days: 7
scope_filter:
  has_ancestor: marker.lock
level: warning
";
        let spec: RuleSpec = ayaml::from_str(yaml).unwrap();
        let r = build(&spec).unwrap();
        assert!(
            r.path_scope()
                .expect("git_blame_age exposes path_scope")
                .scope_filter()
                .is_some(),
            "scope_filter should be wired through build() into Scope"
        );
        let idx = index(&["pkg/marker.lock", "pkg/a.rs", "other/b.rs"]);
        let tmp = tempfile::tempdir().unwrap();
        let cache = BlameCache::new(tmp.path().to_path_buf());
        let analysis_time = analysis_time();
        let ctx = Context {
            root: tmp.path(),
            index: &idx,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: Some(&cache),
            analysis_time: Some(&analysis_time),
            process_environment: None,
            trace: None,
        };
        let v = r.evaluate(&ctx).unwrap();
        assert!(v.is_empty(), "no blame data → no violations: {v:?}");
    }
}
