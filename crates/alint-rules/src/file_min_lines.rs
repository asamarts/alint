//! `file_min_lines` — files in scope must have at least
//! `min_lines` lines.
//!
//! Catches the "README is a title plus two sentences" case
//! where the file exists, isn't empty, but is far too thin to
//! actually document anything. Pairs well with `file_exists`
//! on README / CHANGELOG / SECURITY.md in governance rulesets.
//!
//! A **line** is any run of bytes terminated by `\n`. The
//! trailing segment after the last newline (or the whole file
//! when there is no newline) counts as one additional line
//! only when it is non-empty — so `"a\nb\n"` and `"a\nb"` both
//! report 2 lines, while `"a\nb\n\n"` reports 3 (the empty
//! line between the two newlines counts). This matches the
//! usual `wc -l` semantics closely enough for policy use;
//! pedantic counting differences aren't worth the surprise.

use std::path::Path;

use alint_core::{
    Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation, eval_per_file,
};
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Minimum allowed line count.
    min_lines: u64,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct FileMinLinesRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    min_lines: u64,
}

impl Rule for FileMinLinesRule {
    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        eval_per_file(self, ctx)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for FileMinLinesRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        let lines = count_lines(bytes);
        if lines >= self.min_lines {
            return Ok(Vec::new());
        }
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "file has {} line(s); at least {} required",
                lines, self.min_lines,
            )
        });
        Ok(vec![
            Violation::new(msg).with_path(std::sync::Arc::<Path>::from(path)),
        ])
    }
}

/// Count lines with `wc -l`-style semantics: every `\n` is a
/// line terminator, plus one more line when the file doesn't
/// end with `\n` but has content after the last `\n`. Empty
/// file → 0 lines.
fn count_lines(bytes: &[u8]) -> u64 {
    if bytes.is_empty() {
        return 0;
    }
    // `bytecount` would be faster, but line-count files are
    // typically READMEs / CHANGELOGs (small). Not worth a
    // dep for a hot loop that isn't.
    #[allow(clippy::naive_bytecount)]
    let newlines = bytes.iter().filter(|&&b| b == b'\n').count() as u64;
    let trailing_unterminated = u64::from(!bytes.ends_with(b"\n"));
    newlines + trailing_unterminated
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(_paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "file_min_lines requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    Ok(Box::new(FileMinLinesRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        min_lines: opts.min_lines,
    }))
}

#[cfg(test)]
mod tests {
    use super::count_lines;

    #[test]
    fn empty_file_has_zero_lines() {
        assert_eq!(count_lines(b""), 0);
    }

    #[test]
    fn content_with_trailing_newline_counts_each_line() {
        assert_eq!(count_lines(b"a\n"), 1);
        assert_eq!(count_lines(b"a\nb\n"), 2);
        assert_eq!(count_lines(b"a\nb\nc\n"), 3);
    }

    #[test]
    fn content_without_trailing_newline_adds_one_for_tail() {
        assert_eq!(count_lines(b"a"), 1);
        assert_eq!(count_lines(b"a\nb"), 2);
    }

    #[test]
    fn blank_lines_count() {
        assert_eq!(count_lines(b"a\n\nb\n"), 3);
        assert_eq!(count_lines(b"\n\n"), 2);
    }
}
