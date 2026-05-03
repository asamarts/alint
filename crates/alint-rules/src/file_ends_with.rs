//! `file_ends_with` — every file in scope must end with the
//! configured suffix (byte-level).
//!
//! Useful for required trailing banners ("<!-- end-of-file -->"),
//! closing magic bytes, or enforcing a generated-file sentinel.
//! For the narrower "file must end with a newline" check, prefer
//! `final_newline` — it has a dedicated fixer and integrates with
//! the text-hygiene family.
//!
//! Check-only: the correct fix is `file_append` with matching
//! content. Auto-appending implicitly could silently duplicate a
//! near-matching tail.

use std::path::Path;

use alint_core::{Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

use crate::io::read_suffix_n;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// The required suffix. Matched byte-for-byte.
    suffix: String,
}

#[derive(Debug)]
pub struct FileEndsWithRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    suffix: Vec<u8>,
}

impl Rule for FileEndsWithRule {
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
            if !self.scope.matches(&entry.path, ctx.index) {
                continue;
            }
            // Bounded read: only the trailing `suffix.len()`
            // bytes matter. Solo runs (`alint fix --only`,
            // tests) read just those bytes from the end.
            let full = ctx.root.join(&entry.path);
            let Ok(tail) = read_suffix_n(&full, self.suffix.len()) else {
                continue;
            };
            violations.extend(self.evaluate_file(ctx, &entry.path, &tail)?);
        }
        Ok(violations)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for FileEndsWithRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        if bytes.ends_with(&self.suffix) {
            return Ok(Vec::new());
        }
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| "file does not end with the required suffix".to_string());
        Ok(vec![
            Violation::new(msg).with_path(std::sync::Arc::<Path>::from(path)),
        ])
    }

    fn max_bytes_needed(&self) -> Option<usize> {
        Some(self.suffix.len())
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let _paths = spec
        .paths
        .as_ref()
        .ok_or_else(|| Error::rule_config(&spec.id, "file_ends_with requires a `paths` field"))?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.suffix.is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "file_ends_with.suffix must not be empty",
        ));
    }
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "file_ends_with has no fix op — pair with an explicit `file_append` rule if you \
             want auto-append (avoids silently duplicating near-matching suffixes).",
        ));
    }
    Ok(Box::new(FileEndsWithRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        suffix: opts.suffix.into_bytes(),
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
             kind: file_ends_with\n\
             suffix: \"\\n\"\n\
             level: error\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn build_rejects_empty_suffix() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_ends_with\n\
             paths: \"**/*\"\n\
             suffix: \"\"\n\
             level: error\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(err.contains("empty"), "unexpected: {err}");
    }

    #[test]
    fn build_rejects_fix_block() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_ends_with\n\
             paths: \"**/*\"\n\
             suffix: \"\\n\"\n\
             level: error\n\
             fix:\n  \
               file_append:\n    \
                 content: \"x\"\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn evaluate_passes_when_suffix_matches() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_ends_with\n\
             paths: \"**/*.txt\"\n\
             suffix: \"END\\n\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("a.txt", b"hello\nEND\n")]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn evaluate_fires_when_suffix_missing() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_ends_with\n\
             paths: \"**/*.txt\"\n\
             suffix: \"END\\n\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("a.txt", b"hello\n")]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1);
    }
}
