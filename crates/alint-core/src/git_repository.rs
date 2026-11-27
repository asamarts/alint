//! The Git repository alint names to Git.
//!
//! alint finds the repository for a root itself and passes it to every Git
//! command (`--git-dir`, `--work-tree`), so Git never discovers one. Git's own
//! discovery also takes a directory shaped like a bare repository (`HEAD`,
//! `objects/`, `refs/`, `config`) as one, and its `config` may name a command
//! (`core.fsmonitor`); `safe.bareRepository=explicit` turns that off only from
//! Git 2.38. alint never takes a directory itself as a bare repository.
//!
//! Git's discovery refuses a repository another user owns (unless the user's
//! own configuration lists it in `safe.directory`), and an explicit
//! `--git-dir` skips that check, so alint makes it: a repository whose `.git`,
//! work tree or Git directory the current user does not own is used only when
//! Git, asked to discover exactly that repository, accepts it.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{LazyLock, Mutex};

/// The name of the entry that makes a directory the top of a work tree.
const DOT_GIT: &str = ".git";

/// The most a gitfile may hold; one names a single path.
const MAX_GITFILE_BYTES: u64 = 64 * 1024;

/// A Git repository as alint finds one for a directory (see
/// [`find_repository`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Repository {
    /// The Git directory: `<work tree>/.git`, or the directory a `.git` file
    /// names (a linked worktree's, a submodule's). Canonical.
    pub(crate) git_dir: PathBuf,
    /// The top of the work tree: the directory that holds `.git`. Canonical.
    pub(crate) work_tree: PathBuf,
}

impl Repository {
    /// The arguments that name this repository to Git.
    pub(crate) fn args(&self) -> [OsString; 2] {
        let mut git_dir = OsString::from("--git-dir=");
        git_dir.push(&self.git_dir);
        let mut work_tree = OsString::from("--work-tree=");
        work_tree.push(&self.work_tree);
        [git_dir, work_tree]
    }
}

/// The top of the work tree `start` is in: the nearest directory at or above
/// it (an absolute `start`, or one made absolute lexically) that holds a
/// `.git` directory or file the current user owns, in a directory the current
/// user owns ([`owned_by_current_user`]; on Windows, where alint reads no
/// owner, any). A directory is never taken as a bare repository. `None`
/// outside every repository.
///
/// Configuration discovery stops here, and the language server takes it as
/// the root of a repository a workspace folder holds. Another user's `.git`
/// is no top: an empty file planted in a shared directory (`/tmp/.git`)
/// would otherwise end discovery there, without its check of who owns a
/// config above the start.
#[must_use]
pub fn repository_top(start: &Path) -> Option<PathBuf> {
    repository_top_with(start, &owned_by_current_user)
}

/// [`repository_top`] with the ownership check given, so a test can stand in
/// for another user.
#[doc(hidden)]
#[must_use]
pub fn repository_top_with(
    start: &Path,
    owned_by_current_user: &dyn Fn(&Path) -> Option<bool>,
) -> Option<PathBuf> {
    let start = std::path::absolute(start).ok()?;
    start
        .ancestors()
        .find(|dir| {
            holds_git_entry(dir)
                && owned_by_current_user(&dir.join(DOT_GIT)) != Some(false)
                && owned_by_current_user(dir) != Some(false)
        })
        .map(Path::to_path_buf)
}

/// Whether `dir` holds a `.git` directory or file (through a symlink too, as
/// Git reads one).
fn holds_git_entry(dir: &Path) -> bool {
    std::fs::metadata(dir.join(DOT_GIT))
        .is_ok_and(|metadata| metadata.is_dir() || metadata.is_file())
}

/// The repository Git commands for `root` run against: the nearest directory
/// at or above `root` (resolved as the OS resolves it) that holds a `.git`
/// directory, or a `.git` file whose `gitdir:` line names the Git directory.
///
/// As Git's discovery does, the search stops below `ceiling` (a directory
/// above `root` it never reads, from `GIT_CEILING_DIRECTORIES`, which only
/// `alint test` sets) and, on Unix, at a file-system boundary. It never takes
/// a directory as a bare repository. `None` when there is no repository, when
/// a `.git` file is not a gitfile, and when the repository is another user's
/// and Git would not use it either ([`git_accepts`]): alint then runs no Git
/// command, which reads as Git finding no repository.
pub(crate) fn find_repository(
    root: &Path,
    ceiling: Option<&str>,
    environment: Option<&crate::git::ProcessEnvironment>,
) -> Option<Repository> {
    let start = canonical(root)?;
    let ceiling = ceiling_below(&start, ceiling);
    #[cfg(unix)]
    let device = {
        use std::os::unix::fs::MetadataExt as _;
        std::fs::metadata(&start).ok()?.dev()
    };
    for dir in start.ancestors() {
        if ceiling.as_deref() == Some(dir) {
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            if std::fs::metadata(dir).ok()?.dev() != device {
                return None;
            }
        }
        let entry = dir.join(DOT_GIT);
        let Ok(metadata) = std::fs::metadata(&entry) else {
            continue;
        };
        let git_dir = if metadata.is_dir() {
            entry.clone()
        } else if metadata.is_file() {
            read_gitfile(&entry)?
        } else {
            continue;
        };
        let repository = Repository {
            git_dir: canonical(&git_dir)?,
            work_tree: dir.to_path_buf(),
        };
        return trusted(&repository, &entry, environment, &owned_by_current_user)
            .then_some(repository);
    }
    None
}

/// The deepest directory of the colon-separated (`;` on Windows) absolute
/// `ceilings` strictly above `start`, resolved as the OS resolves it, as Git
/// reads `GIT_CEILING_DIRECTORIES`: discovery reads no directory at or above
/// it. A relative entry is ignored.
fn ceiling_below(start: &Path, ceilings: Option<&str>) -> Option<PathBuf> {
    let separator = if cfg!(windows) { ';' } else { ':' };
    ceilings?
        .split(separator)
        .filter(|entry| Path::new(entry).is_absolute())
        .filter_map(|entry| canonical(Path::new(entry)))
        .filter(|ceiling| start.starts_with(ceiling) && start != ceiling)
        .max_by_key(|ceiling| ceiling.components().count())
}

/// The Git directory the gitfile `path` names, as Git reads one: its content
/// is `gitdir: <path>`, without trailing line breaks, relative to the
/// gitfile's directory unless absolute. `None` for any other content or any
/// file larger than [`MAX_GITFILE_BYTES`] or non-regular.
fn read_gitfile(path: &Path) -> Option<PathBuf> {
    let bytes = crate::bounded_file::read_regular_capped(path, MAX_GITFILE_BYTES).ok()?;
    let mut target = bytes.strip_prefix(b"gitdir: ")?;
    while let Some(rest) = target
        .strip_suffix(b"\n")
        .or_else(|| target.strip_suffix(b"\r"))
    {
        target = rest;
    }
    if target.is_empty() {
        return None;
    }
    let target = path_from_git_bytes(target)?;
    Some(if target.is_absolute() {
        target
    } else {
        path.parent()?.join(target)
    })
}

/// Git's verdicts on repositories the current user does not own, by
/// `(work_tree, git_dir)`, so Git is asked once per repository/worktree pair
/// in the host environment. Runs with an explicit `ProcessEnvironment` bypass
/// the cache so sandboxed policy tests never share verdicts with host Git.
static VERDICTS: LazyLock<Mutex<HashMap<(PathBuf, PathBuf), bool>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Whether alint may name `repository`, whose `.git` is `entry`, to Git. One
/// whose `.git`, work tree and Git directory the current user owns may be
/// named, as Git's discovery checks those three. For any other, Git's
/// discovery would have refused it unless the user's own configuration lists
/// it in `safe.directory`, and naming it would skip that check, so Git is
/// asked ([`git_accepts`]). On Windows, where alint reads no owner, Git is
/// always asked. `owned_by_current_user` is the ownership check, which a
/// test stands in for.
fn trusted(
    repository: &Repository,
    entry: &Path,
    environment: Option<&crate::git::ProcessEnvironment>,
    owned_by_current_user: &dyn Fn(&Path) -> Option<bool>,
) -> bool {
    let owned = [
        entry,
        repository.work_tree.as_path(),
        repository.git_dir.as_path(),
    ]
    .into_iter()
    .all(|path| owned_by_current_user(path) == Some(true));
    if owned {
        return true;
    }
    let cache_key = environment
        .is_none()
        .then(|| (repository.work_tree.clone(), repository.git_dir.clone()));
    if let Some(key) = &cache_key
        && let Some(verdict) = VERDICTS
            .lock()
            .ok()
            .and_then(|verdicts| verdicts.get(key).copied())
    {
        return verdict;
    }
    let verdict = git_accepts(repository, environment);
    if let Some(key) = cache_key
        && let Ok(mut verdicts) = VERDICTS.lock()
    {
        verdicts.insert(key, verdict);
    }
    verdict
}

/// Whether Git, asked to discover exactly `repository` (from its work tree,
/// with the work tree's parent as the ceiling), finds that repository: it
/// refuses one another user owns unless `safe.directory` allows it. The work
/// tree holds `.git`, so Git reads it before anything else; the command
/// (`rev-parse --absolute-git-dir`) reads no index and runs nothing a
/// repository's configuration names. This is the one Git command alint runs
/// without naming the repository, and only for a repository the current user
/// does not own.
fn git_accepts(
    repository: &Repository,
    environment: Option<&crate::git::ProcessEnvironment>,
) -> bool {
    let mut command = Command::new("git");
    if let Some(environment) = environment {
        command.env_clear().envs(environment);
    } else {
        #[cfg(test)]
        command.envs(crate::git::TEST_GIT_ENVIRONMENT);
    }
    crate::git::clear_git_repository_env(&mut command, environment);
    if let Some(parent) = repository.work_tree.parent() {
        command.env("GIT_CEILING_DIRECTORIES", parent);
    }
    let output = command
        .args(["-c", "safe.bareRepository=explicit", "-C"])
        .arg(&repository.work_tree)
        .args(["rev-parse", "--absolute-git-dir"])
        .output();
    let Ok(output) = output else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let mut stdout = output.stdout.as_slice();
    while let Some(rest) = stdout
        .strip_suffix(b"\n")
        .or_else(|| stdout.strip_suffix(b"\r"))
    {
        stdout = rest;
    }
    let Some(found) = path_from_git_bytes(stdout) else {
        return false;
    };
    canonical(&found).is_some_and(|found| found == repository.git_dir)
}

#[cfg(unix)]
#[allow(clippy::unnecessary_wraps)] // non-Unix returns None on invalid UTF-8
fn path_from_git_bytes(bytes: &[u8]) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt as _;
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
}

#[cfg(not(unix))]
fn path_from_git_bytes(bytes: &[u8]) -> Option<PathBuf> {
    std::str::from_utf8(bytes).ok().map(PathBuf::from)
}

/// `path` resolved as the OS resolves it, in a form Git reads: on Windows,
/// without the `\\?\` prefix `canonicalize` puts on a drive or UNC path.
pub(crate) fn canonical(path: &Path) -> Option<PathBuf> {
    let path = path.canonicalize().ok()?;
    #[cfg(windows)]
    {
        if let Some(text) = path.to_str() {
            if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
                return Some(PathBuf::from(format!(r"\\{share}")));
            }
            if let Some(drive) = text.strip_prefix(r"\\?\")
                && drive.as_bytes().get(1) == Some(&b':')
            {
                return Some(PathBuf::from(drive));
            }
        }
    }
    Some(path)
}

/// Whether the current user owns `path` itself (a symlink, not what it leads
/// to): its owner is the effective user id, so a file root owns is the
/// current user's when alint runs as root. `Some(false)` for a path that
/// cannot be read; `None` on Windows, where alint reads no owner.
#[must_use]
pub fn owned_by_current_user(path: &Path) -> Option<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        Some(
            std::fs::symlink_metadata(path)
                .is_ok_and(|metadata| metadata.uid() == rustix::process::geteuid().as_raw()),
        )
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

#[cfg(test)]
#[path = "git_repository_tests.rs"]
pub(super) mod tests;
