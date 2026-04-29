//! `file_header` — first N lines of each file in scope must match a pattern.

use alint_core::{Context, Error, FixSpec, Fixer, Level, Result, Rule, RuleSpec, Scope, Violation};
use regex::Regex;
use serde::Deserialize;

use crate::fixers::FilePrependFixer;

#[derive(Debug, Deserialize)]
struct Options {
    pattern: String,
    #[serde(default = "default_lines")]
    lines: usize,
}

fn default_lines() -> usize {
    20
}

#[derive(Debug)]
pub struct FileHeaderRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    pattern_src: String,
    pattern: Regex,
    lines: usize,
    fixer: Option<FilePrependFixer>,
}

impl Rule for FileHeaderRule {
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
                        Violation::new(format!("could not read file: {e}")).with_path(&entry.path),
                    );
                    continue;
                }
            };
            let Ok(text) = std::str::from_utf8(&bytes) else {
                violations.push(
                    Violation::new("file is not valid UTF-8; cannot match header")
                        .with_path(&entry.path),
                );
                continue;
            };
            let header: String = text.split_inclusive('\n').take(self.lines).collect();
            if !self.pattern.is_match(&header) {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "first {} line(s) do not match required header /{}/",
                        self.lines, self.pattern_src
                    )
                });
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
    let Some(paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "file_header requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.lines == 0 {
        return Err(Error::rule_config(
            &spec.id,
            "file_header `lines` must be > 0",
        ));
    }
    let pattern = Regex::new(&opts.pattern)
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid pattern: {e}")))?;
    let fixer = match &spec.fix {
        Some(FixSpec::FilePrepend { file_prepend }) => {
            let source = alint_core::resolve_content_source(
                &spec.id,
                "file_prepend",
                &file_prepend.content,
                &file_prepend.content_from,
            )?;
            Some(FilePrependFixer::new(source))
        }
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!("fix.{} is not compatible with file_header", other.op_name()),
            ));
        }
        None => None,
    };
    Ok(Box::new(FileHeaderRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        pattern_src: opts.pattern,
        pattern,
        lines: opts.lines,
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
             kind: file_header\n\
             pattern: \"^// SPDX\"\n\
             level: error\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn build_rejects_zero_lines() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_header\n\
             paths: \"src/**/*.rs\"\n\
             pattern: \"^// SPDX\"\n\
             lines: 0\n\
             level: error\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(err.contains("lines"), "unexpected: {err}");
    }

    #[test]
    fn build_rejects_invalid_regex() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_header\n\
             paths: \"src/**/*.rs\"\n\
             pattern: \"[unterminated\"\n\
             level: error\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn evaluate_passes_when_header_matches() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_header\n\
             paths: \"src/**/*.rs\"\n\
             pattern: \"SPDX-License-Identifier: Apache-2.0\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[(
            "src/main.rs",
            b"// SPDX-License-Identifier: Apache-2.0\n\nfn main() {}\n",
        )]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "header should match: {v:?}");
    }

    #[test]
    fn evaluate_fires_when_header_missing() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_header\n\
             paths: \"src/**/*.rs\"\n\
             pattern: \"SPDX-License-Identifier:\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("src/main.rs", b"fn main() {}\n")]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn evaluate_only_inspects_first_n_lines() {
        // Pattern only on line 30, but `lines: 5` only looks at
        // lines 1-5 → rule fires.
        let spec = spec_yaml(
            "id: t\n\
             kind: file_header\n\
             paths: \"src/**/*.rs\"\n\
             pattern: \"NEEDLE\"\n\
             lines: 5\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let mut content = String::new();
        for _ in 0..30 {
            content.push_str("filler\n");
        }
        content.push_str("NEEDLE\n");
        let (tmp, idx) = tempdir_with_files(&[("src/main.rs", content.as_bytes())]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1);
    }
}
