use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::facts::FactValues;
use crate::level::Level;
use crate::location::{RelatedLocation, SourceRange};
use crate::registry::RuleRegistry;
use crate::walker::FileIndex;

/// Classification for a non-violation finding.
///
/// Formatters use the structural kind rather than matching message text:
/// ordinary notes are informational findings, while truncated analysis is an
/// execution condition (SARIF projects it as a tool notification).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    Informational,
    AnalysisTruncated,
}

impl NoteKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Informational => "informational",
            Self::AnalysisTruncated => "analysis_truncated",
        }
    }
}

/// Stable structural class for an analysis failure.
///
/// Unlike a policy violation, an analysis failure means alint did not obtain
/// the configured verdict. It is always error-bearing and never baselinable or
/// fixable, regardless of the owning rule's configured level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisFailureKind {
    Evaluation,
    Condition,
    ResourceRead,
    ResourceParse,
    NestedRuleBuild,
}

impl AnalysisFailureKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Evaluation => "evaluation",
            Self::Condition => "condition",
            Self::ResourceRead => "resource_read",
            Self::ResourceParse => "resource_parse",
            Self::NestedRuleBuild => "nested_rule_build",
        }
    }
}

/// Internal producer classification for a finding returned through the
/// existing `Result<Vec<Violation>>` rule API.
///
/// [`RuleResult::new`] moves notes and analysis failures into their dedicated
/// collections before reporting, baseline, or fix processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FindingDisposition {
    #[default]
    Violation,
    Note(NoteKind),
    AnalysisFailure(AnalysisFailureKind),
}

/// A rule evaluation that could not produce a policy verdict.
#[derive(Debug, Clone)]
pub struct AnalysisFailure {
    pub kind: AnalysisFailureKind,
    pub path: Option<Arc<Path>>,
    pub message: Cow<'static, str>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    sensitive: bool,
    path_sensitive: bool,
}

impl AnalysisFailure {
    #[must_use]
    pub fn new(kind: AnalysisFailureKind, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            kind,
            path: None,
            message: message.into(),
            line: None,
            column: None,
            sensitive: false,
            path_sensitive: false,
        }
    }

    /// Whether this failure originated from secret-bearing evidence.
    #[must_use]
    pub fn is_sensitive(&self) -> bool {
        self.sensitive
    }

    /// Path safe for public reporters and diagnostics.
    #[must_use]
    pub fn report_path(&self) -> Option<&Path> {
        self.path.as_deref().map(|path| {
            if self.path_sensitive {
                Path::new("<redacted>")
            } else {
                path
            }
        })
    }

    pub(crate) fn mark_path_sensitive(&mut self) {
        self.path_sensitive = true;
    }

    /// Whether public consumers must suppress the primary path.
    #[must_use]
    pub const fn path_is_sensitive(&self) -> bool {
        self.path_sensitive
    }

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

    /// Replace repository-controlled detail with the standard sensitive marker.
    pub fn redact_message(&mut self, rule_id: &str) {
        let path = self
            .path
            .as_deref()
            .map_or_else(String::new, |path| path.to_string_lossy().into_owned());
        let locator = format!(
            "explicit\0{rule_id}\0analysis_failure\0{}\0{path}",
            self.kind.as_str()
        );
        self.message = format!("sensitive evidence {}", crate::redaction_marker(&locator)).into();
        self.sensitive = true;
    }

    fn from_finding(finding: Violation, kind: AnalysisFailureKind) -> Self {
        Self {
            kind,
            path: finding.path,
            message: finding.message,
            line: finding.line,
            column: finding.column,
            sensitive: finding.sensitive,
            path_sensitive: finding.path_sensitive,
        }
    }
}

/// A single linting violation or non-violation note produced by a rule.
///
/// `path` holds an [`Arc<Path>`]; rules clone the [`Arc`] from
/// [`FileEntry::path`](crate::walker::FileEntry::path) (a cheap
/// atomic refcount bump) rather than copying the path bytes. At
/// 100k violations this saves 100k path-byte allocations.
///
/// `message` is a [`Cow<'static, str>`]; per-match templated
/// messages live as `Cow::Owned(String)` (no change in cost),
/// while fixed messages can live as `Cow::Borrowed("…")` if a
/// rule chooses to construct them that way. Renderers consume the inner
/// `&str`, so the ownership choice does not alter report bytes.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct Violation {
    pub path: Option<Arc<Path>>,
    pub message: Cow<'static, str>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Optional half-open primary range. The legacy point fields project its start.
    pub range: Option<SourceRange>,
    /// Canonically ordered, bounded secondary locations.
    pub related_locations: Vec<RelatedLocation>,
    /// Number of secondary locations omitted by the model cap.
    pub related_locations_omitted: usize,
    /// Producer-side classification. The engine moves notes and analysis
    /// failures out of the violation collection before exposing a result.
    pub disposition: FindingDisposition,
    /// Optional stable structural identity for baseline fingerprinting
    /// ([`crate::baseline`]). A rule sets this when its violation is not
    /// uniquely identified by `(path, offending-line content)` — e.g. a
    /// structured-query rule (the JSONPath/value), a cross-file rule (the
    /// sorted involved paths), or a first-offender / threshold rule (the
    /// path). `None` (the default) means "use the offending line's
    /// content" (see [`crate::baseline::violation_fingerprint`]). It
    /// never affects rendering or pass/fail.
    pub baseline_key: Option<Cow<'static, str>>,
    /// Opaque check-to-fix correlation data. Unlike `baseline_key`, this never
    /// contributes to fingerprinting or output; it identifies one concrete
    /// extracted occurrence when equal values share a path.
    #[doc(hidden)]
    pub fix_key: Option<Cow<'static, str>>,
    /// Whether dynamic evidence has been redacted from this finding. Producers
    /// set this only through [`Self::redact_evidence`] or
    /// [`Self::redact_message`], which also remove reportable replacement bytes.
    /// The original baseline key remains available to remediation internals;
    /// fingerprinting ignores it whenever this flag is set.
    pub sensitive: bool,
    /// Whether the primary path came from protected configuration. Internal
    /// remediation and baseline logic retain [`Self::path`]; public consumers
    /// MUST use [`Self::report_path`] instead.
    #[doc(hidden)]
    pub path_sensitive: bool,
    /// Engine-computed: whether a bare `alint fix` would resolve *this*
    /// violation. A rule sets this to `false` at construction (rules do not know
    /// about fixers); the engine overwrites it at result-assembly time as
    /// per-violation convertibility ([`Fixer::can_fix`]) AND the fixer applying
    /// at the default (`Safe`) threshold. So an unconvertible `café.rs` under
    /// `snake` (which `fix` skips) is not tagged `fixable`, and neither is an
    /// `Unsafe`-tier `file_remove` violation (which a bare `fix` only *suggests*,
    /// needing `--unsafe-fixes`) -- `check` must not promise a resolution a bare
    /// `fix` never delivers. A convertible Safe-tier sibling still is. `false`
    /// whenever the rule has no fixer. The rule-level [`RuleResult::is_fixable`]
    /// ("the rule declares a fixer") is independent and backs the machine formats.
    pub is_fixable: bool,
    /// Rule-set: this finding can NOT be auto-fixed even though the rule declares
    /// a fixer (e.g. a bidi control in a binary-looking file, a UTF-16 BOM that
    /// would need transcoding, a header missing from a non-UTF-8 file). The engine
    /// then never tags it [`Violation::is_fixable`], and `alint fix` reports it as
    /// unfixable without invoking the fixer. Carried as a flag -- NOT folded into
    /// [`Violation::baseline_key`] -- so fixability never changes a finding's
    /// baseline fingerprint. Defaults to `false`.
    pub not_fixable: bool,
    /// Check-side-computed concrete edits a fix would make to resolve *this*
    /// violation, for the machine formats (SARIF `result.fixes[]`) to render as
    /// source region + replacement text. Empty by default and for every
    /// non-fixable violation; the CLI populates it (via
    /// [`crate::proposed_fix::attach_proposed_edits`]) only for a fix-carrying
    /// format, so the ordinary check path pays nothing. Never affects rendering
    /// of the finding itself, pass/fail, or fixability.
    pub proposed_edits: Vec<crate::proposed_fix::ProposedEdit>,
}

impl Violation {
    pub fn new(message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            path: None,
            message: message.into(),
            line: None,
            column: None,
            range: None,
            related_locations: Vec::new(),
            related_locations_omitted: 0,
            disposition: FindingDisposition::Violation,
            baseline_key: None,
            fix_key: None,
            sensitive: false,
            path_sensitive: false,
            is_fixable: false,
            not_fixable: false,
            proposed_edits: Vec::new(),
        }
    }

    /// Mark this finding as not auto-fixable. See [`Violation::not_fixable`].
    #[must_use]
    pub fn with_not_fixable(mut self) -> Self {
        self.not_fixable = true;
        self
    }

    /// [`Violation::with_not_fixable`] when `cond` holds; unchanged otherwise.
    #[must_use]
    pub fn with_not_fixable_if(self, cond: bool) -> Self {
        if cond { self.with_not_fixable() } else { self }
    }

    /// Mark this finding as an informational note rather than a violation.
    #[must_use]
    pub fn as_note(mut self) -> Self {
        self.disposition = FindingDisposition::Note(NoteKind::Informational);
        self
    }

    /// Mark this finding as a bounded-analysis truncation note.
    #[must_use]
    pub fn as_analysis_truncated_note(mut self) -> Self {
        self.disposition = FindingDisposition::Note(NoteKind::AnalysisTruncated);
        self
    }

    /// Mark this producer finding as an analysis failure. Result assembly moves
    /// it into [`RuleResult::analysis_failures`], so it can never reach
    /// baseline or fix processing as a policy violation.
    #[must_use]
    pub fn as_analysis_failure(mut self, kind: AnalysisFailureKind) -> Self {
        self.disposition = FindingDisposition::AnalysisFailure(kind);
        self.baseline_key = None;
        self.fix_key = None;
        self.is_fixable = false;
        self.not_fixable = true;
        self.proposed_edits.clear();
        self
    }

    /// Mark a value-free engine-generated finding as sensitive while preserving
    /// its structural message. Producer findings must use [`Self::redact_evidence`]
    /// or [`Self::redact_message`] instead.
    pub(crate) fn mark_sensitive(&mut self, _locator: &str) {
        self.sensitive = true;
        self.proposed_edits.clear();
    }

    /// Apply the explicit `sensitive: true` policy to a flattened finding.
    pub fn redact_message(&mut self, rule_id: &str) {
        let path = self
            .path
            .as_deref()
            .map_or_else(String::new, |path| path.to_string_lossy().into_owned());
        let channel = match self.disposition {
            FindingDisposition::Violation => "violation",
            FindingDisposition::Note(kind) => kind.as_str(),
            FindingDisposition::AnalysisFailure(kind) => kind.as_str(),
        };
        let locator = format!("explicit\0{rule_id}\0{channel}\0{path}");
        self.message = format!("sensitive evidence {}", crate::redaction_marker(&locator)).into();
        self.sensitive = true;
        self.proposed_edits.clear();
    }

    /// Redact only known evidence substrings while preserving fixed explanatory
    /// text. `locator` must be structural and value-free.
    pub fn redact_evidence<'a>(
        &mut self,
        rule_id: &str,
        locator: &str,
        evidence: impl IntoIterator<Item = &'a str>,
    ) {
        let locator = format!("implicit\0{rule_id}\0{locator}");
        let mut rendered = Vec::new();
        for value in evidence {
            if value.is_empty() {
                continue;
            }
            let debug = format!("{value:?}");
            // Built-in messages commonly render extracted strings with `Debug`
            // so controls and quotes cannot forge report structure. Include
            // both forms: a message can contain the escaped rendering and a
            // raw/normalized occurrence of the same evidence.
            rendered.push(value.to_string());
            if debug != value {
                rendered.push(debug);
            }
        }
        self.message =
            crate::redact_evidence(&self.message, &locator, rendered.iter().map(String::as_str))
                .into();
        self.sensitive = true;
        self.proposed_edits.clear();
    }

    /// Builder form of [`Self::redact_message`].
    #[must_use]
    pub fn with_redacted_message(mut self, rule_id: &str) -> Self {
        self.redact_message(rule_id);
        self
    }

    /// Whether this finding contains evidence that must remain redacted on every
    /// public surface.
    #[must_use]
    pub fn is_sensitive(&self) -> bool {
        self.sensitive
    }

    /// Primary path safe for public reporters and diagnostics. Baseline and
    /// remediation internals use [`Self::path`] so redaction never changes
    /// identity or a write target.
    #[must_use]
    pub fn report_path(&self) -> Option<&Path> {
        self.path.as_deref().map(|path| {
            if self.path_sensitive {
                Path::new("<redacted>")
            } else {
                path
            }
        })
    }

    /// Whether public consumers must suppress the primary and related paths.
    #[must_use]
    pub const fn path_is_sensitive(&self) -> bool {
        self.path_sensitive
    }

    pub(crate) fn mark_path_sensitive(&mut self) {
        self.path_sensitive = true;
    }

    /// Remove internal-only evidence from a sensitive finding before an engine
    /// result crosses a public API boundary. Fixers need the original key and
    /// related targets while they correlate a finding with an edit, but callers
    /// must never receive value-bearing metadata. The replacement key is derived
    /// only from structural metadata; the original key is not hashed.
    pub(crate) fn sanitize_sensitive_metadata(&mut self, rule_id: &str) {
        if !self.sensitive {
            return;
        }
        let path = self
            .path
            .as_deref()
            .map_or_else(String::new, |path| path.to_string_lossy().into_owned());
        let channel = match self.disposition {
            FindingDisposition::Violation => "violation",
            FindingDisposition::Note(kind) => kind.as_str(),
            FindingDisposition::AnalysisFailure(kind) => kind.as_str(),
        };
        let locator = format!("public\0{rule_id}\0{channel}\0{path}");
        self.baseline_key =
            Some(format!("\0alint-sensitive\0{}", crate::redaction_marker(&locator)).into());
        self.fix_key = None;
        self.proposed_edits.clear();
        // A secondary path may itself be an extracted value (for example, a
        // secret-bearing registry entry interpreted as an unresolved target).
        // Keep the primary source path navigable, but fail closed on every
        // secondary locator because the model cannot distinguish evidence-
        // derived paths from independently known repository locations.
        self.related_locations.clear();
        self.related_locations_omitted = 0;
    }

    /// Structural note kind, if this producer finding is a note.
    #[must_use]
    pub fn note_kind(&self) -> Option<NoteKind> {
        match self.disposition {
            FindingDisposition::Note(kind) => Some(kind),
            FindingDisposition::Violation | FindingDisposition::AnalysisFailure(_) => None,
        }
    }

    /// Structural analysis-failure kind, if this producer finding is a failure.
    #[must_use]
    pub fn analysis_failure_kind(&self) -> Option<AnalysisFailureKind> {
        match self.disposition {
            FindingDisposition::AnalysisFailure(kind) => Some(kind),
            FindingDisposition::Violation | FindingDisposition::Note(_) => None,
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
        self.range = None;
        self
    }

    #[must_use]
    pub fn with_range(mut self, mut range: SourceRange) -> Self {
        let identity_line = self
            .range
            .as_ref()
            .map_or(self.line, SourceRange::baseline_identity_line);
        range.set_baseline_identity_line(identity_line);
        self.line = Some(range.start_line());
        self.column = Some(range.start_column());
        self.range = Some(range);
        self
    }

    /// Attach a single secondary location as the complete location batch. Use
    /// [`Self::with_related_locations`] when producing a collection.
    #[must_use]
    pub fn with_related_location(self, location: RelatedLocation) -> Self {
        self.with_related_locations(std::iter::once(location))
    }

    /// Attach a complete batch of secondary locations.
    ///
    /// The batch is sorted and deduplicated before the canonical first 20 are
    /// retained, so the omitted count is the number of remaining distinct
    /// locations and is independent of producer iteration order.
    #[must_use]
    pub fn with_related_locations(
        mut self,
        locations: impl IntoIterator<Item = RelatedLocation>,
    ) -> Self {
        self.set_related_locations(locations);
        self
    }

    /// Replace secondary locations from one complete producer batch.
    pub fn set_related_locations(&mut self, locations: impl IntoIterator<Item = RelatedLocation>) {
        let mut locations: Vec<_> = locations.into_iter().collect();
        locations.sort_unstable();
        locations.dedup();
        self.related_locations_omitted =
            locations.len().saturating_sub(crate::MAX_RELATED_LOCATIONS);
        locations.truncate(crate::MAX_RELATED_LOCATIONS);
        self.related_locations = locations;
    }

    /// Set the baseline fingerprint key — the rule's stable structural
    /// identity for this violation. See [`Violation::baseline_key`] and
    /// [`crate::baseline::violation_fingerprint`].
    #[must_use]
    pub fn with_baseline_key(mut self, key: impl Into<Cow<'static, str>>) -> Self {
        self.baseline_key = Some(key.into());
        self
    }

    /// Attach opaque occurrence identity for the owning fixer.
    #[must_use]
    pub fn with_fix_key(mut self, key: impl Into<Cow<'static, str>>) -> Self {
        self.fix_key = Some(key.into());
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
    /// Analysis that did not produce a policy verdict. Always error-bearing,
    /// never baselinable or fixable, independent of [`Self::level`].
    pub analysis_failures: Vec<AnalysisFailure>,
    /// Informational notes (non-violation findings) the rule
    /// produced — e.g. entries it skipped rather than failed on.
    /// Partitioned out of the rule's raw output by
    /// [`RuleResult::new`]; never counted in pass/fail.
    pub notes: Vec<Violation>,
    /// Whether the rule declares a [`Fixer`] — surfaced here so
    /// the human formatter can tag violations as `fixable`
    /// without threading the rule registry into the renderer.
    pub is_fixable: bool,
}

impl RuleResult {
    /// Build a result from a rule's raw output, partitioning analysis failures
    /// and notes into dedicated collections. Centralises the split so policy
    /// violation formatters, baseline logic, and fixers cannot mistake either
    /// class for a suppressible or remediable violation.
    #[must_use]
    pub fn new(
        rule_id: Arc<str>,
        level: Level,
        policy_url: Option<Arc<str>>,
        mut raw: Vec<Violation>,
        is_fixable: bool,
    ) -> Self {
        // Reuse the producer's allocation for the overwhelmingly common
        // violation-only path. Extract every non-violation in one traversal so
        // successful analysis does not pay two scans of a large finding set.
        // `extract_if` preserves the relative order of retained and extracted
        // findings.
        let mut analysis_failures = Vec::new();
        let mut notes = Vec::new();
        for finding in raw.extract_if(.., |finding| {
            !matches!(finding.disposition, FindingDisposition::Violation)
        }) {
            match finding.disposition {
                FindingDisposition::AnalysisFailure(kind) => {
                    analysis_failures.push(AnalysisFailure::from_finding(finding, kind));
                }
                FindingDisposition::Note(_) => notes.push(finding),
                FindingDisposition::Violation => {
                    unreachable!("extract predicate excludes policy violations");
                }
            }
        }
        let violations = raw;
        Self {
            rule_id,
            level,
            policy_url,
            violations,
            analysis_failures,
            notes,
            is_fixable,
        }
    }

    /// Apply the common `sensitive: true` policy after finding partitioning.
    pub fn redact_sensitive(&mut self) {
        for finding in self.violations.iter_mut().chain(&mut self.notes) {
            finding.redact_message(&self.rule_id);
        }
        for failure in &mut self.analysis_failures {
            failure.redact_message(&self.rule_id);
        }
    }

    /// Hide primary paths derived from protected selector configuration while
    /// preserving their internal identity for baselines and remediation.
    pub(crate) fn redact_sensitive_paths(&mut self) {
        for finding in self.violations.iter_mut().chain(&mut self.notes) {
            finding.mark_path_sensitive();
        }
        for failure in &mut self.analysis_failures {
            failure.mark_path_sensitive();
        }
    }

    /// Hide a configured policy URL whose winning value came from protected
    /// configuration while preserving that documentation was configured.
    pub(crate) fn redact_policy_url(&mut self) {
        if self.policy_url.is_some() {
            self.policy_url = Some(Arc::from("<redacted>"));
        }
    }

    /// Mark findings selected through a secret-bearing manifest scope. The
    /// selected path is the only manifest-derived evidence available at this
    /// boundary, so redact it from flattened messages while retaining fixed
    /// prose. Analysis failures are conservatively redacted in full.
    pub(crate) fn redact_manifest_scope(&mut self) {
        for (index, finding) in self
            .violations
            .iter_mut()
            .chain(&mut self.notes)
            .enumerate()
        {
            let path = finding
                .path
                .as_deref()
                .map(|path| path.to_string_lossy().into_owned());
            let locator = format!("manifest_scope\0{index}");
            finding.redact_evidence(&self.rule_id, &locator, path.as_deref());
        }
        for failure in &mut self.analysis_failures {
            failure.redact_message(&self.rule_id);
        }
    }

    /// Strip internal sensitive identity after remediation and before returning
    /// this result to a caller.
    pub(crate) fn sanitize_sensitive_metadata(&mut self) {
        for finding in self.violations.iter_mut().chain(&mut self.notes) {
            finding.sanitize_sensitive_metadata(&self.rule_id);
        }
    }

    pub fn passed(&self) -> bool {
        self.violations.is_empty() && self.analysis_failures.is_empty()
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
///   computed once per run when an active rule declares a Git capability.
///   `None` means no rule asked for it or repository probing failed. The engine
///   turns the latter into one analysis failure per active owning rule before
///   dispatch; direct rule-unit tests can still supply `None`.
/// - `git_blame` — per-file `git blame` cache, computed lazily when at least
///   one rule reports `wants_git_blame()`. Repository unavailability is gated
///   before dispatch; an individual untracked or unblameable file remains a
///   `None` lookup and is skipped by the rule.
/// - `analysis_time` — one explicit or HEAD-derived reference instant for
///   time-dependent rules. Resolution failures stay structural until a rule
///   actually asks for the value.
/// - `trace` — optional selected-rule diagnostic sink. Ordinary check/fix/LSP
///   contexts always carry `None`.
#[derive(Debug)]
pub struct Context<'a> {
    pub root: &'a Path,
    pub index: &'a FileIndex,
    pub registry: Option<&'a RuleRegistry>,
    pub facts: Option<&'a FactValues>,
    pub vars: Option<&'a HashMap<String, String>>,
    pub git_tracked: Option<&'a std::collections::HashSet<std::path::PathBuf>>,
    pub git_blame: Option<&'a crate::git::BlameCache>,
    pub analysis_time: Option<&'a crate::AnalysisTimeResolution>,
    /// Complete child-process environment. `None` inherits the host (normal
    /// production behavior); `Some` means callers must clear the inherited
    /// environment before applying this map.
    pub process_environment: Option<&'a std::collections::BTreeMap<String, String>>,
    #[doc(hidden)]
    pub trace: Option<&'a dyn crate::trace::TraceSink>,
}

impl Context<'_> {
    /// True if `rel_path` is in git's index. Returns `false` when no tracked set
    /// was supplied. Production engine dispatch fails an active Git-dependent
    /// owning rule before this method is reached when repository probing failed;
    /// the fallback keeps direct rule evaluation total.
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
    /// Emit one already-sanitized kind-owned stage when selected-rule tracing is
    /// active. The sink rejects engine-owned gate and outcome stages.
    pub fn record_trace(&self, stage: crate::TraceStage) {
        if let Some(trace) = self.trace {
            trace.record_kind(stage);
        }
    }

    /// Add one trace-only extraction observation. Callers invoke this only from
    /// trace-specific evaluation entry points; ordinary per-file dispatch never
    /// executes this method.
    #[doc(hidden)]
    pub fn record_trace_extraction(&self, observation: crate::TraceExtractionObservation) {
        self.trace
            .expect("trace-specific evaluation requires a trace sink")
            .record_extraction(observation);
    }
    /// Configured evidence-row limit for the selected trace, if requested.
    #[doc(hidden)]
    pub fn trace_preview_limit(&self) -> Option<usize> {
        self.trace.and_then(crate::TraceSink::preview_limit)
    }

    /// Whether the selected trace requested evidence rows.
    #[doc(hidden)]
    pub fn trace_preview_enabled(&self) -> bool {
        self.trace_preview_limit().is_some()
    }

    /// Combine producer-local sensitivity with the effective rule's config
    /// provenance. This is called before constructing a retained preview row.
    #[doc(hidden)]
    pub fn trace_evidence_sensitivity(&self, producer_sensitive: bool) -> crate::TraceSensitivity {
        self.trace
            .expect("trace-specific evaluation requires a trace sink")
            .evidence_sensitivity(producer_sensitive)
    }

    /// Path origin adjusted for protected selector configuration.
    #[doc(hidden)]
    pub fn trace_path_origin(&self) -> crate::TracePathOrigin {
        if self
            .trace
            .expect("trace-specific evaluation requires a trace sink")
            .paths_are_sensitive()
        {
            crate::TracePathOrigin::DerivedSensitive
        } else {
            crate::TracePathOrigin::RepositorySource
        }
    }

    /// Retain one already bounded/redacted preview candidate.
    #[doc(hidden)]
    pub fn record_trace_preview(&self, row: crate::Result<crate::TracePreviewRow>) {
        self.trace
            .expect("trace-specific evaluation requires a trace sink")
            .record_preview(row);
    }

    /// Return the deterministic reference instant requested by this rule.
    ///
    /// An unavailable reference is returned as an error; engine result assembly
    /// converts it to a typed analysis failure.
    pub fn analysis_time(&self) -> Result<crate::AnalysisTime> {
        let resolution = self.analysis_time.ok_or_else(|| {
            Error::internal("rule requested analysis time without declaring the capability")
        })?;
        match *resolution {
            crate::AnalysisTimeResolution::Available { time, .. } => Ok(time),
            crate::AnalysisTimeResolution::Unavailable(error) => {
                Err(Error::Other(format!("analysis time unavailable: {error}")))
            }
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

/// Stamp out the three boilerplate `Rule` impl methods every rule
/// kind ships: `id`, `level`, `policy_url`. Expects the impl'ing
/// struct to have fields named `id: String`, `level: Level`, and
/// `policy_url: Option<String>` (the universal shape every rule
/// builder hands back).
///
/// Usage inside an `impl Rule for SomeRule` block:
///
/// ```ignore
/// impl Rule for FileExistsRule {
///     alint_core::rule_common_impl!();
///
///     fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> { ... }
///     // ... other trait methods specific to this rule
/// }
/// ```
///
/// The macro only covers the three universal methods. `fixer()` /
/// `as_per_file()` / `evaluate()` / `requires_full_index()` etc.
/// stay explicit per-rule because they encode the rule's actual
/// behaviour, not boilerplate.
#[macro_export]
macro_rules! rule_common_impl {
    () => {
        fn id(&self) -> &str {
            &self.id
        }
        fn level(&self) -> $crate::Level {
            self.level
        }
        fn policy_url(&self) -> Option<&str> {
            self.policy_url.as_deref()
        }
    };
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

    /// Whether this rule consumes the deterministic analysis reference time.
    /// The engine resolves an explicit value or HEAD committer time once and
    /// shares it across every context and fix pass.
    fn wants_analysis_time(&self) -> bool {
        false
    }

    /// Whether this rule reads resources through the relation layer.
    ///
    /// The engine enables the run-scoped read/decode cache only when an active
    /// rule opts in, so a configuration without relation rules keeps the
    /// direct per-file read path unchanged. `key_parity` (R3) and the R2
    /// test-only relation consumer override this.
    fn uses_relations(&self) -> bool {
        false
    }

    /// Decorate the configuration-integrity finding produced when
    /// `expect_matches: true` sees an empty effective scope. Most rules leave
    /// it unchanged. A rule whose findings carry required contextual data can
    /// attach that data here so framework-generated and evaluator-generated
    /// findings obey the same reporting contract.
    fn decorate_expect_matches_violation(
        &self,
        _violation: &mut Violation,
        _ctx: &Context<'_>,
    ) -> Result<()> {
        Ok(())
    }

    /// Permit this rule's *read* sites to read a config-declared path
    /// that escapes the repo root. Default no-op (hard confinement).
    /// The loader calls this post-build with the result of the
    /// top-level `allow_out_of_root:` policy for the rule's id/kind;
    /// only the read-confinement rule kinds override it to store the
    /// flag. NEVER reachable from an `extends:`'d ruleset — the policy
    /// is parsed from the user's own top-level config only, mirroring
    /// the `SPAWNING_RULE_KINDS` trust gate. A build site that forgets
    /// to call this leaves the rule confined (the safe default). See
    /// `docs/design/v0.12/allow_out_of_root.md`.
    fn set_allow_out_of_root(&mut self, _allow: bool) {}

    /// Validate this rule's nested sub-rules (the `require:` block of
    /// `for_each_dir` / `for_each_file` / `every_matching_has`) against the
    /// registry at config-load time. Default no-op — most rules have none.
    ///
    /// The cross-file iteration rules store their nested specs and build
    /// them lazily, once per matched iteration, so a nested rule with an
    /// unknown kind, an unknown option, or a missing required field would
    /// otherwise slip past `validate-config` entirely — and past `check`
    /// too whenever the selector matches no entries. Overriders dry-build
    /// each nested spec here (with placeholder path tokens) so those
    /// structural errors surface at load. The loader calls this post-build
    /// at both the `validate-config` and `check` build sites.
    fn validate_nested(&self, _registry: &RuleRegistry) -> Result<()> {
        Ok(())
    }

    /// Nested `require:` specs owned by this rule, if any.
    ///
    /// The engine walks this tree before evaluation so cache-backed scope
    /// predicates (`changed_since` and manifest-derived paths) used by nested
    /// per-file rules are resolved just like their top-level counterparts.
    /// Cross-file selector rules override this alongside [`Self::validate_nested`].
    fn nested_rule_specs(&self) -> &[crate::CompiledNestedSpec] {
        &[]
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

    /// Repository-relative files, outside the rule's own scope, whose change can
    /// alter its verdict (for example `json_schema_passes`'s schema). Under
    /// `--changed`, a change to one of them keeps the rule alive and evaluates it
    /// on the full index, so editing only the schema re-validates every in-scope
    /// document. Default: none.
    fn changed_dependencies(&self) -> Vec<&Path> {
        Vec::new()
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

    /// Whether this rule has a file-like scope for which
    /// [`RuleSpec::expect_matches`](crate::RuleSpec::expect_matches) is
    /// meaningful. The default covers ordinary `paths:`-scoped rules.
    /// Existence rules override this to `false`: an empty match set is the
    /// condition they are specifically designed to test.
    fn supports_expect_matches(&self) -> bool {
        self.path_scope().is_some() || self.as_per_file().is_some()
    }

    /// Advisory hints about this rule's own content pattern: `^` / `$`
    /// anchors that cannot do what the author almost certainly meant
    /// ([`crate::pattern_hints`]). Shown as warnings when a config loads; they
    /// never change a finding. Kinds that match a regex against a whole file
    /// or a block of lines override this.
    fn pattern_hints(&self) -> Vec<crate::pattern_hints::PatternHint> {
        Vec::new()
    }

    /// Return whether at least one indexed target belongs to this rule's
    /// effective scope. Called only when [`Self::supports_expect_matches`] is
    /// true. The default target domain is files; rules over a wider path domain
    /// (currently `no_symlinks`) override it.
    fn scope_matches_any(&self, index: &crate::FileIndex) -> bool {
        let scope = self
            .path_scope()
            .or_else(|| self.as_per_file().map(PerFileRule::path_scope));
        scope.is_some_and(|scope| index.files().any(|entry| scope.matches(&entry.path, index)))
    }

    /// Whether every file selected by this rule's path scope is secret-bearing
    /// even without `sensitive: true`. Structured dotenv/properties rules
    /// override this so editor buffers absent from the last walk still suppress
    /// public replacement bytes.
    fn path_contents_sensitive_by_default(&self) -> bool {
        false
    }

    /// Visit repository files whose bytes must not be serialized through a
    /// staged fix. `explicit` is the rule-level `sensitive: true` policy.
    ///
    /// The default covers ordinary path-scoped rules. A cross-file rule with no
    /// declared scope falls back to every indexed file under an explicit policy;
    /// such rules should override this when they can identify their inputs more
    /// precisely. Rules with implicitly secret-bearing extractors override this
    /// even when `explicit` is false.
    fn visit_sensitive_paths(
        &self,
        explicit: bool,
        ctx: &Context<'_>,
        visit: &mut dyn FnMut(&Path),
    ) {
        let scope = self
            .path_scope()
            .or_else(|| self.as_per_file().map(PerFileRule::path_scope));
        let manifest_sensitive =
            scope.is_some_and(|scope| scope.sensitive_manifest_sources().next().is_some());
        if let Some(scope) = scope {
            for path in scope.sensitive_manifest_sources() {
                visit(path);
            }
        }
        if !explicit && !manifest_sensitive {
            return;
        }
        if let Some(scope) = scope {
            for entry in ctx.index.files() {
                if scope.matches(&entry.path, ctx.index) {
                    visit(&entry.path);
                }
            }
        } else {
            for entry in ctx.index.files() {
                visit(&entry.path);
            }
        }
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

/// Build the configuration-integrity violation for an opted-in rule whose
/// effective scope contains no files. The caller is responsible for applying
/// the rule's `when:` gate first. Keeping this check shared gives top-level and
/// nested rules identical semantics and output.
pub fn expect_matches_violation(
    rule: &dyn Rule,
    spec: &crate::RuleSpec,
    index: &crate::FileIndex,
    ctx: &Context<'_>,
) -> Result<Option<Violation>> {
    if !spec.expect_matches || rule.scope_matches_any(index) {
        return Ok(None);
    }
    let rendered_scope = spec
        .paths
        .as_ref()
        .map_or_else(|| "<all files>".to_string(), crate::PathsSpec::render_scope);
    let qualifier = if spec.scope_filter.is_some() {
        " after applying `scope_filter`"
    } else {
        ""
    };
    let message = spec.message.clone().unwrap_or_else(|| {
        format!(
            "expected rule scope to match at least one file, but `{rendered_scope}` matched none{qualifier}"
        )
    });
    let mut violation = Violation::new(message).with_baseline_key("expect_matches");
    rule.decorate_expect_matches_violation(&mut violation, ctx)?;
    Ok(Some(violation))
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

    /// Trace-mode counterpart to [`Self::evaluate_file`]. The engine calls this
    /// only for selected-rule tracing, so ordinary file-major dispatch has no
    /// trace branch. Implementations override it to emit from values already
    /// computed by production evaluation; the default preserves exact behavior.
    #[doc(hidden)]
    fn evaluate_file_traced(
        &self,
        ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        self.evaluate_file(ctx, path, bytes)
    }

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

/// Rule-major fallback for [`PerFileRule`] implementors.
///
/// Every per-file rule needs a [`Rule::evaluate`] body — the engine's
/// file-major fast path uses [`PerFileRule::evaluate_file`] directly,
/// but `alint fix` (sequential filesystem mutation) and a handful of
/// test harnesses still drive rules through [`Rule::evaluate`]. The
/// loop is mechanical:
///
/// ```text
/// for entry in ctx.index.files() {
///     if scope doesn't match { continue }
///     match read_for_analysis(full) {
///         bytes => violations.extend(self.evaluate_file(ctx, path, &bytes)?),
///         failure => violations.push(typed_resource_read_failure(failure)),
///     }
/// }
/// ```
///
/// Twenty-five rules ship the same loop verbatim. Calling
/// `eval_per_file(self, ctx)` from `Rule::evaluate` collapses each
/// of them to a one-liner. The helper takes `&R: PerFileRule` so
/// it inlines for static dispatch.
///
/// Every in-scope file discovered by the walk must produce bytes or a typed
/// `resource_read` analysis failure. The same fail-loud outcome is used by the
/// engine's file-major dispatch, so `check` and `fix` agree.
pub fn eval_per_file<R: PerFileRule + ?Sized>(
    rule: &R,
    ctx: &Context<'_>,
) -> Result<Vec<Violation>> {
    let mut violations = Vec::new();
    for entry in ctx.index.files() {
        if !rule.path_scope().matches(&entry.path, ctx.index) {
            continue;
        }
        let full = ctx.root.join(&entry.path);
        match crate::walker::read_for_analysis(&full, entry.size) {
            crate::walker::AnalysisRead::Bytes(bytes) => {
                violations.extend(rule.evaluate_file(ctx, &entry.path, &bytes)?);
            }
            crate::walker::AnalysisRead::Missing => {
                violations.push(missing_file_violation(&entry.path));
            }
            crate::walker::AnalysisRead::TooLarge(size) => {
                violations.push(too_large_file_violation(&entry.path, size));
            }
            crate::walker::AnalysisRead::Unreadable(e) => {
                violations.push(unreadable_file_violation(&entry.path, &e));
            }
        }
    }
    Ok(violations)
}

/// The analysis failure for a file that vanished after repository discovery.
#[must_use]
pub fn missing_file_violation(rel: &Path) -> Violation {
    Violation::new("file vanished after repository discovery")
        .with_path(Arc::<Path>::from(rel))
        .as_analysis_failure(AnalysisFailureKind::ResourceRead)
}

/// The analysis failure for a file that exceeds the whole-file analysis cap.
#[must_use]
pub fn too_large_file_violation(rel: &Path, size: u64) -> Violation {
    Violation::new(format!(
        "file is too large to analyze ({size} bytes; {} MiB cap)",
        crate::walker::MAX_ANALYZE_BYTES / (1024 * 1024)
    ))
    .with_path(Arc::<Path>::from(rel))
    .as_analysis_failure(AnalysisFailureKind::ResourceRead)
}

/// The analysis failure for an in-scope file that could not be read.
#[must_use]
pub fn unreadable_file_violation(rel: &Path, err: &std::io::Error) -> Violation {
    Violation::new(format!("could not read file: {err}"))
        .with_path(Arc::<Path>::from(rel))
        .as_analysis_failure(AnalysisFailureKind::ResourceRead)
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
    /// The owning rule's resolved `allow_out_of_root:` permission (top-level
    /// policy only — an `extends:`'d rule is always `false`). Fixers that
    /// resolve a config-declared path (`file_create.path`, `content_from`)
    /// MUST confine it to the repo root unless this is `true`, so an untrusted
    /// ruleset can't make `alint fix` write or read outside the tree.
    pub allow_out_of_root: bool,
    /// Compose buffer for a real `alint fix` pass. When `Some`, a whole-file
    /// write routed through [`FixContext::commit_write`] is captured here,
    /// keyed by repo-relative path, instead of hitting disk, and
    /// [`read_for_fix`] reads it back — so a file touched by several fixers in
    /// config order composes in memory and the engine flushes it with a single
    /// atomic write per file. `None` (the default, and every `--dry-run` run)
    /// writes straight through, unchanged. Interior mutability because fixers
    /// hold `&FixContext`. See auto-fix.md 5.2 and the Phase 0 engine rework.
    pub compose: Option<&'a RefCell<BTreeMap<PathBuf, Vec<u8>>>>,
    /// Sink for whole-file filesystem ops (create / remove / rename) when
    /// *staging* for `alint fix --diff`. A content fixer routes through the
    /// [`compose`](Self::compose) buffer, but a whole-file fixer performs its
    /// effect directly in [`apply`](Fixer::apply) and would otherwise mutate the
    /// tree during a preview. When this is `Some`, such a fixer records its
    /// [`FixEdit`] here and returns *without touching disk*, so the diff can
    /// render the op and `--diff` keeps its no-write contract. `Some` only in a
    /// stage pass; a real `fix` and a `--dry-run` both leave it `None` (a real
    /// fix writes directly; a dry run reports only). Interior mutability for the
    /// same reason as `compose`.
    pub stage_ops: Option<&'a RefCell<Vec<FixEdit>>>,
    /// Complete child-process environment; see [`Context::process_environment`].
    pub process_environment: Option<&'a std::collections::BTreeMap<String, String>>,
}

impl FixContext<'_> {
    /// Persist a whole-file write of `bytes` for the file whose absolute path
    /// is `abs`. In compose mode the bytes are buffered for the engine's single
    /// per-file flush; otherwise they are written atomically now — byte-for-byte
    /// the same as a direct [`write_atomic`]. Content fixers call this instead
    /// of `write_atomic` so the engine can unify the write path.
    ///
    /// The compose buffer is keyed by the *resolved write target*
    /// (`resolve_write_target`): a symlink and its in-tree target coalesce to
    /// one entry, so two fixers touching the same underlying file through
    /// different paths compose (the second reads the first's buffered write)
    /// exactly as they would when writing straight to disk.
    ///
    /// # Errors
    /// Propagates the underlying [`write_atomic`] I/O error in direct mode;
    /// buffering in compose mode is infallible.
    pub fn commit_write(&self, abs: &Path, bytes: &[u8]) -> std::io::Result<()> {
        // A dry run must write NOTHING: every fixer guards `if ctx.dry_run` before
        // reaching here, and the fixpoint's cap-confirmation pass relies on that
        // (it runs `fix_run(dry_run=true)` purely to test convergence). Assert the
        // invariant at the choke point so a fixer that forgets the guard fails
        // loudly in tests/debug rather than silently mutating the tree.
        debug_assert!(
            !self.dry_run,
            "commit_write reached during a dry run; a fixer must check ctx.dry_run before writing"
        );
        // Release-safe backstop (the assert is compiled out in release): a dry run
        // writes NOTHING, so a fixer that forgot its `ctx.dry_run` guard becomes a
        // silent no-op here instead of mutating the tree during, e.g., the
        // fixpoint's cap-confirmation pass.
        if self.dry_run {
            return Ok(());
        }
        match self.compose {
            Some(buf) => {
                buf.borrow_mut()
                    .insert(resolve_write_target(abs), bytes.to_vec());
                Ok(())
            }
            None => write_atomic(abs, bytes),
        }
    }

    /// True when a content fixer earlier in this pass has already composed a
    /// pending whole-file write for `abs` (coalesced by resolved target, so a
    /// symlink and its target share the answer). A whole-file op (remove /
    /// rename) MUST yield when this is true: the pending content write is
    /// flushed *after* the fixer loop, so acting on the pre-edit file now would
    /// let the flush resurrect the removed/renamed path or strand the composed
    /// bytes at a vacated path (a file-duplication corruption). Returns `false`
    /// when there is no compose buffer (a `--dry-run` pass composes nothing).
    pub fn has_pending_write(&self, abs: &Path) -> bool {
        self.compose
            .is_some_and(|buf| buf.borrow().contains_key(&resolve_write_target(abs)))
    }
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

/// A proposed fix expressed as data, so a caller can turn it into an
/// editor edit (an LSP `WorkspaceEdit`) instead of writing the file.
/// Returned by [`Fixer::fix_edit`]. Paths are relative to the alint
/// root, matching [`Violation::path`].
///
/// `fix_edit` is the non-writing sibling of [`Fixer::apply`]: `apply`
/// mutates the filesystem (for `alint fix`); `fix_edit` describes the
/// same change so the editor can apply it to the buffer (with undo).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixEdit {
    /// Replace the full contents of an existing file.
    SetContent { path: PathBuf, content: Vec<u8> },
    /// Create a file that doesn't exist yet.
    CreateFile { path: PathBuf, content: Vec<u8> },
    /// Delete a file.
    DeleteFile { path: PathBuf },
    /// Rename a file (same directory or not).
    RenameFile { from: PathBuf, to: PathBuf },
    /// Replace the half-open byte range `[range.start, range.end)` of an
    /// existing file with `content`. The range is in bytes against the
    /// file's current contents. This is the workhorse of *located* edits
    /// (Phase 1+): several disjoint `ReplaceRange`s against one file can
    /// be batched, ordered, and spliced in a single pass. No Phase-0
    /// fixer emits one, but the primitive splice and the batching engine
    /// are built and tested here.
    ReplaceRange {
        path: PathBuf,
        range: std::ops::Range<usize>,
        content: Vec<u8>,
    },
    /// Set the permission bits of a file (a `chmod`). `mode` is the full
    /// mode word (e.g. `0o755`). Applied only on Unix; on other platforms the
    /// host rule never fires, so this is unreachable. Emitted by the `ChmodFixer`
    /// (the `chmod` fix op on `executable_bit` / `shebang_has_executable`); the
    /// fixer performs the `set_permissions` itself and records this in the stage
    /// sink so `fix --diff` renders the mode change.
    SetMode { path: PathBuf, mode: u32 },
}

/// How safe a fix is to apply automatically. A [`CollectedEdit`] declares
/// its tier; `alint fix` filters against the user's chosen threshold
/// (`Safe` by default, `Unsafe` with `--unsafe-fixes`).
///
/// The `Ord` is the application order: `Safe < Unsafe < Suggestion <
/// Never`, so "would this apply at `threshold`?" is `self <= threshold`
/// for the two *applying* tiers. `Suggestion` is shown but never applied;
/// `Never` is collected for provenance only and neither applied nor
/// suggested. See auto-fix.md 5.5 and ADR-0017.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Applicability {
    /// Behavior-preserving; applied by a bare `alint fix`.
    Safe,
    /// May change semantics; applied only with `--unsafe-fixes`.
    Unsafe,
    /// Never applied automatically; surfaced as [`FixStatus::Suggested`](crate::FixStatus).
    Suggestion,
    /// Collected for analysis/provenance only; never applied, never
    /// suggested for automatic application.
    Never,
}

impl Applicability {
    /// Whether an edit at this tier is *applied* when the user opted into
    /// applying up to `threshold` (`Safe` for a bare `alint fix`, `Unsafe`
    /// for `--unsafe-fixes`). `Suggestion` and `Never` never apply.
    #[must_use]
    pub fn applies_at(self, threshold: Applicability) -> bool {
        matches!(self, Applicability::Safe | Applicability::Unsafe) && self <= threshold
    }

    /// Whether an edit at this tier is *suggested* (shown, not applied)
    /// under `threshold`: an `Unsafe` edit the user did not opt into, or a
    /// `Suggestion`. `Never` is never suggested; an applied tier is not
    /// "merely" suggested.
    #[must_use]
    pub fn suggested_at(self, threshold: Applicability) -> bool {
        match self {
            Applicability::Suggestion => true,
            Applicability::Unsafe => !self.applies_at(threshold),
            Applicability::Safe | Applicability::Never => false,
        }
    }
}

/// The value a structured edit's target query must hold *after* the edit
/// applies — the right-hand side of the localized-equivalence (`PutGet`)
/// check the engine runs before committing a located edit.
#[derive(Debug, Clone, PartialEq)]
pub enum ExpectedValue {
    /// The query must resolve to exactly this scalar.
    Scalar(serde_json::Value),
    /// The query must match nothing (used by `*_path_absent` /
    /// `remove_value`: after a batched multi-node removal, array indices
    /// shift, so only re-running the query and asserting zero matches is
    /// correct).
    Absent,
    /// The query must resolve to a STRING that matches this regex (its source).
    /// Used by `*_path_matches` + `replace`: the fixer rewrites the value so it
    /// satisfies the rule's `matches:` pattern, and this re-checks that goal
    /// (stored as the source string, since `regex::Regex` is not `PartialEq`).
    Matches(String),
}

/// An executable post-edit check the engine can run *without knowing the
/// op* that produced the edit. This is the load-bearing verification
/// obligation the design's translation-validation rests on (R-VERIFY): a
/// fixer returns not just an edit but the means to prove the edit did what
/// the rule wanted, so the engine can demote an edit whose result does not
/// verify to a [`FixStatus::Suggested`](crate::FixStatus) instead of writing bad bytes.
#[derive(Debug, Clone, PartialEq)]
pub enum EditVerifier {
    /// No semantic check (whole-file normalizers): the edit is its own
    /// specification. The engine still confirms the write succeeded.
    None,
    /// Re-parse the post-edit bytes in `format` (syntactic validity) and
    /// re-run the `JSONPath` `query` against them, asserting the result
    /// matches `expect`.
    ///
    /// `query` is the *owned* `JSONPath` source string (the rule's
    /// `path_src`), NOT a borrowed `serde_json_path::NormalizedPath`: a
    /// `NormalizedPath` borrows the parsed `Value` that drops when
    /// `collect_edits` returns (a dangling borrow), and it cannot express
    /// `Absent` after index shifts anyway. Re-running the source query is
    /// the only correct check.
    Structured {
        format: crate::structured_format::Format,
        query: String,
        expect: ExpectedValue,
    },
}

/// A tag marking edits that must not co-apply in a single pass even when
/// their byte ranges are disjoint (e.g. two rewrites of the same logical
/// node reached by different queries). The concrete grouping policy is
/// pinned when the first isolation-needing op ships (Phase 2+); Phase 0
/// carries the field and exercises exclusion with a fixture rule.
pub type GroupId = u32;

/// A single proposed edit plus everything the engine needs to decide
/// whether and how to apply it *without knowing the op that produced it*:
/// its tier, its post-edit verification obligation, and its
/// mutual-exclusion group. Returned by [`Fixer::collect_edits`].
///
/// This refines ADR-0017 decision 1 / auto-fix.md 5.2.1, whose
/// `collect_edits -> Vec<(FixEdit, Applicability)>` cannot carry the
/// verifier the Safe acceptance test needs.
#[derive(Debug, Clone)]
pub struct CollectedEdit {
    pub edit: FixEdit,
    pub applicability: Applicability,
    pub verify: EditVerifier,
    pub isolation_group: Option<GroupId>,
}

/// A mechanical corrector for a specific rule's violations.
pub trait Fixer: Send + Sync + std::fmt::Debug {
    /// Short human-readable summary of what this fixer does,
    /// independent of any specific violation.
    fn describe(&self) -> String;

    /// Apply the fix against a single violation.
    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome>;

    /// Express the fix for `violation` as a [`FixEdit`] without touching
    /// the filesystem, given the current `bytes` of the violation's file
    /// (empty for create-style fixers) and the workspace `root` (so
    /// content sourced from a template can be read). Returns `None` when
    /// the fixer has no editor-expressible form, or when there's nothing
    /// to change.
    ///
    /// Used by the LSP server to offer an "Apply fix" code action as a
    /// `WorkspaceEdit`. The default returns `None` — a fixer opts in by
    /// overriding it. Implementations MUST NOT write to disk (reading a
    /// declared template is allowed). Unlike [`apply`](Self::apply),
    /// this is not `fix_size_limit`-guarded: the caller already holds
    /// the bytes (an open editor buffer), so size is bounded by what the
    /// editor opened.
    fn fix_edit(&self, violation: &Violation, bytes: &[u8], root: &Path) -> Option<FixEdit> {
        let _ = (violation, bytes, root);
        None
    }

    /// Collect the edits this fixer proposes for `violations` against the
    /// current `bytes` of one file, each tagged with its tier, post-edit
    /// verifier, and isolation group. The engine batches, tier-filters,
    /// orders, overlap-skips, verifies, and applies the result.
    ///
    /// The default adapts the whole-file fixers unchanged: it delegates to
    /// [`fix_edit`](Self::fix_edit) per violation and wraps each result as
    /// a `Safe`, `verify: None`, ungrouped [`CollectedEdit`], so an
    /// existing fixer's located-edit form is byte-identical to its
    /// `fix_edit`. A fixer that emits `ReplaceRange`s, needs a tier other
    /// than `Safe`, a semantic verifier, or an isolation group overrides
    /// this. `file` is the edit's path relative to `root` (matching
    /// [`Violation::path`]).
    ///
    /// Note: like [`fix_edit`](Self::fix_edit), this reads nothing from
    /// disk beyond a declared template; the engine enforces the
    /// `fix_size_limit` on `bytes` before calling it.
    fn collect_edits(
        &self,
        violations: &[Violation],
        file: &Path,
        bytes: &[u8],
        root: &Path,
    ) -> Vec<CollectedEdit> {
        let _ = file;
        // Carry the fixer's OWN tier, not a hardcoded `Safe`: the located path is
        // tier-gated on each `CollectedEdit`, so hardcoding `Safe` here would let
        // an Unsafe fixer (e.g. `file_remove`) that opts into the located regime
        // in a future phase have its edits applied by a bare `alint fix`, silently
        // bypassing the tier gate. (Dormant in Phase 0 -- no shipped fixer returns
        // `collects_located_edits() == true` -- but this closes the landmine before
        // the path is ever activated.)
        let tier = self.applicability();
        violations
            .iter()
            .filter_map(|v| self.fix_edit(v, bytes, root))
            .map(|edit| CollectedEdit {
                edit,
                applicability: tier,
                verify: EditVerifier::None,
                isolation_group: None,
            })
            .collect()
    }

    /// Whether this fixer emits *located* edits — byte-range
    /// [`FixEdit::ReplaceRange`]s via [`collect_edits`](Self::collect_edits)
    /// that the engine batches, tier-filters, verifies, and splices through the
    /// located-edit path — rather than writing a whole file through
    /// [`apply`](Self::apply).
    ///
    /// Default `false`: every Phase-0 fixer is a whole-file or path/existence op
    /// and uses `apply`, so the located path is dormant (built and wired, but
    /// never entered). The first `true` arrives with the Phase-1 `replace` op;
    /// the engine then routes that rule's edits through
    /// [`located_fix`](crate::located_fix) instead of `apply`.
    fn collects_located_edits(&self) -> bool {
        false
    }

    /// The [`Applicability`] tier this whole-file / path fixer applies at, gating
    /// it against the user's threshold (`Safe` for a bare `alint fix`, `Unsafe`
    /// for `--unsafe-fixes`). Default `Safe`: the content and path-normalizing
    /// ops are behavior-preserving. `file_remove` overrides this to `Unsafe`
    /// (deleting a whole file irreversibly is a poor default), so a bare
    /// `alint fix` surfaces it as a suggestion rather than deleting; a user may
    /// promote it per-rule (auto-fix.md 5.5). (The located-edit path carries its
    /// tier on each `CollectedEdit` instead; this is the whole-file analogue.)
    fn applicability(&self) -> Applicability {
        Applicability::Safe
    }

    /// Whether this fixer can actually fix *this specific* violation -- a pure,
    /// no-I/O predicate the engine calls at result-assembly time so `check` can
    /// tag fixability per-violation (see [`Violation::is_fixable`]) rather than
    /// per-rule.
    ///
    /// Default `true`: after the Phase-0 detector/fixer domain-alignment work
    /// (rounds 3-6), a content or path/existence fixer fixes every violation its
    /// rule flags. The one exception is `FileRenameFixer` (in `alint-rules`),
    /// which overrides this:
    /// a stem with no reachable target form under the requested case (a non-ASCII
    /// letter under `snake` like `café`, a leading-digit stem under `camel`, a
    /// caseless script under `lower`/`upper`) is flagged by the detector but has
    /// no valid rename, so `fix` honestly skips it -- `check` must not promise it.
    ///
    /// This is a *convertibility* verdict, independent of the [`Applicability`]
    /// tier (an Unsafe `file_remove` still `can_fix`; `--unsafe-fixes` gates
    /// whether it is *applied*) and of fix-time filesystem state (collisions,
    /// pending writes), which only the fix pass can know.
    fn can_fix(&self, violation: &Violation) -> bool {
        let _ = violation;
        true
    }
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
    // In compose mode, an earlier fixer's write for this file lives in the
    // buffer, not on disk; read it back so fixers compose in config order.
    // Keyed by the resolved write target (matching `commit_write`), so a read
    // through a symlink sees a write made through the target and vice versa.
    // The bytes were already size-checked on their first (disk) read, so the
    // cap is not re-applied to in-memory bytes.
    if let Some(buf) = ctx.compose
        && let Some(bytes) = buf.borrow().get(&resolve_write_target(abs))
    {
        return Ok(ReadForFix::Bytes(bytes.clone()));
    }
    // Refuse a non-regular file (FIFO / socket / device) BEFORE the read: a bare
    // `std::fs::read` opens a named pipe `O_RDONLY` and BLOCKS until a writer
    // appears, hanging the whole `fix` run -- including the `--dry-run` / `--diff`
    // previews users run to stay safe. The walker prunes special files at index
    // time, but a fixer can be handed a config-verbatim path that skips the walker
    // (a `sync_from` `targets:` list entry or `source:`), so this direct-read path
    // must guard exactly as `read_capped` / `open_regular` do. A stat error (e.g. a
    // missing file) falls through so the read surfaces the same I/O error callers
    // already handle.
    if std::fs::metadata(abs).is_ok_and(|m| !m.is_file()) {
        return Ok(ReadForFix::Skipped(FixOutcome::Skipped(format!(
            "{} is not a regular file",
            display_path.display()
        ))));
    }
    if let Some(outcome) = check_fix_size(abs, display_path, ctx)? {
        return Ok(ReadForFix::Skipped(outcome));
    }
    let bytes = std::fs::read(abs).map_err(|source| crate::error::Error::Io {
        path: abs.to_path_buf(),
        source,
    })?;
    Ok(ReadForFix::Bytes(bytes))
}

/// The canonical underlying file a write to `path` ultimately modifies, with
/// EVERY symlink in the path resolved — a symlinked file AND a file reached
/// through a symlinked directory both collapse to the same real path. The
/// compose buffer keys on this so two paths that alias one underlying file
/// coalesce to a single entry and compose, exactly as the direct-write path
/// does (a temp+rename through either alias lands on the same real file).
///
/// This is a FULL canonicalization, deliberately stronger than
/// [`write_atomic`]'s own final-component symlink check (which exists only to
/// preserve the final link node during its rename): keying must reflect the
/// real file, not the spelling. Falls back to `path` when it cannot be
/// canonicalized (e.g. a race deleted it), so a key is never lost.
#[must_use]
pub(crate) fn resolve_write_target(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Write `bytes` to `path` atomically: write a uniquely-named sibling temp
/// file, copy the original's permissions onto it (so an existing mode — notably
/// the executable bit — survives), `fsync`, then rename it over `path`. Unlike
/// `std::fs::write` (open-truncate-then-write), a crash or I/O error mid-write
/// leaves the original intact rather than truncated. The temp is a sibling so
/// the rename is atomic on the same filesystem, and it is cleaned up on
/// failure. Writes THROUGH a symlink to its canonical target, preserving the
/// link. (Manual temp, no `tempfile` runtime dependency.) An existing
/// read-only target is refused with `PermissionDenied` rather than replaced
/// (the rename would otherwise succeed on directory permissions alone).
///
/// Lives in `alint-core` so both the fixers (`alint-rules`) and the engine's
/// compose flush share one implementation; `alint-rules::io` re-exports it.
///
/// # Errors
/// Propagates any I/O error from creating, writing, syncing, or renaming.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};
    // Unique sibling name: the pid distinguishes concurrent processes, the
    // atomic counter distinguishes concurrent threads in this process.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    // Write THROUGH a symlink to its (canonical) target, preserving the link.
    // A bare temp+rename on the link path would replace the link NODE with a
    // regular file, silently diverging it from its target (common for a
    // symlinked LICENSE / README in a monorepo). `canonicalize` needs the
    // target to exist, which it does: every caller has just read the file. A
    // broken symlink errors here rather than clobbering the link (unlike the
    // key-only `resolve_write_target`, which falls back so a key is never lost).
    let resolved = match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => std::fs::canonicalize(path)?,
        _ => path.to_path_buf(),
    };
    let path = resolved.as_path();
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map_or_else(|| std::path::PathBuf::from("."), Path::to_path_buf);
    // Refuse to replace a file we could not write in place. A temp+rename only
    // needs write access to the DIRECTORY, so without this check a `chmod 444`
    // file (or a Perforce checkout not yet opened for edit) would be silently
    // rewritten -- unlike a plain in-place write, which fails with EACCES. The
    // caller reports the error (the engine downgrades the item to a fix error),
    // so the violation stands visibly instead.
    if let Ok(meta) = std::fs::metadata(path) {
        refuse_unwritable(path, &meta)?;
    }
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    // A short, fixed-shape name: the target's own name is NOT embedded, so a
    // long basename (> ~230 bytes) cannot push the temp past NAME_MAX (255 on
    // most filesystems; the old `.{name}.alint-fix.{pid}.{n}` failed with
    // ENAMETOOLONG / os error 36). pid + counter keep it unique.
    let tmp = dir.join(format!(".alint-fix.{}.{n}.tmp", std::process::id()));
    let write = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        // Preserve the original file's mode when it exists (a rewrite) -- AFTER
        // the write, not before. A `write()` clears the setuid/setgid bits (the
        // kernel's `should_remove_suid`), so copying the mode before writing
        // would silently drop S_ISUID/S_ISGID from a setgid group helper or the
        // like. Applying the mode after the last write preserves the full mode
        // word (the plain executable bit survived either way, which is why the
        // ordering bug went unnoticed).
        if let Ok(meta) = std::fs::metadata(path) {
            f.set_permissions(meta.permissions())?;
        }
        f.sync_all()
    };
    if let Err(e) = write().and_then(|()| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// `Err(PermissionDenied)` when the existing file at `path` is not writable in
/// place by this process.
///
/// `Permissions::readonly()` alone is not enough: on unix it is true only when
/// EVERY write bit is clear, so a `0464` file (owner may not write, group may)
/// or a `0644` file owned by another user passed. The authoritative check is
/// the kernel's own: open the file for writing (append mode, no truncation, no
/// bytes written; mtime is untouched). `readonly()` is kept as well so a
/// `chmod 444` file stays refused even for root, whose open would succeed.
/// Only a regular file is probed (opening a FIFO for writing could block).
fn refuse_unwritable(path: &Path, meta: &std::fs::Metadata) -> std::io::Result<()> {
    let refuse = |msg: String| {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            msg,
        ))
    };
    if meta.permissions().readonly() {
        return refuse(format!(
            "refusing to overwrite read-only file {}",
            path.display()
        ));
    }
    if meta.is_file() {
        match std::fs::OpenOptions::new().append(true).open(path) {
            Ok(_) => {}
            // Vanished since `metadata`: nothing to protect; the write creates it.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return refuse(format!(
                    "refusing to overwrite {}: not writable in place ({e})",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "rule_tests.rs"]
mod tests;
