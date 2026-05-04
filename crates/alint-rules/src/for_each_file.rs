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

use alint_core::when::WhenExpr;
use alint_core::{
    CompiledNestedSpec, Context, Error, Level, NestedRuleSpec, Result, Rule, RuleSpec, Scope,
    Violation,
};
use serde::Deserialize;

use crate::for_each_dir::{
    IterateMode, compile_nested_require, evaluate_for_each, parse_when_iter,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    select: String,
    /// Optional per-iteration filter — typical shapes:
    /// `iter.basename matches "^[a-z]"` to skip uppercase-named
    /// files, or `not iter.has_file(...)` (always false for
    /// file iteration) to no-op the rule.
    #[serde(default)]
    when_iter: Option<String>,
    require: Vec<NestedRuleSpec>,
}

#[derive(Debug)]
pub struct ForEachFileRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    select_scope: Scope,
    when_iter: Option<WhenExpr>,
    require: Vec<CompiledNestedSpec>,
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
            self.when_iter.as_ref(),
            &self.require,
            ctx,
            IterateMode::Files,
        )
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    alint_core::reject_scope_filter_on_cross_file(spec, "for_each_file")?;
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
    let when_iter = parse_when_iter(spec, opts.when_iter.as_deref())?;
    let require = compile_nested_require(&spec.id, opts.require)?;
    Ok(Box::new(ForEachFileRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        select_scope,
        when_iter,
        require,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{FileEntry, FileIndex, RuleRegistry};
    use std::path::Path;

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

    fn registry() -> RuleRegistry {
        crate::builtin_registry()
    }

    #[test]
    fn passes_when_every_file_has_required_sibling() {
        let require: Vec<NestedRuleSpec> = vec![
            serde_yaml_ng::from_str("kind: file_exists\npaths: \"{dir}/{stem}.h\"\n").unwrap(),
        ];
        let require = compile_nested_require("t", require).unwrap();
        let r = ForEachFileRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            select_scope: Scope::from_patterns(&["**/*.c".to_string()]).unwrap(),
            when_iter: None,
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
            git_tracked: None,
            git_blame: None,
        };
        let v = r.evaluate(&ctx).unwrap();
        assert!(v.is_empty(), "unexpected: {v:?}");
    }

    #[test]
    fn violates_per_missing_sibling() {
        let require: Vec<NestedRuleSpec> = vec![
            serde_yaml_ng::from_str("kind: file_exists\npaths: \"{dir}/{stem}.h\"\n").unwrap(),
        ];
        let require = compile_nested_require("t", require).unwrap();
        let r = ForEachFileRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            select_scope: Scope::from_patterns(&["**/*.c".to_string()]).unwrap(),
            when_iter: None,
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
            git_tracked: None,
            git_blame: None,
        };
        let v = r.evaluate(&ctx).unwrap();
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn build_rejects_scope_filter_on_cross_file_rule() {
        // for_each_file is a cross-file rule (requires_full_index
        // = true); scope_filter is per-file-rules-only. The build
        // path must reject it with a clear message pointing at
        // the for_each_dir + when_iter: alternative.
        let yaml = r#"
id: t
kind: for_each_file
select: "**/*.c"
require:
  - kind: file_exists
    paths: "{dir}/{stem}.h"
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
            err.contains("for_each_file"),
            "expected message to name the cross-file kind, got: {err}",
        );
    }
}
