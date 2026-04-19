//! alint-core — engine, walker, rule trait, config AST.
//!
//! See `docs/design/ARCHITECTURE.md` in the alint repository for the
//! rule model, execution order, and crate layout rationale.

mod config;
mod engine;
mod error;
pub mod facts;
mod level;
mod registry;
mod report;
mod rule;
mod scope;
pub mod template;
mod walker;

pub use config::{Config, NestedRuleSpec, PathsSpec, RuleSpec};
pub use engine::Engine;
pub use error::{Error, Result};
pub use facts::{FactKind, FactSpec, FactValue, FactValues, evaluate_facts};
pub use level::Level;
pub use registry::{RuleBuilder, RuleRegistry};
pub use report::Report;
pub use rule::{Context, Rule, RuleResult, Violation};
pub use scope::Scope;
pub use walker::{FileEntry, FileIndex, WalkOptions, walk};
