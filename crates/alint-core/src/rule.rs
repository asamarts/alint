use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::error::Result;
use crate::facts::FactValues;
use crate::level::Level;
use crate::registry::RuleRegistry;
use crate::walker::FileIndex;

/// A single linting violation produced by a rule.
///
/// `path` holds an [`Arc<Path>`]; rules clone the [`Arc`] from
/// [`FileEntry::path`](crate::walker::FileEntry::path) (a cheap
/// atomic refcount bump) rather than copying the path bytes. At
/// 100k violations this saves 100k path-byte allocations.
///
/// `message` is a [`Cow<'static, str>`]; per-match templated
/// messages live as `Cow::Owned(String)` (no change in cost),
/// while fixed messages can live as `Cow::Borrowed("…")` if a
/// rule chooses to construct them that way. Public API on the
/// struct is unchanged at the byte level — `Display` and serde
/// `Serialize` impls go through the inner `&str` / `&Path`.
#[derive(Debug, Clone)]
pub struct Violation {
    pub path: Option<Arc<Path>>,
    pub message: Cow<'static, str>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl Violation {
    pub fn new(message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            path: None,
            message: message.into(),
            line: None,
            column: None,
        }
    }

    /// Attach a path to the violation. Accepts anything convertible
    /// into `Arc<Path>` — the canonical caller is
    /// `.with_path(entry.path.clone())` where `entry.path` is the
    /// `Arc<Path>` already owned by the [`FileIndex`]; this clones
    /// the [`Arc`] (atomic refcount bump) rather than the bytes.
    /// `PathBuf`, `&Path`, and `Box<Path>` are also accepted via
    /// std's `From` impls; for an ad-hoc `&str` use
    /// `Path::new("a.rs")` to convert first.
    #[must_use]
    pub fn with_path(mut self, path: impl Into<Arc<Path>>) -> Self {
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
///
/// `rule_id` holds an [`Arc<str>`]: the engine builds it once
/// per rule run and shares it across every violation that rule
/// produces, saving N-1 allocations per rule. `policy_url`
/// follows the same shape via [`Arc<str>`] — set once per rule,
/// shared across violations.
#[derive(Debug, Clone)]
pub struct RuleResult {
    pub rule_id: Arc<str>,
    pub level: Level,
    pub policy_url: Option<Arc<str>>,
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

/// How a rule narrows its iteration to git-tracked entries.
/// Returned by [`Rule::git_tracked_mode`]; the engine reads
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

/// Trait every built-in and plugin rule implements.
pub trait Rule: Send + Sync + std::fmt::Debug {
    fn id(&self) -> &str;
    fn level(&self) -> Level;
    fn policy_url(&self) -> Option<&str> {
        None
    }
    /// Whether (and how) this rule narrows its iteration to
    /// git-tracked entries. Default [`GitTrackedMode::Off`].
    /// Rule kinds that support `git_tracked_only:` override to
    /// return [`GitTrackedMode::FileOnly`] (file-mode rules:
    /// check `set.contains(path)`) or [`GitTrackedMode::DirAware`]
    /// (dir-mode rules: check `dir_has_tracked_files(path, set)`)
    /// when the user opts in.
    ///
    /// The engine collects the tracked-paths set (via
    /// `git ls-files`) once per run when ANY rule returns a
    /// non-`Off` mode, then builds a pre-filtered `FileIndex`
    /// for each mode and routes opted-in rules to the right
    /// `Context`. Rules iterate `ctx.index.files()` /
    /// `ctx.index.dirs()` exactly as before — the index is
    /// already narrowed, so no per-rule `if self.git_tracked_only
    /// && !ctx.is_git_tracked(...)` runtime check is needed.
    /// Closes the same recurrence-risk shape as v0.9.10's
    /// `Scope`-owns-`scope_filter` fix:
    /// `docs/design/v0.9/git-tracked-filtered-index.md`.
    fn git_tracked_mode(&self) -> GitTrackedMode {
        GitTrackedMode::Off
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

    /// Opt into the file-major dispatch path. Per-file rules that
    /// can evaluate one file at a time given a pre-loaded byte
    /// slice override this to return `Some(self)`; cross-file
    /// rules and any rule with `requires_full_index() == true`
    /// leave it as `None` and keep evaluating under the rule-
    /// major loop.
    ///
    /// When the engine has multiple per-file rules sharing one
    /// scope, the file-major loop reads each matched file once
    /// and dispatches to every applicable per-file rule against
    /// the same byte buffer — coalescing N reads of one file
    /// into 1.
    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        None
    }
}

/// File-major dispatch entry-point for a per-file rule.
///
/// Rules that can evaluate one file at a time given a pre-loaded
/// byte slice implement this trait alongside [`Rule`] and opt
/// into the file-major path via [`Rule::as_per_file`]. The
/// engine reads each file once per evaluation pass and calls
/// `evaluate_file` on every per-file rule whose
/// [`path_scope`](PerFileRule::path_scope) matches that file —
/// avoiding the per-rule `std::fs::read` the rule-major loop
/// would otherwise duplicate.
///
/// Implementations MUST NOT call `std::fs::read` themselves; the
/// `bytes` argument is the engine's already-read content. The
/// rule's existing [`Rule::evaluate`] implementation (which does
/// read the file) stays in place as the rule-major fallback —
/// it's still the path used by `alint fix` (sequential
/// filesystem mutation rules out coalesced reads there) and by
/// fallback test harnesses.
pub trait PerFileRule: Send + Sync + std::fmt::Debug {
    /// The rule's scope. The engine checks
    /// `path_scope().matches(path)` before calling
    /// `evaluate_file`; a rule that returns
    /// [`Scope::match_all`](crate::scope::Scope::match_all) is
    /// in scope for every file.
    fn path_scope(&self) -> &crate::scope::Scope;

    /// Evaluate one file given the engine's already-read byte
    /// content. The `path` is the relative path from the lint
    /// root; the rule should `with_path(path.into())` (or clone
    /// the matched [`FileEntry::path`](crate::walker::FileEntry::path)
    /// if it has one in hand) on emitted violations.
    fn evaluate_file(&self, ctx: &Context<'_>, path: &Path, bytes: &[u8])
    -> Result<Vec<Violation>>;

    /// Optional lower bound on the bytes the rule needs to
    /// evaluate. Default `None` means "I need the whole file."
    /// Used as a hint; the engine in v0.9.3 reads the whole
    /// file regardless and hands it to every applicable rule —
    /// the hint is reserved for a future engine-side bounded-
    /// read optimisation.
    fn max_bytes_needed(&self) -> Option<usize> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_index() -> FileIndex {
        FileIndex::default()
    }

    #[test]
    fn violation_builder_sets_fields_via_chain() {
        let v = Violation::new("trailing whitespace")
            .with_path(Path::new("src/main.rs"))
            .with_location(12, 4);
        assert_eq!(v.message, "trailing whitespace");
        assert_eq!(v.path.as_deref(), Some(Path::new("src/main.rs")));
        assert_eq!(v.line, Some(12));
        assert_eq!(v.column, Some(4));
    }

    #[test]
    fn violation_new_starts_with_no_path_or_location() {
        let v = Violation::new("global note");
        assert!(v.path.is_none());
        assert!(v.line.is_none());
        assert!(v.column.is_none());
    }

    #[test]
    fn rule_result_passed_iff_violations_empty() {
        let mut r = RuleResult {
            rule_id: "x".into(),
            level: Level::Error,
            policy_url: None,
            violations: Vec::new(),
            is_fixable: false,
        };
        assert!(r.passed());
        r.violations.push(Violation::new("oops"));
        assert!(!r.passed());
    }

    #[test]
    fn context_is_git_tracked_returns_false_outside_repo() {
        let idx = empty_index();
        let ctx = Context {
            root: Path::new("/tmp"),
            index: &idx,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None, // outside-a-repo / no rule opted in
            git_blame: None,
        };
        assert!(!ctx.is_git_tracked(Path::new("anything.rs")));
        assert!(!ctx.dir_has_tracked_files(Path::new("src")));
    }

    #[test]
    fn context_is_git_tracked_consults_set_when_present() {
        let mut tracked: std::collections::HashSet<std::path::PathBuf> =
            std::collections::HashSet::new();
        tracked.insert(std::path::PathBuf::from("src/main.rs"));
        let idx = empty_index();
        let ctx = Context {
            root: Path::new("/tmp"),
            index: &idx,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: Some(&tracked),
            git_blame: None,
        };
        assert!(ctx.is_git_tracked(Path::new("src/main.rs")));
        assert!(!ctx.is_git_tracked(Path::new("README.md")));
    }

    /// Stand-in `Rule` impl that returns the trait defaults.
    /// Lets us assert the documented defaults without dragging
    /// in a real registered rule.
    #[derive(Debug)]
    struct DefaultRule;

    impl Rule for DefaultRule {
        fn id(&self) -> &'static str {
            "default"
        }
        fn level(&self) -> Level {
            Level::Warning
        }
        fn evaluate(&self, _ctx: &Context<'_>) -> Result<Vec<Violation>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn rule_trait_defaults_are_safe_no_ops() {
        let r = DefaultRule;
        assert_eq!(r.policy_url(), None);
        assert_eq!(r.git_tracked_mode(), GitTrackedMode::Off);
        assert!(!r.wants_git_blame());
        assert!(!r.requires_full_index());
        assert!(r.path_scope().is_none());
        assert!(r.fixer().is_none());
    }

    #[test]
    fn check_fix_size_returns_none_when_limit_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        std::fs::write(&f, b"hello").unwrap();
        let ctx = FixContext {
            root: dir.path(),
            dry_run: false,
            fix_size_limit: None,
        };
        let outcome = check_fix_size(&f, Path::new("a.txt"), &ctx).unwrap();
        assert!(outcome.is_none());
    }

    #[test]
    fn check_fix_size_skips_over_limit_files() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("big.txt");
        std::fs::write(&f, vec![b'x'; 1024]).unwrap();
        let ctx = FixContext {
            root: dir.path(),
            dry_run: false,
            fix_size_limit: Some(64),
        };
        let outcome = check_fix_size(&f, Path::new("big.txt"), &ctx).unwrap();
        match outcome {
            Some(FixOutcome::Skipped(reason)) => {
                assert!(reason.contains("exceeds fix_size_limit"));
                assert!(reason.contains("big.txt"));
            }
            other => panic!("expected Skipped, got {other:?}"),
        }
    }

    #[test]
    fn read_for_fix_returns_bytes_when_in_limit() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        std::fs::write(&f, b"hello").unwrap();
        let ctx = FixContext {
            root: dir.path(),
            dry_run: false,
            fix_size_limit: Some(1 << 20),
        };
        match read_for_fix(&f, Path::new("a.txt"), &ctx).unwrap() {
            ReadForFix::Bytes(b) => assert_eq!(b, b"hello"),
            ReadForFix::Skipped(_) => panic!("expected Bytes, got Skipped"),
        }
    }

    #[test]
    fn read_for_fix_returns_skipped_when_over_limit() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("big.txt");
        std::fs::write(&f, vec![b'x'; 1024]).unwrap();
        let ctx = FixContext {
            root: dir.path(),
            dry_run: false,
            fix_size_limit: Some(64),
        };
        match read_for_fix(&f, Path::new("big.txt"), &ctx).unwrap() {
            ReadForFix::Skipped(FixOutcome::Skipped(_)) => {}
            ReadForFix::Skipped(FixOutcome::Applied(_)) => {
                panic!("expected Skipped, got Skipped(Applied)")
            }
            ReadForFix::Bytes(_) => panic!("expected Skipped, got Bytes"),
        }
    }

    #[test]
    fn fix_outcome_variants_are_constructible() {
        // Sanity: documented variant shapes haven't drifted.
        let _applied = FixOutcome::Applied("created LICENSE".into());
        let _skipped = FixOutcome::Skipped("already exists".into());
    }
}
