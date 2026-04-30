//! `line_endings` — every line in each file in scope must use
//! the configured line ending (`lf` or `crlf`). Mixed endings
//! in a single file fail.

use alint_core::{Context, Error, FixSpec, Fixer, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

use crate::fixers::{FileNormalizeLineEndingsFixer, LineEndingTarget};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    target: TargetName,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum TargetName {
    Lf,
    Crlf,
}

impl From<TargetName> for LineEndingTarget {
    fn from(t: TargetName) -> Self {
        match t {
            TargetName::Lf => LineEndingTarget::Lf,
            TargetName::Crlf => LineEndingTarget::Crlf,
        }
    }
}

#[derive(Debug)]
pub struct LineEndingsRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    target: LineEndingTarget,
    fixer: Option<FileNormalizeLineEndingsFixer>,
}

impl Rule for LineEndingsRule {
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
            if let Some(line_no) = first_mismatched_line(&bytes, self.target) {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "line {line_no} does not use {} line endings",
                        self.target.name()
                    )
                });
                violations.push(
                    Violation::new(msg)
                        .with_path(entry.path.clone())
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

/// Walk the byte stream and return the 1-based line number of
/// the first line ending that doesn't match `target`. The last
/// "line" (after the final newline, or the whole file if no
/// newline at all) is ignored — `final_newline` is that rule.
fn first_mismatched_line(bytes: &[u8], target: LineEndingTarget) -> Option<usize> {
    let mut line_no = 1usize;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            // LF terminator. For CRLF target we'd need CR just
            // before; if not, flag.
            let prev_is_cr = i > 0 && bytes[i - 1] == b'\r';
            match target {
                LineEndingTarget::Lf if prev_is_cr => return Some(line_no),
                LineEndingTarget::Crlf if !prev_is_cr => return Some(line_no),
                _ => {}
            }
            line_no += 1;
        }
        i += 1;
    }
    None
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let paths = spec
        .paths
        .as_ref()
        .ok_or_else(|| Error::rule_config(&spec.id, "line_endings requires a `paths` field"))?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let target: LineEndingTarget = opts.target.into();
    let scope = Scope::from_paths_spec(paths)?;
    let fixer = match &spec.fix {
        Some(FixSpec::FileNormalizeLineEndings { .. }) => {
            Some(FileNormalizeLineEndingsFixer::new(target))
        }
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "fix.{} is not compatible with line_endings",
                    other.op_name()
                ),
            ));
        }
        None => None,
    };
    Ok(Box::new(LineEndingsRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope,
        target,
        fixer,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lf_target_accepts_pure_lf() {
        assert_eq!(
            first_mismatched_line(b"a\nb\nc\n", LineEndingTarget::Lf),
            None
        );
    }

    #[test]
    fn lf_target_flags_first_crlf() {
        assert_eq!(
            first_mismatched_line(b"a\nb\r\nc\n", LineEndingTarget::Lf),
            Some(2)
        );
    }

    #[test]
    fn crlf_target_accepts_pure_crlf() {
        assert_eq!(
            first_mismatched_line(b"a\r\nb\r\nc\r\n", LineEndingTarget::Crlf),
            None
        );
    }

    #[test]
    fn crlf_target_flags_first_lf() {
        assert_eq!(
            first_mismatched_line(b"a\r\nb\nc\r\n", LineEndingTarget::Crlf),
            Some(2)
        );
    }

    #[test]
    fn empty_file_is_ok() {
        assert_eq!(first_mismatched_line(b"", LineEndingTarget::Lf), None);
    }

    #[test]
    fn lone_cr_without_lf_is_not_a_line_ending() {
        // Standalone CR (no LF) isn't treated as a line ending —
        // modern tools (git, most editors) don't treat Classic-Mac
        // line endings as their own thing.
        assert_eq!(first_mismatched_line(b"a\rb\n", LineEndingTarget::Lf), None);
    }
}
