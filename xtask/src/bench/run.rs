use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::output::write_outputs;
#[allow(clippy::wildcard_imports)]
use super::*;

// ─── Entry point ─────────────────────────────────────────────────────

/// Top-level entry called from `main.rs`. Builds the alint
/// binary, materialises trees, drives hyperfine, and writes
/// the report.
///
/// 137 lines spanning the (size, scenario, mode) matrix loop
/// — splitting would mean threading a 9-arg context tuple
/// through helpers that share lifetimes with the args /
/// output dir / fingerprint. Reads better top-to-bottom as
/// one phased pipeline: `--quick` collapse → out-dir setup
/// → tools filter → per-(size, scenario) tree generation →
/// per-cell hyperfine → per-version aggregation → report
/// emission. Same call as the `Engine::run` allow elsewhere.
#[allow(clippy::too_many_lines)]
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
        // Some scenarios (the consolidated S4) need a real git
        // repo; in that case the tree generator runs `git init &&
        // git add -A && git commit` as part of materialisation.
        // A scenario is served by the polyglot tree iff it
        // `requires_polyglot_tree`, else by the regular tree — so
        // route each tree's git requirement to the tree that
        // actually serves the git-needing scenario. Today only S4
        // needs git AND it is polyglot, so `polyglot_needs_git` is
        // the live path and `regular_needs_git` stays false; the
        // split keeps a future git-on-regular scenario correct.
        let needs_polyglot_tree = args.scenarios.iter().any(|s| s.requires_polyglot_tree());
        let needs_regular_tree = args.scenarios.iter().any(|s| !s.requires_polyglot_tree());
        let polyglot_needs_git = args
            .scenarios
            .iter()
            .any(|s| s.requires_polyglot_tree() && s.requires_git_repo());
        let regular_needs_git = args
            .scenarios
            .iter()
            .any(|s| !s.requires_polyglot_tree() && s.requires_git_repo());
        let (pkgs, fpp) = size.monorepo_shape();

        // Build the regular monorepo tree if any non-polyglot
        // scenario is in this run. Polyglot scenarios (S4) get
        // their own tree below. Most runs use only one of the two;
        // mixing them in one invocation builds both up-front and
        // dispatches per-scenario.
        let regular_tree = if needs_regular_tree {
            eprintln!(
                "[xtask] generating {}monorepo tree of {} files (seed={:#x})...",
                if regular_needs_git { "git-aware " } else { "" },
                size.file_count(),
                args.seed,
            );
            Some(if regular_needs_git {
                alint_bench::tree::generate_git_monorepo(pkgs, fpp, args.seed)
                    .with_context(|| format!("generating {} git-tree", size.label()))?
            } else {
                alint_bench::tree::generate_monorepo(pkgs, fpp, args.seed)
                    .with_context(|| format!("generating {} tree", size.label()))?
            })
        } else {
            None
        };
        let polyglot_tree = if needs_polyglot_tree {
            eprintln!(
                "[xtask] generating {}polyglot monorepo tree of {} files (seed={:#x})...",
                if polyglot_needs_git { "git-aware " } else { "" },
                size.file_count(),
                args.seed ^ 0xB011_F11E,
            );
            Some(if polyglot_needs_git {
                alint_bench::tree::generate_git_nested_polyglot_monorepo(
                    pkgs,
                    fpp,
                    args.seed ^ 0xB011_F11E,
                )
                .with_context(|| format!("generating {} git polyglot tree", size.label()))?
            } else {
                alint_bench::tree::generate_nested_polyglot_monorepo(
                    pkgs,
                    fpp,
                    args.seed ^ 0xB011_F11E,
                )
                .with_context(|| format!("generating {} polyglot tree", size.label()))?
            })
        } else {
            None
        };

        // Initialise git so `--changed` mode has something to
        // diff against. Done once per tree — hyperfine then
        // measures the same disk state across runs. Skipped
        // when no tool requested `Mode::Changed` to save time.
        // Both trees get the treatment if both exist.
        let needs_git = args.modes.contains(&Mode::Changed)
            && args
                .tools
                .iter()
                .any(|t| args.scenarios.iter().any(|s| t.supports(*s, Mode::Changed)));
        if needs_git {
            for tree in [regular_tree.as_ref(), polyglot_tree.as_ref()]
                .into_iter()
                .flatten()
            {
                let tree_root = tree.root();
                init_git_for_changed_mode(tree_root)?;
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
                touch_subset(tree_root, &to_touch)?;
            }
        }

        // Restore a clean page cache before this size's benchmark loop (opt-in
        // via ALINT_BENCH_DROP_CACHES). See the helper for why.
        maybe_quiesce_page_cache(size.label());

        for &scenario in &args.scenarios {
            let tree_for_scenario = if scenario.requires_polyglot_tree() {
                polyglot_tree
                    .as_ref()
                    .expect("polyglot tree built when any polyglot scenario in run")
            } else {
                regular_tree
                    .as_ref()
                    .expect("regular tree built when any non-polyglot scenario in run")
            };
            let tree_root = tree_for_scenario.root().to_path_buf();
            // Per-scenario fixture overlay (write the data files
            // the scenario's rules reference; no-op for S1 / S4,
            // and S2 / S3 / SFIX each plant a deterministic fixture).
            // Paired with `teardown_overlay` after the inner loop
            // so the overlay never leaks into the next scenario
            // running on the same shared tree.
            scenario.setup_overlay(&tree_root)?;
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
            scenario.teardown_overlay(&tree_root)?;
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

/// Flush dirty pages and drop the page cache before a size phase's benchmark
/// loop, when `ALINT_BENCH_DROP_CACHES` is set. On a small-RAM bench host the
/// cache fills with the previous phase's trees + git objects (two 1M trees plus
/// their objects is ~16 GB against 16 GB of RAM), which forces page-cache
/// reclaim — `allocstall`, invisible to disk-util and to `MemAvailable` — mid-
/// measurement on the first content-heavy scenario. That is the S2/1m/full
/// variance artifact. Clearing the cache restores the clean-cache start an
/// isolated run gets for free; hyperfine's warmup re-reads the tree, so the
/// MEASURED runs stay warm. A 62 GB host never reclaims and so never needs this,
/// which is why the flag is opt-in and off by default. Requires passwordless
/// sudo for `drop_caches`; a failure warns and continues, so an unprivileged run
/// simply degrades to the prior behavior rather than aborting.
fn maybe_quiesce_page_cache(size_label: &str) {
    if std::env::var_os("ALINT_BENCH_DROP_CACHES").is_none() {
        return;
    }
    match std::process::Command::new("sudo")
        .args(["sh", "-c", "sync; echo 3 > /proc/sys/vm/drop_caches"])
        .status()
    {
        Ok(s) if s.success() => {
            eprintln!("[xtask] dropped page cache before {size_label} benchmark loop");
        }
        Ok(s) => eprintln!("[xtask] WARN: drop_caches exited {s}; cache not cleared, continuing"),
        Err(e) => eprintln!("[xtask] WARN: drop_caches could not run ({e}); continuing"),
    }
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
///
/// **Idempotent re-entry.** When the matrix includes S4 (the
/// only `requires_git_repo` scenario), the polyglot tree was
/// already generated as a git repo with an initial commit by
/// `generate_git_nested_polyglot_monorepo`. In that case `git
/// init` is a no-op (re-init is silently OK), but `git commit`
/// would fail with "nothing to commit" — every file is in HEAD.
/// We probe `git rev-parse --verify HEAD`: if it succeeds (HEAD
/// exists), we skip the add+commit pair entirely — the existing
/// initial commit IS the bench base. The follow-up file-touch
/// step then produces the working-tree diff `--changed` mode
/// measures.
fn init_git_for_changed_mode(root: &Path) -> Result<()> {
    git(root, &["init", "-q", "-b", "main"])?;
    git(root, &["config", "gc.auto", "0"])?;
    if has_initial_commit(root) {
        return Ok(());
    }
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

/// True iff the repo at `root` already has at least one commit
/// reachable from HEAD. Used by [`init_git_for_changed_mode`]
/// to skip the add+commit pair when an S4 git-aware tree
/// already supplied the bench base.
fn has_initial_commit(root: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--verify", "--quiet", "HEAD"])
        .output()
        .is_ok_and(|o| o.status.success())
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
