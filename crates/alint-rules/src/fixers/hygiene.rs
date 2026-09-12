use std::path::Path;

use alint_core::{Error, FixContext, FixEdit, FixOutcome, Fixer, Result, Violation};

use crate::io::looks_binary;

/// Strips trailing space/tab on every line of each violating
/// file. Preserves original line endings (LF stays LF, CRLF
/// stays CRLF).
#[derive(Debug)]
pub struct FileTrimTrailingWhitespaceFixer;

impl Fixer for FileTrimTrailingWhitespaceFixer {
    fn describe(&self) -> String {
        "strip trailing whitespace on every line".to_string()
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(path) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let abs = ctx.root.join(path);
        let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
            alint_core::ReadForFix::Bytes(b) => b,
            alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        // A NUL byte marks binary content: refuse (H3), and the detector carries
        // the SAME guard so `check` and `fix` agree on which files are in scope.
        if looks_binary(&existing) {
            return Ok(FixOutcome::Skipped(format!(
                "{} looks binary; not trimming",
                path.display()
            )));
        }
        // No `from_utf8` gate: `no_trailing_whitespace` detects at the byte level
        // (a file with one junk byte is still flagged), so the fixer trims at the
        // byte level too and preserves any invalid bytes -- otherwise a file with
        // one junk byte would be flagged-fixable forever yet never fixed.
        let trimmed = strip_trailing_whitespace(&existing);
        if trimmed == existing {
            return Ok(FixOutcome::Skipped(format!(
                "{} already clean",
                path.display()
            )));
        }
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would trim trailing whitespace in {}",
                path.display()
            )));
        }
        ctx.commit_write(&abs, &trimmed)
            .map_err(|source| Error::Io {
                path: abs.clone(),
                source,
            })?;
        Ok(FixOutcome::Applied(format!(
            "trimmed trailing whitespace in {}",
            path.display()
        )))
    }

    fn fix_edit(&self, violation: &Violation, bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        let path = violation.path.as_deref()?;
        if looks_binary(bytes) {
            return None;
        }
        let trimmed = strip_trailing_whitespace(bytes);
        if trimmed == bytes {
            return None;
        }
        Some(FixEdit::SetContent {
            path: path.to_path_buf(),
            content: trimmed,
        })
    }
}

/// Trim trailing space/tab (and CR from a CRLF run) on every line, at the BYTE
/// level so it matches its byte-level detector and preserves any invalid UTF-8
/// bytes. Space/tab/CR/LF are all ASCII and can never be a UTF-8 continuation
/// byte, so trimming them from the raw bytes is unambiguous.
fn strip_trailing_whitespace(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut first = true;
    for line in bytes.split(|&b| b == b'\n') {
        if !first {
            out.push(b'\n');
        }
        first = false;
        // Trim trailing space/tab AND CR from the ws region, then re-add ONE CR
        // iff the line ended in CR, to preserve a CRLF ending. Trimming the CR
        // matters for idempotence: the rule strips a trailing CR before its ws
        // check, so a line like `"x \r "` (ws, CR, ws) that trims only to
        // `"x \r"` would be re-flagged (strip CR -> `"x "` -> trailing ws). Also
        // collapses a doubled trailing CR (`\r\r`) like the line-ending fixer.
        let had_cr = line.last() == Some(&b'\r');
        let end = line
            .iter()
            .rposition(|&b| b != b' ' && b != b'\t' && b != b'\r')
            .map_or(0, |i| i + 1);
        out.extend_from_slice(&line[..end]);
        if had_cr {
            out.push(b'\r');
        }
    }
    out
}

/// Appends a single `\n` byte when a file has content but
/// doesn't end with one.
#[derive(Debug)]
pub struct FileAppendFinalNewlineFixer;

impl Fixer for FileAppendFinalNewlineFixer {
    fn describe(&self) -> String {
        "append final newline when missing".to_string()
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(path) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let abs = ctx.root.join(path);
        let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
            alint_core::ReadForFix::Bytes(b) => b,
            alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        // Match `fix_edit`: nothing to do for an empty or already-terminated
        // file (the rule shouldn't flag these, but stay consistent and
        // idempotent), and never append to a binary.
        if existing.is_empty() || existing.ends_with(b"\n") {
            return Ok(FixOutcome::Skipped(format!(
                "{} already ends with a newline",
                path.display()
            )));
        }
        if looks_binary(&existing) {
            return Ok(FixOutcome::Skipped(format!(
                "{} looks binary; not appending a newline",
                path.display()
            )));
        }
        // Dry-run AFTER the read + guards, so a preview matches the real run.
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would append final newline to {}",
                path.display()
            )));
        }
        let mut out = existing;
        out.push(b'\n');
        ctx.commit_write(&abs, &out).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "appended final newline to {}",
            path.display()
        )))
    }

    fn fix_edit(&self, violation: &Violation, bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        let path = violation.path.as_deref()?;
        if bytes.is_empty() || bytes.ends_with(b"\n") {
            return None;
        }
        // Mirror apply()'s binary guard on the editor (LSP) path too: appending a
        // newline to a binary file corrupts it.
        if looks_binary(bytes) {
            return None;
        }
        let mut content = bytes.to_vec();
        content.push(b'\n');
        Some(FixEdit::SetContent {
            path: path.to_path_buf(),
            content,
        })
    }
}

/// Which line ending [`FileNormalizeLineEndingsFixer`] rewrites to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEndingTarget {
    Lf,
    Crlf,
}

impl LineEndingTarget {
    pub fn name(self) -> &'static str {
        match self {
            Self::Lf => "lf",
            Self::Crlf => "crlf",
        }
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            Self::Lf => b"\n",
            Self::Crlf => b"\r\n",
        }
    }
}

/// Rewrites every line ending in a file to the target (`lf` or `crlf`).
#[derive(Debug)]
pub struct FileNormalizeLineEndingsFixer {
    target: LineEndingTarget,
}

impl FileNormalizeLineEndingsFixer {
    pub fn new(target: LineEndingTarget) -> Self {
        Self { target }
    }
}

impl Fixer for FileNormalizeLineEndingsFixer {
    fn describe(&self) -> String {
        format!("normalize line endings to {}", self.target.name())
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(path) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let abs = ctx.root.join(path);
        let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
            alint_core::ReadForFix::Bytes(b) => b,
            alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        if looks_binary(&existing) {
            return Ok(FixOutcome::Skipped(format!(
                "{} looks binary; not rewriting line endings",
                path.display()
            )));
        }
        let normalized = normalize_line_endings(&existing, self.target);
        if normalized == existing {
            return Ok(FixOutcome::Skipped(format!(
                "{} already {}",
                path.display(),
                self.target.name()
            )));
        }
        // Dry-run AFTER the read + guards, so a preview matches the real run.
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would normalize line endings in {} to {}",
                path.display(),
                self.target.name()
            )));
        }
        ctx.commit_write(&abs, &normalized)
            .map_err(|source| Error::Io {
                path: abs.clone(),
                source,
            })?;
        Ok(FixOutcome::Applied(format!(
            "normalized {} to {}",
            path.display(),
            self.target.name()
        )))
    }

    fn fix_edit(&self, violation: &Violation, bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        let path = violation.path.as_deref()?;
        // Mirror apply()'s binary guard on the editor (LSP) path: rewriting line
        // endings in a binary file inserts stray CR/LF and corrupts it.
        if looks_binary(bytes) {
            return None;
        }
        let normalized = normalize_line_endings(bytes, self.target);
        if normalized == bytes {
            return None;
        }
        Some(FixEdit::SetContent {
            path: path.to_path_buf(),
            content: normalized,
        })
    }
}

fn normalize_line_endings(bytes: &[u8], target: LineEndingTarget) -> Vec<u8> {
    let target_bytes = target.bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            // Drop ALL preceding CRs so `\r\n` -- and a malformed `\r\r\n`
            // (doubled CR) -- collapse before we emit the target. Popping only
            // one CR would leave `\r\n` for `\r\r\n`, which the rule re-flags, so
            // the fix never converged (a second `fix` kept re-applying).
            while out.last().copied() == Some(b'\r') {
                out.pop();
            }
            out.extend_from_slice(target_bytes);
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    out
}

/// Collapses runs of blank lines longer than `max` down to exactly
/// `max` blank lines. A blank line is one whose content between
/// line endings is empty or only spaces/tabs. Preserves the file's
/// line endings (LF vs. CRLF) by operating on byte-level newlines.
#[derive(Debug)]
pub struct FileCollapseBlankLinesFixer {
    max: u32,
}

impl FileCollapseBlankLinesFixer {
    pub fn new(max: u32) -> Self {
        Self { max }
    }
}

impl Fixer for FileCollapseBlankLinesFixer {
    fn describe(&self) -> String {
        format!("collapse runs of blank lines to at most {}", self.max)
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(path) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let abs = ctx.root.join(path);
        let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
            alint_core::ReadForFix::Bytes(b) => b,
            alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        // A NUL byte marks binary content: guard (H3), and the detector carries
        // the SAME guard so `check` and `fix` agree on scope.
        if looks_binary(&existing) {
            return Ok(FixOutcome::Skipped(format!(
                "{} looks binary; not collapsing blank lines",
                path.display()
            )));
        }
        let Ok(text) = std::str::from_utf8(&existing) else {
            return Ok(FixOutcome::Skipped(format!(
                "{} is not UTF-8; cannot collapse",
                path.display()
            )));
        };
        let collapsed = collapse_blank_lines(text, self.max);
        if collapsed.as_bytes() == existing {
            return Ok(FixOutcome::Skipped(format!(
                "{} already clean",
                path.display()
            )));
        }
        // Dry-run AFTER the read + guards, so a preview matches the real run.
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would collapse blank lines in {} to at most {}",
                path.display(),
                self.max,
            )));
        }
        ctx.commit_write(&abs, collapsed.as_bytes())
            .map_err(|source| Error::Io {
                path: abs.clone(),
                source,
            })?;
        Ok(FixOutcome::Applied(format!(
            "collapsed blank-line runs in {} to at most {}",
            path.display(),
            self.max,
        )))
    }

    fn fix_edit(&self, violation: &Violation, bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        let path = violation.path.as_deref()?;
        if looks_binary(bytes) {
            return None;
        }
        let text = std::str::from_utf8(bytes).ok()?;
        let collapsed = collapse_blank_lines(text, self.max);
        if collapsed.as_bytes() == bytes {
            return None;
        }
        Some(FixEdit::SetContent {
            path: path.to_path_buf(),
            content: collapsed.into_bytes(),
        })
    }
}

/// A "blank" line has content consisting only of spaces or tabs.
pub(crate) fn line_is_blank(body: &str) -> bool {
    body.bytes().all(|b| b == b' ' || b == b'\t')
}

/// Walk the file in (body, ending) pairs so the final slot after the
/// last newline doesn't get double-counted as an extra blank line.
/// Preserves CRLF vs LF verbatim.
pub(crate) fn collapse_blank_lines(text: &str, max: u32) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank_run: u32 = 0;
    let mut remaining = text;
    loop {
        let (body, ending, rest) = match remaining.find('\n') {
            Some(i) => {
                let before = &remaining[..i];
                let (body, cr) = match before.strip_suffix('\r') {
                    Some(s) => (s, "\r\n"),
                    None => (before, "\n"),
                };
                (body, cr, &remaining[i + 1..])
            }
            None => (remaining, "", ""),
        };
        let blank = line_is_blank(body);
        if blank {
            blank_run += 1;
            if blank_run > max {
                if ending.is_empty() {
                    break;
                }
                remaining = rest;
                continue;
            }
        } else {
            blank_run = 0;
        }
        out.push_str(body);
        out.push_str(ending);
        if ending.is_empty() {
            break;
        }
        remaining = rest;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_ctx(tmp: &TempDir, dry_run: bool) -> FixContext<'_> {
        FixContext {
            root: tmp.path(),
            dry_run,
            fix_size_limit: None,
            allow_out_of_root: false,
            compose: None,
            stage_ops: None,
        }
    }

    #[test]
    fn strip_trailing_whitespace_preserves_lf_and_crlf() {
        assert_eq!(strip_trailing_whitespace(b"a  \nb\t\n"), b"a\nb\n");
        assert_eq!(strip_trailing_whitespace(b"a  \r\nb\t\r\n"), b"a\r\nb\r\n");
    }

    #[test]
    fn strip_trailing_whitespace_converges_with_interior_cr() {
        // Regression (round-3 audit F2): a CR sitting *inside* the trailing-ws
        // run (`space CR space`) must clean to a genuine fixed point in one pass.
        // `no_trailing_whitespace` strips a trailing CR before its ws test, so
        // trimming only to `"x \r"` leaves `"x "` under the check and re-flags --
        // `fix` would never converge.
        assert_eq!(strip_trailing_whitespace(b"x \r \n"), b"x\n");
        // The original repro: a bare line with no final newline.
        assert_eq!(strip_trailing_whitespace(b"x \r "), b"x");
        // Fixed point: a second application is a no-op.
        let once = strip_trailing_whitespace(b"x \r \n");
        assert_eq!(strip_trailing_whitespace(&once), once);
    }

    #[test]
    fn strip_trailing_whitespace_is_byte_level_and_preserves_invalid_utf8() {
        // Round-4 audit F2: `no_trailing_whitespace` detects at the byte level, so
        // the fixer must trim a file that carries an invalid UTF-8 byte rather
        // than skip it (which left the violation flagged-fixable forever). The
        // junk `0xFF` survives; only the trailing spaces go.
        assert_eq!(strip_trailing_whitespace(b"a\xFF  \n"), b"a\xFF\n".to_vec());
        // A trailing junk byte is not whitespace and is preserved.
        assert_eq!(strip_trailing_whitespace(b"a \xFF"), b"a \xFF".to_vec());
    }

    #[test]
    fn file_trim_trailing_whitespace_rewrites_in_place() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("x.rs"), "let _ = 1;   \n").unwrap();
        let outcome = FileTrimTrailingWhitespaceFixer
            .apply(
                &Violation::new("ws").with_path(std::path::Path::new("x.rs")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Applied(_)));
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("x.rs")).unwrap(),
            "let _ = 1;\n"
        );
    }

    #[test]
    fn file_trim_trailing_whitespace_honors_size_limit() {
        let tmp = TempDir::new().unwrap();
        let big = "x   \n".repeat(2_000);
        std::fs::write(tmp.path().join("big.txt"), &big).unwrap();
        let ctx = FixContext {
            root: tmp.path(),
            dry_run: false,
            fix_size_limit: Some(100),
            allow_out_of_root: false,
            compose: None,
            stage_ops: None,
        };
        let outcome = FileTrimTrailingWhitespaceFixer
            .apply(
                &Violation::new("ws").with_path(std::path::Path::new("big.txt")),
                &ctx,
            )
            .unwrap();
        match outcome {
            FixOutcome::Skipped(reason) => {
                assert!(reason.contains("fix_size_limit"), "{reason}");
            }
            FixOutcome::Applied(_) => panic!("expected Skipped on oversized file"),
        }
        // Disk unchanged.
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("big.txt")).unwrap(),
            big
        );
    }

    #[test]
    fn file_append_final_newline_adds_missing_newline() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("x.txt"), "hello").unwrap();
        FileAppendFinalNewlineFixer
            .apply(
                &Violation::new("eof").with_path(std::path::Path::new("x.txt")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("x.txt")).unwrap(),
            "hello\n"
        );
    }

    fn assert_skips_binary(fixer: &dyn Fixer, binary: &[u8]) {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("blob"), binary).unwrap();
        let violation = Violation::new("x").with_path(std::path::Path::new("blob"));
        // The `alint fix` path: apply() must skip and leave the file byte-identical.
        let outcome = fixer.apply(&violation, &make_ctx(&tmp, false)).unwrap();
        assert!(
            matches!(outcome, FixOutcome::Skipped(_)),
            "a byte-level fixer's apply() must skip a binary file, got {outcome:?}"
        );
        assert_eq!(
            std::fs::read(tmp.path().join("blob")).unwrap(),
            binary,
            "a binary file must be byte-identical after the fixer skips it"
        );
        // The editor (LSP) path: fix_edit() must decline (None) on a binary too,
        // or a code action would hand the editor a corrupting WorkspaceEdit.
        assert!(
            fixer.fix_edit(&violation, binary, tmp.path()).is_none(),
            "a byte-level fixer's fix_edit() must return None for a binary file"
        );
    }

    #[test]
    fn byte_level_fixers_skip_binary_files() {
        // H3 regression + Phase-0 audit. A NUL byte is valid UTF-8 (U+0000), so a
        // `from_utf8` check alone is too weak: EVERY byte-level fixer must consult
        // `looks_binary` and refuse a NUL-bearing file caught by a `paths: "**"`
        // glob. This one fixture would otherwise be edited by all four fixers -
        // it has trailing whitespace (trim), a 3-blank-line run (collapse), LF
        // line endings (normalize), and no final newline (append) - yet each must
        // skip it and leave it byte-identical.
        let binary: &[u8] = b"a \n\x00\n\n\nb";
        assert!(
            std::str::from_utf8(binary).is_ok(),
            "the fixture must be valid UTF-8 so it exercises looks_binary, not from_utf8"
        );
        assert_skips_binary(&FileTrimTrailingWhitespaceFixer, binary);
        assert_skips_binary(&FileCollapseBlankLinesFixer::new(1), binary);
        assert_skips_binary(
            &FileNormalizeLineEndingsFixer::new(LineEndingTarget::Crlf),
            binary,
        );
        assert_skips_binary(&FileAppendFinalNewlineFixer, binary);
        // An invalid-UTF-8 binary (the weaker case) must skip too.
        assert_skips_binary(
            &FileNormalizeLineEndingsFixer::new(LineEndingTarget::Crlf),
            b"\x00\x01\x02\nPNGish\x00\xff\n\x00",
        );
    }

    #[test]
    fn normalize_line_endings_lf_target() {
        let mixed = b"a\r\nb\nc\r\nd".to_vec();
        let out = normalize_line_endings(&mixed, LineEndingTarget::Lf);
        assert_eq!(out, b"a\nb\nc\nd");
    }

    #[test]
    fn normalize_line_endings_crlf_target() {
        let mixed = b"a\r\nb\nc\r\nd".to_vec();
        let out = normalize_line_endings(&mixed, LineEndingTarget::Crlf);
        assert_eq!(out, b"a\r\nb\r\nc\r\nd");
    }

    #[test]
    fn normalize_line_endings_converges_on_doubled_cr() {
        // Regression (round-3 audit F1): a malformed `\r\r\n` (doubled CR before
        // LF) must collapse to the target in ONE pass. Popping only one preceding
        // CR would leave `\r\n` under an LF target, which `line_endings` re-flags,
        // so a second `fix` kept applying (never a fixed point).
        let once = normalize_line_endings(b"x\r\r\ny\r\n", LineEndingTarget::Lf);
        assert_eq!(once, b"x\ny\n");
        // Fixed point: re-normalizing the output changes nothing.
        assert_eq!(normalize_line_endings(&once, LineEndingTarget::Lf), once);
        // A doubled CR under a CRLF target collapses to a single CRLF too.
        assert_eq!(
            normalize_line_endings(b"x\r\r\n", LineEndingTarget::Crlf),
            b"x\r\n"
        );
    }

    #[test]
    fn file_normalize_line_endings_rewrites_to_lf() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.md"), "one\r\ntwo\r\n").unwrap();
        FileNormalizeLineEndingsFixer::new(LineEndingTarget::Lf)
            .apply(
                &Violation::new("le").with_path(std::path::Path::new("a.md")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.md")).unwrap(),
            "one\ntwo\n"
        );
    }

    #[test]
    fn collapse_blank_lines_keeps_up_to_max() {
        assert_eq!(collapse_blank_lines("a\n\n\nb\n", 1), "a\n\nb\n");
        assert_eq!(collapse_blank_lines("a\n\n\n\nb\n", 2), "a\n\n\nb\n");
        assert_eq!(collapse_blank_lines("a\nb\n", 1), "a\nb\n");
    }

    #[test]
    fn collapse_blank_lines_preserves_trailing_newline() {
        // One existing blank line, max=1 → file must still end with "\n\n"
        // (i.e. the blank line plus the EOF newline).
        assert_eq!(collapse_blank_lines("a\n\n", 1), "a\n\n");
    }

    #[test]
    fn collapse_blank_lines_max_zero_drops_all_blanks() {
        assert_eq!(collapse_blank_lines("a\n\n\nb\n", 0), "a\nb\n");
        assert_eq!(collapse_blank_lines("\n", 0), "");
        assert_eq!(collapse_blank_lines("a\n\n", 0), "a\n");
    }

    #[test]
    fn collapse_blank_lines_preserves_crlf() {
        assert_eq!(
            collapse_blank_lines("a\r\n\r\n\r\n\r\nb\r\n", 1),
            "a\r\n\r\nb\r\n"
        );
    }

    #[test]
    fn collapse_blank_lines_treats_whitespace_only_as_blank() {
        // Lines with only spaces/tabs count as blank, and dropped
        // copies disappear entirely (their whitespace goes too).
        assert_eq!(collapse_blank_lines("a\n  \n\t\n\nb\n", 1), "a\n  \nb\n");
    }

    #[test]
    fn collapse_blank_lines_no_op_on_empty_file() {
        assert_eq!(collapse_blank_lines("", 2), "");
    }

    #[test]
    fn trim_fix_edit_returns_set_content_with_trimmed_bytes() {
        let v = Violation::new("ws").with_path(std::path::Path::new("x.rs"));
        let edit = FileTrimTrailingWhitespaceFixer
            .fix_edit(&v, b"let _ = 1;   \n", std::path::Path::new("/repo"))
            .expect("dirty file yields an edit");
        match edit {
            FixEdit::SetContent { path, content } => {
                assert_eq!(path, std::path::Path::new("x.rs"));
                assert_eq!(content, b"let _ = 1;\n");
            }
            other => panic!("expected SetContent, got {other:?}"),
        }
    }

    #[test]
    fn trim_fix_edit_returns_none_when_already_clean() {
        let v = Violation::new("ws").with_path(std::path::Path::new("x.rs"));
        assert!(
            FileTrimTrailingWhitespaceFixer
                .fix_edit(&v, b"clean\n", std::path::Path::new("/repo"))
                .is_none()
        );
    }

    #[test]
    fn append_final_newline_fix_edit_appends_one_newline() {
        let v = Violation::new("eof").with_path(std::path::Path::new("x.txt"));
        let edit = FileAppendFinalNewlineFixer
            .fix_edit(&v, b"hello", std::path::Path::new("/repo"))
            .unwrap();
        assert_eq!(
            edit,
            FixEdit::SetContent {
                path: std::path::PathBuf::from("x.txt"),
                content: b"hello\n".to_vec(),
            }
        );
    }

    #[test]
    fn append_final_newline_fix_edit_none_when_already_terminated() {
        let v = Violation::new("eof").with_path(std::path::Path::new("x.txt"));
        assert!(
            FileAppendFinalNewlineFixer
                .fix_edit(&v, b"ends\n", std::path::Path::new("/repo"))
                .is_none()
        );
    }

    #[test]
    fn normalize_line_endings_fix_edit_rewrites_to_target() {
        let v = Violation::new("le").with_path(std::path::Path::new("a.md"));
        let edit = FileNormalizeLineEndingsFixer::new(LineEndingTarget::Lf)
            .fix_edit(&v, b"a\r\nb\r\n", std::path::Path::new("/repo"))
            .unwrap();
        assert_eq!(
            edit,
            FixEdit::SetContent {
                path: std::path::PathBuf::from("a.md"),
                content: b"a\nb\n".to_vec(),
            }
        );
    }

    #[test]
    fn normalize_line_endings_fix_edit_none_when_already_target() {
        let v = Violation::new("le").with_path(std::path::Path::new("a.md"));
        assert!(
            FileNormalizeLineEndingsFixer::new(LineEndingTarget::Lf)
                .fix_edit(&v, b"a\nb\n", std::path::Path::new("/repo"))
                .is_none()
        );
    }

    #[test]
    fn collapse_blank_lines_fix_edit_collapses_runs() {
        let v = Violation::new("blanks").with_path(std::path::Path::new("a.txt"));
        let edit = FileCollapseBlankLinesFixer::new(1)
            .fix_edit(&v, b"a\n\n\nb\n", std::path::Path::new("/repo"))
            .unwrap();
        assert_eq!(
            edit,
            FixEdit::SetContent {
                path: std::path::PathBuf::from("a.txt"),
                content: b"a\n\nb\n".to_vec(),
            }
        );
    }
}
