//! alint-core — engine, walker, rule trait, config AST.
//!
//! See `PROPOSAL.md` at the repository root for the design rationale.

mod config;
mod engine;
mod error;
mod level;
mod registry;
mod report;
mod rule;
mod scope;
mod walker;

pub use config::{Config, PathsSpec, RuleSpec};
pub use engine::Engine;
pub use error::{Error, Result};
pub use level::Level;
pub use registry::{RuleBuilder, RuleRegistry};
pub use report::Report;
pub use rule::{Context, Rule, RuleResult, Violation};
pub use scope::Scope;
pub use walker::{FileEntry, FileIndex, WalkOptions, walk};
