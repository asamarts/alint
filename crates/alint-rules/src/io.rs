//! Shared I/O helpers for content-reading rules.

use std::io::Read as _;
use std::path::Path;

/// How much of a file to sample when classifying text vs. binary.
pub const TEXT_INSPECT_LEN: usize = 8 * 1024;

/// Read up to `TEXT_INSPECT_LEN` bytes from the start of a file. Returned
/// `Ok(None)` means the file was empty; `Err` is propagated I/O error.
pub fn read_prefix(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; TEXT_INSPECT_LEN];
    let n = file.read(&mut buf)?;
    buf.truncate(n);
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
