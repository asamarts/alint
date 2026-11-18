//! alint-core — engine, walker, rule trait, config AST.
//!
//! See `docs/design/ARCHITECTURE.md` in the alint repository for the
//! rule model, execution order, and crate layout rationale.

// A public type in a private module has no documentation page, so the public-API
// gate (ci/scripts/public-api-parser-gate.sh) would not see what its fields name.
#![deny(unnameable_types)]

pub mod analysis_time;
pub mod baseline;
pub mod bounded_file;
mod category;
mod compatibility;
mod config;
mod config_build;
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
pub mod yaml;
mod yaml_backend;
#[doc(hidden)]
pub mod yaml_budget;
/// The JSON tree [`Format::parse`] gives a YAML document, through ayaml and
/// through libyaml whatever parser the build chose, for the test that measures
/// the heap each takes (`tests/alloc/yaml_memory.rs`, ADR-0042). For alint's
/// own tests, like the feature.
#[cfg(feature = "yaml-reference")]
#[doc(hidden)]
pub mod yaml_reference {
    pub use crate::yaml_backend::{ayaml_json_tree, libyaml_json_tree};
}
mod yaml_decode;
pub mod yaml_depth;
#[cfg(any(test, feature = "yaml-reference"))]
mod yaml_differential;

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
    AllowOutOfRoot, CONFIGURATION_MASK, ChmodFixSpec, CommandFixSpec, CompiledNestedSpec, Config,
    ContentSourceSpec, CreateAndRegisterFixSpec, DirCreateFixSpec, ExtendsEntry,
    FileAppendFinalNewlineFixSpec, FileAppendFixSpec, FileCollapseBlankLinesFixSpec,
    FileCreateFixSpec, FileNormalizeLineEndingsFixSpec, FilePrependFixSpec, FileRemoveFixSpec,
    FileRenameFixSpec, FileStripBidiFixSpec, FileStripBomFixSpec, FileStripZeroWidthFixSpec,
    FileTrimTrailingWhitespaceFixSpec, FixSpec, GitUntrackFixSpec, IndentStyleFixSpec,
    InsertHeaderFixSpec, InsertLineFixSpec, NestedRuleSpec, NumberMask, PathsSpec,
    REDACTED_CONFIGURATION_ERROR, RelocateFixSpec, RemoveValueFixSpec, ReplaceFixSpec, RuleSpec,
    SetValueFixSpec, SortFixSpec, SyncFromFixSpec, configuration_field_may_be_secret,
    mask_configuration_field, mask_configuration_values, redact_environment_values,
    redact_environment_values_in_error, resolve_content_source,
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
pub use pathsafe::{
    CONFIG_DROP_IN_DIRECTORY, CONFIG_FILE_NAMES, NamedRead, POLICY_TEST_DIRECTORY, PathCheckCosts,
    alint_config_refusal, create_landing, derive_target, git_directory_read_refusal,
    git_directory_refusal, names_git_directory, normalize_confined, path_check_costs,
    path_names_alint_config, path_names_git_directory, reaches_alint_config, reaches_git_directory,
    resolve_named_read, resolved_within_root,
};
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
    read_source_for_fix, too_large_file_violation, unreadable_file_violation, write_atomic,
};
pub use scope::{Scope, scope_match_count};
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

/// The types alint-core seals, as a crate that depends on it sees them
/// (ADR-0030 decision 2, ADR-0032 decision 3, ADR-0038 decision 4).
///
/// Each type has two blocks. The first must compile: it uses the type's
/// constructor and a field, or matches every variant with a wildcard arm. The
/// second must fail to compile: it does the same with a functional record
/// update, which a sealed struct refuses, or without the wildcard arm, which a
/// sealed enum needs. With the first block compiling, the seal is the only
/// reason the second can fail, so a second block that compiles means its type
/// lost `#[non_exhaustive]`. `sealed_error_blocks_name_every_variant` keeps the
/// `Error` blocks' variants in step with the enum.
///
/// ```
/// let mut violation = alint_core::Violation::new("m");
/// violation.line = Some(1);
/// ```
///
/// ```compile_fail
/// let _ = alint_core::Violation { line: Some(1), ..alint_core::Violation::new("m") };
/// ```
///
/// ```
/// fn untraced<'a>(ctx: &alint_core::Context<'a>) -> alint_core::Context<'a> {
///     let mut copy = ctx.clone();
///     copy.trace = None;
///     copy
/// }
/// ```
///
/// ```compile_fail
/// fn untraced<'a>(ctx: &alint_core::Context<'a>) -> alint_core::Context<'a> {
///     alint_core::Context { trace: None, ..ctx.clone() }
/// }
/// ```
///
/// ```
/// let mut spec = alint_core::RuleSpec::new("r", "file_exists", alint_core::Level::Error);
/// spec.sensitive = true;
/// ```
///
/// ```compile_fail
/// let _ = alint_core::RuleSpec {
///     sensitive: true,
///     ..alint_core::RuleSpec::new("r", "file_exists", alint_core::Level::Error)
/// };
/// ```
///
/// ```
/// let mut spec = alint_core::NestedRuleSpec::new("file_exists");
/// spec.sensitive = true;
/// ```
///
/// ```compile_fail
/// let _ = alint_core::NestedRuleSpec {
///     sensitive: true,
///     ..alint_core::NestedRuleSpec::new("file_exists")
/// };
/// ```
///
/// ```
/// let mut options: alint_core::WalkOptions = Default::default();
/// options.respect_gitignore = false;
/// ```
///
/// ```compile_fail
/// let _ = alint_core::WalkOptions { respect_gitignore: false, ..Default::default() };
/// ```
///
/// ```
/// let mut context = alint_core::FixContext::new(std::path::Path::new("."), false);
/// context.dry_run = true;
/// ```
///
/// ```compile_fail
/// let _ = alint_core::FixContext {
///     dry_run: true,
///     ..alint_core::FixContext::new(std::path::Path::new("."), false)
/// };
/// ```
///
/// ```
/// let mut config = alint_core::Config::new(1, Vec::new());
/// config.nested_configs = true;
/// ```
///
/// ```compile_fail
/// let _ = alint_core::Config { nested_configs: true, ..alint_core::Config::new(1, Vec::new()) };
/// ```
///
/// ```
/// fn ungated(mut spec: alint_core::CompiledNestedSpec) -> alint_core::CompiledNestedSpec {
///     spec.when = None;
///     spec
/// }
/// ```
///
/// ```compile_fail
/// fn ungated(spec: alint_core::CompiledNestedSpec) -> alint_core::CompiledNestedSpec {
///     alint_core::CompiledNestedSpec { when: None, ..spec }
/// }
/// ```
///
/// ```
/// fn name(error: &alint_core::Error) -> &'static str {
///     match error {
///         alint_core::Error::Io { .. } => "io",
///         alint_core::Error::Walk(_) => "walk",
///         alint_core::Error::Glob { .. } => "glob",
///         alint_core::Error::Yaml(_) => "yaml",
///         alint_core::Error::UnknownRuleKind(_) => "kind",
///         alint_core::Error::RuleConfig { .. } => "rule",
///         alint_core::Error::FileNotInIndex { .. } => "index",
///         alint_core::Error::Other(_) => "other",
///         alint_core::Error::Internal(_) => "internal",
///         _ => "later",
///     }
/// }
/// ```
///
/// ```compile_fail
/// fn name(error: &alint_core::Error) -> &'static str {
///     match error {
///         alint_core::Error::Io { .. } => "io",
///         alint_core::Error::Walk(_) => "walk",
///         alint_core::Error::Glob { .. } => "glob",
///         alint_core::Error::Yaml(_) => "yaml",
///         alint_core::Error::UnknownRuleKind(_) => "kind",
///         alint_core::Error::RuleConfig { .. } => "rule",
///         alint_core::Error::FileNotInIndex { .. } => "index",
///         alint_core::Error::Other(_) => "other",
///         alint_core::Error::Internal(_) => "internal",
///     }
/// }
/// ```
#[cfg(doctest)]
pub struct SealedTypes;

#[cfg(test)]
mod sealed_types_tests {
    /// The variants an `Error` block of [`super::SealedTypes`] names.
    fn named_in(block: &str) -> Vec<&str> {
        block
            .split("alint_core::Error::")
            .skip(1)
            .map(|rest| rest.split(|c: char| !c.is_alphanumeric()).next().unwrap())
            .collect()
    }

    /// Adding a variant to `Error` makes the `compile_fail` block fail for a
    /// missing arm, sealed or not, so both `Error` blocks must name every
    /// variant.
    #[test]
    fn sealed_error_blocks_name_every_variant() {
        let enum_body = include_str!("error.rs")
            .split("pub enum Error {")
            .nth(1)
            .and_then(|rest| rest.split("\n}").next())
            .unwrap();
        let variants: Vec<&str> = enum_body
            .lines()
            .filter_map(|line| line.strip_prefix("    "))
            .filter(|line| line.starts_with(|c: char| c.is_ascii_uppercase()))
            .map(|line| line.split(|c: char| !c.is_alphanumeric()).next().unwrap())
            .collect();
        assert_eq!(variants.len(), 9, "{variants:?}");
        let docs = include_str!("lib.rs")
            .split("pub struct SealedTypes;")
            .next()
            .unwrap();
        // The text between each opening and closing fence.
        let blocks: Vec<&str> = docs
            .split("/// ```")
            .skip(1)
            .step_by(2)
            .filter(|block| block.contains("match error"))
            .collect();
        assert_eq!(blocks.len(), 2);
        for block in blocks {
            assert_eq!(named_in(block), variants, "{block}");
        }
    }
}
