//! Reading and writing a file for a fix: the paths no fix touches, the
//! `fix_size_limit` guard, and the atomic write that keeps a file's mode and
//! refuses one it cannot replace.

use std::path::{Path, PathBuf};

use crate::error::Result;

use super::{FixContext, FixOutcome};

impl FixContext<'_> {
    /// Why a fix may not write, create, remove, rename or change the mode of
    /// `path` (relative to the root, or absolute), or `None` when it may.
    ///
    /// Every fix refuses a path that reaches a Git directory, by name or through
    /// a symlink ([`crate::reaches_git_directory`]): a file it puts there is
    /// configuration Git runs commands from. A fix of an
    /// [`extended_source`](Self::extended_source) rule also refuses alint's own
    /// configuration ([`crate::reaches_alint_config`]), which may run commands
    /// that source may not. The fixers ask before they touch a path, and
    /// [`read_for_fix`] and [`Self::commit_write`] ask for every content edit, so
    /// each reports the refusal as a skip.
    ///
    /// A file [`Self::index`] reached without a symlink is checked by name
    /// alone; any other path is checked on disk.
    #[must_use]
    pub fn write_refusal(&self, path: &Path) -> Option<String> {
        let path = path.strip_prefix(self.root).unwrap_or(path);
        if crate::pathsafe::reaches_git_directory_walked(self.root, path, self.index) {
            return Some(crate::git_directory_refusal(path));
        }
        (self.extended_source
            && crate::pathsafe::reaches_alint_config_walked(
                self.root,
                path,
                self.config_files,
                self.index,
            ))
        .then(|| crate::alint_config_refusal(path))
    }

    /// Persist a whole-file write of `bytes` for the file whose absolute path
    /// is `abs`. In compose mode the bytes are buffered for the engine's single
    /// per-file flush; otherwise they are written atomically now, byte-for-byte
    /// the same as a direct [`write_atomic`]. Content fixers call this instead
    /// of `write_atomic` so the engine can unify the write path.
    ///
    /// The compose buffer is keyed by the *resolved write target*
    /// (`resolve_write_target`): a symlink and its in-tree target coalesce to
    /// one entry, so two fixers touching the same underlying file through
    /// different paths compose (the second reads the first's buffered write)
    /// exactly as they would when writing straight to disk.
    ///
    /// A path [`Self::write_refusal`] refuses is not written: the call fails
    /// with `PermissionDenied` and the reason.
    ///
    /// # Errors
    /// Propagates the underlying [`write_atomic`] I/O error in direct mode;
    /// buffering in compose mode is infallible. A refused path fails either way.
    pub fn commit_write(&self, abs: &Path, bytes: &[u8]) -> std::io::Result<()> {
        // A dry run must write NOTHING: every fixer guards `if ctx.dry_run` before
        // reaching here, and the fixpoint's cap-confirmation pass relies on that
        // (it runs `fix_run(dry_run=true)` purely to test convergence). Assert the
        // invariant at the choke point so a fixer that forgets the guard fails
        // loudly in tests/debug rather than silently mutating the tree.
        debug_assert!(
            !self.dry_run,
            "commit_write reached during a dry run; a fixer must check ctx.dry_run before writing"
        );
        // Release-safe backstop (the assert is compiled out in release): a dry run
        // writes NOTHING, so a fixer that forgot its `ctx.dry_run` guard becomes a
        // silent no-op here instead of mutating the tree during, e.g., the
        // fixpoint's cap-confirmation pass.
        if self.dry_run {
            return Ok(());
        }
        // Backstop for a fixer that writes without first reading through
        // `read_for_fix`, which reports the same refusal as a skip.
        if let Some(reason) = self.write_refusal(abs) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                reason,
            ));
        }
        match self.compose {
            Some(buf) => {
                buf.borrow_mut()
                    .insert(resolve_write_target(abs), bytes.to_vec());
                Ok(())
            }
            None => write_atomic(abs, bytes),
        }
    }

    /// True when a content fixer earlier in this pass has already composed a
    /// pending whole-file write for `abs` (coalesced by resolved target, so a
    /// symlink and its target share the answer). A whole-file op (remove /
    /// rename) MUST yield when this is true: the pending content write is
    /// flushed *after* the fixer loop, so acting on the pre-edit file now would
    /// let the flush resurrect the removed/renamed path or strand the composed
    /// bytes at a vacated path (a file-duplication corruption). Returns `false`
    /// when there is no compose buffer (a `--dry-run` pass composes nothing).
    pub fn has_pending_write(&self, abs: &Path) -> bool {
        self.compose
            .is_some_and(|buf| buf.borrow().contains_key(&resolve_write_target(abs)))
    }
}

/// Result of [`read_for_fix`] — either the bytes of the file,
/// or a [`FixOutcome::Skipped`] the caller should return.
///
/// Content-editing fixers (`file_prepend`, `file_append`,
/// `file_trim_trailing_whitespace`, …) funnel their initial read
/// through this helper so the `fix_size_limit` guard is enforced
/// uniformly: over-limit files are reported as `Skipped` with a
/// clear reason, and a one-line warning is printed to stderr so
/// scripted runs notice.
#[derive(Debug)]
pub enum ReadForFix {
    Bytes(Vec<u8>),
    Skipped(FixOutcome),
}

/// Check whether `abs` is within the `fix_size_limit` on `ctx`.
/// Returns `Some(outcome)` when the file is over-limit (the
/// caller returns this directly); returns `None` when the fix
/// can proceed. Emits a one-line stderr warning on over-limit.
///
/// Use this in fixers that modify the file without reading the
/// full body (e.g. streaming append). For read-modify-write
/// flows, prefer [`read_for_fix`] which folds the check in.
pub fn check_fix_size(
    abs: &Path,
    display_path: &std::path::Path,
    ctx: &FixContext<'_>,
) -> Result<Option<FixOutcome>> {
    let Some(limit) = ctx.fix_size_limit else {
        return Ok(None);
    };
    let metadata = std::fs::metadata(abs).map_err(|source| crate::error::Error::Io {
        path: abs.to_path_buf(),
        source,
    })?;
    if metadata.len() > limit {
        // The limit is left out: it can come from the environment, and this
        // warning reaches stderr without the embedder's scrubbing. So is a
        // path the rule's configuration reads from it.
        let reason = format!(
            "{} is {} bytes; exceeds fix_size_limit. Raise \
             `fix_size_limit` in .alint.yml (or set it to `null` to disable) \
             to fix files this large.",
            ctx.shown_path(display_path),
            metadata.len(),
        );
        eprintln!("alint: warning: {reason}");
        return Ok(Some(FixOutcome::Skipped(reason)));
    }
    Ok(None)
}

/// Read `abs` subject to the size limit on `ctx`. Over-limit
/// files return `ReadForFix::Skipped(Outcome::Skipped(_))` and
/// emit a one-line stderr warning; in-limit files return
/// `ReadForFix::Bytes(...)`. Pass-through I/O errors propagate.
///
/// The file is one the fix will edit, so a path [`FixContext::write_refusal`]
/// refuses is skipped with its reason before anything is read. For a file the
/// fix only reads, use [`read_source_for_fix`].
pub fn read_for_fix(
    abs: &Path,
    display_path: &std::path::Path,
    ctx: &FixContext<'_>,
) -> Result<ReadForFix> {
    if let Some(reason) = ctx.write_refusal(abs) {
        return Ok(ReadForFix::Skipped(FixOutcome::Skipped(reason)));
    }
    read_checked_for_fix(abs, display_path, ctx)
}

/// [`read_for_fix`] for a file a fix reads but does not write (`sync_from`'s
/// canonical source, a cross-file value's source): a path in a Git directory
/// is still skipped, since a fix never reads there either, but alint's own
/// configuration may be read.
///
/// # Errors
/// Propagates an I/O error from the stat or the read, as [`read_for_fix`] does.
pub fn read_source_for_fix(
    abs: &Path,
    display_path: &std::path::Path,
    ctx: &FixContext<'_>,
) -> Result<ReadForFix> {
    let rel = abs.strip_prefix(ctx.root).unwrap_or(abs);
    if crate::pathsafe::reaches_git_directory_walked(ctx.root, rel, ctx.index) {
        return Ok(ReadForFix::Skipped(FixOutcome::Skipped(
            crate::git_directory_refusal(rel),
        )));
    }
    read_checked_for_fix(abs, display_path, ctx)
}

/// [`read_for_fix`] and [`read_source_for_fix`] once their path checks passed.
fn read_checked_for_fix(
    abs: &Path,
    display_path: &std::path::Path,
    ctx: &FixContext<'_>,
) -> Result<ReadForFix> {
    // In compose mode, an earlier fixer's write for this file lives in the
    // buffer, not on disk; read it back so fixers compose in config order.
    // Keyed by the resolved write target (matching `commit_write`), so a read
    // through a symlink sees a write made through the target and vice versa.
    // The bytes were already size-checked on their first (disk) read, so the
    // cap is not re-applied to in-memory bytes.
    if let Some(buf) = ctx.compose
        && let Some(bytes) = buf.borrow().get(&resolve_write_target(abs))
    {
        return Ok(ReadForFix::Bytes(bytes.clone()));
    }
    // Refuse a non-regular file (FIFO / socket / device) BEFORE the read: a bare
    // `std::fs::read` opens a named pipe `O_RDONLY` and BLOCKS until a writer
    // appears, hanging the whole `fix` run -- including the `--dry-run` / `--diff`
    // previews users run to stay safe. The walker prunes special files at index
    // time, but a fixer can be handed a config-verbatim path that skips the walker
    // (a `sync_from` `targets:` list entry or `source:`), so this direct-read path
    // must guard exactly as `read_capped` / `open_regular` do. A stat error (e.g. a
    // missing file) falls through so the read surfaces the same I/O error callers
    // already handle.
    if std::fs::metadata(abs).is_ok_and(|m| !m.is_file()) {
        return Ok(ReadForFix::Skipped(FixOutcome::Skipped(format!(
            "{} is not a regular file",
            display_path.display()
        ))));
    }
    if let Some(outcome) = check_fix_size(abs, display_path, ctx)? {
        return Ok(ReadForFix::Skipped(outcome));
    }
    let bytes = std::fs::read(abs).map_err(|source| crate::error::Error::Io {
        path: abs.to_path_buf(),
        source,
    })?;
    Ok(ReadForFix::Bytes(bytes))
}

/// The canonical underlying file a write to `path` ultimately modifies, with
/// EVERY symlink in the path resolved — a symlinked file AND a file reached
/// through a symlinked directory both collapse to the same real path. The
/// compose buffer keys on this so two paths that alias one underlying file
/// coalesce to a single entry and compose, exactly as the direct-write path
/// does (a temp+rename through either alias lands on the same real file).
///
/// This is a FULL canonicalization, deliberately stronger than
/// [`write_atomic`]'s own final-component symlink check (which exists only to
/// preserve the final link node during its rename): keying must reflect the
/// real file, not the spelling. Falls back to `path` when it cannot be
/// canonicalized (e.g. a race deleted it), so a key is never lost.
#[must_use]
pub(crate) fn resolve_write_target(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Write `bytes` to `path` atomically: write a uniquely-named sibling temp
/// file, copy the original's permissions onto it (so an existing mode — notably
/// the executable bit — survives), `fsync`, then rename it over `path`. Unlike
/// `std::fs::write` (open-truncate-then-write), a crash or I/O error mid-write
/// leaves the original intact rather than truncated. The temp is a sibling so
/// the rename is atomic on the same filesystem, and it is cleaned up on
/// failure. Writes THROUGH a symlink to its canonical target, preserving the
/// link. (Manual temp, no `tempfile` runtime dependency.) An existing
/// read-only target is refused with `PermissionDenied` rather than replaced
/// (the rename would otherwise succeed on directory permissions alone).
///
/// Lives in `alint-core` so both the fixers (`alint-rules`) and the engine's
/// compose flush share one implementation; `alint-rules::io` re-exports it.
///
/// # Errors
/// Propagates any I/O error from creating, writing, syncing, or renaming.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};
    // Unique sibling name: the pid distinguishes concurrent processes, the
    // atomic counter distinguishes concurrent threads in this process.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    // Write THROUGH a symlink to its (canonical) target, preserving the link.
    // A bare temp+rename on the link path would replace the link NODE with a
    // regular file, silently diverging it from its target (common for a
    // symlinked LICENSE / README in a monorepo). `canonicalize` needs the
    // target to exist, which it does: every caller has just read the file. A
    // broken symlink errors here rather than clobbering the link (unlike the
    // key-only `resolve_write_target`, which falls back so a key is never lost).
    let resolved = match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => std::fs::canonicalize(path)?,
        _ => path.to_path_buf(),
    };
    let path = resolved.as_path();
    let dir = temp_dir_for(path);
    // Refuse to replace a file we could not write in place. A temp+rename only
    // needs write access to the DIRECTORY, so without this check a `chmod 444`
    // file (or a Perforce checkout not yet opened for edit) would be silently
    // rewritten -- unlike a plain in-place write, which fails with EACCES. The
    // caller reports the error (the engine downgrades the item to a fix error),
    // so the violation stands visibly instead.
    if let Ok(meta) = std::fs::metadata(path) {
        refuse_unwritable(path, &meta)?;
    }
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    // A short, fixed-shape name: the target's own name is NOT embedded, so a
    // long basename (> ~230 bytes) cannot push the temp past NAME_MAX (255 on
    // most filesystems; the old `.{name}.alint-fix.{pid}.{n}` failed with
    // ENAMETOOLONG / os error 36). pid + counter keep it unique.
    let tmp = dir.join(format!(".alint-fix.{}.{n}.tmp", std::process::id()));
    let write = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        // Preserve the original file's mode when it exists (a rewrite) -- AFTER
        // the write, not before. A `write()` clears the setuid/setgid bits (the
        // kernel's `should_remove_suid`), so copying the mode before writing
        // would silently drop S_ISUID/S_ISGID from a setgid group helper or the
        // like. Applying the mode after the last write preserves the full mode
        // word (the plain executable bit survived either way, which is why the
        // ordering bug went unnoticed).
        if let Ok(meta) = std::fs::metadata(path) {
            f.set_permissions(meta.permissions())?;
        }
        f.sync_all()
    };
    if let Err(e) = write().and_then(|()| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// The directory `write_atomic` writes its temporary file in: the target's
/// own, so the rename stays on one filesystem (`.` for a bare file name).
pub(super) fn temp_dir_for(path: &Path) -> PathBuf {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// `Err(PermissionDenied)` when the existing file at `path` is not writable in
/// place by this process.
///
/// `Permissions::readonly()` alone is not enough: on unix it is true only when
/// EVERY write bit is clear, so a `0464` file (owner may not write, group may)
/// or a `0644` file owned by another user passed. The authoritative check is
/// the kernel's own: open the file for writing (append mode, no truncation, no
/// bytes written; mtime is untouched). `readonly()` is kept as well so a
/// `chmod 444` file stays refused even for root, whose open would succeed.
/// Only a regular file is probed (opening a FIFO for writing could block).
pub(super) fn refuse_unwritable(path: &Path, meta: &std::fs::Metadata) -> std::io::Result<()> {
    let refuse = |msg: String| {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            msg,
        ))
    };
    if meta.permissions().readonly() {
        return refuse(format!(
            "refusing to overwrite read-only file {}",
            path.display()
        ));
    }
    if meta.is_file() {
        match std::fs::OpenOptions::new().append(true).open(path) {
            Ok(_) => {}
            // Vanished since `metadata`: nothing to protect; the write creates it.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return refuse(format!(
                    "refusing to overwrite {}: not writable in place ({e})",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}
