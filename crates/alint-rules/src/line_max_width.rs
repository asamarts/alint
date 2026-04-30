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

use alint_core::{Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    max_width: usize,
}

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
        // `chars().count()` requires a UTF-8-validated `&str` —
        // line widths count Unicode scalars, not bytes. Non-UTF-8
        // files silently skip, matching the rule-major path.
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Ok(Vec::new());
        };
        let Some((line_no, width)) = first_overlong_line(text, self.max_width) else {
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
                .with_location(line_no, self.max_width + 1),
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
    let paths = spec
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
            "line_max_width has no fix op — truncation is unsafe",
        ));
    }
    Ok(Box::new(LineMaxWidthRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
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
