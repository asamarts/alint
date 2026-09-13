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
//!    applied operations on the second pass. Single-rule (Phase-0
//!    idempotence is a per-fixer guarantee) over the full fixer
//!    catalogue.
//! 4. `fix_converges_when_fully_resolved` — when a single fix pass
//!    reports zero skipped and zero unfixable, a subsequent `check`
//!    reports NO violation (of any level). Single-rule, all fixers.
//! 5. `fix_dry_run_is_pure_single_rule` — dry-run purity over the full
//!    fixer catalogue (the multi-rule `fix_dry_run_is_pure` covers
//!    only 4 fixers).
//! 6. `check_fixable_never_overlaps_a_suggestion` — `check` never tags a
//!    violation auto-fixable when a bare `fix` merely suggests it (the
//!    honesty net for the Unsafe-tier `file_remove` reporting).
//! 7. `fix_unsafe_converges_when_fully_resolved` — a fully-applied
//!    `fix --unsafe-fixes` converges, covering the Unsafe apply path
//!    (e.g. `file_remove`) that the Safe convergence law skips.
//!
//! The single-rule invariants use one fixable rule per scenario on
//! purpose. `alint fix` is single-pass in Phase 0 (the fixpoint re-walk
//! arrives in Phase 1), so a MULTI-rule tree is neither idempotent nor
//! convergent across passes by design: one rule creating a file that
//! another must then fix, one rule RENAMING a file out from under another
//! rule's stale-index violation (the vacated fix is silently deferred to
//! the next `fix`), or two content fixers racing on one file via the
//! compose buffer, are known limitations deferred to the next phase.
//! One rule isolates each fixer's own fixed-point behaviour, which is
//! exactly what Phase 0 guarantees.
//!
//! IMPORTANT: these property invariants build scenarios with an empty
//! `expect` and assert against the returned `ScenarioRun` directly, so
//! `run_scenario` must NOT enforce `Scenario::validate` (the
//! corpus-authoring "assert something" lint) — that would reject every
//! scenario here and turn all of these vacuous. The corpus loader
//! (`scenarios.rs`) enforces `validate` instead.
//!
//! Tuning knobs: `PROPTEST_CASES` env var scales case count.

use alint_testkit::scenario::Step;
use alint_testkit::strategies::{
    any_scenario_tree, fixable_scenario_tree, single_fixable_scenario_tree, with_steps,
};
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
    fn fix_is_idempotent(base in single_fixable_scenario_tree()) {
        // Phase-0 idempotence is a PER-FIXER guarantee: with one rule, a second
        // `fix` pass applies nothing (each fixer is a genuine fixed point). This
        // is the property-level guard for the round-3 non-convergence bugs (F1
        // doubled-CR, F2 interior-CR, F3 stacked-BOM), each a fixer whose second
        // pass still applied.
        //
        // It is deliberately SINGLE-rule. `alint fix` is single-pass in Phase 0
        // (the fixpoint re-walk is Phase 1), so a MULTI-rule tree is not
        // idempotent across two `fix` invocations by design: e.g. rule A's
        // `file_create` makes a `REQUIRED.md` that rule B's `file_content_matches`
        // (`**/*.md`) then flags and appends to on the SECOND pass. Asserting
        // multi-rule idempotence here would encode a guarantee Phase 0 does not
        // make. (This assertion was silently vacuous before the round-3 fix that
        // stopped `run_scenario` from rejecting assertion-free property
        // scenarios, which is why the interaction went unnoticed.)
        let scenario = with_steps(base, vec![Step::Fix, Step::Fix]);
        let Ok(run) = run_scenario(&scenario) else { return Ok(()); };
        let Some(StepOutcome::Fix(second)) = run.steps.get(1) else {
            return Ok(());
        };
        prop_assert_eq!(
            second.applied(),
            0,
            "second fix pass applied {} op(s) for a single fixer; expected idempotence; config:\n{}",
            second.applied(),
            scenario.given.config,
        );
    }

    #[test]
    fn fix_converges_when_fully_resolved(base in single_fixable_scenario_tree()) {
        // Convergence law: after a single, fully-applied `fix` (nothing skipped,
        // nothing unfixable), a subsequent `check` finds NOTHING. Driven by the
        // SINGLE-rule strategy so every fixer is exercised with no
        // cross-rule single-pass ordering interference, and asserted against
        // residuals of ANY level. (The old form used the multi-rule strategy --
        // whose fixers were all `level: warning` -- and counted only ERROR-level
        // residuals, so it could never fail: vacuous. Round-3 audit fix.)
        let scenario = with_steps(base, vec![Step::Fix, Step::Check]);
        let Ok(run) = run_scenario(&scenario) else { return Ok(()); };
        let Some((fix_report, check_report)) = extract_fix_then_check(&run) else {
            return Ok(());
        };
        // Only assert convergence when the fix resolved every violation it
        // encountered. A fixer that skipped (binary file, size limit, ...) leaves
        // a real violation on disk; that is not a convergence failure. Likewise a
        // fix surfaced as `suggested` was deliberately withheld at this threshold
        // (an Unsafe fixer such as `file_remove` under a bare `fix`), so its
        // violation correctly survives -- also not a convergence failure.
        if fix_report.skipped() > 0 || fix_report.unfixable() > 0 || fix_report.suggested() > 0 {
            return Ok(());
        }
        let residual: usize = check_report.results.iter()
            .map(|r| r.violations.len())
            .sum();
        prop_assert_eq!(
            residual,
            0,
            "check still reported {} violation(s) after a fully-applied single-rule fix \
             (non-convergent fixer); config:\n{}",
            residual,
            scenario.given.config,
        );
    }

    #[test]
    fn fix_dry_run_is_pure_single_rule(base in single_fixable_scenario_tree()) {
        // Dry-run purity over the full fixer catalogue: even the content
        // fixers (compose buffer) and the direct-write trio (create/remove/
        // rename, via the stage sink) must leave the tree byte-identical under
        // `fix --dry-run`.
        let scenario = with_steps(base, vec![Step::FixDryRun]);
        let Ok(run) = run_scenario(&scenario) else { return Ok(()); };
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
    fn check_fixable_never_overlaps_a_suggestion(base in single_fixable_scenario_tree()) {
        // Honesty law (round-7): `check` tags a violation `is_fixable` only when a
        // bare `alint fix` would RESOLVE it. A single-rule tree shares one fixer
        // tier, so if that bare fix produced ANY `Suggested` (a below-Safe-threshold
        // fixer such as the now-Unsafe `file_remove`), then NONE of its violations
        // may be tagged fixable by `check` -- otherwise `check` promises a
        // resolution the bare `fix` withholds (it needs `--unsafe-fixes`). This is
        // the net that would have caught the pre-fix regression where an Unsafe
        // `file_remove` was still counted "auto-fixable" in `check`.
        let scenario = with_steps(base, vec![Step::Check, Step::Fix]);
        let Ok(run) = run_scenario(&scenario) else { return Ok(()); };
        let (Some(StepOutcome::Check(check)), Some(StepOutcome::Fix(fix))) =
            (run.steps.first(), run.steps.get(1))
        else {
            return Ok(());
        };
        if fix.suggested() == 0 {
            return Ok(()); // no below-threshold fixer in play; nothing to assert
        }
        let check_fixable: usize = check
            .results
            .iter()
            .flat_map(|r| &r.violations)
            .filter(|v| v.is_fixable)
            .count();
        prop_assert_eq!(
            check_fixable,
            0,
            "check tagged {} violation(s) auto-fixable, but a bare fix only SUGGESTED \
             (did not resolve) them; config:\n{}",
            check_fixable,
            scenario.given.config,
        );
    }

    #[test]
    fn fix_unsafe_converges_when_fully_resolved(base in single_fixable_scenario_tree()) {
        // Convergence under `--unsafe-fixes` (round-7): the Safe-threshold
        // convergence law early-returns on a `Suggested` below-threshold fixer, so
        // since `file_remove` became Unsafe that op got ZERO apply/convergence
        // coverage from the property net. At the Unsafe threshold every shipped
        // fixer applies (`Safe <= Unsafe`, `Unsafe == Unsafe`), so a single
        // fully-applied `fix --unsafe-fixes` must leave `check` finding nothing --
        // restoring file_remove's apply + convergence coverage.
        let scenario = with_steps(base, vec![Step::FixUnsafe, Step::Check]);
        let Ok(run) = run_scenario(&scenario) else { return Ok(()); };
        let Some((fix_report, check_report)) = extract_fix_then_check(&run) else {
            return Ok(());
        };
        // Same guard as the Safe law: a genuine skip/unfixable leaves a real
        // violation. No Suggestion-tier fixer is generated, so `suggested` is 0 at
        // the Unsafe threshold; guard it anyway for parity.
        if fix_report.skipped() > 0 || fix_report.unfixable() > 0 || fix_report.suggested() > 0 {
            return Ok(());
        }
        let residual: usize = check_report.results.iter().map(|r| r.violations.len()).sum();
        prop_assert_eq!(
            residual,
            0,
            "check still reported {} violation(s) after a fully-applied single-rule \
             `fix --unsafe-fixes` (non-convergent fixer); config:\n{}",
            residual,
            scenario.given.config,
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
