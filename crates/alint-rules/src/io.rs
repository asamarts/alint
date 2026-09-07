//! Shared I/O helpers for content-reading rules.

use std::io::{Read as _, Seek, SeekFrom};
use std::path::Path;

/// How much of a file to sample when classifying text vs. binary.
pub const TEXT_INSPECT_LEN: usize = 8 * 1024;

/// The `InvalidInput` error a direct-read helper returns for a non-regular
/// file, so the message is uniform across the three read helpers.
fn not_regular_file(path: &Path) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("{} is not a regular file", path.display()),
    )
}

/// Open `path` for reading, first refusing a non-regular file (FIFO / socket /
/// device). Opening a FIFO `O_RDONLY` blocks until a writer appears, so a
/// direct read of a planted in-tree named pipe (reachable via a config-declared
/// path or a symlink the walker followed) would hang the whole run. The walker
/// skips these at index time (`result_to_entry`); the direct-read helpers that
/// bypass the walker must apply the same guard. The `metadata` stat follows
/// symlinks, so a symlink-to-FIFO is rejected too. (A vanishingly small TOCTOU
/// window remains between the stat and the open - the real threat is a
/// committed / planted special file, which the stat catches.)
fn open_regular(path: &Path) -> std::io::Result<std::fs::File> {
    if !std::fs::metadata(path)?.is_file() {
        return Err(not_regular_file(path));
    }
    std::fs::File::open(path)
}

/// Read up to `TEXT_INSPECT_LEN` bytes from the start of a file. Returned
/// `Ok(None)` means the file was empty; `Err` is propagated I/O error.
pub fn read_prefix(path: &Path) -> std::io::Result<Vec<u8>> {
    read_prefix_n(path, TEXT_INSPECT_LEN)
}

/// Read up to `n` bytes from the start of `path`. Used by rules that
/// only need to inspect a leading window - `executable_has_shebang`
/// (2 bytes for `#!`), `file_starts_with` (`pattern.len()` bytes).
/// Reads less than `n` if the file is shorter; returns the actual byte
/// count in the returned `Vec`'s length.
pub fn read_prefix_n(path: &Path, n: usize) -> std::io::Result<Vec<u8>> {
    let mut file = open_regular(path)?;
    let mut buf = vec![0u8; n];
    let read = file.read(&mut buf)?;
    buf.truncate(read);
    Ok(buf)
}

/// Read up to `n` bytes from the END of `path`. Used by rules that
/// only need to inspect the tail - `file_ends_with` (`pattern.len()`
/// bytes). Returns the actual byte count in the returned `Vec`'s
/// length; fewer than `n` bytes if the file is shorter. Files smaller
/// than `n` are read whole.
pub fn read_suffix_n(path: &Path, n: usize) -> std::io::Result<Vec<u8>> {
    let mut file = open_regular(path)?;
    let len = file.seek(SeekFrom::End(0))?;
    // 32-bit platforms: `usize::MAX < u64::MAX`, so a > 4 GiB
    // file would truncate. `try_from` falls back to reading the
    // requested `n` (which is bounded to a sane caller value)
    // when the conversion fails.
    let to_read = usize::try_from(len).unwrap_or(n).min(n);
    file.seek(SeekFrom::Start(len - to_read as u64))?;
    let mut buf = vec![0u8; to_read];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

/// Classification of a file's contents. Computed lazily - callers check the
/// subset they care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    Text,
    Binary,
}

pub fn classify_bytes(bytes: &[u8]) -> Classification {
    match content_inspector::inspect(bytes) {
        content_inspector::ContentType::BINARY => Classification::Binary,
        _ => Classification::Text,
    }
}

/// Whether `bytes` look like binary content (per `content_inspector`,
/// sampling the same leading window as `file_is_text`). The byte-level
/// fixers consult this and refuse to rewrite a binary file - a line-ending,
/// BOM, final-newline, or prepend/append edit on a binary corrupts it.
pub fn looks_binary(bytes: &[u8]) -> bool {
    let window = &bytes[..bytes.len().min(TEXT_INSPECT_LEN)];
    classify_bytes(window) == Classification::Binary
}

/// Write `bytes` to `path` atomically: write a uniquely-named sibling temp
/// file, copy the original's permissions onto it (so an existing mode -
/// notably the executable bit - survives), `fsync`, then rename it over
/// `path`. Unlike `std::fs::write` (open-truncate-then-write), a crash or
/// I/O error mid-write leaves the original intact rather than truncated or
/// destroyed. The temp is a sibling so the rename is atomic on the same
/// filesystem, and it is cleaned up on failure. (Manual temp, no `tempfile`
/// runtime dependency - matching the extends cache.)
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};
    // Unique sibling name: the pid distinguishes concurrent processes, the
    // atomic counter distinguishes concurrent threads in this process.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    // Write THROUGH a symlink to its (canonical) target, preserving the link —
    // matching the prior `fs::write`/append behavior. A bare temp+rename on the
    // link path would replace the link NODE with a regular file, silently
    // diverging it from its target (common for a symlinked LICENSE / README in
    // a monorepo). `canonicalize` needs the target to exist, which it does:
    // every caller has just read the file via `read_for_fix`.
    let resolved = match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => std::fs::canonicalize(path)?,
        _ => path.to_path_buf(),
    };
    let path = resolved.as_path();
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map_or_else(|| std::path::PathBuf::from("."), Path::to_path_buf);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let stem = path.file_name().and_then(|f| f.to_str()).unwrap_or("tmp");
    let tmp = dir.join(format!(".{stem}.alint-fix.{}.{n}", std::process::id()));
    let write = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        // Preserve the original file's mode when it exists (a rewrite).
        if let Ok(meta) = std::fs::metadata(path) {
            f.set_permissions(meta.permissions())?;
        }
        f.write_all(bytes)?;
        f.sync_all()
    };
    if let Err(e) = write().and_then(|()| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Hard cap on a single whole-file read across the rule/engine read paths.
/// Generous - every realistic manifest / source / generated file is orders of
/// magnitude smaller - yet bounded so a hostile or accidental multi-GB file in
/// a linted repo can't OOM the run. The over-cap *outcome* varies by read
/// site: the cross-file kinds (`registry_paths_resolve`, `pair_hash`, …) and
/// the `for_each` single-literal path yield a clear over-cap violation
/// (fail-closed); the per-file engine/rule loops and the per-file content
/// rules skip the file (fail-open, resilient - a file too big to analyze is
/// left un-analyzed rather than failing the build), logging at `warn` where a
/// `tracing` sink is available (the `alint-core` loops).
///
/// Re-exported from `alint-core` so every read path shares one cap (M3).
pub use alint_core::MAX_ANALYZE_BYTES;

/// Failure of [`read_capped`]: the file exceeds
/// [`MAX_ANALYZE_BYTES`] (carrying its size), or an ordinary I/O
/// error (kept distinct so callers turn "too large" into a clear
/// violation rather than reusing their not-found / skip path).
#[derive(Debug)]
pub enum ReadCapError {
    TooLarge(u64),
    Io(std::io::Error),
}

/// The canonical `"<n> bytes; <cap> MiB cap"` tail shared by every
/// over-cap violation message, so the cap is rendered from
/// [`MAX_ANALYZE_BYTES`] in one place instead of a hardcoded `256`
/// scattered across the rule kinds. Callers supply the verb, e.g.
/// `format!("is too large to analyze ({})", over_cap(n))`.
pub fn over_cap(n: u64) -> String {
    format!("{n} bytes; {} MiB cap", MAX_ANALYZE_BYTES / (1024 * 1024))
}

/// Read a whole file, refusing (via a cheap `metadata` stat, so
/// the oversized bytes are never read) anything larger than
/// `max`. `pub(crate)` so rule-level tests can inject a tiny
/// `max` to exercise the over-cap violation path without
/// materialising a >256 MiB fixture.
pub(crate) fn read_capped_with(path: &Path, max: u64) -> Result<Vec<u8>, ReadCapError> {
    use std::io::Read as _;
    // Fast reject via a cheap stat, so the oversized bytes are never read for
    // the common case. The stat length is also kept as the read buffer's
    // preallocation hint below — it is already computed here, so reusing it is
    // free.
    let stat_len = match std::fs::metadata(path) {
        // Refuse a non-regular file (FIFO/socket/device). Opening a FIFO
        // `O_RDONLY` blocks until a writer appears, so a direct read of a
        // planted in-tree named pipe (reachable via a config-declared path or
        // a symlink the walker followed) would hang the run. The walker skips
        // these at index time; the direct-read helpers must too.
        Ok(m) if !m.is_file() => {
            return Err(ReadCapError::Io(not_regular_file(path)));
        }
        Ok(m) if m.len() > max => return Err(ReadCapError::TooLarge(m.len())),
        Ok(m) => m.len(),
        Err(e) => return Err(ReadCapError::Io(e)),
    };
    // But ALSO bound the actual read: a file that grows past `max` between the
    // stat above and the read must not be slurped in full (TOCTOU / OOM — the
    // M3-F2 class the walker's `read_bounded` already closes). `take(max + 1)`
    // lets us distinguish "exactly at cap" from "over".
    //
    // Preallocate to the stat length: a `Take<File>` has no `read_to_end`
    // fstat-preallocation specialization (bare `File` does), so an empty `Vec`
    // would grow-and-reread — extra `read()` syscalls per file that cost wall
    // clock while staying ~invisible to the Valgrind-Ir gate. The `take(max+1)`
    // is still the sole size bound, so the hint can't force an over-read. See
    // docs/benchmarks/investigations/2026-07-v0.14-s2-harness-artifact/.
    let file = std::fs::File::open(path).map_err(ReadCapError::Io)?;
    let prealloc = usize::try_from(stat_len.min(max.saturating_add(1))).unwrap_or(0);
    let mut buf = Vec::with_capacity(prealloc);
    file.take(max.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(ReadCapError::Io)?;
    if u64::try_from(buf.len()).is_ok_and(|n| n > max) {
        return Err(ReadCapError::TooLarge(buf.len() as u64));
    }
    Ok(buf)
}

/// Whole-file read bounded by [`MAX_ANALYZE_BYTES`]. Used by the
/// cross-file / structured rules for the manifest / source /
/// target / committed-file reads they do themselves.
pub fn read_capped(path: &Path) -> Result<Vec<u8>, ReadCapError> {
    read_capped_with(path, MAX_ANALYZE_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_capped_returns_bytes_under_cap() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, b"hello").unwrap();
        match read_capped(&p) {
            Ok(b) => assert_eq!(b, b"hello"),
            _ => panic!("expected Bytes under the cap"),
        }
    }

    #[test]
    fn read_capped_with_rejects_over_cap_without_reading() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big");
        std::fs::write(&p, b"0123456789").unwrap();
        match read_capped_with(&p, 4) {
            Err(ReadCapError::TooLarge(n)) => assert_eq!(n, 10),
            _ => panic!("a 10-byte file must exceed a 4-byte cap"),
        }
    }

    #[test]
    fn read_capped_missing_path_is_io_error() {
        let dir = tempfile::tempdir().unwrap();
        match read_capped(&dir.path().join("nope")) {
            Err(ReadCapError::Io(_)) => {}
            _ => panic!("a missing path must be an Io error"),
        }
    }

    #[test]
    fn looks_binary_distinguishes_binary_from_text() {
        assert!(looks_binary(b"\x00\x01\x02\x00binary\x00data\x00"));
        assert!(!looks_binary(b"plain text\nmore text\n"));
    }

    #[test]
    fn write_atomic_replaces_content_preserves_mode_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.txt");
        std::fs::write(&p, b"old contents").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        write_atomic(&p, b"new").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
            assert_eq!(
                mode, 0o755,
                "the executable bit must survive an atomic write"
            );
        }
        // No sibling temp file left behind.
        let leaked = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .any(|e| e.file_name().to_string_lossy().contains("alint-fix"));
        assert!(!leaked, "atomic write leaked a temp file");
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_writes_through_a_symlink_preserving_the_link() {
        // Regression: a bare temp+rename would replace the symlink NODE with a
        // regular file, diverging it from its target. write_atomic must write
        // THROUGH to the target (as the old fs::write did), link intact.
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.txt");
        std::fs::write(&target, b"old").unwrap();
        let link = dir.path().join("link.txt");
        symlink(&target, &link).unwrap();
        write_atomic(&link, b"new").unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the symlink must survive (not be clobbered into a regular file)"
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"new", "target updated");
        assert_eq!(std::fs::read(&link).unwrap(), b"new", "reads through link");
    }

    #[test]
    fn direct_read_helpers_reject_a_non_regular_file() {
        // Regression (W5): the direct-read helpers bypass the walker (which skips
        // special files at index time). The `is_file()` guard in `open_regular` /
        // `read_capped_with` must reject a non-regular file as `InvalidInput`
        // *before* `File::open` — that pre-open rejection is exactly what keeps a
        // planted in-tree FIFO from being opened `O_RDONLY` and blocking the whole
        // run forever. A directory is the portable, hang-free proxy for a
        // non-regular file: it has `metadata().is_file() == false` just like a
        // FIFO, so it drives the identical guard branch. (`File::open` on a
        // directory even *succeeds* on Linux — proving the value of rejecting via
        // the stat rather than relying on open to fail.) This crate forbids
        // `unsafe_code` and takes no libc dependency, so it cannot `mkfifo(3)` a
        // real pipe here; the directory case covers the guard faithfully.
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let e = read_prefix_n(d, 16).expect_err("a directory is not a regular file");
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
        let e = read_suffix_n(d, 16).expect_err("a directory is not a regular file");
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
        match read_capped_with(d, 1024) {
            Err(ReadCapError::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput),
            _ => panic!("read_capped_with must reject a directory as an Io error"),
        }
    }
}
