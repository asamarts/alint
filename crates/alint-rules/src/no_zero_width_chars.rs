//! `no_zero_width_chars` — flag invisible zero-width characters
//! that can hide text, break identifiers, or leak data.
//!
//! Codepoints flagged:
//!   - U+200B ZERO WIDTH SPACE
//!   - U+200C ZERO WIDTH NON-JOINER
//!   - U+200D ZERO WIDTH JOINER
//!   - U+2060 WORD JOINER (the no-break sibling of U+200B)
//!   - U+180E MONGOLIAN VOWEL SEPARATOR (renders zero-width)
//!   - U+FEFF ZERO WIDTH NO-BREAK SPACE (BOM) — *but only when
//!     not at byte position 0*. A leading BOM is `no_bom`'s
//!     territory; this rule stays focused on body-internal ZWs
//!     so the two rules don't double-report.
//!
//! Note on U+200D (ZWJ): it is flagged even though it joins emoji
//! sequences (e.g. the multi-person "family" emoji, built by joining
//! several person glyphs with ZWJ), because in source it is far more
//! often an obfuscation vector than legitimate. The strip fixer
//! therefore *will* break a literal emoji ZWJ sequence — scope the rule
//! away from files that legitimately carry such emoji. (Grapheme-cluster-
//! aware ZWJ handling is a possible future refinement.)

use std::path::Path;

use alint_core::{
    Context, Error, FixSpec, Fixer, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation,
    eval_per_file,
};

use crate::fixers::FileStripZeroWidthFixer;

/// Returns true if `c` is a zero-width character that this rule
/// flags. `is_leading_feff == true` means U+FEFF at byte 0 of
/// the file (the BOM case) - that's deliberately NOT flagged.
pub fn is_flagged_zero_width(c: char, is_leading_feff: bool) -> bool {
    match c {
        '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{180E}' => true,
        '\u{FEFF}' => !is_leading_feff,
        _ => false,
    }
}

#[derive(Debug)]
pub struct NoZeroWidthCharsRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    fixer: Option<FileStripZeroWidthFixer>,
}

impl Rule for NoZeroWidthCharsRule {
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

impl PerFileRule for NoZeroWidthCharsRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        // Lossily decode rather than abandon the file on the first invalid byte:
        // this is a security-posture rule, so a stray non-UTF-8 byte must NOT
        // suppress detection of a zero-width char elsewhere (fail-open evasion).
        let text = String::from_utf8_lossy(bytes);
        let Some((line_no, col, codepoint)) = first_zero_width(&text) else {
            return Ok(Vec::new());
        };
        let msg = self.message.clone().unwrap_or_else(|| {
            format!("zero-width character U+{codepoint:04X} at line {line_no} col {col}")
        });
        Ok(vec![
            Violation::new(msg)
                .with_path(std::sync::Arc::<Path>::from(path))
                .with_location(line_no, col),
        ])
    }
}

fn first_zero_width(text: &str) -> Option<(usize, usize, u32)> {
    let mut line = 1usize;
    let mut col = 1usize;
    let mut first_char = true;
    for c in text.chars() {
        let is_leading = first_char && c == '\u{FEFF}';
        if !is_leading && is_flagged_zero_width(c, false) {
            return Some((line, col, c as u32));
        }
        first_char = false;
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
    let _paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(&spec.id, "no_zero_width_chars requires a `paths` field")
    })?;
    let fixer = match &spec.fix {
        Some(FixSpec::FileStripZeroWidth { .. }) => Some(FileStripZeroWidthFixer),
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "fix.{} is not compatible with no_zero_width_chars",
                    other.op_name()
                ),
            ));
        }
        None => None,
    };
    Ok(Box::new(NoZeroWidthCharsRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        fixer,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_zwsp() {
        let s = "ab\u{200B}cd";
        let (line, col, cp) = first_zero_width(s).unwrap();
        assert_eq!((line, col, cp), (1, 3, 0x200B));
    }

    #[test]
    fn flags_zwj() {
        assert_eq!(first_zero_width("\u{200D}x").unwrap().2, 0x200D);
    }

    #[test]
    fn flags_word_joiner_and_mongolian_vowel_separator() {
        // L1: U+2060 (WORD JOINER) and U+180E complete the zero-width set.
        assert_eq!(first_zero_width("a\u{2060}b").unwrap().2, 0x2060);
        assert_eq!(first_zero_width("a\u{180E}b").unwrap().2, 0x180E);
    }

    #[test]
    fn leading_bom_is_not_flagged() {
        assert!(first_zero_width("\u{FEFF}hello\n").is_none());
    }

    #[test]
    fn midstream_feff_is_flagged() {
        let (line, col, cp) = first_zero_width("hello\u{FEFF}world").unwrap();
        assert_eq!((line, col, cp), (1, 6, 0xFEFF));
    }

    #[test]
    fn clean_ascii_passes() {
        assert!(first_zero_width("nothing hidden here\n").is_none());
    }

    fn rule() -> NoZeroWidthCharsRule {
        NoZeroWidthCharsRule {
            id: "no-zw".to_string(),
            level: Level::Error,
            policy_url: None,
            message: None,
            scope: Scope::match_all(),
            fixer: None,
        }
    }

    #[test]
    fn invalid_utf8_byte_does_not_suppress_a_later_zero_width_char() {
        // Fail-open evasion regression (mirrors no_bidi_controls): a stray
        // `0xFF` must not abandon the scan — a hidden ZWSP after it is still
        // flagged. evaluate_file decodes lossily rather than strictly.
        let idx = alint_core::FileIndex::from_entries(Vec::new());
        let ctx = Context {
            root: Path::new("/r"),
            index: &idx,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
        };
        let mut bytes = vec![0xFFu8];
        bytes.extend_from_slice("a\u{200B}b".as_bytes());
        let vs = rule()
            .evaluate_file(&ctx, Path::new("a.rs"), &bytes)
            .unwrap();
        assert_eq!(
            vs.len(),
            1,
            "the ZWSP after a bad byte must still be flagged"
        );
    }
}
