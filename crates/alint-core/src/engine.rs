use std::collections::HashMap;
use std::path::Path;

use rayon::prelude::*;

use crate::error::Result;
use crate::facts::{FactSpec, evaluate_facts};
use crate::registry::RuleRegistry;
use crate::report::Report;
use crate::rule::{Context, Rule, RuleResult, Violation};
use crate::walker::FileIndex;

/// Executes a set of rules against a pre-built [`FileIndex`].
///
/// The engine owns a [`RuleRegistry`] so cross-file rules (e.g.
/// `for_each_dir`) can build nested rules on demand during evaluation.
/// Optional `facts` and `vars` (set via the builder chain) are evaluated
/// at run time and threaded into each rule's [`Context`].
#[derive(Debug)]
pub struct Engine {
    rules: Vec<Box<dyn Rule>>,
    registry: RuleRegistry,
    facts: Vec<FactSpec>,
    vars: HashMap<String, String>,
}

impl Engine {
    pub fn new(rules: Vec<Box<dyn Rule>>, registry: RuleRegistry) -> Self {
        Self {
            rules,
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
        self.rules.len()
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
        let results: Vec<RuleResult> = self
            .rules
            .par_iter()
            .map(|rule| run_one(rule.as_ref(), &ctx))
            .collect();
        Ok(Report { results })
    }
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
