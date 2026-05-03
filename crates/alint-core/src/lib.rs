//! alint-core — engine, walker, rule trait, config AST.
//!
//! See `docs/design/ARCHITECTURE.md` in the alint repository for the
//! rule model, execution order, and crate layout rationale.

mod config;
mod engine;
mod error;
pub mod facts;
pub mod git;
mod level;
mod registry;
mod report;
mod rule;
mod scope;
mod scope_filter;
pub mod template;
mod walker;
pub mod when;

pub use config::{
    Config, ContentSourceSpec, ExtendsEntry, FileAppendFinalNewlineFixSpec, FileAppendFixSpec,
    FileCollapseBlankLinesFixSpec, FileCreateFixSpec, FileNormalizeLineEndingsFixSpec,
    FilePrependFixSpec, FileRemoveFixSpec, FileRenameFixSpec, FileStripBidiFixSpec,
    FileStripBomFixSpec, FileStripZeroWidthFixSpec, FileTrimTrailingWhitespaceFixSpec, FixSpec,
    NestedRuleSpec, PathsSpec, RuleSpec, resolve_content_source,
};
pub use engine::{Engine, RuleEntry};
pub use error::{Error, Result};
pub use facts::{FactKind, FactSpec, FactValue, FactValues, evaluate_facts};
pub use level::Level;
pub use registry::{RuleBuilder, RuleRegistry};
pub use report::{FixItem, FixReport, FixRuleResult, FixStatus, Report};
pub use rule::{
    Context, FixContext, FixOutcome, Fixer, PerFileRule, ReadForFix, Rule, RuleResult, Violation,
    check_fix_size, read_for_fix,
};
pub use scope::Scope;
pub use scope_filter::{
    ScopeFilter, ScopeFilterSpec, reject_scope_filter_on_cross_file,
    reject_scope_filter_with_reason,
};
pub use walker::{FileEntry, FileIndex, WalkOptions, walk};
pub use when::{WhenEnv, WhenError, WhenExpr};
