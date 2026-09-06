//! `max_consecutive_blank_lines` — cap the number of blank lines
//! that may appear in a row. A "blank" line is one whose content
//! is empty or only spaces/tabs.
//!
//! Useful for keeping markdown, source files, and configs tidy —
//! editors commonly auto-insert extra blank lines on paste, and
//! they accumulate without this check.
//!
//! Fixable via `file_collapse_blank_lines: {}`, which rewrites
//! each over-long run down to exactly `max` blank lines. The
//! fixer preserves the file's line endings (LF vs CRLF).

use std::path::Path;

use alint_core::{
    Context, Error, FixSpec, Fixer, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation,
    eval_per_file,
};
use serde::Deserialize;

use crate::fixers::{FileCollapseBlankLinesFixer, line_is_blank};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Maximum number of blank lines allowed in a row. `0` means
    /// no blank lines at all.
    max: u32,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct MaxConsecutiveBlankLinesRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    max: u32,
    fixer: Option<FileCollapseBlankLinesFixer>,
}

impl Rule for MaxConsecutiveBlankLinesRule {
    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        eval_per_file(self, ctx)
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for MaxConsecutiveBlankLinesRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        // The blank-line check uses `line_is_blank` (str-based,
        // matches the fixer's logic — they share the helper) so
        // we keep the UTF-8 validation pass here. Non-UTF-8
        // files are silently skipped, matching the rule-major
        // path's behaviour.
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Ok(Vec::new());
        };
        let Some(line_no) = first_over_limit(text, self.max) else {
            return Ok(Vec::new());
        };
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| format!("more than {} consecutive blank line(s)", self.max));
        Ok(vec![
            Violation::new(msg)
                .with_path(std::sync::Arc::<Path>::from(path))
                .with_location(line_no, 1),
        ])
    }
}

/// Return the 1-based line number of the first blank line that
/// pushes the consecutive-blank counter above `max`. Only counts
/// blank lines that lie between file content - the trailing slot
/// after the final newline is ignored so a regular `foo\n` file
/// doesn't trip max=0.
fn first_over_limit(text: &str, max: u32) -> Option<usize> {
    let mut blank_run: u32 = 0;
    let mut remaining = text;
    let mut line_no: usize = 0;
    loop {
        let (body, has_ending, rest) = match remaining.find('\n') {
            Some(i) => {
                let before = &remaining[..i];
                let body = before.strip_suffix('\r').unwrap_or(before);
                (body, true, &remaining[i + 1..])
            }
            None => (remaining, false, ""),
        };
        if !has_ending && body.is_empty() {
            // The tail after the last newline; not a real line.
            return None;
        }
        line_no += 1;
        if line_is_blank(body) {
            blank_run += 1;
            if blank_run > max {
                return Some(line_no);
            }
        } else {
            blank_run = 0;
        }
        if !has_ending {
            return None;
        }
        remaining = rest;
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let _paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(
            &spec.id,
            "max_consecutive_blank_lines requires a `paths` field",
        )
    })?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let fixer = match &spec.fix {
        Some(FixSpec::FileCollapseBlankLines { .. }) => {
            Some(FileCollapseBlankLinesFixer::new(opts.max))
        }
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "fix.{} is not compatible with max_consecutive_blank_lines",
                    other.op_name()
                ),
            ));
        }
        None => None,
    };
    Ok(Box::new(MaxConsecutiveBlankLinesRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        max: opts.max,
        fixer,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn within_limit_is_ok() {
        assert_eq!(first_over_limit("a\n\nb\n", 1), None);
    }

    #[test]
    fn one_over_limit_is_flagged() {
        assert_eq!(first_over_limit("a\n\n\nb\n", 1), Some(3));
    }

    #[test]
    fn max_zero_flags_any_blank() {
        assert_eq!(first_over_limit("a\n\nb\n", 0), Some(2));
    }

    #[test]
    fn trailing_newline_is_not_a_blank_line() {
        assert_eq!(first_over_limit("a\n", 0), None);
    }

    #[test]
    fn whitespace_only_line_counts_as_blank() {
        assert_eq!(first_over_limit("a\n  \n\t\nb\n", 1), Some(3));
    }

    #[test]
    fn crlf_endings_are_counted() {
        assert_eq!(first_over_limit("a\r\n\r\n\r\n\r\nb\r\n", 1), Some(3));
    }
}
