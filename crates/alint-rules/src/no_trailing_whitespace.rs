//! `no_trailing_whitespace` — every line in each file in scope
//! must not end with a space or tab.
//!
//! The rule reads each file's UTF-8 content, walks line-by-line,
//! and reports one violation per file (with the 1-based line
//! number of the first offender in `violation.line`). Non-UTF-8
//! files are skipped silently.

use alint_core::{Context, Error, FixSpec, Fixer, Level, Result, Rule, RuleSpec, Scope, Violation};

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
            let Ok(text) = std::str::from_utf8(&bytes) else {
                continue;
            };
            if let Some((line_no, _)) = first_offending_line(text) {
                let msg = self
                    .message
                    .clone()
                    .unwrap_or_else(|| format!("trailing whitespace on line {line_no}"));
                violations.push(
                    Violation::new(msg)
                        .with_path(&entry.path)
                        .with_location(line_no, 1),
                );
            }
        }
        Ok(violations)
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }
}

/// Returns (1-based line number, line-without-terminator) for
/// the first line ending in a space or tab. `None` if clean.
fn first_offending_line(text: &str) -> Option<(usize, &str)> {
    for (idx, line) in text.split('\n').enumerate() {
        // `split` yields a trailing empty element if text ends
        // with `\n`; that's not a real line, so skip empties at
        // the final position only when the overall file ended in
        // `\n`. Checking `.ends_with(' ' | '\t')` on "" is false
        // anyway, so no special-case is needed.
        let trimmed = line.strip_suffix('\r').unwrap_or(line);
        if trimmed.ends_with(' ') || trimmed.ends_with('\t') {
            return Some((idx + 1, trimmed));
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
        assert_eq!(
            first_offending_line("clean\nbad  \nclean\n"),
            Some((2, "bad  "))
        );
    }

    #[test]
    fn detects_trailing_tab() {
        assert_eq!(
            first_offending_line("clean\nbad\t\nclean\n"),
            Some((2, "bad\t"))
        );
    }

    #[test]
    fn crlf_with_trailing_whitespace_flagged() {
        assert_eq!(first_offending_line("bad \r\n"), Some((1, "bad ")));
    }

    #[test]
    fn clean_file_has_no_match() {
        assert_eq!(first_offending_line("one\ntwo\nthree\n"), None);
    }

    #[test]
    fn single_line_no_trailing_newline_clean() {
        assert_eq!(first_offending_line("hello"), None);
    }
}
