//! Fix command execution and exit-status policy.

use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use alint_core::{Engine, FixReport, FixRuleResult, FixStatus, WalkOptions, walk};
use alint_output::{
    Format, sanitize_terminal as term, sanitize_terminal_multiline as term_multiline,
};
use anyhow::{Context, Result, bail};

use super::check::{
    ChangedMode, apply_only_filter, exclude_baseline_from_walk, load_baseline, require_directory,
};
use super::config_cmd::{load_rules, render_env, report_pattern_hints};
use super::{Cli, tolerate_broken_pipe};

/// The boolean flags of `alint fix`, grouped so the command signature stays
/// under clippy's argument/bool-parameter caps (mirrors [`crate::SuggestOptions`]).
// Independent CLI flags, not a state machine - same shape (and allow) as `Cli`.
#[allow(clippy::struct_excessive_bools)]
pub(super) struct FixOptions {
    /// `--dry-run`: report what would change, write nothing.
    pub(super) dry_run: bool,
    /// `--unsafe-fixes`: raise the applied tier from Safe to Unsafe.
    pub(super) unsafe_fixes: bool,
    /// `--fix-only`: report only what was applied; exit 0 unless a fix errored.
    pub(super) fix_only: bool,
    /// `--diff`: render a unified diff of the composed result, write nothing.
    pub(super) diff: bool,
}

#[allow(clippy::too_many_lines)]
pub(super) fn cmd_fix(
    path: &Path,
    changed: &ChangedMode,
    opts: &FixOptions,
    only: &[String],
    cli: &Cli,
) -> Result<ExitCode> {
    let &FixOptions {
        dry_run,
        unsafe_fixes,
        fix_only,
        diff,
    } = opts;
    require_directory(path)?;
    // Gate the output format *before* touching the tree: `fix` mutates files,
    // so a format we can't render must fail here, not after the write. Only
    // `human`, `json`, and `markdown` have dedicated fix-report renderers. The
    // finding-oriented formats (SARIF / GitHub / JUnit / GitLab) and the
    // check-side `agent` format would otherwise degrade *silently* to human
    // output with exit 0 — a machine consumer asking for `sarif` and getting
    // human text is the same trap M13 closed for `validate-config`. Fail loudly
    // instead, and point `agent` at its real home: `check --format agent`
    // (whose per-violation `fix_command` drives the agentic fix loop; `fix`
    // itself has no agent report).
    //
    // `--diff` is exempt from the fix-report-format restriction: it emits a
    // unified diff, not a fix report, so it is format-independent (a stray
    // `--format sarif` alongside `--diff` still yields the diff, matching the
    // flag's documented "regardless of --format"). The string is still parsed so
    // a genuine typo (`--format bogus`) fails loudly either way.
    let format: Format = cli.format.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    if !diff && !matches!(format, Format::Human | Format::Json | Format::Markdown) {
        bail!(
            "`alint fix` supports only `--format human`, `--format json`, or \
             `--format markdown` (got {fmt:?}); the SARIF/GitHub/JUnit/GitLab/agent \
             formats describe findings, not fixes - run `alint check --format {fmt}` \
             for those",
            fmt = cli.format,
        );
    }
    let loaded = load_rules(path, cli)?;
    let root = loaded.root.clone();
    let entries = apply_only_filter(loaded.entries, only)?;
    report_pattern_hints(&entries, &loaded.registry, &loaded.facts, cli);
    let mut engine = Engine::from_entries(entries, loaded.registry)
        .with_facts(loaded.facts)
        .with_vars(loaded.vars)
        .with_fix_size_limit(loaded.fix_size_limit);
    if let Some(analysis_time) = cli.as_of {
        engine = engine.with_analysis_time(analysis_time);
    }
    if let Some(set) = changed.resolve(&root)? {
        engine = engine.with_changed_paths(set);
    }

    let effective_gitignore = if cli.no_gitignore {
        false
    } else {
        loaded.respect_gitignore
    };
    // Keep the baseline artifact out of the walk so a broad content-fixer can't
    // rewrite alint's own committed JSON-Lines file (mirrors `check`/`baseline`).
    let effective_baseline = cli
        .baseline
        .clone()
        .or_else(|| loaded.baseline.as_ref().map(|b| root.join(b)));
    // W4: a resolved baseline (flag or config `baseline:` key) makes `fix` skip
    // grandfathered findings and resolve only NEW ones (mirrors `check --baseline`;
    // the artifact is excluded from the walk below). Classified in engine `fix_run`.
    if let Some(baseline_path) = &effective_baseline {
        let baseline = load_baseline(baseline_path)?;
        engine = engine.with_fix_baseline(baseline);
    }
    let mut extra_ignores = loaded.extra_ignores;
    exclude_baseline_from_walk(&mut extra_ignores, &root, effective_baseline.as_deref());
    let walk_opts = WalkOptions {
        respect_gitignore: effective_gitignore,
        extra_ignores,
    };

    let index = walk(&root, &walk_opts).context("walking repository")?;
    // `--unsafe-fixes` raises the applied tier to Unsafe; the default is Safe.
    // At Safe, an Unsafe op (`file_remove`) is surfaced as a suggestion; this
    // flag is what applies it.
    let threshold = if unsafe_fixes {
        alint_core::Applicability::Unsafe
    } else {
        alint_core::Applicability::Safe
    };

    // `--diff`: stage the composed result in memory and render it as a unified
    // diff instead of writing. This is a preview, so it never touches the tree
    // (regardless of `--dry-run`), and the diff is emitted verbatim regardless
    // of `--format` (a unified diff is not a fix report). The exit code matches
    // the equivalent real `fix` for the CONVERGED case, with four caveats inherent
    // to a single-pass no-write preview: (1) it cannot predict an I/O write
    // failure (a read-only target), which a real `fix` would surface as an
    // unfixable error (exit 1); (2) it cannot detect NON-CONVERGENCE -- the
    // preview is one pass, so it never re-walks and never returns the fixpoint's
    // exit 2, even for a config a real `fix` would fail to converge (the report's
    // `non_convergent` is always false here); and (3) it cannot see a CASCADE -- a
    // file a fix CREATES or rewrites this pass, which a LATER pass's rule then flags
    // (e.g. an unfixable finding on the created content), is invisible to the single
    // walk, so the preview can under-report the unfixable count and exit 0 where a
    // real multi-pass `fix` exits 1 (audit: preview-fidelity MED); and (4) it does
    // not make `--dry-run`'s prediction of a create the walk leaves out, which a
    // real `fix` reports unresolved. The preview exit code is thus a LOWER BOUND on
    // a real run's. All four surface the edits as would-apply. See docs/design/v0.17/fixpoint.md.
    if diff {
        let (report, staged) = engine
            .stage_fixes(&root, &index, threshold)
            .context("staging fixes")?;
        let (mut out, _opts) = render_env(cli)?;
        tolerate_broken_pipe(alint_output::write_fix_diff(&staged, &mut out))
            .context("writing diff")?;
        out.flush().ok();
        if report.has_analysis_failures() {
            let mut err = io::stderr().lock();
            for result in &report.results {
                for failure in &result.analysis_failures {
                    let location = failure.report_path().map_or_else(String::new, |path| {
                        format!("{}: ", term(&path.display().to_string()))
                    });
                    let message = term_multiline(&failure.message).replace('\n', "\nalint:   ");
                    writeln!(
                        err,
                        "alint: {location}{}: analysis failure ({}): {message}",
                        term(&result.rule_id),
                        failure.kind.as_str(),
                    )?;
                }
            }
        }
        return Ok(fix_exit_code(&report, fix_only, cli));
    }

    let report = engine
        .fix(&root, &index, &walk_opts, dry_run, threshold)
        .context("applying fixes")?;

    let (mut out, opts) = render_env(cli)?;
    if fix_only {
        // Report the fixes that were applied AND any that ERRORED. `--fix-only`
        // suppresses BENIGN residuals (already-clean / binary / size-limit skips,
        // suggestions, unfixable) -- but a genuine fix error (a `Skipped` carrying
        // `FIX_ERROR_PREFIX`, e.g. a read-only target) drives a nonzero exit
        // (`fix_exit_code` below), so hiding it would leave a CI operator with a
        // failed step, a report reading "0 applied, 0 skipped", and no cause.
        let applied_only = FixReport {
            // Carry the real convergence verdict (was hardcoded `false`): the
            // exit code comes from the unfiltered `report`, but the rendered
            // JSON's `non_convergent` is the only machine signal of a capped
            // exit-2 run, so `--fix-only --format json` must not report `false`
            // on a non-convergent run.
            non_convergent: report.non_convergent,
            results: report
                .results
                .iter()
                .filter_map(|r| {
                    let items: Vec<_> = r
                        .items
                        .iter()
                        .filter(|i| match &i.status {
                            FixStatus::Applied(_) => true,
                            // Classify on the structural kind, not the reason text:
                            // a benign residual on a file named `fix error:*` must
                            // not be kept as an error (audit F2, 2026-09-20).
                            FixStatus::Skipped { kind, .. } => {
                                *kind == alint_core::SkipKind::Errored
                            }
                            _ => false,
                        })
                        .cloned()
                        .collect();
                    (!items.is_empty() || !r.analysis_failures.is_empty()).then(|| FixRuleResult {
                        rule_id: r.rule_id.clone(),
                        level: r.level,
                        items,
                        analysis_failures: r.analysis_failures.clone(),
                    })
                })
                .collect(),
        };
        tolerate_broken_pipe(format.write_fix_report(&applied_only, &mut out, opts, dry_run))
            .context("writing output")?;
    } else {
        tolerate_broken_pipe(format.write_fix_report(&report, &mut out, opts, dry_run))
            .context("writing output")?;
    }
    out.flush().ok();

    Ok(fix_exit_code(&report, fix_only, cli))
}

/// The process exit code for a fix pass, shared by the write path, `--dry-run`,
/// and `--diff`. Thin wrapper over [`fix_exit_status`] (which carries the logic
/// and is unit-testable, `ExitCode` being opaque).
fn fix_exit_code(report: &FixReport, fix_only: bool, cli: &Cli) -> ExitCode {
    ExitCode::from(fix_exit_status(report, fix_only, cli.fail_on_warning))
}

/// The numeric exit status for a fix pass: `0` success, `1` a residual that
/// stands (an unfixable/errored fix), `2` the fixpoint could not converge.
/// `--fix-only` applied what it could, so a residual finding is expected and
/// suppressed - it fails only if a fix was attempted and errored. Otherwise an
/// unfixable error (or an unfixable warning under `--fail-on-warning`) fails.
pub(super) fn fix_exit_status(report: &FixReport, fix_only: bool, fail_on_warning: bool) -> u8 {
    // Non-convergence is the most severe fix outcome and outranks every other
    // branch, including `--fix-only`'s residual suppression: the fixpoint loop
    // hit its pass cap without settling (a config whose fixes keep re-triggering,
    // see docs/design/v0.17/fixpoint.md 2). Exit 2 -- "fix could not complete" --
    // is distinct from 1 ("ran, violations remain") and shares the code alint
    // already uses for a user-fixable config error (M11), which is what this is.
    if report.non_convergent {
        return 2;
    }
    if fix_only {
        // A benign policy residual is suppressed. Incomplete analysis and a
        // genuine attempted-fix error still fail.
        return u8::from(report.has_analysis_failures() || report.had_fix_error());
    }
    // `had_fix_error()`: a fix was ATTEMPTED and hit a genuine I/O error (a
    // read-only target, ENOSPC, ...), recorded as a `Skipped("fix error: ...")`.
    // That is not a benign declined skip (already-clean, binary, size-limit), so
    // it must fail the process regardless of the rule's level -- otherwise `alint
    // fix` reports success (exit 0) while a warning/info-level fix silently did
    // not land. This matches the `--fix-only` branch above.
    u8::from(
        report.had_fix_error()
            || report.has_unfixable_errors()
            || (fail_on_warning && report.has_unfixable_warnings()),
    )
}
