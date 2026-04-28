use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::facts::FactValues;
use crate::level::Level;
use crate::registry::RuleRegistry;
use crate::walker::FileIndex;

/// A single linting violation produced by a rule.
#[derive(Debug, Clone)]
pub struct Violation {
    pub path: Option<PathBuf>,
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl Violation {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            path: None,
            message: message.into(),
            line: None,
            column: None,
        }
    }

    #[must_use]
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    #[must_use]
    pub fn with_location(mut self, line: usize, column: usize) -> Self {
        self.line = Some(line);
        self.column = Some(column);
        self
    }
}

/// The collected outcome of evaluating a single rule.
#[derive(Debug, Clone)]
pub struct RuleResult {
    pub rule_id: String,
    pub level: Level,
    pub policy_url: Option<String>,
    pub violations: Vec<Violation>,
    /// Whether the rule declares a [`Fixer`] — surfaced here so
    /// the human formatter can tag violations as `fixable`
    /// without threading the rule registry into the renderer.
    pub is_fixable: bool,
}

impl RuleResult {
    pub fn passed(&self) -> bool {
        self.violations.is_empty()
    }
}

/// Execution context handed to each rule during evaluation.
///
/// - `registry` — available for rules that need to build and evaluate nested
///   rules at runtime (e.g. `for_each_dir`). Tests that don't exercise
///   nested evaluation can set this to `None`.
/// - `facts` — resolved fact values, computed once per `Engine::run`.
/// - `vars` — user-supplied string variables from the config's `vars:` section.
/// - `git_tracked` — set of repo paths reported by `git ls-files`,
///   computed once per run when at least one rule has
///   `git_tracked_only: true`. `None` outside a git repo or when
///   no rule asked for it. Rules that opt in consult it via
///   [`Context::is_git_tracked`].
/// - `git_blame` — per-file `git blame` cache, computed lazily
///   when at least one rule reports `wants_git_blame()`. `None`
///   when no rule asked for it. Rules consult it via
///   [`crate::git::BlameCache::get`]; both "outside a git repo"
///   and "blame failed for this file" surface as a `None`
///   lookup, which the rule treats as "silent no-op."
#[derive(Debug)]
pub struct Context<'a> {
    pub root: &'a Path,
    pub index: &'a FileIndex,
    pub registry: Option<&'a RuleRegistry>,
    pub facts: Option<&'a FactValues>,
    pub vars: Option<&'a HashMap<String, String>>,
    pub git_tracked: Option<&'a std::collections::HashSet<std::path::PathBuf>>,
    pub git_blame: Option<&'a crate::git::BlameCache>,
}

impl Context<'_> {
    /// True if `rel_path` is in git's index. Returns `false` when
    /// no tracked-set was computed (no git repo, or no rule asked
    /// for it). Rules that opt into `git_tracked_only` therefore
    /// silently skip every entry outside a git repo, which is the
    /// right behaviour for the canonical "don't let X be
    /// committed" use case.
    pub fn is_git_tracked(&self, rel_path: &Path) -> bool {
        match self.git_tracked {
            Some(set) => set.contains(rel_path),
            None => false,
        }
    }

    /// True if the directory at `rel_path` contains at least one
    /// git-tracked file. Used by `dir_*` rules opting into
    /// `git_tracked_only`. Same `None`-means-untracked semantics
    /// as [`Context::is_git_tracked`].
    pub fn dir_has_tracked_files(&self, rel_path: &Path) -> bool {
        match self.git_tracked {
            Some(set) => crate::git::dir_has_tracked_files(rel_path, set),
            None => false,
        }
    }
}

/// Trait every built-in and plugin rule implements.
pub trait Rule: Send + Sync + std::fmt::Debug {
    fn id(&self) -> &str;
    fn level(&self) -> Level;
    fn policy_url(&self) -> Option<&str> {
        None
    }
    /// Whether this rule needs the git-tracked-paths set on
    /// [`Context`]. Default `false`; rule kinds that support
    /// `git_tracked_only` override to return `true` only when
    /// the user actually opted in. The engine collects the set
    /// (via `git ls-files`) once per run when ANY rule returns
    /// `true`, so the cost is paid at most once even if many
    /// rules opt in.
    fn wants_git_tracked(&self) -> bool {
        false
    }

    /// Whether this rule needs `git blame` output on
    /// [`Context`]. Default `false`; the `git_blame_age` rule
    /// kind overrides to return `true`. The engine builds the
    /// shared [`crate::git::BlameCache`] once per run when any
    /// rule opts in, so multiple blame-aware rules over
    /// overlapping `paths:` re-use the parsed result.
    fn wants_git_blame(&self) -> bool {
        false
    }

    /// In `--changed` mode, return `true` to evaluate this rule
    /// against the **full** [`FileIndex`] rather than the
    /// changed-only filtered subset. Default `false` (per-file
    /// semantics — the rule sees only changed files in scope).
    ///
    /// Cross-file rules (`pair`, `for_each_dir`,
    /// `every_matching_has`, `unique_by`, `dir_contains`,
    /// `dir_only_contains`) override to `true` because their
    /// inputs span the whole tree by definition — a verdict on
    /// the changed file depends on what's still in the rest of
    /// the tree. Existence rules (`file_exists`, `file_absent`,
    /// `dir_exists`, `dir_absent`) likewise consult the whole
    /// tree to answer "is X present?" correctly.
    fn requires_full_index(&self) -> bool {
        false
    }

    /// In `--changed` mode, return the [`Scope`](crate::Scope)
    /// this rule is scoped to (typically the rule's `paths:`
    /// field). The engine intersects the scope with the
    /// changed-set; rules whose scope doesn't intersect are
    /// skipped, which is the optimisation `--changed` exists
    /// for.
    ///
    /// Default `None` ("no scope information") means the rule is
    /// always evaluated. Cross-file rules deliberately leave this
    /// as `None` (they always evaluate per the roadmap contract).
    /// Per-file rules with a single `Scope` field should override
    /// to return `Some(&self.scope)`.
    fn path_scope(&self) -> Option<&crate::scope::Scope> {
        None
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>>;

    /// Optional automatic-fix strategy. Rules whose violations can be
    /// mechanically corrected (e.g. creating a missing file, removing a
    /// forbidden one, renaming to the correct case) return a
    /// [`Fixer`] here; the default implementation reports the rule as
    /// unfixable.
    fn fixer(&self) -> Option<&dyn Fixer> {
        None
    }
}

/// Runtime context for applying a fix.
#[derive(Debug)]
pub struct FixContext<'a> {
    pub root: &'a Path,
    /// When true, fixers must describe what they would do without
    /// touching the filesystem.
    pub dry_run: bool,
    /// Max bytes a content-editing fix will read + rewrite.
    /// `None` means no cap. Honored by the `read_for_fix` helper
    /// (and any custom fixer that opts in).
    pub fix_size_limit: Option<u64>,
}

/// The result of applying (or simulating) one fix against one violation.
#[derive(Debug, Clone)]
pub enum FixOutcome {
    /// The fix was applied (or would be, under `dry_run`). The string
    /// is a human-readable one-liner — e.g. `"created LICENSE"`,
    /// `"would remove target/debug.log"`.
    Applied(String),
    /// The fixer intentionally did nothing; the string explains why
    /// (e.g. `"already exists"`, `"no path on violation"`). This is
    /// distinct from a hard error returned via `Result::Err`.
    Skipped(String),
}

/// A mechanical corrector for a specific rule's violations.
pub trait Fixer: Send + Sync + std::fmt::Debug {
    /// Short human-readable summary of what this fixer does,
    /// independent of any specific violation.
    fn describe(&self) -> String;

    /// Apply the fix against a single violation.
    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome>;
}

/// Result of [`read_for_fix`] — either the bytes of the file,
/// or a [`FixOutcome::Skipped`] the caller should return.
///
/// Content-editing fixers (`file_prepend`, `file_append`,
/// `file_trim_trailing_whitespace`, …) funnel their initial read
/// through this helper so the `fix_size_limit` guard is enforced
/// uniformly: over-limit files are reported as `Skipped` with a
/// clear reason, and a one-line warning is printed to stderr so
/// scripted runs notice.
#[derive(Debug)]
pub enum ReadForFix {
    Bytes(Vec<u8>),
    Skipped(FixOutcome),
}

/// Check whether `abs` is within the `fix_size_limit` on `ctx`.
/// Returns `Some(outcome)` when the file is over-limit (the
/// caller returns this directly); returns `None` when the fix
/// can proceed. Emits a one-line stderr warning on over-limit.
///
/// Use this in fixers that modify the file without reading the
/// full body (e.g. streaming append). For read-modify-write
/// flows, prefer [`read_for_fix`] which folds the check in.
pub fn check_fix_size(
    abs: &Path,
    display_path: &std::path::Path,
    ctx: &FixContext<'_>,
) -> Result<Option<FixOutcome>> {
    let Some(limit) = ctx.fix_size_limit else {
        return Ok(None);
    };
    let metadata = std::fs::metadata(abs).map_err(|source| crate::error::Error::Io {
        path: abs.to_path_buf(),
        source,
    })?;
    if metadata.len() > limit {
        let reason = format!(
            "{} is {} bytes; exceeds fix_size_limit ({}). Raise \
             `fix_size_limit` in .alint.yml (or set it to `null` to disable) \
             to fix files this large.",
            display_path.display(),
            metadata.len(),
            limit,
        );
        eprintln!("alint: warning: {reason}");
        return Ok(Some(FixOutcome::Skipped(reason)));
    }
    Ok(None)
}

/// Read `abs` subject to the size limit on `ctx`. Over-limit
/// files return `ReadForFix::Skipped(Outcome::Skipped(_))` and
/// emit a one-line stderr warning; in-limit files return
/// `ReadForFix::Bytes(...)`. Pass-through I/O errors propagate.
pub fn read_for_fix(
    abs: &Path,
    display_path: &std::path::Path,
    ctx: &FixContext<'_>,
) -> Result<ReadForFix> {
    if let Some(outcome) = check_fix_size(abs, display_path, ctx)? {
        return Ok(ReadForFix::Skipped(outcome));
    }
    let bytes = std::fs::read(abs).map_err(|source| crate::error::Error::Io {
        path: abs.to_path_buf(),
        source,
    })?;
    Ok(ReadForFix::Bytes(bytes))
}
