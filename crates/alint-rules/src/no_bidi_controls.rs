//! `no_bidi_controls` — flag Unicode bidirectional control
//! characters in source.
//!
//! Trojan Source (CVE-2021-42574) exploits these chars to render
//! code differently from what compilers / interpreters see. The
//! offending codepoints:
//!   - U+202A LEFT-TO-RIGHT EMBEDDING
//!   - U+202B RIGHT-TO-LEFT EMBEDDING
//!   - U+202C POP DIRECTIONAL FORMATTING
//!   - U+202D LEFT-TO-RIGHT OVERRIDE
//!   - U+202E RIGHT-TO-LEFT OVERRIDE
//!   - U+2066 LEFT-TO-RIGHT ISOLATE
//!   - U+2067 RIGHT-TO-LEFT ISOLATE
//!   - U+2068 FIRST STRONG ISOLATE
//!   - U+2069 POP DIRECTIONAL ISOLATE
//!
//! Non-UTF-8 files are skipped (can't have these codepoints
//! anyway without being invalid UTF-8).

use alint_core::{Context, Error, FixSpec, Fixer, Level, Result, Rule, RuleSpec, Scope, Violation};

use crate::fixers::FileStripBidiFixer;

/// Returns true if `c` is one of the nine Unicode bidi control
/// characters.
pub fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

#[derive(Debug)]
pub struct NoBidiControlsRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    fixer: Option<FileStripBidiFixer>,
}

impl Rule for NoBidiControlsRule {
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
            if let Some((line_no, col, codepoint)) = first_bidi(text) {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "Unicode bidi control U+{codepoint:04X} at line {line_no} col {col} \
                         (Trojan-Source defense)"
                    )
                });
                violations.push(
                    Violation::new(msg)
                        .with_path(entry.path.clone())
                        .with_location(line_no, col),
                );
            }
        }
        Ok(violations)
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }
}

/// Scan for the first bidi control character and return
/// (1-based line, 1-based column, codepoint as u32).
fn first_bidi(text: &str) -> Option<(usize, usize, u32)> {
    let mut line = 1usize;
    let mut col = 1usize;
    for c in text.chars() {
        if is_bidi_control(c) {
            return Some((line, col, c as u32));
        }
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    None
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let paths = spec
        .paths
        .as_ref()
        .ok_or_else(|| Error::rule_config(&spec.id, "no_bidi_controls requires a `paths` field"))?;
    let fixer = match &spec.fix {
        Some(FixSpec::FileStripBidi { .. }) => Some(FileStripBidiFixer),
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "fix.{} is not compatible with no_bidi_controls",
                    other.op_name()
                ),
            ));
        }
        None => None,
    };
    Ok(Box::new(NoBidiControlsRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        fixer,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_first_rlo() {
        let s = "hi\n  \u{202E}reverse\n";
        let got = first_bidi(s).unwrap();
        assert_eq!(got.0, 2);
        assert_eq!(got.1, 3);
        assert_eq!(got.2, 0x202E);
    }

    #[test]
    fn flags_isolate_range() {
        for &cp in &[0x2066u32, 0x2067, 0x2068, 0x2069] {
            let c = char::from_u32(cp).unwrap();
            let s = format!("a{c}b");
            let got = first_bidi(&s).unwrap();
            assert_eq!(got.2, cp);
        }
    }

    #[test]
    fn clean_ascii_passes() {
        assert!(first_bidi("nothing to see here\n").is_none());
    }

    #[test]
    fn non_bidi_unicode_passes() {
        // ☃ snowman is not a bidi control.
        assert!(first_bidi("☃ chilly ☃\n").is_none());
    }
}
