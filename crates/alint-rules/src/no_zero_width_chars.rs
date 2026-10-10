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
        // Same binary policy as no_bidi_controls (see there): a binary-looking
        // file is scanned only when it is valid UTF-8 (so a NUL cannot hide a
        // char, but random image/font bytes are not lossily decoded into false
        // positives); a stray invalid byte in a text file is one U+FFFD.
        let binary = match crate::io::char_scan_mode(bytes) {
            crate::io::CharScan::Skip => return Ok(Vec::new()),
            crate::io::CharScan::BinaryUtf8 => true,
            crate::io::CharScan::Text => false,
        };
        let Some((line_no, col, codepoint)) = first_zero_width(bytes) else {
            return Ok(Vec::new());
        };
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "zero-width character U+{codepoint:04X} at line {line_no} col {col}{}",
                if binary {
                    crate::no_bidi_controls::BINARY_NOTE
                } else {
                    ""
                }
            )
        });
        Ok(vec![
            Violation::new(msg)
                .with_path(std::sync::Arc::<Path>::from(path))
                .with_location(line_no, col)
                // First-offender rule with a WHOLE-FILE fixer (it strips every
                // occurrence). The file is the unit of accepted debt: key on the
                // path so `fix --baseline` grandfathers the whole file and never
                // strips a grandfathered char when a NEW one precedes it (audit
                // F3, 2026-09-20). Matches no_trailing_whitespace.
                .with_baseline_key(crate::slash(path))
                // The strip fixer refuses binary content (see no_bidi_controls).
                .with_not_fixable_if(binary),
        ])
    }
}

fn first_zero_width(bytes: impl AsRef<[u8]>) -> Option<(usize, usize, u32)> {
    // A leading U+FEFF is a BOM (no_bom's job), not a zero-width char.
    crate::io::first_char_where(bytes.as_ref(), |c, first| {
        !(first && c == '\u{FEFF}') && is_flagged_zero_width(c, false)
    })
    .map(|(l, c, ch)| (l, c, ch as u32))
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

#[cfg(test)]
mod binary_evasion_tests {
    use super::*;

    #[test]
    fn a_nul_byte_does_not_hide_a_zero_width_char() {
        // Evasion regression (mirrors no_bidi_controls): a NUL byte must not
        // make the rule skip the file. The finding is reported but not marked
        // auto-fixable, since the strip fixer refuses binary content.
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
        let rule = NoZeroWidthCharsRule {
            id: "no-zw".to_string(),
            level: Level::Error,
            policy_url: None,
            message: None,
            scope: Scope::match_all(),
            fixer: Some(FileStripZeroWidthFixer),
        };
        let vs = rule
            .evaluate_file(&ctx, Path::new("a.rs"), "x\u{0}a\u{200B}b".as_bytes())
            .unwrap();
        assert_eq!(
            vs.len(),
            1,
            "the ZWSP in a NUL-bearing file must be flagged"
        );
        assert!(vs[0].not_fixable);
        let vs = rule
            .evaluate_file(&ctx, Path::new("a.rs"), "a\u{200B}b".as_bytes())
            .unwrap();
        assert!(!vs[0].not_fixable);
        assert_eq!(vs[0].baseline_key.as_deref(), Some("a.rs"));
    }

    fn rule() -> NoZeroWidthCharsRule {
        NoZeroWidthCharsRule {
            id: "no-zw".to_string(),
            level: Level::Error,
            policy_url: None,
            message: None,
            scope: Scope::match_all(),
            fixer: Some(FileStripZeroWidthFixer),
        }
    }

    fn eval(bytes: &[u8]) -> Vec<Violation> {
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
        rule().evaluate_file(&ctx, Path::new("f"), bytes).unwrap()
    }

    #[test]
    fn crafted_valid_utf8_with_nul_is_still_scanned() {
        // NUL is valid UTF-8: a source file with a NUL is binary-looking but
        // still scanned, so the NUL cannot hide the char.
        let vs = eval("int x;\u{0}// \u{200B} evil\n".as_bytes());
        assert_eq!(vs.len(), 1);
        assert!(vs[0].not_fixable);
        assert_eq!(vs[0].baseline_key.as_deref(), Some("f"));
    }

    #[test]
    fn png_like_invalid_utf8_binary_is_skipped() {
        // Real binaries (images, fonts) are invalid UTF-8; a lossy decode would
        // turn random bytes into controls (`D8 9C` -> U+061C; `E2 80 8B` is a
        // ZWSP). Such files are skipped, as before the evasion fix.
        let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\xff\xfe".to_vec();
        png.extend_from_slice(b"\xd8\x9c\x00\xe2\x80\x8b\xe2\x80\xae\x00\xc3");
        assert!(
            eval(&png).is_empty(),
            "invalid-UTF-8 binary must be skipped"
        );
    }

    #[test]
    fn stray_invalid_byte_counts_as_one_column() {
        // A text file with a lone invalid byte is scanned; the bad run counts as
        // one U+FFFD column, matching a lossy decode.
        let mut b = b"a\xffb".to_vec();
        b.extend_from_slice("\u{200B}".as_bytes());
        let vs = eval(&b);
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].column, Some(4));
    }
}
