use std::path::{Path, PathBuf};

use alint_core::{
    Applicability, ContentSourceSpec, Error, FixContext, FixEdit, FixOutcome, Fixer, Result,
    Violation,
};

use crate::io::looks_binary;

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
    applicability: Applicability,
}

impl FileCreateFixer {
    pub fn new(path: PathBuf, source: ContentSourceSpec, create_parents: bool) -> Self {
        Self {
            path,
            source,
            create_parents,
            // Behavior-preserving by default; the rule builder demotes a remote
            // `extends:`'d create to a suggestion via `with_applicability` (W2).
            applicability: Applicability::Safe,
        }
    }

    /// Override the fix tier. W2 content-fixer trust demotes a `file_create` from
    /// an untrusted remote `extends:` to [`Applicability::Suggestion`]. Defaults to
    /// `Safe`.
    #[must_use]
    pub fn with_applicability(mut self, applicability: Applicability) -> Self {
        self.applicability = applicability;
        self
    }
}

impl Fixer for FileCreateFixer {
    fn applicability(&self) -> Applicability {
        self.applicability
    }

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
        // Confine the config-declared write target to the repo root (honoring
        // the owning rule's `allow_out_of_root`), so a `file_create.path` like
        // `../../x` from an untrusted `extends:`'d ruleset can't write outside
        // the tree on `alint fix`. Refuse (skip loudly) when it escapes.
        let abs = match confine_fix_path(&self.path, ctx.root, ctx.allow_out_of_root) {
            Ok(p) => p,
            Err(reason) => return Ok(FixOutcome::Skipped(reason)),
        };
        if abs.exists() {
            // A DIRECTORY at the target is not the required file: `file_exists`
            // counts only files, so it keeps flagging while the fixer used to
            // report the false "already exists" and never converge. Report the
            // real blocker honestly.
            if abs.is_dir() {
                return Ok(FixOutcome::Skipped(format!(
                    "{} is a directory; cannot create a file there",
                    self.path.display()
                )));
            }
            return Ok(FixOutcome::Skipped(format!(
                "{} already exists",
                self.path.display()
            )));
        }
        // Defense in depth: never create-write THROUGH a symlink at the target.
        // `abs.exists()` follows the link, so an existing target is caught above;
        // a BROKEN symlink (target absent) is where it matters - it slips past
        // the lexical/canonicalize confinement (a non-existent target
        // canonicalizes to nothing, so a planted `evil -> /outside` link reads
        // as in-root) and `fs::write` would then follow it out of the tree. A
        // create should only ever make a NEW regular file, so refuse a symlink
        // node here (`symlink_metadata` does not follow the link).
        if abs
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Ok(FixOutcome::Skipped(format!(
                "{} is a symlink; refusing to create through it",
                self.path.display()
            )));
        }
        let content = match resolve_source_bytes(&self.source, ctx.root, ctx.allow_out_of_root) {
            Ok(bytes) => bytes,
            Err(skip_msg) => return Ok(FixOutcome::Skipped(skip_msg)),
        };
        // A dry run reports only; a stage (`--diff`) records the create (with
        // its resolved content) so the diff can render the new file. Both return
        // before touching disk.
        if ctx.dry_run || ctx.stage_ops.is_some() {
            if let Some(sink) = ctx.stage_ops {
                sink.borrow_mut().push(FixEdit::CreateFile {
                    path: self.path.clone(),
                    content: content.clone(),
                });
            }
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

    fn fix_edit(&self, _violation: &Violation, _bytes: &[u8], root: &Path) -> Option<FixEdit> {
        // The target is set at build time, not taken from the violation. The
        // editor (LSP) fix path doesn't thread `allow_out_of_root`, so confine
        // strictly (deny escape): an editor code-action must never create a
        // file — or read a template — outside the repo root.
        let abs = confine_fix_path(&self.path, root, false).ok()?;
        // Refuse a symlink AT the target (mirrors apply()): a BROKEN symlink
        // slips past `exists()` below (which follows the link) AND past
        // `confine_fix_path` (a non-existent target canonicalizes to nothing, so a
        // planted `evil -> /outside` reads as in-root), and the editor's CreateFile
        // would then follow it out of the tree. `symlink_metadata` does not follow.
        if abs
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return None;
        }
        // Don't propose creating a file where a directory (or any node) already
        // sits -- the editor's CreateFile would fail, and `file_exists` would keep
        // flagging (mirrors apply()'s dir-at-target guard).
        if abs.exists() {
            return None;
        }
        let content = resolve_source_bytes(&self.source, root, false).ok()?;
        Some(FixEdit::CreateFile {
            path: self.path.clone(),
            content,
        })
    }
}

/// Confine a config-declared fixer path (a `file_create.path` write target or a
/// `content_from` read source) to the repo root, honoring the owning rule's
/// `allow_out_of_root`. Returns the joinable absolute path, or `Err(reason)`
/// when it escapes and isn't permitted — the write/read is then refused. This is
/// the fixer-side counterpart of the read rules' `confine_read` gate; without it
/// an untrusted `extends:`'d ruleset's fixer could write or exfiltrate
/// out-of-tree on `alint fix`. Shared with `DirCreateFixer` (see
/// `crate::fixers::file_ops`), which must confine its `dir_create` target the
/// same way.
pub(crate) fn confine_fix_path(
    rel: &Path,
    root: &Path,
    allow: bool,
) -> std::result::Result<PathBuf, String> {
    match crate::pathsafe::confine_read(rel, root, allow) {
        crate::pathsafe::Confined::In(p) | crate::pathsafe::Confined::AllowedEscape(p) => {
            Ok(root.join(p))
        }
        crate::pathsafe::Confined::Denied => Err(format!(
            "{} escapes the repo root (set a top-level `allow_out_of_root` to permit)",
            rel.display()
        )),
    }
}

/// Read a `ContentSourceSpec` to bytes. Returns the raw payload
/// for inline content; for file-sourced content, reads the file
/// at apply time, resolving its path relative to `ctx_root`. A
/// missing or unreadable source produces a `Skipped`-friendly
/// `Err(String)` so the caller can degrade gracefully rather
/// than abort the whole fix run.
pub(crate) fn resolve_source_bytes(
    source: &ContentSourceSpec,
    ctx_root: &std::path::Path,
    allow_out_of_root: bool,
) -> std::result::Result<Vec<u8>, String> {
    match source {
        ContentSourceSpec::Inline(s) => Ok(s.as_bytes().to_vec()),
        ContentSourceSpec::File(rel) => {
            // Confine the `content_from` read the same way rule reads are
            // confined, so an untrusted ruleset can't exfiltrate an out-of-tree
            // secret (`content_from: ../../secret`) into an in-repo file.
            let abs = confine_fix_path(rel, ctx_root, allow_out_of_root)?;
            // Read through the capped/regular-file helper, NOT raw `fs::read`: a
            // config-declared `content_from` is an attacker-reachable path, so a
            // planted in-tree FIFO would otherwise block the whole run forever
            // (even under `--dry-run`/`--diff`, which read here before their
            // no-write early return), and a huge template would be slurped whole
            // (OOM). `read_capped` refuses a non-regular file via a stat before
            // opening and bounds the read at `MAX_ANALYZE_BYTES`.
            crate::io::read_capped(&abs).map_err(|e| match e {
                crate::io::ReadCapError::TooLarge(n) => format!(
                    "content_from `{}` is too large to inline ({})",
                    rel.display(),
                    crate::io::over_cap(n)
                ),
                crate::io::ReadCapError::Io(source) => {
                    format!(
                        "content_from `{}` could not be read: {source}",
                        rel.display()
                    )
                }
            })
        }
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
    applicability: Applicability,
}

impl FilePrependFixer {
    pub fn new(source: ContentSourceSpec) -> Self {
        Self {
            source,
            applicability: Applicability::Safe,
        }
    }

    /// Override the fix tier. W2 demotes a `file_prepend` from an untrusted remote
    /// `extends:` to [`Applicability::Suggestion`]. Defaults to `Safe`.
    #[must_use]
    pub fn with_applicability(mut self, applicability: Applicability) -> Self {
        self.applicability = applicability;
        self
    }
}

impl Fixer for FilePrependFixer {
    fn applicability(&self) -> Applicability {
        self.applicability
    }

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
        let prepend = match resolve_source_bytes(&self.source, ctx.root, ctx.allow_out_of_root) {
            Ok(b) => b,
            Err(skip_msg) => return Ok(FixOutcome::Skipped(skip_msg)),
        };
        let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
            alint_core::ReadForFix::Bytes(b) => b,
            alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        if looks_binary(&existing) {
            return Ok(FixOutcome::Skipped(format!(
                "{} looks binary; not prepending content",
                path.display()
            )));
        }
        // Idempotency guard (L4): if the file already begins with exactly this
        // content (after any BOM), prepending again would stack a duplicate on
        // every `--fix` — which happens when the configured content doesn't
        // satisfy the rule's own pattern, so the violation never clears.
        // `file_starts_with` refuses a fixer outright for this reason; here we
        // can at least no-op safely.
        let body = existing.strip_prefix(UTF8_BOM).unwrap_or(&existing);
        if body.starts_with(prepend.as_slice()) {
            return Ok(FixOutcome::Skipped(format!(
                "{} already begins with the required content",
                path.display()
            )));
        }
        // Dry-run AFTER the read + guards, so a preview matches the real run
        // (Skipped for a binary/oversized/already-satisfied file).
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would prepend {} byte(s) to {}",
                prepend.len(),
                path.display()
            )));
        }
        let mut out = Vec::with_capacity(existing.len() + prepend.len());
        if existing.starts_with(UTF8_BOM) {
            out.extend_from_slice(UTF8_BOM);
            out.extend_from_slice(&prepend);
            out.extend_from_slice(&existing[UTF8_BOM.len()..]);
        } else {
            out.extend_from_slice(&prepend);
            out.extend_from_slice(&existing);
        }
        ctx.commit_write(&abs, &out).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!("prepended {}", path.display())))
    }

    fn fix_edit(&self, violation: &Violation, bytes: &[u8], root: &Path) -> Option<FixEdit> {
        let path = violation.path.as_deref()?;
        // Mirror apply()'s binary guard on the editor (LSP) path: prepending
        // content to a binary file corrupts it.
        if looks_binary(bytes) {
            return None;
        }
        let prepend = resolve_source_bytes(&self.source, root, false).ok()?;
        // Idempotency guard (L4): already-present content is not re-prepended.
        let body = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
        if body.starts_with(prepend.as_slice()) {
            return None;
        }
        let mut out = Vec::with_capacity(bytes.len() + prepend.len());
        if bytes.starts_with(UTF8_BOM) {
            out.extend_from_slice(UTF8_BOM);
            out.extend_from_slice(&prepend);
            out.extend_from_slice(&bytes[UTF8_BOM.len()..]);
        } else {
            out.extend_from_slice(&prepend);
            out.extend_from_slice(bytes);
        }
        Some(FixEdit::SetContent {
            path: path.to_path_buf(),
            content: out,
        })
    }
}

/// Where a header should be inserted: the byte offset after a leading UTF-8 BOM,
/// then after a leading shebang (`#!...`) line OR an XML declaration
/// (`<?xml ...?>`). Returns `(offset, consumed_line_prefix)`: the bool is `true`
/// only when a shebang / XML declaration was skipped (NOT for a bare BOM), so the
/// caller adds a separating newline only when such a prefix lacks its own trailing
/// newline. A file has at most one of a shebang / XML declaration at the very top
/// (each must be the first content), so they are checked in turn; a file with none
/// returns `(0-or-BOM-len, false)` -- a plain BOF-after-BOM insert, like `file_prepend`.
fn header_insert_offset(bytes: &[u8]) -> (usize, bool) {
    let bom = if bytes.starts_with(UTF8_BOM) {
        UTF8_BOM.len()
    } else {
        0
    };
    let rest = &bytes[bom..];
    if rest.starts_with(b"#!") {
        // Skip the shebang line, through its newline (or to EOF if unterminated).
        let line = rest
            .iter()
            .position(|&b| b == b'\n')
            .map_or(rest.len(), |nl| nl + 1);
        (bom + line, true)
    } else if rest.starts_with(b"<?xml")
        && rest
            .get(5)
            .is_none_or(|&c| c.is_ascii_whitespace() || c == b'?')
    {
        // Skip the XML declaration, through its closing `?>` and a single trailing
        // newline. A malformed declaration with no `?>` leaves the insert at the BOM
        // boundary (insert at the very top -- the safe fallback).
        if let Some(close) = rest.windows(2).position(|w| w == b"?>") {
            let mut e = close + 2;
            if rest[e..].starts_with(b"\r\n") {
                e += 2;
            } else if rest[e..].starts_with(b"\n") {
                e += 1;
            }
            (bom + e, true)
        } else {
            (bom, false)
        }
    } else {
        (bom, false)
    }
}

/// Inserts `source` header content near the TOP of each violating file, AFTER any
/// leading UTF-8 BOM, shebang (`#!...` line), or XML declaration (`<?xml ...?>`) --
/// so the header does not displace a line that must stay first (a kernel reads a
/// shebang only on line 1; an XML parser needs the declaration first). The
/// position-aware refinement of [`FilePrependFixer`] for `file_header`; for a file
/// with none of those prefixes it inserts at BOF, exactly like `file_prepend`.
#[derive(Debug)]
pub struct InsertHeaderFixer {
    source: ContentSourceSpec,
    applicability: Applicability,
}

impl InsertHeaderFixer {
    pub fn new(source: ContentSourceSpec) -> Self {
        Self {
            source,
            applicability: Applicability::Safe,
        }
    }

    /// Override the fix tier. W2 demotes an `insert_header` from an untrusted remote
    /// `extends:` to [`Applicability::Suggestion`]. Defaults to `Safe`.
    #[must_use]
    pub fn with_applicability(mut self, applicability: Applicability) -> Self {
        self.applicability = applicability;
        self
    }

    /// The file with `header` inserted at [`header_insert_offset`], or `None` when
    /// the header is already there (idempotence). The idempotency check covers TWO
    /// positions: the computed insertion point (`off`) AND the top after any BOM.
    /// The second is essential when the header content ITSELF begins with a
    /// skippable prefix (`#!` / `<?xml`): a later fixpoint pass's
    /// `header_insert_offset` would treat the just-inserted header as that prefix
    /// and return an `off` BEYOND it, so the `off` check alone would miss the copy
    /// and stack a second one (a bounded fixpoint double -- audit F1). Checking the
    /// after-BOM top too -- mirroring [`FilePrependFixer`]'s fixed-position guard --
    /// makes a repeated fix a guaranteed no-op. Adds one separating newline when the
    /// preceding prefix (a shebang with no trailing newline) leaves `off` mid-line,
    /// so the header always starts on its own line.
    fn inserted(existing: &[u8], header: &[u8]) -> Option<Vec<u8>> {
        let (off, consumed_line_prefix) = header_insert_offset(existing);
        let after_bom = existing.strip_prefix(UTF8_BOM).unwrap_or(existing);
        if existing[off..].starts_with(header) || after_bom.starts_with(header) {
            return None;
        }
        let mut out = Vec::with_capacity(existing.len() + header.len() + 1);
        out.extend_from_slice(&existing[..off]);
        // Separate ONLY a consumed shebang / XML-decl that lacks its own trailing
        // newline (a one-line `#!...` file); never after a bare BOM (whose last byte
        // is not `\n` but needs no separator -- that is a plain BOF-after-BOM insert).
        if consumed_line_prefix && existing[off - 1] != b'\n' {
            out.push(b'\n');
        }
        out.extend_from_slice(header);
        out.extend_from_slice(&existing[off..]);
        Some(out)
    }
}

impl Fixer for InsertHeaderFixer {
    fn applicability(&self) -> Applicability {
        self.applicability
    }

    fn describe(&self) -> String {
        match &self.source {
            ContentSourceSpec::Inline(s) => format!(
                "insert a {}-byte header after any BOM / shebang / XML declaration",
                s.len()
            ),
            ContentSourceSpec::File(rel) => format!(
                "insert the header from {} after any BOM / shebang / XML declaration",
                rel.display()
            ),
        }
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(path) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let abs = ctx.root.join(path);
        let header = match resolve_source_bytes(&self.source, ctx.root, ctx.allow_out_of_root) {
            Ok(b) => b,
            Err(skip_msg) => return Ok(FixOutcome::Skipped(skip_msg)),
        };
        let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
            alint_core::ReadForFix::Bytes(b) => b,
            alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        if looks_binary(&existing) {
            return Ok(FixOutcome::Skipped(format!(
                "{} looks binary; not inserting a header",
                path.display()
            )));
        }
        let Some(out) = Self::inserted(&existing, &header) else {
            return Ok(FixOutcome::Skipped(format!(
                "{} already has the required header at the top",
                path.display()
            )));
        };
        // Dry-run AFTER the read + guards, so a preview matches the real run.
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would insert a {}-byte header into {}",
                header.len(),
                path.display()
            )));
        }
        ctx.commit_write(&abs, &out).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "inserted header into {}",
            path.display()
        )))
    }

    fn fix_edit(&self, violation: &Violation, bytes: &[u8], root: &Path) -> Option<FixEdit> {
        let path = violation.path.as_deref()?;
        // Mirror apply()'s binary guard on the editor (LSP) path.
        if looks_binary(bytes) {
            return None;
        }
        let header = resolve_source_bytes(&self.source, root, false).ok()?;
        let out = Self::inserted(bytes, &header)?;
        Some(FixEdit::SetContent {
            path: path.to_path_buf(),
            content: out,
        })
    }
}

/// Appends `source` content to the end of each violating file.
/// Paired with `file_content_matches` / `file_footer` when the
/// required content is satisfied by the appended bytes.
#[derive(Debug)]
pub struct FileAppendFixer {
    source: ContentSourceSpec,
    applicability: Applicability,
}

impl FileAppendFixer {
    pub fn new(source: ContentSourceSpec) -> Self {
        Self {
            source,
            applicability: Applicability::Safe,
        }
    }

    /// Override the fix tier. W2 demotes a `file_append` from an untrusted remote
    /// `extends:` to [`Applicability::Suggestion`]. Defaults to `Safe`.
    #[must_use]
    pub fn with_applicability(mut self, applicability: Applicability) -> Self {
        self.applicability = applicability;
        self
    }
}

impl Fixer for FileAppendFixer {
    fn applicability(&self) -> Applicability {
        self.applicability
    }

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
        let payload = match resolve_source_bytes(&self.source, ctx.root, ctx.allow_out_of_root) {
            Ok(b) => b,
            Err(skip_msg) => return Ok(FixOutcome::Skipped(skip_msg)),
        };
        let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
            alint_core::ReadForFix::Bytes(b) => b,
            alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        if looks_binary(&existing) {
            return Ok(FixOutcome::Skipped(format!(
                "{} looks binary; not appending content",
                path.display()
            )));
        }
        // Idempotency guard (L4): see FilePrependFixer — don't stack the footer
        // on every `--fix` when the content doesn't satisfy the rule's pattern.
        if existing.ends_with(payload.as_slice()) {
            return Ok(FixOutcome::Skipped(format!(
                "{} already ends with the required content",
                path.display()
            )));
        }
        // Dry-run AFTER the read + guards, so a preview matches the real run
        // (Skipped for a binary/oversized/already-satisfied file).
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would append {} byte(s) to {}",
                payload.len(),
                path.display()
            )));
        }
        let mut out = existing;
        out.extend_from_slice(&payload);
        ctx.commit_write(&abs, &out).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "appended to {}",
            path.display()
        )))
    }

    fn fix_edit(&self, violation: &Violation, bytes: &[u8], root: &Path) -> Option<FixEdit> {
        let path = violation.path.as_deref()?;
        // Mirror apply()'s binary guard on the editor (LSP) path: appending
        // content to a binary file corrupts it.
        if looks_binary(bytes) {
            return None;
        }
        let payload = resolve_source_bytes(&self.source, root, false).ok()?;
        // Idempotency guard (L4): already-present content is not re-appended.
        if bytes.ends_with(payload.as_slice()) {
            return None;
        }
        let mut out = bytes.to_vec();
        out.extend_from_slice(&payload);
        Some(FixEdit::SetContent {
            path: path.to_path_buf(),
            content: out,
        })
    }
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
    fn file_create_reports_honestly_when_a_directory_occupies_the_target() {
        // Round-5 audit (A4): `file_exists` counts only files, so a DIRECTORY at
        // the target keeps it flagging; the fixer used to skip with the false
        // "already exists" and never converge. It must report the real blocker.
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("LICENSE")).unwrap();
        let fixer = FileCreateFixer::new(PathBuf::from("LICENSE"), "MIT\n".into(), true);
        let outcome = fixer
            .apply(&Violation::new("missing LICENSE"), &make_ctx(&tmp, false))
            .unwrap();
        match outcome {
            FixOutcome::Skipped(reason) => {
                assert!(reason.contains("is a directory"), "honest reason: {reason}");
                assert!(
                    !reason.contains("already exists"),
                    "must not mislead: {reason}"
                );
            }
            FixOutcome::Applied(_) => panic!("expected Skipped for a dir at the target"),
        }
        assert!(
            tmp.path().join("LICENSE").is_dir(),
            "the directory is untouched"
        );
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
    fn file_create_rejects_a_non_regular_content_from_source() {
        // Phase-0 audit: `content_from` reads through `read_capped`, which
        // refuses a non-regular file (FIFO/socket/device/dir) via a stat BEFORE
        // opening. A planted in-tree FIFO would otherwise block `alint fix`
        // (even `--dry-run`/`--diff`) forever on the `O_RDONLY` open. A directory
        // is the portable, hang-free proxy (`is_file() == false`, same as a
        // FIFO) - this crate takes no libc dep, so it cannot `mkfifo` here.
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("tmpl_dir")).unwrap();
        let fixer = FileCreateFixer::new(
            PathBuf::from("LICENSE"),
            ContentSourceSpec::File(PathBuf::from("tmpl_dir")),
            true,
        );
        let outcome = fixer
            .apply(&Violation::new("missing"), &make_ctx(&tmp, false))
            .unwrap();
        let FixOutcome::Skipped(msg) = &outcome else {
            panic!("expected Skipped for a non-regular content_from, got {outcome:?}")
        };
        assert!(
            msg.contains("could not be read") && msg.contains("not a regular file"),
            "message should name the non-regular source: {msg}"
        );
        assert!(!tmp.path().join("LICENSE").exists());
    }

    #[cfg(unix)]
    #[test]
    fn file_create_refuses_to_write_through_a_symlink_target() {
        // Phase-0 audit (defense in depth): a broken symlink at the target slips
        // past the canonicalize-based confinement (its absent target
        // canonicalizes to nothing = in-root), and `fs::write` would follow it
        // out of the tree. The fixer must refuse a symlink node.
        use std::os::unix::fs::symlink;
        let tmp = TempDir::new().unwrap();
        let outside = tmp.path().join("OUTSIDE.txt"); // target absent -> broken link
        symlink(&outside, tmp.path().join("evil")).unwrap();
        let fixer = FileCreateFixer::new(PathBuf::from("evil"), "PWNED\n".into(), true);
        let outcome = fixer
            .apply(&Violation::new("x"), &make_ctx(&tmp, false))
            .unwrap();
        let FixOutcome::Skipped(msg) = &outcome else {
            panic!("expected Skipped for a symlink target, got {outcome:?}")
        };
        assert!(
            msg.contains("symlink"),
            "message should name the symlink: {msg}"
        );
        assert!(
            !outside.exists(),
            "must NOT have written through the symlink to the out-of-tree target"
        );
    }

    #[cfg(unix)]
    #[test]
    fn file_create_fix_edit_refuses_to_propose_a_create_through_a_symlink() {
        // Round-6 audit: the symlink-target refusal was in apply() only, so the
        // editor (LSP) code-action would still hand the editor a CreateFile that
        // follows a broken `evil -> /outside` link out of the tree. fix_edit must
        // mirror apply's `symlink_metadata` guard.
        use std::os::unix::fs::symlink;
        let tmp = TempDir::new().unwrap();
        let outside = tmp.path().join("OUTSIDE.txt"); // absent -> broken link
        symlink(&outside, tmp.path().join("evil")).unwrap();
        let fixer = FileCreateFixer::new(PathBuf::from("evil"), "PWNED\n".into(), true);
        assert!(
            fixer
                .fix_edit(&Violation::new("x"), &[], tmp.path())
                .is_none(),
            "fix_edit must not propose creating through a symlink target"
        );
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
            FixOutcome::Applied(s) => {
                assert!(s.starts_with("would create"));
                assert!(s.contains("x.txt"), "summary must name the file: {s}");
            }
            FixOutcome::Skipped(_) => panic!("expected Applied"),
        }
        assert!(!tmp.path().join("x.txt").exists());
    }

    #[test]
    fn file_create_in_stage_mode_records_content_without_writing() {
        // Stage mode (`--diff`): a sink present, `dry_run` false. The create is
        // recorded with its resolved content, and disk stays untouched.
        let tmp = TempDir::new().unwrap();
        let fixer = FileCreateFixer::new(PathBuf::from("README.md"), "# Hi\n".into(), true);
        let sink = std::cell::RefCell::new(Vec::new());
        let ctx = FixContext {
            root: tmp.path(),
            dry_run: false,
            fix_size_limit: None,
            allow_out_of_root: false,
            compose: None,
            stage_ops: Some(&sink),
        };
        let outcome = fixer.apply(&Violation::new("missing"), &ctx).unwrap();
        assert!(matches!(outcome, FixOutcome::Applied(_)));
        assert!(
            !tmp.path().join("README.md").exists(),
            "stage must not create the file"
        );
        assert_eq!(
            sink.into_inner(),
            vec![FixEdit::CreateFile {
                path: PathBuf::from("README.md"),
                content: b"# Hi\n".to_vec(),
            }]
        );
    }

    #[test]
    fn prepend_and_append_skip_binary_files_on_both_paths() {
        // Phase-0 audit: prepend/append must refuse a NUL-bearing binary on BOTH
        // the `alint fix` (apply) and editor (fix_edit) paths -- inserting text
        // into a binary corrupts it. (`\x00` marks binary; the file is otherwise
        // valid UTF-8 so `from_utf8` alone would not catch it.)
        let binary: &[u8] = b"hdr\n\x00body\n";
        let v = Violation::new("x").with_path(std::path::Path::new("blob"));
        let fixers: [Box<dyn Fixer>; 2] = [
            Box::new(FilePrependFixer::new("// header\n".into())),
            Box::new(FileAppendFixer::new("// footer\n".into())),
        ];
        for fixer in &fixers {
            let tmp = TempDir::new().unwrap();
            std::fs::write(tmp.path().join("blob"), binary).unwrap();
            let outcome = fixer.apply(&v, &make_ctx(&tmp, false)).unwrap();
            assert!(
                matches!(outcome, FixOutcome::Skipped(_)),
                "apply() must skip a binary, got {outcome:?}"
            );
            assert_eq!(
                std::fs::read(tmp.path().join("blob")).unwrap(),
                binary,
                "the binary file must be byte-identical after skipping"
            );
            assert!(
                fixer.fix_edit(&v, binary, tmp.path()).is_none(),
                "fix_edit() must decline a binary (the editor/LSP path)"
            );
        }
    }

    #[test]
    fn file_prepend_inserts_at_start() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "fn main() {}\n").unwrap();
        let fixer = FilePrependFixer::new("// Copyright 2026\n".into());
        fixer
            .apply(
                &Violation::new("missing header").with_path(std::path::Path::new("a.rs")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.rs")).unwrap(),
            "// Copyright 2026\nfn main() {}\n"
        );
    }

    #[test]
    fn file_prepend_is_idempotent_across_runs() {
        // L4: a second `--fix` must NOT stack a duplicate header (the failure
        // mode when the content doesn't satisfy the rule's pattern).
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "fn main() {}\n").unwrap();
        let fixer = FilePrependFixer::new("// Copyright 2026\n".into());
        let v = Violation::new("missing header").with_path(std::path::Path::new("a.rs"));
        let first = fixer.apply(&v, &make_ctx(&tmp, false)).unwrap();
        assert!(matches!(first, FixOutcome::Applied(_)));
        let second = fixer.apply(&v, &make_ctx(&tmp, false)).unwrap();
        assert!(matches!(second, FixOutcome::Skipped(_)), "second run skips");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.rs")).unwrap(),
            "// Copyright 2026\nfn main() {}\n",
            "header not stacked"
        );
        // The editor path is idempotent too.
        let bytes = std::fs::read(tmp.path().join("a.rs")).unwrap();
        assert!(fixer.fix_edit(&v, &bytes, tmp.path()).is_none());
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
                &Violation::new("m").with_path(std::path::Path::new("x.txt")),
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
                &Violation::new("m").with_path(std::path::Path::new("a.rs")),
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
                &Violation::new("missing section").with_path(std::path::Path::new("notes.md")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("notes.md")).unwrap(),
            "# Notes\n\n## Section\n"
        );
    }

    #[test]
    fn file_append_is_idempotent_across_runs() {
        // L4: a second `--fix` must NOT stack a duplicate footer.
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("notes.md"), "# Notes\n").unwrap();
        let fixer = FileAppendFixer::new("\n## Section\n".into());
        let v = Violation::new("missing section").with_path(std::path::Path::new("notes.md"));
        assert!(matches!(
            fixer.apply(&v, &make_ctx(&tmp, false)).unwrap(),
            FixOutcome::Applied(_)
        ));
        assert!(
            matches!(
                fixer.apply(&v, &make_ctx(&tmp, false)).unwrap(),
                FixOutcome::Skipped(_)
            ),
            "second run skips"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("notes.md")).unwrap(),
            "# Notes\n\n## Section\n",
            "footer not stacked"
        );
    }

    #[test]
    fn file_append_dry_run_leaves_file_unchanged() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("x.txt"), "orig\n").unwrap();
        FileAppendFixer::new("extra\n".into())
            .apply(
                &Violation::new("m").with_path(std::path::Path::new("x.txt")),
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
    fn file_create_fix_edit_returns_create_with_inline_content() {
        let tmp = TempDir::new().unwrap();
        let fixer = FileCreateFixer::new(PathBuf::from("LICENSE"), "Apache-2.0\n".into(), true);
        let edit = fixer
            .fix_edit(&Violation::new("missing"), &[], tmp.path())
            .unwrap();
        assert_eq!(
            edit,
            FixEdit::CreateFile {
                path: PathBuf::from("LICENSE"),
                content: b"Apache-2.0\n".to_vec(),
            }
        );
    }

    #[test]
    fn file_create_fix_edit_declines_when_a_node_occupies_the_target() {
        // Round-6 audit: mirror apply()'s dir-at-target guard on the editor path
        // -- don't propose creating a file where a directory already sits (the
        // editor's CreateFile would fail and `file_exists` would keep flagging).
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("LICENSE")).unwrap();
        let fixer = FileCreateFixer::new(PathBuf::from("LICENSE"), "MIT\n".into(), true);
        assert!(
            fixer
                .fix_edit(&Violation::new("missing"), &[], tmp.path())
                .is_none()
        );
    }

    #[test]
    fn file_append_fix_edit_appends_payload() {
        let tmp = TempDir::new().unwrap();
        let fixer = FileAppendFixer::new("\n## Section\n".into());
        let edit = fixer
            .fix_edit(
                &Violation::new("m").with_path(std::path::Path::new("notes.md")),
                b"# Notes\n",
                tmp.path(),
            )
            .unwrap();
        assert_eq!(
            edit,
            FixEdit::SetContent {
                path: PathBuf::from("notes.md"),
                content: b"# Notes\n\n## Section\n".to_vec(),
            }
        );
    }

    #[test]
    fn file_prepend_fix_edit_inserts_before_existing_bytes() {
        let tmp = TempDir::new().unwrap();
        let fixer = FilePrependFixer::new("// header\n".into());
        let edit = fixer
            .fix_edit(
                &Violation::new("m").with_path(std::path::Path::new("a.rs")),
                b"fn main() {}\n",
                tmp.path(),
            )
            .unwrap();
        assert_eq!(
            edit,
            FixEdit::SetContent {
                path: PathBuf::from("a.rs"),
                content: b"// header\nfn main() {}\n".to_vec(),
            }
        );
    }

    // ---- insert_header ----

    fn ih(content: &str) -> InsertHeaderFixer {
        InsertHeaderFixer::new(content.into())
    }

    fn ins(content: &str, existing: &[u8]) -> Option<Vec<u8>> {
        InsertHeaderFixer::inserted(existing, content.as_bytes())
    }

    #[test]
    fn insert_header_goes_after_a_shebang() {
        assert_eq!(
            ins("# (c) acme\n", b"#!/bin/bash\necho hi\n").unwrap(),
            b"#!/bin/bash\n# (c) acme\necho hi\n".to_vec()
        );
    }

    #[test]
    fn insert_header_goes_after_an_xml_declaration() {
        assert_eq!(
            ins("<!-- (c) acme -->\n", b"<?xml version=\"1.0\"?>\n<root/>\n").unwrap(),
            b"<?xml version=\"1.0\"?>\n<!-- (c) acme -->\n<root/>\n".to_vec()
        );
    }

    #[test]
    fn insert_header_at_bof_without_a_prefix_matches_file_prepend() {
        // No BOM / shebang / xml-decl -> plain BOF insert (== file_prepend).
        assert_eq!(
            ins("// (c) acme\n", b"fn main() {}\n").unwrap(),
            b"// (c) acme\nfn main() {}\n".to_vec()
        );
    }

    #[test]
    fn insert_header_goes_after_a_bom() {
        let existing = b"\xEF\xBB\xBFfn main() {}\n";
        assert_eq!(
            ins("// h\n", existing).unwrap(),
            b"\xEF\xBB\xBF// h\nfn main() {}\n".to_vec()
        );
    }

    #[test]
    fn insert_header_after_bom_and_shebang() {
        // BOM then shebang: skip BOTH.
        let existing = b"\xEF\xBB\xBF#!/bin/sh\ncode\n";
        assert_eq!(
            ins("# h\n", existing).unwrap(),
            b"\xEF\xBB\xBF#!/bin/sh\n# h\ncode\n".to_vec()
        );
    }

    #[test]
    fn insert_header_adds_a_separator_when_the_shebang_has_no_newline() {
        // A one-line `#!...` file with no trailing newline: the header must not be
        // glued onto the shebang.
        assert_eq!(
            ins("# h\n", b"#!/bin/sh").unwrap(),
            b"#!/bin/sh\n# h\n".to_vec()
        );
    }

    #[test]
    fn insert_header_is_idempotent_when_already_present() {
        // Header already sits right after the shebang -> no-op (guaranteed no
        // runaway: the byte check round-trips exactly what apply would insert).
        assert!(ins("# h\n", b"#!/bin/sh\n# h\ncode\n").is_none());
    }

    #[test]
    fn insert_header_does_not_double_when_content_is_itself_a_skippable_prefix() {
        // AUDIT F1: a header whose content itself begins with `<?xml` / `#!` would,
        // on a second fixpoint pass, be re-parsed by `header_insert_offset` as a
        // skippable prefix -- so the `off`-only idempotency check would jump PAST the
        // just-inserted copy and stack a SECOND one (two xml-decls = invalid XML).
        // The after-BOM guard makes the second pass a no-op.
        let decl = b"<?xml version=\"1.0\"?>\n".as_slice();
        let once = InsertHeaderFixer::inserted(b"<root/>\n", decl).unwrap();
        assert_eq!(once, b"<?xml version=\"1.0\"?>\n<root/>\n".to_vec());
        assert!(
            InsertHeaderFixer::inserted(&once, decl).is_none(),
            "an xml-decl header must not double: {}",
            String::from_utf8_lossy(&once)
        );
        // Shebang-shaped header, same trap.
        let sh = b"#!/usr/bin/env doit\n".as_slice();
        let sh_once = InsertHeaderFixer::inserted(b"plain\n", sh).unwrap();
        assert!(InsertHeaderFixer::inserted(&sh_once, sh).is_none());
        // With a BOM before the (xml-decl) header.
        let bom_once = InsertHeaderFixer::inserted(b"\xEF\xBB\xBF<root/>\n", decl).unwrap();
        assert!(InsertHeaderFixer::inserted(&bom_once, decl).is_none());
    }

    #[test]
    fn insert_header_malformed_xml_decl_falls_back_to_top() {
        // A `<?xml` with no closing `?>` -> insert at the top (after BOM), never
        // scan past EOF or corrupt.
        assert_eq!(
            ins("<!-- h -->\n", b"<?xml version truncated").unwrap(),
            b"<!-- h -->\n<?xml version truncated".to_vec()
        );
    }

    #[test]
    fn insert_header_apply_writes_after_the_shebang_on_disk() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("run.sh"), b"#!/bin/bash\necho hi\n").unwrap();
        let outcome = ih("# (c) acme\n")
            .apply(
                &Violation::new("m").with_path(std::path::Path::new("run.sh")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Applied(_)), "{outcome:?}");
        assert_eq!(
            std::fs::read(tmp.path().join("run.sh")).unwrap(),
            b"#!/bin/bash\n# (c) acme\necho hi\n"
        );
    }

    #[test]
    fn insert_header_skips_a_binary_file() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("bin"), b"\x00\x01\x02\x03bin\x00").unwrap();
        let outcome = ih("# h\n")
            .apply(
                &Violation::new("m").with_path(std::path::Path::new("bin")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert!(
            matches!(&outcome, FixOutcome::Skipped(s) if s.contains("binary")),
            "{outcome:?}"
        );
    }

    #[test]
    fn insert_header_dry_run_does_not_touch_disk() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("run.sh"), b"#!/bin/sh\ncode\n").unwrap();
        let outcome = ih("# h\n")
            .apply(
                &Violation::new("m").with_path(std::path::Path::new("run.sh")),
                &make_ctx(&tmp, true),
            )
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Applied(_)), "{outcome:?}");
        // dry-run reports Applied but leaves the file byte-identical.
        assert_eq!(
            std::fs::read(tmp.path().join("run.sh")).unwrap(),
            b"#!/bin/sh\ncode\n"
        );
    }
}
