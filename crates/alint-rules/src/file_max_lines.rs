//! `file_max_lines` — files in scope must have AT MOST
//! `max_lines` lines. Mirror of [`crate::file_min_lines`];
//! shares the line-counting semantics so the two compose
//! cleanly when both are applied to the same file.
//!
//! Catches the "everything-module" anti-pattern — a single
//! `lib.rs` / `index.ts` / `helpers.py` that grew until it
//! does the work of a half-dozen smaller files. The threshold
//! is intentionally a soft signal rather than a hard limit;
//! we ship it at `level: warning` in tutorials and rulesets,
//! and leave the cap value to the team.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Options {
    max_lines: u64,
}

#[derive(Debug)]
pub struct FileMaxLinesRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    max_lines: u64,
}

impl Rule for FileMaxLinesRule {
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
                // Same silent-skip policy as the rest of the
                // content family — a permission flake or race
                // shouldn't blow up the whole check run.
                continue;
            };
            let lines = count_lines(&bytes);
            if lines > self.max_lines {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "file has {} line(s); at most {} allowed",
                        lines, self.max_lines,
                    )
                });
                violations.push(Violation::new(msg).with_path(&entry.path));
            }
        }
        Ok(violations)
    }
}

/// Count lines with the same `wc -l`-style semantics as
/// `file_min_lines::count_lines`. Kept as a private function
/// here (rather than reused from `file_min_lines`) because
/// inlining it makes the unit tests explicit about what this
/// rule's threshold is being compared against — and the
/// implementation is one line, not worth a cross-module dep.
fn count_lines(bytes: &[u8]) -> u64 {
    if bytes.is_empty() {
        return 0;
    }
    #[allow(clippy::naive_bytecount)]
    let newlines = bytes.iter().filter(|&&b| b == b'\n').count() as u64;
    let trailing_unterminated = u64::from(!bytes.ends_with(b"\n"));
    newlines + trailing_unterminated
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "file_max_lines requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    Ok(Box::new(FileMaxLinesRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        max_lines: opts.max_lines,
    }))
}

#[cfg(test)]
mod tests {
    use super::count_lines;

    #[test]
    fn empty_file_is_zero_lines() {
        assert_eq!(count_lines(b""), 0);
    }

    #[test]
    fn matches_min_lines_semantics() {
        // Identical accounting to file_min_lines so the two
        // rules agree on what "this file has N lines" means.
        assert_eq!(count_lines(b"a\n"), 1);
        assert_eq!(count_lines(b"a\nb\n"), 2);
        assert_eq!(count_lines(b"a\nb"), 2);
        assert_eq!(count_lines(b"\n\n"), 2);
        assert_eq!(count_lines(b"a\n\nb\n"), 3);
    }
}
