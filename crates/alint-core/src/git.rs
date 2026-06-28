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
        Some(base) => {
            // Defense-in-depth, matching `diff_name_only`: reject a `base`
            // starting with `-` explicitly (treat as "no changed-set"), in
            // addition to the `--end-of-options` guard below.
            if base.starts_with('-') {
                return None;
            }
            Command::new("git")
                .arg("-C")
                .arg(root)
                .args(["diff", "--name-only", "--relative", "-z"])
                // `--end-of-options` so a `base`/`since` starting with `-`
                // can't be parsed as a git OPTION (e.g. `--output=…`, which
                // would write/truncate an arbitrary file).
                .arg("--end-of-options")
                .arg(format!("{base}...HEAD"))
                .output()
                .ok()?
        }
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

/// Like [`collect_changed_paths`] with a `base` ref, but distinguishes
/// "not a git repo" (silent) from "ref doesn't resolve" (hard error) —
/// the contract `scope_filter.changed_since:` needs. Returns the set of
/// paths changed in `<since>...HEAD` (three-dot, merge-base diff —
/// matching `alint check --changed`), relative to `root`.
///
/// - `Ok(Some(set))` — resolved.
/// - `Ok(None)`       — not a git repo / `git` not on PATH (silent).
/// - `Err(BadRange)`  — in a repo, but `<since>` didn't resolve (e.g.
///   a shallow-clone gotcha). The caller surfaces a fetch-depth hint.
pub fn collect_changed_paths_checked(
    root: &Path,
    since: &str,
) -> Result<Option<HashSet<PathBuf>>, CommitRangeError> {
    diff_name_only(root, since, None)
}

/// Like [`collect_changed_paths_checked`] but restricted to a git
/// `--diff-filter` (e.g. `"A"` for added paths, `"M"` for modified).
/// Same posture: `Ok(None)` outside a repo / `git` missing,
/// `Err(BadRange)` on an unresolvable `since`. Used by
/// `changeset_requires_path` to find files *added* in `<since>...HEAD`.
pub fn collect_changed_paths_filtered(
    root: &Path,
    since: &str,
    diff_filter: &str,
) -> Result<Option<HashSet<PathBuf>>, CommitRangeError> {
    diff_name_only(root, since, Some(diff_filter))
}

/// Shared `git diff --name-only --relative -z <since>...HEAD`
/// (optionally `--diff-filter=<…>`), with the git-repo probe and NUL
/// parsing both [`collect_changed_paths_checked`] and
/// [`collect_changed_paths_filtered`] need.
fn diff_name_only(
    root: &Path,
    since: &str,
    diff_filter: Option<&str>,
) -> Result<Option<HashSet<PathBuf>>, CommitRangeError> {
    // Probe: are we in a git repo at all? If not, silent None —
    // matching the advisory posture of the rest of this module.
    let Ok(probe) = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--git-dir"])
        .output()
    else {
        return Ok(None);
    };
    if !probe.status.success() {
        return Ok(None);
    }
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(root)
        .args(["diff", "--name-only", "--relative", "-z"]);
    if since.starts_with('-') {
        return Err(CommitRangeError::BadRange {
            stderr: format!("`since` must not start with '-' (got {since:?})"),
        });
    }
    if let Some(filter) = diff_filter {
        cmd.arg(format!("--diff-filter={filter}"));
    }
    // `--end-of-options`: a config-controlled `since` starting with `-`
    // (e.g. `--output=…`) must never be parsed as a git OPTION — that
    // would write/truncate an arbitrary out-of-tree file. Force it into
    // the revision-range slot.
    cmd.arg("--end-of-options");
    cmd.arg(format!("{since}...HEAD"));
    let Ok(output) = cmd.output() else {
        return Ok(None);
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(CommitRangeError::BadRange { stderr });
    }
    let mut out = HashSet::new();
    for chunk in output.stdout.split(|&b| b == 0) {
        if chunk.is_empty() {
            continue;
        }
        let Ok(s) = std::str::from_utf8(chunk) else {
            return Ok(None);
        };
        out.insert(PathBuf::from(s));
    }
    Ok(Some(out))
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

/// HEAD as a full [`CommitRecord`] — abbreviated SHA, author name +
/// email, and the message. Used by the commit-validation family's
/// HEAD-only mode (`since:` unset), where rules like
/// `git_commit_author_allowlist` need the author and the SHA in
/// addition to the message.
///
/// Returns `None` on the same conditions as [`head_commit_message`]
/// (no `git`, not a repo, unborn HEAD), so the rule silently no-ops.
/// Uses the same NUL-separated `--format` encoding as
/// [`commit_messages_in_range`] so a single commit round-trips
/// through the shared commit-log parser.
pub fn head_commit_record(root: &Path) -> Option<CommitRecord> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "log",
            "-1",
            "--abbrev-commit",
            "--format=%h%x00%an%x00%ae%x00%B%x1e",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_commit_log(&output.stdout).into_iter().next()
}

/// One commit in a `<since>..HEAD` range, as returned by
/// [`commit_messages_in_range`]. `sha` is the abbreviated SHA from
/// `git log --abbrev-commit` (typically 7 chars; git auto-extends if
/// the prefix is ambiguous in the local repo). `message` is the full
/// commit message (subject + body, separated by a blank line) with
/// the trailing newline that `git log --format=%B` appends already
/// trimmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitRecord {
    pub sha: String,
    pub message: String,
    /// Author name (`git log %an`). Empty when synthesised for a
    /// HEAD-only check that didn't capture authorship.
    pub author_name: String,
    /// Author email (`git log %ae`).
    pub author_email: String,
}

/// Errors that distinguish "git is here but the range is invalid"
/// from "git isn't here at all." The rule layer uses this to hard-
/// fail on misconfiguration (a bad `since:` ref, often a shallow-
/// clone gotcha in CI) while silently no-op'ing in non-git
/// directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitRangeError {
    /// The `<since>` ref doesn't resolve, or the range itself is
    /// rejected by git (e.g. `bad revision`). Carries the stderr
    /// `git` produced so the caller can include it in its error.
    /// Typically caused by:
    /// - typo in the ref name
    /// - shallow clone that doesn't have the ref in local objects
    ///   (the most common CI gotcha; `actions/checkout` defaults to
    ///   `fetch-depth: 1`)
    BadRange { stderr: String },
}

/// Enumerate commits reachable from `HEAD` but not from `since`,
/// i.e. the standard `<since>..HEAD` range, oldest first.
///
/// `since` is anything `git rev-parse` accepts: a 40-char SHA, an
/// abbreviated SHA, a branch (`origin/main`), a tag (`v1.2.3`), or
/// a relative ref (`HEAD~5`).
///
/// `include_merges` controls whether merge commits in the range are
/// returned. Defaults to `false` at the call site for PR workflows
/// (where the merge commit at HEAD is the synthetic
/// `actions/checkout`-produced one) but the caller decides.
///
/// Returns:
/// - `Ok(Some(records))` on success. The vec may be empty if the
///   range itself is empty (`since` == HEAD on a force-push PR, or
///   no non-merge commits in the range).
/// - `Ok(None)` if `git` isn't on PATH or `root` isn't inside a git
///   repo. Matches the advisory posture of the rest of this module;
///   rules that consult this helper silently no-op in non-git
///   settings.
/// - `Err(CommitRangeError::BadRange)` if `git` is present and the
///   repo is valid but the range couldn't be resolved. Rules
///   surface this as a hard error so the user sees the
///   misconfiguration instead of a confused empty range.
///
/// Implementation note: uses `--format=%h%x00%B%x1e` so the SHA and
/// the message are NUL-separated (NUL never appears in either) and
/// commits are RS-separated (RS = U+001E, "record separator", which
/// also doesn't appear in well-formed commit text). The compound
/// encoding is robust against commit messages containing arbitrary
/// text — including em dashes, blank lines, and Unicode shenanigans
/// — without resorting to fragile line-counting.
pub fn commit_messages_in_range(
    root: &Path,
    since: &str,
    include_merges: bool,
) -> Result<Option<Vec<CommitRecord>>, CommitRangeError> {
    // First check `git rev-parse` (no range syntax) confirms we're
    // in a git repo at all. If not, this returns Ok(None) (the
    // "silent" branch) without surfacing the BadRange error,
    // matching head_commit_message's posture.
    let probe = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--git-dir"])
        .output();
    let Ok(probe) = probe else {
        return Ok(None);
    };
    if !probe.status.success() {
        return Ok(None);
    }

    // Now invoke `git log <since>..HEAD`. If THIS fails, it's a bad
    // ref / shallow-clone case, not a "no git" case — bubble the
    // BadRange error.
    if since.starts_with('-') {
        return Err(CommitRangeError::BadRange {
            stderr: format!("`since` must not start with '-' (got {since:?})"),
        });
    }
    let range = format!("{since}..HEAD");
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(root).args([
        "log",
        "--reverse",
        "--abbrev-commit",
        "--format=%h%x00%an%x00%ae%x00%B%x1e",
    ]);
    if !include_merges {
        cmd.arg("--no-merges");
    }
    // `--end-of-options`: a config `since` starting with `-` (e.g.
    // `--output=…`) must never be parsed as a git OPTION (which would
    // write/truncate an arbitrary file); force it to the range slot.
    cmd.arg("--end-of-options");
    cmd.arg(&range);

    let Ok(output) = cmd.output() else {
        return Ok(None);
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(CommitRangeError::BadRange { stderr });
    }

    Ok(Some(parse_commit_log(&output.stdout)))
}

/// Parse the NUL+RS-separated `git log` output produced by
/// [`commit_messages_in_range`]'s `--format` string. Empty trailing
/// records (from the final RS) are skipped. Messages have their
/// trailing newline trimmed (`git log` always appends one).
fn parse_commit_log(stdout: &[u8]) -> Vec<CommitRecord> {
    let mut out = Vec::new();
    // Records are RS-separated (0x1e). The last record ends with
    // RS too, so the final split chunk is empty.
    for record in stdout.split(|&b| b == 0x1e) {
        if record.is_empty() {
            continue;
        }
        // Each record is sha + NUL + author-name + NUL +
        // author-email + NUL + message. Trim the leading newline
        // that git inserts between records.
        let record = record.strip_prefix(b"\n").unwrap_or(record);
        let mut parts = record.splitn(4, |&b| b == 0);
        let (Some(sha_bytes), Some(name_bytes), Some(email_bytes), Some(msg_bytes)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        // Lossily decode rather than dropping the whole commit on any
        // non-UTF-8 field: silently skipping a commit lets a contributor
        // bypass commit linting (conventional-subject / author-allowlist /
        // forbidden-pattern) just by using a non-UTF-8 author name or message.
        // With a lossy decode the commit is still linted; the sha is always
        // hex so it is unaffected.
        let sha = String::from_utf8_lossy(sha_bytes);
        let name = String::from_utf8_lossy(name_bytes);
        let email = String::from_utf8_lossy(email_bytes);
        let msg = String::from_utf8_lossy(msg_bytes);
        // `--format=%B` ends every body with a trailing newline.
        let message = msg.trim_end_matches('\n').to_string();
        out.push(CommitRecord {
            sha: sha.to_string(),
            message,
            author_name: name.to_string(),
            author_email: email.to_string(),
        });
    }
    out
}

/// Verify a commit's signature via `git verify-commit <sha>`.
///
/// Returns:
/// - `Some(true)`  — `verify-commit` exited 0 (a good signature that
///   verified against the local keyring).
/// - `Some(false)` — it exited non-zero: the commit is unsigned, or
///   the signature didn't verify (e.g. signed with a key not in the
///   local keyring).
/// - `None`        — `git` isn't on PATH (the shell-out itself
///   failed). Callers iterating commits from a valid repo never see
///   this; it's the advisory-posture escape hatch.
///
/// This reflects git's own verdict and deliberately does NOT
/// distinguish "unsigned" from "signed with an untrusted key" —
/// trust is the user's GPG config / `.git/allowed_signers`, not this
/// rule's job.
pub fn verify_commit(root: &Path, sha: &str) -> Option<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["verify-commit", sha])
        .output()
        .ok()?;
    Some(output.status.success())
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
                    // `checked_add`: a crafted commit can carry a 19-digit
                    // author-time that parses as u64 but overflows SystemTime
                    // (which panics on `+`). On overflow, leave the time unset
                    // — matching the "drop the malformed block" posture
                    // elsewhere in the parser — instead of aborting the run.
                    author_time = UNIX_EPOCH.checked_add(Duration::from_secs(secs));
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

    // ----- commit_messages_in_range -----------------------------

    /// Build a temp dir into a git repo with the given list of
    /// empty commits in order (commit N is HEAD~(len-1-N)). Returns
    /// the tempdir so the caller controls its lifetime.
    ///
    /// Uses `git commit --allow-empty` so the test doesn't need to
    /// write fixture files. Disables GPG signing and sets a fixed
    /// author so the commits are deterministic.
    #[test]
    fn commit_range_rejects_dash_since_and_writes_no_file() {
        // Security regression (git arg-injection): a config-controlled
        // `since` starting with `-` (e.g. `--output=…`) must be rejected
        // before git runs — it must never write/truncate a file. (Affects
        // the released `git_commit_message` `since:` path.)
        let outdir = tempfile::tempdir().unwrap();
        let stem = outdir.path().join("sentinel");
        let would_write = outdir.path().join("sentinel..HEAD");
        let evil = format!("--output={}", stem.display());
        let err = commit_messages_in_range(Path::new("."), &evil, false).unwrap_err();
        assert!(matches!(err, CommitRangeError::BadRange { .. }), "{err:?}");
        assert!(
            !would_write.exists(),
            "git must not have written {would_write:?}"
        );
    }

    #[test]
    fn collect_changed_paths_dash_base_writes_no_file() {
        // The `--changed` / `changed_since` diff path: `--end-of-options`
        // forces a dash-leading base into the revision slot, so git never
        // parses `--output=…` and writes nothing.
        let outdir = tempfile::tempdir().unwrap();
        let stem = outdir.path().join("sentinel");
        let would_write = outdir.path().join("sentinel...HEAD");
        let evil = format!("--output={}", stem.display());
        let _ = collect_changed_paths(Path::new("."), Some(&evil));
        assert!(
            !would_write.exists(),
            "git diff must not have written {would_write:?}"
        );
    }

    fn make_repo_with_commits(subjects: &[&str]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let init_dir = tmp.path();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            let out = Command::new("git")
                .arg("-C")
                .arg(init_dir)
                .args(&args)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?} failed");
        }
        for subject in subjects {
            let out = Command::new("git")
                .arg("-C")
                .arg(init_dir)
                .args(["commit", "--allow-empty", "-m", subject])
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git commit failed: stderr={}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        tmp
    }

    #[test]
    fn parse_commit_log_empty_input() {
        assert!(parse_commit_log(b"").is_empty());
    }

    #[test]
    fn parse_commit_log_single_commit() {
        // sha NUL name NUL email NUL body-with-trailing-newline RS.
        let raw =
            b"abc1234\0Jane Doe\0jane@example.com\0subject line\n\nbody line one\nbody line two\n\x1e";
        let records = parse_commit_log(raw);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].sha, "abc1234");
        assert_eq!(records[0].author_name, "Jane Doe");
        assert_eq!(records[0].author_email, "jane@example.com");
        assert_eq!(
            records[0].message,
            "subject line\n\nbody line one\nbody line two"
        );
    }

    #[test]
    fn parse_commit_log_multiple_commits() {
        // Two commits, oldest first (matches --reverse). Between
        // records, git inserts a newline before the next SHA; the
        // parser strips it.
        let raw = b"a1\0A\0a@x.test\0first\n\x1e\nb2\0B\0b@x.test\0second\n\x1e";
        let records = parse_commit_log(raw);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].sha, "a1");
        assert_eq!(records[0].author_email, "a@x.test");
        assert_eq!(records[0].message, "first");
        assert_eq!(records[1].sha, "b2");
        assert_eq!(records[1].message, "second");
    }

    #[test]
    fn parse_commit_log_subject_only_no_body() {
        let raw = b"deadbef\0N\0n@x.test\0just the subject\n\x1e";
        let records = parse_commit_log(raw);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].message, "just the subject");
    }

    #[test]
    fn parse_commit_log_preserves_blank_lines_in_body() {
        // A real commit body with multiple paragraphs survives the
        // round-trip unchanged.
        let raw = b"sha7777\0N\0n@x.test\0fix: thing\n\nfirst paragraph.\n\nsecond paragraph.\n\nthird.\n\x1e";
        let records = parse_commit_log(raw);
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].message,
            "fix: thing\n\nfirst paragraph.\n\nsecond paragraph.\n\nthird."
        );
    }

    #[test]
    fn parse_commit_log_lossily_decodes_invalid_utf8_rather_than_dropping() {
        // A record whose message field is invalid UTF-8. The parser must NOT
        // drop it — silently skipping the commit would let a contributor
        // bypass commit linting (conventional-subject / author-allowlist /
        // forbidden-pattern) just by using a non-UTF-8 message or author.
        // It now lossily decodes so the commit is still linted; the sha is
        // always hex, so it survives intact.
        let mut raw: Vec<u8> = b"abc1234\0N\0n@x.test\0".to_vec();
        raw.extend_from_slice(&[0xff, 0xfe, 0xfd]); // invalid UTF-8
        raw.push(0x1e);
        let records = parse_commit_log(&raw);
        assert_eq!(records.len(), 1, "the commit must be parsed, not dropped");
        assert_eq!(records[0].sha, "abc1234");
        assert_eq!(records[0].author_name, "N");
        // The invalid bytes become U+FFFD replacement chars (not silently lost).
        assert!(records[0].message.contains('\u{FFFD}'));
    }

    #[test]
    fn commit_range_returns_none_outside_git() {
        let tmp = tempfile::tempdir().unwrap();
        // Non-git directory: silent None. Distinguishes from the
        // BadRange error (which a bad ref inside a real repo
        // produces) so the rule layer can decide between "skip
        // silently" and "hard fail."
        let result = commit_messages_in_range(tmp.path(), "main", false);
        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn commit_range_returns_empty_vec_for_head_to_head() {
        let repo = make_repo_with_commits(&["feat: first commit"]);
        let result = commit_messages_in_range(repo.path(), "HEAD", false).unwrap();
        // HEAD..HEAD is the empty range. Some(empty), not None.
        assert_eq!(result, Some(Vec::new()));
    }

    #[test]
    fn commit_range_enumerates_real_commits_oldest_first() {
        // Four commits. Use the root commit's full SHA as the
        // `since` base; the range then yields the three later
        // commits, oldest first.
        let repo =
            make_repo_with_commits(&["root: zero", "feat: alpha", "fix: beta", "chore: gamma"]);
        let root_sha = String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(repo.path())
                .args(["rev-parse", "HEAD~3"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();
        let records = commit_messages_in_range(repo.path(), &root_sha, false)
            .unwrap()
            .unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].message, "feat: alpha");
        assert_eq!(records[1].message, "fix: beta");
        assert_eq!(records[2].message, "chore: gamma");
        // SHAs are abbreviated (7+ chars, hex).
        for r in &records {
            assert!(r.sha.len() >= 7);
            assert!(r.sha.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn commit_range_skips_merges_by_default() {
        // Build the canonical PR-CI shape: a base branch with one
        // commit, a feature branch off it with two commits, then a
        // merge commit on the base branch. The merge is what
        // actions/checkout produces at HEAD on a pull_request
        // trigger.
        let repo = make_repo_with_commits(&["init commit on main"]);
        let root = repo.path();
        let run = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap()
        };
        let base_sha = run(&["rev-parse", "HEAD"]).trim().to_string();
        run(&["checkout", "-q", "-b", "feature"]);
        run(&["commit", "--allow-empty", "-m", "feat: A"]);
        run(&["commit", "--allow-empty", "-m", "fix: B"]);
        run(&["checkout", "-q", "main"]);
        run(&["merge", "--no-ff", "--no-edit", "feature"]);

        // Range main-base..HEAD: includes feat:A, fix:B, and the
        // merge commit. Default skips the merge.
        let records = commit_messages_in_range(root, &base_sha, false)
            .unwrap()
            .unwrap();
        let subjects: Vec<&str> = records.iter().map(|r| r.message.as_str()).collect();
        assert_eq!(subjects, vec!["feat: A", "fix: B"]);

        // Same range with include_merges: true picks up the merge.
        let with_merge = commit_messages_in_range(root, &base_sha, true)
            .unwrap()
            .unwrap();
        assert_eq!(with_merge.len(), 3);
        assert!(with_merge.iter().any(|r| r.message.starts_with("Merge ")));
    }

    #[test]
    fn changed_paths_checked_none_outside_git_and_bad_range_inside() {
        // Outside a git repo: silent None (so changed_since no-ops).
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(
            collect_changed_paths_checked(tmp.path(), "origin/main"),
            Ok(None)
        ));
        // Inside a repo, an unresolvable ref hard-errors.
        let repo = make_repo_with_commits(&["init"]);
        assert!(matches!(
            collect_changed_paths_checked(repo.path(), "no-such-ref"),
            Err(CommitRangeError::BadRange { .. })
        ));
    }

    #[test]
    fn verify_commit_returns_false_for_unsigned_commit() {
        // make_repo_with_commits disables gpg signing, so HEAD is
        // unsigned; verify-commit exits non-zero → Some(false).
        let repo = make_repo_with_commits(&["init: unsigned commit"]);
        let head = String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(repo.path())
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();
        assert_eq!(verify_commit(repo.path(), &head), Some(false));
    }

    #[test]
    fn commit_range_returns_bad_range_for_unknown_ref() {
        let repo = make_repo_with_commits(&["init"]);
        let result = commit_messages_in_range(repo.path(), "does-not-exist-ref", false);
        match result {
            Err(CommitRangeError::BadRange { stderr }) => {
                // Git typically says "unknown revision or path not
                // in the working tree." We don't assert the exact
                // wording (varies across git versions); just that
                // we got a non-empty stderr.
                assert!(!stderr.is_empty());
            }
            other => panic!("expected BadRange, got {other:?}"),
        }
    }
}
