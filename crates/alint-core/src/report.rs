use std::sync::Arc;

use crate::level::Level;
use crate::rule::{FixEdit, RuleResult, Violation};

/// Prefix the engine puts on a [`FixStatus::Skipped`] reason when a fix was
/// *attempted but errored* (a fixer `Err`, or a failed write) -- as opposed to
/// a declined or unfixable skip. [`FixReport::had_fix_error`] recognizes it and
/// `Engine::fix` produces it; kept here as the single source of truth so the
/// two never drift.
pub const FIX_ERROR_PREFIX: &str = "fix error:";

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
    pub rule_id: Arc<str>,
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
    /// A fix is available but was NOT applied, so the violation stands: an
    /// `Unsafe` edit without `--unsafe-fixes`, a `Suggestion`-tier edit, or
    /// an edit whose post-edit verification failed (the engine declined to
    /// write rather than corrupt the file). `summary` is the human
    /// one-liner; `edit` is the proposed change, carried so `fix --diff
    /// --unsafe-fixes` can preview it and the check-side finding formats can
    /// emit it. Produced whenever a fixer's tier is below the run's threshold --
    /// in Phase 0 that is the `Unsafe` `file_remove` (for `file_absent` /
    /// `no_empty_files` / `no_submodules` / `no_symlinks`) under a bare
    /// `alint fix`, or any fixer a user demotes to `suggestion`.
    Suggested { summary: String, edit: FixEdit },
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

    /// Count of fixes that are available but were not applied (below the
    /// tier threshold, `Suggestion` tier, or verification-demoted). Each
    /// leaves its violation standing.
    pub fn suggested(&self) -> usize {
        self.items()
            .filter(|i| matches!(i.status, FixStatus::Suggested { .. }))
            .count()
    }

    /// Whether any fix was *attempted and errored* (as opposed to declined or
    /// unfixable). The engine reports a fixer error or a failed write as a
    /// `Skipped` whose reason begins with `"fix error:"` (see `Engine::fix`);
    /// this recognizes that convention. Used by `alint fix --fix-only`, which
    /// otherwise exits 0: an errored fix is a real problem, a declined one is
    /// the residual the flag is meant to suppress. The `FIX_ERROR_PREFIX`
    /// constant is the shared source of truth for the marker.
    #[must_use]
    pub fn had_fix_error(&self) -> bool {
        self.items().any(|i| {
            matches!(&i.status, FixStatus::Skipped(reason) if reason.starts_with(FIX_ERROR_PREFIX))
        })
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
    // `Suggested` counts as unresolved: the fix was NOT applied, so at
    // `level: error` it must still drive a nonzero exit (a user must opt
    // into `--unsafe-fixes` or act on the suggestion). This is the W1
    // exit-code contract.
    items.iter().any(|i| {
        matches!(
            i.status,
            FixStatus::Skipped(_) | FixStatus::Suggested { .. } | FixStatus::Unfixable
        )
    })
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
            notes: Vec::new(),
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

    #[test]
    fn suggested_counts_as_unresolved_and_is_counted_separately() {
        // W1 contract: a `Suggested` fix was NOT applied, so at error level
        // it must drive a nonzero exit (has_unfixable_errors), and it is
        // counted apart from applied/skipped/unfixable.
        let sugg = || FixStatus::Suggested {
            summary: "would set $.x".into(),
            edit: FixEdit::SetContent {
                path: std::path::PathBuf::from("f"),
                content: Vec::new(),
            },
        };
        let r = FixReport {
            results: vec![frr(
                "a",
                Level::Error,
                vec![sugg(), FixStatus::Applied("x".into())],
            )],
        };
        assert_eq!(r.suggested(), 1);
        assert_eq!(r.applied(), 1);
        assert_eq!(r.skipped(), 0);
        assert_eq!(r.unfixable(), 0);
        assert!(
            r.has_unfixable_errors(),
            "error-level Suggested is unresolved"
        );

        // At warning level a lone Suggested is a warning-level unresolved,
        // not an error-level one -- severity gates the exit, same as Skipped.
        let rw = FixReport {
            results: vec![frr("b", Level::Warning, vec![sugg()])],
        };
        assert!(!rw.has_unfixable_errors());
        assert!(rw.has_unfixable_warnings());
    }

    #[test]
    fn had_fix_error_recognizes_the_error_prefix() {
        // A fixer error / failed write is a Skipped whose reason starts with
        // FIX_ERROR_PREFIX; a declined skip is not.
        let errored = FixReport {
            results: vec![frr(
                "a",
                Level::Error,
                vec![FixStatus::Skipped(format!(
                    "{FIX_ERROR_PREFIX} permission denied"
                ))],
            )],
        };
        assert!(errored.had_fix_error());

        let declined = FixReport {
            results: vec![frr(
                "a",
                Level::Error,
                vec![FixStatus::Skipped("already exists".into())],
            )],
        };
        assert!(
            !declined.had_fix_error(),
            "a declined skip is not a fix error"
        );
    }
}
