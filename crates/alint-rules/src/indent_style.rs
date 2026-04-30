//! `indent_style` — every non-blank line in each file in scope must
//! indent with the configured style: `tabs` or `spaces`.
//!
//! The check is byte-level and only inspects the *leading* run of
//! whitespace on each line. Mid-line tabs or spaces are not the
//! rule's business (many formatters use a mix for alignment after
//! the indent column).
//!
//! Optional `width`: when `style: spaces`, the number of leading
//! spaces must be an exact multiple of `width`. Ignored for
//! `style: tabs` since a tab is a single character regardless of
//! visual width.
//!
//! Check-only. Auto-converting tabs ↔ spaces requires knowing the
//! visual tab width, and the correct conversion for continuation
//! indentation is language-specific — so this rule flags but does
//! not repair. Users typically pair it with their editor's own
//! "reindent on save" feature.

use std::path::Path;

use alint_core::{Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    style: StyleName,
    #[serde(default)]
    width: Option<u32>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum StyleName {
    Tabs,
    Spaces,
}

#[derive(Debug)]
pub struct IndentStyleRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    style: StyleName,
    width: Option<u32>,
}

impl Rule for IndentStyleRule {
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

impl PerFileRule for IndentStyleRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        // The leading-indent scan inspects ASCII whitespace
        // characters and uses `char_indices` to slice the prefix
        // — we keep the UTF-8 validation pass for parity with
        // the rule-major path. Non-UTF-8 files silently skip.
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Ok(Vec::new());
        };
        let Some((line_no, reason)) = first_bad_line(text, self.style, self.width) else {
            return Ok(Vec::new());
        };
        let msg = self.message.clone().unwrap_or_else(|| match reason {
            BadReason::WrongChar => format!(
                "line {line_no} indented with the wrong character (expected {})",
                self.style_name()
            ),
            BadReason::WidthMismatch => format!(
                "line {line_no} has leading spaces that are not a multiple of {}",
                self.width.unwrap_or(0),
            ),
        });
        Ok(vec![
            Violation::new(msg)
                .with_path(std::sync::Arc::<Path>::from(path))
                .with_location(line_no, 1),
        ])
    }
}

impl IndentStyleRule {
    fn style_name(&self) -> &'static str {
        match self.style {
            StyleName::Tabs => "tabs",
            StyleName::Spaces => "spaces",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BadReason {
    WrongChar,
    WidthMismatch,
}

/// Return the 1-based line number of the first line whose leading
/// whitespace violates the configured style. Blank lines (empty or
/// whitespace-only) are skipped so trailing indentation on an
/// otherwise-blank line doesn't cause spurious failures.
fn first_bad_line(text: &str, style: StyleName, width: Option<u32>) -> Option<(usize, BadReason)> {
    for (idx, line) in text.split('\n').enumerate() {
        let body = line.strip_suffix('\r').unwrap_or(line);
        let lead: &str = body
            .char_indices()
            .find(|(_, c)| *c != ' ' && *c != '\t')
            .map_or(body, |(i, _)| &body[..i]);
        // Blank / whitespace-only line: no indent to judge.
        if lead.len() == body.len() {
            continue;
        }
        let line_no = idx + 1;
        match style {
            StyleName::Tabs => {
                if lead.bytes().any(|b| b == b' ') {
                    return Some((line_no, BadReason::WrongChar));
                }
            }
            StyleName::Spaces => {
                if lead.bytes().any(|b| b == b'\t') {
                    return Some((line_no, BadReason::WrongChar));
                }
                if let Some(w) = width
                    && w > 0
                    && lead.len() % (w as usize) != 0
                {
                    return Some((line_no, BadReason::WidthMismatch));
                }
            }
        }
    }
    None
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let paths = spec
        .paths
        .as_ref()
        .ok_or_else(|| Error::rule_config(&spec.id, "indent_style requires a `paths` field"))?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "indent_style has no fix op — tab-width-aware reindentation is deferred",
        ));
    }
    Ok(Box::new(IndentStyleRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        style: opts.style,
        width: opts.width,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_style_accepts_pure_tab_indent() {
        assert_eq!(
            first_bad_line("fn x() {\n\tlet a = 1;\n}\n", StyleName::Tabs, None),
            None
        );
    }

    #[test]
    fn tabs_style_flags_space_indent() {
        let (line, reason) =
            first_bad_line("fn x() {\n    let a = 1;\n}\n", StyleName::Tabs, None).unwrap();
        assert_eq!(line, 2);
        assert_eq!(reason, BadReason::WrongChar);
    }

    #[test]
    fn spaces_style_accepts_pure_space_indent() {
        assert_eq!(
            first_bad_line("x:\n  a: 1\n  b: 2\n", StyleName::Spaces, Some(2)),
            None
        );
    }

    #[test]
    fn spaces_style_flags_tab_indent() {
        let (line, reason) = first_bad_line("x:\n\ta: 1\n", StyleName::Spaces, Some(2)).unwrap();
        assert_eq!(line, 2);
        assert_eq!(reason, BadReason::WrongChar);
    }

    #[test]
    fn spaces_style_flags_width_mismatch() {
        let (line, reason) = first_bad_line("x:\n   a: 1\n", StyleName::Spaces, Some(2)).unwrap();
        assert_eq!(line, 2);
        assert_eq!(reason, BadReason::WidthMismatch);
    }

    #[test]
    fn blank_lines_are_not_judged() {
        assert_eq!(first_bad_line("\n   \na\n", StyleName::Tabs, None), None);
    }

    #[test]
    fn crlf_is_handled() {
        assert_eq!(
            first_bad_line("a\r\n  b\r\n", StyleName::Spaces, Some(2)),
            None
        );
    }
}
