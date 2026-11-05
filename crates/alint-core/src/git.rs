//! Bounded Git integration.
//!
//! The engine derives active Git dependencies from rule capability metadata,
//! probes repository usability once, and routes probe failure to one typed
//! analysis failure per active owning rule. The helpers in this module retain
//! `Option`-based, best-effort results for operation-specific absence
//! (unborn HEAD, untracked files, unblameable paths) and direct library use.
//! `git_tracked_only` filtering reuses the probe's `git ls-files` set through
//! the rule [`Context`](crate::Context).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{AnalysisTime, AnalysisTimeUnavailable};

type ProcessEnvironment = BTreeMap<String, String>;
type ScopedEnvironmentRegistry = Mutex<HashMap<PathBuf, Arc<ProcessEnvironment>>>;

static SCOPED_PROCESS_ENVIRONMENTS: LazyLock<ScopedEnvironmentRegistry> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// RAII registration for a complete Git child-process environment at one
/// repository root.
///
/// This is intended for hermetic embedders such as `alint-testkit`: every Git
/// helper in this module consults the registration before spawning. Distinct
/// roots can run concurrently. Dropping the guard restores normal host
/// inheritance for that root.
#[derive(Debug)]
pub struct ScopedProcessEnvironment {
    root: PathBuf,
}

impl Drop for ScopedProcessEnvironment {
    fn drop(&mut self) {
        if let Ok(mut environments) = SCOPED_PROCESS_ENVIRONMENTS.lock() {
            environments.remove(&self.root);
        }
    }
}

#[must_use]
pub fn scoped_process_environment(
    root: &Path,
    environment: BTreeMap<String, String>,
) -> ScopedProcessEnvironment {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    SCOPED_PROCESS_ENVIRONMENTS
        .lock()
        .expect("Git process-environment registry lock poisoned")
        .insert(root.clone(), Arc::new(environment));
    ScopedProcessEnvironment { root }
}

/// Environment variables that can redirect a Git command away from the
/// repository selected by `-C`, replace its object/ref view, alter repository
/// discovery, or change literal path interpretation. This includes Git's
/// `rev-parse --local-env-vars` set plus the pathspec-mode controls; clearing it
/// keeps repository identity and path inputs explicit.
const GIT_LOCAL_ENV_VARS: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_INTERNAL_SUPER_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_NAMESPACE",
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    "GIT_COMMON_DIR",
    "GIT_LITERAL_PATHSPECS",
    "GIT_GLOB_PATHSPECS",
    "GIT_NOGLOB_PATHSPECS",
    "GIT_ICASE_PATHSPECS",
];

fn clear_git_repository_env(
    command: &mut Command,
    scoped_environment: Option<&ProcessEnvironment>,
) {
    for name in GIT_LOCAL_ENV_VARS {
        // `Command::env_remove` forces Rust to copy the complete process
        // environment at spawn time, even when the named variable is absent.
        // Preserve the direct-inheritance fast path without weakening scoped
        // runners: inspect whichever environment the child will inherit.
        let is_present = scoped_environment.map_or_else(
            || std::env::var_os(name).is_some(),
            |environment| environment.contains_key(*name),
        );
        if is_present {
            command.env_remove(name);
        }
    }
}

fn git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    let environment = SCOPED_PROCESS_ENVIRONMENTS
        .lock()
        .ok()
        .and_then(|environments| {
            if environments.is_empty() {
                return None;
            }
            let key = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
            environments.get(&key).cloned()
        });
    if let Some(environment) = environment.as_deref() {
        command.env_clear().envs(environment);
    }
    clear_git_repository_env(&mut command, environment.as_deref());
    // Repository paths are byte-exact inputs, never user-authored pathspecs;
    // replacement refs are clone-local state, never repository history; log
    // display preferences must not invoke signature verifiers or alter stdout.
    command.args([
        "--literal-pathspecs",
        "--no-replace-objects",
        "-c",
        "log.showSignature=false",
    ]);
    command
}
/// Resolve the current HEAD commit's committer timestamp.
///
/// The command emits only `%ct`; raw Git stderr is deliberately excluded from
/// the stable error returned to rule evaluation and machine formats.
pub fn head_commit_time(root: &Path) -> Result<AnalysisTime, AnalysisTimeUnavailable> {
    let mut command = git_command(root);
    let output = command
        .arg("-C")
        .arg(root)
        .args(["log", "-1", "--format=%ct", "--end-of-options", "HEAD"])
        .output()
        .map_err(|_| AnalysisTimeUnavailable::GitUnavailable)?;
    if !output.status.success() {
        return Err(AnalysisTimeUnavailable::HeadUnavailable);
    }
    parse_head_commit_time(&output.stdout)
}

fn parse_head_commit_time(stdout: &[u8]) -> Result<AnalysisTime, AnalysisTimeUnavailable> {
    let digits = stdout
        .strip_suffix(b"\n")
        .unwrap_or(stdout)
        .strip_suffix(b"\r")
        .unwrap_or_else(|| stdout.strip_suffix(b"\n").unwrap_or(stdout));
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return Err(AnalysisTimeUnavailable::InvalidHeadTime);
    }
    let text = std::str::from_utf8(digits).map_err(|_| AnalysisTimeUnavailable::InvalidHeadTime)?;
    let seconds = text
        .parse::<u64>()
        .map_err(|_| AnalysisTimeUnavailable::InvalidHeadTime)?;
    AnalysisTime::from_unix_seconds(seconds).ok_or(AnalysisTimeUnavailable::InvalidHeadTime)
}

/// Resolve the repo's tracked-paths set, relative to `root`.
///
/// `root` should be the alint root (the path passed to
/// `alint check`). The implementation runs `git -C <root> ls-files
/// -z` with NO pathspec, so git returns paths relative to `root`
/// whether `root` is the git root or a subdirectory of it (git's
/// `-C <dir>` default). No pathspec means no globbing, so this
/// reader is immune to the pathspec-magic hazard the mutating
/// [`untrack_path`] guards against.
///
/// Returns `None` when:
/// - `git` isn't on PATH
/// - `root` (or any ancestor) isn't inside a git repo
/// - the `git` invocation exits non-zero for any other reason
///
/// All these cases produce an empty `Option`, never panic — the
/// caller is responsible for treating `None` as "no tracked-set
/// available" in whatever way makes sense for the calling rule.
/// Build a `PathBuf` from a NUL-delimited `git -z` path chunk WITHOUT a UTF-8
/// round-trip, so a non-UTF-8 tracked/changed path is PRESERVED (M6) rather than
/// collapsing the whole set to `None` — which made `git_tracked_only`
/// over-permissive (a non-UTF-8 tracked file wasn't recognized as tracked). On
/// Unix the raw bytes ARE the path; elsewhere git emits UTF-8, so a lossy decode
/// keeps a best-effort path instead of dropping the entire set.
fn path_from_git_chunk(chunk: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        PathBuf::from(std::ffi::OsStr::from_bytes(chunk))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(chunk).into_owned())
    }
}

pub fn collect_tracked_paths(root: &Path) -> Option<HashSet<PathBuf>> {
    // `-z` separates entries with NUL so paths with newlines or
    // exotic bytes round-trip correctly. `--full-name` would force
    // repo-root-relative paths, but we want CWD-relative — git's
    // default with `-C <dir>` already gives that.
    let output = git_command(root)
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
        out.insert(path_from_git_chunk(chunk));
    }
    Some(out)
}

/// The result of a [`untrack_path`] attempt.
#[derive(Debug, PartialEq, Eq)]
pub enum UntrackOutcome {
    /// The path was removed from git's index (it stays on disk).
    Untracked,
    /// `root` is not inside a git repo, or `git` is unavailable — nothing done.
    NotAGitRepo,
    /// The path is already absent from the index — nothing done (idempotent).
    NotTracked,
    /// `git rm --cached` ran but exited non-zero (e.g. staged content differs
    /// from HEAD, so a non-forced remove is refused); carries the stderr tail.
    Failed(String),
}

/// Remove `rel_path` from git's index WITHOUT deleting it from the working tree
/// (`git rm --cached -- <path>`), running in `root`. `rel_path` is
/// root-relative (a rule's violation path). Unlike the advisory readers above,
/// this MUTATES the repo (unless `dry_run`), so the caller (the `git_untrack`
/// fixer) reports Applied / Skipped / error from the returned [`UntrackOutcome`].
///
/// `dry_run` adds git's own `--dry-run`: git runs its FULL safety check (incl.
/// refusing a path whose staged content differs from both HEAD and the worktree)
/// but changes nothing, so the fixer's dry-run / `--diff` preview predicts EXACTLY
/// what a real run would do -- `Untracked` = "would untrack", `Failed` = "would
/// fail". The round-4 dry-run-faithfulness invariant then holds by construction
/// (the preview and the real run are the same code path with the same git checks),
/// rather than via a separately-maintained prediction that could drift.
///
/// Safety and idempotence:
/// - the shared `git_command` passes `--literal-pathspecs`, disabling git's
///   pathspec globbing/magic so a path containing `*` / `?` / `[` / `]` (a
///   Next.js `[id].tsx` route, or a file literally named `*`) is matched
///   LITERALLY. `--` only ends OPTION parsing, NOT globbing, so without this a
///   single flagged path would collaterally untrack every sibling git globs it
///   against -- up to the WHOLE index for a `*` -- all hidden behind a "1
///   applied" report. Preferred over a `:(literal)` prefix: it needs no UTF-8
///   round-trip, so a non-UTF-8 path (which this module preserves) stays intact.
///   `collect_tracked_paths` takes no pathspec, so it is unaffected.
/// - the path is passed after `--`, never as an option, so a path beginning with
///   `-` can never be read as a git flag (option-injection guard);
/// - outside a repo it returns `NotAGitRepo` and an already-untracked path returns
///   `NotTracked` (both no-ops), so a second run is a clean skip, not a `git` error;
/// - it does NOT pass `-f`: a path with staged content differing from HEAD is left
///   untouched and reported as `Failed`, never force-removed.
pub fn untrack_path(root: &Path, rel_path: &Path, dry_run: bool) -> UntrackOutcome {
    // Pre-classify by reusing the advisory reader: outside a repo -> NotAGitRepo;
    // not in the index -> NotTracked (the idempotent no-op). This also means the
    // real `git rm` only runs on a path we have already confirmed is tracked.
    match collect_tracked_paths(root) {
        None => return UntrackOutcome::NotAGitRepo,
        Some(tracked) if !tracked.contains(rel_path) => return UntrackOutcome::NotTracked,
        Some(_) => {}
    }
    let mut cmd = git_command(root);
    cmd.arg("-C").arg(root).args(["rm", "--cached", "--quiet"]);
    if dry_run {
        cmd.arg("--dry-run");
    }
    cmd.arg("--").arg(rel_path);
    match cmd.output() {
        Ok(o) if o.status.success() => UntrackOutcome::Untracked,
        Ok(o) => UntrackOutcome::Failed(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => UntrackOutcome::Failed(e.to_string()),
    }
}

/// Resolve the set of paths that have changed in the working tree
/// (and optionally relative to a base ref), expressed as paths
/// relative to `root`.
///
/// `base` selects the diff:
/// - `Some("main")` — `git diff --name-only --relative main...HEAD`
///   (three-dot — diff against the merge-base of `main` and
///   `HEAD`). Right shape for PR-check use cases.
/// - `None` — the union of `git diff --cached --name-only --relative`
///   and `git ls-files --modified --others --exclude-standard` from
///   `root`. This includes staged, unstaged, and untracked changes,
///   which is the right shape for pre-commit / local-dev use cases.
///   Deleted files are also returned (they're in the diff but not on
///   disk, so the engine's intersect-with-walked-index step filters
///   them out naturally).
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
    let outputs = if let Some(base) = base {
        // Defense-in-depth, matching `diff_name_only`: reject a `base`
        // starting with `-` explicitly (treat as "no changed-set"), in
        // addition to the `--end-of-options` guard below.
        if base.starts_with('-') {
            return None;
        }
        let output = git_command(root)
            .arg("-C")
            .arg(root)
            .args(["diff", "--name-only", "--relative", "-z"])
            // `--end-of-options` so a `base`/`since` starting with `-`
            // can't be parsed as a git OPTION (e.g. `--output=…`, which
            // would write/truncate an arbitrary file).
            .arg("--end-of-options")
            .arg(format!("{base}...HEAD"))
            .output()
            .ok()?;
        vec![output]
    } else {
        // `ls-files --modified` compares the worktree with the index, so a
        // staged edit disappears from it. Union the index-vs-HEAD diff with
        // the working-tree/untracked set; otherwise a pre-commit hook can
        // silently check zero files after the framework stashes unstaged
        // changes. `--cached` also works for an unborn HEAD.
        //
        // The two reads are independent, so the staged diff runs in the
        // background while `ls-files` runs: one process's latency instead of
        // two, on every `--changed` run. A staged list too big for the pipe
        // buffer (64 KiB on Linux, 4 KiB on Windows) makes the child wait for
        // `ls-files` to finish, which is no slower than running them in turn.
        // `wait_with_output` then drains both pipes, so the child can't wedge,
        // and it is always reaped, even when `ls-files` fails to spawn.
        let staged = git_command(root)
            .arg("-C")
            .arg(root)
            .args(["diff", "--cached", "--name-only", "--relative", "-z"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .ok()?;
        let working = git_command(root)
            .arg("-C")
            .arg(root)
            .args([
                "ls-files",
                "--modified",
                "--others",
                "--exclude-standard",
                "-z",
            ])
            .output();
        let staged = staged.wait_with_output().ok()?;
        vec![staged, working.ok()?]
    };
    if outputs.iter().any(|output| !output.status.success()) {
        return None;
    }
    let mut out = HashSet::new();
    for output in outputs {
        for chunk in output.stdout.split(|&b| b == 0) {
            if chunk.is_empty() {
                continue;
            }
            out.insert(path_from_git_chunk(chunk));
        }
    }
    Some(out)
}

/// Like [`collect_changed_paths`] with a `base` ref, but distinguishes
/// repository unavailability from a ref that does not resolve — the contract
/// `scope_filter.changed_since:` needs. Returns the set of paths changed in
/// `<since>...HEAD` (three-dot, merge-base diff — matching
/// `alint check --changed`), relative to `root`.
///
/// - `Ok(Some(set))` — resolved.
/// - `Ok(None)`       — the repository or Git executable is unavailable.
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
    let Ok(probe) = git_command(root)
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
    let mut cmd = git_command(root);
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
        // Preserve a non-UTF-8 changed path instead of collapsing the whole set
        // to `None` (which the caller maps to an empty changed-set, silently
        // no-opping every `changed_since`-scoped rule — a fail-open lint bypass).
        // Same decode the top-level `--changed` collector uses (M6).
        out.insert(path_from_git_chunk(chunk));
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
/// Used by the `git_commit_message` rule kind. The engine handles repository
/// unavailability before dispatch; `None` remains valid for an unborn HEAD or
/// an operation-specific failure.
pub fn head_commit_message(root: &Path) -> Option<String> {
    let output = git_command(root)
        .arg("-C")
        .arg(root)
        .args(["log", "-1", "--format=%B"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    // Lossily decode a non-UTF-8 HEAD message instead of returning `None` (which
    // silently no-ops `git_commit_message` — letting a non-UTF-8 commit message
    // bypass conventional-subject / forbidden-pattern / length checks; the same
    // fail-open the range-mode `parse_commit_log` was already hardened against, M6).
    let raw = String::from_utf8_lossy(&output.stdout);
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
/// (no `git`, not a repo, unborn HEAD). The engine gates the first two before
/// rule dispatch; an unborn HEAD has no commit policy input.
/// Uses the same NUL-separated `--format` encoding as
/// [`commit_messages_in_range`] so a single commit round-trips
/// through the shared commit-log parser.
pub fn head_commit_record(root: &Path) -> Option<CommitRecord> {
    let output = git_command(root)
        .arg("-C")
        .arg(root)
        .args([
            "log",
            "-1",
            "--abbrev=7",
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
/// [`commit_messages_in_range`]. `sha` uses a requested seven-hex abbreviation;
/// Git extends it only when that prefix is ambiguous in the repository. `message`
/// is the full commit message (subject + body, separated by a blank line) with the
/// trailing newline that `git log --format=%B` appends already trimmed.
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

/// What a [`CommitRangeError::BadRange`] carries in place of git's message
/// when the ref may be secret: git quotes the ref, or only part of it.
pub const BAD_RANGE_MESSAGE_LEFT_OUT: &str =
    "git's message is left out because the ref may be secret";

/// Errors that distinguish an invalid range from an unavailable repository.
/// The engine gates repository availability from capability metadata; the rule
/// layer uses `BadRange` to preserve the specific shallow-clone migration hint.
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
/// - `Ok(None)` if `git` isn't on PATH or `root` isn't inside a Git repository.
///   The engine normally gates this before rule dispatch; the explicit shape
///   keeps this low-level helper total for direct callers.
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
    // First check `git rev-parse` (no range syntax) confirms repository
    // availability. If it fails, return the distinct `Ok(None)` shape that
    // engine capability routing promotes before normal rule dispatch.
    let probe = git_command(root)
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
    let mut cmd = git_command(root);
    cmd.arg("-C").arg(root).args([
        "log",
        "--reverse",
        "--abbrev=7",
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
    let output = git_command(root)
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

const DEFAULT_BLAME_IGNORE_REVS_FILE: &str = ".git-blame-ignore-revs";

/// Run `git blame --line-porcelain` for `rel_path` (relative to
/// `root`) and return one [`BlameLine`] per source line.
///
/// `--line-porcelain` repeats the full per-commit metadata block
/// for every line so we don't have to track the most-recent
/// commit across runs — every line carries its own
/// `author-time`. When the repository contains a regular
/// `.git-blame-ignore-revs` file, alint passes it explicitly rather than
/// depending on clone-local Git configuration.
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
    blame_lines_with_ignore_revs(root, rel_path, default_ignore_revs_is_tracked_regular(root))
}

fn blame_lines_with_ignore_revs(
    root: &Path,
    rel_path: &Path,
    use_default_ignore_revs: bool,
) -> Option<Vec<BlameLine>> {
    let mut command = git_command(root);
    command
        // Clone-local/global ignore lists are ambient machine state. A command-line
        // empty file name clears their revisions before the repository convention
        // is added below. A malformed configured file can still make Git fail while
        // loading its config; W2 will promote that existing advisory failure path.
        // The documented `diff.algorithm` setting changes blame attribution, so pin
        // Myers rather than inheriting user/global configuration.
        .args(["-c", "diff.algorithm=myers"])
        .arg("-C")
        .arg(root)
        .args(["blame", "--ignore-revs-file", "", "--line-porcelain"]);
    if use_default_ignore_revs {
        // Apply only the repository-tracked regular-file convention, independent
        // of each clone's `blame.ignoreRevsFile`.
        command.args(["--ignore-revs-file", DEFAULT_BLAME_IGNORE_REVS_FILE]);
    }
    let output = command.arg("--").arg(rel_path).output().ok()?;
    if !output.status.success() {
        return None;
    }
    // Lossily decode: `--line-porcelain` embeds source-line content AND author
    // names, so one non-UTF-8 byte anywhere in the file must NOT collapse the
    // whole blame to `None` — that silently no-ops `git_blame_age` for the file
    // (every stale line evades the age check), the M6 fail-open the sibling git
    // paths were already hardened against. The porcelain framing (SHA/header
    // keywords, tab-prefixed content lines) is ASCII, so lossy decoding leaves
    // the structure parseable.
    let text = String::from_utf8_lossy(&output.stdout);
    Some(parse_porcelain(&text))
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
    let mut uncommitted = false;
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
        // Header lines start with a full SHA-1 (40 hex digits) or SHA-256
        // object ID (64 hex digits); subsequent lines are `key value` pairs.
        let mut parts = line.splitn(2, ' ');
        let key = parts.next().unwrap_or("");
        let value = parts.next().unwrap_or("");
        match key {
            "author-time" if !uncommitted => {
                if let Ok(secs) = value.parse::<u64>() {
                    // `checked_add`: a crafted commit can carry a 19-digit
                    // author-time that parses as u64 but overflows SystemTime
                    // (which panics on `+`). On overflow, leave the time unset
                    // — matching the "drop the malformed block" posture
                    // elsewhere in the parser — instead of aborting the run.
                    author_time = UNIX_EPOCH.checked_add(Duration::from_secs(secs));
                }
            }
            // Full object-id header + 3 numbers. Accept both repository object
            // formats; hard-coding SHA-1 silently drops every line in a SHA-256
            // repository.
            sha if matches!(sha.len(), 40 | 64) && sha.chars().all(|c| c.is_ascii_hexdigit()) => {
                // Git assigns working-tree-only lines the all-zero pseudo SHA and
                // may synthesize their author time from the wall clock. They are
                // not ageable committed history, so omit them deterministically.
                uncommitted = sha.bytes().all(|b| b == b'0');
                author_time = None;
                // The header line is `<sha> <orig> <final> [<num_lines>]`.
                let mut cols = value.split(' ');
                let _orig = cols.next();
                final_line = cols.next().and_then(|s| s.parse::<usize>().ok());
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
    use_default_ignore_revs: bool,
    inner: Mutex<HashMap<PathBuf, CacheEntry>>,
}

#[derive(Debug, Clone)]
enum CacheEntry {
    Ok(Arc<Vec<BlameLine>>),
    Failed,
}

fn default_ignore_revs_is_tracked_regular(root: &Path) -> bool {
    let ignore_revs = root.join(DEFAULT_BLAME_IGNORE_REVS_FILE);
    if !ignore_revs
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_file())
    {
        return false;
    }
    git_command(root)
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "--error-unmatch",
            "--",
            DEFAULT_BLAME_IGNORE_REVS_FILE,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

impl BlameCache {
    pub fn new(root: PathBuf) -> Self {
        let use_default_ignore_revs = default_ignore_revs_is_tracked_regular(&root);
        Self::with_default_ignore_revs(root, use_default_ignore_revs)
    }

    pub(crate) fn new_with_tracked_paths(root: PathBuf, tracked: &HashSet<PathBuf>) -> Self {
        let ignore_revs = root.join(DEFAULT_BLAME_IGNORE_REVS_FILE);
        let use_default_ignore_revs = tracked.contains(Path::new(DEFAULT_BLAME_IGNORE_REVS_FILE))
            && ignore_revs
                .symlink_metadata()
                .is_ok_and(|metadata| metadata.file_type().is_file());
        Self::with_default_ignore_revs(root, use_default_ignore_revs)
    }

    fn with_default_ignore_revs(root: PathBuf, use_default_ignore_revs: bool) -> Self {
        Self {
            root,
            use_default_ignore_revs,
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Return the blame for `rel_path`, computing once and
    /// caching forever (within this run). `None` means this individual path
    /// could not be blamed, so the caller skips it; repository-wide
    /// unavailability is handled before dispatch.
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
        let computed =
            blame_lines_with_ignore_revs(&self.root, rel_path, self.use_default_ignore_revs);
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
#[path = "git_tests.rs"]
mod tests;
