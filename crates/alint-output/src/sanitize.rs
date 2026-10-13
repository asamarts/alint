//! Terminal-safety sanitizer for untrusted text in the human formatters.
//!
//! The human / compact / fix renderers interpolate attacker-controlled spans
//! — repo file *paths*, violation *messages* (which can embed a matched value
//! or a `kind: command` rule's subprocess stdout/stderr) — directly into the
//! terminal. A repo file named `evil\x1b[2J\x1b[H` or a command whose output
//! carries ANSI sequences could otherwise clear the screen, move the cursor,
//! hide findings below the fold, or forge an "all rules passed." banner when a
//! human lints an untrusted repo.
//!
//! alint's *own* styling is emitted as separate `{STYLE}…{STYLE:#}` tokens
//! around the content (and gated by `anstream`), so sanitizing the content
//! strings here never strips alint's colors. It runs unconditionally — not
//! only on a TTY — so output is byte-identical whether written to a terminal
//! or a pipe (`anstream` passes raw bytes straight through on a TTY, which is
//! precisely when injected escapes would fire).

use std::borrow::Cow;

use alint_core::invisible::is_deceptive_on_display;

/// A control char alint emits on purpose and must preserve: the newline that
/// [`crate::wrap_message`] treats as a paragraph break in a wrapped message.
const fn is_intentional(c: char) -> bool {
    c == '\n'
}

/// Whether `c` must be neutralized before reaching a terminal.
fn is_unsafe(c: char) -> bool {
    (c.is_control() && !is_intentional(c)) || is_deceptive_on_display(c)
}

/// Render untrusted `s` safe to write to a terminal: every control character
/// (C0, `DEL`, C1) except the intentional newline is replaced with a visible,
/// inert `\xNN` escape — so an embedded `ESC` becomes the four literal
/// characters `\x1b`, carrying no control byte for the terminal to act on.
/// Characters that reorder or hide displayed text become a visible
/// `\u{XXXX}` escape: everything `no_bidi_controls` and `no_zero_width_chars`
/// flag (one shared definition, [`alint_core::invisible`]) plus other
/// display-invisible characters such as the tag characters and U+2028/U+2029.
///
/// Returns the input borrowed unchanged when there is nothing to strip (the
/// overwhelming common case), so clean output pays no allocation.
#[must_use]
pub fn sanitize_terminal(s: &str) -> Cow<'_, str> {
    if !s.chars().any(is_unsafe) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        use std::fmt::Write as _;
        if c.is_control() && !is_intentional(c) {
            // Control chars are Unicode category Cc: U+0000–U+001F and
            // U+007F–U+009F, all ≤ 0x9f, so a 2-digit hex escape suffices.
            let _ = write!(out, "\\x{:02x}", c as u32);
        } else if is_deceptive_on_display(c) {
            let _ = write!(out, "\\u{{{:04x}}}", c as u32);
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_text_is_borrowed_unchanged() {
        let s = "src/main.rs: tidy up";
        assert!(matches!(sanitize_terminal(s), Cow::Borrowed(_)));
        assert_eq!(sanitize_terminal(s), s);
    }

    #[test]
    fn escape_sequences_are_neutralized() {
        // A path that clears the screen + forges a banner.
        let evil = "ok\x1b[2J\x1b[Hall rules passed.";
        let out = sanitize_terminal(evil);
        assert!(!out.contains('\x1b'), "no raw ESC survives: {out:?}");
        assert!(out.contains("\\x1b"), "ESC rendered visibly: {out:?}");
    }

    #[test]
    fn carriage_return_and_del_are_stripped() {
        // CR can overwrite the current line; DEL / C1 are interpreted by
        // some terminals. All neutralized.
        let out = sanitize_terminal("a\rb\x7fc\u{85}d");
        assert_eq!(out, "a\\x0db\\x7fc\\x85d");
    }

    #[test]
    fn newline_is_preserved_as_paragraph_break() {
        // `command` rule messages embed subprocess output with `\n`
        // separators that wrap_message renders as paragraph breaks.
        let out = sanitize_terminal("failed (1):\nline one\nline two");
        assert_eq!(out, "failed (1):\nline one\nline two");
    }

    #[test]
    fn tab_is_neutralized_but_newline_kept() {
        let out = sanitize_terminal("a\tb\nc");
        assert_eq!(out, "a\\x09b\nc");
    }

    #[test]
    fn bidi_and_zero_width_are_made_visible() {
        // Trojan-Source style reordering + hidden zero-width content.
        let out = sanitize_terminal("ok\u{202E}gnp.exe\u{2066}x\u{200B}y\u{FEFF}\u{061C}");
        for c in ['\u{202E}', '\u{2066}', '\u{200B}', '\u{FEFF}', '\u{061C}'] {
            assert!(!out.contains(c), "{c:?} survived: {out:?}");
        }
        assert_eq!(
            out,
            "ok\\u{202e}gnp.exe\\u{2066}x\\u{200b}y\\u{feff}\\u{061c}"
        );
        // Ordinary non-ASCII text is untouched.
        assert!(matches!(
            sanitize_terminal("caf\u{e9} \u{1F600}"),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn everything_the_rules_flag_is_escaped() {
        // U+180E is flagged by `no_zero_width_chars` and was printed raw while
        // the sanitizer kept its own list; now both read one definition.
        for c in (0..=0x0010_FFFF).filter_map(char::from_u32) {
            if alint_core::invisible::is_bidi_control(c)
                || alint_core::invisible::is_flagged_zero_width(c, false)
            {
                let s = c.to_string();
                assert!(
                    !sanitize_terminal(&s).contains(c),
                    "{:04X} printed raw",
                    c as u32
                );
            }
        }
        assert_eq!(sanitize_terminal("a\u{180E}b"), "a\\u{180e}b");
    }

    #[test]
    fn hidden_tag_characters_and_line_separators_are_made_visible() {
        // "ASCII smuggling": tag characters spell hidden text a terminal won't
        // show. U+2028 can render as a line break and forge an extra line.
        let hidden: String = "ignore"
            .chars()
            .map(|c| char::from_u32(0xE0000 + c as u32).unwrap())
            .collect();
        let input = format!("ok{hidden}\u{2028}\u{00AD}x");
        let out = sanitize_terminal(&input);
        assert!(
            !out.chars()
                .any(|c| ('\u{E0000}'..='\u{E007F}').contains(&c)),
            "{out:?}"
        );
        assert!(out.starts_with("ok\\u{e0069}\\u{e0067}"), "{out:?}");
        assert!(out.ends_with("\\u{2028}\\u{00ad}x"), "{out:?}");
    }

    #[test]
    fn rtl_text_and_emoji_pass_through() {
        for s in [
            "\u{5e9}\u{5dc}\u{5d5}\u{5dd}",
            "\u{645}\u{631}\u{62d}\u{628}\u{627}",
            "\u{2764}\u{FE0F}",
            "\u{1F468}",
        ] {
            assert!(matches!(sanitize_terminal(s), Cow::Borrowed(_)), "{s:?}");
        }
    }
}
