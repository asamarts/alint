use std::path::Path;

use alint_core::{Error, FixContext, FixEdit, FixOutcome, Fixer, Result, Violation};

use crate::io::{looks_binary, write_atomic};

/// Strips Unicode bidi control characters (the Trojan Source
/// codepoints U+202A–202E, U+2066–2069) from the file's content.
#[derive(Debug)]
pub struct FileStripBidiFixer;

impl Fixer for FileStripBidiFixer {
    fn describe(&self) -> String {
        "strip Unicode bidi control characters".to_string()
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        apply_char_filter(
            "bidi",
            "stripped bidi controls from",
            violation,
            ctx,
            crate::no_bidi_controls::is_bidi_control,
            /* preserve_leading_feff = */ false,
        )
    }

    fn fix_edit(&self, violation: &Violation, bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        char_filter_edit(
            violation,
            bytes,
            crate::no_bidi_controls::is_bidi_control,
            false,
        )
    }
}

/// Strips zero-width characters (U+200B / U+200C / U+200D, plus
/// body-internal U+FEFF — a leading BOM is preserved so
/// `no_bom` can own that concern).
#[derive(Debug)]
pub struct FileStripZeroWidthFixer;

impl Fixer for FileStripZeroWidthFixer {
    fn describe(&self) -> String {
        "strip zero-width characters (U+200B/C/D, body-internal U+FEFF)".to_string()
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        apply_char_filter(
            "zero-width",
            "stripped zero-width chars from",
            violation,
            ctx,
            |c| matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}'),
            /* preserve_leading_feff = */ true,
        )
    }

    fn fix_edit(&self, violation: &Violation, bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        char_filter_edit(
            violation,
            bytes,
            |c| matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}'),
            true,
        )
    }
}

/// Strips a leading BOM (UTF-8 / UTF-16 / UTF-32 LE & BE) from
/// the violating file.
#[derive(Debug)]
pub struct FileStripBomFixer;

impl Fixer for FileStripBomFixer {
    fn describe(&self) -> String {
        "strip leading BOM".to_string()
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(path) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let abs = ctx.root.join(path);
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would strip BOM from {}",
                path.display()
            )));
        }
        let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
            alint_core::ReadForFix::Bytes(b) => b,
            alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        if looks_binary(&existing) {
            return Ok(FixOutcome::Skipped(format!(
                "{} looks binary; not stripping a BOM",
                path.display()
            )));
        }
        let Some(bom) = crate::no_bom::detect_bom(&existing) else {
            return Ok(FixOutcome::Skipped(format!(
                "{} has no BOM",
                path.display()
            )));
        };
        let stripped = &existing[bom.byte_len()..];
        write_atomic(&abs, stripped).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "stripped {} BOM from {}",
            bom.name(),
            path.display()
        )))
    }

    fn fix_edit(&self, violation: &Violation, bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        let path = violation.path.as_deref()?;
        let bom = crate::no_bom::detect_bom(bytes)?;
        Some(FixEdit::SetContent {
            path: path.to_path_buf(),
            content: bytes[bom.byte_len()..].to_vec(),
        })
    }
}

/// Shared read-modify-write helper for "remove every char that
/// matches `predicate`" fix ops.
fn apply_char_filter(
    label: &str,
    verb: &str,
    violation: &Violation,
    ctx: &FixContext<'_>,
    predicate: impl Fn(char) -> bool,
    preserve_leading_feff: bool,
) -> Result<FixOutcome> {
    let Some(path) = &violation.path else {
        return Ok(FixOutcome::Skipped(
            "violation did not carry a path".to_string(),
        ));
    };
    let abs = ctx.root.join(path);
    if ctx.dry_run {
        return Ok(FixOutcome::Applied(format!(
            "would strip {label} chars from {}",
            path.display()
        )));
    }
    let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
        alint_core::ReadForFix::Bytes(b) => b,
        alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
    };
    let Ok(text) = std::str::from_utf8(&existing) else {
        return Ok(FixOutcome::Skipped(format!(
            "{} is not UTF-8; cannot filter {label} chars",
            path.display()
        )));
    };
    let out = filter_chars(text, predicate, preserve_leading_feff);
    if out.as_bytes() == existing {
        return Ok(FixOutcome::Skipped(format!(
            "{} has no {label} chars to strip",
            path.display()
        )));
    }
    write_atomic(&abs, out.as_bytes()).map_err(|source| Error::Io {
        path: abs.clone(),
        source,
    })?;
    Ok(FixOutcome::Applied(format!("{verb} {}", path.display())))
}

/// Pure "drop every char matching `predicate`" transform, shared by the
/// disk-writing `apply_char_filter` and the editor-edit `char_filter_edit`
/// so the two paths can't diverge.
fn filter_chars(
    text: &str,
    predicate: impl Fn(char) -> bool,
    preserve_leading_feff: bool,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut first_char = true;
    for c in text.chars() {
        let keep_because_leading_bom = preserve_leading_feff && first_char && c == '\u{FEFF}';
        if keep_because_leading_bom || !predicate(c) {
            out.push(c);
        }
        first_char = false;
    }
    out
}

/// [`FixEdit`] form of the char-filter fixers: returns `None` when the
/// violation has no path, the content isn't UTF-8, or nothing changes.
fn char_filter_edit(
    violation: &Violation,
    bytes: &[u8],
    predicate: impl Fn(char) -> bool,
    preserve_leading_feff: bool,
) -> Option<FixEdit> {
    let path = violation.path.as_deref()?;
    let text = std::str::from_utf8(bytes).ok()?;
    let out = filter_chars(text, predicate, preserve_leading_feff);
    if out.as_bytes() == bytes {
        return None;
    }
    Some(FixEdit::SetContent {
        path: path.to_path_buf(),
        content: out.into_bytes(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v() -> Violation {
        Violation::new("x").with_path(std::path::Path::new("a.txt"))
    }

    #[test]
    fn bidi_fix_edit_strips_control_chars() {
        // U+202E (RLO) embedded in otherwise-ASCII content.
        let edit = FileStripBidiFixer
            .fix_edit(&v(), "a\u{202E}b".as_bytes(), std::path::Path::new("/r"))
            .unwrap();
        assert_eq!(
            edit,
            FixEdit::SetContent {
                path: std::path::PathBuf::from("a.txt"),
                content: b"ab".to_vec(),
            }
        );
    }

    #[test]
    fn bidi_fix_edit_none_when_clean() {
        assert!(
            FileStripBidiFixer
                .fix_edit(&v(), b"clean ascii", std::path::Path::new("/r"))
                .is_none()
        );
    }

    #[test]
    fn zero_width_fix_edit_strips_but_preserves_leading_bom() {
        let edit = FileStripZeroWidthFixer
            .fix_edit(
                &v(),
                "\u{FEFF}a\u{200B}b".as_bytes(),
                std::path::Path::new("/r"),
            )
            .unwrap();
        let FixEdit::SetContent { content, .. } = edit else {
            panic!("expected SetContent");
        };
        assert_eq!(content, "\u{FEFF}ab".as_bytes());
    }

    #[test]
    fn bom_fix_edit_strips_leading_bom() {
        let edit = FileStripBomFixer
            .fix_edit(&v(), "\u{FEFF}hello".as_bytes(), std::path::Path::new("/r"))
            .unwrap();
        assert_eq!(
            edit,
            FixEdit::SetContent {
                path: std::path::PathBuf::from("a.txt"),
                content: b"hello".to_vec(),
            }
        );
    }

    #[test]
    fn bom_fix_edit_none_when_no_bom() {
        assert!(
            FileStripBomFixer
                .fix_edit(&v(), b"no bom", std::path::Path::new("/r"))
                .is_none()
        );
    }
}
