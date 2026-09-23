use std::sync::Arc;

use crate::level::Level;
use crate::rule::{FixEdit, RuleResult, Violation};

/// Human-output prefix the engine puts on an errored `Skipped` reason (a fixer
/// `Err`, or a failed write). Classification is now STRUCTURAL
/// ([`SkipKind::Errored`], via [`FixStatus::errored`]); this is only the display
/// convention so `fix` output still reads `fix error: ...`. Kept as the single
/// source of truth for that text.
pub const FIX_ERROR_PREFIX: &str = "fix error:";

/// Human-output prefix on a baseline-grandfathered `Skipped` reason
/// (`fix --baseline`). Classification is now STRUCTURAL ([`SkipKind::Baselined`],
/// via [`FixStatus::baselined`]); this is only the display convention. Single
/// source of truth for that text.
pub const BASELINED_SKIP_PREFIX: &str = "baselined:";

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
    /// Set by the fixpoint loop when it hit the pass cap without converging (a
    /// config whose fixes keep re-triggering). Drives the distinct `exit 2`
    /// ("fix could not complete", vs `1` = "ran, violations remain"). `false`
    /// for a converged run and for any single-pass construction.
    /// See docs/design/v0.17/fixpoint.md.
    pub non_convergent: bool,
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

/// Why a fixable violation was left unresolved by a `Skipped` outcome. This
/// classification is STRUCTURAL -- never sniffed from the human `reason` string.
/// The reason is frequently built from a file path (`"{path} exceeds ..."`), so a
/// path could forge a sentinel prefix and flip the exit code (audit F1/F2,
/// 2026-09-20): a file named `baselined:*` made a genuinely-unresolved error skip
/// look grandfathered (exit 0 instead of 1), and one named `fix error:*` made a
/// benign residual look like a fix error (exit 1 instead of 0). Keying off this
/// enum makes the class unforgeable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipKind {
    /// The fixer declined (already-satisfied, missing path, unresolvable span,
    /// over `fix_size_limit`, below the tier threshold, an isolation conflict).
    /// The violation STANDS -> a nonzero exit at `level: error`.
    Declined,
    /// A fix was ATTEMPTED and hit a genuine I/O error (a read-only target,
    /// ENOSPC, a failed re-read). Recognized by [`FixReport::had_fix_error`];
    /// the violation stands.
    Errored,
    /// Grandfathered by a `fix --baseline` run -- intentionally not fixed, like a
    /// baseline-suppressed `check` finding. Benign: NOT unresolved, never a
    /// nonzero exit.
    Baselined,
}

#[derive(Debug, Clone)]
pub enum FixStatus {
    /// The fix was applied (or would be, under `--dry-run`).
    Applied(String),
    /// The rule has a fixer but the fix was not applied; `reason` is the human
    /// one-liner and `kind` classifies it (see [`SkipKind`]). Construct via
    /// [`FixStatus::declined`] / [`FixStatus::errored`] / [`FixStatus::baselined`]
    /// so the class is set explicitly at every site.
    Skipped { reason: String, kind: SkipKind },
    /// A fix is available but was NOT applied, so the violation stands: an
    /// `Unsafe` edit without `--unsafe-fixes`, a `Suggestion`-tier edit, or
    /// an edit whose post-edit verification failed (the engine declined to
    /// write rather than corrupt the file). `summary` is the human one-liner (what
    /// every shipped formatter renders). `edit` is the proposed change when the
    /// fixer has an editor-expressible form, `None` for a fixer with no `fix_edit`
    /// -- a spawning/side-effect op such as `git_untrack` (`git rm --cached` has no
    /// worktree edit) or a `command` fix -- which is still a genuine withheld
    /// suggestion (`requires --unsafe-fixes`), NOT a decline. The field is
    /// public-API context only: no built-in formatter reads it (human/json/markdown
    /// use `summary`; `fix --diff` recomputes edits via the stage pass), so `None`
    /// renders identically.
    /// Produced whenever a fixer's tier is below the run's threshold -- the
    /// `Unsafe` `file_remove` / `git_untrack` (on `file_absent` etc.) under a bare
    /// `alint fix`, or any fixer a user demotes to `suggestion`.
    Suggested {
        summary: String,
        edit: Option<FixEdit>,
    },
    /// The rule has no fixer; violation stands.
    Unfixable,
}

impl FixStatus {
    /// A `Skipped` the fixer DECLINED (the violation stands). The common case.
    pub fn declined(reason: impl Into<String>) -> Self {
        Self::Skipped {
            reason: reason.into(),
            kind: SkipKind::Declined,
        }
    }

    /// A `Skipped` for a fix that was ATTEMPTED and errored (I/O). The reason is
    /// conventionally prefixed [`FIX_ERROR_PREFIX`] for human output, but
    /// [`FixReport::had_fix_error`] classifies on the kind, not the text.
    pub fn errored(reason: impl Into<String>) -> Self {
        Self::Skipped {
            reason: reason.into(),
            kind: SkipKind::Errored,
        }
    }

    /// A `Skipped` a `fix --baseline` run GRANDFATHERS (benign; never unresolved).
    /// The reason is conventionally prefixed `BASELINED_SKIP_PREFIX` for output,
    /// but the `has_unresolved` classifier keys on the kind, not the text.
    pub fn baselined(reason: impl Into<String>) -> Self {
        Self::Skipped {
            reason: reason.into(),
            kind: SkipKind::Baselined,
        }
    }
}

impl FixReport {
    pub fn applied(&self) -> usize {
        self.items()
            .filter(|i| matches!(i.status, FixStatus::Applied(_)))
            .count()
    }

    pub fn skipped(&self) -> usize {
        self.items()
            .filter(|i| matches!(i.status, FixStatus::Skipped { .. }))
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
    /// unfixable). The engine records a fixer error or a failed write as a
    /// `Skipped { kind: SkipKind::Errored, .. }` (see `Engine::fix`). Used by
    /// `alint fix --fix-only`, which otherwise exits 0: an errored fix is a real
    /// problem, a declined one is the residual the flag is meant to suppress.
    /// Classifies on the structural kind, NOT the reason text -- a residual on a
    /// file named `fix error:*` must not forge this (audit F2, 2026-09-20).
    #[must_use]
    pub fn had_fix_error(&self) -> bool {
        self.items().any(|i| {
            matches!(
                &i.status,
                FixStatus::Skipped {
                    kind: SkipKind::Errored,
                    ..
                }
            )
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
    items.iter().any(|i| match &i.status {
        // A baseline-grandfathered violation (`fix --baseline`) is intentionally
        // not fixed -- like a baseline-suppressed `check` finding, it is NOT
        // unresolved and must not drive a nonzero exit. Every OTHER skip
        // (declined, errored) means the error still stands. Classified on the
        // structural kind, NOT the reason text -- a size-skip on a file named
        // `baselined:*` must not forge benignity (audit F1, 2026-09-20).
        FixStatus::Skipped { kind, .. } => *kind != SkipKind::Baselined,
        FixStatus::Suggested { .. } | FixStatus::Unfixable => true,
        FixStatus::Applied(_) => false,
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
            non_convergent: false,
            results: vec![
                frr(
                    "a",
                    Level::Error,
                    vec![
                        FixStatus::Applied("ok".into()),
                        FixStatus::Applied("ok".into()),
                        FixStatus::declined("nope"),
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
            non_convergent: false,
            results: vec![frr("a", Level::Error, vec![FixStatus::Unfixable])],
        };
        assert!(r.has_unfixable_errors());
        assert!(!r.has_unfixable_warnings());
    }

    #[test]
    fn has_unfixable_errors_false_for_a_baselined_skip_but_true_for_a_plain_one() {
        // W4: a baseline-grandfathered violation is a BENIGN skip -- it must not
        // count as unresolved, so `fix --baseline` exits 0 with only grandfathered
        // findings left. Every OTHER error-level skip still means the error stands.
        let grandfathered = FixReport {
            non_convergent: false,
            results: vec![frr(
                "a",
                Level::Error,
                vec![FixStatus::baselined(format!(
                    "{BASELINED_SKIP_PREFIX} grandfathered"
                ))],
            )],
        };
        assert!(
            !grandfathered.has_unfixable_errors(),
            "a baselined skip must be benign for the exit code"
        );
        let plain = FixReport {
            non_convergent: false,
            results: vec![frr(
                "a",
                Level::Error,
                vec![FixStatus::declined("size limit; not fixed")],
            )],
        };
        assert!(
            plain.has_unfixable_errors(),
            "a non-baselined error-level skip still stands"
        );
    }

    #[test]
    fn has_unfixable_errors_false_when_all_applied() {
        let r = FixReport {
            non_convergent: false,
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
            non_convergent: false,
            results: vec![frr("a", Level::Warning, vec![FixStatus::declined("nope")])],
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
            edit: Some(FixEdit::SetContent {
                path: std::path::PathBuf::from("f"),
                content: Vec::new(),
            }),
        };
        let r = FixReport {
            non_convergent: false,
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
            non_convergent: false,
            results: vec![frr("b", Level::Warning, vec![sugg()])],
        };
        assert!(!rw.has_unfixable_errors());
        assert!(rw.has_unfixable_warnings());
    }

    #[test]
    fn had_fix_error_recognizes_the_errored_kind() {
        // A fixer error / failed write is a Skipped of kind `Errored`
        // (FixStatus::errored); a declined skip is not.
        let errored = FixReport {
            non_convergent: false,
            results: vec![frr(
                "a",
                Level::Error,
                vec![FixStatus::errored(format!(
                    "{FIX_ERROR_PREFIX} permission denied"
                ))],
            )],
        };
        assert!(errored.had_fix_error());

        let declined = FixReport {
            non_convergent: false,
            results: vec![frr(
                "a",
                Level::Error,
                vec![FixStatus::declined("already exists")],
            )],
        };
        assert!(
            !declined.had_fix_error(),
            "a declined skip is not a fix error"
        );
    }

    #[test]
    fn skip_classification_is_structural_not_reason_text() {
        // Audit F1/F2 (2026-09-20): the skip class is the KIND, never a reason-text
        // prefix. A skip reason is usually built from a file path, so a path could
        // otherwise forge a sentinel and flip the exit code.

        // F1: a DECLINED error-level skip whose reason merely STARTS WITH the
        // literal `baselined:` (a file named `baselined:evil.txt`, size-skipped) is
        // STILL unresolved -> exit 1, not the benign exit 0 a baselined skip gets.
        let forged_baseline = FixReport {
            non_convergent: false,
            results: vec![frr(
                "a",
                Level::Error,
                vec![FixStatus::declined(format!(
                    "{BASELINED_SKIP_PREFIX}evil.txt is 40 bytes; exceeds fix_size_limit"
                ))],
            )],
        };
        assert!(
            forged_baseline.has_unfixable_errors(),
            "a declined skip stays unresolved even if its reason starts with `baselined:`"
        );
        // The genuinely baselined skip (kind) is benign despite the same prefix.
        let real_baseline = FixReport {
            non_convergent: false,
            results: vec![frr(
                "a",
                Level::Error,
                vec![FixStatus::baselined(format!(
                    "{BASELINED_SKIP_PREFIX} grandfathered"
                ))],
            )],
        };
        assert!(!real_baseline.has_unfixable_errors());

        // F2: a DECLINED skip whose reason STARTS WITH `fix error:` (a file named
        // `fix error:big.txt`, size-skipped) is NOT a fix error -> `--fix-only`
        // exit 0, not a spurious 1.
        let forged_error = FixReport {
            non_convergent: false,
            results: vec![frr(
                "a",
                Level::Warning,
                vec![FixStatus::declined(format!(
                    "{FIX_ERROR_PREFIX}big.txt is 40 bytes; exceeds fix_size_limit"
                ))],
            )],
        };
        assert!(
            !forged_error.had_fix_error(),
            "a declined skip is not a fix error even if its reason starts with `fix error:`"
        );
        // The genuinely errored skip (kind) IS a fix error despite... no prefix at all.
        let real_error = FixReport {
            non_convergent: false,
            results: vec![frr(
                "a",
                Level::Warning,
                vec![FixStatus::errored("could not write: read-only file system")],
            )],
        };
        assert!(
            real_error.had_fix_error(),
            "an errored-kind skip is a fix error regardless of its reason text"
        );
    }
}
