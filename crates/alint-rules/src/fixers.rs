//! Shared [`Fixer`] implementations.
//!
//! Each fixer is a small, rule-agnostic helper: rule builders (e.g.
//! `file_exists`, `file_absent`) decide whether the configured `fix:`
//! op makes sense for their kind and, if so, construct one of the
//! fixers here and attach it to the built rule.

use std::io::Write;
use std::path::PathBuf;

use alint_core::{ContentSourceSpec, Error, FixContext, FixOutcome, Fixer, Result, Violation};

use crate::case::CaseConvention;

/// UTF-8 byte-order mark. Preserved across prepend operations so
/// editors that rely on it don't break.
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// Creates a file with pre-declared content. Target path is set at
/// rule-build time (either explicit `fix.file_create.path` or the
/// rule's first literal `paths:` entry). Content is either inline
/// or read at apply time from a path-relative-to-root.
#[derive(Debug)]
pub struct FileCreateFixer {
    path: PathBuf,
    source: ContentSourceSpec,
    create_parents: bool,
}

impl FileCreateFixer {
    pub fn new(path: PathBuf, source: ContentSourceSpec, create_parents: bool) -> Self {
        Self {
            path,
            source,
            create_parents,
        }
    }
}

impl Fixer for FileCreateFixer {
    fn describe(&self) -> String {
        match &self.source {
            ContentSourceSpec::Inline(s) => format!(
                "create {} ({} byte{})",
                self.path.display(),
                s.len(),
                if s.len() == 1 { "" } else { "s" }
            ),
            ContentSourceSpec::File(rel) => format!(
                "create {} (content from {})",
                self.path.display(),
                rel.display()
            ),
        }
    }

    fn apply(&self, _violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let abs = ctx.root.join(&self.path);
        if abs.exists() {
            return Ok(FixOutcome::Skipped(format!(
                "{} already exists",
                self.path.display()
            )));
        }
        let content = match resolve_source_bytes(&self.source, ctx.root) {
            Ok(bytes) => bytes,
            Err(skip_msg) => return Ok(FixOutcome::Skipped(skip_msg)),
        };
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would create {}",
                self.path.display()
            )));
        }
        if self.create_parents
            && let Some(parent) = abs.parent()
        {
            std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        std::fs::write(&abs, &content).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "created {}",
            self.path.display()
        )))
    }
}

/// Read a `ContentSourceSpec` to bytes. Returns the raw payload
/// for inline content; for file-sourced content, reads the file
/// at apply time, resolving its path relative to `ctx_root`. A
/// missing or unreadable source produces a `Skipped`-friendly
/// `Err(String)` so the caller can degrade gracefully rather
/// than abort the whole fix run.
fn resolve_source_bytes(
    source: &ContentSourceSpec,
    ctx_root: &std::path::Path,
) -> std::result::Result<Vec<u8>, String> {
    match source {
        ContentSourceSpec::Inline(s) => Ok(s.as_bytes().to_vec()),
        ContentSourceSpec::File(rel) => {
            let abs = ctx_root.join(rel);
            std::fs::read(&abs)
                .map_err(|e| format!("content_from `{}` could not be read: {e}", rel.display()))
        }
    }
}

/// Removes the file named by the violation's `path`. Used by
/// `file_absent` to purge committed files that shouldn't be there.
#[derive(Debug)]
pub struct FileRemoveFixer;

impl Fixer for FileRemoveFixer {
    fn describe(&self) -> String {
        "remove the violating file".to_string()
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(path) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let abs = ctx.root.join(path);
        if !abs.exists() {
            return Ok(FixOutcome::Skipped(format!(
                "{} does not exist",
                path.display()
            )));
        }
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would remove {}",
                path.display()
            )));
        }
        std::fs::remove_file(&abs).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!("removed {}", path.display())))
    }
}

/// Prepends `source` content to the start of each violating
/// file. Paired with `file_header` to inject a required header
/// comment / boilerplate.
///
/// If the file starts with a UTF-8 BOM, the prepended bytes go
/// *after* the BOM so editors that rely on it don't break.
#[derive(Debug)]
pub struct FilePrependFixer {
    source: ContentSourceSpec,
}

impl FilePrependFixer {
    pub fn new(source: ContentSourceSpec) -> Self {
        Self { source }
    }
}

impl Fixer for FilePrependFixer {
    fn describe(&self) -> String {
        match &self.source {
            ContentSourceSpec::Inline(s) => format!(
                "prepend {} byte{} to each violating file",
                s.len(),
                if s.len() == 1 { "" } else { "s" }
            ),
            ContentSourceSpec::File(rel) => {
                format!(
                    "prepend content from {} to each violating file",
                    rel.display()
                )
            }
        }
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(path) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let abs = ctx.root.join(path);
        let prepend = match resolve_source_bytes(&self.source, ctx.root) {
            Ok(b) => b,
            Err(skip_msg) => return Ok(FixOutcome::Skipped(skip_msg)),
        };
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would prepend {} byte(s) to {}",
                prepend.len(),
                path.display()
            )));
        }
        let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
            alint_core::ReadForFix::Bytes(b) => b,
            alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        let mut out = Vec::with_capacity(existing.len() + prepend.len());
        if existing.starts_with(UTF8_BOM) {
            out.extend_from_slice(UTF8_BOM);
            out.extend_from_slice(&prepend);
            out.extend_from_slice(&existing[UTF8_BOM.len()..]);
        } else {
            out.extend_from_slice(&prepend);
            out.extend_from_slice(&existing);
        }
        std::fs::write(&abs, &out).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!("prepended {}", path.display())))
    }
}

/// Appends `source` content to the end of each violating file.
/// Paired with `file_content_matches` / `file_footer` when the
/// required content is satisfied by the appended bytes.
#[derive(Debug)]
pub struct FileAppendFixer {
    source: ContentSourceSpec,
}

impl FileAppendFixer {
    pub fn new(source: ContentSourceSpec) -> Self {
        Self { source }
    }
}

impl Fixer for FileAppendFixer {
    fn describe(&self) -> String {
        match &self.source {
            ContentSourceSpec::Inline(s) => format!(
                "append {} byte{} to each violating file",
                s.len(),
                if s.len() == 1 { "" } else { "s" }
            ),
            ContentSourceSpec::File(rel) => {
                format!(
                    "append content from {} to each violating file",
                    rel.display()
                )
            }
        }
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(path) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let abs = ctx.root.join(path);
        let payload = match resolve_source_bytes(&self.source, ctx.root) {
            Ok(b) => b,
            Err(skip_msg) => return Ok(FixOutcome::Skipped(skip_msg)),
        };
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would append {} byte(s) to {}",
                payload.len(),
                path.display()
            )));
        }
        if let Some(skip) = alint_core::check_fix_size(&abs, path, ctx)? {
            return Ok(skip);
        }
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&abs)
            .map_err(|source| Error::Io {
                path: abs.clone(),
                source,
            })?;
        f.write_all(&payload).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "appended to {}",
            path.display()
        )))
    }
}

/// Renames the violating file's stem to a target case convention,
/// preserving the extension and keeping the file in the same parent
/// directory. Paired with `filename_case`.
///
/// Skips with a clear reason when: the violation has no path, the
/// target name equals the current name (already conforming), or a
/// different file already occupies the target name (collision).
#[derive(Debug)]
pub struct FileRenameFixer {
    case: CaseConvention,
}

impl FileRenameFixer {
    pub fn new(case: CaseConvention) -> Self {
        Self { case }
    }
}

impl Fixer for FileRenameFixer {
    fn describe(&self) -> String {
        format!("rename stems to {}", self.case.display_name())
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(path) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            return Ok(FixOutcome::Skipped(format!(
                "cannot decode filename stem for {}",
                path.display()
            )));
        };
        let new_stem = self.case.convert(stem);
        if new_stem == stem {
            return Ok(FixOutcome::Skipped(format!(
                "{} already matches target case",
                path.display()
            )));
        }
        if new_stem.is_empty() {
            return Ok(FixOutcome::Skipped(format!(
                "case conversion produced an empty stem for {}",
                path.display()
            )));
        }

        let mut new_basename = new_stem;
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            new_basename.push('.');
            new_basename.push_str(ext);
        }
        let new_path: PathBuf = match path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.join(&new_basename),
            _ => PathBuf::from(&new_basename),
        };

        let abs_from = ctx.root.join(path);
        let abs_to = ctx.root.join(&new_path);
        if abs_to.exists() {
            return Ok(FixOutcome::Skipped(format!(
                "target {} already exists",
                new_path.display()
            )));
        }
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would rename {} → {}",
                path.display(),
                new_path.display()
            )));
        }
        std::fs::rename(&abs_from, &abs_to).map_err(|source| Error::Io {
            path: abs_from,
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "renamed {} → {}",
            path.display(),
            new_path.display()
        )))
    }
}

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
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would trim trailing whitespace in {}",
                path.display()
            )));
        }
        let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
            alint_core::ReadForFix::Bytes(b) => b,
            alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        let Ok(text) = std::str::from_utf8(&existing) else {
            return Ok(FixOutcome::Skipped(format!(
                "{} is not UTF-8; cannot trim",
                path.display()
            )));
        };
        let trimmed = strip_trailing_whitespace(text);
        if trimmed.as_bytes() == existing {
            return Ok(FixOutcome::Skipped(format!(
                "{} already clean",
                path.display()
            )));
        }
        std::fs::write(&abs, trimmed.as_bytes()).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "trimmed trailing whitespace in {}",
            path.display()
        )))
    }
}

fn strip_trailing_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut first = true;
    for line in text.split('\n') {
        if !first {
            out.push('\n');
        }
        first = false;
        // Preserve CR before the (upcoming) LF so CRLF endings survive.
        let (body, cr) = match line.strip_suffix('\r') {
            Some(stripped) => (stripped, "\r"),
            None => (line, ""),
        };
        out.push_str(body.trim_end_matches([' ', '\t']));
        out.push_str(cr);
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
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would append final newline to {}",
                path.display()
            )));
        }
        if let Some(skip) = alint_core::check_fix_size(&abs, path, ctx)? {
            return Ok(skip);
        }
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&abs)
            .map_err(|source| Error::Io {
                path: abs.clone(),
                source,
            })?;
        f.write_all(b"\n").map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "appended final newline to {}",
            path.display()
        )))
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
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would normalize line endings in {} to {}",
                path.display(),
                self.target.name()
            )));
        }
        let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
            alint_core::ReadForFix::Bytes(b) => b,
            alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        let normalized = normalize_line_endings(&existing, self.target);
        if normalized == existing {
            return Ok(FixOutcome::Skipped(format!(
                "{} already {}",
                path.display(),
                self.target.name()
            )));
        }
        std::fs::write(&abs, &normalized).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "normalized {} to {}",
            path.display(),
            self.target.name()
        )))
    }
}

fn normalize_line_endings(bytes: &[u8], target: LineEndingTarget) -> Vec<u8> {
    let target_bytes = target.bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            // Drop a preceding CR so `\r\n` collapses to `\n` before
            // we emit the target.
            if out.last().copied() == Some(b'\r') {
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
        let Some(bom) = crate::no_bom::detect_bom(&existing) else {
            return Ok(FixOutcome::Skipped(format!(
                "{} has no BOM",
                path.display()
            )));
        };
        let stripped = &existing[bom.byte_len()..];
        std::fs::write(&abs, stripped).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "stripped {} BOM from {}",
            bom.name(),
            path.display()
        )))
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
    let mut out = String::with_capacity(text.len());
    let mut first_char = true;
    for c in text.chars() {
        let keep_because_leading_bom = preserve_leading_feff && first_char && c == '\u{FEFF}';
        if keep_because_leading_bom || !predicate(c) {
            out.push(c);
        }
        first_char = false;
    }
    if out.as_bytes() == existing {
        return Ok(FixOutcome::Skipped(format!(
            "{} has no {label} chars to strip",
            path.display()
        )));
    }
    std::fs::write(&abs, out.as_bytes()).map_err(|source| Error::Io {
        path: abs.clone(),
        source,
    })?;
    Ok(FixOutcome::Applied(format!("{verb} {}", path.display())))
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
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would collapse blank lines in {} to at most {}",
                path.display(),
                self.max,
            )));
        }
        let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
            alint_core::ReadForFix::Bytes(b) => b,
            alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
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
        std::fs::write(&abs, collapsed.as_bytes()).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "collapsed blank-line runs in {} to at most {}",
            path.display(),
            self.max,
        )))
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
        }
    }

    #[test]
    fn file_create_writes_content_when_missing() {
        let tmp = TempDir::new().unwrap();
        let fixer = FileCreateFixer::new(PathBuf::from("LICENSE"), "Apache-2.0\n".into(), true);
        let outcome = fixer
            .apply(&Violation::new("missing LICENSE"), &make_ctx(&tmp, false))
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Applied(_)));
        let written = std::fs::read_to_string(tmp.path().join("LICENSE")).unwrap();
        assert_eq!(written, "Apache-2.0\n");
    }

    #[test]
    fn file_create_reads_content_from_relative_path() {
        // `content_from` relative to ctx.root: stage a template
        // file in the tempdir, point the fixer at it via a
        // relative path, and verify the apply step reads from
        // disk at apply time.
        let tmp = TempDir::new().unwrap();
        let template_dir = tmp.path().join(".alint/templates");
        std::fs::create_dir_all(&template_dir).unwrap();
        std::fs::write(
            template_dir.join("LICENSE-MIT.txt"),
            "MIT License\n\nCopyright (c) 2026 demo\n",
        )
        .unwrap();
        let fixer = FileCreateFixer::new(
            PathBuf::from("LICENSE"),
            ContentSourceSpec::File(PathBuf::from(".alint/templates/LICENSE-MIT.txt")),
            true,
        );
        let outcome = fixer
            .apply(&Violation::new("missing LICENSE"), &make_ctx(&tmp, false))
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Applied(_)));
        let written = std::fs::read_to_string(tmp.path().join("LICENSE")).unwrap();
        assert!(written.starts_with("MIT License"));
        assert!(written.contains("Copyright (c) 2026"));
    }

    #[test]
    fn file_create_skips_when_content_from_missing() {
        // Missing source file produces a `Skipped` outcome
        // rather than aborting the whole fix run — same posture
        // as the rest of the fixer module.
        let tmp = TempDir::new().unwrap();
        let fixer = FileCreateFixer::new(
            PathBuf::from("LICENSE"),
            ContentSourceSpec::File(PathBuf::from("does/not/exist.txt")),
            true,
        );
        let outcome = fixer
            .apply(&Violation::new("missing"), &make_ctx(&tmp, false))
            .unwrap();
        let FixOutcome::Skipped(msg) = &outcome else {
            panic!("expected Skipped, got {outcome:?}")
        };
        assert!(msg.contains("could not be read"));
        // The target file should NOT have been created since
        // we skipped before the write.
        assert!(!tmp.path().join("LICENSE").exists());
    }

    #[test]
    fn file_prepend_with_content_from_reads_at_apply() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("hdr.txt"),
            "// SPDX-License-Identifier: MIT\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("a.rs"), "fn main() {}\n").unwrap();
        let fixer = FilePrependFixer::new(ContentSourceSpec::File(PathBuf::from("hdr.txt")));
        let outcome = fixer
            .apply(
                &Violation::new("missing header").with_path(PathBuf::from("a.rs")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Applied(_)));
        let updated = std::fs::read_to_string(tmp.path().join("a.rs")).unwrap();
        assert!(updated.starts_with("// SPDX-License-Identifier: MIT\n"));
        assert!(updated.contains("fn main() {}"));
    }

    #[test]
    fn file_create_creates_intermediate_directories() {
        let tmp = TempDir::new().unwrap();
        let fixer = FileCreateFixer::new(PathBuf::from("a/b/c/config.yaml"), "k: v\n".into(), true);
        fixer
            .apply(&Violation::new("missing"), &make_ctx(&tmp, false))
            .unwrap();
        assert!(tmp.path().join("a/b/c/config.yaml").exists());
    }

    #[test]
    fn file_create_skips_when_target_exists() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("README.md"), "existing\n").unwrap();
        let fixer = FileCreateFixer::new(PathBuf::from("README.md"), "NEW\n".into(), true);
        let outcome = fixer
            .apply(&Violation::new("x"), &make_ctx(&tmp, false))
            .unwrap();
        match outcome {
            FixOutcome::Skipped(reason) => assert!(reason.contains("already exists")),
            FixOutcome::Applied(_) => panic!("expected Skipped"),
        }
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("README.md")).unwrap(),
            "existing\n",
            "pre-existing content must not be overwritten"
        );
    }

    #[test]
    fn file_create_dry_run_does_not_touch_disk() {
        let tmp = TempDir::new().unwrap();
        let fixer = FileCreateFixer::new(PathBuf::from("x.txt"), "body".into(), true);
        let outcome = fixer
            .apply(&Violation::new("x"), &make_ctx(&tmp, true))
            .unwrap();
        match outcome {
            FixOutcome::Applied(s) => assert!(s.starts_with("would create")),
            FixOutcome::Skipped(_) => panic!("expected Applied"),
        }
        assert!(!tmp.path().join("x.txt").exists());
    }

    #[test]
    fn file_remove_deletes_violating_path() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("debug.log");
        std::fs::write(&target, "noise").unwrap();
        let outcome = FileRemoveFixer
            .apply(
                &Violation::new("forbidden").with_path("debug.log"),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Applied(_)));
        assert!(!target.exists());
    }

    #[test]
    fn file_remove_skips_when_violation_has_no_path() {
        let tmp = TempDir::new().unwrap();
        let outcome = FileRemoveFixer
            .apply(&Violation::new("no path"), &make_ctx(&tmp, false))
            .unwrap();
        match outcome {
            FixOutcome::Skipped(reason) => assert!(reason.contains("path")),
            FixOutcome::Applied(_) => panic!("expected Skipped"),
        }
    }

    #[test]
    fn file_remove_dry_run_keeps_the_file() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("victim.bak");
        std::fs::write(&target, "bytes").unwrap();
        let outcome = FileRemoveFixer
            .apply(
                &Violation::new("forbidden").with_path("victim.bak"),
                &make_ctx(&tmp, true),
            )
            .unwrap();
        match outcome {
            FixOutcome::Applied(s) => assert!(s.starts_with("would remove")),
            FixOutcome::Skipped(_) => panic!("expected Applied"),
        }
        assert!(target.exists());
    }

    #[test]
    fn file_prepend_inserts_at_start() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "fn main() {}\n").unwrap();
        let fixer = FilePrependFixer::new("// Copyright 2026\n".into());
        fixer
            .apply(
                &Violation::new("missing header").with_path("a.rs"),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.rs")).unwrap(),
            "// Copyright 2026\nfn main() {}\n"
        );
    }

    #[test]
    fn file_prepend_preserves_utf8_bom() {
        let tmp = TempDir::new().unwrap();
        // BOM + "hello\n"
        let mut bytes = b"\xEF\xBB\xBF".to_vec();
        bytes.extend_from_slice(b"hello\n");
        std::fs::write(tmp.path().join("x.txt"), &bytes).unwrap();
        let fixer = FilePrependFixer::new("HEAD\n".into());
        fixer
            .apply(
                &Violation::new("m").with_path("x.txt"),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        let got = std::fs::read(tmp.path().join("x.txt")).unwrap();
        assert_eq!(&got[..3], b"\xEF\xBB\xBF");
        assert_eq!(&got[3..], b"HEAD\nhello\n");
    }

    #[test]
    fn file_prepend_dry_run_does_not_touch_disk() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "original\n").unwrap();
        FilePrependFixer::new("HEAD\n".into())
            .apply(
                &Violation::new("m").with_path("a.rs"),
                &make_ctx(&tmp, true),
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.rs")).unwrap(),
            "original\n"
        );
    }

    #[test]
    fn file_prepend_skips_when_violation_has_no_path() {
        let tmp = TempDir::new().unwrap();
        let outcome = FilePrependFixer::new("h".into())
            .apply(&Violation::new("m"), &make_ctx(&tmp, false))
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Skipped(_)));
    }

    #[test]
    fn file_append_writes_at_end() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("notes.md"), "# Notes\n").unwrap();
        let fixer = FileAppendFixer::new("\n## Section\n".into());
        fixer
            .apply(
                &Violation::new("missing section").with_path("notes.md"),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("notes.md")).unwrap(),
            "# Notes\n\n## Section\n"
        );
    }

    #[test]
    fn file_append_dry_run_leaves_file_unchanged() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("x.txt"), "orig\n").unwrap();
        FileAppendFixer::new("extra\n".into())
            .apply(
                &Violation::new("m").with_path("x.txt"),
                &make_ctx(&tmp, true),
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("x.txt")).unwrap(),
            "orig\n"
        );
    }

    #[test]
    fn file_append_skips_when_violation_has_no_path() {
        let tmp = TempDir::new().unwrap();
        let outcome = FileAppendFixer::new("x".into())
            .apply(&Violation::new("m"), &make_ctx(&tmp, false))
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Skipped(_)));
    }

    #[test]
    fn file_rename_converts_stem_preserving_extension() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("FooBar.rs"), "fn main() {}\n").unwrap();
        FileRenameFixer::new(CaseConvention::Snake)
            .apply(
                &Violation::new("case").with_path("FooBar.rs"),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert!(tmp.path().join("foo_bar.rs").exists());
        assert!(!tmp.path().join("FooBar.rs").exists());
    }

    #[test]
    fn file_rename_keeps_file_in_same_directory() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/MyModule.rs"), "").unwrap();
        FileRenameFixer::new(CaseConvention::Snake)
            .apply(
                &Violation::new("case").with_path("src/MyModule.rs"),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert!(tmp.path().join("src/my_module.rs").exists());
    }

    #[test]
    fn file_rename_skips_when_already_in_target_case() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("foo_bar.rs"), "").unwrap();
        let outcome = FileRenameFixer::new(CaseConvention::Snake)
            .apply(
                &Violation::new("case").with_path("foo_bar.rs"),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        match outcome {
            FixOutcome::Skipped(reason) => assert!(reason.contains("already")),
            FixOutcome::Applied(_) => panic!("expected Skipped"),
        }
    }

    #[test]
    fn file_rename_skips_on_target_collision() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("FooBar.rs"), "A").unwrap();
        std::fs::write(tmp.path().join("foo_bar.rs"), "B").unwrap();
        let outcome = FileRenameFixer::new(CaseConvention::Snake)
            .apply(
                &Violation::new("case").with_path("FooBar.rs"),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        match outcome {
            FixOutcome::Skipped(reason) => assert!(reason.contains("already exists")),
            FixOutcome::Applied(_) => panic!("expected Skipped"),
        }
        // Neither file should have been touched.
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("FooBar.rs")).unwrap(),
            "A"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("foo_bar.rs")).unwrap(),
            "B"
        );
    }

    #[test]
    fn file_rename_dry_run_does_not_touch_disk() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("FooBar.rs"), "").unwrap();
        FileRenameFixer::new(CaseConvention::Snake)
            .apply(
                &Violation::new("case").with_path("FooBar.rs"),
                &make_ctx(&tmp, true),
            )
            .unwrap();
        assert!(tmp.path().join("FooBar.rs").exists());
        assert!(!tmp.path().join("foo_bar.rs").exists());
    }

    // ── text-hygiene fixers ────────────────────────────────────

    #[test]
    fn strip_trailing_whitespace_preserves_lf_and_crlf() {
        assert_eq!(strip_trailing_whitespace("a  \nb\t\n"), "a\nb\n");
        assert_eq!(strip_trailing_whitespace("a  \r\nb\t\r\n"), "a\r\nb\r\n");
    }

    #[test]
    fn file_trim_trailing_whitespace_rewrites_in_place() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("x.rs"), "let _ = 1;   \n").unwrap();
        let outcome = FileTrimTrailingWhitespaceFixer
            .apply(
                &Violation::new("ws").with_path("x.rs"),
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
        };
        let outcome = FileTrimTrailingWhitespaceFixer
            .apply(&Violation::new("ws").with_path("big.txt"), &ctx)
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
                &Violation::new("eof").with_path("x.txt"),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("x.txt")).unwrap(),
            "hello\n"
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
    fn file_normalize_line_endings_rewrites_to_lf() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.md"), "one\r\ntwo\r\n").unwrap();
        FileNormalizeLineEndingsFixer::new(LineEndingTarget::Lf)
            .apply(
                &Violation::new("le").with_path("a.md"),
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
}
