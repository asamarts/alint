//! Guards the data flow from per-version `results.json` files
//! under `docs/benchmarks/macro/results/<arch>/<ver>/` to the
//! trajectory JSON that alint.org's `/benchmarks/` page renders.
//!
//! The flow is:
//!
//!   bench-record.yml → per-version `results.json` (committed)
//!     ↓ (via xtask docs-export running render-history.py)
//!   target/docs-bundle/benchmarks-trajectory.json
//!     ↓ (via alint.org's sync-from-alint.mjs)
//!   alint.org/public/_alint/benchmarks-trajectory.json
//!     ↓ (read at build time by src/pages/benchmarks.astro)
//!   live trajectory table on alint.org/benchmarks/
//!
//! This test runs `render-history.py --json-out <tmp>` and asserts:
//! - The renderer exits cleanly.
//! - Output is valid JSON with `schema_version: 2` (the consolidated
//!   5-scenario series; v1 was the frozen 14-scenario series).
//! - The top row's version matches the highest semver-sorted dir
//!   under `docs/benchmarks/macro/results/linux-x86_64/`. If a new
//!   release dir lands but doesn't surface as the top row, the
//!   alint.org page would silently stay on the older version.
//! - The top row has a non-null `s4_1m_full` cell — S4 (workspace
//!   bundle) is the anchor scenario every release captures, so a
//!   missing value means bench-record.yml ran but didn't publish
//!   full data.
//!
//! Skipped (not failed) if `python3` isn't on PATH, since the
//! renderer is Python and not every contributor's local env has it.
//! CI uses ubuntu-latest which always ships python3.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Semver-ish sort: split on `.`, parse each component as a number,
/// non-numeric suffixes count as 0. Newest first. Mirrors the
/// `semver_key` helper in `xtask/scripts/render-history.py`.
fn semver_key(v: &str) -> Vec<u64> {
    v.trim_start_matches('v')
        .split('.')
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

#[test]
fn benchmarks_trajectory_renders_with_latest_version_on_top() {
    if Command::new("python3").arg("--version").output().is_err() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }

    let workspace = workspace_root();
    let script = workspace.join("xtask/scripts/render-history.py");
    assert!(
        script.is_file(),
        "renderer missing at {} — has the script moved?",
        script.display(),
    );

    let tmp = tempfile::Builder::new()
        .suffix(".json")
        .tempfile()
        .expect("tempfile");
    let json_out = tmp.path().to_path_buf();

    let out = Command::new("python3")
        .arg(&script)
        .arg("--json-out")
        .arg(&json_out)
        .current_dir(&workspace)
        .output()
        .expect("run render-history.py");
    assert!(
        out.status.success(),
        "render-history.py exited {:?}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
    );

    let body = std::fs::read_to_string(&json_out).expect("read trajectory JSON");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("parse trajectory JSON");

    assert_eq!(
        parsed["schema_version"], 2,
        "schema_version drift; consumers pin to v2 (consolidated 5-scenario series)",
    );

    let rows = parsed["rows"].as_array().expect("rows is an array");
    assert!(!rows.is_empty(), "trajectory has no rows");

    let top_version = rows[0]["version"]
        .as_str()
        .expect("top row has a version string")
        .to_string();

    // Highest version dir on disk = expected top row.
    let arch = parsed["arch"].as_str().expect("arch present");
    let results_dir = workspace.join("docs/benchmarks/macro/results").join(arch);
    let mut on_disk: Vec<String> = std::fs::read_dir(&results_dir)
        .unwrap_or_else(|e| panic!("read {}: {}", results_dir.display(), e))
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with('v'))
        .collect();
    on_disk.sort_by_key(|v| std::cmp::Reverse(semver_key(v)));

    assert!(
        !on_disk.is_empty(),
        "no version dirs under {}",
        results_dir.display()
    );

    assert_eq!(
        top_version, on_disk[0],
        "trajectory.json top row is {top_version} but the newest published bench dir is {} — \
         the alint.org /benchmarks/ page would render stale data. Either bench-record.yml \
         hasn't run yet for the newest release (wait for it to PR results in), or \
         render-history.py is mis-sorting versions.",
        on_disk[0],
    );

    // The S4 (workspace bundle) anchor scenario is captured by every
    // release. Missing data on the top row means bench-record.yml ran
    // but didn't publish full results — a partial publish that we want
    // to catch at CI time.
    assert!(
        !rows[0]["cells"]["s4_1m_full"].is_null(),
        "top row ({top_version}) has no s4_1m_full cell — partial bench publish?",
    );
}

#[test]
fn every_published_version_appears_in_trajectory() {
    if Command::new("python3").arg("--version").output().is_err() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }

    let workspace = workspace_root();
    let script = workspace.join("xtask/scripts/render-history.py");
    let tmp = tempfile::Builder::new()
        .suffix(".json")
        .tempfile()
        .expect("tempfile");
    let json_out = tmp.path().to_path_buf();

    let out = Command::new("python3")
        .arg(&script)
        .arg("--json-out")
        .arg(&json_out)
        .current_dir(&workspace)
        .output()
        .expect("run render-history.py");
    assert!(out.status.success(), "renderer failed");

    let body = std::fs::read_to_string(&json_out).expect("read trajectory JSON");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("parse JSON");
    let rendered: std::collections::BTreeSet<String> = parsed["rows"]
        .as_array()
        .expect("rows array")
        .iter()
        .map(|r| r["version"].as_str().unwrap().to_string())
        .collect();

    let arch = parsed["arch"].as_str().expect("arch");
    let results_dir = workspace.join("docs/benchmarks/macro/results").join(arch);
    let on_disk: std::collections::BTreeSet<String> = std::fs::read_dir(&results_dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with('v'))
        .collect();

    // Trajectory may legitimately include MANUAL fallbacks
    // (v0.5.6) without a results.json dir; rendered ⊇ on_disk.
    let missing: Vec<&String> = on_disk.difference(&rendered).collect();
    assert!(
        missing.is_empty(),
        "trajectory.json is missing rows for these published versions: {missing:?}",
    );
}
