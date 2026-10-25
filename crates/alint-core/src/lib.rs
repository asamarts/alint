//! alint-core — engine, walker, rule trait, config AST.
//!
//! See `docs/design/ARCHITECTURE.md` in the alint repository for the
//! rule model, execution order, and crate layout rationale.

pub mod analysis_time;
pub mod baseline;
mod category;
mod compatibility;
mod config;
pub mod did_you_mean;
mod dotenv;
mod engine;
mod error;
pub mod extract;
pub mod facts;
pub mod git;
mod ini;
pub mod invisible;
pub mod jsonpath_diagnostics;
mod kind_meta;
mod level;
pub mod located_fix;
mod location;
mod pathsafe;
pub mod pattern_hints;
#[doc(hidden)]
pub mod plugin_api;
pub mod process;
pub mod proposed_fix;
mod redaction;
mod registry;
#[doc(hidden)]
pub mod relation;
mod report;
mod rule;
mod scope;
mod scope_filter;
pub mod structured_fix;
mod structured_format;
pub mod template;
#[doc(hidden)]
pub mod trace;
mod walker;
pub mod when;
pub mod yaml_depth;

pub use analysis_time::{
    AnalysisTime, AnalysisTimeResolution, AnalysisTimeSource, AnalysisTimeUnavailable,
    ParseAnalysisTimeError, resolve_analysis_time,
};
pub use category::Category;
pub use compatibility::{
    ConfigCompatibility, ConfigRequirements, ConfigSourceClass, ParseStableVersionError,
    RunningVersion, StableVersion,
};
pub use config::{
    AllowOutOfRoot, CompiledNestedSpec, Config, ContentSourceSpec, ExtendsEntry,
    FileAppendFinalNewlineFixSpec, FileAppendFixSpec, FileCollapseBlankLinesFixSpec,
    FileCreateFixSpec, FileNormalizeLineEndingsFixSpec, FilePrependFixSpec, FileRemoveFixSpec,
    FileRenameFixSpec, FileStripBidiFixSpec, FileStripBomFixSpec, FileStripZeroWidthFixSpec,
    FileTrimTrailingWhitespaceFixSpec, FixSpec, NestedRuleSpec, PathsSpec, RuleSpec,
    resolve_content_source,
};
pub use engine::{Engine, RuleEntry, StagedFix, StagedKind};
pub use error::{Error, Result};
pub use extract::{
    ByteSpan, EntryPathSegment, EntryRowsError, Extract, ExtractSpec, ExtractedValues, LinesOpts,
    NonStringValueKind, StructuredEntryRow, StructuredExtract, WholeFileOpts, extract_entry_rows,
    extract_entry_rows_bounded, extract_entry_rows_strict_yaml,
    extract_entry_rows_strict_yaml_bounded, extract_values, extract_values_bounded, is_non_literal,
};
pub use facts::{FactKind, FactSpec, FactValue, FactValues, evaluate_facts};
pub use kind_meta::{
    Capability, CapabilityCondition, CapabilityMeta, ContentAccess, DependencyMeta, DispatchClass,
    FixOpMeta, FixTrustClass, GitOperation, GitTrackedSupport, KindMeta, OptionPredicate,
    PathsSupport, ScopeFilterSupport, SelectorMeta, SelectorType, SinceSupport,
};
pub use level::Level;
pub use location::{MAX_RELATED_LOCATIONS, RelatedLocation, SourceRange, physical_lines};
pub use pathsafe::{derive_target, normalize_confined, resolved_within_root};
pub use proposed_fix::{EditRegion, ProposedEdit, attach_proposed_edits, byte_to_line_col};
pub use redaction::{redact_evidence, redaction_marker};
pub use registry::{
    ConfiguredField, ConfiguredFieldViolation, ConfiguredFields, ConfiguredScopeFilterField,
    KindCatalog, RuleBuilder, RuleRegistry, validate_configured_fields,
};
pub use report::{
    FIX_ERROR_PREFIX, FixItem, FixReport, FixRuleResult, FixStatus, Report, SkipKind,
};
pub use rule::{
    AnalysisFailure, AnalysisFailureKind, Applicability, CollectedEdit, Context, EditVerifier,
    ExpectedValue, FindingDisposition, FixContext, FixEdit, FixOutcome, Fixer, GitTrackedMode,
    GroupId, NoteKind, PerFileRule, ReadForFix, Rule, RuleResult, Violation, check_fix_size,
    eval_per_file, expect_matches_violation, missing_file_violation, read_for_fix,
    too_large_file_violation, unreadable_file_violation, write_atomic,
};
pub use scope::Scope;
pub use scope_filter::{
    ManifestDeriveTarget, ManifestPathSpec, ResolvedManifestScope, ScopeFilter, ScopeFilterSpec,
    reject_scope_filter_on_cross_file, reject_scope_filter_with_reason,
};
pub use structured_format::MAX_STRUCTURED_BYTES;
pub use structured_format::{Format, MAX_XML_DEPTH};
#[doc(hidden)]
pub use trace::{
    RuleTrace, RuleTraceDocument, TraceAnalysisFailure, TraceCounts, TraceExecution,
    TraceExtractionObservation, TraceMeta, TraceNote, TraceOptions, TracePathOrigin, TracePreview,
    TracePreviewCollector, TracePreviewRow, TraceProvenance, TraceReason, TraceRelatedLocation,
    TraceResult, TraceResultRange, TraceSensitivity, TraceSink, TraceSourceRange, TraceStage,
    TraceStageClass, TraceStatus, TraceValueType, TraceViolation, rule_trace_schema,
};
pub use walker::{
    AnalysisRead, FileEntry, FileIndex, MAX_ANALYZE_BYTES, WalkOptions, read_capped_or_skip,
    read_for_analysis, walk,
};
pub use when::{WhenEnv, WhenError, WhenExpr};
