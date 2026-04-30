//! `no_trailing_whitespace` — every line in each file in scope
//! must not end with a space or tab.
//!
//! The rule walks line-by-line and reports one violation per
//! file (with the 1-based line number of the first offender in
//! `violation.line`). Read failures are skipped silently.
//!
//! Trailing whitespace is a byte-pattern check (`b' '` /
//! `b'\t'`), so the per-file dispatch path scans the engine-
//! supplied `&[u8]` directly without a UTF-8 validation pass.
//! The rule-major fallback (`Rule::evaluate`, used by
//! `alint fix` and tests that bypass the engine) reads each
//! file itself and delegates to `evaluate_file`.

use std::path::Path;

use alint_core::{
    Context, Error, FixSpec, Fixer, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation,
};

use crate::fixers::FileTrimTrailingWhitespaceFixer;

#[derive(Debug)]
pub struct NoTrailingWhitespaceRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    fixer: Option<FileTrimTrailingWhitespaceFixer>,
}

impl Rule for NoTrailingWhitespaceRule {
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
            violations.extend(self.evaluate_file(ctx, &entry.path, &bytes)?);
        }
        Ok(violations)
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for NoTrailingWhitespaceRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        let Some(line_no) = first_offending_line(bytes) else {
            return Ok(Vec::new());
        };
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| format!("trailing whitespace on line {line_no}"));
        Ok(vec![
            Violation::new(msg)
                .with_path(std::sync::Arc::<Path>::from(path))
                .with_location(line_no, 1),
        ])
    }
}

/// Returns the 1-based line number of the first line ending in
/// a space or tab, or `None` if the file is clean. Operates on
/// bytes directly; trailing whitespace is a byte-pattern check
/// that doesn't need UTF-8 validation, so we skip the
/// `from_utf8` walk.
fn first_offending_line(bytes: &[u8]) -> Option<usize> {
    for (idx, line) in bytes.split(|&b| b == b'\n').enumerate() {
        let trimmed = line.strip_suffix(b"\r").unwrap_or(line);
        if matches!(trimmed.last(), Some(b' ' | b'\t')) {
            return Some(idx + 1);
        }
    }
    None
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(&spec.id, "no_trailing_whitespace requires a `paths` field")
    })?;
    let scope = Scope::from_paths_spec(paths)?;
    let fixer = match &spec.fix {
        Some(FixSpec::FileTrimTrailingWhitespace { .. }) => Some(FileTrimTrailingWhitespaceFixer),
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "fix.{} is not compatible with no_trailing_whitespace",
                    other.op_name()
                ),
            ));
        }
        None => None,
    };
    Ok(Box::new(NoTrailingWhitespaceRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope,
        fixer,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_trailing_space() {
        assert_eq!(first_offending_line(b"clean\nbad  \nclean\n"), Some(2));
    }

    #[test]
    fn detects_trailing_tab() {
        assert_eq!(first_offending_line(b"clean\nbad\t\nclean\n"), Some(2));
    }

    #[test]
    fn crlf_with_trailing_whitespace_flagged() {
        assert_eq!(first_offending_line(b"bad \r\n"), Some(1));
    }

    #[test]
    fn clean_file_has_no_match() {
        assert_eq!(first_offending_line(b"one\ntwo\nthree\n"), None);
    }

    #[test]
    fn single_line_no_trailing_newline_clean() {
        assert_eq!(first_offending_line(b"hello"), None);
    }
}
