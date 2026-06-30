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

/// A control char alint emits on purpose and must preserve: the newline that
/// [`crate::wrap_message`] treats as a paragraph break in a wrapped message.
const fn is_intentional(c: char) -> bool {
    c == '\n'
}

/// Render untrusted `s` safe to write to a terminal: every control character
/// (C0, `DEL`, C1) except the intentional newline is replaced with a visible,
/// inert `\xNN` escape — so an embedded `ESC` becomes the four literal
/// characters `\x1b`, carrying no control byte for the terminal to act on.
///
/// Returns the input borrowed unchanged when there is nothing to strip (the
/// overwhelming common case), so clean output pays no allocation.
pub(crate) fn sanitize_terminal(s: &str) -> Cow<'_, str> {
    if !s.chars().any(|c| c.is_control() && !is_intentional(c)) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if c.is_control() && !is_intentional(c) {
            use std::fmt::Write as _;
            // Control chars are Unicode category Cc: U+0000–U+001F and
            // U+007F–U+009F, all ≤ 0x9f, so a 2-digit hex escape suffices.
            let _ = write!(out, "\\x{:02x}", c as u32);
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
}
