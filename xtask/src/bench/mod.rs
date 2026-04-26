//! `bench-scale` — the v0.5 scale-ceiling benchmark.
//!
//! Two orthogonal dimensions:
//!
//! - **size**: 1k / 10k / 100k / 1m files. The 1m size is opt-in
//!   via `--include-1m` because it generates ~3-5 GB of synthetic
//!   data and runs in minutes, not seconds.
//! - **mode**: `full` (every file evaluated) and `changed` (a
//!   deterministic subset modified post-commit, then `alint check
//!   --changed` measures the v0.5.0 incremental path).
//!
//! Each (size, mode, scenario) triple becomes one hyperfine row.
//! Scenarios live in `scenarios/*.yml` — three configs spanning
//! filename hygiene (S1), existence + content (S2), and the
//! full workspace bundle (S3).
//!
//! Output: a per-platform directory under
//! `docs/benchmarks/v0.5/scale/<os>-<arch>/` containing a
//! `results.json` (machine-readable) plus per-size `results.md`
//! files and an `index.md` summary. Cross-machine comparisons
//! always require like-for-like (same fingerprint) — see
//! `docs/benchmarks/METHODOLOGY.md`.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

mod fingerprint;
pub mod tools;

pub use tools::Tool;

/// Embedded scenario YAMLs. Each ships in the xtask binary so
/// running on any cloned checkout produces byte-identical
/// configs without depending on workspace-relative path resolution.
const SCENARIO_S1: &str = include_str!("scenarios/s1_filename.yml");
const SCENARIO_S2: &str = include_str!("scenarios/s2_existence_content.yml");
const SCENARIO_S3: &str = include_str!("scenarios/s3_workspace.yml");

/// Parameters parsed from CLI flags. Defaults pick the
/// "publish-grade run" — full size matrix (excluding 1m), all
/// scenarios, both modes — so a bare `xtask bench-scale`
/// produces a committable result.
#[derive(Debug, Clone)]
pub struct ScaleArgs {
    pub sizes: Vec<Size>,
    pub scenarios: Vec<Scenario>,
    pub modes: Vec<Mode>,
    pub tools: Vec<Tool>,
    pub warmup: u32,
    pub runs: u32,
    pub seed: u64,
    pub diff_pct: f64,
    pub out: Option<PathBuf>,
    pub quick: bool,
    pub json_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Size {
    /// 1,000 files — small repo / smoke test.
    K1,
    /// 10,000 files — small-to-mid monorepo.
    K10,
    /// 100,000 files — workspace-tier upper bound.
    K100,
    /// 1,000,000 files — Bazel territory; opt-in.
    M1,
}

impl Size {
    /// Parse the `--sizes` flag's comma-separated values.
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "1k" => Ok(Self::K1),
            "10k" => Ok(Self::K10),
            "100k" => Ok(Self::K100),
            "1m" => Ok(Self::M1),
            other => bail!("unknown size {other:?}; expected one of 1k, 10k, 100k, 1m"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::K1 => "1k",
            Self::K10 => "10k",
            Self::K100 => "100k",
            Self::M1 => "1m",
        }
    }

    pub fn file_count(self) -> usize {
        match self {
            Self::K1 => 1_000,
            Self::K10 => 10_000,
            Self::K100 => 100_000,
            Self::M1 => 1_000_000,
        }
    }

    /// `(packages, files_per_package)` for the monorepo
    /// generator that hits this size's file count exactly.
    /// Each package contributes `2 + files_per_package` files
    /// (Cargo.toml + README + N source files); plus the
    /// workspace root Cargo.toml. Tunes the package count to
    /// keep `files_per_package` in a reasonable range
    /// (10-100), so per-package work matches realistic
    /// monorepos.
    pub fn monorepo_shape(self) -> (usize, usize) {
        match self {
            Self::K1 => (50, 18),     // 50 * 20 + 1 = 1001
            Self::K10 => (200, 48),   // 200 * 50 + 1 = 10001
            Self::K100 => (1000, 98), // 1000 * 100 + 1 = 100001
            Self::M1 => (5000, 198),  // 5000 * 200 + 1 = 1000001
        }
    }

    pub fn is_opt_in(self) -> bool {
        matches!(self, Self::M1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scenario {
    S1,
    S2,
    S3,
}

impl Scenario {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_uppercase().as_str() {
            "S1" => Ok(Self::S1),
            "S2" => Ok(Self::S2),
            "S3" => Ok(Self::S3),
            other => bail!("unknown scenario {other:?}; expected S1, S2, or S3"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::S1 => "S1",
            Self::S2 => "S2",
            Self::S3 => "S3",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::S1 => "Filename hygiene (8 rules)",
            Self::S2 => "Existence + content (8 rules)",
            Self::S3 => "Workspace bundle (oss-baseline + rust + monorepo + cargo-workspace)",
        }
    }

    pub fn config_yaml(self) -> &'static str {
        match self {
            Self::S1 => SCENARIO_S1,
            Self::S2 => SCENARIO_S2,
            Self::S3 => SCENARIO_S3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Full,
    Changed,
}

impl Mode {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "full" => Ok(Self::Full),
            "changed" => Ok(Self::Changed),
            other => bail!("unknown mode {other:?}; expected `full` or `changed`"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Changed => "changed",
        }
    }
}

/// One hyperfine row in the report. Times are in milliseconds
/// (hyperfine reports seconds; we convert at parse time so
/// the output schema stays fixed at "ms").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    /// Tool name (`alint`, `ls-lint`, …). Identifies which
    /// implementation produced this row.
    pub tool: String,
    pub size_files: usize,
    pub size_label: String,
    pub scenario: String,
    pub mode: String,
    pub mean_ms: f64,
    pub stddev_ms: f64,
    pub median_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub samples: usize,
    pub command: String,
}

/// Top-level result document — one per `bench-scale`
/// invocation. Serialised to `results.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub fingerprint: fingerprint::Fingerprint,
    pub args: ReportArgs,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportArgs {
    pub seed: String,
    pub diff_pct: f64,
    pub warmup: u32,
    pub runs: u32,
    pub sizes: Vec<String>,
    pub scenarios: Vec<String>,
    pub modes: Vec<String>,
    pub tools: Vec<String>,
}

// ─── Entry point ─────────────────────────────────────────────────────

/// Top-level entry called from `main.rs`. Builds the alint
/// binary, materialises trees, drives hyperfine, and writes
/// the report.
pub fn bench_scale(mut args: ScaleArgs) -> Result<()> {
    if args.quick {
        // `--quick` collapses the matrix to a smoke test.
        // Useful for "did the harness break?" CI gates.
        args.sizes = vec![Size::K1];
        args.scenarios = vec![Scenario::S1];
        args.modes = vec![Mode::Full];
        args.tools = vec![Tool::Alint];
        args.warmup = 1;
        args.runs = 3;
    }

    ensure_hyperfine()?;
    let alint_bin = build_release_binary()?;
    let fingerprint = fingerprint::capture(&args.tools);

    eprintln!(
        "[xtask] bench-scale: tools={} sizes={} scenarios={} modes={} warmup={} runs={} seed={:#x}",
        join_labels(&args.tools, Tool::name),
        join_labels(&args.sizes, Size::label),
        join_labels(&args.scenarios, Scenario::label),
        join_labels(&args.modes, Mode::label),
        args.warmup,
        args.runs,
        args.seed,
    );

    let mut rows: Vec<Row> = Vec::new();
    for &size in &args.sizes {
        eprintln!(
            "[xtask] generating monorepo tree of {} files (seed={:#x})...",
            size.file_count(),
            args.seed,
        );
        let (pkgs, fpp) = size.monorepo_shape();
        let tree = alint_bench::tree::generate_monorepo(pkgs, fpp, args.seed)
            .with_context(|| format!("generating {} tree", size.label()))?;
        let tree_root = tree.root().to_path_buf();

        // Initialise git so `--changed` mode has something to
        // diff against. Done once per tree — hyperfine then
        // measures the same disk state across runs. Skipped
        // when no tool requested `Mode::Changed` to save time.
        let needs_git = args.modes.contains(&Mode::Changed)
            && args
                .tools
                .iter()
                .any(|t| args.scenarios.iter().any(|s| t.supports(*s, Mode::Changed)));
        if needs_git {
            init_git_for_changed_mode(&tree_root)?;
            let to_touch = alint_bench::tree::select_subset(
                &tree.files,
                args.diff_pct / 100.0,
                args.seed ^ 0xD1FF,
            );
            eprintln!(
                "[xtask] touching {} of {} files for --changed diff ({}%)",
                to_touch.len(),
                tree.files.len(),
                args.diff_pct,
            );
            touch_subset(&tree_root, &to_touch)?;
        }

        for &scenario in &args.scenarios {
            for &tool in &args.tools {
                // Tool decides whether to write a config; ls-lint's
                // `.ls-lint.yml` and alint's `.alint.yml` coexist
                // since they're keyed on different filenames.
                tool.setup_config(&tree_root, scenario)?;
                for &mode in &args.modes {
                    if !tool.supports(scenario, mode) {
                        continue;
                    }
                    eprintln!(
                        "[xtask] hyperfine {}/{}/{}/{} ...",
                        tool.name(),
                        size.label(),
                        scenario.label(),
                        mode.label(),
                    );
                    let row = run_one(&alint_bin, &tree_root, tool, size, scenario, mode, &args)?;
                    rows.push(row);
                }
            }
        }
    }

    let report = Report {
        schema_version: 1,
        fingerprint,
        args: ReportArgs {
            seed: format!("{:#x}", args.seed),
            diff_pct: args.diff_pct,
            warmup: args.warmup,
            runs: args.runs,
            sizes: args.sizes.iter().map(|s| s.label().to_string()).collect(),
            scenarios: args
                .scenarios
                .iter()
                .map(|s| s.label().to_string())
                .collect(),
            modes: args.modes.iter().map(|m| m.label().to_string()).collect(),
            tools: args.tools.iter().map(|t| t.name().to_string()).collect(),
        },
        rows,
    };

    write_outputs(&report, &args)
}

fn join_labels<T: Copy, F: Fn(T) -> &'static str>(items: &[T], f: F) -> String {
    items.iter().map(|&t| f(t)).collect::<Vec<_>>().join(",")
}

// ─── Hyperfine driver ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct HfOutput {
    results: Vec<HfResult>,
}

#[derive(Debug, Deserialize)]
struct HfResult {
    command: String,
    mean: f64,
    /// Hyperfine reports `null` for stddev when only one
    /// measured run was made (no variance to compute). The
    /// 1M-size auto-reduction can hit `runs=1` legitimately;
    /// surface it as 0.0 in our schema rather than failing
    /// the whole bench.
    #[serde(default)]
    stddev: Option<f64>,
    median: f64,
    min: f64,
    max: f64,
    times: Vec<f64>,
}

#[allow(clippy::too_many_arguments)]
fn run_one(
    alint: &Path,
    tree_root: &Path,
    tool: Tool,
    size: Size,
    scenario: Scenario,
    mode: Mode,
    args: &ScaleArgs,
) -> Result<Row> {
    // Tool returns the full shell command line. Hyperfine
    // spawns commands via `sh -c`, so pipes / semicolons /
    // globs in `GrepPipeline`'s output work as written;
    // single-program tools like alint and ls-lint reduce to a
    // simple `bin args...` string.
    let cmd_str = tool.invocation(alint, tree_root, scenario, mode);
    let label = format!(
        "{tool} ({size}/{scen}/{mode_label})",
        tool = tool.name(),
        size = size.label(),
        scen = scenario.label(),
        mode_label = mode.label(),
    );

    let json_file = tempfile::NamedTempFile::new()?;
    let json_path = json_file.path().to_path_buf();

    // Auto-reduce sampling at the 1M size: at the upper bound a
    // single S3 invocation can run for minutes, and 13 runs
    // (3 warmup + 10 measured) per row would push the full
    // matrix to several hours. Cap warmup at 1 and runs at 3
    // — the resulting stddev is wider but the means stay
    // representative, and the bench finishes in a sitting.
    // Document this in methodology.md so readers don't compare
    // 1M's stddev to the smaller-size rows like-for-like.
    let (warmup, runs) = if size == Size::M1 {
        (args.warmup.min(1), args.runs.min(3))
    } else {
        (args.warmup, args.runs)
    };

    let status = Command::new("hyperfine")
        .args(["--warmup", &warmup.to_string()])
        .args(["--min-runs", &runs.to_string()])
        .args(["--max-runs", &runs.to_string()])
        // alint exits 1 when rules fire — that's fine for the
        // bench, we measure wall-time regardless of verdict.
        // Synthetic trees don't satisfy `oss-baseline@v1`'s
        // README/LICENSE rules etc., and the cost of finding
        // those violations is exactly what we want to measure.
        .arg("--ignore-failure")
        .arg("--command-name")
        .arg(&label)
        .arg("--export-json")
        .arg(&json_path)
        .arg(&cmd_str)
        .status()
        .context("invoking hyperfine")?;
    if !status.success() {
        bail!("hyperfine exited non-zero for {label}");
    }

    let raw = fs::read_to_string(&json_path)?;
    let parsed: HfOutput =
        serde_json::from_str(&raw).context("parsing hyperfine --export-json output")?;
    let r = parsed
        .results
        .into_iter()
        .next()
        .context("hyperfine produced no results")?;

    Ok(Row {
        tool: tool.name().into(),
        size_files: size.file_count(),
        size_label: size.label().into(),
        scenario: scenario.label().into(),
        mode: mode.label().into(),
        mean_ms: r.mean * 1000.0,
        stddev_ms: r.stddev.unwrap_or(0.0) * 1000.0,
        median_ms: r.median * 1000.0,
        min_ms: r.min * 1000.0,
        max_ms: r.max * 1000.0,
        samples: r.times.len(),
        command: r.command,
    })
}

// ─── --changed-mode setup ────────────────────────────────────────────

/// Initialise a git repo in the tree, add all files, commit.
/// Done once per (size) tree before any `Mode::Changed` row
/// runs; hyperfine then runs many times against the same
/// committed-then-modified state.
///
/// Git's auto-gc threshold (~7000 loose objects by default)
/// fires on the initial 10k+ commit, which would repack the
/// objects directory mid-bench-run. Disabling `gc.auto`
/// per-repo prevents that — alint's walker also excludes
/// `.git/` so the race is doubly impossible, but the
/// belt-and-suspenders is cheap.
fn init_git_for_changed_mode(root: &Path) -> Result<()> {
    git(root, &["init", "-q", "-b", "main"])?;
    git(root, &["config", "gc.auto", "0"])?;
    git(root, &["add", "-A"])?;
    git(
        root,
        &[
            "-c",
            "user.name=alint bench",
            "-c",
            "user.email=bench@alint.test",
            "commit",
            "-q",
            "-m",
            "bench base",
        ],
    )?;
    Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<()> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .with_context(|| format!("git {args:?}"))?;
    if !out.status.success() {
        bail!(
            "git {args:?} in {} failed: {}",
            root.display(),
            String::from_utf8_lossy(&out.stderr),
        );
    }
    Ok(())
}

/// Append a marker line to each path in `subset` so the file
/// shows up in `git ls-files --modified`. Cheap and
/// deterministic — alint reads the bytes anyway, so the marker
/// content doesn't materially change content-rule timing.
fn touch_subset(root: &Path, subset: &[&PathBuf]) -> Result<()> {
    for rel in subset {
        let abs = root.join(rel);
        let mut content = fs::read(&abs).with_context(|| format!("reading {}", abs.display()))?;
        content.extend_from_slice(b"\n// bench-scale: --changed marker\n");
        fs::write(&abs, content)?;
    }
    Ok(())
}

// ─── Output ──────────────────────────────────────────────────────────

fn write_outputs(report: &Report, args: &ScaleArgs) -> Result<()> {
    let out_dir = match &args.out {
        Some(p) => p.clone(),
        None => default_out_dir(&report.fingerprint)?,
    };
    fs::create_dir_all(&out_dir)?;

    // results.json — machine-readable, the canonical record.
    let json = serde_json::to_string_pretty(report)?;
    let json_path = out_dir.join("results.json");
    fs::write(&json_path, json)?;
    eprintln!("[xtask] wrote {}", json_path.display());

    if args.json_only {
        return Ok(());
    }

    // index.md + per-size results.md.
    let index = render_index(report);
    fs::write(out_dir.join("index.md"), index)?;
    eprintln!("[xtask] wrote {}", out_dir.join("index.md").display());

    for &size in &args.sizes {
        let body = render_per_size(report, size);
        let dir = out_dir.join(size.label());
        fs::create_dir_all(&dir)?;
        let path = dir.join("results.md");
        fs::write(&path, body)?;
        eprintln!("[xtask] wrote {}", path.display());
    }

    Ok(())
}

/// `docs/benchmarks/v0.5/scale/<os>-<arch>/` — the canonical
/// committable location. Maintainers pass `--out` to override
/// (e.g. for ad-hoc local runs they don't intend to commit).
fn default_out_dir(fp: &fingerprint::Fingerprint) -> Result<PathBuf> {
    let workspace = workspace_root()?;
    let platform = format!("{}-{}", fp.os, fp.arch);
    Ok(workspace
        .join("docs")
        .join("benchmarks")
        .join("v0.5")
        .join("scale")
        .join(platform))
}

fn render_index(report: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(&mut out, "# alint bench-scale results");
    let _ = writeln!(&mut out);
    write_fingerprint_block(&mut out, &report.fingerprint, &report.args);
    let _ = writeln!(&mut out);
    let _ = writeln!(
        &mut out,
        "Per-size detail under `<size>/results.md`. JSON: `results.json`."
    );
    let _ = writeln!(&mut out);
    let _ = writeln!(&mut out, "## Scenarios");
    let _ = writeln!(&mut out);
    for label in &report.args.scenarios {
        if let Ok(s) = Scenario::parse(label) {
            let _ = writeln!(&mut out, "- **{}** — {}", s.label(), s.description());
        }
    }
    let _ = writeln!(&mut out);
    let _ = writeln!(&mut out, "## Summary (mean ± stddev, ms)");
    let _ = writeln!(&mut out);
    let _ = writeln!(
        &mut out,
        "| Tool | Size | Scenario | Mode | Mean | Stddev | Min | Max | Samples |"
    );
    let _ = writeln!(&mut out, "|---|---|---|---|---:|---:|---:|---:|---:|");
    for r in &report.rows {
        let _ = writeln!(
            &mut out,
            "| {tool} | {size} | {scen} | {mode} | {mean:.1} | {stddev:.1} | {min:.1} | {max:.1} | {samples} |",
            tool = r.tool,
            size = r.size_label,
            scen = r.scenario,
            mode = r.mode,
            mean = r.mean_ms,
            stddev = r.stddev_ms,
            min = r.min_ms,
            max = r.max_ms,
            samples = r.samples,
        );
    }
    out
}

fn render_per_size(report: &Report, size: Size) -> String {
    let mut out = String::new();
    let _ = writeln!(&mut out, "# alint bench-scale — {} files", size.label());
    let _ = writeln!(&mut out);
    write_fingerprint_block(&mut out, &report.fingerprint, &report.args);
    let _ = writeln!(&mut out);
    let _ = writeln!(&mut out, "## Rows");
    let _ = writeln!(&mut out);
    let _ = writeln!(
        &mut out,
        "| Tool | Scenario | Mode | Mean (ms) | Stddev | Min | Max | Samples |"
    );
    let _ = writeln!(&mut out, "|---|---|---|---:|---:|---:|---:|---:|");
    for r in report.rows.iter().filter(|r| r.size_label == size.label()) {
        let _ = writeln!(
            &mut out,
            "| {tool} | {scen} | {mode} | {mean:.1} | {stddev:.1} | {min:.1} | {max:.1} | {samples} |",
            tool = r.tool,
            scen = r.scenario,
            mode = r.mode,
            mean = r.mean_ms,
            stddev = r.stddev_ms,
            min = r.min_ms,
            max = r.max_ms,
            samples = r.samples,
        );
    }
    let _ = writeln!(&mut out);
    let _ = writeln!(
        &mut out,
        "Tree shape: monorepo (`packages={pkg}, files_per_package={fpp}, total={total}`).",
        pkg = size.monorepo_shape().0,
        fpp = size.monorepo_shape().1,
        total = size.file_count(),
    );
    out
}

fn write_fingerprint_block(out: &mut String, fp: &fingerprint::Fingerprint, args: &ReportArgs) {
    let _ = writeln!(out, "**Platform:** `{}/{}`  ", fp.os, fp.arch);
    let _ = writeln!(
        out,
        "**CPU:** `{}` ({} cores)  ",
        fp.cpu_model, fp.cpu_cores
    );
    let _ = writeln!(out, "**RAM:** {} GB  ", fp.ram_gb);
    let _ = writeln!(out, "**FS:** `{}`  ", fp.fs_type);
    let _ = writeln!(out, "**rustc:** `{}`  ", fp.rustc);
    let _ = writeln!(
        out,
        "**alint:** `{}` ({})  ",
        fp.alint_version, fp.alint_git_sha
    );
    let _ = writeln!(out, "**hyperfine:** `{}`  ", fp.hyperfine_version);
    if !fp.tool_versions.is_empty() {
        let listing: String = fp
            .tool_versions
            .iter()
            .map(|(name, ver)| format!("{name}=`{ver}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "**Tools:** {listing}  ");
    }
    let _ = writeln!(out, "**Seed:** `{}`  ", args.seed);
    let _ = writeln!(out, "**Warmup/runs:** {} / {}  ", args.warmup, args.runs);
    let _ = writeln!(out, "**Generated:** `{}`  ", fp.timestamp);
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md`. \
         Compare numbers like-for-like (same fingerprint), never absolutely."
    );
}

// ─── Helpers ─────────────────────────────────────────────────────────

fn ensure_hyperfine() -> Result<()> {
    match Command::new("hyperfine").arg("--version").output() {
        Ok(out) if out.status.success() => Ok(()),
        _ => bail!(
            "hyperfine not found in PATH. Install:\n  cargo install hyperfine\n  \
             # or apt/brew/choco install hyperfine"
        ),
    }
}

fn build_release_binary() -> Result<PathBuf> {
    eprintln!("[xtask] cargo build --release -p alint");
    let status = Command::new(env!("CARGO"))
        .args(["build", "--release", "-p", "alint"])
        .status()
        .context("invoking cargo")?;
    if !status.success() {
        bail!("release build failed");
    }
    let workspace_root = workspace_root()?;
    let bin = workspace_root
        .join("target")
        .join("release")
        .join(if cfg!(windows) { "alint.exe" } else { "alint" });
    if !bin.is_file() {
        bail!("expected binary at {}", bin.display());
    }
    Ok(bin)
}

fn workspace_root() -> Result<PathBuf> {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let root = Path::new(manifest)
        .parent()
        .context("xtask has no parent directory")?;
    Ok(root.to_path_buf())
}

#[allow(dead_code)] // re-exported by main.rs but the linter doesn't see across mods.
pub(crate) fn now_iso() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!("unix:{secs}")
}
