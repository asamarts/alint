//! Property-based invariants for alint.
//!
//! Each invariant is a `proptest!` that feeds generated scenarios
//! into the testkit runner and asserts one global property:
//!
//! 1. `check_never_panics` — check tolerates arbitrary tree/config
//!    pairs from the broad strategy without panicking. Runner
//!    errors (invalid globs, rule-config errors, etc.) are
//!    permitted; panics are not.
//! 2. `fix_dry_run_is_pure` — under the fixable strategy with
//!    `Step::FixDryRun`, the resulting on-disk state equals the
//!    input tree byte-for-byte.
//! 3. `fix_is_idempotent` — running `fix` twice never performs
//!    applied operations on the second pass.
//! 4. `fix_converges` — when the fix pass reports zero skipped
//!    and zero unfixable, a subsequent `check` reports no errors.
//!
//! Tuning knobs: `PROPTEST_CASES` env var scales case count.

use alint_testkit::scenario::Step;
use alint_testkit::strategies::{any_scenario_tree, fixable_scenario_tree, with_steps};
use alint_testkit::treespec::{Discrepancy, VerifyMode, verify};
use alint_testkit::{ScenarioRun, StepOutcome, run_scenario};
use proptest::prelude::*;

/// Ignore the runner's own `.alint.yml` bookkeeping file when
/// comparing the on-disk state to the input tree.
fn ignore_runner_machinery(discrepancies: &mut Vec<Discrepancy>) {
    discrepancies.retain(|d| !matches!(d, Discrepancy::Extra { path } if path == ".alint.yml"));
}

proptest! {
    // Keep the per-invariant case count small; each case spins up
    // a tempdir and runs alint end-to-end, which is ~5–10 ms.
    #![proptest_config(ProptestConfig {
        cases: 48,
        max_shrink_iters: 64,
        ..ProptestConfig::default()
    })]

    #[test]
    fn check_never_panics(base in any_scenario_tree()) {
        let scenario = with_steps(base, vec![Step::Check]);
        // We don't care whether the scenario's config is valid
        // alint input — only that `run_scenario` never panics.
        // Dropping the Result swallows legitimate errors.
        let _ = run_scenario(&scenario);
    }

    #[test]
    fn fix_dry_run_is_pure(base in fixable_scenario_tree()) {
        let scenario = with_steps(base, vec![Step::FixDryRun]);
        let Ok(run) = run_scenario(&scenario) else { return Ok(()); };
        // After dry-run the disk must equal the input tree.
        let Ok(mut report) = verify(&scenario.given.tree, &run.root, VerifyMode::Strict) else {
            return Ok(());
        };
        ignore_runner_machinery(&mut report.discrepancies);
        prop_assert!(
            report.is_match(),
            "dry-run mutated disk state:\n{report}",
        );
    }

    #[test]
    fn fix_is_idempotent(base in fixable_scenario_tree()) {
        let scenario = with_steps(base, vec![Step::Fix, Step::Fix]);
        let Ok(run) = run_scenario(&scenario) else { return Ok(()); };
        let Some(StepOutcome::Fix(second)) = run.steps.get(1) else {
            return Ok(());
        };
        prop_assert_eq!(
            second.applied(),
            0,
            "second fix pass applied {} op(s); expected idempotence",
            second.applied(),
        );
    }

    #[test]
    fn fix_converges_when_fully_resolved(base in fixable_scenario_tree()) {
        let scenario = with_steps(base, vec![Step::Fix, Step::Check]);
        let Ok(run) = run_scenario(&scenario) else { return Ok(()); };
        let Some((fix_report, check_report)) = extract_fix_then_check(&run) else {
            return Ok(());
        };
        // Only assert convergence when the fix resolved every
        // violation it encountered. Fixers that skipped leave real
        // violations on disk; those aren't bugs.
        if fix_report.skipped() > 0 || fix_report.unfixable() > 0 {
            return Ok(());
        }
        prop_assert!(
            !check_report.has_errors(),
            "check reported {} error-level violations after a fully-applied fix",
            check_report.results.iter()
                .filter(|r| matches!(r.level, alint_core::Level::Error))
                .map(|r| r.violations.len())
                .sum::<usize>(),
        );
    }
}

fn extract_fix_then_check(
    run: &ScenarioRun,
) -> Option<(&alint_core::FixReport, &alint_core::Report)> {
    let fix = match run.steps.first()? {
        StepOutcome::Fix(r) => r,
        StepOutcome::Check(_) => return None,
    };
    let check = match run.steps.get(1)? {
        StepOutcome::Check(r) => r,
        StepOutcome::Fix(_) => return None,
    };
    Some((fix, check))
}
