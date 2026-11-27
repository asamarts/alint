//! Whole-file reads of a regular file, bounded.

use std::io::Read as _;
use std::path::Path;

/// The whole of `path` when it is a regular file (after symlinks) of at most
/// `cap` bytes.
///
/// Anything else is refused before it is opened: opening a FIFO blocks until a
/// writer appears, and a device such as `/dev/zero` never ends. The read itself
/// stops one byte past `cap`, so a file that grows meanwhile cannot make it
/// unbounded.
///
/// # Errors
///
/// The error of the `metadata` call or of the read; `InvalidInput` for a file
/// that is not a regular file and `FileTooLarge` for one larger than `cap`. No
/// error names the path: the caller does, in the form it shows paths in.
pub fn read_regular_capped(path: &Path, cap: u64) -> std::io::Result<Vec<u8>> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    if metadata.len() > cap {
        return Err(too_large(cap));
    }
    // Open with O_NONBLOCK on Unix and re-verify on the opened descriptor so a
    // TOCTOU swap to a FIFO/device or growth past `cap` cannot block or OOM.
    let file = crate::walker::open_nonblocking(path)?;
    let open_meta = file.metadata()?;
    if !open_meta.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    if open_meta.len() > cap {
        return Err(too_large(cap));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(open_meta.len()).unwrap_or(0));
    file.take(cap.saturating_add(1)).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).is_ok_and(|len| len > cap) {
        return Err(too_large(cap));
    }
    Ok(bytes)
}

/// `cap` for a message: in MiB when it is a whole number of them.
#[must_use]
pub fn describe_cap(cap: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    if cap >= MIB && cap.is_multiple_of(MIB) {
        format!("{} MiB", cap / MIB)
    } else {
        format!("{cap} bytes")
    }
}

fn too_large(cap: u64) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::FileTooLarge,
        format!("larger than {}", describe_cap(cap)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_regular_file_within_the_cap_and_refuses_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("file");
        std::fs::write(&file, b"12345").unwrap();
        assert_eq!(read_regular_capped(&file, 5).unwrap(), b"12345");
        let error = read_regular_capped(&file, 4).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::FileTooLarge);
        assert_eq!(error.to_string(), "larger than 4 bytes");
        let error = read_regular_capped(tmp.path(), 4).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        let error = read_regular_capped(&tmp.path().join("missing"), 4).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        // A file past the cap is refused. Twice the cap, so a regression that
        // read it whole takes 64 MiB of this process, not the host's memory
        // (sparse on Unix; NTFS allocates it).
        let sparse = tmp.path().join("sparse");
        std::fs::File::create(&sparse)
            .unwrap()
            .set_len(64 << 20)
            .unwrap();
        let error = read_regular_capped(&sparse, 32 << 20).unwrap_err();
        assert_eq!(error.to_string(), "larger than 32 MiB");
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_device_without_reading_it() {
        let error = read_regular_capped(Path::new("/dev/zero"), 4).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_fifo_without_blocking() {
        let tmp = tempfile::tempdir().unwrap();
        let fifo = tmp.path().join("pipe");
        let ok = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .is_ok_and(|s| s.success());
        if !ok || !fifo.exists() {
            return;
        }
        let error = read_regular_capped(&fifo, 64).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }
}
