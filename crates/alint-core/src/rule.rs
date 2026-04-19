use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::facts::FactValues;
use crate::level::Level;
use crate::registry::RuleRegistry;
use crate::walker::FileIndex;

/// A single linting violation produced by a rule.
#[derive(Debug, Clone)]
pub struct Violation {
    pub path: Option<PathBuf>,
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl Violation {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            path: None,
            message: message.into(),
            line: None,
            column: None,
        }
    }

    #[must_use]
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
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
#[derive(Debug, Clone)]
pub struct RuleResult {
    pub rule_id: String,
    pub level: Level,
    pub policy_url: Option<String>,
    pub violations: Vec<Violation>,
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
#[derive(Debug)]
pub struct Context<'a> {
    pub root: &'a Path,
    pub index: &'a FileIndex,
    pub registry: Option<&'a RuleRegistry>,
    pub facts: Option<&'a FactValues>,
    pub vars: Option<&'a HashMap<String, String>>,
}

/// Trait every built-in and plugin rule implements.
pub trait Rule: Send + Sync + std::fmt::Debug {
    fn id(&self) -> &str;
    fn level(&self) -> Level;
    fn policy_url(&self) -> Option<&str> {
        None
    }
    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>>;
}
