//! `file_footer` — last N lines of each file in scope must match a pattern.
//!
//! Mirror of [`crate::file_header`], anchored at the END of the
//! file. Use cases:
//!
//! - License footers ("Licensed under the Apache License…")
//! - Generated-file trailers ("DO NOT EDIT — regenerate via …")
//! - Signed-off-by trailers
//! - Final blank-line + sentinel patterns
//!
//! Same `pattern:` + `lines:` shape as `file_header` so configs
//! that mix the two read symmetrically. The fix op is
//! `file_append`: when the rule fires and a fixer is attached,
//! the configured content is appended to the file (inverse of
//! `file_header` + `file_prepend`).

use std::path::Path;

use alint_core::{
    Context, Error, FixSpec, Fixer, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation,
};
use regex::Regex;
use serde::Deserialize;

use crate::fixers::FileAppendFixer;

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
pub struct FileFooterRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    pattern_src: String,
    pattern: Regex,
    lines: usize,
    fixer: Option<FileAppendFixer>,
}

impl Rule for FileFooterRule {
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
                        Violation::new(format!("could not read file: {e}"))
                            .with_path(entry.path.clone()),
                    );
                    continue;
                }
            };
            violations.extend(self.evaluate_file(ctx, &entry.path, &bytes)?);
        }
        Ok(violations)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for FileFooterRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Ok(vec![
                Violation::new("file is not valid UTF-8; cannot match footer")
                    .with_path(std::sync::Arc::<Path>::from(path)),
            ]);
        };
        let footer = last_lines(text, self.lines);
        if self.pattern.is_match(&footer) {
            return Ok(Vec::new());
        }
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "last {} line(s) do not match required footer /{}/",
                self.lines, self.pattern_src
            )
        });
        Ok(vec![
            Violation::new(msg).with_path(std::sync::Arc::<Path>::from(path)),
        ])
    }
}

/// Return the last `n` lines of `text` as a single string,
/// preserving the line terminators that were already there.
/// Empty files return the empty string. Files shorter than `n`
/// lines return the entire file.
///
/// We split on `\n`, take the trailing `n` slices, then re-join
/// with `\n` so the result reads identically to what
/// `file_header`'s `take(N)` would produce on a flipped
/// document.
fn last_lines(text: &str, n: usize) -> String {
    if n == 0 || text.is_empty() {
        return String::new();
    }
    // `split_inclusive` keeps the `\n` on every line except
    // possibly the last, mirroring `file_header`'s parser.
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let take = lines.len().min(n);
    let start = lines.len() - take;
    lines[start..].concat()
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "file_footer requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.lines == 0 {
        return Err(Error::rule_config(
            &spec.id,
            "file_footer `lines` must be > 0",
        ));
    }
    let pattern = Regex::new(&opts.pattern)
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid pattern: {e}")))?;
    let fixer = match &spec.fix {
        Some(FixSpec::FileAppend { file_append }) => {
            let source = alint_core::resolve_content_source(
                &spec.id,
                "file_append",
                &file_append.content,
                &file_append.content_from,
            )?;
            Some(FileAppendFixer::new(source))
        }
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!("fix.{} is not compatible with file_footer", other.op_name()),
            ));
        }
        None => None,
    };
    Ok(Box::new(FileFooterRule {
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
    use super::last_lines;

    #[test]
    fn empty_file_returns_empty() {
        assert_eq!(last_lines("", 5), "");
    }

    #[test]
    fn short_file_returns_whole_thing() {
        // 2 lines, asked for 5 → return both.
        assert_eq!(last_lines("a\nb\n", 5), "a\nb\n");
    }

    #[test]
    fn returns_trailing_n_lines() {
        let body = "1\n2\n3\n4\n5\n";
        assert_eq!(last_lines(body, 2), "4\n5\n");
        assert_eq!(last_lines(body, 3), "3\n4\n5\n");
    }

    #[test]
    fn unterminated_last_line_carries_through() {
        // No trailing newline; "c" is the last line.
        assert_eq!(last_lines("a\nb\nc", 1), "c");
        assert_eq!(last_lines("a\nb\nc", 2), "b\nc");
    }
}
