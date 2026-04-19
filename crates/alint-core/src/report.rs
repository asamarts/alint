use crate::level::Level;
use crate::rule::{RuleResult, Violation};

#[derive(Debug, Clone)]
pub struct Report {
    pub results: Vec<RuleResult>,
}

impl Report {
    pub fn has_errors(&self) -> bool {
        self.results
            .iter()
            .any(|r| r.level == Level::Error && !r.violations.is_empty())
    }

    pub fn has_warnings(&self) -> bool {
        self.results
            .iter()
            .any(|r| r.level == Level::Warning && !r.violations.is_empty())
    }

    pub fn total_violations(&self) -> usize {
        self.results.iter().map(|r| r.violations.len()).sum()
    }

    pub fn failing_rules(&self) -> usize {
        self.results.iter().filter(|r| !r.passed()).count()
    }

    pub fn passing_rules(&self) -> usize {
        self.results.iter().filter(|r| r.passed()).count()
    }
}

/// Outcome of running [`Engine::fix`](crate::Engine::fix) against a
/// repository. One [`FixRuleResult`] per rule that produced violations;
/// rules that passed are omitted.
#[derive(Debug, Clone)]
pub struct FixReport {
    pub results: Vec<FixRuleResult>,
}

#[derive(Debug, Clone)]
pub struct FixRuleResult {
    pub rule_id: String,
    pub level: Level,
    pub items: Vec<FixItem>,
}

#[derive(Debug, Clone)]
pub struct FixItem {
    pub violation: Violation,
    pub status: FixStatus,
}

#[derive(Debug, Clone)]
pub enum FixStatus {
    /// The fix was applied (or would be, under `--dry-run`).
    Applied(String),
    /// The rule has a fixer but it declined to act (e.g. file already
    /// exists, violation lacked a path).
    Skipped(String),
    /// The rule has no fixer; violation stands.
    Unfixable,
}

impl FixReport {
    pub fn applied(&self) -> usize {
        self.items()
            .filter(|i| matches!(i.status, FixStatus::Applied(_)))
            .count()
    }

    pub fn skipped(&self) -> usize {
        self.items()
            .filter(|i| matches!(i.status, FixStatus::Skipped(_)))
            .count()
    }

    pub fn unfixable(&self) -> usize {
        self.items()
            .filter(|i| matches!(i.status, FixStatus::Unfixable))
            .count()
    }

    /// Any rule at `level: error` whose violations were not all fixed.
    pub fn has_unfixable_errors(&self) -> bool {
        self.results
            .iter()
            .any(|r| r.level == Level::Error && has_unresolved(&r.items))
    }

    pub fn has_unfixable_warnings(&self) -> bool {
        self.results
            .iter()
            .any(|r| r.level == Level::Warning && has_unresolved(&r.items))
    }

    fn items(&self) -> impl Iterator<Item = &FixItem> {
        self.results.iter().flat_map(|r| &r.items)
    }
}

fn has_unresolved(items: &[FixItem]) -> bool {
    items
        .iter()
        .any(|i| matches!(i.status, FixStatus::Skipped(_) | FixStatus::Unfixable))
}
