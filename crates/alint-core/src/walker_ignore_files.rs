//! The ignore files the walk reads, held to regular files of bounded size.
//!
//! The owned parallel walker (`walker_parallel`) discovers each directory's
//! `.ignore`, `.gitignore` and `.git` directly from `std::fs::read_dir` and
//! reads them through [`read_from_entry`] (open-then-`fstat` for regular files
//! without an extra path-based `stat` probe) or [`read_by_path`] (pre-`stat`
//! before opening for symlinks, `info/exclude`, `commondir`, parent dirs, and
//! global excludes so opening a FIFO never blocks and `/dev/zero` is refused
//! before open).
//!
//! Policy (ADR-0048 decision 5b, #568):
//! - Regular files of at most [`MAX_IGNORE_FILE_BYTES`] (32 MiB) are read.
//! - Inside the checked root (and oversize global excludes), a non-regular,
//!   oversize, or unreadable ignore file fails the walk via
//!   [`IgnoreRefusal::into_error`].
//! - Above the checked root, the same condition is skipped with a visible
//!   warning via [`IgnoreRefusal::into_warning`].

use std::io::Read as _;
use std::path::{Path, PathBuf};

/// The most of one ignore file, `.git` file or `commondir` the walk reads: as
/// much as a config ([`crate::MAX_STRUCTURED_BYTES`], 32 MiB).
pub(super) const MAX_IGNORE_FILE_BYTES: u64 = crate::MAX_STRUCTURED_BYTES as u64;

/// Outcome of reading one ignore file (or `.git` / `commondir` on the way to
/// `info/exclude`) under the regular-file and 32 MiB bounds.
pub(super) enum IgnoreFileRead {
    /// Regular file read completely within [`MAX_IGNORE_FILE_BYTES`].
    Bytes(Vec<u8>),
    /// Missing (`NotFound`) or a directory (harmlessly ignored everywhere).
    Absent,
    /// Non-regular file (FIFO, device, socket, or symlink to one), oversize
    /// (> 32 MiB), or unreadable (I/O error such as `PermissionDenied`).
    Refused(IgnoreRefusal),
}

/// Why the walk refused to read an ignore file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IgnoreRefusalKind {
    /// Special file (FIFO, device, socket, or symlink to one).
    NonRegular,
    /// Exceeds [`MAX_IGNORE_FILE_BYTES`] (32 MiB).
    TooLarge,
    /// I/O error such as `PermissionDenied` or `ELOOP`.
    Io,
}

/// A file the walk refused to read, with its path, category, and human-readable
/// reason.
pub(super) struct IgnoreRefusal {
    path: PathBuf,
    kind: IgnoreRefusalKind,
    reason: String,
}

impl IgnoreRefusal {
    pub(super) fn non_regular(path: PathBuf) -> Self {
        Self {
            path,
            kind: IgnoreRefusalKind::NonRegular,
            reason: "it is not a regular file".to_string(),
        }
    }

    pub(super) fn too_large(path: PathBuf) -> Self {
        Self {
            path,
            kind: IgnoreRefusalKind::TooLarge,
            reason: too_large_reason(),
        }
    }

    pub(super) fn io(path: PathBuf, err: impl std::fmt::Display) -> Self {
        Self {
            path,
            kind: IgnoreRefusalKind::Io,
            reason: err.to_string(),
        }
    }

    pub(super) fn kind(&self) -> IgnoreRefusalKind {
        self.kind
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn reason(&self) -> &str {
        &self.reason
    }

    pub(super) fn into_error(self) -> crate::Error {
        crate::Error::Other(format!(
            "the walk does not read the ignore file {}: {}; alint reads an ignore file \
             (and a `.git` file or `commondir` on the way to one) only when it is a \
             regular file of at most {}",
            self.path.display(),
            self.reason,
            crate::bounded_file::describe_cap(MAX_IGNORE_FILE_BYTES)
        ))
    }

    pub(super) fn into_warning(self) -> String {
        format!(
            "skipping ignore file {} above root: {}; alint reads an ignore file \
             (and a `.git` file or `commondir` on the way to one) only when it is a \
             regular file of at most {}",
            self.path.display(),
            self.reason,
            crate::bounded_file::describe_cap(MAX_IGNORE_FILE_BYTES)
        )
    }
}

/// Read an ignore file discovered in a `read_dir` listing using its listing
/// `FileType` to avoid a path-based `stat` probe on regular files (#573).
///
/// - Regular file (`ft.is_file()`): open directly with `O_NONBLOCK` on Unix
///   so a TOCTOU swap to a FIFO cannot hang, `fstat` (`file.metadata()`) to
///   verify type and size, then read in bounded 8 KiB blocks.
/// - Symlink (`ft.is_symlink()`): delegate to [`read_by_path`] (`metadata`
///   first so a symlink to a FIFO never blocks and `/dev/zero` is never opened).
/// - Directory (`ft.is_dir()`): [`IgnoreFileRead::Absent`].
/// - Special file (FIFO, socket, char/block device): refused immediately with
///   zero `open` or `stat` syscalls.
pub(super) fn read_from_entry(path: PathBuf, ft: std::fs::FileType) -> IgnoreFileRead {
    if ft.is_dir() {
        return IgnoreFileRead::Absent;
    }
    if ft.is_symlink() {
        return read_by_path(path);
    }
    if !ft.is_file() {
        return IgnoreFileRead::Refused(IgnoreRefusal::non_regular(path));
    }
    open_and_read_bounded(path)
}

/// Read an ignore file by path when no listing `FileType` is available (a
/// symlink, `info/exclude`, `commondir`, a parent directory above the root, or
/// the global excludes file). Stats first so opening a FIFO never blocks and
/// known devices are never opened.
pub(super) fn read_by_path(path: PathBuf) -> IgnoreFileRead {
    let metadata = match std::fs::metadata(&path) {
        Ok(meta) => meta,
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return IgnoreFileRead::Absent;
        }
        Err(err) => return IgnoreFileRead::Refused(IgnoreRefusal::io(path, err)),
    };
    if metadata.is_dir() {
        return IgnoreFileRead::Absent;
    }
    if !metadata.is_file() {
        return IgnoreFileRead::Refused(IgnoreRefusal::non_regular(path));
    }
    if metadata.len() > MAX_IGNORE_FILE_BYTES {
        return IgnoreFileRead::Refused(IgnoreRefusal::too_large(path));
    }
    open_and_read_bounded(path)
}

/// Open `path` without blocking on a FIFO (using `O_NONBLOCK` on Unix so a
/// TOCTOU swap between `read_dir`/`stat` and `open` never stalls).
#[cfg(unix)]
fn open_nonblocking(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NONBLOCK.bits().cast_signed())
        .open(path)
}

#[cfg(not(unix))]
fn open_nonblocking(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

/// Open `path`, verify its open handle via `fstat` (`file.metadata()`), and
/// read in fixed 8 KiB blocks up to [`MAX_IGNORE_FILE_BYTES`].
///
/// Reading in fixed 8192-byte blocks serves two purposes:
/// - Virtual files under `/proc` (such as `/proc/self/pagemap`, which reports
///   `len == 0` in `fstat` and rejects non-multiple-of-8 read lengths) are
///   bounded cleanly on the single open handle without a second open.
/// - Concurrent file growth past the cap stops one chunk past the cap.
pub(super) fn open_and_read_bounded(path: PathBuf) -> IgnoreFileRead {
    let mut file = match open_nonblocking(&path) {
        Ok(file) => file,
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return IgnoreFileRead::Absent;
        }
        Err(err) => return IgnoreFileRead::Refused(IgnoreRefusal::io(path, err)),
    };
    let meta = match file.metadata() {
        Ok(meta) => meta,
        Err(err) => return IgnoreFileRead::Refused(IgnoreRefusal::io(path, err)),
    };
    if meta.is_dir() {
        return IgnoreFileRead::Absent;
    }
    if !meta.is_file() {
        return IgnoreFileRead::Refused(IgnoreRefusal::non_regular(path));
    }
    if meta.len() > MAX_IGNORE_FILE_BYTES {
        return IgnoreFileRead::Refused(IgnoreRefusal::too_large(path));
    }
    let prealloc = usize::try_from(meta.len().min(MAX_IGNORE_FILE_BYTES)).unwrap_or(0);
    let mut buf = Vec::with_capacity(prealloc);
    let mut block = [0u8; 8192];
    loop {
        match file.read(&mut block) {
            Ok(0) => return IgnoreFileRead::Bytes(buf),
            Ok(count) => {
                let next_len = (buf.len() as u64).saturating_add(count as u64);
                if next_len > MAX_IGNORE_FILE_BYTES {
                    return IgnoreFileRead::Refused(IgnoreRefusal::too_large(path));
                }
                buf.extend_from_slice(&block[..count]);
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(err) => return IgnoreFileRead::Refused(IgnoreRefusal::io(path, err)),
        }
    }
}

fn too_large_reason() -> String {
    format!(
        "it is larger than {}",
        crate::bounded_file::describe_cap(MAX_IGNORE_FILE_BYTES)
    )
}

/// The bytes of `path`, a file `WalkSkips` reads to decide what it leaves out,
/// when it is a regular file within [`MAX_IGNORE_FILE_BYTES`]. `None` for any
/// other, as for one that is absent or unreadable.
pub(super) fn read(path: &Path) -> Option<Vec<u8>> {
    match read_by_path(path.to_path_buf()) {
        IgnoreFileRead::Bytes(bytes) => Some(bytes),
        IgnoreFileRead::Absent | IgnoreFileRead::Refused(_) => None,
    }
}

/// The first line of `path` ([`read`]), as `BufRead::lines` gives it: `None`
/// when there is none or it is not UTF-8.
pub(super) fn first_line(path: &Path) -> Option<String> {
    first_line_from_bytes(&read(path)?)
}

/// The first line of already-read bytes, as `BufRead::lines` gives it: `None`
/// when there is none or it is not UTF-8.
pub(super) fn first_line_from_bytes(bytes: &[u8]) -> Option<String> {
    use std::io::BufRead as _;
    bytes.lines().next()?.ok()
}

/// The global excludes file as a matcher, for the walk: read only when it is a
/// regular file within [`MAX_IGNORE_FILE_BYTES`], failing on an oversize
/// regular file and skipping an absent, non-regular (such as `/dev/null`), or
/// unreadable file consistently before and after open (matching `ignore` while
/// never blocking on a FIFO or reading `/dev/zero`).
pub(super) fn load_global_excludes() -> crate::Result<Option<ignore::gitignore::Gitignore>> {
    let Some(path) = ignore::gitignore::gitconfig_excludes_path() else {
        return Ok(None);
    };
    match read_by_path(path.clone()) {
        IgnoreFileRead::Bytes(content) => {
            let Some(cwd) = std::env::current_dir().ok() else {
                return Ok(None);
            };
            Ok(super::gitignore_from(&cwd, &path, &content))
        }
        IgnoreFileRead::Absent => Ok(None),
        IgnoreFileRead::Refused(refused) => match refused.kind() {
            IgnoreRefusalKind::TooLarge => Err(refused.into_error()),
            IgnoreRefusalKind::NonRegular | IgnoreRefusalKind::Io => Ok(None),
        },
    }
}

/// The global excludes file as a matcher for `WalkSkips`, read through [`read`].
pub(super) fn global_excludes() -> Option<ignore::gitignore::Gitignore> {
    let path = ignore::gitignore::gitconfig_excludes_path()?;
    let content = read(&path)?;
    let cwd = std::env::current_dir().ok()?;
    super::gitignore_from(&cwd, &path, &content)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Duration;

    /// `f` on another thread: its result, or `None` past `deadline`.
    fn within<T: Send + 'static>(
        deadline: Duration,
        f: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(f());
        });
        receiver.recv_timeout(deadline).ok()
    }

    /// A sparse file past the cap: twice it, so a regression that read it
    /// whole takes 64 MiB of this process, not the host's memory.
    fn sparse(path: &Path) {
        std::fs::File::create(path)
            .unwrap()
            .set_len(2 * MAX_IGNORE_FILE_BYTES)
            .unwrap();
    }

    /// alint's own reads of the files the walk reads (`WalkSkips`, which
    /// `fix` and `git_tracked_only` consult) take only a regular file within
    /// the cap, as the walk does: a sparse `.gitignore` past it, and a linked
    /// worktree whose `commondir` is `/dev/zero`, are passed over within the
    /// deadline instead of read.
    #[test]
    fn alints_own_reads_take_only_a_regular_file_within_the_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        sparse(&sub.join(".gitignore"));
        std::os::unix::fs::symlink("/dev/zero", sub.join(".ignore")).unwrap();
        let git_dir = tmp.path().join("main.git/worktrees/sub");
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::write(sub.join(".git"), format!("gitdir: {}\n", git_dir.display())).unwrap();
        std::os::unix::fs::symlink("/dev/zero", git_dir.join("commondir")).unwrap();

        assert_eq!(read(&sub.join(".gitignore")), None);
        assert_eq!(read(&sub.join(".ignore")), None);
        assert_eq!(first_line(&git_dir.join("commondir")), None);
        let skipped = within(Duration::from_secs(30), move || {
            let opts = crate::WalkOptions::default().with_global_git_excludes(false);
            super::super::would_walk_skip(&root, &opts, Path::new("sub/x.log"), false, &[])
        });
        assert_eq!(skipped, Some(false));
    }

    #[test]
    fn read_by_path_accepts_regular_files_skips_dirs_and_refuses_devices_and_oversize() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("repo");
        std::fs::create_dir_all(dir.join(".git/info")).unwrap();
        assert!(matches!(
            read_by_path(dir.join(".git/info/exclude")),
            IgnoreFileRead::Absent
        ));
        std::fs::write(dir.join(".git/info/exclude"), "*.log\n").unwrap();
        assert!(matches!(
            read_by_path(dir.join(".git/info/exclude")),
            IgnoreFileRead::Bytes(ref b) if b == b"*.log\n"
        ));
        std::fs::remove_file(dir.join(".git/info/exclude")).unwrap();
        std::os::unix::fs::symlink("/dev/zero", dir.join(".git/info/exclude")).unwrap();
        match read_by_path(dir.join(".git/info/exclude")) {
            IgnoreFileRead::Refused(refused) => {
                assert_eq!(refused.path(), dir.join(".git/info/exclude"));
                assert_eq!(refused.reason(), "it is not a regular file");
            }
            _ => panic!("expected Refused for /dev/zero symlink"),
        }
        // A `.gitignore` that is a directory is harmlessly absent.
        std::fs::create_dir(dir.join(".gitignore")).unwrap();
        assert!(matches!(
            read_by_path(dir.join(".gitignore")),
            IgnoreFileRead::Absent
        ));
        sparse(&dir.join(".ignore"));
        match read_by_path(dir.join(".ignore")) {
            IgnoreFileRead::Refused(refused) => {
                assert_eq!(refused.path(), dir.join(".ignore"));
                assert_eq!(refused.reason(), "it is larger than 32 MiB");
            }
            _ => panic!("expected Refused for sparse file"),
        }
    }

    /// A regular file that gives its size as zero is counted by reading it in
    /// 8 KiB blocks: an empty `.gitignore` passes, `/proc/self/environ` passes,
    /// and `/proc/self/pagemap` is refused as past the cap.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_file_that_gives_its_size_as_zero_is_counted_by_reading_it() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("repo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".gitignore"), "").unwrap();
        assert!(matches!(
            read_by_path(dir.join(".gitignore")),
            IgnoreFileRead::Bytes(ref b) if b.is_empty()
        ));
        std::fs::remove_file(dir.join(".gitignore")).unwrap();
        std::os::unix::fs::symlink("/proc/self/environ", dir.join(".gitignore")).unwrap();
        assert!(matches!(
            read_by_path(dir.join(".gitignore")),
            IgnoreFileRead::Bytes(_)
        ));
        std::os::unix::fs::symlink("/proc/self/pagemap", dir.join(".ignore")).unwrap();
        match read_by_path(dir.join(".ignore")) {
            IgnoreFileRead::Refused(refused) => {
                assert_eq!(refused.path(), dir.join(".ignore"));
                assert_eq!(refused.reason(), "it is larger than 32 MiB");
            }
            _ => panic!("expected Refused for /proc/self/pagemap"),
        }
    }
}
