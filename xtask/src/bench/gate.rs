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
//!   Full/fix cells at ≥10k and every 100k/1m cell present in both
//!   reports must keep their `min_ms` delta within +15 %. `min_ms`
//!   is the robust cross-version statistic (corpus reproducibility
//!   ~2.7 % at ≥10k vs ~8.8 % for `mean_ms`). Improvements never
//!   gate. The small `changed`-mode cells (1k/10k) report their
//!   delta as an advisory instead: they are dominated by the
//!   multi-core wakeup cost of a tiny parallel burst and drift
//!   with the host's C-state / power state independent of code
//!   (see the 2026-09 v0.16 investigation). If both reports declare
//!   different semantic revisions for a scenario, its cells are
//!   intentionally incomparable and the regression check skips them.
//!
//! Exit is non-zero iff a *gating* check fails; advisory notices
//! never affect it.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::{Report, Row};

/// Within-run CV ceiling for gated (100k/1m) cells.
const QUALITY_CV_MAX: f64 = 0.10;
/// `min_ms` regression ceiling vs baseline for gating cells.
const REGRESSION_MAX: f64 = 0.15;
/// Cumulative `min_ms` ceiling vs the long-horizon anchor release
/// (`docs/benchmarks/ANCHOR`) for the same gating cells: creep that passes
/// every adjacent-release check still fails here, until the anchor is
/// deliberately bumped in a reviewed commit.
const ANCHOR_REGRESSION_MAX: f64 = 0.10;
/// Sizes whose within-run CV is advisory-only (measurement-floor
/// noise per the investigation; chronic across every shipped
/// release, so not a publish-quality signal).
const ADVISORY_SIZES: &[&str] = &["1k", "10k"];
/// Sizes the cross-version regression gate applies to (`min_ms` is
/// reproducible here). A changed-mode 1k cell is also evaluated, but only
/// to report its explicitly advisory delta.
const REGRESSION_SIZES: &[&str] = &["10k", "100k", "1m"];
/// `changed`-mode cells at these sizes are advisory for the REGRESSION check,
/// not just CV. They are dominated by the multi-core wakeup cost of a tiny
/// parallel burst (git-diff runs single-threaded, then the rule `par_iter` runs
/// over a tiny changed subset), which drifts with the host's C-state / frequency
/// behavior independent of any code change. See
/// docs/benchmarks/investigations/2026-09-v0.16-changed-mode-bench-artifact/.
const CHANGED_REGRESSION_ADVISORY_SIZES: &[&str] = &["1k", "10k"];

/// A `changed`-mode 1k/10k cell reports its regression delta but does not gate.
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

fn workload_revision_change(
    scenario: &str,
    revisions: &BTreeMap<String, u32>,
    baseline_revisions: &BTreeMap<String, u32>,
) -> Option<(u32, u32)> {
    let revision = *revisions.get(scenario)?;
    let baseline_revision = *baseline_revisions.get(scenario)?;
    (baseline_revision != revision).then_some((baseline_revision, revision))
}

fn report_workload_resets(
    outcome: &mut Outcome,
    revisions: &BTreeMap<String, u32>,
    baseline_revisions: &BTreeMap<String, u32>,
) {
    for scenario in revisions.keys() {
        if let Some((baseline_revision, revision)) =
            workload_revision_change(scenario, revisions, baseline_revisions)
        {
            outcome.lines.push(format!(
                "  RESET    {scenario}: workload r{baseline_revision} → r{revision}; \
                 regression comparison skipped"
            ));
        }
    }
}

/// The gate policy as a pure function over rows. `baseline` is
/// the prior release's rows for the regression check (skipped
/// when `None`). Legacy callers do not provide workload revisions,
/// so they retain the historical like-named-cell comparison.
#[cfg(test)]
fn evaluate(rows: &[Row], baseline: Option<&[Row]>) -> Outcome {
    evaluate_with_revisions(rows, baseline, &BTreeMap::new(), &BTreeMap::new())
}

fn evaluate_with_revisions(
    rows: &[Row],
    baseline: Option<&[Row]>,
    revisions: &BTreeMap<String, u32>,
    baseline_revisions: &BTreeMap<String, u32>,
) -> Outcome {
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

    // ── Regression: min_ms vs baseline; changed 1k/10k is advisory ──
    match baseline {
        None => o.lines.push("[regression] skipped (no --baseline)".into()),
        Some(base) => {
            let bmap: HashMap<_, _> = base.iter().map(|r| (key(r), r)).collect();
            o.lines.push(format!(
                "[regression] min_ms vs baseline (gate: full/fix ≥10k and all 100k/1m, +{:.0}%; changed 1k/10k advisory)",
                REGRESSION_MAX * 100.0
            ));
            report_workload_resets(&mut o, revisions, baseline_revisions);
            let mut deltas: Vec<(f64, String, bool)> = Vec::new();
            let mut r_fail = 0usize;
            for r in rows {
                if workload_revision_change(&r.scenario, revisions, baseline_revisions).is_some() {
                    continue;
                }
                let advisory = regression_advisory_only(r);
                if !REGRESSION_SIZES.contains(&r.size_label.as_str()) && !advisory {
                    continue;
                }
                let Some(b) = bmap.get(&key(r)) else {
                    continue;
                };
                if b.min_ms == 0.0 {
                    continue;
                }
                let d = (r.min_ms - b.min_ms) / b.min_ms;
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

/// Cumulative check against the anchor release: every cell the regression
/// gate covers (advisory `changed` cells and workload resets excluded) must
/// stay within `ANCHOR_REGRESSION_MAX` of the anchor's `min_ms`.
fn evaluate_anchor(
    rows: &[Row],
    anchor: &[Row],
    anchor_version: &str,
    revisions: &BTreeMap<String, u32>,
    anchor_revisions: &BTreeMap<String, u32>,
) -> Outcome {
    let mut o = Outcome::default();
    o.lines.push(format!(
        "[anchor] min_ms vs anchor {anchor_version} (gate: same cells, +{:.0}% cumulative)",
        ANCHOR_REGRESSION_MAX * 100.0
    ));
    for scenario in revisions.keys() {
        if let Some((from, to)) = workload_revision_change(scenario, revisions, anchor_revisions) {
            o.lines.push(format!(
                "  RESET    {scenario}: workload r{from} → r{to} since the anchor; skipped"
            ));
        }
    }
    let amap: HashMap<_, _> = anchor.iter().map(|r| (key(r), r)).collect();
    let mut deltas: Vec<(f64, String)> = Vec::new();
    for r in rows {
        if workload_revision_change(&r.scenario, revisions, anchor_revisions).is_some()
            || regression_advisory_only(r)
            || !REGRESSION_SIZES.contains(&r.size_label.as_str())
        {
            continue;
        }
        let Some(a) = amap.get(&key(r)) else {
            continue;
        };
        if a.min_ms == 0.0 {
            continue;
        }
        let d = (r.min_ms - a.min_ms) / a.min_ms;
        if d > ANCHOR_REGRESSION_MAX {
            o.gating_failures += 1;
        }
        deltas.push((d, cell(r)));
    }
    deltas.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    for (d, c) in deltas.iter().take(8) {
        let tag = if *d > ANCHOR_REGRESSION_MAX {
            "FAIL"
        } else {
            "ok  "
        };
        o.lines.push(format!(
            "  {tag} {c:34} min_ms {:+.1}% vs anchor",
            d * 100.0
        ));
    }
    if o.gating_failures == 0 {
        o.lines.push(format!(
            "  anchor: PASS (max cumulative min_ms delta ≤ +{:.0}%)",
            ANCHOR_REGRESSION_MAX * 100.0
        ));
    } else {
        o.lines.push(
            "  anchor: FAIL; fix the regression, or accept it by bumping \
             docs/benchmarks/ANCHOR in a reviewed commit with a reason"
                .into(),
        );
    }
    o
}

/// Resolve `--anchor`: `auto` reads the version from `docs/benchmarks/ANCHOR`
/// and finds its `results.json` beside the gated run's (same arch dir).
fn resolve_anchor(anchor: &str, results: &Path) -> Result<(PathBuf, String)> {
    if anchor != "auto" {
        let path = PathBuf::from(anchor);
        let label = path
            .parent()
            .and_then(Path::file_name)
            .map_or_else(|| anchor.to_string(), |n| n.to_string_lossy().into_owned());
        return Ok((path, label));
    }
    let root = crate::workspace_root_from_xtask()?;
    let text = std::fs::read_to_string(root.join("docs/benchmarks/ANCHOR"))
        .context("reading docs/benchmarks/ANCHOR")?;
    let version = super::micro_det::anchor_version(&text)
        .context("docs/benchmarks/ANCHOR names no version")?;
    let arch_dir = results
        .parent()
        .and_then(Path::parent)
        .context("cannot derive the arch directory from --results")?;
    Ok((arch_dir.join(&version).join("results.json"), version))
}

fn read_report(p: &Path) -> Result<Report> {
    let s = std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
    serde_json::from_str(&s).with_context(|| format!("parsing {} as a bench Report", p.display()))
}

/// CLI entry point: read `results` (and optional `baseline`),
/// print the gate report, exit non-zero on a gating failure.
pub fn run(results: &Path, baseline: Option<&Path>, anchor: Option<&str>) -> Result<()> {
    let rep = read_report(results)?;
    let base = match baseline {
        Some(p) => Some(read_report(p)?),
        None => None,
    };
    let anchor = match anchor {
        Some(a) => {
            let (path, label) = resolve_anchor(a, results)?;
            Some((read_report(&path)?, label))
        }
        None => None,
    };

    println!("bench gate: {}", results.display());
    println!(
        "  fingerprint: {} / {} / hyperfine {}",
        rep.fingerprint.cpu_model, rep.fingerprint.os, rep.fingerprint.hyperfine_version
    );

    let empty_revisions = BTreeMap::new();
    let outcome = evaluate_with_revisions(
        &rep.rows,
        base.as_ref().map(|report| report.rows.as_slice()),
        &rep.args.scenario_revisions,
        base.as_ref()
            .map_or(&empty_revisions, |report| &report.args.scenario_revisions),
    );
    for l in &outcome.lines {
        println!("{l}");
    }
    let mut gating_failures = outcome.gating_failures;
    if let Some((anchor, label)) = &anchor {
        let anchored = evaluate_anchor(
            &rep.rows,
            &anchor.rows,
            label,
            &rep.args.scenario_revisions,
            &anchor.args.scenario_revisions,
        );
        for l in &anchored.lines {
            println!("{l}");
        }
        gating_failures += anchored.gating_failures;
    }

    if gating_failures > 0 {
        bail!(
            "bench gate: {gating_failures} gating failure(s) — not publishable; see \
             docs/benchmarks/investigations/2026-05-bench-runner-instability/README.md"
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
            times_ms: Vec::new(),
            peak_rss_kb: None,
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
    fn workload_revision_change_skips_only_that_scenario_regression() {
        let base = vec![
            row("S3", "100k", "full", 100.0, 1.0, 100.0),
            row("S4", "100k", "full", 100.0, 1.0, 100.0),
        ];
        let after = vec![
            row("S3", "100k", "full", 110.0, 1.0, 110.0),
            row("S4", "100k", "full", 140.0, 1.0, 140.0),
        ];
        let revisions = BTreeMap::from([("S3".to_string(), 1), ("S4".to_string(), 2)]);
        let baseline_revisions = BTreeMap::from([("S3".to_string(), 1), ("S4".to_string(), 1)]);

        let outcome = evaluate_with_revisions(&after, Some(&base), &revisions, &baseline_revisions);

        assert_eq!(outcome.gating_failures, 0, "{:#?}", outcome.lines);
        assert!(
            outcome
                .lines
                .iter()
                .any(|line| line.contains("S4: workload r1 → r2")),
            "{:#?}",
            outcome.lines
        );
        assert!(
            outcome
                .lines
                .iter()
                .any(|line| line.contains("S3 100k full")),
            "unchanged workloads must still be compared: {:#?}",
            outcome.lines
        );
    }

    #[test]
    fn improvement_never_gates() {
        let base = vec![row("S3", "1m", "full", 730_000.0, 500.0, 726_000.0)];
        let after = vec![row("S3", "1m", "full", 13_200.0, 30.0, 13_100.0)]; // -98%
        let o = evaluate(&after, Some(&base));
        assert_eq!(o.gating_failures, 0, "improvements never gate");
    }

    #[test]
    fn regression_check_ignores_full_1k() {
        // Full/fix 1k is unreliable at every statistic and excluded from the
        // regression check; changed 1k is handled separately as an advisory.
        let base = vec![row("S9", "1k", "full", 22.0, 1.0, 21.0)];
        let after = vec![row("S9", "1k", "full", 30.0, 1.0, 29.0)]; // +38%, 1k
        let o = evaluate(&after, Some(&base));
        assert_eq!(o.gating_failures, 0);
        assert_eq!(o.advisories, 0);
    }

    #[test]
    fn changed_small_cell_regression_is_advisory_not_gating() {
        // The v0.16.0 finding: small `changed` cells are floor-level and
        // environment-sensitive (multi-core wakeup on a tiny burst), so a large
        // min_ms delta there is reported as an advisory, not a gate failure.
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
        assert_eq!(o.advisories, 2, "both changed small cells are advisories");
        assert!(
            o.lines
                .iter()
                .any(|line| line.contains("ADV") && line.contains("1k changed")),
            "the 1k delta must be visible: {:#?}",
            o.lines
        );
    }

    #[test]
    fn full_10k_regression_still_gates() {
        // The advisory carve-out is scoped to `changed` mode: `full`/10k still gates.
        let base = vec![row("S1", "10k", "full", 24.0, 0.5, 24.0)];
        let after = vec![row("S1", "10k", "full", 30.0, 0.5, 30.0)]; // +25% min
        let o = evaluate(&after, Some(&base));
        assert_eq!(o.gating_failures, 1, "full/10k regression must still gate");
    }

    #[test]
    fn anchor_flags_cumulative_creep_that_adjacent_checks_pass() {
        let anchor = vec![
            row("S2", "100k", "full", 1000.0, 10.0, 1000.0),
            row("S1", "1m", "full", 4000.0, 40.0, 4000.0),
        ];
        // +12% vs anchor: under the +15% adjacent gate, over the +10% anchor gate.
        let rows = vec![
            row("S2", "100k", "full", 1120.0, 10.0, 1120.0),
            row("S1", "1m", "full", 4200.0, 40.0, 4200.0),
        ];
        let empty = BTreeMap::new();
        let o = evaluate_anchor(&rows, &anchor, "v0.20.0", &empty, &empty);
        assert_eq!(o.gating_failures, 1, "{:#?}", o.lines);
        assert!(
            o.lines
                .iter()
                .any(|l| l.contains("FAIL") && l.contains("S2 100k full"))
        );
    }

    #[test]
    fn anchor_skips_advisory_cells_and_workload_resets() {
        let anchor = vec![
            row("S1", "1k", "changed", 10.0, 1.0, 10.0),
            row("S4", "100k", "full", 1000.0, 10.0, 1000.0),
        ];
        let rows = vec![
            row("S1", "1k", "changed", 30.0, 1.0, 30.0),
            row("S4", "100k", "full", 2000.0, 10.0, 2000.0),
        ];
        let revisions: BTreeMap<String, u32> = [("S4".to_string(), 4)].into_iter().collect();
        let anchor_revisions: BTreeMap<String, u32> = [("S4".to_string(), 3)].into_iter().collect();
        let o = evaluate_anchor(&rows, &anchor, "v0.20.0", &revisions, &anchor_revisions);
        assert_eq!(o.gating_failures, 0, "{:#?}", o.lines);
        assert!(
            o.lines
                .iter()
                .any(|l| l.contains("RESET") && l.contains("S4"))
        );
    }
}
