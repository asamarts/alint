//! `dir_only_contains` — every direct child file of a directory matching
//! `select:` must match at least one glob in `allow:`. Subdirectories are
//! not checked (use `dir_absent` if you need to forbid nested directories).
//!
//! Canonical shape — `src/` subdirectories may only contain Rust sources:
//!
//! ```yaml
//! - id: src-only-rs
//!   kind: dir_only_contains
//!   select: "src/*"
//!   allow: ["*.rs", "README.md"]
//!   level: error
//! ```
//!
//! `allow` patterns match the CHILD's basename — not the full path — so
//! `"*.rs"` matches any `.rs` file regardless of its directory.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    select: String,
    allow: AllowList,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AllowList {
    One(String),
    Many(Vec<String>),
}

impl AllowList {
    fn into_vec(self) -> Vec<String> {
        match self {
            Self::One(s) => vec![s],
            Self::Many(v) => v,
        }
    }
}

#[derive(Debug)]
pub struct DirOnlyContainsRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    select_scope: Scope,
    allow_globs: Vec<String>,
    allow_matcher: GlobSet,
}

impl Rule for DirOnlyContainsRule {
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
        for dir in ctx.index.dirs() {
            if !self.select_scope.matches(&dir.path) {
                continue;
            }
            for file in ctx.index.files() {
                if !is_direct_child(&file.path, &dir.path) {
                    continue;
                }
                let Some(basename) = file.path.file_name().and_then(|s| s.to_str()) else {
                    continue;
                };
                if self.allow_matcher.is_match(basename) {
                    continue;
                }
                let msg = self.format_message(&dir.path, &file.path, basename);
                violations.push(Violation::new(msg).with_path(&file.path));
            }
        }
        Ok(violations)
    }
}

impl DirOnlyContainsRule {
    fn format_message(&self, dir: &Path, file: &Path, basename: &str) -> String {
        if let Some(user) = self.message.as_deref() {
            let dir_str = dir.display().to_string();
            let file_str = file.display().to_string();
            let basename_str = basename.to_string();
            return alint_core::template::render_message(user, |ns, key| match (ns, key) {
                ("ctx", "dir") => Some(dir_str.clone()),
                ("ctx", "file") => Some(file_str.clone()),
                ("ctx", "basename") => Some(basename_str.clone()),
                _ => None,
            });
        }
        format!(
            "{} is not allowed in {} (allow: [{}])",
            file.display(),
            dir.display(),
            self.allow_globs.join(", "),
        )
    }
}

fn is_direct_child(child: &Path, parent: &Path) -> bool {
    child.parent() == Some(parent)
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let allow_globs = opts.allow.into_vec();
    if allow_globs.is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "dir_only_contains `allow` must not be empty",
        ));
    }
    let select_scope = Scope::from_patterns(&[opts.select])?;
    let mut builder = GlobSetBuilder::new();
    for pat in &allow_globs {
        let glob = Glob::new(pat).map_err(|source| Error::Glob {
            pattern: pat.clone(),
            source,
        })?;
        builder.add(glob);
    }
    let allow_matcher = builder.build().map_err(|source| Error::Glob {
        pattern: allow_globs.join(","),
        source,
    })?;
    Ok(Box::new(DirOnlyContainsRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        select_scope,
        allow_globs,
        allow_matcher,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{FileEntry, FileIndex};
    use std::path::PathBuf;

    fn index(entries: &[(&str, bool)]) -> FileIndex {
        FileIndex {
            entries: entries
                .iter()
                .map(|(p, is_dir)| FileEntry {
                    path: PathBuf::from(p),
                    is_dir: *is_dir,
                    size: 1,
                })
                .collect(),
        }
    }

    fn rule(select: &str, allow: &[&str]) -> DirOnlyContainsRule {
        let allow_globs: Vec<String> = allow.iter().map(|s| (*s).to_string()).collect();
        let mut builder = GlobSetBuilder::new();
        for p in &allow_globs {
            builder.add(Glob::new(p).unwrap());
        }
        DirOnlyContainsRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            select_scope: Scope::from_patterns(&[select.to_string()]).unwrap(),
            allow_globs,
            allow_matcher: builder.build().unwrap(),
        }
    }

    fn eval(rule: &DirOnlyContainsRule, files: &[(&str, bool)]) -> Vec<Violation> {
        let idx = index(files);
        let ctx = Context {
            root: Path::new("/"),
            index: &idx,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
        };
        rule.evaluate(&ctx).unwrap()
    }

    #[test]
    fn passes_when_every_child_allowed() {
        let r = rule("src/*", &["*.rs", "mod.rs"]);
        let v = eval(
            &r,
            &[
                ("src", true),
                ("src/foo", true),
                ("src/foo/lib.rs", false),
                ("src/foo/mod.rs", false),
                ("src/bar", true),
                ("src/bar/main.rs", false),
            ],
        );
        assert!(v.is_empty(), "unexpected: {v:?}");
    }

    #[test]
    fn flags_disallowed_child() {
        let r = rule("src/*", &["*.rs"]);
        let v = eval(
            &r,
            &[
                ("src", true),
                ("src/foo", true),
                ("src/foo/lib.rs", false),
                ("src/foo/README.md", false), // disallowed
            ],
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].path.as_deref(), Some(Path::new("src/foo/README.md")));
    }

    #[test]
    fn multiple_disallowed_children_emit_multiple_violations() {
        let r = rule("src/*", &["*.rs"]);
        let v = eval(
            &r,
            &[
                ("src", true),
                ("src/foo", true),
                ("src/foo/a.rs", false),
                ("src/foo/a.md", false),   // disallowed
                ("src/foo/a.json", false), // disallowed
            ],
        );
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn subdirectories_are_not_flagged() {
        // `src/foo` is an iterated dir. Its child `src/foo/inner` is a
        // subdirectory — we only check files, so it passes.
        let r = rule("src/*", &["*.rs"]);
        let v = eval(
            &r,
            &[
                ("src", true),
                ("src/foo", true),
                ("src/foo/a.rs", false),
                ("src/foo/inner", true), // subdirectory — skipped
            ],
        );
        assert!(v.is_empty());
    }

    #[test]
    fn deeper_files_are_not_direct_children() {
        // A file two levels below the iterated dir is not a direct child, so
        // it is not subject to this rule.
        let r = rule("src/*", &["*.rs"]);
        let v = eval(
            &r,
            &[
                ("src", true),
                ("src/foo", true),
                ("src/foo/a.rs", false),
                ("src/foo/inner", true),
                ("src/foo/inner/weird.bin", false), // not a direct child of src/foo
            ],
        );
        assert!(v.is_empty());
    }

    #[test]
    fn no_matched_dirs_means_no_violations() {
        let r = rule("components/*", &["*.tsx"]);
        let v = eval(&r, &[("src", true), ("src/foo", true)]);
        assert!(v.is_empty());
    }

    #[test]
    fn allow_can_be_single_string() {
        let yaml = r"
select: src/*
allow: '*.rs'
";
        let opts: super::Options = serde_yaml_ng::from_str(yaml).unwrap();
        assert!(matches!(opts.allow, super::AllowList::One(_)));
    }

    #[test]
    fn allow_can_be_list() {
        let yaml = r#"
select: src/*
allow: ["*.rs", "*.toml"]
"#;
        let opts: super::Options = serde_yaml_ng::from_str(yaml).unwrap();
        assert!(matches!(opts.allow, super::AllowList::Many(_)));
    }
}
