//! Best-effort git-tracking integration.
//!
//! `git_tracked_only` rules opt in to filtering matches against the
//! repo's tracked-paths set — i.e. the output of `git ls-files`.
//! That set is computed once per [`Engine::run`](crate::Engine::run)
//! when at least one rule wants it and stashed on the rule
//! [`Context`](crate::Context).
//!
//! The set is *advisory*: alint never refuses to run because a
//! `git` invocation failed. If the directory isn't a git repo, or
//! `git` isn't on PATH, or the repo is empty, the set is `None`
//! and rules that consult it treat every walked entry as
//! "untracked." Rules opting into `git_tracked_only` therefore
//! become silent no-ops in non-git settings — which is the right
//! default for "absence-style" rules whose intent is "don't let
//! this be committed."

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Resolve the repo's tracked-paths set, relative to `root`.
///
/// `root` should be the alint root (the path passed to
/// `alint check`). When `root` IS the git root, this returns the
/// full set of tracked files (no path translation needed). When
/// `root` is a subdirectory of the git root, the implementation
/// uses `git ls-files -- <root>` so the returned paths are still
/// relative to `root`.
///
/// Returns `None` when:
/// - `git` isn't on PATH
/// - `root` (or any ancestor) isn't inside a git repo
/// - the `git` invocation exits non-zero for any other reason
///
/// All these cases produce an empty `Option`, never panic — the
/// caller is responsible for treating `None` as "no tracked-set
/// available" in whatever way makes sense for the calling rule.
pub fn collect_tracked_paths(root: &Path) -> Option<HashSet<PathBuf>> {
    // `-z` separates entries with NUL so paths with newlines or
    // exotic bytes round-trip correctly. `--full-name` would force
    // repo-root-relative paths, but we want CWD-relative — git's
    // default with `-C <dir>` already gives that.
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut out = HashSet::new();
    for chunk in output.stdout.split(|&b| b == 0) {
        if chunk.is_empty() {
            continue;
        }
        let s = std::str::from_utf8(chunk).ok()?;
        out.insert(PathBuf::from(s));
    }
    Some(out)
}

/// Test whether `dir_rel` (a relative-to-root directory path)
/// "exists in git" — defined as: at least one tracked file lives
/// underneath it. Used by `dir_exists` / `dir_absent` when
/// `git_tracked_only: true` is set.
///
/// Linear scan over the tracked set. Acceptable for repos with
/// O(thousands) of files; revisit with a prefix-tree if a future
/// dir-rule benchmark shows it dominate.
///
/// Generic over the hasher so callers can use any
/// `HashSet` flavour without an extra collection allocation.
pub fn dir_has_tracked_files<S>(
    dir_rel: &Path,
    tracked: &std::collections::HashSet<PathBuf, S>,
) -> bool
where
    S: std::hash::BuildHasher,
{
    tracked.iter().any(|p| p.starts_with(dir_rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_returns_none_outside_git() {
        let tmp = tempfile::tempdir().unwrap();
        // `git ls-files` in a non-git directory exits non-zero;
        // we report None. Tests that need a populated set
        // construct a real repo via fixtures elsewhere.
        let result = collect_tracked_paths(tmp.path());
        assert!(result.is_none());
    }

    #[test]
    fn dir_has_tracked_files_walks_prefix() {
        let mut set = HashSet::new();
        set.insert(PathBuf::from("src/main.rs"));
        set.insert(PathBuf::from("README.md"));
        assert!(dir_has_tracked_files(Path::new("src"), &set));
        assert!(!dir_has_tracked_files(Path::new("target"), &set));
        // `src` matches `src/main.rs` via prefix; `tar` does not
        // match `target/foo` because no tracked path is under
        // `tar/`.
        assert!(!dir_has_tracked_files(Path::new("tar"), &set));
    }
}
