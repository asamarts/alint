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
//! Non-UTF-8 files are skipped (can't have these codepoints
//! anyway without being invalid UTF-8).

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
        // Skip binary content (NUL-bearing): the `file_strip_bidi` fixer refuses
        // it (editing binary would corrupt it), so flagging it here would nag a
        // file that can never be fixed -- `check` and `fix` must agree on scope.
        // A lone invalid byte (`0xFF`, no NUL) is NOT binary, so this does not
        // reopen the fail-open evasion below: such a file is still scanned.
        if crate::io::looks_binary(bytes) {
            return Ok(Vec::new());
        }
        // Lossily decode rather than abandon the whole file on the first invalid
        // byte: this is a Trojan-Source (CVE-2021-42574) defense, so a single
        // stray `0xFF` must NOT suppress detection of a bidi override elsewhere
        // in the file (a trivial fail-open evasion otherwise). `U+FFFD` replaces
        // only the invalid bytes; bidi controls in the valid runs are preserved.
        let text = String::from_utf8_lossy(bytes);
        let Some((line_no, col, codepoint)) = first_bidi(&text) else {
            return Ok(Vec::new());
        };
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "Unicode bidi control U+{codepoint:04X} at line {line_no} col {col} \
                 (Trojan-Source defense)"
            )
        });
        Ok(vec![
            Violation::new(msg)
                .with_path(std::sync::Arc::<Path>::from(path))
                .with_location(line_no, col),
        ])
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
            let got = first_bidi(&format!("a{c}b")).unwrap();
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
