//! [`GitTrackedMode`], how a rule narrows its iteration to git-tracked
//! entries ([`super::Rule::git_tracked_mode`]).

/// How a rule narrows its iteration to git-tracked entries.
/// Returned by [`Rule::git_tracked_mode`](super::Rule::git_tracked_mode); the engine reads
/// this at construction time to pick the right pre-filtered
/// `FileIndex` (file-only or dir-aware) for each opted-in
/// rule.
///
/// The mode is per-rule (a config might opt in some rules and
/// not others). The engine builds at most two filtered indexes
/// per run regardless of how many rules opt in, so the cost
/// amortises across the whole rule set.
///
/// See `docs/design/v0.9/git-tracked-filtered-index.md` for
/// the v0.9.11 structural fix this enum is the entry point of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitTrackedMode {
    /// Rule does not consult the git-tracked set. Engine
    /// routes the rule's evaluation against the unfiltered
    /// `FileIndex`. The default; do not override unless the
    /// rule opts into `git_tracked_only:`.
    Off,
    /// Rule iterates files (`ctx.index.files()`) and the
    /// engine narrows that to entries where
    /// `git_tracked.contains(path)` before the rule sees them.
    /// File-mode existence rules (`file_exists`, `file_absent`)
    /// pick this mode when the spec's `git_tracked_only: true`.
    FileOnly,
    /// Rule iterates dirs (`ctx.index.dirs()`) and the engine
    /// narrows that to dirs where
    /// `dir_has_tracked_files(path, &git_tracked)`. Dir-mode
    /// existence rules (`dir_exists`, `dir_absent`) pick this
    /// mode when the spec's `git_tracked_only: true`. The
    /// filtered index also includes the tracked files
    /// themselves so a `dir_*` rule's nested per-file checks
    /// (e.g. `paths:` glob) still match.
    DirAware,
}
