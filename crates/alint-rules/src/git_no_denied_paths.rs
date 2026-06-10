//! `git_no_denied_paths` — fire when any tracked file matches a
//! configured denylist of glob patterns.
//!
//! The companion of `git_tracked_only` (v0.4.8) for the
//! "absence" axis. `file_absent` checks whether a file exists on
//! disk; `git_no_denied_paths` checks whether the *git index*
//! contains any path matching a pattern, regardless of whether
//! it's currently in the working tree. A user's locally-built
//! `target/` is silent; a `target/` that ended up tracked
//! through a misconfigured `.gitignore` fires.
//!
//! Use cases: secrets (`*.env`, `id_rsa`, `*.pem`), bulky
//! generated artefacts that don't belong in version control
//! (`*.log`, `dist/**`), legacy "do not commit" sentinels.
//!
//! Outside a git repo (no `git` on PATH, `root` not inside a
//! repo, etc.) the rule silently no-ops — same advisory posture
//! as `git_tracked_only`. Check-only — fixing means
//! `git rm --cached`, which is a sensitive operation alint
//! should never automate.

use alint_core::git::{CommitRangeError, collect_changed_paths_checked, collect_tracked_paths};
use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Violation};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Globset patterns no tracked path may match. Both whole-path patterns
    /// (`secrets/**`) and basename-only patterns (`*.env`) work.
    #[schemars(length(min = 1))]
    denied: Vec<String>,
    /// Optional git ref. When set, only denied paths that changed in the
    /// `<since>...HEAD` diff are flagged, catches a secret added in a PR even if
    /// HEAD's tree still tracks an older one. Accepts the `{{env.X}}`
    /// interpolation, e.g. `since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"`.
    #[serde(default)]
    since: Option<String>,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct GitNoDeniedPathsRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    denied_set: GlobSet,
    /// Original patterns, kept for the violation message so the
    /// user sees which entry on their denylist matched.
    denied_src: Vec<String>,
    /// `since:` value (already `{{env.X}}`-interpolated at load).
    /// `None` checks every tracked path; `Some` restricts to the
    /// `<since>...HEAD` diff.
    since_raw: Option<String>,
}

impl Rule for GitNoDeniedPathsRule {
    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        let Some(tracked) = collect_tracked_paths(ctx.root) else {
            // Not a git repo, or git not on PATH — silent
            // no-op, by design. The rule's intent only makes
            // sense inside a tracked working tree.
            return Ok(violations);
        };

        // When `since:` is set, restrict to paths that changed in the
        // `<since>...HEAD` diff. A bad ref hard-fails (shallow-clone
        // hint); no-git is already covered by the `tracked` guard.
        let diff = match &self.since_raw {
            None => None,
            Some(since) => match collect_changed_paths_checked(ctx.root, since) {
                Ok(diff) => diff,
                Err(CommitRangeError::BadRange { stderr }) => {
                    return Err(Error::rule_config(
                        &self.id,
                        format!(
                            "could not resolve `{since}...HEAD`: {stderr}. Common cause: \
                             shallow clone. In a GitHub Actions PR workflow, use \
                             `actions/checkout@v4` with `fetch-depth: 0` so the base ref is \
                             reachable."
                        ),
                    ));
                }
            },
        };

        for path in &tracked {
            if let Some(diff) = &diff
                && !diff.contains(path)
            {
                continue;
            }
            let matches = self.denied_set.matches(path);
            if matches.is_empty() {
                continue;
            }
            // Report all matching patterns so a path that hits
            // multiple denylist entries (e.g. `*.env` AND
            // `secrets/**`) shows the user the full picture.
            let pattern_list: Vec<&str> = matches
                .iter()
                .map(|i| self.denied_src[*i].as_str())
                .collect();
            let detail = format!(
                "tracked path matches denied pattern{} `{}`",
                if pattern_list.len() == 1 { "" } else { "s" },
                pattern_list.join("`, `"),
            );
            let msg = self.message.clone().unwrap_or(detail);
            violations.push(Violation::new(msg).with_path(path.clone()));
        }

        Ok(violations)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;

    if opts.denied.is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "git_no_denied_paths requires a non-empty `denied:` list",
        ));
    }
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "git_no_denied_paths has no fix op — `git rm --cached` is too destructive to automate",
        ));
    }

    let mut builder = GlobSetBuilder::new();
    for pattern in &opts.denied {
        let glob = Glob::new(pattern).map_err(|e| {
            Error::rule_config(&spec.id, format!("invalid denied pattern `{pattern}`: {e}"))
        })?;
        builder.add(glob);
    }
    let denied_set = builder
        .build()
        .map_err(|e| Error::rule_config(&spec.id, format!("could not build denied set: {e}")))?;

    Ok(Box::new(GitNoDeniedPathsRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        denied_set,
        denied_src: opts.denied,
        since_raw: opts.since,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_set(patterns: &[&str]) -> GlobSet {
        let mut b = GlobSetBuilder::new();
        for p in patterns {
            b.add(Glob::new(p).unwrap());
        }
        b.build().unwrap()
    }

    #[test]
    fn extension_glob_matches_root_basename() {
        // `*.env` is a basename pattern: it matches `.env` at
        // the repo root but not under subdirectories (globset's
        // `*` doesn't cross `/`). Users who want recursive
        // matching write `**/.env`.
        let set = build_set(&["*.env"]);
        assert!(!set.matches(std::path::Path::new(".env")).is_empty());
        assert!(
            set.matches(std::path::Path::new("config/.envrc"))
                .is_empty()
        );
        assert!(set.matches(std::path::Path::new("README.md")).is_empty());
    }

    #[test]
    fn double_star_glob_matches_under_any_directory() {
        let set = build_set(&["**/.env"]);
        assert!(!set.matches(std::path::Path::new(".env")).is_empty());
        assert!(
            !set.matches(std::path::Path::new("apps/api/.env"))
                .is_empty()
        );
    }

    #[test]
    fn directory_glob_matches_under_directory() {
        let set = build_set(&["secrets/**"]);
        assert!(
            !set.matches(std::path::Path::new("secrets/keys.txt"))
                .is_empty()
        );
        assert!(
            !set.matches(std::path::Path::new("secrets/nested/deep.txt"))
                .is_empty()
        );
        assert!(
            set.matches(std::path::Path::new("public/secrets-doc.md"))
                .is_empty()
        );
    }

    #[test]
    fn multiple_patterns_match_independently() {
        let set = build_set(&["*.env", "*.pem"]);
        assert_eq!(set.matches(std::path::Path::new("private.pem")).len(), 1);
        assert_eq!(set.matches(std::path::Path::new(".env")).len(), 1);
        assert_eq!(set.matches(std::path::Path::new("README.md")).len(), 0);
    }

    fn spec(toml: &str) -> RuleSpec {
        let mut full =
            String::from("id = \"d\"\nkind = \"git_no_denied_paths\"\nlevel = \"error\"\n");
        full.push_str(toml);
        toml::from_str(&full).unwrap()
    }

    #[test]
    fn build_accepts_optional_since() {
        assert!(build(&spec("denied = [\"*.env\"]\n")).is_ok());
        assert!(build(&spec("denied = [\"*.env\"]\nsince = \"origin/main\"\n")).is_ok());
        // `denied` is still required.
        assert!(build(&spec("since = \"origin/main\"\n")).is_err());
    }

    #[test]
    fn one_path_can_hit_multiple_patterns() {
        let set = build_set(&["secrets/**", "*.pem"]);
        // `secrets/private.pem` matches both — the rule reports
        // every matching denylist entry so the user sees the
        // full picture.
        let hits = set.matches(std::path::Path::new("secrets/private.pem"));
        assert_eq!(hits.len(), 2);
    }
}
