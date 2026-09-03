//! `bench gate` — programmatic publish gate for a macro
//! `results.json`. Replaces the unenforced, never-met "skim
//! `results.json` for any cell CV > 10 %" human step in
//! `RELEASING.md`.
//!
//! Both checks were validated against the full cross-version
//! corpus (v0.5.7 → v0.9.22 plus the two held v0.9.23 runs); see
//! `docs/benchmarks/investigations/2026-05-bench-runner-instability/README.md`
//! for the evidence and why a flat per-cell CV gate is the wrong
//! instrument.
//!
//! - **Quality** — is this run trustworthy? Per-cell within-run
//!   CV (`stddev_ms / mean_ms`) must be ≤ 10 %, but **only for
//!   100k and 1m cells**. 1k and 10k are *advisory*: their
//!   within-run CV is chronic measurement-floor noise (every
//!   shipped v0.9.x release had 7–16 cells over 10 %; the flat
//!   gate was never met), while their cross-version `min_ms` is
//!   stable. Advisory lines are reported, never block.
//!
//! - **Regression** — did perf regress? Only with `--baseline`.
//!   For every cell of size ≥ 10k present in both reports, the
//!   `min_ms` delta vs baseline must not exceed +15 %. `min_ms`
//!   is the robust cross-version statistic (corpus
//!   reproducibility ~2.7 % at ≥10k vs ~8.8 % for `mean_ms`).
//!   Improvements never gate. The small `changed`-mode cells
//!   (1k/10k) are *advisory* here too, not just for CV: they are
//!   dominated by the multi-core wakeup cost of a tiny parallel
//!   burst and drift with the host's C-state / power state
//!   independent of code (see the 2026-09 v0.16 investigation).
//!
//! Exit is non-zero iff a *gating* check fails; advisory notices
//! never affect it.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::{Report, Row};

/// Within-run CV ceiling for gated (100k/1m) cells.
const QUALITY_CV_MAX: f64 = 0.10;
/// `min_ms` regression ceiling vs baseline for ≥10k cells.
const REGRESSION_MAX: f64 = 0.15;
/// Sizes whose within-run CV is advisory-only (measurement-floor
/// noise per the investigation; chronic across every shipped
/// release, so not a publish-quality signal).
const ADVISORY_SIZES: &[&str] = &["1k", "10k"];
/// Sizes the cross-version regression check applies to (`min_ms`
/// is reproducible here; 1k is not, at any statistic).
const REGRESSION_SIZES: &[&str] = &["10k", "100k", "1m"];
/// `changed`-mode cells at these sizes are advisory for the REGRESSION check,
/// not just CV. They are dominated by the multi-core wakeup cost of a tiny
/// parallel burst (git-diff runs single-threaded, then the rule `par_iter` runs
/// over a tiny changed subset), which drifts with the host's C-state / frequency
/// behavior independent of any code change. See
/// docs/benchmarks/investigations/2026-09-v0.16-changed-mode-bench-artifact/.
const CHANGED_REGRESSION_ADVISORY_SIZES: &[&str] = &["1k", "10k"];

/// A `changed`-mode small cell reports its regression delta but does not gate.
fn regression_advisory_only(r: &Row) -> bool {
    r.mode == "changed" && CHANGED_REGRESSION_ADVISORY_SIZES.contains(&r.size_label.as_str())
}

fn cv(r: &Row) -> f64 {
    if r.mean_ms == 0.0 {
        0.0
    } else {
        r.stddev_ms / r.mean_ms
    }
}

fn cell(r: &Row) -> String {
    format!("{} {} {} {}", r.tool, r.scenario, r.size_label, r.mode)
}

fn key(r: &Row) -> (String, String, String, String) {
    (
        r.tool.clone(),
        r.scenario.clone(),
        r.size_label.clone(),
        r.mode.clone(),
    )
}

/// Result of evaluating a report against the gate. Pure data so
/// the policy is unit-testable without filesystem access or
/// constructing a `Fingerprint`.
#[derive(Debug, Default)]
pub struct Outcome {
    pub gating_failures: usize,
    pub advisories: usize,
    pub lines: Vec<String>,
}

/// The gate policy as a pure function over rows. `baseline` is
/// the prior release's rows for the regression check (skipped
/// when `None`).
pub fn evaluate(rows: &[Row], baseline: Option<&[Row]>) -> Outcome {
    let mut o = Outcome::default();

    // ── Quality: within-run CV, gated on 100k/1m only ──
    o.lines.push(format!(
        "[quality] within-run CV (gate: 100k+ ≤ {:.0}%; 1k/10k advisory)",
        QUALITY_CV_MAX * 100.0
    ));
    let mut by_cv: Vec<&Row> = rows.iter().collect();
    by_cv.sort_by(|a, b| {
        cv(b)
            .partial_cmp(&cv(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut q_fail = 0usize;
    for r in by_cv {
        let c = cv(r);
        if c <= QUALITY_CV_MAX {
            continue;
        }
        if ADVISORY_SIZES.contains(&r.size_label.as_str()) {
            o.advisories += 1;
            o.lines.push(format!(
                "  ADVISORY {:34} CV {:5.1}% (mean {:.1}ms) — measurement-floor, not gating",
                cell(r),
                c * 100.0,
                r.mean_ms
            ));
        } else {
            q_fail += 1;
            o.gating_failures += 1;
            o.lines.push(format!(
                "  FAIL     {:34} CV {:5.1}% (mean {:.1}ms) > {:.0}%",
                cell(r),
                c * 100.0,
                r.mean_ms,
                QUALITY_CV_MAX * 100.0
            ));
        }
    }
    if q_fail == 0 {
        o.lines.push(format!(
            "  quality: PASS ({} advisory small-cell notice(s))",
            o.advisories
        ));
    }

    // ── Regression: min_ms vs baseline, ≥10k ──
    match baseline {
        None => o.lines.push("[regression] skipped (no --baseline)".into()),
        Some(base) => {
            let bmap: HashMap<_, _> = base.iter().map(|r| (key(r), r)).collect();
            o.lines.push(format!(
                "[regression] min_ms vs baseline (gate: ≥10k, +{:.0}%; small changed cells advisory)",
                REGRESSION_MAX * 100.0
            ));
            let mut deltas: Vec<(f64, String, bool)> = Vec::new();
            let mut r_fail = 0usize;
            for r in rows {
                if !REGRESSION_SIZES.contains(&r.size_label.as_str()) {
                    continue;
                }
                let Some(b) = bmap.get(&key(r)) else {
                    continue;
                };
                if b.min_ms == 0.0 {
                    continue;
                }
                let d = (r.min_ms - b.min_ms) / b.min_ms;
                let advisory = regression_advisory_only(r);
                if d > REGRESSION_MAX {
                    if advisory {
                        o.advisories += 1;
                    } else {
                        r_fail += 1;
                        o.gating_failures += 1;
                    }
                }
                deltas.push((d, cell(r), advisory));
            }
            deltas.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            for (d, c, advisory) in deltas.iter().take(8) {
                let tag = if *d > REGRESSION_MAX {
                    if *advisory { "ADV " } else { "FAIL" }
                } else {
                    "ok  "
                };
                o.lines
                    .push(format!("  {tag} {:34} min_ms {:+.1}%", c, d * 100.0));
            }
            if r_fail == 0 {
                o.lines.push(format!(
                    "  regression: PASS (max gating min_ms delta ≤ +{:.0}%)",
                    REGRESSION_MAX * 100.0
                ));
            }
        }
    }

    o
}

fn read_report(p: &Path) -> Result<Report> {
    let s = std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
    serde_json::from_str(&s).with_context(|| format!("parsing {} as a bench Report", p.display()))
}

/// CLI entry point: read `results` (and optional `baseline`),
/// print the gate report, exit non-zero on a gating failure.
pub fn run(results: &Path, baseline: Option<&Path>) -> Result<()> {
    let rep = read_report(results)?;
    let base = match baseline {
        Some(p) => Some(read_report(p)?),
        None => None,
    };

    println!("bench gate: {}", results.display());
    println!(
        "  fingerprint: {} / {} / hyperfine {}",
        rep.fingerprint.cpu_model, rep.fingerprint.os, rep.fingerprint.hyperfine_version
    );

    let outcome = evaluate(&rep.rows, base.as_ref().map(|b| b.rows.as_slice()));
    for l in &outcome.lines {
        println!("{l}");
    }

    if outcome.gating_failures > 0 {
        bail!(
            "bench gate: {} gating failure(s) — not publishable; see \
             docs/benchmarks/investigations/2026-05-bench-runner-instability/README.md",
            outcome.gating_failures
        );
    }
    println!("bench gate: PASS");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(scen: &str, size: &str, mode: &str, mean: f64, sd: f64, min: f64) -> Row {
        Row {
            tool: "alint".into(),
            size_files: 0,
            size_label: size.into(),
            scenario: scen.into(),
            mode: mode.into(),
            mean_ms: mean,
            stddev_ms: sd,
            median_ms: mean,
            min_ms: min,
            max_ms: mean + sd,
            samples: 10,
            command: format!("alint ({size}/{scen}/{mode})"),
        }
    }

    #[test]
    fn clean_run_passes() {
        let rows = vec![
            row("S3", "100k", "full", 1159.0, 22.0, 1119.0), // CV 1.9%
            row("S3", "1m", "full", 11_748.0, 215.0, 11_500.0), // CV 1.8%
            row("S1", "10k", "full", 24.0, 11.0, 19.0),      // 10k: advisory
        ];
        let o = evaluate(&rows, None);
        assert_eq!(o.gating_failures, 0, "{:#?}", o.lines);
    }

    #[test]
    fn small_cell_noise_is_advisory_not_failure() {
        // The whole point: 1k/10k blown CV must NOT gate.
        let rows = vec![
            row("S7", "1k", "full", 15.5, 15.0, 10.0),     // CV ~97%
            row("S4", "10k", "changed", 55.0, 19.0, 40.0), // CV ~35%
        ];
        let o = evaluate(&rows, None);
        assert_eq!(o.gating_failures, 0, "small cells must be advisory");
        assert_eq!(o.advisories, 2);
    }

    #[test]
    fn hundredk_cv_spike_fails_quality() {
        let rows = vec![row("S5", "100k", "changed", 768.0, 204.0, 600.0)]; // CV ~27%
        let o = evaluate(&rows, None);
        assert_eq!(o.gating_failures, 1);
    }

    #[test]
    fn min_ms_regression_fails() {
        let base = vec![row("S3", "1m", "full", 11_700.0, 200.0, 11_500.0)];
        let after = vec![row("S3", "1m", "full", 14_000.0, 200.0, 14_000.0)]; // +21.7% min
        let o = evaluate(&after, Some(&base));
        assert_eq!(o.gating_failures, 1);
    }

    #[test]
    fn improvement_never_gates() {
        let base = vec![row("S3", "1m", "full", 730_000.0, 500.0, 726_000.0)];
        let after = vec![row("S3", "1m", "full", 13_200.0, 30.0, 13_100.0)]; // -98%
        let o = evaluate(&after, Some(&base));
        assert_eq!(o.gating_failures, 0, "improvements never gate");
    }

    #[test]
    fn regression_check_ignores_1k() {
        // 1k is unreliable at every statistic — excluded from
        // the regression check (would false-fire on -32% noise).
        let base = vec![row("S9", "1k", "changed", 22.0, 1.0, 21.0)];
        let after = vec![row("S9", "1k", "changed", 15.0, 1.0, 14.0)]; // -32%, 1k
        let o = evaluate(&after, Some(&base));
        assert_eq!(o.gating_failures, 0);
    }

    #[test]
    fn changed_small_cell_regression_is_advisory_not_gating() {
        // The v0.16.0 finding: small `changed` cells are floor-level and
        // environment-sensitive (multi-core wakeup on a tiny burst), so a large
        // min_ms delta there is advisory, not a gate failure. 1k is excluded from
        // the regression check entirely; the 10k changed cell reports as advisory.
        let base = vec![
            row("S1", "10k", "changed", 43.0, 1.0, 43.0),
            row("S1", "1k", "changed", 10.0, 1.0, 10.0),
        ];
        let after = vec![
            row("S1", "10k", "changed", 117.0, 2.0, 117.0), // +172% min
            row("S1", "1k", "changed", 17.0, 1.0, 17.0),    // +70% min
        ];
        let o = evaluate(&after, Some(&base));
        assert_eq!(
            o.gating_failures, 0,
            "small changed cells must not gate: {:#?}",
            o.lines
        );
        assert_eq!(o.advisories, 1, "the 10k changed cell is an advisory");
    }

    #[test]
    fn full_10k_regression_still_gates() {
        // The advisory carve-out is scoped to `changed` mode: `full`/10k still gates.
        let base = vec![row("S1", "10k", "full", 24.0, 0.5, 24.0)];
        let after = vec![row("S1", "10k", "full", 30.0, 0.5, 30.0)]; // +25% min
        let o = evaluate(&after, Some(&base));
        assert_eq!(o.gating_failures, 1, "full/10k regression must still gate");
    }
}
