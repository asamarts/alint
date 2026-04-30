//! `every_matching_has` — every file OR directory matching `select` must
//! satisfy every rule in `require`. Sugar over `for_each_file` +
//! `for_each_dir`: one rule that iterates both entry kinds so users who
//! don't care whether a glob matches files or dirs can write a single
//! rule instead of two.
//!
//! ```yaml
//! - id: every-pkg-has-readme
//!   kind: every_matching_has
//!   select: "packages/*"   # matches dirs today; might also match files tomorrow
//!   require:
//!     - kind: file_exists
//!       paths: "{path}/README.md"
//!   level: error
//! ```

use alint_core::when::WhenExpr;
use alint_core::{Context, Error, Level, NestedRuleSpec, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

use crate::for_each_dir::{IterateMode, evaluate_for_each, parse_when_iter};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    select: String,
    #[serde(default)]
    when_iter: Option<String>,
    require: Vec<NestedRuleSpec>,
}

#[derive(Debug)]
pub struct EveryMatchingHasRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    select_scope: Scope,
    when_iter: Option<WhenExpr>,
    require: Vec<NestedRuleSpec>,
}

impl Rule for EveryMatchingHasRule {
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
        // Cross-file: every entry matching `select` must satisfy
        // `require`, regardless of whether it (or its required
        // partners) was in the diff. Per roadmap, opts out of
        // `--changed` filtering.
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        evaluate_for_each(
            &self.id,
            self.level,
            &self.select_scope,
            self.when_iter.as_ref(),
            &self.require,
            ctx,
            IterateMode::Both,
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
            "every_matching_has requires at least one nested rule under `require:`",
        ));
    }
    let select_scope = Scope::from_patterns(&[opts.select])?;
    let when_iter = parse_when_iter(spec, opts.when_iter.as_deref())?;
    Ok(Box::new(EveryMatchingHasRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        select_scope,
        when_iter,
        require: opts.require,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{FileEntry, FileIndex, RuleRegistry};
    use std::path::Path;

    fn index(entries: &[(&str, bool)]) -> FileIndex {
        FileIndex {
            entries: entries
                .iter()
                .map(|(p, is_dir)| FileEntry {
                    path: std::path::Path::new(p).into(),
                    is_dir: *is_dir,
                    size: 1,
                })
                .collect(),
        }
    }

    fn registry() -> RuleRegistry {
        crate::builtin_registry()
    }

    #[test]
    fn iterates_both_files_and_dirs() {
        // `packages/*` matches a dir `packages/a` AND a file `packages/x.md`
        // (rare but possible). The rule should evaluate require against
        // both.
        let require: Vec<NestedRuleSpec> =
            vec![serde_yaml_ng::from_str("kind: file_exists\npaths: \"{path}\"\n").unwrap()];
        let r = EveryMatchingHasRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            select_scope: Scope::from_patterns(&["packages/*".to_string()]).unwrap(),
            when_iter: None,
            require,
        };
        let idx = index(&[
            ("packages", true),
            ("packages/a", true),
            ("packages/x.md", false),
        ]);
        let reg = registry();
        let ctx = Context {
            root: Path::new("/"),
            index: &idx,
            registry: Some(&reg),
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
        };
        let v = r.evaluate(&ctx).unwrap();
        // `{path}` resolves to "packages/a" (dir) and "packages/x.md" (file).
        // The dir "packages/a" is not a file in the index — file_exists
        // cannot find it because file_exists iterates files(), not dirs().
        // So we expect one violation for the dir case and none for the file.
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].path.as_deref(), Some(Path::new("packages/a")));
    }
}
