//! `for_each_file` — iterate over every file matching `select:` and
//! evaluate a nested `require:` block against each. Same mechanics as
//! [`crate::for_each_dir`] — differs only in iterating files instead of
//! directories from the `FileIndex`.
//!
//! Canonical shape — for every `tests/unit/*.rs`, require a corresponding
//! `tests/snapshots/{stem}.snap`:
//!
//! ```yaml
//! - id: unit-has-snapshot
//!   kind: for_each_file
//!   select: "tests/unit/*.rs"
//!   require:
//!     - kind: file_exists
//!       paths: "tests/snapshots/{stem}.snap"
//!   level: warning
//! ```

use alint_core::{Context, Error, Level, NestedRuleSpec, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

use crate::for_each_dir::{IterateMode, evaluate_for_each};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    select: String,
    require: Vec<NestedRuleSpec>,
}

#[derive(Debug)]
pub struct ForEachFileRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    select_scope: Scope,
    require: Vec<NestedRuleSpec>,
}

impl Rule for ForEachFileRule {
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
            IterateMode::Files,
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
            "for_each_file requires at least one nested rule under `require:`",
        ));
    }
    let select_scope = Scope::from_patterns(&[opts.select])?;
    Ok(Box::new(ForEachFileRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        select_scope,
        require: opts.require,
    }))
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

    #[test]
    fn passes_when_every_file_has_required_sibling() {
        let require: Vec<NestedRuleSpec> = vec![
            serde_yaml_ng::from_str("kind: file_exists\npaths: \"{dir}/{stem}.h\"\n").unwrap(),
        ];
        let r = ForEachFileRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            select_scope: Scope::from_patterns(&["**/*.c".to_string()]).unwrap(),
            require,
        };
        let idx = index(&[
            ("src/foo.c", false),
            ("src/foo.h", false),
            ("src/bar.c", false),
            ("src/bar.h", false),
        ]);
        let reg = registry();
        let ctx = Context {
            root: Path::new("/"),
            index: &idx,
            registry: Some(&reg),
            facts: None,
            vars: None,
        };
        let v = r.evaluate(&ctx).unwrap();
        assert!(v.is_empty(), "unexpected: {v:?}");
    }

    #[test]
    fn violates_per_missing_sibling() {
        let require: Vec<NestedRuleSpec> = vec![
            serde_yaml_ng::from_str("kind: file_exists\npaths: \"{dir}/{stem}.h\"\n").unwrap(),
        ];
        let r = ForEachFileRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            select_scope: Scope::from_patterns(&["**/*.c".to_string()]).unwrap(),
            require,
        };
        let idx = index(&[
            ("src/foo.c", false),
            ("src/foo.h", false), // matched
            ("src/bar.c", false), // no bar.h
            ("src/baz.c", false), // no baz.h
        ]);
        let reg = registry();
        let ctx = Context {
            root: Path::new("/"),
            index: &idx,
            registry: Some(&reg),
            facts: None,
            vars: None,
        };
        let v = r.evaluate(&ctx).unwrap();
        assert_eq!(v.len(), 2);
    }
}
