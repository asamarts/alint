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

#[cfg(test)]
mod tests {
    use super::*;

    fn rr(rule_id: &str, level: Level, n_violations: usize) -> RuleResult {
        RuleResult {
            rule_id: rule_id.into(),
            level,
            policy_url: None,
            violations: (0..n_violations)
                .map(|i| Violation::new(format!("v{i}")))
                .collect(),
            is_fixable: false,
        }
    }

    fn frr(rule_id: &str, level: Level, statuses: Vec<FixStatus>) -> FixRuleResult {
        FixRuleResult {
            rule_id: rule_id.into(),
            level,
            items: statuses
                .into_iter()
                .map(|status| FixItem {
                    violation: Violation::new("v"),
                    status,
                })
                .collect(),
        }
    }

    #[test]
    fn empty_report_has_no_errors_or_warnings() {
        let r = Report { results: vec![] };
        assert!(!r.has_errors());
        assert!(!r.has_warnings());
        assert_eq!(r.total_violations(), 0);
        assert_eq!(r.failing_rules(), 0);
        assert_eq!(r.passing_rules(), 0);
    }

    #[test]
    fn passing_rules_count_passing_results() {
        // A passing RuleResult has zero violations.
        let r = Report {
            results: vec![rr("a", Level::Error, 0), rr("b", Level::Warning, 0)],
        };
        assert_eq!(r.passing_rules(), 2);
        assert_eq!(r.failing_rules(), 0);
        assert!(!r.has_errors());
    }

    #[test]
    fn has_errors_true_when_error_level_has_violations() {
        let r = Report {
            results: vec![rr("a", Level::Error, 1), rr("b", Level::Warning, 5)],
        };
        assert!(r.has_errors());
        assert!(r.has_warnings());
        assert_eq!(r.total_violations(), 6);
        assert_eq!(r.failing_rules(), 2);
    }

    #[test]
    fn has_errors_false_when_only_warnings_have_violations() {
        let r = Report {
            results: vec![rr("a", Level::Error, 0), rr("b", Level::Warning, 3)],
        };
        assert!(!r.has_errors());
        assert!(r.has_warnings());
    }

    #[test]
    fn fix_report_applied_skipped_unfixable_counts_summed_across_rules() {
        let r = FixReport {
            results: vec![
                frr(
                    "a",
                    Level::Error,
                    vec![
                        FixStatus::Applied("ok".into()),
                        FixStatus::Applied("ok".into()),
                        FixStatus::Skipped("nope".into()),
                    ],
                ),
                frr(
                    "b",
                    Level::Warning,
                    vec![FixStatus::Unfixable, FixStatus::Applied("ok".into())],
                ),
            ],
        };
        assert_eq!(r.applied(), 3);
        assert_eq!(r.skipped(), 1);
        assert_eq!(r.unfixable(), 1);
    }

    #[test]
    fn has_unfixable_errors_true_when_error_rule_has_unresolved() {
        let r = FixReport {
            results: vec![frr("a", Level::Error, vec![FixStatus::Unfixable])],
        };
        assert!(r.has_unfixable_errors());
        assert!(!r.has_unfixable_warnings());
    }

    #[test]
    fn has_unfixable_errors_false_when_all_applied() {
        let r = FixReport {
            results: vec![frr(
                "a",
                Level::Error,
                vec![FixStatus::Applied("done".into())],
            )],
        };
        assert!(!r.has_unfixable_errors());
    }

    #[test]
    fn has_unfixable_errors_false_when_skip_only_at_warning_level() {
        // Skips at warning level matter for `has_unfixable_warnings`,
        // not `has_unfixable_errors` — severity gates the check.
        let r = FixReport {
            results: vec![frr(
                "a",
                Level::Warning,
                vec![FixStatus::Skipped("nope".into())],
            )],
        };
        assert!(!r.has_unfixable_errors());
        assert!(r.has_unfixable_warnings());
    }

    #[test]
    fn rule_result_passed_method_is_correct() {
        let passing = rr("a", Level::Error, 0);
        let failing = rr("b", Level::Error, 1);
        assert!(passing.passed());
        assert!(!failing.passed());
    }
}
