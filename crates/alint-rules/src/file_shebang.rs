//! `file_shebang` — first line of each file in scope must
//! match a shebang regex.
//!
//! Pairs naturally with [`crate::executable_has_shebang`]
//! (which checks shebang *presence* on `+x` files) and
//! [`crate::shebang_has_executable`] (which checks the
//! reverse, that a shebang file is `+x`). `file_shebang`
//! checks shebang *shape*: enforce a specific interpreter
//! pinning, ban `#!/usr/bin/env <foo>` in favour of an
//! absolute path, require `set -euo pipefail` immediately
//! after, etc.
//!
//! The default `shebang:` regex (when the user omits the
//! field) is just `^#!`, which only enforces presence.
//! Most useful configs supply a tighter regex like
//! `^#!/usr/bin/env bash$` or `^#!/bin/bash -euo pipefail$`.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Options {
    #[serde(default = "default_shebang")]
    shebang: String,
}

fn default_shebang() -> String {
    "^#!".to_string()
}

#[derive(Debug)]
pub struct FileShebangRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    pattern_src: String,
    pattern: Regex,
}

impl Rule for FileShebangRule {
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
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path) {
                continue;
            }
            let full = ctx.root.join(&entry.path);
            let Ok(bytes) = std::fs::read(&full) else {
                continue;
            };
            let first_line = match std::str::from_utf8(&bytes) {
                Ok(text) => text.split('\n').next().unwrap_or(""),
                Err(_) => "",
            };
            if !self.pattern.is_match(first_line) {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "first line {first_line:?} does not match required shebang /{}/",
                        self.pattern_src
                    )
                });
                violations.push(
                    Violation::new(msg)
                        .with_path(entry.path.clone())
                        .with_location(1, 1),
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
            "file_shebang requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let pattern = Regex::new(&opts.shebang)
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid shebang regex: {e}")))?;
    Ok(Box::new(FileShebangRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        pattern_src: opts.shebang,
        pattern,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, spec_yaml, tempdir_with_files};

    #[test]
    fn build_rejects_missing_paths_field() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_shebang\n\
             shebang: \"^#!/bin/sh\"\n\
             level: error\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn build_rejects_invalid_regex() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_shebang\n\
             paths: \"**/*.sh\"\n\
             shebang: \"[unterminated\"\n\
             level: error\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn evaluate_passes_when_shebang_matches() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_shebang\n\
             paths: \"**/*.sh\"\n\
             shebang: \"^#!/(usr/)?bin/(env )?(ba)?sh\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("a.sh", b"#!/usr/bin/env bash\necho hi\n")]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "shebang should match: {v:?}");
    }

    #[test]
    fn evaluate_fires_when_shebang_missing() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_shebang\n\
             paths: \"**/*.sh\"\n\
             shebang: \"^#!\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("a.sh", b"echo hi\n")]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn evaluate_only_inspects_first_line() {
        // A shebang on line 5 must NOT satisfy the rule —
        // shebangs are line-1-only.
        let spec = spec_yaml(
            "id: t\n\
             kind: file_shebang\n\
             paths: \"**/*.sh\"\n\
             shebang: \"^#!/bin/sh\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("a.sh", b"echo first\n#!/bin/sh\n")]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "shebang on line 2 shouldn't satisfy");
    }
}
