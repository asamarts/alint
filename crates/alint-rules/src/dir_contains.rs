//! `dir_contains` — every directory matching `select` must have at least
//! one direct child matching each glob in `require`. Sugar over
//! `for_each_dir` + `file_exists` for the common shape "this dir must
//! have X, Y, and Z."
//!
//! Canonical shape — every `packages/*` must have both a README and a
//! license file:
//!
//! ```yaml
//! - id: packages-have-readme-and-license
//!   kind: dir_contains
//!   select: "packages/*"
//!   require: ["README.md", "LICENSE*"]
//!   level: error
//! ```
//!
//! `require` patterns match direct-child **basenames**. Use
//! `for_each_dir` with nested rules if you need deeper semantics.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use globset::{Glob, GlobMatcher};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    select: String,
    require: RequireList,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RequireList {
    One(String),
    Many(Vec<String>),
}

impl RequireList {
    fn into_vec(self) -> Vec<String> {
        match self {
            Self::One(s) => vec![s],
            Self::Many(v) => v,
        }
    }
}

#[derive(Debug)]
pub struct DirContainsRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    select_scope: Scope,
    require_globs: Vec<String>,
    require_matchers: Vec<GlobMatcher>,
}

impl Rule for DirContainsRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        self.level
    }
    fn policy_url(&self) -> Option<&str> {
        self.policy_url.as_deref()
    }

    fn requires_full_index(&self) -> bool {
        // Cross-file: every selected dir's verdict depends on
        // its current child set, not just the diff. Per roadmap,
        // opts out of `--changed` filtering.
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        for dir in ctx.index.dirs() {
            if !self.select_scope.matches(&dir.path, ctx.index) {
                continue;
            }
            // v0.9.8: collect direct child basenames once per dir
            // (cheap; iterator borrows into the Arc<Path>), then
            // run each matcher over that small slice rather than
            // scanning all entries per (dir, matcher) pair.
            // O(D × children) replaces the prior O(D × R × N).
            //
            // dir_contains accepts BOTH file and subdir basenames
            // (a require of `src` matches a `src/` subdir as well
            // as a `src` file), so we use `children_of` directly
            // instead of `file_basenames_of` which would filter
            // out subdirs.
            let basenames: Vec<&str> = ctx
                .index
                .children_of(&dir.path)
                .iter()
                .filter_map(|&i| {
                    ctx.index.entries[i]
                        .path
                        .file_name()
                        .and_then(|s| s.to_str())
                })
                .collect();
            for (i, matcher) in self.require_matchers.iter().enumerate() {
                let found = basenames.iter().any(|b| matcher.is_match(b));
                if !found {
                    let glob = &self.require_globs[i];
                    let msg = self.format_message(&dir.path, glob);
                    violations.push(Violation::new(msg).with_path(dir.path.clone()));
                }
            }
        }
        Ok(violations)
    }
}

impl DirContainsRule {
    fn format_message(&self, dir: &Path, glob: &str) -> String {
        if let Some(user) = self.message.as_deref() {
            let dir_str = dir.display().to_string();
            let glob_str = glob.to_string();
            return alint_core::template::render_message(user, |ns, key| match (ns, key) {
                ("ctx", "dir") => Some(dir_str.clone()),
                ("ctx", "require") => Some(glob_str.clone()),
                _ => None,
            });
        }
        format!("{} is missing a child matching {:?}", dir.display(), glob)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    alint_core::reject_scope_filter_on_cross_file(spec, "dir_contains")?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let require_globs = opts.require.into_vec();
    if require_globs.is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "dir_contains `require` must not be empty",
        ));
    }
    let select_scope = Scope::from_patterns(&[opts.select])?;
    let mut require_matchers = Vec::with_capacity(require_globs.len());
    for pat in &require_globs {
        let glob = Glob::new(pat).map_err(|source| Error::Glob {
            pattern: pat.clone(),
            source,
        })?;
        require_matchers.push(glob.compile_matcher());
    }
    Ok(Box::new(DirContainsRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        select_scope,
        require_globs,
        require_matchers,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{FileEntry, FileIndex};

    fn index(entries: &[(&str, bool)]) -> FileIndex {
        FileIndex::from_entries(
            entries
                .iter()
                .map(|(p, is_dir)| FileEntry {
                    path: std::path::Path::new(p).into(),
                    is_dir: *is_dir,
                    size: 1,
                })
                .collect(),
        )
    }

    fn rule(select: &str, require: &[&str]) -> DirContainsRule {
        let globs: Vec<String> = require.iter().map(|s| (*s).to_string()).collect();
        let matchers: Vec<GlobMatcher> = globs
            .iter()
            .map(|p| Glob::new(p).unwrap().compile_matcher())
            .collect();
        DirContainsRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            select_scope: Scope::from_patterns(&[select.to_string()]).unwrap(),
            require_globs: globs,
            require_matchers: matchers,
        }
    }

    fn eval(rule: &DirContainsRule, files: &[(&str, bool)]) -> Vec<Violation> {
        let idx = index(files);
        let ctx = Context {
            root: Path::new("/"),
            index: &idx,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
        };
        rule.evaluate(&ctx).unwrap()
    }

    #[test]
    fn passes_when_every_require_satisfied() {
        let r = rule("packages/*", &["README.md", "LICENSE*"]);
        let v = eval(
            &r,
            &[
                ("packages", true),
                ("packages/a", true),
                ("packages/a/README.md", false),
                ("packages/a/LICENSE-APACHE", false),
                ("packages/b", true),
                ("packages/b/README.md", false),
                ("packages/b/LICENSE", false),
            ],
        );
        assert!(v.is_empty(), "unexpected: {v:?}");
    }

    #[test]
    fn violates_once_per_missing_require_per_dir() {
        let r = rule("packages/*", &["README.md", "LICENSE*"]);
        let v = eval(
            &r,
            &[
                ("packages", true),
                ("packages/a", true),
                ("packages/a/README.md", false),
                // missing LICENSE
            ],
        );
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("LICENSE"));
    }

    #[test]
    fn multiple_missing_across_multiple_dirs() {
        let r = rule("packages/*", &["README.md", "LICENSE*"]);
        let v = eval(
            &r,
            &[
                ("packages", true),
                ("packages/a", true),
                // a: missing both
                ("packages/b", true),
                ("packages/b/README.md", false),
                // b: missing LICENSE
            ],
        );
        assert_eq!(v.len(), 3);
    }

    #[test]
    fn directory_children_count_too() {
        // `src` as a required name — matches a subdir named `src`.
        let r = rule("packages/*", &["src"]);
        let v = eval(
            &r,
            &[
                ("packages", true),
                ("packages/a", true),
                ("packages/a/src", true),
            ],
        );
        assert!(v.is_empty());
    }

    #[test]
    fn require_can_be_single_string() {
        let yaml = r"
select: 'packages/*'
require: 'README.md'
";
        let opts: Options = serde_yaml_ng::from_str(yaml).unwrap();
        assert!(matches!(opts.require, RequireList::One(_)));
    }

    #[test]
    fn no_matching_dirs_means_no_violations() {
        let r = rule("packages/*", &["README.md"]);
        let v = eval(&r, &[("src", true), ("src/foo", true)]);
        assert!(v.is_empty());
    }

    #[test]
    fn build_rejects_scope_filter_on_cross_file_rule() {
        // dir_contains is a cross-file rule (requires_full_index =
        // true); scope_filter is per-file-rules-only. The build
        // path must reject it with a clear message pointing at
        // the for_each_dir + when_iter: alternative.
        let yaml = r#"
id: t
kind: dir_contains
select: "packages/*"
require: ["README.md"]
level: error
scope_filter:
  has_ancestor: Cargo.toml
"#;
        let spec = crate::test_support::spec_yaml(yaml);
        let err = build(&spec).unwrap_err().to_string();
        assert!(
            err.contains("scope_filter is supported on per-file rules only"),
            "expected per-file-only message, got: {err}",
        );
        assert!(
            err.contains("dir_contains"),
            "expected message to name the cross-file kind, got: {err}",
        );
    }
}
