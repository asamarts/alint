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

    // A requested tool may be installed but have no meaningful row in the
    // selected scenario/mode matrix (for example ls-lint with S2 only). Drop
    // those tools before fingerprinting or staging configs, and fail clearly
    // if the entire requested matrix is empty.
    args.tools
        .retain(|&tool| tool_supports_any_cell(tool, &args.scenarios, &args.modes));
    if args.tools.is_empty() {
        bail!("the selected tools, scenarios, and modes have no supported benchmark rows");
    }

    ensure_hyperfine()?;
    // A supplied `--alint-binary` (past-version backfill) is measured as-is; the
    // normal path builds the current checkout.
    let alint_bin = match &args.alint_binary {
        Some(path) => {
            if !path.is_file() {
                bail!("--alint-binary {} is not a file", path.display());
            }
            eprintln!("[xtask] using pre-built alint binary: {}", path.display());
            path.clone()
        }
        None => build_release_binary()?,
    };
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
                if !args.modes.iter().any(|&mode| tool.supports(scenario, mode)) {
                    continue;
                }
                // Stage only this competitor's config. It is removed after
                // all of the tool's supported modes so later rows see the
                // same generated tree rather than accumulated config files.
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
                tool.teardown_config(&tree_root)?;
            }
            scenario.teardown_overlay(&tree_root)?;
        }
    }

    for warning in flat_scaling_warnings(&rows) {
        eprintln!("[xtask] WARN: {warning}");
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

fn tool_supports_any_cell(tool: Tool, scenarios: &[Scenario], modes: &[Mode]) -> bool {
    scenarios
        .iter()
        .any(|&scenario| modes.iter().any(|&mode| tool.supports(scenario, mode)))
}

/// Flag a full-tree command whose 100k mean is less than twice its 1k mean.
/// Startup cost can flatten small cells, but a 100x larger walk should still
/// show material growth. This is advisory because host noise and a genuinely
/// sublinear tool are possible; the original broken ls-lint rows had a ratio
/// below 1 and would have triggered this warning.
fn flat_scaling_warnings(rows: &[Row]) -> Vec<String> {
    let mut warnings = Vec::new();
    for small in rows
        .iter()
        .filter(|row| row.size_files == 1_000 && row.mode == "full")
    {
        let Some(large) = rows.iter().find(|row| {
            row.size_files == 100_000
                && row.tool == small.tool
                && row.scenario == small.scenario
                && row.mode == small.mode
        }) else {
            continue;
        };
        if !small.mean_ms.is_finite() || !large.mean_ms.is_finite() || small.mean_ms <= 0.0 {
            continue;
        }
        let ratio = large.mean_ms / small.mean_ms;
        if ratio < 2.0 {
            warnings.push(format!(
                "suspiciously flat full-tree scaling for {}/{}: 100k/1k is {:.2}x ({:.1} ms / {:.1} ms); verify the tool read its config and walked the tree",
                small.tool, small.scenario, ratio, large.mean_ms, small.mean_ms,
            ));
        }
    }
    warnings
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

    let readiness_cmd = tool.readiness_invocation(alint, tree_root, scenario, mode);
    validate_benchmark_command(tool, tree_root, scenario, mode, &readiness_cmd, &label)?;

    let mut hyperfine = Command::new("hyperfine");
    hyperfine
        .args(["--warmup", &warmup.to_string()])
        .args(["--min-runs", &runs.to_string()])
        .args(["--max-runs", &runs.to_string()]);
    // Ignore only the per-tool statuses whose meaning is pinned by the
    // readiness check above. Never ignore setup/internal failures wholesale.
    let ignored_codes = tool.hyperfine_ignored_exit_codes();
    if !ignored_codes.is_empty() {
        let codes = ignored_codes
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        hyperfine.arg(format!("--ignore-failure={codes}"));
    }
    let status = hyperfine
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
    let times_ms = r.times.iter().map(|seconds| seconds * 1000.0).collect();

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
        times_ms,
        command: r.command,
    })
}

/// Run each row once before timing it. For config-driven competitors, plant a
/// deterministic violation and require their output to name it; this catches
/// the "tool started but never loaded its config" failure that invalidated the
/// original ls-lint comparison.
fn validate_benchmark_command(
    tool: Tool,
    tree_root: &Path,
    scenario: Scenario,
    mode: Mode,
    command: &str,
    label: &str,
) -> Result<()> {
    let probe = readiness_probe(tool, scenario, mode);
    let probe_path = probe.map(|probe| tree_root.join(probe.path));
    if let Some(path) = &probe_path {
        if path.exists() {
            bail!(
                "benchmark readiness probe path already exists; refusing to overwrite {}",
                path.display()
            );
        }
        fs::write(path, probe.expect("probe path requires a probe").contents)?;
    }
    let output = Command::new("sh")
        .args(["-c", command])
        .output()
        .with_context(|| format!("readiness probe for {label}"));
    if let Some(path) = &probe_path {
        fs::remove_file(path)
            .with_context(|| format!("remove readiness probe {}", path.display()))?;
    }
    let output = output?;
    let code = output.status.code();
    let allowed = match probe {
        Some(probe) => code == Some(probe.expected_exit),
        None => match tool {
            Tool::LsLint => code == Some(0),
            Tool::Alint | Tool::GrepPipeline | Tool::Repolinter => matches!(code, Some(0 | 1)),
        },
    };
    if !allowed {
        bail!(
            "benchmark readiness probe failed for {label} (exit {code:?}): {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    if let Some(probe) = probe {
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if !combined.contains(probe.needle) {
            bail!(
                "benchmark readiness probe for {label} did not report planted finding {:?}; the tool may not have loaded its config",
                probe.needle,
            );
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct ReadinessProbe {
    path: &'static str,
    contents: &'static str,
    needle: &'static str,
    expected_exit: i32,
}

/// Competitive full-mode rows get a scenario-specific planted finding. The
/// exact exit status plus diagnostic needle distinguish a real lint result
/// from startup/config errors (notably Repolinter, which returns 1 for both).
fn readiness_probe(tool: Tool, scenario: Scenario, mode: Mode) -> Option<ReadinessProbe> {
    if mode != Mode::Full {
        return None;
    }
    match (tool, scenario) {
        (Tool::Alint, Scenario::S1) => Some(ReadinessProbe {
            path: "alint_bench_probe.bak",
            contents: "benchmark readiness probe\n",
            needle: "no-bak",
            expected_exit: 1,
        }),
        (Tool::LsLint, Scenario::S1) => Some(ReadinessProbe {
            path: "alint_bench_BadName.rs",
            contents: "// benchmark readiness probe\n",
            needle: "alint_bench_BadName.rs",
            expected_exit: 0,
        }),
        (Tool::GrepPipeline, Scenario::S1) => Some(ReadinessProbe {
            path: "alint_bench_BadName.rs",
            contents: "// benchmark readiness probe\n",
            needle: "alint_bench_BadName.rs",
            expected_exit: 1,
        }),
        (Tool::Alint, Scenario::S2) => Some(ReadinessProbe {
            path: "alint_bench_probe.ts",
            contents: "debugger;\n",
            needle: "ts-no-debugger",
            expected_exit: 1,
        }),
        (Tool::GrepPipeline, Scenario::S2) => Some(ReadinessProbe {
            path: "alint_bench_probe.rs",
            contents: "// TODO: benchmark readiness probe\n",
            needle: "alint_bench_probe.rs",
            expected_exit: 0,
        }),
        (Tool::Repolinter, Scenario::S2) => Some(ReadinessProbe {
            path: "alint_bench_probe.rs",
            contents: "// TODO: benchmark readiness probe\n",
            needle: "no-todo-rust",
            expected_exit: 1,
        }),
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn row(tool: &str, size_files: usize, mean_ms: f64) -> Row {
        Row {
            tool: tool.into(),
            size_files,
            size_label: if size_files == 1_000 { "1k" } else { "100k" }.into(),
            scenario: "S1".into(),
            mode: "full".into(),
            mean_ms,
            stddev_ms: 0.0,
            median_ms: mean_ms,
            min_ms: mean_ms,
            max_ms: mean_ms,
            samples: 1,
            times_ms: vec![mean_ms],
            command: String::new(),
        }
    }

    #[test]
    fn flat_scaling_warns_below_two_x() {
        let warnings =
            flat_scaling_warnings(&[row("ls-lint", 1_000, 28.0), row("ls-lint", 100_000, 27.0)]);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("ls-lint/S1"));
        assert!(warnings[0].contains("0.96x"));
    }

    #[test]
    fn flat_scaling_accepts_material_growth_and_incomplete_pairs() {
        let warnings = flat_scaling_warnings(&[
            row("alint", 1_000, 10.0),
            row("alint", 100_000, 390.0),
            row("shell", 1_000, 58.0),
        ]);
        assert_eq!(warnings, Vec::<String>::new());
    }

    #[test]
    fn readiness_probes_pin_each_competitive_contract() {
        let cases = [
            (Tool::Alint, Scenario::S1, 1, "no-bak"),
            (Tool::LsLint, Scenario::S1, 0, "alint_bench_BadName.rs"),
            (
                Tool::GrepPipeline,
                Scenario::S1,
                1,
                "alint_bench_BadName.rs",
            ),
            (Tool::Alint, Scenario::S2, 1, "ts-no-debugger"),
            (Tool::GrepPipeline, Scenario::S2, 0, "alint_bench_probe.rs"),
            (Tool::Repolinter, Scenario::S2, 1, "no-todo-rust"),
        ];
        for (tool, scenario, expected_exit, needle) in cases {
            let probe = readiness_probe(tool, scenario, Mode::Full).unwrap();
            assert_eq!(probe.expected_exit, expected_exit);
            assert_eq!(probe.needle, needle);
        }
        assert!(readiness_probe(Tool::Alint, Scenario::S1, Mode::Changed).is_none());
    }

    #[test]
    fn unsupported_tools_are_excluded_from_the_selected_matrix() {
        assert!(tool_supports_any_cell(
            Tool::LsLint,
            &[Scenario::S1],
            &[Mode::Full]
        ));
        assert!(!tool_supports_any_cell(
            Tool::LsLint,
            &[Scenario::S2],
            &[Mode::Full, Mode::Changed]
        ));
        assert!(!tool_supports_any_cell(
            Tool::Repolinter,
            &[Scenario::S1],
            &[Mode::Full]
        ));
        assert!(tool_supports_any_cell(
            Tool::Alint,
            &[Scenario::Sfix],
            &[Mode::Fix]
        ));
    }
}
