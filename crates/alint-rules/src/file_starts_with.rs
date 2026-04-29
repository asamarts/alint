//! `file_starts_with` — every file in scope must begin with the
//! configured prefix (byte-level).
//!
//! Useful for SPDX headers (when `file_header`'s line-oriented
//! matching is too loose), magic bytes on binary formats, or a
//! required "do not edit — generated" banner. Works on any byte
//! content, not just UTF-8.
//!
//! Check-only: the correct fix is to call `file_prepend` with
//! the same content, and having the rule do it implicitly would
//! silently duplicate the prefix on files that start with a
//! similar but non-matching string.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// The required prefix. Matched byte-for-byte.
    prefix: String,
}

#[derive(Debug)]
pub struct FileStartsWithRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    prefix: Vec<u8>,
}

impl Rule for FileStartsWithRule {
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
            if !bytes.starts_with(&self.prefix) {
                let msg = self
                    .message
                    .clone()
                    .unwrap_or_else(|| "file does not start with the required prefix".to_string());
                violations.push(
                    Violation::new(msg)
                        .with_path(&entry.path)
                        .with_location(1, 1),
                );
            }
        }
        Ok(violations)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let paths = spec
        .paths
        .as_ref()
        .ok_or_else(|| Error::rule_config(&spec.id, "file_starts_with requires a `paths` field"))?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.prefix.is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "file_starts_with.prefix must not be empty",
        ));
    }
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "file_starts_with has no fix op — pair with an explicit `file_prepend` rule if you \
             want auto-prepend (avoids silently duplicating near-matching prefixes).",
        ));
    }
    Ok(Box::new(FileStartsWithRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        prefix: opts.prefix.into_bytes(),
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
             kind: file_starts_with\n\
             prefix: \"#!/bin/sh\"\n\
             level: error\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn build_rejects_empty_prefix() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_starts_with\n\
             paths: \"**/*.sh\"\n\
             prefix: \"\"\n\
             level: error\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(err.contains("empty"), "unexpected: {err}");
    }

    #[test]
    fn build_rejects_fix_block() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_starts_with\n\
             paths: \"**/*.sh\"\n\
             prefix: \"#!/bin/sh\\n\"\n\
             level: error\n\
             fix:\n  \
               file_prepend:\n    \
                 content: \"x\"\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn evaluate_passes_when_prefix_matches() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_starts_with\n\
             paths: \"**/*.sh\"\n\
             prefix: \"#!/bin/sh\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("script.sh", b"#!/bin/sh\necho hi\n")]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "expected pass: {v:?}");
    }

    #[test]
    fn evaluate_fires_when_prefix_missing() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_starts_with\n\
             paths: \"**/*.sh\"\n\
             prefix: \"#!/bin/sh\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("script.sh", b"echo hi\n")]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1);
    }
}
