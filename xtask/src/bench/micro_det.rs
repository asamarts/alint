//! `micro-det` — deterministic instruction counts for the criterion micro
//! suite, compared across git refs.
//!
//! Each ref's own, unmodified `alint-bench` criterion benches are compiled
//! against a counting stand-in for `criterion`
//! (`micro_det_criterion_shim.rs`, patched over crates.io's crate with
//! `--config patch.crates-io.criterion.path=…`). Every benchmark then runs
//! under Callgrind twice, at `K1` and `K2` iterations, collecting only inside
//! the stand-in's per-iteration `__inv_measure` frame and the rayon jobs it
//! hands to the (single, `RAYON_NUM_THREADS=1`) worker thread. The
//! difference divided by `K2 - K1` is the exact steady-state instruction
//! count of one iteration across every thread, with setup and first-call
//! warm-up excluded, and independent of host load.
//!
//! Wall-clock micro numbers from one release to the next cannot separate a
//! 10% step from run-to-run noise; this can (see
//! `docs/benchmarks/investigations/2026-10-v0.20-bench-increases/`). The gate
//! fails when a gated bench's instructions grow more than `--threshold`
//! percent over `--base`, or more than `--anchor-threshold` percent over
//! the long-horizon anchor release (`docs/benchmarks/ANCHOR`).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use anyhow::{Context, Result, bail};
use serde::Serialize;

const SHIM_LIB: &str = include_str!("micro_det_criterion_shim.rs");

/// Callgrind arguments: collect only the measured iterations and the rayon
/// jobs they dispatch (the worker thread runs `StackJob`/`HeapJob::execute`).
const CALLGRIND_TOGGLES: &[&str] = &[
    "--collect-atstart=no",
    "--toggle-collect=*__inv_measure*",
    "--toggle-collect=*StackJob*as rayon_core::job::Job*::execute*",
    "--toggle-collect=*HeapJob*as rayon_core::job::Job*::execute*",
];

/// Which benchmarks a run measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Suite {
    /// The curated per-PR subset: low-noise, in-memory, n >= 1000.
    Pr,
    /// Every criterion benchmark both refs define.
    Full,
}

#[derive(Debug, Clone, clap::Args)]
pub struct Args {
    /// Base ref (the previous release tag, or a PR's merge base).
    #[arg(long)]
    pub base: String,
    /// Head ref.
    #[arg(long, default_value = "HEAD")]
    pub head: String,
    /// Long-horizon anchor ref, or `auto` for `docs/benchmarks/ANCHOR`.
    #[arg(long)]
    pub anchor: Option<String>,
    /// Which benchmarks to measure.
    #[arg(long, value_enum, default_value_t = Suite::Pr)]
    pub suite: Suite,
    /// Fail when a gated bench grows more than this many percent over base.
    #[arg(long = "threshold", default_value_t = 3.0)]
    pub threshold_pct: f64,
    /// Fail when a gated bench grows more than this many percent over the anchor.
    #[arg(long = "anchor-threshold", default_value_t = 10.0)]
    pub anchor_threshold_pct: f64,
    /// Report regressions without failing.
    #[arg(long)]
    pub advisory: bool,
    /// Parallel valgrind processes (default: available parallelism).
    #[arg(long)]
    pub jobs: Option<usize>,
    /// Only benchmarks whose id contains this string.
    #[arg(long)]
    pub filter: Option<String>,
    /// Scratch directory (default: `target/micro-det`).
    #[arg(long)]
    pub workdir: Option<PathBuf>,
    /// Write the per-benchmark rows as JSON.
    #[arg(long)]
    pub out_json: Option<PathBuf>,
    /// Append the Markdown report here (e.g. `$GITHUB_STEP_SUMMARY`).
    #[arg(long)]
    pub summary: Option<PathBuf>,
    /// Keep the per-ref worktrees.
    #[arg(long)]
    pub keep: bool,
}

/// Per-benchmark measurement policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// A regression past the threshold fails the gate; otherwise report-only.
    pub gate: bool,
    /// In the per-PR subset.
    pub in_pr: bool,
    /// Second iteration count (the first is always 1).
    pub k2: u32,
}

/// Benchmarks whose per-iteration cost is tiny, so K must be large for the
/// instruction count to dominate Callgrind's fixed per-run accounting.
const TINY: &[&str] = &["regex_content/", "single_file/reeval_vs_full/run_for_file/"];
/// Benchmarks that build a fresh tree of real files for every iteration, so
/// one iteration's count varies with the tree's random directory name: at
/// six iterations the same build of `trim_trailing_whitespace/100` ranged
/// over 3.3% (1,570k to 1,622k), which put v0.22.0's record 3.16% over
/// v0.21.2 though the two differ by 0.09%. At 51 the spread is under 0.8%.
const FRESH_TREE: &[&str] = &["fix_throughput/trim_trailing_whitespace/"];
/// Benchmarks costing >100M instructions per iteration: one extra
/// iteration is plenty and keeps the full suite affordable.
const HEAVY: &[&str] = &[
    "glob_compile/1000",
    "glob_compile/100",
    "cross_file/unique_by/100000",
    "rule_engine/100000",
    "glob_match/100000",
    "single_file/pinned_references/10000",
    "output_formats/10000_violations/",
];
/// Report-only benches, by prefix (or exact id). Two kinds:
/// - wall-clock-bound or git-backed, where instructions are not the cost
///   (walker, blame cache, the dry-run fix);
/// - benches whose instruction count moved 1-18% between identical runs in
///   calibration (2026-10: two full runs of v0.19.0 and v0.20.0). They read
///   real files, and glibc's allocator takes history-dependent paths there.
///   The same-session wall-clock A/B in bench-record covers them.
///
/// On GitHub-hosted runners `xml_path_equals/1000` moved 1.8% and
/// `yaml_path_matches/1000` 2.75% between identical builds (dev box: 1.0%,
/// 0.4%): too close to the 3% gate to fail a release on. Everything else
/// moved under 1% everywhere and gates.
const REPORT_ONLY: &[&str] = &[
    "walker/",
    "blame_cache/",
    "fix_throughput/dry_run_only/",
    "single_file/file_content_forbidden/",
    "single_file/file_content_matches/",
    "single_file/file_header/",
    "single_file/file_is_text/",
    "single_file/file_starts_with/",
    "single_file/final_newline/",
    "single_file/no_trailing_whitespace/",
    "single_file/reeval_vs_full/full_run/",
    "structured_query/json_path_equals/",
    "structured_query/json_schema_passes/",
    "structured_query/toml_path_equals/",
    "structured_query/xml_path_equals/",
    "structured_query/xml_path_matches/100",
    "structured_query/yaml_path_matches/",
    "cross_file/every_matching_has/100",
    "cross_file/for_each_dir/5000",
];
/// The per-PR subset: gated, cheap, and covering each regression-prone
/// area (nested and paired rules, config loading, every output format, the
/// engine, globbing, structured queries, facts, the LSP re-evaluation path).
const PR_SUBSET: &[&str] = &[
    "cross_file/every_matching_has/1000",
    "cross_file/for_each_dir/1000",
    "cross_file/pair/1000",
    "cross_file/unique_by/1000",
    "dsl_extends/chain_depth/",
    "fix_throughput/trim_trailing_whitespace/1000",
    "glob_compile/10",
    "glob_match/1000",
    "literal_facts/20_absent/1000",
    "output_formats/1000_violations/",
    "rule_engine/1000",
    "single_file/file_hash/1000",
    "single_file/pinned_references/1000",
    "single_file/reeval_vs_full/run_for_file/1000",
    "structured_query/xml_path_matches/1000",
];

fn matches_any(id: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| {
        if p.ends_with('/') {
            id.starts_with(p)
        } else {
            id == *p
        }
    })
}

pub fn policy(id: &str) -> Policy {
    let k2 = if matches_any(id, TINY) {
        1001
    } else if matches_any(id, FRESH_TREE) {
        51
    } else if matches_any(id, HEAVY) {
        2
    } else {
        6
    };
    Policy {
        gate: !matches_any(id, REPORT_ONLY),
        in_pr: matches_any(id, PR_SUBSET),
        k2,
    }
}

// ─── refs, worktrees, builds ──────────────────────────────────────────

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .with_context(|| format!("git {args:?}"))?;
    if !out.status.success() {
        bail!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The locked version of `name` in a `Cargo.lock`.
pub fn locked_version(lock: &str, name: &str) -> Option<String> {
    let mut in_pkg = false;
    for line in lock.lines() {
        if line.starts_with("[[package]]") {
            in_pkg = false;
        } else if line == format!("name = \"{name}\"") {
            in_pkg = true;
        } else if in_pkg && let Some(v) = line.strip_prefix("version = \"") {
            return v.strip_suffix('"').map(str::to_string);
        }
    }
    None
}

/// The version named by `docs/benchmarks/ANCHOR` (`version = "vX.Y.Z"`).
pub fn anchor_version(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .find_map(|l| l.strip_prefix("version"))
        .and_then(|rest| rest.trim_start().strip_prefix('='))
        .map(|v| v.trim().trim_matches('"').to_string())
        .filter(|v| !v.is_empty())
}

struct Built {
    bins: BTreeMap<String, PathBuf>,
    /// Benchmark id -> bench target.
    ids: BTreeMap<String, String>,
}

fn write_shim(dir: &Path, criterion_version: &str) -> Result<()> {
    std::fs::create_dir_all(dir.join("src"))?;
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"criterion\"\nversion = \"{criterion_version}\"\n\
             edition = \"2021\"\npublish = false\n\n[lib]\npath = \"src/lib.rs\"\n\n\
             [workspace]\n"
        ),
    )?;
    std::fs::write(dir.join("src/lib.rs"), SHIM_LIB)?;
    Ok(())
}

fn criterion_benches(wt: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(wt.join("crates/alint-bench/benches"))? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "rs")
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            && !stem.starts_with("det_")
        {
            out.push(stem.to_string());
        }
    }
    out.sort();
    Ok(out)
}

fn build_ref(repo: &Path, workdir: &Path, target_dir: &Path, sha: &str) -> Result<Built> {
    let short = &sha[..12.min(sha.len())];
    let wt = workdir.join(format!("wt-{short}"));
    if wt.exists() {
        let _ = git(
            repo,
            &["worktree", "remove", "--force", &wt.to_string_lossy()],
        );
        let _ = std::fs::remove_dir_all(&wt);
    }
    git(
        repo,
        &["worktree", "add", "--detach", &wt.to_string_lossy(), sha],
    )?;
    let lock = std::fs::read_to_string(wt.join("Cargo.lock"))?;
    let version = locked_version(&lock, "criterion")
        .with_context(|| format!("{short}: no criterion in Cargo.lock"))?;
    let shim = workdir.join(format!("criterion-shim-{version}"));
    write_shim(&shim, &version)?;

    let benches = criterion_benches(&wt)?;
    let manifest = std::fs::read_to_string(wt.join("crates/alint-bench/Cargo.toml"))?;
    clean_workspace_packages(&wt, target_dir)?;
    let mut cmd = Command::new(cargo());
    cmd.current_dir(&wt)
        .env("CARGO_TARGET_DIR", target_dir)
        .arg("--config")
        .arg(format!(
            "patch.crates-io.criterion.path={:?}",
            shim.to_string_lossy()
        ))
        .args([
            "bench",
            "-p",
            "alint-bench",
            "--no-run",
            "--message-format=json",
        ]);
    if manifest.contains("\nfs-benches") {
        cmd.args(["--features", "fs-benches"]);
    }
    for b in &benches {
        cmd.args(["--bench", b]);
    }
    eprintln!(
        "[micro-det] building {short} ({} bench targets)",
        benches.len()
    );
    let out = cmd
        .stderr(std::process::Stdio::inherit())
        .output()
        .context("spawning cargo bench --no-run")?;
    if !out.status.success() {
        bail!("{short}: cargo bench --no-run failed");
    }
    let bin_dir = workdir.join(format!("bin-{short}"));
    std::fs::create_dir_all(&bin_dir)?;
    let mut bins = BTreeMap::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let is_bench = msg["target"]["kind"]
            .as_array()
            .is_some_and(|k| k.iter().any(|v| v == "bench"));
        if msg["reason"] == "compiler-artifact"
            && is_bench
            && let (Some(exe), Some(name)) =
                (msg["executable"].as_str(), msg["target"]["name"].as_str())
            && benches.iter().any(|b| b == name)
        {
            let dest = bin_dir.join(name);
            std::fs::copy(exe, &dest).with_context(|| format!("copying {exe}"))?;
            bins.insert(name.to_string(), dest);
        }
    }
    let ids = list_ids(&bins, &workdir.join("tmp"), short, &version)?;
    Ok(Built { bins, ids })
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

/// Remove the target dir's artifacts for the worktree's own workspace
/// packages, keeping the dependencies. Cargo identifies a workspace package by
/// name, version and its path *relative to the workspace*, so two worktrees at
/// the same version (any PR and its base) share these artifacts, and cargo
/// judges their freshness by stat-ing the sources of whichever worktree built
/// them last: without this, the second ref silently reuses the first ref's
/// binaries and every comparison reports no change.
fn clean_workspace_packages(wt: &Path, target_dir: &Path) -> Result<()> {
    let meta = Command::new(cargo())
        .current_dir(wt)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .context("cargo metadata")?;
    if !meta.status.success() {
        bail!("cargo metadata failed in {}", wt.display());
    }
    let meta: serde_json::Value = serde_json::from_slice(&meta.stdout)?;
    let mut clean = Command::new(cargo());
    clean
        .current_dir(wt)
        .env("CARGO_TARGET_DIR", target_dir)
        .args(["clean", "--quiet"]);
    for pkg in meta["packages"].as_array().into_iter().flatten() {
        if let Some(name) = pkg["name"].as_str() {
            clean.args(["-p", name]);
        }
    }
    if !clean.status().context("cargo clean")?.success() {
        bail!("cargo clean of the workspace packages failed");
    }
    Ok(())
}

/// Every benchmark id each bench binary defines (`INV_LIST=1`), mapped to
/// its bench target. An empty listing means the stand-in was not patched in
/// (real criterion ignores `INV_LIST`) or the bench failed during setup.
fn list_ids(
    bins: &BTreeMap<String, PathBuf>,
    tmp: &Path,
    short: &str,
    criterion_version: &str,
) -> Result<BTreeMap<String, String>> {
    std::fs::create_dir_all(tmp)?;
    let mut ids = BTreeMap::new();
    for (name, bin) in bins {
        let listed = Command::new(bin)
            .env("INV_LIST", "1")
            .env("TMPDIR", tmp)
            .output()
            .with_context(|| format!("listing {name}"))?;
        let stderr = String::from_utf8_lossy(&listed.stderr);
        let found: Vec<&str> = stderr
            .lines()
            .filter_map(|l| l.strip_prefix("INV-ID\t"))
            .collect();
        if found.is_empty() {
            bail!(
                "{short}: bench `{name}` listed no benchmarks (status {}); either the criterion \
                 stand-in was not patched in (is the workspace criterion requirement still \
                 satisfied by {criterion_version}?) or the bench failed during setup: {}",
                listed.status,
                stderr.lines().rev().take(5).collect::<Vec<_>>().join(" | ")
            );
        }
        for id in found {
            ids.insert(id.to_string(), name.clone());
        }
    }
    Ok(ids)
}

// ─── measurement ──────────────────────────────────────────────────────

/// Total `Ir` of a Callgrind output file.
pub fn callgrind_total_ir(text: &str) -> Option<u64> {
    let mut events: Vec<&str> = Vec::new();
    let mut total = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("events:") {
            events = rest.split_whitespace().collect();
        } else if let Some(rest) = line
            .strip_prefix("totals:")
            .or_else(|| line.strip_prefix("summary:"))
        {
            let idx = events.iter().position(|e| *e == "Ir").unwrap_or(0);
            total = rest
                .split_whitespace()
                .nth(idx)
                .and_then(|v| v.parse().ok());
        }
    }
    total
}

fn callgrind_run(bin: &Path, id: &str, k: u32, out: &Path, tmp: &Path) -> Result<u64> {
    let output = Command::new("valgrind")
        .arg("--tool=callgrind")
        .args(CALLGRIND_TOGGLES)
        .arg("--dump-instr=no")
        .arg(format!("--callgrind-out-file={}", out.display()))
        .arg(bin)
        .env("INV_FILTER", id)
        .env("INV_K", k.to_string())
        .env("RAYON_NUM_THREADS", "1")
        .env("TMPDIR", tmp)
        .output()
        .context("spawning valgrind (is it installed?)")?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.contains(&format!("INV matched {id} ")) {
        bail!(
            "{id}: benchmark did not run under valgrind (status {}): {}",
            output.status,
            stderr.lines().rev().take(5).collect::<Vec<_>>().join(" | ")
        );
    }
    let text = std::fs::read_to_string(out)?;
    let _ = std::fs::remove_file(out);
    callgrind_total_ir(&text).with_context(|| format!("{id}: no Ir total in {}", out.display()))
}

/// Exact steady-state instructions per iteration.
fn measure(bin: &Path, id: &str, k2: u32, scratch: &Path) -> Result<f64> {
    let tmp = scratch.join("tmp");
    std::fs::create_dir_all(&tmp)?;
    let stem: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let a = callgrind_run(bin, id, 1, &scratch.join(format!("{stem}.k1.out")), &tmp)?;
    let b = callgrind_run(bin, id, k2, &scratch.join(format!("{stem}.k2.out")), &tmp)?;
    #[allow(clippy::cast_precision_loss)]
    Ok((b as f64 - a as f64) / f64::from(k2 - 1))
}

// ─── comparison ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub id: String,
    pub base: Option<f64>,
    pub head: Option<f64>,
    pub anchor: Option<f64>,
    pub delta_pct: Option<f64>,
    pub anchor_delta_pct: Option<f64>,
    pub gated: bool,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Ok,
    Regression,
    AnchorRegression,
    ReportOnly,
    New,
    Removed,
}

fn pct(new: f64, old: f64) -> Option<f64> {
    (old > 0.0).then(|| (new / old - 1.0) * 100.0)
}

/// Compare measured counts. Pure, so the policy is unit-testable.
pub fn compare(
    base: &BTreeMap<String, f64>,
    head: &BTreeMap<String, f64>,
    anchor: Option<&BTreeMap<String, f64>>,
    threshold_pct: f64,
    anchor_threshold_pct: f64,
) -> Vec<Row> {
    let ids: BTreeSet<&String> = base.keys().chain(head.keys()).collect();
    let mut rows = Vec::new();
    for id in ids {
        let (b, h) = (base.get(id).copied(), head.get(id).copied());
        let a = anchor.and_then(|m| m.get(id).copied());
        let gated = policy(id).gate;
        let delta_pct = b.zip(h).and_then(|(b, h)| pct(h, b));
        let anchor_delta_pct = a.zip(h).and_then(|(a, h)| pct(h, a));
        let verdict = match (b, h) {
            (None, Some(_)) => Verdict::New,
            (Some(_), None) => Verdict::Removed,
            _ if !gated => Verdict::ReportOnly,
            _ if delta_pct.is_some_and(|d| d > threshold_pct) => Verdict::Regression,
            _ if anchor_delta_pct.is_some_and(|d| d > anchor_threshold_pct) => {
                Verdict::AnchorRegression
            }
            _ => Verdict::Ok,
        };
        rows.push(Row {
            id: id.clone(),
            base: b,
            head: h,
            anchor: a,
            delta_pct,
            anchor_delta_pct,
            gated,
            verdict,
        });
    }
    rows.sort_by(|x, y| {
        y.delta_pct
            .unwrap_or(f64::NEG_INFINITY)
            .partial_cmp(&x.delta_pct.unwrap_or(f64::NEG_INFINITY))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    rows
}

fn fmt_ir(v: Option<f64>) -> String {
    v.map_or_else(|| "-".to_string(), |v| format!("{v:.0}"))
}

fn fmt_pct(v: Option<f64>) -> String {
    v.map_or_else(String::new, |v| format!("{v:+.2}%"))
}

/// Markdown report. Rows within ±0.5% that pass are summarized as a count.
pub fn markdown(rows: &[Row], header: &str, anchor_label: Option<&str>) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "{header}\n");
    let anchor_col = anchor_label.map(|a| format!(" vs anchor {a} |"));
    let _ = writeln!(
        s,
        "| Bench | Base | Head | Delta |{} Verdict |",
        anchor_col.as_deref().unwrap_or("")
    );
    let _ = writeln!(
        s,
        "|---|---:|---:|---:|{}---|",
        if anchor_label.is_some() { "---:|" } else { "" }
    );
    let mut quiet = 0usize;
    for r in rows {
        let small = r.delta_pct.is_some_and(|d| d.abs() < 0.5)
            && r.anchor_delta_pct.is_none_or(|d| d.abs() < 0.5);
        if small && r.verdict == Verdict::Ok {
            quiet += 1;
            continue;
        }
        let verdict = match r.verdict {
            Verdict::Ok => "ok",
            Verdict::Regression => "**REGRESSION**",
            Verdict::AnchorRegression => "**over anchor**",
            Verdict::ReportOnly => "report-only",
            Verdict::New => "new",
            Verdict::Removed => "removed",
        };
        let anchor = if anchor_label.is_some() {
            format!(" {} |", fmt_pct(r.anchor_delta_pct))
        } else {
            String::new()
        };
        let _ = writeln!(
            s,
            "| `{}` | {} | {} | {} |{anchor} {verdict} |",
            r.id,
            fmt_ir(r.base),
            fmt_ir(r.head),
            fmt_pct(r.delta_pct)
        );
    }
    if quiet > 0 {
        let _ = writeln!(s, "\n{quiet} more benchmark(s) within ±0.5%.");
    }
    s
}

/// The scratch directory (and so the `TMPDIR`) of the `n`th measurement.
/// Every name has the same length: a bench's base and head run at different
/// queue positions, and the length of the path its files sit under moved
/// `trim_trailing_whitespace/100` by up to 0.9% at 51 iterations (`job-7`,
/// `job-77`, `job-177`), where equal-length names agree within 0.3%.
fn job_dir(scratch: &Path, n: usize) -> PathBuf {
    scratch.join(format!("job-{n:05}"))
}

// ─── entry point ──────────────────────────────────────────────────────

fn measure_all(
    work: &[(String, PathBuf, String, u32)],
    jobs: usize,
    scratch: &Path,
) -> Result<BTreeMap<(String, String), f64>> {
    let queue = Mutex::new(work.iter().enumerate().collect::<Vec<_>>());
    let results = Mutex::new(BTreeMap::new());
    let errors = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..jobs {
            s.spawn(|| {
                loop {
                    let Some((n, (rev, bin, id, k2))) = queue.lock().unwrap().pop() else {
                        break;
                    };
                    let dir = job_dir(scratch, n);
                    let _ = std::fs::create_dir_all(&dir);
                    match measure(bin, id, *k2, &dir) {
                        Ok(v) => {
                            results.lock().unwrap().insert((rev.clone(), id.clone()), v);
                        }
                        Err(e) => errors.lock().unwrap().push(format!("{rev} {id}: {e:#}")),
                    }
                    let _ = std::fs::remove_dir_all(&dir);
                }
            });
        }
    });
    let errors = errors.into_inner().unwrap();
    if !errors.is_empty() {
        bail!(
            "{} measurement(s) failed:\n{}",
            errors.len(),
            errors.join("\n")
        );
    }
    Ok(results.into_inner().unwrap())
}

pub fn run(args: &Args) -> Result<()> {
    let repo = crate::workspace_root_from_xtask()?;
    let workdir = args
        .workdir
        .clone()
        .unwrap_or_else(|| repo.join("target/micro-det"));
    std::fs::create_dir_all(&workdir)?;
    let workdir = workdir.canonicalize()?;

    let anchor_ref = match args.anchor.as_deref() {
        None => None,
        Some("auto") => {
            let text = std::fs::read_to_string(repo.join("docs/benchmarks/ANCHOR"))
                .context("reading docs/benchmarks/ANCHOR")?;
            Some(anchor_version(&text).context("docs/benchmarks/ANCHOR names no version")?)
        }
        Some(r) => Some(r.to_string()),
    };
    let rev = |r: &str| git(&repo, &["rev-parse", &format!("{r}^{{commit}}")]);
    let base = rev(&args.base)?;
    let head = rev(&args.head)?;
    let anchor = anchor_ref.as_deref().map(rev).transpose()?;
    let mut shas = vec![head.clone()];
    for sha in [Some(&base), anchor.as_ref()].into_iter().flatten() {
        if !shas.contains(sha) {
            shas.push(sha.clone());
        }
    }

    let result = measure_refs(args, &repo, &workdir, &shas);
    if !args.keep {
        for sha in &shas {
            let wt = workdir.join(format!("wt-{}", &sha[..12]));
            let _ = git(
                &repo,
                &["worktree", "remove", "--force", &wt.to_string_lossy()],
            );
        }
    }
    let measured = result?;
    let of = |sha: &str| -> BTreeMap<String, f64> {
        measured
            .iter()
            .filter(|((r, _), _)| r == sha)
            .map(|((_, id), v)| (id.clone(), *v))
            .collect()
    };
    let anchor_counts = anchor.as_deref().map(of);
    let rows = compare(
        &of(&base),
        &of(&head),
        anchor_counts.as_ref(),
        args.threshold_pct,
        args.anchor_threshold_pct,
    );
    let failures = report(args, &rows, &head, &base, anchor_ref.as_deref())?;
    if failures > 0 && !args.advisory {
        bail!("micro-det: {failures} gated regression(s)");
    }
    Ok(())
}

/// Build every ref and measure the selected benchmarks of each.
fn measure_refs(
    args: &Args,
    repo: &Path,
    workdir: &Path,
    shas: &[String],
) -> Result<BTreeMap<(String, String), f64>> {
    // A dedicated target dir by default: each build cleans the workspace's own
    // packages there (see `clean_workspace_packages`), which must not touch the
    // checkout's own build. CI points CARGO_TARGET_DIR at its cached target.
    let target_dir =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| workdir.join("target"), PathBuf::from);
    let mut work = Vec::new();
    for sha in shas {
        let built = build_ref(repo, workdir, &target_dir, sha)?;
        for (id, bench) in &built.ids {
            if (args.suite == Suite::Full || policy(id).in_pr)
                && args.filter.as_deref().is_none_or(|f| id.contains(f))
            {
                work.push((
                    sha.clone(),
                    built.bins[bench].clone(),
                    id.clone(),
                    policy(id).k2,
                ));
            }
        }
    }
    // `measure_all` pops from the end: put the heavy (k2 = 2) benches last so
    // they start first and the pool drains evenly.
    work.sort_by_key(|w| std::cmp::Reverse(w.3));
    let jobs = args.jobs.unwrap_or_else(|| {
        std::thread::available_parallelism().map_or(2, std::num::NonZeroUsize::get)
    });
    eprintln!(
        "[micro-det] measuring {} (ref, bench) pairs with {jobs} jobs",
        work.len()
    );
    measure_all(&work, jobs, &workdir.join("scratch"))
}

/// Print the report, append it to `--summary` and write `--out-json`.
/// Returns the number of gated regressions.
fn report(
    args: &Args,
    rows: &[Row],
    head: &str,
    base: &str,
    anchor_ref: Option<&str>,
) -> Result<usize> {
    let failures = rows
        .iter()
        .filter(|r| matches!(r.verdict, Verdict::Regression | Verdict::AnchorRegression))
        .count();
    let status = match (failures, args.advisory) {
        (0, _) => "PASS".to_string(),
        (n, true) => format!("{n} regression(s) (advisory, not failing)"),
        (n, false) => format!("FAIL ({n} regression(s))"),
    };
    let header = format!(
        "### Deterministic micro benches: {status}\n\nInstructions per iteration \
         (Callgrind, all threads, setup excluded), head `{}` vs base `{}`. Gate: \
         +{}% per gated bench{}.",
        &head[..12],
        &base[..12],
        args.threshold_pct,
        anchor_ref
            .map(|a| format!(", +{}% vs anchor {a}", args.anchor_threshold_pct))
            .unwrap_or_default()
    );
    let md = markdown(rows, &header, anchor_ref);
    println!("{md}");
    if let Some(p) = &args.summary {
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)?;
        writeln!(f, "{md}")?;
    }
    if let Some(p) = &args.out_json {
        std::fs::write(p, serde_json::to_string_pretty(rows)?)?;
    }
    Ok(failures)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_classifies_benches() {
        assert_eq!(
            policy("cross_file/pair/1000"),
            Policy {
                gate: true,
                in_pr: true,
                k2: 6
            }
        );
        assert!(!policy("cross_file/pair/100").in_pr);
        assert!(!policy("walker/1000").gate);
        assert!(
            !policy("single_file/final_newline/1000").gate,
            "allocator-noisy"
        );
        assert!(!policy("structured_query/xml_path_equals/100").gate);
        assert!(
            !policy("structured_query/xml_path_equals/1000").gate,
            "1.8% CI noise"
        );
        assert!(policy("structured_query/xml_path_matches/1000").gate);
        assert!(
            !policy("structured_query/yaml_path_matches/1000").gate,
            "2.75% CI noise"
        );
        // Every PR-subset bench gates: an advisory subset of report-only
        // benches would be noise.
        for prefix in PR_SUBSET {
            assert!(
                !matches_any(prefix.trim_end_matches('/'), REPORT_ONLY),
                "{prefix}"
            );
        }
        assert_eq!(policy("glob_compile/1000").k2, 2);
        assert!(
            !policy("glob_compile/1000").in_pr,
            "exact match, not prefix"
        );
        assert_eq!(policy("regex_content/1024").k2, 1001);
        assert_eq!(policy("fix_throughput/trim_trailing_whitespace/100").k2, 51);
        assert_eq!(
            policy("fix_throughput/trim_trailing_whitespace/1000").k2,
            51
        );
        assert_eq!(policy("fix_throughput/dry_run_only/100").k2, 6);
        assert!(policy("output_formats/1000_violations/sarif").in_pr);
        assert_eq!(policy("output_formats/10000_violations/sarif").k2, 2);
    }

    #[test]
    fn every_measurement_gets_a_scratch_path_of_one_length() {
        let scratch = Path::new("/scratch");
        let lengths: BTreeSet<usize> = [0, 7, 77, 177, 293, 99_999]
            .into_iter()
            .map(|n| job_dir(scratch, n).as_os_str().len())
            .collect();
        assert_eq!(lengths.len(), 1, "{lengths:?}");
        assert_ne!(job_dir(scratch, 7), job_dir(scratch, 77));
    }

    #[test]
    fn locked_version_reads_the_named_package() {
        let lock = "[[package]]\nname = \"cast\"\nversion = \"0.3.0\"\n\n\
                    [[package]]\nname = \"criterion\"\nversion = \"0.8.2\"\n";
        assert_eq!(locked_version(lock, "criterion").as_deref(), Some("0.8.2"));
        assert_eq!(locked_version(lock, "serde"), None);
    }

    #[test]
    fn anchor_version_parses_the_file() {
        let text = "# comment\nversion = \"v0.20.0\"\nreason = \"x\"\n";
        assert_eq!(anchor_version(text).as_deref(), Some("v0.20.0"));
        assert_eq!(anchor_version("reason = \"x\"\n"), None);
    }

    #[test]
    fn callgrind_totals_pick_the_ir_column() {
        let text = "events: Ir Dr\nfn=main\n1 5 2\ntotals: 1234 99\n";
        assert_eq!(callgrind_total_ir(text), Some(1234));
        let summary = "events: Ir\nsummary: 77\n";
        assert_eq!(callgrind_total_ir(summary), Some(77));
    }

    fn map(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
        pairs.iter().map(|(k, v)| ((*k).to_string(), *v)).collect()
    }

    #[test]
    fn compare_gates_regressions_and_anchor_drift() {
        let base = map(&[
            ("cross_file/pair/1000", 100.0),
            ("rule_engine/1000", 100.0),
            ("walker/1000", 100.0),
            ("dsl_extends/chain_depth/1", 100.0),
        ]);
        let head = map(&[
            ("cross_file/pair/1000", 104.0),
            ("rule_engine/1000", 102.0),
            ("walker/1000", 150.0),
            ("dsl_extends/chain_depth/1", 102.0),
            ("literal_facts/20_absent/1000", 5.0),
        ]);
        let anchor = map(&[
            ("rule_engine/1000", 90.0),
            ("dsl_extends/chain_depth/1", 99.0),
        ]);
        let rows = compare(&base, &head, Some(&anchor), 3.0, 10.0);
        let v = |id: &str| rows.iter().find(|r| r.id == id).unwrap().verdict;
        assert_eq!(v("cross_file/pair/1000"), Verdict::Regression);
        assert_eq!(v("rule_engine/1000"), Verdict::AnchorRegression);
        assert_eq!(v("walker/1000"), Verdict::ReportOnly);
        assert_eq!(v("dsl_extends/chain_depth/1"), Verdict::Ok);
        assert_eq!(v("literal_facts/20_absent/1000"), Verdict::New);
    }

    #[test]
    fn markdown_collapses_quiet_rows() {
        let base = map(&[("a/1", 100.0), ("cross_file/pair/1000", 100.0)]);
        let head = map(&[("a/1", 100.1), ("cross_file/pair/1000", 110.0)]);
        let rows = compare(&base, &head, None, 3.0, 10.0);
        let md = markdown(&rows, "### t", None);
        assert!(md.contains("`cross_file/pair/1000`"));
        assert!(md.contains("**REGRESSION**"));
        assert!(md.contains("1 more benchmark(s) within ±0.5%."));
    }
}
