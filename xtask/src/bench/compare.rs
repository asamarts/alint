//! `xtask bench-compare` — diff two criterion result trees.
//!
//! Criterion stores per-bench timing in
//! `target/criterion/<group>/<id>/{base,new}/estimates.json`.
//! Two consecutive `cargo bench` runs produce a baseline plus a
//! new measurement at every leaf — but for PR-time gating we
//! also need to compare across **separate** runs (e.g. a
//! `target/criterion-main/` saved off the main branch vs. a
//! fresh `target/criterion/` from the PR).
//!
//! This module walks two such trees, pairs estimates.json files
//! by relative path, computes the mean-time delta, and emits a
//! markdown table. Exits non-zero when any pair regresses past
//! `--threshold` percent — the gate-ready signal for CI.
//!
//! Comparison anchor: `mean.point_estimate` (nanoseconds).
//! `median.point_estimate` is also captured but not the gate
//! input — mean is the default criterion reports against and
//! pairs cleanly with the published criterion summary.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Estimates {
    mean: PointEstimate,
}

#[derive(Debug, Deserialize)]
struct PointEstimate {
    point_estimate: f64,
}

/// Walk `root` for every `estimates.json` reachable via a
/// `<...>/new/estimates.json` segment. Returns a map keyed by
/// the path relative to `root` *with* the trailing `new/`
/// segment stripped — so two trees compare cleanly even when
/// only one side has fresh `base/` data.
fn collect(root: &Path) -> Result<BTreeMap<String, f64>> {
    let mut out = BTreeMap::new();
    if !root.exists() {
        bail!("path does not exist: {}", root.display());
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().and_then(|s| s.to_str()) == Some("estimates.json") {
                let Some(parent) = path.parent().and_then(|p| p.file_name()) else {
                    continue;
                };
                if parent != "new" {
                    continue;
                }
                let bench_dir = path.parent().and_then(|p| p.parent()).ok_or_else(|| {
                    anyhow::anyhow!("malformed criterion dir: {}", path.display())
                })?;
                let rel = bench_dir
                    .strip_prefix(root)
                    .with_context(|| format!("strip_prefix {}", bench_dir.display()))?
                    .to_string_lossy()
                    .into_owned();
                let json = std::fs::read_to_string(&path)
                    .with_context(|| format!("read {}", path.display()))?;
                let est: Estimates = serde_json::from_str(&json)
                    .with_context(|| format!("parse {}", path.display()))?;
                out.insert(rel, est.mean.point_estimate);
            }
        }
    }
    Ok(out)
}

#[allow(clippy::cast_precision_loss)]
fn fmt_ns(ns: f64) -> String {
    if ns >= 1_000_000_000.0 {
        format!("{:>8.2} s", ns / 1_000_000_000.0)
    } else if ns >= 1_000_000.0 {
        format!("{:>8.2} ms", ns / 1_000_000.0)
    } else if ns >= 1_000.0 {
        format!("{:>8.2} µs", ns / 1_000.0)
    } else {
        format!("{ns:>8.0} ns")
    }
}

#[derive(Debug)]
struct Row {
    name: String,
    before: f64,
    after: f64,
    pct: f64,
}

pub fn compare(before: &Path, after: &Path, threshold_pct: f64) -> Result<()> {
    let before_map = collect(before)?;
    let after_map = collect(after)?;

    let mut paired: Vec<Row> = Vec::new();
    let mut only_before: Vec<&String> = Vec::new();
    let mut only_after: Vec<&String> = Vec::new();

    for (name, before_ns) in &before_map {
        match after_map.get(name) {
            Some(after_ns) => {
                let pct = (after_ns - before_ns) / before_ns * 100.0;
                paired.push(Row {
                    name: name.clone(),
                    before: *before_ns,
                    after: *after_ns,
                    pct,
                });
            }
            None => only_before.push(name),
        }
    }
    for name in after_map.keys() {
        if !before_map.contains_key(name) {
            only_after.push(name);
        }
    }

    paired.sort_by(|a, b| {
        b.pct
            .abs()
            .partial_cmp(&a.pct.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut md = String::new();
    md.push_str("# bench-compare\n\n");
    let _ = writeln!(md, "Before: `{}`", before.display());
    let _ = writeln!(md, "After:  `{}`", after.display());
    let _ = writeln!(md, "Gate threshold: ±{threshold_pct:.1}%\n");

    md.push_str("| bench | before | after | delta |\n");
    md.push_str("|---|---:|---:|---:|\n");
    for row in &paired {
        let marker = if row.pct.abs() > threshold_pct {
            if row.pct > 0.0 {
                " ⚠️ regress"
            } else {
                " ✅ improve"
            }
        } else {
            ""
        };
        let _ = writeln!(
            md,
            "| `{}` | {} | {} | {:+.2}%{} |",
            row.name,
            fmt_ns(row.before),
            fmt_ns(row.after),
            row.pct,
            marker
        );
    }

    if !only_before.is_empty() {
        md.push_str("\n## Only in --before (removed?)\n\n");
        for name in &only_before {
            let _ = writeln!(md, "- `{name}`");
        }
    }
    if !only_after.is_empty() {
        md.push_str("\n## Only in --after (new bench?)\n\n");
        for name in &only_after {
            let _ = writeln!(md, "- `{name}`");
        }
    }

    print!("{md}");

    let regressed: Vec<&Row> = paired.iter().filter(|r| r.pct > threshold_pct).collect();
    if !regressed.is_empty() {
        eprintln!(
            "\n{} bench(es) regressed past {:.1}%:",
            regressed.len(),
            threshold_pct
        );
        for r in &regressed {
            eprintln!("  - {} ({:+.2}%)", r.name, r.pct);
        }
        bail!("bench-compare: regression threshold exceeded");
    }

    Ok(())
}

pub fn run(before: &Path, after: &Path, threshold: f64) -> Result<()> {
    compare(before, after, threshold)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_est(root: &Path, group: &str, id: &str, mean_ns: f64) {
        let dir = root.join(group).join(id).join("new");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("estimates.json"),
            format!(
                r#"{{"mean":{{"point_estimate":{mean_ns},"confidence_interval":{{"confidence_level":0.95,"lower_bound":0.0,"upper_bound":0.0}},"standard_error":0.0}}}}"#,
            ),
        )
        .unwrap();
    }

    #[test]
    fn collects_and_pairs_estimates() {
        let tmp = tempfile::tempdir().unwrap();
        write_est(tmp.path(), "g", "a", 100.0);
        write_est(tmp.path(), "g", "b", 200.0);
        let map = collect(tmp.path()).unwrap();
        assert_eq!(map.len(), 2);
        assert!((map["g/a"] - 100.0).abs() < 1e-9);
        assert!((map["g/b"] - 200.0).abs() < 1e-9);
    }

    #[test]
    fn passes_when_within_threshold() {
        let before = tempfile::tempdir().unwrap();
        let after = tempfile::tempdir().unwrap();
        write_est(before.path(), "g", "a", 100.0);
        write_est(after.path(), "g", "a", 105.0); // +5%
        compare(before.path(), after.path(), 10.0).expect("within threshold");
    }

    #[test]
    fn fails_on_regression_past_threshold() {
        let before = tempfile::tempdir().unwrap();
        let after = tempfile::tempdir().unwrap();
        write_est(before.path(), "g", "a", 100.0);
        write_est(after.path(), "g", "a", 130.0); // +30%
        let err = compare(before.path(), after.path(), 10.0).unwrap_err();
        assert!(err.to_string().contains("threshold"));
    }

    #[test]
    fn improvements_do_not_fail_gate() {
        let before = tempfile::tempdir().unwrap();
        let after = tempfile::tempdir().unwrap();
        write_est(before.path(), "g", "a", 200.0);
        write_est(after.path(), "g", "a", 100.0); // -50%
        compare(before.path(), after.path(), 10.0).expect("improvement should pass");
    }

    #[test]
    fn handles_only_one_side() {
        let before = tempfile::tempdir().unwrap();
        let after = tempfile::tempdir().unwrap();
        write_est(before.path(), "g", "removed", 100.0);
        write_est(after.path(), "g", "added", 100.0);
        // No paired bench, so nothing to gate on. Should pass.
        compare(before.path(), after.path(), 10.0).expect("orphan benches do not gate");
    }
}
