//! `filename_regex` — every file in scope must have a basename matching a
//! regex. Anchored with `^...$` automatically; use the full basename
//! (including extension) in your pattern.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Options {
    pattern: String,
    /// Check against the file *stem* (no final extension) instead of the
    /// full basename. Defaults to `false` (full basename is matched).
    #[serde(default)]
    stem: bool,
}

#[derive(Debug)]
pub struct FilenameRegexRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    pattern_src: String,
    pattern: Regex,
    stem: bool,
}

impl Rule for FilenameRegexRule {
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
            let name = if self.stem {
                entry.path.file_stem().and_then(|s| s.to_str())
            } else {
                entry.path.file_name().and_then(|s| s.to_str())
            };
            let Some(name) = name else { continue };
            if !self.pattern.is_match(name) {
                let target = if self.stem { "stem" } else { "basename" };
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "filename {target} {:?} does not match /^{}$/",
                        name, self.pattern_src
                    )
                });
                violations.push(Violation::new(msg).with_path(entry.path.clone()));
            }
        }
        Ok(violations)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(_paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "filename_regex requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let anchored = format!("^{}$", opts.pattern);
    let pattern = Regex::new(&anchored)
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid pattern: {e}")))?;
    Ok(Box::new(FilenameRegexRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        pattern_src: opts.pattern,
        pattern,
        stem: opts.stem,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, index, spec_yaml};
    use std::path::Path;

    #[test]
    fn build_rejects_missing_paths_field() {
        let spec = spec_yaml(
            "id: t\n\
             kind: filename_regex\n\
             pattern: \"^test_.*$\"\n\
             level: error\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(err.contains("paths"), "unexpected: {err}");
    }

    #[test]
    fn build_rejects_invalid_regex() {
        let spec = spec_yaml(
            "id: t\n\
             kind: filename_regex\n\
             paths: \"tests/**/*.rs\"\n\
             pattern: \"[unterminated\"\n\
             level: error\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn evaluate_passes_on_matching_basename() {
        // The pattern is anchored automatically — note no `^…$`.
        let spec = spec_yaml(
            "id: t\n\
             kind: filename_regex\n\
             paths: \"tests/**/*.rs\"\n\
             pattern: \"test_[a-z0-9_]+\\\\.rs\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["tests/test_basic.rs", "tests/test_widget.rs"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert!(v.is_empty(), "unexpected: {v:?}");
    }

    #[test]
    fn evaluate_fires_on_non_matching_basename() {
        let spec = spec_yaml(
            "id: t\n\
             kind: filename_regex\n\
             paths: \"tests/**/*.rs\"\n\
             pattern: \"test_[a-z0-9_]+\\\\.rs\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["tests/no_test_prefix.rs", "tests/test_ok.rs"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 1, "only the no-prefix file should fire");
    }

    #[test]
    fn stem_mode_matches_against_extensionless_name() {
        // With `stem: true`, the regex sees `test_widget`,
        // not `test_widget.rs`. The pattern reflects that.
        let spec = spec_yaml(
            "id: t\n\
             kind: filename_regex\n\
             paths: \"tests/**/*.rs\"\n\
             pattern: \"test_[a-z0-9_]+\"\n\
             stem: true\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["tests/test_widget.rs"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert!(v.is_empty(), "stem-mode should match: {v:?}");
    }

    #[test]
    fn pattern_is_anchored() {
        // A pattern of `widget` should NOT match `xwidgety.rs` —
        // build wraps it in `^…$` automatically.
        let spec = spec_yaml(
            "id: t\n\
             kind: filename_regex\n\
             paths: \"src/**/*.rs\"\n\
             pattern: \"widget\\\\.rs\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["src/xwidgety.rs"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 1, "non-anchored partial match shouldn't pass");
    }

    #[test]
    fn scope_filter_narrows() {
        // Two non-matching basenames; only the one inside a
        // directory with `marker.lock` as ancestor should fire.
        let spec = spec_yaml(
            "id: t\n\
             kind: filename_regex\n\
             paths: \"**/*.rs\"\n\
             pattern: \"good_[a-z]+\\\\.rs\"\n\
             scope_filter:\n  \
               has_ancestor: marker.lock\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["pkg/marker.lock", "pkg/bad.rs", "other/bad.rs"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 1, "only in-scope file should fire: {v:?}");
        assert_eq!(v[0].path.as_deref(), Some(Path::new("pkg/bad.rs")));
    }
}
