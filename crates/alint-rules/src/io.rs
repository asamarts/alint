//! Shared I/O helpers for content-reading rules.

use std::io::{Read as _, Seek, SeekFrom};
use std::path::Path;

/// How much of a file to sample when classifying text vs. binary.
pub const TEXT_INSPECT_LEN: usize = 8 * 1024;

/// The `InvalidInput` error a direct-read helper returns for a non-regular
/// file, so the message is uniform across the three read helpers. Like an OS
/// error, it names no path: callers attach the repo-relative one, and the
/// helpers only see the absolute path, which would make findings depend on
/// where the repository is checked out.
fn not_regular_file() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a regular file")
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
        return Err(not_regular_file());
    }
    std::fs::File::open(path)
}

/// The fail-loud outcome of a per-file content rule's own read (the
/// rule-major `evaluate` loop used by `alint fix`). Because these paths came
/// from the repository index, every I/O error, including a mid-walk removal,
/// prevents a policy verdict and becomes a `resource_read` analysis failure.
pub(crate) fn io_error_violation(rel: &Path, err: &std::io::Error) -> alint_core::Violation {
    alint_core::unreadable_file_violation(rel, err)
}

/// Structural failure conversion for a bounded whole-file read.
pub(crate) fn read_cap_error_violation(rel: &Path, err: &ReadCapError) -> alint_core::Violation {
    match err {
        ReadCapError::TooLarge(size) => alint_core::too_large_file_violation(rel, *size),
        ReadCapError::Io(e) => io_error_violation(rel, e),
    }
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
    read_prefix_from_reader(&mut file, n)
}

fn read_prefix_from_reader<R: std::io::Read>(reader: &mut R, n: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0u8; n];
    let mut total = 0;
    while total < n {
        match reader.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(read) => total += read,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    buf.truncate(total);
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

/// Leading byte window scanned for non-text C0/DEL control-byte density.
/// NUL bytes are still scanned across the full slice passed to
/// [`classify_bytes`] (up to [`TEXT_INSPECT_LEN`]) and across the entire file
/// in [`looks_binary`].
const CONTROL_SCAN_LEN: usize = 256;

/// Binary magic signatures that can appear in a NUL-free prefix.
const BINARY_MAGIC_PREFIXES: &[&[u8]] = &[
    b"%PDF",         // PDF document
    b"\x89PNG",      // PNG image
    b"\xFF\xD8\xFF", // JPEG image
    b"\x7FELF",      // ELF executable/object
];

/// True when `byte` is an ASCII control byte that does not normally appear in
/// plain text (excluding tab, LF, VT, FF, CR, and ANSI ESC). High bytes
/// (`0x80..=0xFF`) are intentionally NOT flagged so UTF-8 multibyte text
/// (including a sequence split at `TEXT_INSPECT_LEN`) and Latin-1 / Windows-1252
/// text remain classified as text.
#[inline]
const fn is_non_text_control(byte: u8) -> bool {
    matches!(byte, 0x01..=0x08 | 0x0E..=0x1A | 0x1C..=0x1F | 0x7F)
}

#[inline]
fn has_binary_magic(bytes: &[u8]) -> bool {
    // Fast-path first-byte guard avoids 4 prefix compares on normal text.
    matches!(bytes.first(), Some(b'%' | 0x89 | 0xFF | 0x7F))
        && BINARY_MAGIC_PREFIXES
            .iter()
            .any(|magic| bytes.starts_with(magic))
}

/// Check whether the leading [`CONTROL_SCAN_LEN`] bytes contain a binary-like
/// density of non-text C0/DEL control bytes.
///
/// Uses exact, zero-borrow / zero-carry 64-bit SWAR formulas that test 8 bytes
/// per iteration for any byte `< 0x20` (other than `\n`) or `0x7F`. Ordinary LF
/// source files (including indented lines after `\n`) return `false` without
/// entering the per-byte counter.
fn has_suspicious_control_density(sample: &[u8]) -> bool {
    const LO: u64 = 0x0101_0101_0101_0101;
    const HI: u64 = 0x8080_8080_8080_8080;
    const SPACE: u64 = 0x20 * LO;
    const NL: u64 = (b'\n' as u64) * LO;

    let prefix = &sample[..sample.len().min(CONTROL_SCAN_LEN)];
    let (chunks, remainder) = prefix.as_chunks::<8>();
    let mut maybe_ctl = 0u64;
    for chunk in chunks {
        let w = u64::from_ne_bytes(*chunk);
        // Zero-borrow / zero-carry per-lane checks (exact on both endiannesses):
        //   lt20   : bit 7 set in lane i iff byte i < 0x20
        //   is_nl  : bit 7 set in lane i iff byte i == b'\n'
        //   is_del : bit 7 set in lane i iff byte i == 0x7F
        let lt20 = !((w | HI).wrapping_sub(SPACE)) & !w & HI;
        let xnl = w ^ NL;
        let is_nl = !((xnl | HI).wrapping_sub(LO)) & !xnl & HI;
        let is_del = ((w & !HI).wrapping_add(LO)) & !w & HI;
        maybe_ctl |= (lt20 & !is_nl) | is_del;
    }
    if maybe_ctl == 0 && remainder.iter().all(|&b| !is_non_text_control(b)) {
        return false;
    }
    // Terminal logs / OSC-8 hyperlinks use ESC (`\x1b`) paired with BEL
    // (`\x07`) terminators; exempt BEL when ESC is present in the window.
    let has_esc = prefix.contains(&0x1B);
    let suspicious = prefix
        .iter()
        .filter(|&&b| is_non_text_control(b) && !(has_esc && b == 0x07))
        .count();
    (suspicious >= 2 && suspicious.saturating_mul(20) > prefix.len())
        || suspicious.saturating_mul(2) > prefix.len()
}

/// Classify a non-empty, NUL-free, BOM-stripped sample.
#[inline]
fn classify_nul_free_sample(sample: &[u8]) -> Classification {
    if has_binary_magic(sample) || has_suspicious_control_density(sample) {
        Classification::Binary
    } else {
        Classification::Text
    }
}

/// Strip leading UTF-8 BOMs only when the first byte matches `0xEF` (fast path
/// for the common non-BOM case).
#[inline]
fn strip_leading_utf8_boms(bytes: &[u8]) -> &[u8] {
    if bytes.first() == Some(&0xEF) {
        &bytes[crate::no_bom::utf8_bom_run_len(bytes)..]
    } else {
        bytes
    }
}

/// True when `bytes` starts with a UTF-16 or UTF-32 BOM (caller has already
/// stripped leading UTF-8 BOMs).
#[inline]
fn starts_with_wide_bom(bytes: &[u8]) -> bool {
    matches!(bytes.first(), Some(0xFF | 0xFE | 0x00)) && crate::no_bom::detect_bom(bytes).is_some()
}

/// Classify `bytes` as text or binary using an internal heuristic:
/// - Empty buffers and UTF-16/UTF-32 BOMs are classified as text (matching
///   `file_is_text`'s contract; `looks_binary` separately rejects wide BOMs
///   before calling this so byte-level fixers never corrupt wide encodings).
/// - Leading UTF-8 BOMs are skipped before inspecting the payload so a BOM
///   cannot mask binary content.
/// - Any NUL byte in `bytes` (SIMD `memchr` over the whole passed slice)
///   classifies as binary.
/// - Known NUL-free binary magic headers (`%PDF`, PNG, JPEG, ELF) classify as
///   binary.
/// - High density of non-text C0/DEL control bytes in the leading window
///   classifies as binary without penalizing UTF-8 or Latin-1 high bytes.
pub fn classify_bytes(bytes: &[u8]) -> Classification {
    if bytes.is_empty() {
        return Classification::Text;
    }
    let after_utf8_bom = strip_leading_utf8_boms(bytes);
    if after_utf8_bom.is_empty() {
        return Classification::Text;
    }
    // Wide-encoding BOMs legitimately carry NUL bytes in ASCII code units.
    if starts_with_wide_bom(after_utf8_bom) {
        return Classification::Text;
    }
    if memchr::memchr(0x00, after_utf8_bom).is_some() {
        return Classification::Binary;
    }
    classify_nul_free_sample(after_utf8_bom)
}

/// Whether `bytes` look like binary content. The byte-level fixers (and their
/// detectors) consult this and refuse to touch a binary file -- a reorder,
/// reindent, line-ending, BOM, final-newline, or prepend/append edit on a binary
/// corrupts it.
///
/// A NUL byte ANYWHERE is the definitive binary marker, so the WHOLE slice is
/// scanned for one via SIMD `memchr`.
///
/// A file that opens with a UTF-16 / UTF-32 byte-order mark (including after
/// stacked UTF-8 BOMs) counts too, NUL or not: its code units are 2 / 4 bytes
/// wide, so every byte-level edit these fixers make (append a `\n`, drop a
/// `0x20` byte, strip the 2-byte mark without transcoding) desynchronizes or
/// corrupts it -- and UTF-16 text in non-Latin scripts (`中文` is `2D 4E 87 65`)
/// carries no NUL for the check above to see.
///
/// For NUL-free, non-wide-BOM inputs, the leading window is checked for binary
/// magic headers and non-text control-byte density without repeating the NUL or
/// BOM scans.
pub fn looks_binary(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    if memchr::memchr(0x00, bytes).is_some() {
        return true;
    }
    let after_utf8_bom = strip_leading_utf8_boms(bytes);
    if after_utf8_bom.is_empty() {
        return false;
    }
    if starts_with_wide_bom(after_utf8_bom) {
        return true;
    }
    classify_nul_free_sample(after_utf8_bom) == Classification::Binary
}

/// How a security-posture character scan (`no_bidi_controls`,
/// `no_zero_width_chars`) treats a file, decided from its bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CharScan {
    /// Not binary-looking: scan (lossily; invalid bytes count as one U+FFFD).
    Text,
    /// Binary-looking (a NUL, a UTF-16/32 BOM, ...) but VALID UTF-8: scan it.
    /// NUL is valid UTF-8, so a crafted source file cannot hide a control
    /// behind one NUL byte. Findings are reported but not auto-fixed (the
    /// byte-strip fixers refuse binary content).
    BinaryUtf8,
    /// Binary-looking AND invalid UTF-8 (a PNG, a font, a JPEG): skipped. A
    /// lossy decode of random bytes manufactures controls out of noise
    /// (`D8 9C` decodes to U+061C), so scanning these is all false positives.
    Skip,
}

/// Strong binary markers (a NUL byte anywhere, a wide UTF-16/UTF-32 BOM, or a
/// known binary magic header). Used by [`char_scan_mode`] so that an attacker
/// cannot evade Trojan-Source / secret scanning by planting a handful of C0
/// control bytes and a single Latin-1 byte in an otherwise executable source
/// comment.
fn has_strong_binary_markers(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    if memchr::memchr(0x00, bytes).is_some() {
        return true;
    }
    let after_utf8_bom = strip_leading_utf8_boms(bytes);
    if after_utf8_bom.is_empty() {
        return false;
    }
    starts_with_wide_bom(after_utf8_bom) || has_binary_magic(after_utf8_bom)
}

/// Classify `bytes` for a [`CharScan`].
pub(crate) fn char_scan_mode(bytes: &[u8]) -> CharScan {
    if !looks_binary(bytes) {
        CharScan::Text
    } else if std::str::from_utf8(bytes).is_ok() || !has_strong_binary_markers(bytes) {
        // Scan valid UTF-8 (NUL cannot hide Trojan-Source controls) AND
        // NUL-free/non-magic control-dense files (a few C0 controls + a Latin-1
        // byte in a source comment cannot hide controls either), while keeping
        // them non-fixable because `looks_binary` is true.
        CharScan::BinaryUtf8
    } else {
        CharScan::Skip
    }
}

/// `text` without a leading byte-order mark. Editors hide the BOM, so a
/// column or width counted from the start of line 1 must not include it.
pub(crate) fn without_leading_bom(text: &str) -> &str {
    text.strip_prefix('\u{FEFF}').unwrap_or(text)
}

/// Iterate physical lines separated by LF, CRLF, or CR without allocating.
/// Each item carries the byte offset of its first character in `text`.
pub(crate) fn physical_lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    alint_core::physical_lines(text)
}

/// Find the first char of `bytes` (decoded as UTF-8, lossily) for which
/// `pred(char, is_first_char)` holds, returning `(1-based line, 1-based
/// column in chars, char)`. Each maximal invalid byte sequence counts as ONE
/// U+FFFD, exactly as `String::from_utf8_lossy` would decode it, but nothing is
/// allocated -- a big file is walked in place.
pub(crate) fn first_char_where(
    bytes: &[u8],
    mut pred: impl FnMut(char, bool) -> bool,
) -> Option<(usize, usize, char)> {
    let mut line = 1usize;
    let mut col = 1usize;
    let mut first = true;
    let mut previous_was_cr = false;
    for chunk in bytes.utf8_chunks() {
        let invalid = (!chunk.invalid().is_empty()).then_some('\u{FFFD}');
        for c in chunk.valid().chars().chain(invalid) {
            if pred(c, first) {
                return Some((line, col, c));
            }
            // A leading BOM is invisible in every editor, so it takes no column.
            let leading_bom = first && c == '\u{FEFF}';
            first = false;
            match c {
                '\r' => {
                    line += 1;
                    col = 1;
                    previous_was_cr = true;
                }
                '\n' if previous_was_cr => previous_was_cr = false,
                '\n' => {
                    line += 1;
                    col = 1;
                }
                _ => {
                    if !leading_bom {
                        col += 1;
                    }
                    previous_was_cr = false;
                }
            }
        }
    }
    None
}

/// Atomic whole-file write, re-exported from `alint-core` so the fixers and the
/// engine's compose flush share one implementation. See
/// [`alint_core::write_atomic`].
pub use alint_core::write_atomic;

/// Hard cap on a single whole-file read across the rule/engine read paths.
/// Generous - every realistic manifest / source / generated file is orders of
/// magnitude smaller - yet bounded so a hostile or accidental multi-GB file in
/// a linted repo cannot OOM the run. An over-cap file produces a typed
/// `resource_read` analysis failure wherever its bytes are required for a
/// policy verdict.
///
/// Re-exported from `alint-core` so every read path shares one cap (M3).
pub use alint_core::MAX_ANALYZE_BYTES;

/// Failure of [`read_capped`]: the file exceeds
/// [`MAX_ANALYZE_BYTES`] (carrying its size), or an ordinary I/O
/// error. Callers preserve this distinction in the analysis-failure message.
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
            return Err(ReadCapError::Io(not_regular_file()));
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
    fn classify_bytes_handles_utf8_latin1_ansi_and_split_multibyte_windows() {
        assert_eq!(classify_bytes(b""), Classification::Text);
        assert_eq!(
            classify_bytes("Simple UTF-8 string \u{2614} 日本語\n".as_bytes()),
            Classification::Text
        );
        // ANSI escape sequences and normal whitespace stay text.
        assert_eq!(
            classify_bytes(b"\x1b[32mPASS\x1b[0m\tline\r\n\x0cpage\n"),
            Classification::Text
        );
        // Latin-1 / Windows-1252 high bytes stay text (structured_path relies on this).
        let latin1 = b"name=caf\xe9\ncity=M\xfcnchen\n";
        assert_eq!(classify_bytes(latin1), Classification::Text);
        assert!(!looks_binary(latin1));
        // A multibyte UTF-8 sequence split right at TEXT_INSPECT_LEN stays text.
        let mut split_utf8 = vec![b'a'; TEXT_INSPECT_LEN - 1];
        split_utf8.extend_from_slice("é".as_bytes()); // 2 bytes straddling boundary
        assert_eq!(
            classify_bytes(&split_utf8[..TEXT_INSPECT_LEN]),
            Classification::Text
        );
        assert!(!looks_binary(&split_utf8));
    }

    #[test]
    fn classify_bytes_and_looks_binary_handle_boms_and_nul_free_binary_fixtures() {
        // UTF-8 BOM + text is text; UTF-8 BOM + NUL or PDF magic is binary.
        assert_eq!(classify_bytes(b"\xEF\xBB\xBFhello\n"), Classification::Text);
        assert_eq!(
            classify_bytes(b"\xEF\xBB\xBF\x00binary"),
            Classification::Binary
        );
        assert_eq!(
            classify_bytes(b"\xEF\xBB\xBF%PDF-1.7\n"),
            Classification::Binary
        );
        // Wide BOMs: classify_bytes treats them as text (file_is_text contract),
        // while looks_binary refuses them so byte-level fixers never corrupt them.
        for wide in [
            b"\xFF\xFEh\x00i\x00".as_slice(),
            b"\xFE\xFF\x00h\x00i".as_slice(),
            b"\xFF\xFE\x00\x00h\x00\x00\x00".as_slice(),
            b"\x00\x00\xFE\xFF\x00\x00\x00h".as_slice(),
            // NUL-free CJK UTF-16 and UTF-8 BOM stacked before UTF-16 BOM.
            b"\xFF\xFE\x2D\x4E\x87\x65".as_slice(),
            b"\xEF\xBB\xBF\xFF\xFE\x2D\x4E".as_slice(),
        ] {
            assert_eq!(classify_bytes(wide), Classification::Text);
            assert!(looks_binary(wide));
        }
        // NUL between 1 KiB and 8 KiB is now caught by classify_bytes too.
        let mut mid_nul = vec![b'x'; 2048];
        mid_nul[1500] = 0;
        assert_eq!(classify_bytes(&mid_nul), Classification::Binary);
        // NUL-free binary magic prefixes.
        for magic in [
            b"%PDF-1.7\n1 0 obj\n".as_slice(),
            b"\x89PNG\r\n\x1a\nIHDR".as_slice(),
            b"\xFF\xD8\xFF\xE0JFIF".as_slice(),
            b"\x7FELF\x02\x01\x01".as_slice(),
        ] {
            assert_eq!(classify_bytes(magic), Classification::Binary);
            assert!(looks_binary(magic));
        }
        // NUL-free control-heavy binary data is classified binary, while a single
        // stray control byte in normal text stays text.
        let nul_free_binary: Vec<u8> = (1u8..=255).cycle().take(512).collect();
        assert_eq!(classify_bytes(&nul_free_binary), Classification::Binary);
        assert!(looks_binary(&nul_free_binary));
        assert_eq!(classify_bytes(b"\x01"), Classification::Binary);
        assert_eq!(
            classify_bytes(b"hello\x07world\nmore text here\n"),
            Classification::Text
        );
        // char_scan_mode preserves Text / BinaryUtf8 / Skip semantics.
        assert_eq!(char_scan_mode(b"hello\n"), CharScan::Text);
        assert_eq!(char_scan_mode(b"hello\x00world\n"), CharScan::BinaryUtf8);
        assert_eq!(char_scan_mode(b"\x89PNG\r\n\x1a\n\xFF"), CharScan::Skip);
    }

    #[test]
    fn swar_control_scan_matches_exact_count_across_all_byte_lanes() {
        // Every non-text control byte at every lane 0..8 triggers exact counting
        // and classifies binary when density exceeds threshold.
        for ctl in (1u8..=0x1F)
            .chain(std::iter::once(0x7F))
            .filter(|&b| is_non_text_control(b))
        {
            for lane in 0..8 {
                let mut buf = [b'a'; 16];
                buf[lane] = ctl;
                buf[lane + 8] = ctl; // 2/16 = 12.5% > 5% -> Binary
                assert_eq!(
                    classify_bytes(&buf),
                    Classification::Binary,
                    "ctl=0x{ctl:02X} lane={lane}"
                );
            }
        }
        // Adjacent 0xFF + 0x7E (carry), '\n' + ' ' (borrow), '\n' + '\x0B', and
        // OSC-8 hyperlinks (ESC + BEL) stay Text.
        let tricky_text = b"pub fn f() {\n    let x = 1;\n\x0b}\n\xFF\x7E\xFF\x7E\r\n\tindented\n";
        assert_eq!(classify_bytes(tricky_text), Classification::Text);
        assert!(!looks_binary(tricky_text));
        let osc8 = b"\x1b]8;;https://alint.org\x07ok\x1b]8;;\x07\n";
        assert_eq!(classify_bytes(osc8), Classification::Text);
        // Short files: single stray control in a 3-byte file stays Text;
        // majority controls (1/1, 2/3) classify Binary; remainder lengths 7/8/9.
        assert_eq!(classify_bytes(b"a\x07\n"), Classification::Text);
        assert_eq!(classify_bytes(b"\x01\x02a"), Classification::Binary);
        assert_eq!(classify_bytes(b"abcdefg"), Classification::Text);
        assert_eq!(classify_bytes(b"abcdefgh"), Classification::Text);
        assert_eq!(classify_bytes(b"abcdefghi"), Classification::Text);
        // Stacked UTF-8 BOM only, and UTF-8 BOM stacked before UTF-32 BE BOM.
        assert_eq!(
            classify_bytes(b"\xEF\xBB\xBF\xEF\xBB\xBF"),
            Classification::Text
        );
        assert!(!looks_binary(b"\xEF\xBB\xBF\xEF\xBB\xBF"));
        assert_eq!(
            classify_bytes(b"\xEF\xBB\xBF\x00\x00\xFE\xFF\x00\x00\x00a"),
            Classification::Text
        );
        assert!(looks_binary(b"\xEF\xBB\xBF\x00\x00\xFE\xFF\x00\x00\x00a"));
    }

    #[test]
    fn char_scan_mode_does_not_skip_nul_free_c0_plus_latin1_trojan_source() {
        // Security regression (#612 audit): 7 C0 bytes + 1 Latin-1 byte in a
        // comment makes `looks_binary` true and `from_utf8` false, but MUST NOT
        // let an attacker bypass Trojan-Source / secret scanning (`CharScan::Skip`).
        let mut crafted = b"# coding: latin-1 \x01\x02\x03\x04\x05\x06\x07\xe9\n".to_vec();
        crafted.extend_from_slice("x = '\u{202E}evil'\n".as_bytes());
        assert!(looks_binary(&crafted));
        assert!(std::str::from_utf8(&crafted).is_err());
        assert_eq!(char_scan_mode(&crafted), CharScan::BinaryUtf8);
    }

    #[test]
    fn looks_binary_detects_a_nul_past_the_inspect_window() {
        // AUDIT R2: a NUL byte ANYWHERE means binary -- not just within the leading
        // `TEXT_INSPECT_LEN` window. A file that is clean text through the window but
        // has a NUL later must still be refused, or a content fixer corrupts it.
        let mut buf = vec![b'a'; TEXT_INSPECT_LEN * 2]; // clean text well past the window
        for chunk in buf.chunks_mut(64) {
            if let Some(last) = chunk.last_mut() {
                *last = b'\n';
            }
        }
        assert!(
            !looks_binary(&buf),
            "an all-text buffer past the window must NOT be binary"
        );
        let nul_pos = TEXT_INSPECT_LEN + 100; // past the leading window
        buf[nul_pos] = 0;
        assert!(
            looks_binary(&buf),
            "a NUL past the inspect window must still be detected as binary"
        );
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
    fn write_atomic_preserves_setuid_and_setgid_bits() {
        // Round-5 audit: `write_atomic` set the temp file's mode BEFORE writing,
        // and a `write()` clears S_ISUID/S_ISGID (the kernel's
        // `should_remove_suid`), silently stripping setuid/setgid from a fixed
        // file. The plain executable bit survived, which is why the test above
        // missed it. Now the mode is applied AFTER the last write, preserving the
        // full mode word. `2775` = setgid + rwxrwxr-x is the tell (setgid is
        // cleared on write only when group-executable).
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("helper.sh");
        std::fs::write(&p, b"echo old").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o6755)).unwrap();
        // Re-read: the FS may already mask bits we can't set unprivileged; assert
        // against what actually stuck so the test is meaningful either way.
        let before = std::fs::metadata(&p).unwrap().permissions().mode() & 0o7777;
        write_atomic(&p, b"echo new").unwrap();
        let after = std::fs::metadata(&p).unwrap().permissions().mode() & 0o7777;
        assert_eq!(
            after, before,
            "the full mode word (incl. setuid/setgid) must survive an atomic write"
        );
        // And specifically: if setgid stuck before, it must still be set.
        if before & 0o2000 != 0 {
            assert_ne!(after & 0o2000, 0, "setgid must survive an atomic write");
        }
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
        // The message names no path: callers attach the repo-relative one, and an
        // absolute path would make findings depend on the checkout location.
        let e = read_prefix_n(d, 16).expect_err("a directory is not a regular file");
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(e.to_string(), "not a regular file");
        let e = read_suffix_n(d, 16).expect_err("a directory is not a regular file");
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(e.to_string(), "not a regular file");
        match read_capped_with(d, 1024) {
            Err(ReadCapError::Io(e)) => {
                assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
                assert_eq!(e.to_string(), "not a regular file");
            }
            _ => panic!("read_capped_with must reject a directory as an Io error"),
        }
    }

    #[test]
    fn a_leading_bom_takes_no_column() {
        let find = |b: &[u8]| first_char_where(b, |c, _| c == 'x');
        assert_eq!(find(b"\xEF\xBB\xBFabx"), find(b"abx"));
        assert_eq!(find(b"\xEF\xBB\xBFabx"), Some((1, 3, 'x')));
        // Only at the very start: a BOM mid-line is a real character.
        assert_eq!(find(b"a\xEF\xBB\xBFx"), Some((1, 3, 'x')));
    }

    #[test]
    fn character_locations_recognize_cr_and_crlf_line_endings() {
        let find = |b: &[u8]| first_char_where(b, |c, _| c == 'x');
        assert_eq!(find(b"a\rb\r\nx"), Some((3, 1, 'x')));
    }

    #[test]
    fn read_prefix_handles_short_reads_and_eintr() {
        struct FlakyReader<'a> {
            data: &'a [u8],
            pos: usize,
            calls: usize,
        }
        impl std::io::Read for FlakyReader<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.calls += 1;
                if self.calls.is_multiple_of(2) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "EINTR",
                    ));
                }
                if self.pos >= self.data.len() {
                    return Ok(0);
                }
                buf[0] = self.data[self.pos];
                self.pos += 1;
                Ok(1)
            }
        }
        let mut reader = FlakyReader {
            data: b"hello world",
            pos: 0,
            calls: 0,
        };
        assert_eq!(read_prefix_from_reader(&mut reader, 5).unwrap(), b"hello");
    }
}
