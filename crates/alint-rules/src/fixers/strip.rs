use std::path::Path;

use alint_core::{Error, FixContext, FixEdit, FixOutcome, Fixer, Result, Violation};

use crate::io::looks_binary;

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

/// Strips zero-width characters (U+200B / U+200C / U+200D / U+2060 /
/// U+180E, plus body-internal U+FEFF — a leading BOM is preserved so
/// `no_bom` can own that concern).
///
/// The flagged set is not hard-coded here: both fix paths defer to the
/// detector's [`crate::no_zero_width_chars::is_flagged_zero_width`], so the
/// fixer can never strip a narrower set than the rule flags — that skew
/// made `--fix` non-convergent (U+2060 / U+180E were reported every run but
/// never removed) until the two were unified.
#[derive(Debug)]
pub struct FileStripZeroWidthFixer;

impl Fixer for FileStripZeroWidthFixer {
    fn describe(&self) -> String {
        "strip zero-width characters (U+200B/C/D, U+2060, U+180E, body-internal U+FEFF)".to_string()
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        apply_char_filter(
            "zero-width",
            "stripped zero-width chars from",
            violation,
            ctx,
            // `is_leading_feff = false`: a leading BOM is already exempted by
            // `preserve_leading_feff` in `filter_chars`, so the predicate only
            // needs to flag body-internal U+FEFF.
            |c| crate::no_zero_width_chars::is_flagged_zero_width(c, false),
            /* preserve_leading_feff = */ true,
        )
    }

    fn fix_edit(&self, violation: &Violation, bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        char_filter_edit(
            violation,
            bytes,
            |c| crate::no_zero_width_chars::is_flagged_zero_width(c, false),
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
        ctx.commit_write(&abs, stripped)
            .map_err(|source| Error::Io {
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
        // Mirror `apply`'s binary guard so the editor (LSP) fix path and the disk
        // path behave identically: don't strip a "BOM" prefix from a file that's
        // actually binary (a leading `EF BB BF` may be data, not an encoding mark).
        if looks_binary(bytes) {
            return None;
        }
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
    // Binary guard (H3): a NUL byte is valid UTF-8, so the `from_utf8` check
    // below is too weak on its own -- stripping a bidi/zero-width byte sequence
    // out of a NUL-bearing binary would corrupt it. Match the other byte-level
    // fixers and refuse.
    if looks_binary(&existing) {
        return Ok(FixOutcome::Skipped(format!(
            "{} looks binary; not stripping {label} chars",
            path.display()
        )));
    }
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
    ctx.commit_write(&abs, out.as_bytes())
        .map_err(|source| Error::Io {
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
    // Binary guard (H3), mirroring apply_char_filter on the editor (LSP) path.
    if looks_binary(bytes) {
        return None;
    }
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
    fn strip_fixers_skip_binary_on_both_paths() {
        // Phase-0 audit / H3 consistency: strip-bidi and strip-zero-width are
        // byte-level fixers too, so they must refuse a NUL-bearing binary on both
        // the `alint fix` (apply) and editor (fix_edit) paths, even though the
        // file also carries the char they would otherwise strip. NUL is valid
        // UTF-8, so the `from_utf8` check alone would not catch it.
        use tempfile::TempDir;
        // valid UTF-8: 'a', U+0000 (NUL -> binary), U+202E (bidi), U+200B (ZWSP).
        let binary = "a\u{0}\u{202e}\u{200b}b".as_bytes();
        let viol = Violation::new("x").with_path(std::path::Path::new("blob"));
        let fixers: [&dyn Fixer; 2] = [&FileStripBidiFixer, &FileStripZeroWidthFixer];
        for fixer in fixers {
            assert!(
                fixer
                    .fix_edit(&viol, binary, std::path::Path::new("/r"))
                    .is_none(),
                "strip fix_edit must decline a binary file"
            );
            let tmp = TempDir::new().unwrap();
            std::fs::write(tmp.path().join("blob"), binary).unwrap();
            let ctx = FixContext {
                root: tmp.path(),
                dry_run: false,
                fix_size_limit: None,
                allow_out_of_root: false,
                compose: None,
                stage_ops: None,
            };
            let outcome = fixer.apply(&viol, &ctx).unwrap();
            assert!(
                matches!(outcome, FixOutcome::Skipped(_)),
                "strip apply() must skip a binary, got {outcome:?}"
            );
            assert_eq!(
                std::fs::read(tmp.path().join("blob")).unwrap(),
                binary,
                "the binary file must be byte-identical after skipping"
            );
        }
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
    fn zero_width_fix_edit_strips_word_joiner_and_mongolian_vowel_sep() {
        // Regression (L1): the detector flags U+2060 (WORD JOINER) and U+180E
        // (MONGOLIAN VOWEL SEPARATOR), but the fixer used to hard-code only
        // U+200B/C/D/FEFF, so a file containing 2060/180E was reported every
        // run yet never repaired — `--fix` never converged.
        let edit = FileStripZeroWidthFixer
            .fix_edit(
                &v(),
                "a\u{2060}b\u{180E}c".as_bytes(),
                std::path::Path::new("/r"),
            )
            .unwrap();
        let FixEdit::SetContent { content, .. } = edit else {
            panic!("expected SetContent");
        };
        assert_eq!(content, b"abc");
    }

    #[test]
    fn zero_width_fix_converges_leaving_nothing_the_detector_flags() {
        // The fix output must be a fixed point of the detector: run the fixer,
        // then assert no surviving char is still flagged (a leading BOM aside).
        // This is the invariant that keeps the fixer and rule from drifting.
        let input = "\u{FEFF}x\u{200B}y\u{200C}z\u{200D}w\u{2060}v\u{180E}u\u{FEFF}t";
        let edit = FileStripZeroWidthFixer
            .fix_edit(&v(), input.as_bytes(), std::path::Path::new("/r"))
            .unwrap();
        let FixEdit::SetContent { content, .. } = edit else {
            panic!("expected SetContent");
        };
        let out = std::str::from_utf8(&content).unwrap();
        assert_eq!(out, "\u{FEFF}xyzwvut");
        for (i, c) in out.chars().enumerate() {
            let is_leading_feff = i == 0 && c == '\u{FEFF}';
            assert!(
                !crate::no_zero_width_chars::is_flagged_zero_width(c, is_leading_feff),
                "fixer left a flagged char U+{:04X} at {i}",
                c as u32
            );
        }
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

    #[test]
    fn bom_fix_edit_binary_guard_mirrors_apply() {
        // The editor-side `fix_edit` now carries the same `looks_binary` guard
        // as `apply`, so the two fix paths can't diverge on a binary file. In
        // practice the guard is INERT for a real BOM: `content_inspector`
        // classifies any BOM-prefixed content as a text-with-BOM type (never
        // binary), so `looks_binary` is false whenever `detect_bom` is Some —
        // this test pins that invariant. Should content_inspector ever start
        // classifying a BOM+binary payload as binary, this assert flips and
        // signals that both fix paths must be re-examined together.
        let mut bytes = vec![0xEF, 0xBB, 0xBF]; // UTF-8 BOM
        bytes.extend_from_slice(&[0x00, 0x01, 0x02, 0x00, 0xFF, 0x00, 0x03]);
        assert!(
            !looks_binary(&bytes),
            "a BOM-prefixed payload is classified as text-with-BOM, so the guard is inert"
        );
        // With the guard inert, fix_edit strips the BOM exactly as apply would.
        let edit = FileStripBomFixer
            .fix_edit(&v(), &bytes, std::path::Path::new("/r"))
            .expect("a detectable BOM yields an edit");
        let FixEdit::SetContent { content, .. } = edit else {
            panic!("expected SetContent");
        };
        assert_eq!(content, &bytes[3..], "strips only the 3 BOM bytes");
    }
}
