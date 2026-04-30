//! Shared I/O helpers for content-reading rules.

use std::io::{Read as _, Seek, SeekFrom};
use std::path::Path;

/// How much of a file to sample when classifying text vs. binary.
pub const TEXT_INSPECT_LEN: usize = 8 * 1024;

/// Read up to `TEXT_INSPECT_LEN` bytes from the start of a file. Returned
/// `Ok(None)` means the file was empty; `Err` is propagated I/O error.
pub fn read_prefix(path: &Path) -> std::io::Result<Vec<u8>> {
    read_prefix_n(path, TEXT_INSPECT_LEN)
}

/// Read up to `n` bytes from the start of `path`. Used by rules that
/// only need to inspect a leading window — `executable_has_shebang`
/// (2 bytes for `#!`), `file_starts_with` (`pattern.len()` bytes).
/// Reads less than `n` if the file is shorter; returns the actual byte
/// count in the returned `Vec`'s length.
pub fn read_prefix_n(path: &Path, n: usize) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; n];
    let read = file.read(&mut buf)?;
    buf.truncate(read);
    Ok(buf)
}

/// Read up to `n` bytes from the END of `path`. Used by rules that
/// only need to inspect the tail — `file_ends_with` (`pattern.len()`
/// bytes). Returns the actual byte count in the returned `Vec`'s
/// length; fewer than `n` bytes if the file is shorter. Files smaller
/// than `n` are read whole.
pub fn read_suffix_n(path: &Path, n: usize) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
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

/// Classification of a file's contents. Computed lazily — callers check the
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
