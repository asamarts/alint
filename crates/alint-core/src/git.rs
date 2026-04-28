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

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

/// Resolve the set of paths that have changed in the working tree
/// (and optionally relative to a base ref), expressed as paths
/// relative to `root`.
///
/// `base` selects the diff:
/// - `Some("main")` — `git diff --name-only --relative main...HEAD`
///   (three-dot — diff against the merge-base of `main` and
///   `HEAD`). Right shape for PR-check use cases.
/// - `None` — `git ls-files --modified --others --exclude-standard`
///   from `root`. Right shape for pre-commit / local-dev use
///   cases. Untracked-but-not-gitignored files are included so a
///   freshly-added `.env` in the working tree shows up; deleted
///   files are also returned (they're in the diff but not on
///   disk, so the engine's intersect-with-walked-index step
///   filters them out naturally).
///
/// Returns `None` on the same conditions as
/// [`collect_tracked_paths`]: `git` not on PATH, `root` outside
/// a repo, or the invocation exits non-zero. Callers should
/// treat `None` as "no changed-set available" and fall back to
/// a full check (or surface a hard error, depending on intent —
/// `alint check --changed` errors out rather than fall back, so
/// the user's "diff-only" intent isn't silently broken).
pub fn collect_changed_paths(root: &Path, base: Option<&str>) -> Option<HashSet<PathBuf>> {
    // Two distinct invocations: ref-based diff vs. working-tree
    // status. Both emit NUL-separated output so paths with
    // newlines / non-UTF-8 bytes round-trip.
    let output = match base {
        Some(base) => Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["diff", "--name-only", "--relative", "-z"])
            .arg(format!("{base}...HEAD"))
            .output()
            .ok()?,
        None => Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "ls-files",
                "--modified",
                "--others",
                "--exclude-standard",
                "-z",
            ])
            .output()
            .ok()?,
    };
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

/// HEAD's commit message, as a single string with newlines
/// preserved between subject and body. The subject is the first
/// line; everything after the first blank line is the body.
///
/// Returns `None` when:
/// - `git` isn't on PATH
/// - `root` (or any ancestor) isn't inside a git repo
/// - the repo has no commits yet (HEAD is unborn)
/// - the `git log` invocation otherwise exits non-zero
///
/// Used by the `git_commit_message` rule kind. Same advisory
/// posture as the rest of the git module: a non-git workspace
/// silently no-ops the rule rather than raising a hard error.
pub fn head_commit_message(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["log", "-1", "--format=%B"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8(output.stdout).ok()?;
    // `git log --format=%B` appends a trailing newline that's not
    // part of the message body — trim once at the end so length
    // checks against the subject and body don't trip on it.
    Some(raw.trim_end_matches('\n').to_string())
}

/// One line of `git blame --line-porcelain` output: the
/// 1-indexed final line number in the working-tree file, the
/// authoring time of the commit that last touched the line
/// (per `.git-blame-ignore-revs`, when present), and the line
/// content with its trailing newline stripped.
///
/// Used by the `git_blame_age` rule kind to decide whether a
/// pattern-matching line is older than a configured threshold.
/// The line content is preserved as-is so the rule can apply
/// its own regex match.
#[derive(Debug, Clone)]
pub struct BlameLine {
    pub line_number: usize,
    pub author_time: SystemTime,
    pub content: String,
}

/// Run `git blame --line-porcelain` for `rel_path` (relative to
/// `root`) and return one [`BlameLine`] per source line.
///
/// `--line-porcelain` repeats the full per-commit metadata block
/// for every line so we don't have to track the most-recent
/// commit across runs — every line carries its own
/// `author-time`. Honors `.git-blame-ignore-revs` automatically
/// (git applies it before producing porcelain output).
///
/// Returns `None` when:
/// - `git` isn't on PATH
/// - `root` (or any ancestor) isn't inside a git repo
/// - `rel_path` isn't tracked (untracked files have no blame)
/// - the `git blame` invocation otherwise exits non-zero
///
/// Same advisory posture as the rest of the git module: a
/// non-blameable file silently no-ops the rule rather than
/// raising a hard error.
pub fn blame_lines(root: &Path, rel_path: &Path) -> Option<Vec<BlameLine>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["blame", "--line-porcelain", "--"])
        .arg(rel_path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = std::str::from_utf8(&output.stdout).ok()?;
    Some(parse_porcelain(text))
}

/// Parse the `--line-porcelain` output of `git blame`. Pure
/// string-handling so it's exercised by unit tests without
/// shelling out to git.
///
/// Each line of the source file produces one porcelain block:
///
/// ```text
/// <sha> <orig_line> <final_line> <num_lines>
/// author <name>
/// author-mail <<email>>
/// author-time <unix_ts>
/// author-tz <tz>
/// committer …
/// summary …
/// previous … (optional)
/// filename …
/// \t<source line>
/// ```
///
/// We track `author-time` and the trailing tab-prefixed source
/// line; everything else passes through. Lines that don't fit
/// the shape are skipped silently — git blame output is well-
/// defined, but we don't want a parse-error to torpedo a check
/// run on a corrupted repo.
fn parse_porcelain(text: &str) -> Vec<BlameLine> {
    let mut out = Vec::new();
    let mut final_line: Option<usize> = None;
    let mut author_time: Option<SystemTime> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix('\t') {
            // Source line. Emit a BlameLine when we have both a
            // final-line number and an author-time; otherwise
            // skip (malformed block).
            if let (Some(n), Some(t)) = (final_line.take(), author_time.take()) {
                out.push(BlameLine {
                    line_number: n,
                    author_time: t,
                    content: rest.to_string(),
                });
            }
            continue;
        }
        // Header lines start with the 40-hex sha; subsequent
        // lines are `key value` pairs we may care about.
        let mut parts = line.splitn(2, ' ');
        let key = parts.next().unwrap_or("");
        let value = parts.next().unwrap_or("");
        match key {
            "author-time" => {
                if let Ok(secs) = value.parse::<u64>() {
                    author_time = Some(UNIX_EPOCH + Duration::from_secs(secs));
                }
            }
            // SHA header: 40 hex digits + space + 3 numbers. We
            // detect by length and hex-ness; cheap heuristic.
            sha if sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit()) => {
                // The header line is `<sha> <orig> <final> [<num_lines>]`.
                // We want the third field — the final line number.
                // (Already in `value`; split off the `<orig>` first.)
                let mut cols = value.split(' ');
                let _orig = cols.next();
                if let Some(final_str) = cols.next()
                    && let Ok(n) = final_str.parse::<usize>()
                {
                    final_line = Some(n);
                }
            }
            _ => {}
        }
    }
    out
}

/// Per-run cache of `git blame` output, shared across rules so
/// multiple `git_blame_age` rules over overlapping `paths:`
/// re-use the parsed result instead of re-shelling-out.
///
/// Constructed once per [`Engine::run`](crate::Engine::run) when
/// at least one rule reports `wants_git_blame()`. Lookups lock
/// once per (path, miss) — `git blame` itself dwarfs any lock
/// contention (process spawn + read of full file history). The
/// cache also memoises *failures* (file untracked, blame exited
/// non-zero) so a rule iterating thousands of out-of-scope files
/// doesn't re-probe each one repeatedly.
#[derive(Debug)]
pub struct BlameCache {
    root: PathBuf,
    inner: Mutex<HashMap<PathBuf, CacheEntry>>,
}

#[derive(Debug, Clone)]
enum CacheEntry {
    Ok(Arc<Vec<BlameLine>>),
    Failed,
}

impl BlameCache {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Return the blame for `rel_path`, computing once and
    /// caching forever (within this run). `None` means blame
    /// failed for this path — the caller silently no-ops, by
    /// the rule-kind's advisory posture.
    pub fn get(&self, rel_path: &Path) -> Option<Arc<Vec<BlameLine>>> {
        // Hold the lock through the shell-out: the `git blame`
        // process spawn is the dominant cost, so contention from
        // other threads waiting is negligible relative to letting
        // them duplicate the work. If/when we have evidence of
        // hot-loop contention here, switch to a "compute outside
        // the lock with a Pending sentinel" pattern.
        let mut guard = self.inner.lock().expect("blame cache lock poisoned");
        if let Some(entry) = guard.get(rel_path) {
            return match entry {
                CacheEntry::Ok(arc) => Some(Arc::clone(arc)),
                CacheEntry::Failed => None,
            };
        }
        let computed = blame_lines(&self.root, rel_path);
        if let Some(v) = computed {
            let arc = Arc::new(v);
            guard.insert(rel_path.to_path_buf(), CacheEntry::Ok(Arc::clone(&arc)));
            Some(arc)
        } else {
            guard.insert(rel_path.to_path_buf(), CacheEntry::Failed);
            None
        }
    }
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
    fn collect_changed_returns_none_outside_git() {
        let tmp = tempfile::tempdir().unwrap();
        // Both diff modes shell out to git; both should report
        // None outside a repo so callers can decide between
        // hard-error (CLI's `--changed`) and silent fallback.
        assert!(collect_changed_paths(tmp.path(), None).is_none());
        assert!(collect_changed_paths(tmp.path(), Some("main")).is_none());
    }

    #[test]
    fn head_message_returns_none_outside_git() {
        let tmp = tempfile::tempdir().unwrap();
        // Same advisory posture: the `git_commit_message` rule
        // silently no-ops outside a repo rather than failing
        // a check on workspaces that don't track in git yet.
        assert!(head_commit_message(tmp.path()).is_none());
    }

    #[test]
    fn parse_porcelain_two_lines_two_commits() {
        // Two source lines, each in its own porcelain block. The
        // first line is from an old commit (1700000000 = 2023-11-15);
        // the second is from a more recent one (1750000000 =
        // 2025-06-15). Both blocks repeat the full metadata per
        // line-porcelain semantics.
        let porcelain = "\
abcd1234abcd1234abcd1234abcd1234abcd1234 1 1 1
author Old Author
author-mail <old@example.com>
author-time 1700000000
author-tz +0000
committer Old Author
committer-mail <old@example.com>
committer-time 1700000000
committer-tz +0000
summary first commit
filename src/main.rs
\told line content
ef01ef01ef01ef01ef01ef01ef01ef01ef01ef01 2 2 1
author New Author
author-mail <new@example.com>
author-time 1750000000
author-tz +0000
committer New Author
committer-mail <new@example.com>
committer-time 1750000000
committer-tz +0000
summary recent commit
filename src/main.rs
\tnew line content
";
        let lines = parse_porcelain(porcelain);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].line_number, 1);
        assert_eq!(lines[0].content, "old line content");
        assert_eq!(
            lines[0].author_time,
            UNIX_EPOCH + Duration::from_secs(1_700_000_000)
        );
        assert_eq!(lines[1].line_number, 2);
        assert_eq!(lines[1].content, "new line content");
        assert_eq!(
            lines[1].author_time,
            UNIX_EPOCH + Duration::from_secs(1_750_000_000)
        );
    }

    #[test]
    fn parse_porcelain_handles_previous_marker() {
        // The optional `previous <sha> <name>` line shows up when
        // the line was rewritten — the parser must not get
        // confused by it.
        let porcelain = "\
abcd1234abcd1234abcd1234abcd1234abcd1234 5 5 1
author X
author-mail <x@example.com>
author-time 1700000000
author-tz +0000
committer X
committer-mail <x@example.com>
committer-time 1700000000
committer-tz +0000
summary did a thing
previous 1111111111111111111111111111111111111111 src/old.rs
filename src/main.rs
\tline body
";
        let lines = parse_porcelain(porcelain);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].line_number, 5);
        assert_eq!(lines[0].content, "line body");
    }

    #[test]
    fn parse_porcelain_skips_blocks_missing_metadata() {
        // A block whose author-time line is corrupt (non-numeric)
        // should drop that line rather than panic. The next valid
        // block still emits.
        let porcelain = "\
abcd1234abcd1234abcd1234abcd1234abcd1234 1 1 1
author X
author-time not-a-number
filename a.rs
\tbroken
ef01ef01ef01ef01ef01ef01ef01ef01ef01ef01 2 2 1
author Y
author-time 1700000000
filename a.rs
\tworks
";
        let lines = parse_porcelain(porcelain);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].content, "works");
    }

    #[test]
    fn blame_lines_returns_none_outside_git() {
        let tmp = tempfile::tempdir().unwrap();
        // No repo, so blame on anything (existing or not) fails.
        let result = blame_lines(tmp.path(), Path::new("missing.rs"));
        assert!(result.is_none());
    }

    #[test]
    fn blame_cache_memoises_failure() {
        // Calling `get` twice on a non-existent file in a
        // non-git directory must short-circuit on the second
        // call. We can't observe the cache directly from outside,
        // but we can verify both calls return None and the cache
        // ends up with an entry for the path.
        let tmp = tempfile::tempdir().unwrap();
        let cache = BlameCache::new(tmp.path().to_path_buf());
        assert!(cache.get(Path::new("missing.rs")).is_none());
        assert!(cache.get(Path::new("missing.rs")).is_none());
        let guard = cache.inner.lock().unwrap();
        assert!(matches!(
            guard.get(Path::new("missing.rs")),
            Some(CacheEntry::Failed)
        ));
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
