//! alint-core — engine, walker, rule trait, config AST.
//!
//! See `docs/design/ARCHITECTURE.md` in the alint repository for the
//! rule model, execution order, and crate layout rationale.

pub mod baseline;
mod category;
mod config;
pub mod did_you_mean;
mod dotenv;
mod engine;
mod error;
mod extract;
pub mod facts;
pub mod git;
mod ini;
pub mod jsonpath_diagnostics;
mod level;
mod pathsafe;
mod registry;
mod report;
mod rule;
mod scope;
mod scope_filter;
mod structured_format;
pub mod template;
mod walker;
pub mod when;
pub mod yaml_depth;

pub use category::Category;
pub use config::{
    AllowOutOfRoot, CompiledNestedSpec, Config, ContentSourceSpec, ExtendsEntry,
    FileAppendFinalNewlineFixSpec, FileAppendFixSpec, FileCollapseBlankLinesFixSpec,
    FileCreateFixSpec, FileNormalizeLineEndingsFixSpec, FilePrependFixSpec, FileRemoveFixSpec,
    FileRenameFixSpec, FileStripBidiFixSpec, FileStripBomFixSpec, FileStripZeroWidthFixSpec,
    FileTrimTrailingWhitespaceFixSpec, FixSpec, NestedRuleSpec, PathsSpec, RuleSpec,
    resolve_content_source,
};
pub use engine::{Engine, RuleEntry};
pub use error::{Error, Result};
pub use extract::{Extract, ExtractSpec, LinesOpts, WholeFileOpts, extract_values, is_non_literal};
pub use facts::{FactKind, FactSpec, FactValue, FactValues, evaluate_facts};
pub use level::Level;
pub use pathsafe::{derive_target, normalize_confined};
pub use registry::{RuleBuilder, RuleRegistry};
pub use report::{FixItem, FixReport, FixRuleResult, FixStatus, Report};
pub use rule::{
    Context, FixContext, FixEdit, FixOutcome, Fixer, GitTrackedMode, PerFileRule, ReadForFix, Rule,
    RuleResult, Violation, check_fix_size, eval_per_file, read_for_fix,
};
pub use scope::Scope;
pub use scope_filter::{
    ManifestDeriveTarget, ManifestPathSpec, ResolvedManifestScope, ScopeFilter, ScopeFilterSpec,
    reject_scope_filter_on_cross_file, reject_scope_filter_with_reason,
};
pub use structured_format::{Format, MAX_XML_DEPTH};
pub use walker::{FileEntry, FileIndex, MAX_ANALYZE_BYTES, WalkOptions, read_capped_or_skip, walk};
pub use when::{WhenEnv, WhenError, WhenExpr};
