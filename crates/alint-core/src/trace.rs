//! Typed execution-trace records and the trace-only recorder.
//!
//! The wire format remains experimental. These types are public only so the
//! `alint` binary can project them; they are not part of the stable core API.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{BTreeSet, BinaryHeap};
use std::path::{Component, Path, PathBuf};

use crate::{Error, Result, RuleResult};
use schemars::JsonSchema;
use serde::{Serialize, Serializer};

#[cfg(test)]
thread_local! {
    static RECORDERS_CREATED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// A trace stage class. Common gates and the terminal outcome are engine-owned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TraceStageClass {
    Gate,
    Selection,
    Extraction,
    Cardinality,
    Assertion,
    Nested,
    Outcome,
}

/// The status of one trace stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TraceStatus {
    Satisfied,
    Violated,
    Skipped,
    AnalysisFailure,
    NotEvaluated,
}

impl TraceStatus {
    const fn matches_reason(self, reason: TraceReason) -> bool {
        matches!(
            (self, reason),
            (
                Self::Satisfied,
                TraceReason::LevelEnabled
                    | TraceReason::WhenTrue
                    | TraceReason::ChangedScopeMatched
                    | TraceReason::GitAvailable
                    | TraceReason::AnalysisTimeAvailable
                    | TraceReason::ManifestAvailable
                    | TraceReason::ExpectMatchesSatisfied
                    | TraceReason::SelectorMatched
                    | TraceReason::SelectorNoMatch
                    | TraceReason::ExtractionSucceeded
                    | TraceReason::CardinalitySatisfied
                    | TraceReason::AssertionSatisfied
                    | TraceReason::NestedSatisfied
                    | TraceReason::OutcomeSatisfied
            ) | (
                Self::Violated,
                TraceReason::ExpectMatchesViolated
                    | TraceReason::CardinalityMismatch
                    | TraceReason::AssertionViolated
                    | TraceReason::NestedViolated
                    | TraceReason::OutcomeViolated
            ) | (
                Self::Skipped,
                TraceReason::LevelOff
                    | TraceReason::WhenFalse
                    | TraceReason::ChangedScopeEmpty
                    | TraceReason::OutcomeSkipped
            ) | (
                Self::AnalysisFailure,
                TraceReason::WhenEvaluationFailed
                    | TraceReason::GitUnavailable
                    | TraceReason::AnalysisTimeUnavailable
                    | TraceReason::ManifestUnavailable
                    | TraceReason::ExpectMatchesFailed
                    | TraceReason::ParseFailed
                    | TraceReason::AssertionFailed
                    | TraceReason::NestedFailed
                    | TraceReason::OutcomeAnalysisFailure
            ) | (Self::NotEvaluated, TraceReason::ParseFailed)
        )
    }
}
/// Stable machine-readable reason for a trace stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TraceReason {
    LevelEnabled,
    LevelOff,
    WhenTrue,
    WhenFalse,
    WhenEvaluationFailed,
    ChangedScopeMatched,
    ChangedScopeEmpty,
    GitAvailable,
    GitUnavailable,
    AnalysisTimeAvailable,
    AnalysisTimeUnavailable,
    ManifestAvailable,
    ManifestUnavailable,
    ExpectMatchesSatisfied,
    ExpectMatchesViolated,
    ExpectMatchesFailed,
    SelectorMatched,
    SelectorNoMatch,
    ParseFailed,
    ExtractionSucceeded,
    CardinalitySatisfied,
    CardinalityMismatch,
    AssertionSatisfied,
    AssertionViolated,
    AssertionFailed,
    NestedSatisfied,
    NestedViolated,
    NestedFailed,
    OutcomeSatisfied,
    OutcomeViolated,
    OutcomeSkipped,
    OutcomeAnalysisFailure,
}

impl TraceReason {
    const fn belongs_to(self, class: TraceStageClass) -> bool {
        match class {
            TraceStageClass::Gate => matches!(
                self,
                Self::LevelEnabled
                    | Self::LevelOff
                    | Self::WhenTrue
                    | Self::WhenFalse
                    | Self::WhenEvaluationFailed
                    | Self::ChangedScopeMatched
                    | Self::ChangedScopeEmpty
                    | Self::GitAvailable
                    | Self::GitUnavailable
                    | Self::AnalysisTimeAvailable
                    | Self::AnalysisTimeUnavailable
                    | Self::ManifestAvailable
                    | Self::ManifestUnavailable
                    | Self::ExpectMatchesSatisfied
                    | Self::ExpectMatchesViolated
                    | Self::ExpectMatchesFailed
            ),
            TraceStageClass::Selection => {
                matches!(self, Self::SelectorMatched | Self::SelectorNoMatch)
            }
            TraceStageClass::Extraction => {
                matches!(self, Self::ParseFailed | Self::ExtractionSucceeded)
            }
            TraceStageClass::Cardinality => matches!(
                self,
                Self::ParseFailed | Self::CardinalitySatisfied | Self::CardinalityMismatch
            ),
            TraceStageClass::Assertion => matches!(
                self,
                Self::AssertionSatisfied | Self::AssertionViolated | Self::AssertionFailed
            ),
            TraceStageClass::Nested => matches!(
                self,
                Self::NestedSatisfied | Self::NestedViolated | Self::NestedFailed
            ),
            TraceStageClass::Outcome => matches!(
                self,
                Self::OutcomeSatisfied
                    | Self::OutcomeViolated
                    | Self::OutcomeSkipped
                    | Self::OutcomeAnalysisFailure
            ),
        }
    }
}

/// Checked aggregate counters carried by a stage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraceCounts {
    pub considered: u64,
    pub matched: u64,
    pub inputs: u64,
    pub values: u64,
    pub violations: u64,
    pub notes: u64,
    pub analysis_failures: u64,
    pub children: u64,
    pub satisfied_children: u64,
    pub violated_children: u64,
    pub failed_children: u64,
    pub skipped_children: u64,
}

impl TraceCounts {
    /// Add one counter while treating overflow as an engine invariant failure.
    pub fn checked_increment(counter: &mut u64, label: &str) -> Result<()> {
        *counter = counter
            .checked_add(1)
            .ok_or_else(|| Error::internal(format!("trace counter `{label}` overflowed")))?;
        Ok(())
    }
}

/// One work item's aggregate extraction/cardinality observation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TraceExtractionObservation {
    pub inputs: u64,
    pub values: u64,
    pub parse_failures: u64,
    pub cardinality_checks: u64,
    pub cardinality_mismatches: u64,
}
/// Selected-rule trace controls. Counts-only tracing is the default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TraceOptions {
    preview_limit: Option<usize>,
}

impl TraceOptions {
    #[must_use]
    pub const fn counts_only() -> Self {
        Self {
            preview_limit: None,
        }
    }

    pub fn with_preview(limit: usize) -> Result<Self> {
        if !(1..=100).contains(&limit) {
            return Err(Error::Other(format!(
                "trace preview limit {limit} is outside 1..=100"
            )));
        }
        Ok(Self {
            preview_limit: Some(limit),
        })
    }

    #[must_use]
    pub const fn preview_limit(self) -> Option<usize> {
        self.preview_limit
    }
}

/// Half-open, one-based source coordinates retained by a preview row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraceSourceRange {
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
}

impl TraceSourceRange {
    pub fn new(
        start_line: usize,
        start_column: usize,
        end_line: usize,
        end_column: usize,
    ) -> Result<Self> {
        if start_line == 0
            || start_column == 0
            || end_line == 0
            || end_column == 0
            || (end_line, end_column) < (start_line, start_column)
        {
            return Err(Error::internal("invalid trace preview source range"));
        }
        Ok(Self {
            start_line,
            start_column,
            end_line,
            end_column,
        })
    }

    pub const fn start_line(&self) -> usize {
        self.start_line
    }

    pub const fn start_column(&self) -> usize {
        self.start_column
    }

    pub const fn end_line(&self) -> usize {
        self.end_line
    }

    pub const fn end_column(&self) -> usize {
        self.end_column
    }
}

/// Provenance of one retained trace-evidence value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TraceProvenance {
    Literal,
    Structured,
    Capture,
    Derived,
    Command,
    Unknown,
}

/// Producer-supplied sensitivity. Unknown provenance is always redacted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceSensitivity {
    Public,
    Sensitive,
    Unknown,
}

/// Whether a preview path is repository structure or derived evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TracePathOrigin {
    RepositorySource,
    DerivedPublic,
    DerivedSensitive,
    DerivedUnknown,
}

/// Stable type classification for a preview value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TraceValueType {
    String,
    Number,
    Boolean,
    Null,
    Array,
    Object,
    Bytes,
    Unknown,
}

/// One bounded trace-evidence row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TracePreviewRow {
    #[serde(serialize_with = "serialize_path")]
    #[schemars(with = "String")]
    path: PathBuf,
    range: Option<TraceSourceRange>,
    pointer: Option<String>,
    occurrence: u64,
    provenance: TraceProvenance,
    value_type: TraceValueType,
    display_value: String,
    redacted: bool,
    value_truncated: bool,
}

impl TracePreviewRow {
    const MAX_DISPLAY_BYTES: usize = 256;

    /// Build a row after applying producer-boundary redaction and display bounds.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        path: &Path,
        path_origin: TracePathOrigin,
        range: Option<TraceSourceRange>,
        pointer: Option<String>,
        occurrence: u64,
        provenance: TraceProvenance,
        sensitivity: TraceSensitivity,
        value_type: TraceValueType,
        value: &str,
    ) -> Result<Self> {
        let value_redacted = !matches!(sensitivity, TraceSensitivity::Public)
            || matches!(provenance, TraceProvenance::Unknown);
        let path_redacted = matches!(
            path_origin,
            TracePathOrigin::DerivedSensitive | TracePathOrigin::DerivedUnknown
        );
        let (display_value, value_truncated) = if value_redacted {
            ("<redacted>".to_string(), false)
        } else {
            truncate_utf8(value, Self::MAX_DISPLAY_BYTES)
        };
        Ok(Self {
            path: if path_redacted {
                PathBuf::from("<redacted>")
            } else {
                normalize_repository_path(path)?
            },
            range: if path_redacted { None } else { range },
            pointer: if value_redacted || path_redacted {
                None
            } else {
                pointer
            },
            occurrence,
            provenance,
            value_type,
            display_value,
            redacted: value_redacted || path_redacted,
            value_truncated,
        })
    }

    pub(crate) fn redact_path(&mut self) {
        self.path = PathBuf::from("<redacted>");
        self.range = None;
        self.pointer = None;
        self.redacted = true;
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub const fn range(&self) -> Option<TraceSourceRange> {
        self.range
    }

    pub fn pointer(&self) -> Option<&str> {
        self.pointer.as_deref()
    }

    pub const fn occurrence(&self) -> u64 {
        self.occurrence
    }

    pub const fn provenance(&self) -> TraceProvenance {
        self.provenance
    }

    pub const fn value_type(&self) -> TraceValueType {
        self.value_type
    }

    pub fn display_value(&self) -> &str {
        &self.display_value
    }

    pub const fn redacted(&self) -> bool {
        self.redacted
    }

    pub const fn value_truncated(&self) -> bool {
        self.value_truncated
    }

    fn compare_order(&self, other: &Self) -> Ordering {
        self.path
            .cmp(&other.path)
            .then_with(|| self.range.cmp(&other.range))
            .then_with(|| self.pointer.cmp(&other.pointer))
            .then_with(|| self.occurrence.cmp(&other.occurrence))
            .then_with(|| self.provenance.cmp(&other.provenance))
            .then_with(|| self.value_type.cmp(&other.value_type))
            .then_with(|| self.display_value.cmp(&other.display_value))
            .then_with(|| self.redacted.cmp(&other.redacted))
            .then_with(|| self.value_truncated.cmp(&other.value_truncated))
    }
}

impl Ord for TracePreviewRow {
    fn cmp(&self, other: &Self) -> Ordering {
        self.compare_order(other)
    }
}

impl PartialOrd for TracePreviewRow {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Exact trace-evidence accounting plus the lexicographically first retained rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TracePreview {
    total: u64,
    shown: u64,
    omitted: u64,
    redacted: u64,
    value_truncated: u64,
    rows: Vec<TracePreviewRow>,
}

impl TracePreview {
    pub const fn total(&self) -> u64 {
        self.total
    }

    pub const fn shown(&self) -> u64 {
        self.shown
    }

    pub const fn omitted(&self) -> u64 {
        self.omitted
    }

    pub const fn redacted(&self) -> u64 {
        self.redacted
    }

    pub const fn value_truncated(&self) -> u64 {
        self.value_truncated
    }

    pub fn rows(&self) -> &[TracePreviewRow] {
        &self.rows
    }
}

/// Bounded deterministic evidence collector. Memory is `O(limit)`.
#[derive(Debug)]
pub struct TracePreviewCollector {
    limit: usize,
    total: u64,
    redacted: u64,
    value_truncated: u64,
    rows: BinaryHeap<TracePreviewRow>,
}

impl TracePreviewCollector {
    pub fn new(limit: usize) -> Result<Self> {
        if !(1..=100).contains(&limit) {
            return Err(Error::internal(format!(
                "trace preview limit {limit} is outside 1..=100"
            )));
        }
        Ok(Self {
            limit,
            total: 0,
            redacted: 0,
            value_truncated: 0,
            rows: BinaryHeap::with_capacity(limit),
        })
    }

    pub fn push(&mut self, row: TracePreviewRow) -> Result<()> {
        TraceCounts::checked_increment(&mut self.total, "preview.total")?;
        if row.redacted {
            TraceCounts::checked_increment(&mut self.redacted, "preview.redacted")?;
        }
        if row.value_truncated {
            TraceCounts::checked_increment(&mut self.value_truncated, "preview.value_truncated")?;
        }
        self.push_retained(row);
        Ok(())
    }

    fn push_retained(&mut self, row: TracePreviewRow) {
        if self.rows.len() < self.limit {
            self.rows.push(row);
        } else if self.rows.peek().is_some_and(|largest| row < *largest) {
            let _ = self.rows.pop();
            self.rows.push(row);
        }
    }

    /// Merge one bounded worker-local preview without losing exact counters.
    pub fn absorb(&mut self, preview: TracePreview) -> Result<()> {
        self.total = self
            .total
            .checked_add(preview.total)
            .ok_or_else(|| Error::internal("trace counter `preview.total` overflowed"))?;
        self.redacted = self
            .redacted
            .checked_add(preview.redacted)
            .ok_or_else(|| Error::internal("trace counter `preview.redacted` overflowed"))?;
        self.value_truncated = self
            .value_truncated
            .checked_add(preview.value_truncated)
            .ok_or_else(|| Error::internal("trace counter `preview.value_truncated` overflowed"))?;
        for row in preview.rows {
            self.push_retained(row);
        }
        Ok(())
    }

    pub fn finish(self) -> TracePreview {
        let mut rows = self.rows.into_vec();
        rows.sort();
        let shown = u64::try_from(rows.len()).expect("preview limit fits u64");
        TracePreview {
            total: self.total,
            shown,
            omitted: self.total - shown,
            redacted: self.redacted,
            value_truncated: self.value_truncated,
            rows,
        }
    }
}

/// One typed trace stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraceStage {
    pub ordinal: u32,
    pub class: TraceStageClass,
    name: String,
    pub status: TraceStatus,
    pub reason: TraceReason,
    pub counts: TraceCounts,
    preview: Option<TracePreview>,
}

impl TraceStage {
    pub fn new(
        class: TraceStageClass,
        name: &'static str,
        status: TraceStatus,
        reason: TraceReason,
    ) -> Self {
        Self {
            ordinal: 0,
            class,
            name: name.to_string(),
            status,
            reason,
            counts: TraceCounts::default(),
            preview: None,
        }
    }

    #[must_use]
    pub fn with_counts(mut self, counts: TraceCounts) -> Self {
        self.counts = counts;
        self
    }

    #[must_use]
    pub fn with_preview(mut self, preview: TracePreview) -> Self {
        self.preview = Some(preview);
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn preview(&self) -> Option<&TracePreview> {
        self.preview.as_ref()
    }

    fn validate_shape(&self) -> Result<()> {
        if !self.reason.belongs_to(self.class) {
            return Err(Error::internal(format!(
                "trace reason `{:?}` is invalid for stage class `{:?}`",
                self.reason, self.class
            )));
        }
        if !self.status.matches_reason(self.reason) {
            return Err(Error::internal(format!(
                "trace status `{:?}` is invalid for reason `{:?}`",
                self.status, self.reason
            )));
        }
        if self.preview.is_some()
            && !matches!(
                self.class,
                TraceStageClass::Extraction | TraceStageClass::Nested
            )
        {
            return Err(Error::internal(format!(
                "trace stage `{:?}` cannot carry preview rows",
                self.class
            )));
        }
        Ok(())
    }
}
/// Trace-only producer boundary carried by an evaluation [`crate::Context`].
///
/// Implementations must redact evidence before retaining it. The engine uses a
/// mutex-backed implementation only for selected-rule tracing; ordinary checks
/// carry `None` and never construct or call a sink.
pub trait TraceSink: Sync + std::fmt::Debug {
    fn record_kind(&self, stage: TraceStage);

    fn record_extraction(&self, observation: TraceExtractionObservation);

    fn preview_limit(&self) -> Option<usize>;

    fn preview_enabled(&self) -> bool {
        self.preview_limit().is_some()
    }

    fn evidence_sensitivity(&self, producer_sensitive: bool) -> TraceSensitivity;

    fn paths_are_sensitive(&self) -> bool;

    fn record_preview(&self, row: Result<TracePreviewRow>);
}

/// Static trace inventory attached to every canonical rule kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TraceMeta {
    pub allowed_stages: Cow<'static, [TraceStageClass]>,
    pub required_stages: Cow<'static, [TraceStageClass]>,
    pub emits_preview_rows: bool,
    pub nested_evaluation: bool,
}

const ASSERTION_STAGES: &[TraceStageClass] = &[TraceStageClass::Assertion];
const SELECTION_STAGES: &[TraceStageClass] =
    &[TraceStageClass::Selection, TraceStageClass::Assertion];
const EXTRACTION_STAGES: &[TraceStageClass] = &[
    TraceStageClass::Selection,
    TraceStageClass::Extraction,
    TraceStageClass::Cardinality,
    TraceStageClass::Assertion,
];
const NESTED_STAGES: &[TraceStageClass] = &[
    TraceStageClass::Selection,
    TraceStageClass::Nested,
    TraceStageClass::Assertion,
];

impl TraceMeta {
    pub const ASSERTION: Self = Self {
        allowed_stages: Cow::Borrowed(ASSERTION_STAGES),
        required_stages: Cow::Borrowed(ASSERTION_STAGES),
        emits_preview_rows: false,
        nested_evaluation: false,
    };
    pub const SELECTION: Self = Self {
        allowed_stages: Cow::Borrowed(SELECTION_STAGES),
        required_stages: Cow::Borrowed(SELECTION_STAGES),
        emits_preview_rows: false,
        nested_evaluation: false,
    };
    /// Extraction/cardinality events are option-dependent; selection and the
    /// final policy assertion remain the unconditional required stages.
    pub const EXTRACTION: Self = Self {
        allowed_stages: Cow::Borrowed(EXTRACTION_STAGES),
        required_stages: Cow::Borrowed(SELECTION_STAGES),
        emits_preview_rows: true,
        nested_evaluation: false,
    };
    pub const NESTED: Self = Self {
        allowed_stages: Cow::Borrowed(NESTED_STAGES),
        required_stages: Cow::Borrowed(NESTED_STAGES),
        emits_preview_rows: true,
        nested_evaluation: true,
    };

    /// Validate one canonical kind's trace inventory before evaluation.
    pub fn validate_for_kind(&self, kind: &str) -> Result<()> {
        let mut allowed = BTreeSet::new();
        for class in self.allowed_stages.iter() {
            if matches!(class, TraceStageClass::Gate | TraceStageClass::Outcome) {
                return Err(Error::internal(format!(
                    "rule kind `{kind}` declares engine-owned trace stage `{class:?}`"
                )));
            }
            if !allowed.insert(*class) {
                return Err(Error::internal(format!(
                    "rule kind `{kind}` declares duplicate trace stage `{class:?}`"
                )));
            }
        }
        let mut required = BTreeSet::new();
        for class in self.required_stages.iter() {
            if !required.insert(*class) {
                return Err(Error::internal(format!(
                    "rule kind `{kind}` requires duplicate trace stage `{class:?}`"
                )));
            }
            if !allowed.contains(class) {
                return Err(Error::internal(format!(
                    "rule kind `{kind}` requires undeclared trace stage `{class:?}`"
                )));
            }
        }
        if !required.contains(&TraceStageClass::Assertion) {
            return Err(Error::internal(format!(
                "rule kind `{kind}` must require the policy assertion trace stage"
            )));
        }
        if self.emits_preview_rows
            && !allowed.contains(&TraceStageClass::Extraction)
            && !allowed.contains(&TraceStageClass::Nested)
        {
            return Err(Error::internal(format!(
                "rule kind `{kind}` can emit preview rows without declaring an evidence stage"
            )));
        }
        if self.nested_evaluation != required.contains(&TraceStageClass::Nested) {
            return Err(Error::internal(format!(
                "rule kind `{kind}` nested trace metadata is inconsistent"
            )));
        }
        Ok(())
    }
}

/// Complete traced execution of one effective configured rule.
#[derive(Debug, Clone)]
pub struct RuleTrace {
    pub rule_id: String,
    pub kind: String,
    pub stages: Vec<TraceStage>,
    pub outcome: TraceStatus,
    pub result: Option<RuleResult>,
}
/// Execution controls recorded in the experimental trace envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraceExecution {
    pub changed: bool,
    pub base: Option<String>,
    pub preview: bool,
    pub preview_limit: usize,
}

/// Half-open source coordinates in a trace result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraceResultRange {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_start: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_end: Option<usize>,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

impl From<&crate::SourceRange> for TraceResultRange {
    fn from(range: &crate::SourceRange) -> Self {
        Self {
            byte_start: range.byte_start(),
            byte_end: range.byte_end(),
            start_line: range.start_line(),
            start_column: range.start_column(),
            end_line: range.end_line(),
            end_column: range.end_column(),
        }
    }
}

/// One secondary location in a trace finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraceRelatedLocation {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<TraceResultRange>,
    pub label: String,
}

/// Complete public projection of one violation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraceViolation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<TraceResultRange>,
    pub related_locations: Vec<TraceRelatedLocation>,
    pub related_locations_omitted: usize,
    pub sensitive: bool,
    pub fixable: bool,
}

impl TraceViolation {
    fn from_finding(finding: &crate::Violation, repository_root: &Path) -> Self {
        let path_sensitive = finding.path_is_sensitive();
        Self {
            path: finding.report_path().map(trace_result_path),
            message: trace_result_message(&finding.message, repository_root),
            line: (!path_sensitive).then_some(finding.line).flatten(),
            column: (!path_sensitive).then_some(finding.column).flatten(),
            range: (!path_sensitive)
                .then(|| finding.range.as_ref().map(TraceResultRange::from))
                .flatten(),
            related_locations: if path_sensitive {
                Vec::new()
            } else {
                finding
                    .related_locations
                    .iter()
                    .map(|location| TraceRelatedLocation {
                        path: trace_result_path(&location.path),
                        range: location.range.as_ref().map(TraceResultRange::from),
                        label: trace_result_message(&location.label, repository_root),
                    })
                    .collect()
            },
            related_locations_omitted: if path_sensitive {
                0
            } else {
                finding.related_locations_omitted
            },
            sensitive: finding.is_sensitive(),
            fixable: finding.is_fixable,
        }
    }
}

/// Complete public projection of one non-violation note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraceNote {
    #[serde(flatten)]
    pub finding: TraceViolation,
    pub kind: String,
}

/// Complete public projection of one typed analysis failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraceAnalysisFailure {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    pub sensitive: bool,
}

/// Selected rule's complete public result in the trace envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraceResult {
    pub status: TraceStatus,
    pub violations: Vec<TraceViolation>,
    pub analysis_failures: Vec<TraceAnalysisFailure>,
    pub notes: Vec<TraceNote>,
}

fn trace_rule_payload_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "id": { "type": "string" },
            "rule_kind": { "type": "string" },
            "categories": { "type": "array", "items": { "type": "string" } },
            "summary": { "type": "string" },
            "capabilities": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "capability": { "type": "object" },
                        "condition": { "type": "object" }
                    },
                    "required": ["capability", "condition"]
                }
            },
            "selectors": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "name": { "type": "string" },
                        "type": { "type": "string" },
                        "rerootable": { "type": "boolean" }
                    },
                    "required": ["name", "type", "rerootable"]
                }
            },
            "fix_ops": { "type": "array", "items": { "type": "string" } },
            "docs": { "type": ["string", "null"] },
            "level": { "enum": ["off", "info", "warning", "error"] },
            "paths": {
                "anyOf": [
                    {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "include": { "type": "array", "items": { "type": "string" } },
                            "exclude": { "type": "array", "items": { "type": "string" } }
                        },
                        "required": ["include", "exclude"]
                    },
                    { "type": "null" }
                ]
            },
            "scope_filter": { "type": ["object", "string", "null"] },
            "options": { "type": ["object", "null"] },
            "message": { "type": ["string", "null"] },
            "policy_url": { "type": ["string", "null"] },
            "when": { "type": ["string", "null"] },
            "expect_matches": { "type": "boolean" },
            "conditional": { "type": "boolean" },
            "fixable": { "type": "boolean" },
            "fix": { "type": ["string", "null"] },
            "plan": { "type": ["object", "string"] },
            "analysis_time": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "date": { "type": ["string", "null"] },
                    "source": { "type": "string" },
                    "error": { "type": "string" }
                },
                "required": ["date", "source"]
            }
        },
        "required": [
            "id", "rule_kind", "categories", "summary", "capabilities",
            "selectors", "fix_ops", "docs", "level", "paths", "scope_filter",
            "options", "message", "policy_url", "when", "expect_matches",
            "conditional", "fixable", "fix"
        ]
    })
}

/// Versioned, explicitly experimental JSON document for `explain --trace`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleTraceDocument {
    pub schema_version: u32,
    pub kind: String,
    pub stability: String,
    #[schemars(schema_with = "trace_rule_payload_schema")]
    pub rule: serde_json::Value,
    pub execution: TraceExecution,
    pub stages: Vec<TraceStage>,
    pub result: TraceResult,
}

impl RuleTrace {
    #[must_use]
    pub fn into_document(
        self,
        rule: serde_json::Value,
        execution: TraceExecution,
        repository_root: &Path,
    ) -> RuleTraceDocument {
        let result = self.result.as_ref();
        RuleTraceDocument {
            schema_version: 1,
            kind: "rule-trace".to_string(),
            stability: "experimental".to_string(),
            rule,
            execution,
            stages: self.stages,
            result: TraceResult {
                status: self.outcome,
                violations: result
                    .into_iter()
                    .flat_map(|result| &result.violations)
                    .map(|finding| TraceViolation::from_finding(finding, repository_root))
                    .collect(),
                analysis_failures: result
                    .into_iter()
                    .flat_map(|result| &result.analysis_failures)
                    .map(|failure| TraceAnalysisFailure {
                        kind: failure.kind.as_str().to_string(),
                        path: failure.report_path().map(trace_result_path),
                        message: trace_result_message(&failure.message, repository_root),
                        line: (!failure.path_is_sensitive())
                            .then_some(failure.line)
                            .flatten(),
                        column: (!failure.path_is_sensitive())
                            .then_some(failure.column)
                            .flatten(),
                        sensitive: failure.is_sensitive(),
                    })
                    .collect(),
                notes: result
                    .into_iter()
                    .flat_map(|result| &result.notes)
                    .map(|note| TraceNote {
                        finding: TraceViolation::from_finding(note, repository_root),
                        kind: note
                            .note_kind()
                            .unwrap_or(crate::NoteKind::Informational)
                            .as_str()
                            .to_string(),
                    })
                    .collect(),
            },
        }
    }
}

/// Canonical generated JSON Schema for the experimental trace document.
#[must_use]
pub fn rule_trace_schema() -> serde_json::Value {
    let mut schema = serde_json::to_value(schemars::schema_for!(RuleTraceDocument))
        .expect("trace schema serializes");
    let object = schema
        .as_object_mut()
        .expect("derived trace schema is an object");
    object.insert(
        "$id".to_string(),
        "https://raw.githubusercontent.com/asamarts/alint/main/schemas/experimental/rule-trace.json"
            .into(),
    );
    object.insert("title".to_string(), "alint experimental rule trace".into());
    if let Some(properties) = object
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
    {
        for (field, value) in [
            ("schema_version", serde_json::json!(1)),
            ("kind", serde_json::json!("rule-trace")),
            ("stability", serde_json::json!("experimental")),
        ] {
            if let Some(property) = properties
                .get_mut(field)
                .and_then(serde_json::Value::as_object_mut)
            {
                property.insert("const".to_string(), value);
            }
        }
    }
    schema
}

fn trace_result_message(message: &str, repository_root: &Path) -> String {
    let mut replacements = Vec::new();
    for (path, marker) in [
        (std::path::absolute(repository_root).ok(), "<repository>"),
        (std::fs::canonicalize(repository_root).ok(), "<repository>"),
        (Some(std::env::temp_dir()), "<temporary-path>"),
        (
            std::fs::canonicalize(std::env::temp_dir()).ok(),
            "<temporary-path>",
        ),
    ] {
        let Some(path) = path else { continue };
        let native = path.to_string_lossy().into_owned();
        if native.len() > 1 {
            replacements.push((native.clone(), marker));
            let slash = native.replace('\\', "/");
            if slash != native {
                replacements.push((slash, marker));
            }
        }
    }
    replacements.sort_unstable_by_key(|(path, _)| std::cmp::Reverse(path.len()));
    replacements.dedup_by(|left, right| left.0 == right.0);
    let mut sanitized = message.to_string();
    for (path, marker) in replacements {
        sanitized = sanitized.replace(&path, marker);
    }
    sanitized
}

fn trace_result_path(path: &Path) -> String {
    let rendered = path.to_string_lossy().replace('\\', "/");
    let windows_absolute = rendered.as_bytes().get(1) == Some(&b':')
        && rendered
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic);
    if rendered.starts_with('/')
        || path.is_absolute()
        || windows_absolute
        || rendered.split('/').any(|component| component == "..")
    {
        "<outside-repository>".to_string()
    } else {
        rendered
            .split('/')
            .filter(|component| !component.is_empty() && *component != ".")
            .collect::<Vec<_>>()
            .join("/")
    }
}

/// Trace-only stage recorder. Ordinary checks never construct one.
#[derive(Debug)]
pub(crate) struct TraceRecorder {
    meta: TraceMeta,
    stages: Vec<TraceStage>,
    next_ordinal: u32,
}

impl TraceRecorder {
    pub(crate) fn new(meta: TraceMeta) -> Self {
        #[cfg(test)]
        RECORDERS_CREATED.with(|count| count.set(count.get() + 1));
        Self {
            meta,
            stages: Vec::new(),
            next_ordinal: 0,
        }
    }

    pub(crate) fn record(&mut self, mut stage: TraceStage) -> Result<()> {
        stage.validate_shape()?;
        stage.ordinal = self.next_ordinal;
        self.next_ordinal = self
            .next_ordinal
            .checked_add(1)
            .ok_or_else(|| Error::internal("trace stage ordinal overflowed"))?;
        self.stages.push(stage);
        Ok(())
    }

    fn validate_completion(&self, kind: &str, result: Option<&RuleResult>) -> Result<bool> {
        self.meta.validate_for_kind(kind)?;
        if self
            .stages
            .iter()
            .any(|stage| stage.class == TraceStageClass::Outcome)
        {
            return Err(Error::internal(format!(
                "rule kind `{kind}` emitted the engine-owned terminal trace stage"
            )));
        }
        if self
            .stages
            .iter()
            .any(|stage| stage.preview.is_some() && !self.meta.emits_preview_rows)
        {
            return Err(Error::internal(format!(
                "rule kind `{kind}` emitted preview rows without declaring preview capability"
            )));
        }
        let assertion_reached = self
            .stages
            .iter()
            .any(|stage| stage.class == TraceStageClass::Assertion);
        let observed: BTreeSet<TraceStageClass> = self
            .stages
            .iter()
            .filter_map(|stage| {
                (!matches!(
                    stage.class,
                    TraceStageClass::Gate | TraceStageClass::Outcome
                ))
                .then_some(stage.class)
            })
            .collect();
        if let Some(stage) = observed
            .iter()
            .find(|stage| !self.meta.allowed_stages.contains(stage))
        {
            return Err(Error::internal(format!(
                "rule kind `{kind}` emitted undeclared trace stage `{stage:?}`"
            )));
        }
        let terminal_gate_result = self.stages.iter().any(|stage| {
            stage.class == TraceStageClass::Gate
                && matches!(
                    stage.status,
                    TraceStatus::Violated | TraceStatus::AnalysisFailure
                )
        });
        if assertion_reached || (result.is_some() && !terminal_gate_result) {
            for required in self.meta.required_stages.iter() {
                if !observed.contains(required) {
                    return Err(Error::internal(format!(
                        "rule kind `{kind}` completed evaluation without required trace stage \
                         `{required:?}`"
                    )));
                }
            }
        }
        let stage_analysis_failure = self
            .stages
            .iter()
            .any(|stage| stage.status == TraceStatus::AnalysisFailure);
        let result_analysis_failure =
            result.is_some_and(|result| !result.analysis_failures.is_empty());
        if result.is_some() && stage_analysis_failure != result_analysis_failure {
            return Err(Error::internal(format!(
                "rule kind `{kind}` trace/result analysis-failure status is inconsistent"
            )));
        }
        let stage_violated = self
            .stages
            .iter()
            .any(|stage| stage.status == TraceStatus::Violated);
        if let Some(result) = result
            && !result_analysis_failure
            && stage_violated == result.violations.is_empty()
        {
            return Err(Error::internal(format!(
                "rule kind `{kind}` trace/result violation status is inconsistent"
            )));
        }
        Ok(stage_analysis_failure)
    }

    pub(crate) fn finish(
        mut self,
        rule_id: String,
        kind: String,
        result: Option<RuleResult>,
    ) -> Result<RuleTrace> {
        let stage_analysis_failure = self.validate_completion(&kind, result.as_ref())?;
        let result_analysis_failure = result
            .as_ref()
            .is_some_and(|result| !result.analysis_failures.is_empty());
        let outcome = match result.as_ref() {
            Some(_) if result_analysis_failure => TraceStatus::AnalysisFailure,
            Some(result) if !result.violations.is_empty() => TraceStatus::Violated,
            Some(_) => TraceStatus::Satisfied,
            None if stage_analysis_failure => TraceStatus::AnalysisFailure,
            None if self
                .stages
                .iter()
                .any(|stage| stage.status == TraceStatus::Skipped) =>
            {
                TraceStatus::Skipped
            }
            None => {
                return Err(Error::internal(format!(
                    "rule kind `{kind}` produced neither a result nor a represented skip"
                )));
            }
        };
        let reason = match outcome {
            TraceStatus::Satisfied => TraceReason::OutcomeSatisfied,
            TraceStatus::Violated => TraceReason::OutcomeViolated,
            TraceStatus::Skipped => TraceReason::OutcomeSkipped,
            TraceStatus::AnalysisFailure => TraceReason::OutcomeAnalysisFailure,
            TraceStatus::NotEvaluated => {
                return Err(Error::internal(
                    "terminal trace outcome cannot be not_evaluated",
                ));
            }
        };
        let counts = result
            .as_ref()
            .map_or_else(TraceCounts::default, |result| TraceCounts {
                violations: u64::try_from(result.violations.len()).expect("length fits u64"),
                notes: u64::try_from(result.notes.len()).expect("length fits u64"),
                analysis_failures: u64::try_from(result.analysis_failures.len())
                    .expect("length fits u64"),
                ..TraceCounts::default()
            });
        self.record(
            TraceStage::new(TraceStageClass::Outcome, "outcome", outcome, reason)
                .with_counts(counts),
        )?;
        Ok(RuleTrace {
            rule_id,
            kind,
            stages: self.stages,
            outcome,
            result,
        })
    }
}

#[cfg(test)]
pub(crate) fn recorders_created_on_current_thread() -> u64 {
    RECORDERS_CREATED.with(std::cell::Cell::get)
}

fn serialize_path<S>(path: &Path, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&path.to_string_lossy())
}

fn normalize_repository_path(path: &Path) -> Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(Error::internal(format!(
                    "trace preview path `{}` is not repository-relative",
                    path.display()
                )));
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(Error::internal(
            "trace preview path must identify a repository entry",
        ));
    }
    #[cfg(windows)]
    {
        return Ok(PathBuf::from(
            normalized.to_string_lossy().replace('\\', "/"),
        ));
    }
    #[cfg(not(windows))]
    {
        Ok(normalized)
    }
}

fn truncate_utf8(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.to_string(), false);
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(path: &str, occurrence: u64) -> TracePreviewRow {
        TracePreviewRow::new(
            Path::new(path),
            TracePathOrigin::RepositorySource,
            None,
            None,
            occurrence,
            TraceProvenance::Structured,
            TraceSensitivity::Public,
            TraceValueType::String,
            path,
        )
        .unwrap()
    }

    #[test]
    fn result_paths_never_serialize_absolute_or_parent_locations() {
        assert_eq!(
            trace_result_path(Path::new("/tmp/secret-canary")),
            "<outside-repository>"
        );
        assert_eq!(
            trace_result_path(Path::new(r"C:\checkout\secret-canary")),
            "<outside-repository>"
        );
        assert_eq!(
            trace_result_path(Path::new("src/../secret-canary")),
            "<outside-repository>"
        );
        assert_eq!(trace_result_path(Path::new("./src/lib.rs")), "src/lib.rs");
    }

    #[test]
    fn result_messages_hide_checkout_and_temporary_paths() {
        let checkout = tempfile::tempdir().unwrap();
        let temporary = std::env::temp_dir().join("alint-trace-canary");
        let message = format!(
            "failed under {} while staging {}",
            checkout.path().display(),
            temporary.display()
        );
        let sanitized = trace_result_message(&message, checkout.path());
        assert!(!sanitized.contains(&checkout.path().display().to_string()));
        assert!(!sanitized.contains(&temporary.display().to_string()));
        assert!(sanitized.contains("<repository>"), "{sanitized}");
        assert!(sanitized.contains("<temporary-path>"), "{sanitized}");
    }

    #[test]
    fn checked_trace_counters_fail_on_overflow() {
        let mut counter = u64::MAX;
        let error = TraceCounts::checked_increment(&mut counter, "test").unwrap_err();
        assert!(error.is_internal());
        assert_eq!(counter, u64::MAX);
    }

    #[test]
    fn preview_retains_lexicographically_first_rows_with_exact_counts() {
        let mut collector = TracePreviewCollector::new(2).unwrap();
        collector.push(row("z", 0)).unwrap();
        collector.push(row("a", 0)).unwrap();
        collector.push(row("m", 0)).unwrap();
        let preview = collector.finish();
        assert_eq!(preview.total, 3);
        assert_eq!(preview.shown, 2);
        assert_eq!(preview.omitted, 1);
        assert_eq!(preview.rows[0].path, Path::new("a"));
        assert_eq!(preview.rows[1].path, Path::new("m"));
    }

    #[test]
    fn preview_order_is_deterministic_when_structural_keys_collide() {
        let make_row = |value: &str| {
            TracePreviewRow::new(
                Path::new("same"),
                TracePathOrigin::RepositorySource,
                None,
                Some("/same".to_string()),
                0,
                TraceProvenance::Structured,
                TraceSensitivity::Public,
                TraceValueType::String,
                value,
            )
            .unwrap()
        };
        for values in [["z", "a"], ["a", "z"]] {
            let mut collector = TracePreviewCollector::new(1).unwrap();
            for value in values {
                collector.push(make_row(value)).unwrap();
            }
            assert_eq!(collector.finish().rows()[0].display_value(), "a");
        }
    }

    #[test]
    fn preview_merge_preserves_global_order_and_exact_accounting() {
        let mut left = TracePreviewCollector::new(2).unwrap();
        left.push(row("z", 0)).unwrap();
        left.push(row("b", 0)).unwrap();
        let mut right = TracePreviewCollector::new(2).unwrap();
        right.push(row("a", 0)).unwrap();
        right.push(row("m", 0)).unwrap();

        let mut merged = TracePreviewCollector::new(2).unwrap();
        merged.absorb(left.finish()).unwrap();
        merged.absorb(right.finish()).unwrap();
        let preview = merged.finish();

        assert_eq!(
            (preview.total(), preview.shown(), preview.omitted()),
            (4, 2, 2)
        );
        assert_eq!(preview.rows()[0].path(), Path::new("a"));
        assert_eq!(preview.rows()[1].path(), Path::new("b"));
    }

    #[test]
    fn preview_redacts_unknown_and_sensitive_values_before_storage() {
        for (provenance, sensitivity) in [
            (TraceProvenance::Unknown, TraceSensitivity::Public),
            (TraceProvenance::Command, TraceSensitivity::Sensitive),
            (TraceProvenance::Structured, TraceSensitivity::Unknown),
        ] {
            let row = TracePreviewRow::new(
                Path::new("a"),
                TracePathOrigin::RepositorySource,
                None,
                None,
                0,
                provenance,
                sensitivity,
                TraceValueType::String,
                "secret-canary",
            )
            .unwrap();
            assert!(row.redacted());
            assert_eq!(row.display_value(), "<redacted>");
        }
    }

    #[test]
    fn preview_redacts_sensitive_derived_paths_and_structural_pointers() {
        let row = TracePreviewRow::new(
            Path::new("secret-canary/result.txt"),
            TracePathOrigin::DerivedSensitive,
            None,
            Some("/secret-canary".to_string()),
            0,
            TraceProvenance::Derived,
            TraceSensitivity::Sensitive,
            TraceValueType::String,
            "secret-canary",
        )
        .unwrap();
        assert_eq!(row.path(), Path::new("<redacted>"));
        assert!(row.pointer().is_none());
        assert_eq!(row.display_value(), "<redacted>");
        assert!(row.redacted());

        let path_only = TracePreviewRow::new(
            Path::new("secret-canary/result.txt"),
            TracePathOrigin::DerivedSensitive,
            Some(TraceSourceRange {
                start_line: 1,
                start_column: 2,
                end_line: 3,
                end_column: 4,
            }),
            Some("/secret-canary".to_string()),
            0,
            TraceProvenance::Derived,
            TraceSensitivity::Public,
            TraceValueType::String,
            "public",
        )
        .unwrap();
        assert_eq!(path_only.path(), Path::new("<redacted>"));
        assert!(path_only.range().is_none());
        assert!(path_only.pointer().is_none());
        assert_eq!(path_only.display_value(), "public");
        assert!(path_only.redacted());
    }

    #[test]
    fn preview_limit_is_bounded_and_accounting_includes_omitted_rows() {
        assert!(TracePreviewCollector::new(0).is_err());
        assert!(TracePreviewCollector::new(101).is_err());

        for limit in [1, 20, 100] {
            let mut collector = TracePreviewCollector::new(limit).unwrap();
            for index in (0..101).rev() {
                collector.push(row(&format!("{index:03}"), 0)).unwrap();
            }
            let preview = collector.finish();
            assert_eq!(
                (preview.total(), preview.shown(), preview.omitted()),
                (101, limit as u64, 101 - limit as u64)
            );
            assert_eq!(preview.rows()[0].path(), Path::new("000"));
            assert_eq!(
                preview.rows().last().unwrap().path(),
                Path::new(&format!("{:03}", limit - 1))
            );
        }
    }

    #[test]
    fn preview_truncates_at_a_utf8_boundary() {
        let value = format!("{}é", "a".repeat(255));
        let row = TracePreviewRow::new(
            Path::new("a"),
            TracePathOrigin::RepositorySource,
            None,
            None,
            0,
            TraceProvenance::Literal,
            TraceSensitivity::Public,
            TraceValueType::String,
            &value,
        )
        .unwrap();
        assert_eq!(row.display_value().len(), 255);
        assert!(row.value_truncated());
    }

    #[test]
    fn preview_rejects_absolute_and_parent_paths_before_storage() {
        for path in [Path::new("/outside"), Path::new("../outside")] {
            let error = TracePreviewRow::new(
                path,
                TracePathOrigin::RepositorySource,
                None,
                None,
                0,
                TraceProvenance::Structured,
                TraceSensitivity::Public,
                TraceValueType::String,
                "public",
            )
            .unwrap_err();
            assert!(error.to_string().contains("repository-relative"));
        }
    }

    #[test]
    fn preview_normalizes_dot_components_and_validates_source_ranges() {
        let range = TraceSourceRange::new(1, 2, 1, 3).unwrap();
        let row = TracePreviewRow::new(
            Path::new("./dir/./file"),
            TracePathOrigin::RepositorySource,
            Some(range),
            None,
            0,
            TraceProvenance::Structured,
            TraceSensitivity::Public,
            TraceValueType::String,
            "public",
        )
        .unwrap();
        assert_eq!(row.path(), Path::new("dir/file"));
        assert_eq!(row.range(), Some(range));
        assert!(TraceSourceRange::new(0, 1, 1, 1).is_err());
        assert!(TraceSourceRange::new(2, 1, 1, 1).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn preview_json_handles_non_utf8_repository_paths() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt as _;

        let row = TracePreviewRow::new(
            Path::new(&OsString::from_vec(b"dir/\xff.txt".to_vec())),
            TracePathOrigin::RepositorySource,
            None,
            None,
            0,
            TraceProvenance::Structured,
            TraceSensitivity::Public,
            TraceValueType::String,
            "public",
        )
        .unwrap();
        let json = serde_json::to_string(&row).unwrap();
        assert!(json.contains("dir/"));
        assert!(json.contains('\u{fffd}'));
    }

    #[test]
    fn recorder_rejects_malformed_stage_shape_and_preview_capability() {
        let mut malformed = TraceRecorder::new(TraceMeta::ASSERTION);
        assert!(
            malformed
                .record(TraceStage::new(
                    TraceStageClass::Assertion,
                    "policy",
                    TraceStatus::Satisfied,
                    TraceReason::SelectorMatched,
                ))
                .unwrap_err()
                .to_string()
                .contains("invalid for stage class")
        );

        let mut collector = TracePreviewCollector::new(1).unwrap();
        collector.push(row("a", 0)).unwrap();
        let mut unexpected_preview = TraceRecorder::new(TraceMeta::ASSERTION);
        unexpected_preview
            .record(
                TraceStage::new(
                    TraceStageClass::Extraction,
                    "extract",
                    TraceStatus::Satisfied,
                    TraceReason::ExtractionSucceeded,
                )
                .with_preview(collector.finish()),
            )
            .unwrap();
        assert!(
            unexpected_preview
                .finish("id".into(), "kind".into(), None)
                .unwrap_err()
                .to_string()
                .contains("without declaring preview capability")
        );
    }

    #[test]
    fn recorder_rejects_unrepresented_analysis_failure() {
        let mut recorder = TraceRecorder::new(TraceMeta::ASSERTION);
        recorder
            .record(TraceStage::new(
                TraceStageClass::Gate,
                "analysis_time",
                TraceStatus::AnalysisFailure,
                TraceReason::AnalysisTimeUnavailable,
            ))
            .unwrap();
        recorder
            .record(TraceStage::new(
                TraceStageClass::Assertion,
                "policy",
                TraceStatus::Satisfied,
                TraceReason::AssertionSatisfied,
            ))
            .unwrap();
        assert!(
            recorder
                .finish("id".into(), "kind".into(), Some(passing_result()))
                .unwrap_err()
                .to_string()
                .contains("analysis-failure status is inconsistent")
        );
    }

    #[test]
    fn recorder_rejects_unrepresented_policy_violation() {
        let mut recorder = TraceRecorder::new(TraceMeta::ASSERTION);
        recorder
            .record(TraceStage::new(
                TraceStageClass::Assertion,
                "policy",
                TraceStatus::Violated,
                TraceReason::AssertionViolated,
            ))
            .unwrap();
        assert!(
            recorder
                .finish("id".into(), "kind".into(), Some(passing_result()))
                .unwrap_err()
                .to_string()
                .contains("violation status is inconsistent")
        );
    }

    #[test]
    fn successful_result_requires_assertion_stage() {
        let recorder = TraceRecorder::new(TraceMeta::ASSERTION);
        assert!(
            recorder
                .finish("id".into(), "kind".into(), Some(passing_result()))
                .unwrap_err()
                .to_string()
                .contains("required trace stage")
        );
    }

    #[test]
    fn terminal_gate_result_does_not_require_evaluation_stages() {
        let mut recorder = TraceRecorder::new(TraceMeta::SELECTION);
        recorder
            .record(TraceStage::new(
                TraceStageClass::Gate,
                "expect_matches",
                TraceStatus::Violated,
                TraceReason::ExpectMatchesViolated,
            ))
            .unwrap();
        let result = RuleResult::new(
            "id".into(),
            crate::Level::Error,
            None,
            vec![crate::Violation::new("scope assertion failed")],
            false,
        );
        let trace = recorder
            .finish("id".into(), "kind".into(), Some(result))
            .unwrap();
        assert_eq!(trace.outcome, TraceStatus::Violated);
    }

    #[test]
    fn failed_assertion_still_enforces_required_stage_inventory() {
        let mut recorder = TraceRecorder::new(TraceMeta::SELECTION);
        recorder
            .record(TraceStage::new(
                TraceStageClass::Assertion,
                "policy",
                TraceStatus::AnalysisFailure,
                TraceReason::AssertionFailed,
            ))
            .unwrap();
        assert!(
            recorder
                .finish("id".into(), "kind".into(), Some(passing_result()))
                .unwrap_err()
                .to_string()
                .contains("required trace stage")
        );
    }

    #[test]
    fn recorder_rejects_missing_and_undeclared_kind_stages() {
        let mut missing = TraceRecorder::new(TraceMeta::SELECTION);
        missing
            .record(TraceStage::new(
                TraceStageClass::Assertion,
                "policy",
                TraceStatus::Satisfied,
                TraceReason::AssertionSatisfied,
            ))
            .unwrap();
        assert!(
            missing
                .finish("id".into(), "kind".into(), Some(passing_result()))
                .unwrap_err()
                .to_string()
                .contains("required trace stage")
        );

        let mut undeclared = TraceRecorder::new(TraceMeta::ASSERTION);
        undeclared
            .record(TraceStage::new(
                TraceStageClass::Selection,
                "selector",
                TraceStatus::Satisfied,
                TraceReason::SelectorMatched,
            ))
            .unwrap();
        assert!(
            undeclared
                .finish("id".into(), "kind".into(), None)
                .unwrap_err()
                .to_string()
                .contains("undeclared trace stage")
        );
    }

    #[test]
    fn recorder_rejects_producer_owned_terminal_and_invalid_inventory() {
        let mut terminal = TraceRecorder::new(TraceMeta::ASSERTION);
        terminal
            .record(TraceStage::new(
                TraceStageClass::Outcome,
                "outcome",
                TraceStatus::Satisfied,
                TraceReason::OutcomeSatisfied,
            ))
            .unwrap();
        assert!(
            terminal
                .finish("id".into(), "kind".into(), None)
                .unwrap_err()
                .to_string()
                .contains("engine-owned terminal")
        );

        let invalid = TraceMeta {
            allowed_stages: Cow::Borrowed(&[TraceStageClass::Assertion]),
            required_stages: Cow::Borrowed(&[TraceStageClass::Extraction]),
            emits_preview_rows: false,
            nested_evaluation: false,
        };
        assert!(
            invalid
                .validate_for_kind("broken")
                .unwrap_err()
                .to_string()
                .contains("requires undeclared")
        );
    }

    #[test]
    fn runtime_owned_trace_metadata_remains_supported() {
        let metadata = TraceMeta {
            allowed_stages: Cow::Owned(vec![
                TraceStageClass::Selection,
                TraceStageClass::Assertion,
            ]),
            required_stages: Cow::Owned(vec![TraceStageClass::Assertion]),
            emits_preview_rows: false,
            nested_evaluation: false,
        };
        metadata.validate_for_kind("runtime-plugin").unwrap();
    }

    #[test]
    fn common_gate_analysis_failure_is_not_misreported_as_skip() {
        let mut recorder = TraceRecorder::new(TraceMeta::ASSERTION);
        recorder
            .record(TraceStage::new(
                TraceStageClass::Gate,
                "when",
                TraceStatus::AnalysisFailure,
                TraceReason::WhenEvaluationFailed,
            ))
            .unwrap();
        let trace = recorder.finish("id".into(), "kind".into(), None).unwrap();
        assert_eq!(trace.outcome, TraceStatus::AnalysisFailure);
        assert_eq!(
            trace.stages.last().map(|stage| stage.reason),
            Some(TraceReason::OutcomeAnalysisFailure)
        );
    }

    fn passing_result() -> RuleResult {
        RuleResult::new("id".into(), crate::Level::Error, None, Vec::new(), false)
    }
}
