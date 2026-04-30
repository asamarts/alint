//! `file_content_matches` — every file in scope must match a regex.

use alint_core::{Context, Error, FixSpec, Fixer, Level, Result, Rule, RuleSpec, Scope, Violation};
use regex::Regex;
use serde::Deserialize;

use crate::fixers::FileAppendFixer;

#[derive(Debug, Deserialize)]
struct Options {
    pattern: String,
}

#[derive(Debug)]
pub struct FileContentMatchesRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    pattern_src: String,
    pattern: Regex,
    fixer: Option<FileAppendFixer>,
}

impl Rule for FileContentMatchesRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        self.level
    }
    fn policy_url(&self) -> Option<&str> {
        self.policy_url.as_deref()
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path) {
                continue;
            }
            let full = ctx.root.join(&entry.path);
            let bytes = match std::fs::read(&full) {
                Ok(b) => b,
                Err(e) => {
                    violations.push(
                        Violation::new(format!("could not read file: {e}"))
                            .with_path(entry.path.clone()),
                    );
                    continue;
                }
            };
            let Ok(text) = std::str::from_utf8(&bytes) else {
                violations.push(
                    Violation::new("file is not valid UTF-8; cannot match regex")
                        .with_path(entry.path.clone()),
                );
                continue;
            };
            if !self.pattern.is_match(text) {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "content does not match required pattern /{}/",
                        self.pattern_src
                    )
                });
                violations.push(Violation::new(msg).with_path(entry.path.clone()));
            }
        }
        Ok(violations)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "file_content_matches requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let pattern = Regex::new(&opts.pattern)
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid pattern: {e}")))?;
    let fixer = match &spec.fix {
        Some(FixSpec::FileAppend { file_append }) => {
            let source = alint_core::resolve_content_source(
                &spec.id,
                "file_append",
                &file_append.content,
                &file_append.content_from,
            )?;
            Some(FileAppendFixer::new(source))
        }
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "fix.{} is not compatible with file_content_matches",
                    other.op_name()
                ),
            ));
        }
        None => None,
    };
    Ok(Box::new(FileContentMatchesRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        pattern_src: opts.pattern,
        pattern,
        fixer,
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
             kind: file_content_matches\n\
             pattern: \".*\"\n\
             level: error\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn build_rejects_invalid_regex() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_content_matches\n\
             paths: \"**/*\"\n\
             pattern: \"[unterminated\"\n\
             level: error\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn evaluate_passes_when_pattern_matches() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_content_matches\n\
             paths: \"LICENSE\"\n\
             pattern: \"Apache License\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) =
            tempdir_with_files(&[("LICENSE", b"Apache License Version 2.0, January 2004\n")]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "pattern should match: {v:?}");
    }

    #[test]
    fn evaluate_fires_when_pattern_missing() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_content_matches\n\
             paths: \"LICENSE\"\n\
             pattern: \"Apache License\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("LICENSE", b"MIT License\n\nCopyright ...\n")]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn evaluate_skips_files_outside_scope() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_content_matches\n\
             paths: \"LICENSE\"\n\
             pattern: \"Apache\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("README.md", b"no apache here")]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "out-of-scope shouldn't fire: {v:?}");
    }

    #[test]
    fn evaluate_fires_with_clear_message_on_non_utf8() {
        // file_content_matches needs to read text to apply the
        // regex; non-UTF-8 input surfaces an explicit violation
        // rather than silently skipping (so a binary commit
        // doesn't accidentally hide a missing-pattern policy).
        let spec = spec_yaml(
            "id: t\n\
             kind: file_content_matches\n\
             paths: \"img.bin\"\n\
             pattern: \"never matches\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("img.bin", &[0xff, 0xfe, 0xfd])]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "non-UTF-8 should report one violation");
        assert!(
            v[0].message.contains("UTF-8"),
            "message should mention UTF-8: {}",
            v[0].message
        );
    }
}
