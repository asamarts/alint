//! The ignore files the walk reads, held to regular files of bounded size.
//!
//! `ignore` reads the `.ignore` and `.gitignore` of every directory it lists
//! and of every directory above the root, a repository's `info/exclude`
//! (through a linked worktree's `.git` file and its `commondir`), and the
//! global excludes file, whole and through symlinks. A `.gitignore` that is a
//! symlink to `/dev/zero` would have that read take memory without end, and
//! one to a FIFO would block it. So before `ignore` reads a directory's files
//! the walk checks them ([`refused_in`]) and stops with an error naming the
//! first one it will not read; alint's own reads of the same files ([`read`])
//! take only what passes.

use std::path::{Path, PathBuf};

/// The most of one ignore file, `.git` file or `commondir` the walk reads: as
/// much as a config ([`crate::MAX_STRUCTURED_BYTES`]).
pub(super) const MAX_IGNORE_FILE_BYTES: u64 = crate::MAX_STRUCTURED_BYTES as u64;

/// The bytes of `path`, a file the walk reads to decide what it leaves out,
/// when it is a regular file within [`MAX_IGNORE_FILE_BYTES`]. `None` for any
/// other, as for one that is absent or unreadable.
pub(super) fn read(path: &Path) -> Option<Vec<u8>> {
    crate::bounded_file::read_regular_capped(path, MAX_IGNORE_FILE_BYTES).ok()
}

/// The first line of `path` ([`read`]), as `BufRead::lines` gives it: `None`
/// when there is none or it is not UTF-8.
pub(super) fn first_line(path: &Path) -> Option<String> {
    use std::io::BufRead as _;
    read(path)?.lines().next()?.ok()
}

/// A file `ignore` would read that the walk refuses, and why.
pub(super) struct Refused {
    path: PathBuf,
    reason: String,
}

impl Refused {
    /// Why `ignore` must not read `path`: something there that is neither a
    /// regular file nor a directory (a device never ends; opening a FIFO
    /// blocks), or a regular file larger than [`MAX_IGNORE_FILE_BYTES`]. A
    /// regular file that gives its size as zero is read to count what it
    /// holds ([`reads_past_cap`]): a `/proc` file does, whatever it holds, and
    /// `/proc/self/pagemap` holds gigabytes. `None` when nothing is there, it
    /// cannot be read (which `ignore` tolerates), it is a directory (which
    /// `ignore` fails to read, harmlessly) or it is a regular file within the
    /// cap.
    fn check(path: PathBuf) -> Option<Self> {
        let metadata = std::fs::metadata(&path).ok()?;
        let reason = if metadata.is_dir() {
            return None;
        } else if !metadata.is_file() {
            "it is not a regular file".to_string()
        } else if metadata.len() > MAX_IGNORE_FILE_BYTES
            || (metadata.len() == 0 && reads_past_cap(&path))
        {
            format!(
                "it is larger than {}",
                crate::bounded_file::describe_cap(MAX_IGNORE_FILE_BYTES)
            )
        } else {
            return None;
        };
        Some(Self { path, reason })
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
}

/// Whether reading `path` yields more than [`MAX_IGNORE_FILE_BYTES`], counted
/// in whole blocks without keeping what it reads (`/proc/self/pagemap` fails a
/// read of a length that is not a multiple of 8). An empty file costs one
/// read; a read that fails within the cap is no refusal, as `ignore`
/// tolerates it.
fn reads_past_cap(path: &Path) -> bool {
    use std::io::Read as _;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut block = [0; 8192];
    let mut read = 0_u64;
    while read <= MAX_IGNORE_FILE_BYTES {
        match file.read(&mut block) {
            Ok(0) => return false,
            Ok(count) => read = read.saturating_add(u64::try_from(count).unwrap_or(u64::MAX)),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return false,
        }
    }
    true
}

/// The first file among those `ignore` reads for `dir` that the walk refuses
/// ([`Refused::check`]): its `.ignore` and `.gitignore`, and, when it holds a
/// `.git`, the files on the way to its repository's `info/exclude`
/// ([`super::git_exclude_file`]) and that file. Each is checked before
/// anything after it is read.
pub(super) fn refused_in(dir: &Path) -> Option<Refused> {
    for name in [".ignore", ".gitignore"] {
        if let Some(refused) = Refused::check(dir.join(name)) {
            return Some(refused);
        }
    }
    let dot_git = dir.join(".git");
    let metadata = std::fs::metadata(&dot_git).ok()?;
    if metadata.is_file() {
        if let Some(refused) = Refused::check(dot_git.clone()) {
            return Some(refused);
        }
        let git_dir = PathBuf::from(first_line(&dot_git)?.strip_prefix("gitdir: ")?);
        if let Some(refused) = Refused::check(git_dir.join("commondir")) {
            return Some(refused);
        }
    } else if !metadata.is_dir() {
        return None;
    }
    Refused::check(super::git_exclude_file(dir)?)
}

/// The first file `ignore` reads before the walk starts that the walk refuses:
/// those of the root ([`refused_in`]) and of every directory above it, which
/// `ignore` reads whether or not it applies them (ADR-0029), and the global
/// excludes file when the walk uses one (`ignore` reads it only when it is a
/// regular file). `canonical_root` is `root` canonicalized, which the walk
/// already holds.
pub(super) fn refused_before_walk(
    root: &Path,
    canonical_root: Option<&Path>,
    global_excludes: bool,
) -> Option<Refused> {
    refused_in(root)
        .or_else(|| canonical_root?.ancestors().skip(1).find_map(refused_in))
        .or_else(|| {
            let path = global_excludes
                .then(ignore::gitignore::gitconfig_excludes_path)
                .flatten()?;
            std::fs::metadata(&path)
                .is_ok_and(|metadata| metadata.is_file())
                .then(|| Refused::check(path))
                .flatten()
        })
}

/// The global excludes file as a matcher, read as `ignore` reads it
/// (`Gitignore::global`) but through [`read`].
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
    fn each_file_on_the_way_to_info_exclude_is_checked_before_it_is_read() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("repo");
        std::fs::create_dir_all(dir.join(".git/info")).unwrap();
        assert!(refused_in(&dir).is_none());
        std::fs::write(dir.join(".git/info/exclude"), "*.log\n").unwrap();
        assert!(refused_in(&dir).is_none());
        std::fs::remove_file(dir.join(".git/info/exclude")).unwrap();
        std::os::unix::fs::symlink("/dev/zero", dir.join(".git/info/exclude")).unwrap();
        let refused = refused_in(&dir).unwrap();
        assert_eq!(refused.path, dir.join(".git/info/exclude"));
        assert_eq!(refused.reason, "it is not a regular file");
        // A `.gitignore` that is a directory is no refusal: `ignore` fails to
        // read it and goes on, as before.
        std::fs::remove_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir(dir.join(".gitignore")).unwrap();
        assert!(refused_in(&dir).is_none());
        sparse(&dir.join(".ignore"));
        let refused = refused_in(&dir).unwrap();
        assert_eq!(refused.path, dir.join(".ignore"));
        assert_eq!(refused.reason, "it is larger than 32 MiB");
    }

    /// A regular file that gives its size as zero is counted by reading it:
    /// an empty `.gitignore` passes, and `/proc/self/pagemap`, which gives its
    /// size as zero and holds gigabytes, is refused as past the cap.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_file_that_gives_its_size_as_zero_is_counted_by_reading_it() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("repo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".gitignore"), "").unwrap();
        assert!(refused_in(&dir).is_none());
        std::fs::remove_file(dir.join(".gitignore")).unwrap();
        std::os::unix::fs::symlink("/proc/self/environ", dir.join(".gitignore")).unwrap();
        assert!(refused_in(&dir).is_none());
        std::os::unix::fs::symlink("/proc/self/pagemap", dir.join(".ignore")).unwrap();
        let refused = refused_in(&dir).unwrap();
        assert_eq!(refused.path, dir.join(".ignore"));
        assert_eq!(refused.reason, "it is larger than 32 MiB");
    }
}
