//! `line_max_width` — cap on characters per line.
//!
//! Counts Unicode scalar values (chars) per line, not bytes or
//! display cells. CJK, combining marks, and emoji that occupy
//! two terminal columns count as one char — if you want real
//! display-width accounting, use a formatter (Biome, prettier);
//! that's out of alint's byte/structure scope.
//!
//! Check-only: truncation isn't a safe auto-fix. Users either
//! refactor the line or widen the limit.

use std::path::Path;

use alint_core::{
    Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation, eval_per_file,
};
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Maximum number of Unicode scalar values (chars) allowed per line.
    #[schemars(range(min = 1))]
    max_width: usize,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct LineMaxWidthRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    max_width: usize,
}

impl Rule for LineMaxWidthRule {
    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        eval_per_file(self, ctx)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for LineMaxWidthRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        // Skip binary content (like max_consecutive_blank_lines): an image or
        // archive has no meaningful "lines", and measuring one lossily flagged
        // every binary in scope.
        if crate::io::looks_binary(bytes) {
            return Ok(Vec::new());
        }
        // Line widths count Unicode scalars, not bytes. Decode lossily so a
        // non-UTF-8 file is still measured rather than silently skipped: each
        // invalid byte decodes to one U+FFFD and so counts as one column.
        let text = String::from_utf8_lossy(bytes);
        let Some((line_no, width)) = first_overlong_line(&text, self.max_width) else {
            return Ok(Vec::new());
        };
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "line {line_no} is {width} chars wide; max is {}",
                self.max_width
            )
        });
        Ok(vec![
            Violation::new(msg)
                .with_path(std::sync::Arc::<Path>::from(path))
                .with_location(line_no, self.max_width + 1)
                // First-offender rule: the baseline identity is the *file*, not
                // the offending line's content (which would churn on any edit
                // to that line). Mirrors `no_trailing_whitespace`/`line_endings`
                // (M14). See `docs/design/baseline.md` §4.
                .with_baseline_key(crate::slash(path)),
        ])
    }
}

fn first_overlong_line(text: &str, max_width: usize) -> Option<(usize, usize)> {
    for (idx, line) in text.split('\n').enumerate() {
        let trimmed = line.strip_suffix('\r').unwrap_or(line);
        let width = trimmed.chars().count();
        if width > max_width {
            return Some((idx + 1, width));
        }
    }
    None
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let _paths = spec
        .paths
        .as_ref()
        .ok_or_else(|| Error::rule_config(&spec.id, "line_max_width requires a `paths` field"))?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.max_width == 0 {
        return Err(Error::rule_config(
            &spec.id,
            "line_max_width `max_width` must be > 0",
        ));
    }
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "line_max_width has no fix op - truncation is unsafe",
        ));
    }
    Ok(Box::new(LineMaxWidthRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        max_width: opts.max_width,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_file_is_ok() {
        assert_eq!(first_overlong_line("hi\nthere\n", 10), None);
    }

    #[test]
    fn flags_first_overlong_line() {
        let txt = "short\nway too looooong for ten\nok\n";
        // "way too looooong for ten" is 24 chars.
        assert_eq!(first_overlong_line(txt, 10), Some((2, 24)));
    }

    #[test]
    fn width_exactly_at_limit_is_ok() {
        assert_eq!(first_overlong_line("0123456789\n", 10), None);
    }

    #[test]
    fn crlf_is_stripped_before_counting() {
        // "hi\r\n" should count as 2 chars ("hi"), not 3.
        assert_eq!(first_overlong_line("hi\r\n", 2), None);
    }

    #[test]
    fn counts_unicode_scalar_values_not_bytes() {
        // "☃☃☃" is 3 scalars / 9 bytes. Under `max_width: 3` it's fine.
        assert_eq!(first_overlong_line("☃☃☃\n", 3), None);
        // Under max_width: 2 it's flagged.
        assert_eq!(first_overlong_line("☃☃☃\n", 2), Some((1, 3)));
    }
}

#[cfg(test)]
mod non_utf8_tests {
    use crate::test_support::{ctx, spec_yaml, tempdir_with_files};

    #[test]
    fn non_utf8_text_is_still_measured() {
        // Fail-closed regression: one Latin-1 byte used to skip the whole file.
        // An invalid byte counts as one column (it decodes to one U+FFFD).
        let rule = super::build(&spec_yaml(
            "id: t\nkind: line_max_width\npaths: \"**/*\"\nmax_width: 10\nlevel: error\n",
        ))
        .unwrap();
        let body: &[u8] = b"caf\xe9\n0123456789abc\n";
        let (tmp, idx) = tempdir_with_files(&[("a.txt", body)]);
        let vs = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].line, Some(2));
        let ok: &[u8] = b"0123456789\xe9\n";
        let (tmp, idx) = tempdir_with_files(&[("b.txt", ok)]);
        assert_eq!(rule.evaluate(&ctx(tmp.path(), &idx)).unwrap().len(), 1);
    }

    #[test]
    fn binary_files_are_skipped() {
        // Regression: the lossy decode measured binaries too, so every image /
        // archive in scope was flagged as one overlong "line".
        let rule = super::build(&spec_yaml(
            "id: t\nkind: line_max_width\npaths: \"**/*\"\nmax_width: 10\nlevel: error\n",
        ))
        .unwrap();
        let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR".to_vec();
        png.extend(std::iter::repeat_n(0xA5u8, 200));
        let (tmp, idx) = tempdir_with_files(&[("img.png", png.as_slice())]);
        assert!(rule.evaluate(&ctx(tmp.path(), &idx)).unwrap().is_empty());
    }
}
