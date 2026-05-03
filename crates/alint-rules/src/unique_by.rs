//! `unique_by` — flag any group of files (matching `select:`) that share
//! the same rendered `key`. The key is a path template evaluated per
//! matched file; default is `{basename}` (catches any two files with the
//! same name regardless of directory).
//!
//! Canonical shape — every Rust source stem must be unique repo-wide:
//!
//! ```yaml
//! - id: unique-rs-stems
//!   kind: unique_by
//!   select: "**/*.rs"
//!   key: "{stem}"
//!   level: warning
//! ```
//!
//! Violations are emitted **one per collision group**, anchored on the
//! lexicographically-first path of the group; the message enumerates
//! every colliding file. For groups of N, that is one violation (not N),
//! because the collision is a single fact.

use std::collections::BTreeMap;

use alint_core::template::{PathTokens, render_message, render_path};
use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    select: String,
    #[serde(default = "default_key")]
    key: String,
}

fn default_key() -> String {
    "{basename}".to_string()
}

#[derive(Debug)]
pub struct UniqueByRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    select_scope: Scope,
    key_template: String,
}

impl Rule for UniqueByRule {
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
        // Cross-file: detecting duplicate keys is only valid over
        // the full set. A new file in the diff might collide with
        // an unchanged-but-existing file elsewhere — invisible if
        // we only see the diff. Per roadmap, opts out of
        // `--changed` filtering.
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        // BTreeMap gives a stable (sorted) iteration order →
        // deterministic output. Storing `Arc<Path>` re-uses the
        // walker's per-file allocation rather than copying bytes
        // through a `PathBuf`.
        let mut groups: BTreeMap<String, Vec<std::sync::Arc<std::path::Path>>> = BTreeMap::new();
        for entry in ctx.index.files() {
            if !self.select_scope.matches(&entry.path, ctx.index) {
                continue;
            }
            let tokens = PathTokens::from_path(&entry.path);
            let key = render_path(&self.key_template, &tokens);
            if key.is_empty() {
                // Skip files whose key renders to the empty string — likely a
                // missing component like `{parent_name}` on a root-level file.
                continue;
            }
            groups.entry(key).or_default().push(entry.path.clone());
        }
        let mut violations = Vec::new();
        for (key, mut paths) in groups {
            if paths.len() <= 1 {
                continue;
            }
            paths.sort();
            let anchor = paths[0].clone();
            let msg = self.format_message(&key, &paths);
            violations.push(Violation::new(msg).with_path(anchor));
        }
        Ok(violations)
    }
}

impl UniqueByRule {
    fn format_message(&self, key: &str, paths: &[std::sync::Arc<std::path::Path>]) -> String {
        let paths_joined = paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        if let Some(user) = self.message.as_deref() {
            let key_str = key.to_string();
            let paths_str = paths_joined.clone();
            let count = paths.len().to_string();
            return render_message(user, |ns, k| match (ns, k) {
                ("ctx", "key") => Some(key_str.clone()),
                ("ctx", "paths") => Some(paths_str.clone()),
                ("ctx", "count") => Some(count.clone()),
                _ => None,
            });
        }
        format!(
            "duplicate key {:?} shared by {} file(s): {}",
            key,
            paths.len(),
            paths_joined,
        )
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    alint_core::reject_scope_filter_on_cross_file(spec, "unique_by")?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.key.trim().is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "unique_by `key` must not be empty",
        ));
    }
    let select_scope = Scope::from_patterns(&[opts.select])?;
    Ok(Box::new(UniqueByRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        select_scope,
        key_template: opts.key,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{FileEntry, FileIndex};
    use std::path::Path;

    fn index(files: &[&str]) -> FileIndex {
        FileIndex::from_entries(
            files
                .iter()
                .map(|p| FileEntry {
                    path: std::path::Path::new(p).into(),
                    is_dir: false,
                    size: 1,
                })
                .collect(),
        )
    }

    fn rule(select: &str, key: &str) -> UniqueByRule {
        UniqueByRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            select_scope: Scope::from_patterns(&[select.to_string()]).unwrap(),
            key_template: key.to_string(),
        }
    }

    fn eval(rule: &UniqueByRule, files: &[&str]) -> Vec<Violation> {
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
    fn passes_when_every_key_unique() {
        let r = rule("**/*.rs", "{stem}");
        let v = eval(&r, &["src/foo.rs", "src/bar.rs", "tests/baz.rs"]);
        assert!(v.is_empty(), "unexpected: {v:?}");
    }

    #[test]
    fn flags_stem_collision() {
        let r = rule("**/*.rs", "{stem}");
        let v = eval(&r, &["src/mod1/foo.rs", "src/mod2/foo.rs"]);
        assert_eq!(v.len(), 1);
        // Anchor is lex-smallest of the collision group.
        assert_eq!(v[0].path.as_deref(), Some(Path::new("src/mod1/foo.rs")));
        assert!(v[0].message.contains("src/mod1/foo.rs"));
        assert!(v[0].message.contains("src/mod2/foo.rs"));
    }

    #[test]
    fn one_violation_per_group_regardless_of_group_size() {
        let r = rule("**/*.rs", "{stem}");
        let v = eval(
            &r,
            &[
                "src/a/foo.rs",
                "src/b/foo.rs",
                "src/c/foo.rs", // 3-way collision on "foo"
                "src/bar.rs",   // unique
            ],
        );
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains('3'));
    }

    #[test]
    fn multiple_independent_groups() {
        let r = rule("**/*.rs", "{stem}");
        let v = eval(
            &r,
            &[
                "src/a/foo.rs",
                "src/b/foo.rs", // group "foo"
                "tests/bar.rs",
                "integration/bar.rs", // group "bar"
                "src/solo.rs",
            ],
        );
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn default_key_is_basename() {
        // No key option = default {basename}: collisions require identical
        // filename including extension.
        let r = UniqueByRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            select_scope: Scope::from_patterns(&["**/*".to_string()]).unwrap(),
            key_template: default_key(),
        };
        let v = eval(&r, &["src/a/mod.rs", "src/b/mod.rs"]);
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn different_extensions_same_stem_are_not_colliding_by_basename() {
        let r = UniqueByRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            select_scope: Scope::from_patterns(&["**/*".to_string()]).unwrap(),
            key_template: default_key(),
        };
        let v = eval(&r, &["src/foo.rs", "src/foo.md"]);
        assert!(v.is_empty());
    }

    #[test]
    fn empty_key_rendering_skips_entry() {
        // `{parent_name}` on a root-level file renders to "" — excluded.
        let r = rule("*.md", "{parent_name}");
        let v = eval(&r, &["README.md", "CHANGELOG.md"]);
        assert!(v.is_empty());
    }

    #[test]
    fn message_template_substitution() {
        let r = UniqueByRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: Some("{{ctx.count}} files share stem {{ctx.key}}".into()),
            select_scope: Scope::from_patterns(&["**/*.rs".to_string()]).unwrap(),
            key_template: "{stem}".into(),
        };
        let v = eval(&r, &["a/foo.rs", "b/foo.rs"]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].message, "2 files share stem foo");
    }

    #[test]
    fn build_rejects_scope_filter_on_cross_file_rule() {
        // unique_by is a cross-file rule (requires_full_index =
        // true); scope_filter is per-file-rules-only. The build
        // path must reject it with a clear message pointing at
        // the for_each_dir + when_iter: alternative.
        let yaml = r#"
id: t
kind: unique_by
select: "**/*.rs"
key: "{stem}"
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
            err.contains("unique_by"),
            "expected message to name the cross-file kind, got: {err}",
        );
    }
}
