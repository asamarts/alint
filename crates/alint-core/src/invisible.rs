//! Invisible and text-reordering characters: the one definition shared by the
//! `no_bidi_controls` / `no_zero_width_chars` rules, their strip fixers, and
//! the terminal sanitizer in `alint-output`.
//!
//! Keeping the sets here, below both consumers, means the sanitizer can never
//! fall behind the rules: whatever a rule flags as dangerous in a file, alint
//! also refuses to print raw.

/// Returns true if `c` is one of the Unicode bidi control characters
/// (the five explicit embeddings/overrides, the four isolates, and the
/// three implicit directional marks ALM/LRM/RLM).
#[must_use]
pub const fn is_bidi_control(c: char) -> bool {
    matches!(c,
        '\u{061C}'                  // ALM
        | '\u{200E}' | '\u{200F}'   // LRM, RLM
        | '\u{202A}'..='\u{202E}'   // LRE, RLE, PDF, LRO, RLO
        | '\u{2066}'..='\u{2069}') // LRI, RLI, FSI, PDI
}

/// Returns true if `c` is a zero-width character that `no_zero_width_chars`
/// flags. `is_leading_feff == true` means U+FEFF at byte 0 of the file (the
/// BOM case) - that's deliberately NOT flagged.
#[must_use]
pub const fn is_flagged_zero_width(c: char, is_leading_feff: bool) -> bool {
    match c {
        '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{180E}' => true,
        '\u{FEFF}' => !is_leading_feff,
        _ => false,
    }
}

/// Characters that render as nothing (or as a line break) and so can hide or
/// disguise text on a terminal, beyond the two rule sets above. Not flagged in
/// files (they have legitimate uses there); only escaped when alint prints.
///
/// The common emoji variation selectors U+FE00-U+FE0F are deliberately absent:
/// U+FE0F follows most emoji (as in a red heart), and escaping it would garble
/// ordinary text. The tag characters are present even though emoji subdivision
/// flags use them, because they are also the "ASCII smuggling" channel for
/// hidden instructions.
#[must_use]
pub const fn is_hidden_on_display(c: char) -> bool {
    matches!(c,
        '\u{00AD}'                      // SOFT HYPHEN
        | '\u{034F}'                    // COMBINING GRAPHEME JOINER
        | '\u{115F}' | '\u{1160}'       // HANGUL CHOSEONG / JUNGSEONG FILLER
        | '\u{17B4}' | '\u{17B5}'       // KHMER VOWEL INHERENT AQ / AA
        | '\u{2028}' | '\u{2029}'       // LINE / PARAGRAPH SEPARATOR
        | '\u{2061}'..='\u{2064}'       // invisible math operators
        | '\u{206A}'..='\u{206F}'       // deprecated format characters
        | '\u{3164}'                    // HANGUL FILLER
        | '\u{FFA0}'                    // HALFWIDTH HANGUL FILLER
        | '\u{FFF9}'..='\u{FFFB}'       // interlinear annotation controls
        | '\u{1D173}'..='\u{1D17A}'     // musical symbol format controls
        | '\u{E0000}'..='\u{E007F}'     // tag characters
        | '\u{E0100}'..='\u{E01EF}') // variation selectors supplement
}

/// Whether printing `c` raw could reorder or hide what a reader sees: a bidi
/// control, a flagged zero-width character (U+FEFF included, wherever it
/// sits), or another display-invisible character.
#[must_use]
pub const fn is_deceptive_on_display(c: char) -> bool {
    is_bidi_control(c) || is_flagged_zero_width(c, false) || is_hidden_on_display(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_set_covers_both_rule_sets() {
        for c in (0..=0x0010_FFFF).filter_map(char::from_u32) {
            if is_bidi_control(c) || is_flagged_zero_width(c, false) {
                assert!(
                    is_deceptive_on_display(c),
                    "{:04X} flagged in files but printed raw",
                    c as u32
                );
            }
        }
    }

    #[test]
    fn ordinary_text_is_not_deceptive() {
        for s in [
            "café",
            "שלום",
            "مرحبا",
            "日本語",
            "❤\u{FE0F}",
            "😀",
            "a b\tc",
        ] {
            assert!(!s.chars().any(is_deceptive_on_display), "{s:?}");
        }
    }

    #[test]
    fn leading_bom_is_only_exempt_in_the_rule() {
        assert!(!is_flagged_zero_width('\u{FEFF}', true));
        assert!(is_flagged_zero_width('\u{FEFF}', false));
        assert!(is_deceptive_on_display('\u{FEFF}'));
    }
}
