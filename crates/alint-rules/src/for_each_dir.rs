//! `for_each_dir` — iterate over every directory matching `select:` and
//! evaluate a nested `require:` block against each. Path-template tokens
//! in the nested specs are pre-substituted per iteration using the
//! iterated directory as the anchor.
//!
//! Token conventions (shared with `for_each_file` and `pair`):
//!
//! - `{path}` — full relative path of the iterated entry.
//! - `{dir}`  — parent directory of the iterated entry.
//! - `{basename}` — name of the iterated entry.
//! - `{stem}` — name with the final extension stripped.
//! - `{ext}` — final extension without the dot.
//! - `{parent_name}` — name of the entry's parent directory.
//!
//! When iterating *directories*, use `{path}` to name the iterated dir
//! itself (e.g. `"{path}/mod.rs"` to require a `mod.rs` inside it). Use
//! `{dir}` only when you need the parent of the matched entry.
//!
//! Canonical shape — for every direct subdirectory of `src/`, require a
//! `mod.rs`:
//!
//! ```yaml
//! - id: every-module-has-mod
//!   kind: for_each_dir
//!   select: "src/*"
//!   require:
//!     - kind: file_exists
//!       paths: "{path}/mod.rs"
//!   level: error
//! ```

use alint_core::template::PathTokens;
use alint_core::{Context, Error, Level, NestedRuleSpec, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    select: String,
    require: Vec<NestedRuleSpec>,
}

#[derive(Debug)]
pub struct ForEachDirRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    select_scope: Scope,
    require: Vec<NestedRuleSpec>,
}

impl Rule for ForEachDirRule {
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
        evaluate_for_each(
            &self.id,
            self.level,
            &self.select_scope,
            &self.require,
            ctx,
            IterateMode::Dirs,
        )
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.require.is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "for_each_dir requires at least one nested rule under `require:`",
        ));
    }
    let select_scope = Scope::from_patterns(&[opts.select])?;
    Ok(Box::new(ForEachDirRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        select_scope,
        require: opts.require,
    }))
}

/// What to iterate in [`evaluate_for_each`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IterateMode {
    Dirs,
    Files,
    /// Both files and dirs (dirs first) — used by `every_matching_has`.
    Both,
}

/// Shared evaluation logic for `for_each_dir`, `for_each_file`, and
/// `every_matching_has`. `mode` selects which entries to iterate.
pub(crate) fn evaluate_for_each(
    parent_id: &str,
    level: Level,
    select_scope: &Scope,
    require: &[NestedRuleSpec],
    ctx: &Context<'_>,
    mode: IterateMode,
) -> Result<Vec<Violation>> {
    let Some(registry) = ctx.registry else {
        return Err(Error::Other(format!(
            "rule {parent_id}: nested-rule evaluation needs a RuleRegistry in the Context \
             (likely an Engine constructed without one)",
        )));
    };

    let entries: Box<dyn Iterator<Item = _>> = match mode {
        IterateMode::Dirs => Box::new(ctx.index.dirs()),
        IterateMode::Files => Box::new(ctx.index.files()),
        IterateMode::Both => Box::new(ctx.index.dirs().chain(ctx.index.files())),
    };

    let mut violations = Vec::new();
    for entry in entries {
        if !select_scope.matches(&entry.path) {
            continue;
        }
        let tokens = PathTokens::from_path(&entry.path);
        for (i, nested) in require.iter().enumerate() {
            let nested_spec = nested.instantiate(parent_id, i, level, &tokens);
            let nested_rule = match registry.build(&nested_spec) {
                Ok(r) => r,
                Err(e) => {
                    violations.push(
                        Violation::new(format!(
                            "{parent_id}: failed to build nested rule #{i} for {}: {e}",
                            entry.path.display()
                        ))
                        .with_path(&entry.path),
                    );
                    continue;
                }
            };
            let nested_violations = nested_rule.evaluate(ctx)?;
            for mut v in nested_violations {
                if v.path.is_none() {
                    v.path = Some(entry.path.clone());
                }
                violations.push(v);
            }
        }
    }
    Ok(violations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{FileEntry, FileIndex, RuleRegistry};
    use std::path::{Path, PathBuf};

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

    fn registry() -> RuleRegistry {
        crate::builtin_registry()
    }

    fn eval_with(rule: &ForEachDirRule, files: &[(&str, bool)]) -> Vec<Violation> {
        let idx = index(files);
        let reg = registry();
        let ctx = Context {
            root: Path::new("/"),
            index: &idx,
            registry: Some(&reg),
            facts: None,
            vars: None,
        };
        rule.evaluate(&ctx).unwrap()
    }

    fn rule(select: &str, require: Vec<NestedRuleSpec>) -> ForEachDirRule {
        ForEachDirRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            select_scope: Scope::from_patterns(&[select.to_string()]).unwrap(),
            require,
        }
    }

    fn require_file_exists(path: &str) -> NestedRuleSpec {
        // Build via YAML to exercise the same path production users take.
        let yaml = format!("kind: file_exists\npaths: \"{path}\"\n");
        serde_yaml_ng::from_str(&yaml).unwrap()
    }

    #[test]
    fn passes_when_every_dir_has_required_file() {
        let r = rule("src/*", vec![require_file_exists("{path}/mod.rs")]);
        let v = eval_with(
            &r,
            &[
                ("src", true),
                ("src/foo", true),
                ("src/foo/mod.rs", false),
                ("src/bar", true),
                ("src/bar/mod.rs", false),
            ],
        );
        assert!(v.is_empty(), "unexpected: {v:?}");
    }

    #[test]
    fn violates_when_a_dir_missing_required_file() {
        let r = rule("src/*", vec![require_file_exists("{path}/mod.rs")]);
        let v = eval_with(
            &r,
            &[
                ("src", true),
                ("src/foo", true),
                ("src/foo/mod.rs", false),
                ("src/bar", true), // no mod.rs
            ],
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].path.as_deref(), Some(Path::new("src/bar")));
    }

    #[test]
    fn no_matched_dirs_means_no_violations() {
        let r = rule("components/*", vec![require_file_exists("{dir}/index.tsx")]);
        let v = eval_with(&r, &[("src", true), ("src/foo", true)]);
        assert!(v.is_empty());
    }

    #[test]
    fn every_require_rule_evaluated_per_dir() {
        let r = rule(
            "src/*",
            vec![
                require_file_exists("{path}/mod.rs"),
                require_file_exists("{path}/README.md"),
            ],
        );
        let v = eval_with(
            &r,
            &[
                ("src", true),
                ("src/foo", true),
                ("src/foo/mod.rs", false), // has mod.rs, missing README
            ],
        );
        assert_eq!(v.len(), 1);
        assert!(
            v[0].message.contains("README"),
            "expected README in message; got {:?}",
            v[0].message
        );
    }
}
