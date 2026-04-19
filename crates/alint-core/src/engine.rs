use std::collections::HashMap;
use std::path::Path;

use rayon::prelude::*;

use crate::error::Result;
use crate::facts::{FactSpec, FactValues, evaluate_facts};
use crate::registry::RuleRegistry;
use crate::report::Report;
use crate::rule::{Context, Rule, RuleResult, Violation};
use crate::walker::FileIndex;
use crate::when::{WhenEnv, WhenExpr};

/// A rule bundled with an optional `when` expression. Rules with a `when`
/// that evaluates to false at runtime are skipped (no `RuleResult` is
/// produced) — same observable effect as `level: off`, but gated on facts.
#[derive(Debug)]
pub struct RuleEntry {
    pub rule: Box<dyn Rule>,
    pub when: Option<WhenExpr>,
}

impl RuleEntry {
    pub fn new(rule: Box<dyn Rule>) -> Self {
        Self { rule, when: None }
    }

    #[must_use]
    pub fn with_when(mut self, expr: WhenExpr) -> Self {
        self.when = Some(expr);
        self
    }
}

/// Executes a set of rules against a pre-built [`FileIndex`].
///
/// The engine owns a [`RuleRegistry`] so cross-file rules (e.g.
/// `for_each_dir`) can build nested rules on demand during evaluation.
/// Optional `facts` and `vars` (set via the builder chain) are evaluated
/// at run time and threaded into each rule's [`Context`] and into the
/// `when` expression evaluator that gates rules.
#[derive(Debug)]
pub struct Engine {
    entries: Vec<RuleEntry>,
    registry: RuleRegistry,
    facts: Vec<FactSpec>,
    vars: HashMap<String, String>,
}

impl Engine {
    /// Backward-compatible: wrap each rule in a [`RuleEntry`] with no `when`.
    pub fn new(rules: Vec<Box<dyn Rule>>, registry: RuleRegistry) -> Self {
        let entries = rules.into_iter().map(RuleEntry::new).collect();
        Self {
            entries,
            registry,
            facts: Vec::new(),
            vars: HashMap::new(),
        }
    }

    /// Construct from rule entries (each carrying an optional `when`).
    pub fn from_entries(entries: Vec<RuleEntry>, registry: RuleRegistry) -> Self {
        Self {
            entries,
            registry,
            facts: Vec::new(),
            vars: HashMap::new(),
        }
    }

    #[must_use]
    pub fn with_facts(mut self, facts: Vec<FactSpec>) -> Self {
        self.facts = facts;
        self
    }

    #[must_use]
    pub fn with_vars(mut self, vars: HashMap<String, String>) -> Self {
        self.vars = vars;
        self
    }

    pub fn rule_count(&self) -> usize {
        self.entries.len()
    }

    pub fn run(&self, root: &Path, index: &FileIndex) -> Result<Report> {
        let fact_values = evaluate_facts(&self.facts, root, index)?;
        let ctx = Context {
            root,
            index,
            registry: Some(&self.registry),
            facts: Some(&fact_values),
            vars: Some(&self.vars),
        };
        let when_env = WhenEnv {
            facts: &fact_values,
            vars: &self.vars,
        };
        let results: Vec<RuleResult> = self
            .entries
            .par_iter()
            .filter_map(|entry| run_entry(entry, &ctx, &when_env, &fact_values))
            .collect();
        Ok(Report { results })
    }
}

fn run_entry(
    entry: &RuleEntry,
    ctx: &Context<'_>,
    when_env: &WhenEnv<'_>,
    _facts: &FactValues,
) -> Option<RuleResult> {
    if let Some(expr) = &entry.when {
        match expr.evaluate(when_env) {
            Ok(true) => {} // proceed
            Ok(false) => return None,
            Err(e) => {
                return Some(RuleResult {
                    rule_id: entry.rule.id().to_string(),
                    level: entry.rule.level(),
                    policy_url: entry.rule.policy_url().map(str::to_string),
                    violations: vec![Violation::new(format!("when evaluation error: {e}"))],
                });
            }
        }
    }
    Some(run_one(entry.rule.as_ref(), ctx))
}

fn run_one(rule: &dyn Rule, ctx: &Context<'_>) -> RuleResult {
    let violations = match rule.evaluate(ctx) {
        Ok(v) => v,
        Err(e) => vec![Violation::new(format!("rule error: {e}"))],
    };
    RuleResult {
        rule_id: rule.id().to_string(),
        level: rule.level(),
        policy_url: rule.policy_url().map(str::to_string),
        violations,
    }
}
