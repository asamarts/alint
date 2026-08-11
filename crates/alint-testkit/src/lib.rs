//! Internal test kit for alint: scenario DSL + tree-spec utilities.
//!
//! Two layers live here:
//!
//! - [`treespec`] — a generic "declarative tree ↔ filesystem" toolkit
//!   (materialize, verify, extract). Deliberately free of any
//!   alint-specific types; designed to be spun out as a standalone
//!   crate if there's demand (see `TREE_SPEC.md`).
//! - [`scenario`] + [`runner`] — the alint-specific layer that
//!   drives `check` / `fix` against a materialized tree and asserts
//!   on reports.
//!
//! Harnesses typically call [`run_scenario`] + [`assert_scenario`].

mod error;
pub mod runner;
pub mod scenario;
pub mod strategies;
pub mod treespec;

pub use error::{Error, Result};
pub use runner::{ScenarioRun, StepOutcome, assert_scenario, run_scenario};
pub use scenario::{
    CommitSpec, DetailedCommit, DocsCase, DocsExample, ExpectStep, ExpectTreeMode, ExpectViolation,
    Given, GivenGit, LevelName, Scenario, Step,
};
pub use treespec::{
    Discrepancy, ExecNode, ExtractOpts, SymlinkNode, TreeNode, TreeSpec, VerifyMode, VerifyReport,
    extract, materialize, verify,
};
