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
//! Plus the implicit directional marks, which reorder neighbouring
//! runs without an explicit embedding and so can still mislead a
//! reader (rustc's Trojan-Source lint flags these too):
//!   - U+061C ARABIC LETTER MARK
//!   - U+200E LEFT-TO-RIGHT MARK
//!   - U+200F RIGHT-TO-LEFT MARK
//!
//! Text files are scanned even when they hold stray invalid UTF-8 (each
//! invalid run counts as one U+FFFD), so a junk byte cannot hide a control. A
//! binary-looking file is scanned only when it is valid UTF-8 -- NUL is valid
//! UTF-8, so a NUL byte cannot hide a control in a crafted source file -- and a
//! binary-looking file that is not valid UTF-8 (images, fonts, archives) is
//! skipped. A finding in a binary-looking file is reported but not auto-fixed
//! (the strip fixer refuses to edit binary content).

use std::path::Path;

use alint_core::{
    Context, Error, FixSpec, Fixer, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation,
    eval_per_file,
};

use crate::fixers::FileStripBidiFixer;

/// Returns true if `c` is one of the Unicode bidi control characters
/// (the five explicit embeddings/overrides, the four isolates, and the
/// three implicit directional marks ALM/LRM/RLM).
pub fn is_bidi_control(c: char) -> bool {
    matches!(c,
        '\u{061C}'                  // ALM
        | '\u{200E}' | '\u{200F}'   // LRM, RLM
        | '\u{202A}'..='\u{202E}'   // LRE, RLE, PDF, LRO, RLO
        | '\u{2066}'..='\u{2069}') // LRI, RLI, FSI, PDI
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

impl PerFileRule for NoBidiControlsRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        // A binary-looking file is scanned only when it is valid UTF-8. NUL is
        // valid UTF-8, so one NUL byte cannot hide a control in a crafted source
        // file (the Trojan-Source fail-open evasion); but a genuinely binary file
        // (PNG / font / JPEG: invalid UTF-8) is skipped, because a lossy decode
        // of random bytes manufactures controls out of noise (`D8 9C` is U+061C).
        // A finding in a binary-looking file is reported but marked not fixable
        // (the strip fixer refuses binary content).
        //
        // A non-binary file with a stray invalid byte is still scanned (each
        // invalid run counts as one U+FFFD), so a lone `0xFF` cannot suppress
        // detection either.
        let binary = match crate::io::char_scan_mode(bytes) {
            crate::io::CharScan::Skip => return Ok(Vec::new()),
            crate::io::CharScan::BinaryUtf8 => true,
            crate::io::CharScan::Text => false,
        };
        let Some((line_no, col, codepoint)) = first_bidi(bytes) else {
            return Ok(Vec::new());
        };
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "Unicode bidi control U+{codepoint:04X} at line {line_no} col {col} \
                 (Trojan-Source defense){}",
                if binary { BINARY_NOTE } else { "" }
            )
        });
        Ok(vec![
            Violation::new(msg)
                .with_path(std::sync::Arc::<Path>::from(path))
                .with_location(line_no, col)
                // First-offender rule with a WHOLE-FILE fixer (it strips every
                // occurrence). The file is the unit of accepted debt: key on the
                // path so `fix --baseline` grandfathers the whole file and never
                // strips a grandfathered control when a NEW one precedes it (audit
                // F3, 2026-09-20). Matches no_trailing_whitespace.
                .with_baseline_key(crate::slash(path))
                // The strip fixer refuses binary content, so `check` must not
                // promise a fix for a binary-looking file (a flag, not a key
                // prefix, so the baseline fingerprint stays the path).
                .with_not_fixable_if(binary),
        ])
    }
}

/// Default-message suffix for a finding in a binary-looking file. Shared with
/// `no_zero_width_chars`.
pub(crate) const BINARY_NOTE: &str = "; the file looks binary, so it is not auto-fixed";

/// Scan for the first bidi control character and return
/// (1-based line, 1-based column, codepoint as u32).
fn first_bidi(bytes: impl AsRef<[u8]>) -> Option<(usize, usize, u32)> {
    crate::io::first_char_where(bytes.as_ref(), |c, _| is_bidi_control(c))
        .map(|(l, c, ch)| (l, c, ch as u32))
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let _paths = spec
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
        scope: Scope::from_spec(spec)?,
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
    fn flags_implicit_directional_marks() {
        // L1: ALM, LRM, RLM complete the Trojan-Source set (rustc flags these).
        for &cp in &[0x061Cu32, 0x200E, 0x200F] {
            let c = char::from_u32(cp).unwrap();
            let got = first_bidi(format!("a{c}b")).unwrap();
            assert_eq!(got.2, cp, "codepoint U+{cp:04X} must be flagged");
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

    fn rule() -> NoBidiControlsRule {
        NoBidiControlsRule {
            id: "no-bidi".to_string(),
            level: Level::Error,
            policy_url: None,
            message: None,
            scope: Scope::match_all(),
            fixer: None,
        }
    }

    #[test]
    fn invalid_utf8_byte_does_not_suppress_a_later_bidi_control() {
        // Fail-open evasion regression: an earlier code path abandoned the whole
        // file on the first invalid UTF-8 byte (`str::from_utf8` else-return
        // empty), so appending a stray `0xFF` anywhere hid every bidi override —
        // a trivial bypass of a CVE-2021-42574 defense. evaluate_file must decode
        // lossily and still flag the RLO after the bad byte.
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
        let mut bytes = vec![0xFFu8]; // invalid UTF-8 lead byte
        bytes.extend_from_slice("code\u{202E}evil".as_bytes());
        let vs = rule()
            .evaluate_file(&ctx, Path::new("a.rs"), &bytes)
            .unwrap();
        assert_eq!(
            vs.len(),
            1,
            "the RLO after a bad byte must still be flagged"
        );
    }
}

#[cfg(test)]
mod binary_evasion_tests {
    use super::*;

    #[test]
    fn a_nul_byte_does_not_hide_a_bidi_control() {
        // Trojan-Source evasion regression: one NUL made `looks_binary` true and
        // the rule skipped the file, so `\0` + RLO passed. The security rule now
        // scans every in-scope file; the finding on a binary-looking file is
        // reported but marked not-auto-fixable (the strip fixer refuses binary).
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
        let rule = NoBidiControlsRule {
            id: "no-bidi".to_string(),
            level: Level::Error,
            policy_url: None,
            message: None,
            scope: Scope::match_all(),
            fixer: Some(FileStripBidiFixer),
        };
        let vs = rule
            .evaluate_file(&ctx, Path::new("a.rs"), "x\u{0}code\u{202E}evil".as_bytes())
            .unwrap();
        assert_eq!(vs.len(), 1, "the RLO in a NUL-bearing file must be flagged");
        assert!(
            vs[0].not_fixable,
            "check must not promise a fix the binary guard refuses"
        );
        // A text file's finding stays fixable.
        let vs = rule
            .evaluate_file(&ctx, Path::new("a.rs"), "a\u{202E}b".as_bytes())
            .unwrap();
        assert!(!vs[0].not_fixable);
        assert_eq!(vs[0].baseline_key.as_deref(), Some("a.rs"));
    }

    fn rule() -> NoBidiControlsRule {
        NoBidiControlsRule {
            id: "no-bidi".to_string(),
            level: Level::Error,
            policy_url: None,
            message: None,
            scope: Scope::match_all(),
            fixer: Some(FileStripBidiFixer),
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
        let vs = eval("int x;\u{0}// \u{202E} evil\n".as_bytes());
        assert_eq!(vs.len(), 1);
        assert!(vs[0].not_fixable);
        // Fixability is a flag, never folded into the baseline fingerprint.
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
        b.extend_from_slice("\u{202E}".as_bytes());
        let vs = eval(&b);
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].column, Some(4));
    }
}
