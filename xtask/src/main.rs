//! `xtask` — ancillary helpers for alint that don't belong in the shipped binary.
//!
//! Current commands:
//!
//! - `bench-release` — builds alint in release mode, generates deterministic
//!   synthetic trees, runs `hyperfine` across a tree-size × rule-count
//!   matrix, and emits a platform-fingerprinted markdown report. Used to
//!   produce the numbers published in `docs/benchmarks/<version>/`.
//! - `gen-fixture`   — materialize a synthetic tree for ad-hoc experimentation.
//! - `docs-export`   — emit a `docs-bundle/` directory consumed by the
//!   `asamarts/alint.org` site at build time. The bundle is the canonical
//!   handoff format between the alint repo (source of truth for technical
//!   docs) and the site repo (presentation).

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

mod bench;

#[derive(Parser)]
#[command(name = "xtask", about = "alint developer helpers")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build alint in release mode and run hyperfine across a tree × rules matrix.
    /// Legacy v0.1 single-config harness — see `bench-scale` for the v0.5
    /// scenario × size × mode matrix.
    BenchRelease {
        /// Skip the large tree sizes; produce a smoke-test-sized report in ~seconds.
        #[arg(long)]
        quick: bool,
        /// Where to write the markdown report. Defaults to stdout.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Seed used to generate the synthetic trees.
        #[arg(long, default_value_t = 0xA11E47)]
        seed: u64,
    },
    /// Scale-ceiling benchmark: hyperfine across a (size × scenario × mode)
    /// matrix with hardware fingerprint capture and JSON + Markdown
    /// publication. Default sizes 1k/10k/100k; opt into 1m via
    /// `--include-1m`.
    BenchScale {
        /// Comma-separated sizes (1k,10k,100k,1m).
        #[arg(long, default_value = "1k,10k,100k", value_delimiter = ',')]
        sizes: Vec<String>,
        /// Include the 1M-file size (multi-GB working set, slow).
        #[arg(long)]
        include_1m: bool,
        /// Comma-separated scenarios. Default `S1,S2,S3` is the
        /// publication trio (filename / existence+content /
        /// workspace bundle). `S4` (agent-era hygiene) and `S5`
        /// (fix-pass) are opt-in for characterization runs.
        #[arg(long, default_value = "S1,S2,S3", value_delimiter = ',')]
        scenarios: Vec<String>,
        /// Comma-separated modes (full,changed).
        #[arg(long, default_value = "full,changed", value_delimiter = ',')]
        modes: Vec<String>,
        /// Comma-separated tools (alint, ls-lint, or `all`).
        /// Default `alint` (preserves v0.5.6's alint-only
        /// publication shape). `all` expands to every known
        /// tool variant; tools not on PATH are auto-skipped
        /// with a stderr note rather than aborting the run.
        #[arg(long, default_value = "alint", value_delimiter = ',')]
        tools: Vec<String>,
        /// Hyperfine warmup runs.
        #[arg(long, default_value_t = 3)]
        warmup: u32,
        /// Hyperfine measured runs.
        #[arg(long, default_value_t = 10)]
        runs: u32,
        /// Tree-generator seed.
        #[arg(long, default_value_t = 0xA11E47)]
        seed: u64,
        /// Percent of files modified for `changed` mode (1-100).
        #[arg(long, default_value_t = 10.0)]
        diff_pct: f64,
        /// Output directory. Defaults to
        /// `docs/benchmarks/macro/results/<os>-<arch>/v<workspace-version>/`.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Smoke mode: collapses the matrix to a single 1k/S1/full row in seconds.
        #[arg(long)]
        quick: bool,
        /// Skip the Markdown reports; emit JSON only.
        #[arg(long)]
        json_only: bool,
        /// Re-execute inside the published `alint-bench` Docker
        /// image so every competitor tool's version is fixed by
        /// the image tag. Bind-mounts the workspace at /work and
        /// uses a named volume for the cargo target dir.
        /// Override the image with `ALINT_BENCH_IMAGE=...`.
        #[arg(long)]
        docker: bool,
    },
    /// Materialize a synthetic tree (persistent) for manual experimentation.
    GenFixture {
        #[arg(long, default_value_t = 1000)]
        files: usize,
        #[arg(long, default_value_t = 4)]
        depth: usize,
        #[arg(long, default_value_t = 42)]
        seed: u64,
        /// Where to place the tree. Defaults to a fresh tempdir.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Materialize a Cargo-workspace-shaped monorepo tree at a
    /// fixed path. Used by the perf-investigation flow to keep
    /// a 1k/10k/100k/1m tree across profile runs (skip 5 min of
    /// tree-gen per iteration). Same shape as `bench-scale`'s
    /// internal tree (matches its size labels).
    GenMonorepo {
        /// Size label: 1k / 10k / 100k / 1m. Picks
        /// `(packages, files_per_package)` to hit the size.
        #[arg(long)]
        size: String,
        /// Tree-generator seed (matches bench-scale default so
        /// trees are byte-identical to the published bench corpus).
        #[arg(long, default_value_t = 0xA11E47)]
        seed: u64,
        /// Where to place the tree.
        #[arg(long)]
        out: PathBuf,
    },
    /// Compare two `target/criterion` trees and gate on
    /// regressions. `--before` and `--after` should each be a
    /// criterion-format directory (a tree of
    /// `<group>/<id>/new/estimates.json` files). Exits non-zero
    /// when any paired bench's mean time has grown by more than
    /// `--threshold` percent — wire into PR CI to gate
    /// performance regressions.
    BenchCompare {
        /// Baseline criterion directory (typically saved off the
        /// main branch as `target/criterion-main`).
        #[arg(long)]
        before: PathBuf,
        /// Candidate criterion directory (typically the freshly
        /// produced `target/criterion`).
        #[arg(long)]
        after: PathBuf,
        /// Regression gate: fail when any pair grows past this
        /// percent. Defaults to 10.0.
        #[arg(long, default_value_t = 10.0)]
        threshold: f64,
    },
    /// Snapshot `target/criterion/` into the per-version
    /// committable location under
    /// `docs/benchmarks/micro/results/<os>-<arch>/<workspace-version>/criterion/`.
    /// Run after a publication-grade `cargo bench -p alint-bench`
    /// to materialise a snapshot ready for `git add`.
    PublishBenches {
        /// Source criterion directory. Defaults to `target/criterion`.
        #[arg(long, default_value = "target/criterion")]
        from: PathBuf,
        /// Override the per-version output dir. Defaults to
        /// `docs/benchmarks/micro/results/<os>-<arch>/v<workspace-version>/criterion/`.
        #[arg(long)]
        to: Option<PathBuf>,
        /// Skip the html / svg / raw-sample artefacts that
        /// criterion writes. Default: false (full snapshot).
        /// Use --trim for committable snapshots that would
        /// otherwise add tens of MB of HTML reports.
        #[arg(long)]
        trim: bool,
    },
    /// Emit `docs-bundle/` — the handoff bundle consumed by
    /// `asamarts/alint.org` at site-build time.
    DocsExport {
        /// Output directory. Defaults to `target/docs-bundle/`.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Validate the export would succeed without writing
        /// anything. Used by CI to gate merges on a buildable
        /// bundle.
        #[arg(long)]
        check: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::BenchRelease { quick, out, seed } => bench_release(quick, out, seed),
        Commands::BenchScale {
            sizes,
            include_1m,
            scenarios,
            modes,
            tools,
            warmup,
            runs,
            seed,
            diff_pct,
            out,
            quick,
            json_only,
            docker,
        } => dispatch_bench_scale(
            &sizes, include_1m, &scenarios, &modes, &tools, warmup, runs, seed, diff_pct, out,
            quick, json_only, docker,
        ),
        Commands::GenFixture {
            files,
            depth,
            seed,
            out,
        } => gen_fixture(files, depth, seed, out),
        Commands::GenMonorepo { size, seed, out } => gen_monorepo(&size, seed, &out),
        Commands::PublishBenches { from, to, trim } => publish_benches(&from, to.as_deref(), trim),
        Commands::BenchCompare {
            before,
            after,
            threshold,
        } => bench::compare::run(&before, &after, threshold),
        Commands::DocsExport { out, check } => docs_export(out, check),
    }
}

#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
fn dispatch_bench_scale(
    sizes: &[String],
    include_1m: bool,
    scenarios: &[String],
    modes: &[String],
    tools: &[String],
    warmup: u32,
    runs: u32,
    seed: u64,
    diff_pct: f64,
    out: Option<PathBuf>,
    quick: bool,
    json_only: bool,
    docker: bool,
) -> Result<()> {
    if docker {
        // The `--docker` path forwards args verbatim into the
        // image's entrypoint. Skip host-side parse so the
        // container's xtask sees the exact same flags the
        // user typed (including `--include-1m`, `--quick`,
        // etc.); the container's matrix-parse and tool-detect
        // happen against the image's installed toolset.
        return bench::docker::run_in_docker(&bench::docker::ForwardedArgs {
            sizes: sizes.to_vec(),
            include_1m,
            scenarios: scenarios.to_vec(),
            modes: modes.to_vec(),
            tools: tools.to_vec(),
            warmup,
            runs,
            seed,
            diff_pct,
            out,
            quick,
            json_only,
        });
    }

    // Parse + filter the matrix args before handing to the
    // bench module. Keeps the bench module typed (Size /
    // Scenario / Mode / Tool) and the CLI surface stringy.
    let mut parsed_sizes: Vec<bench::Size> = sizes
        .iter()
        .map(|s| bench::Size::parse(s))
        .collect::<Result<_>>()?;
    if include_1m {
        // Implicit add: `--include-1m` should produce a run
        // that includes 1m even if `--sizes` was left at its
        // default (1k,10k,100k). The opt-in flag's job is to
        // gate the 1m size against accidental inclusion, not
        // to require also retyping the size list.
        if !parsed_sizes.contains(&bench::Size::M1) {
            parsed_sizes.push(bench::Size::M1);
        }
    } else {
        parsed_sizes.retain(|s| !s.is_opt_in());
    }
    if parsed_sizes.is_empty() {
        bail!("no sizes selected — pass --include-1m if you only requested `1m`");
    }
    let parsed_scenarios: Vec<bench::Scenario> = scenarios
        .iter()
        .map(|s| bench::Scenario::parse(s))
        .collect::<Result<_>>()?;
    let parsed_modes: Vec<bench::Mode> = modes
        .iter()
        .map(|s| bench::Mode::parse(s))
        .collect::<Result<_>>()?;
    let parsed_tools = bench::tools::resolve(tools)?;
    if !(0.0..=100.0).contains(&diff_pct) {
        bail!("--diff-pct must be in [0, 100]; got {diff_pct}");
    }
    bench::bench_scale(bench::ScaleArgs {
        sizes: parsed_sizes,
        scenarios: parsed_scenarios,
        modes: parsed_modes,
        tools: parsed_tools,
        warmup,
        runs,
        seed,
        diff_pct,
        out,
        quick,
        json_only,
    })
}

fn gen_fixture(files: usize, depth: usize, seed: u64, out: Option<PathBuf>) -> Result<()> {
    let tree = alint_bench::tree::generate_tree(files, depth, seed)?;
    let final_path = match out {
        Some(p) => {
            fs::create_dir_all(&p)?;
            copy_tree(tree.root(), &p)?;
            p
        }
        None => tree.into_persistent()?,
    };
    println!("generated {files} files under {}", final_path.display());
    Ok(())
}

fn gen_monorepo(size: &str, seed: u64, out: &Path) -> Result<()> {
    let (packages, files_per_package, total) = match size {
        "1k" => (50, 18, 1_000),
        "10k" => (200, 48, 10_000),
        "100k" => (1000, 98, 100_000),
        "1m" => (5000, 198, 1_000_000),
        other => bail!("unknown size {other:?}; expected one of 1k / 10k / 100k / 1m"),
    };
    if out.exists() {
        bail!(
            "{} already exists; remove it first or pick a fresh path",
            out.display()
        );
    }
    let tree = alint_bench::tree::generate_monorepo(packages, files_per_package, seed)?;
    fs::create_dir_all(out)?;
    copy_tree(tree.root(), out)?;
    println!(
        "generated {total} files (packages={packages}, files_per_package={files_per_package}) under {}",
        out.display(),
    );
    Ok(())
}

/// Snapshot `target/criterion/` into the per-version published
/// directory. Default destination
/// `docs/benchmarks/micro/results/<os>-<arch>/v<workspace-version>/criterion/`
/// matches the layout `docs/benchmarks/micro/README.md` documents.
///
/// Pass `--trim` to skip the html / svg / raw-sample artefacts;
/// useful for committable snapshots that would otherwise add tens
/// of MB of HTML reports per release.
fn publish_benches(from: &Path, to: Option<&Path>, trim: bool) -> Result<()> {
    if !from.exists() {
        bail!(
            "source criterion dir {} does not exist; run `cargo bench -p alint-bench --features fs-benches` first",
            from.display()
        );
    }
    let workspace = workspace_root_from_xtask()?;
    let dest_owned: PathBuf;
    let dest = if let Some(p) = to {
        p
    } else {
        let arch = std::env::consts::ARCH;
        let os = std::env::consts::OS;
        let version = workspace_version_from_manifest(&workspace)?;
        dest_owned = workspace
            .join("docs")
            .join("benchmarks")
            .join("micro")
            .join("results")
            .join(format!("{os}-{arch}"))
            .join(format!("v{version}"))
            .join("criterion");
        &dest_owned
    };
    if dest.exists() {
        bail!(
            "{} already exists; remove it first or pick a different --to path",
            dest.display()
        );
    }
    fs::create_dir_all(dest)?;
    copy_criterion_tree(from, dest, trim)?;
    let trimmed_note = if trim { " (trimmed)" } else { "" };
    println!(
        "published {} → {}{trimmed_note}",
        from.display(),
        dest.display(),
    );
    Ok(())
}

/// Find the workspace root by walking up from the xtask binary's
/// `CARGO_MANIFEST_DIR`. xtask itself lives at `<workspace>/xtask`,
/// so the parent of `CARGO_MANIFEST_DIR` IS the workspace root.
fn workspace_root_from_xtask() -> Result<PathBuf> {
    let xtask_manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    xtask_manifest
        .parent()
        .map(Path::to_path_buf)
        .context("no parent dir for xtask CARGO_MANIFEST_DIR")
}

/// Tiny inline parse of the workspace `Cargo.toml`'s
/// `version = "..."` line. Same shape as
/// `bench::workspace_version` — duplicated here to keep `xtask`
/// from depending on `bench::` private internals.
fn workspace_version_from_manifest(workspace: &Path) -> Result<String> {
    let manifest = std::fs::read_to_string(workspace.join("Cargo.toml"))
        .context("read workspace Cargo.toml")?;
    for line in manifest.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("version") {
            if let Some(eq) = rest.find('=')
                && let Some(start) = rest[eq..].find('"')
                && let Some(end) = rest[eq + start + 1..].find('"')
            {
                let value = &rest[eq + start + 1..eq + start + 1 + end];
                return Ok(value.to_string());
            }
        }
    }
    bail!(
        "could not find workspace version in {}/Cargo.toml",
        workspace.display(),
    )
}

/// Copy a criterion-format tree, optionally skipping the
/// non-essential artefacts. The `--trim` mode keeps everything
/// `xtask bench-compare` reads (`new/estimates.json`,
/// `new/sample.json`, `new/benchmark.json`, the matching `base/`
/// trio) and drops everything `criterion-html-report` produces
/// (`report/`, `*.svg` files, `change/` subdirs).
fn copy_criterion_tree(from: &Path, to: &Path, trim: bool) -> Result<()> {
    for entry in walkdir_plain(from)? {
        let rel = entry.strip_prefix(from).unwrap();
        if trim && should_trim_path(rel) {
            continue;
        }
        let dest = to.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&dest)?;
        } else if entry.is_file() {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&entry, &dest)?;
        }
    }
    Ok(())
}

/// True for paths under a criterion tree that we drop in `--trim`
/// mode. Conservative: only things `bench-compare` provably
/// doesn't read get trimmed.
fn should_trim_path(rel: &Path) -> bool {
    let s = rel.to_string_lossy();
    s.contains("/report/")
        || s.starts_with("report/")
        || s.ends_with(".svg")
        || s.ends_with(".html")
        || s.contains("/change/")
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    for entry in walkdir_plain(from)? {
        let rel = entry.strip_prefix(from).unwrap();
        let dest = to.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&dest)?;
        } else if entry.is_file() {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&entry, &dest)?;
        }
    }
    Ok(())
}

fn walkdir_plain(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        let md = fs::metadata(&p)?;
        if md.is_dir() {
            for entry in fs::read_dir(&p)? {
                stack.push(entry?.path());
            }
            out.push(p);
        } else {
            out.push(p);
        }
    }
    Ok(out)
}

// ---- bench-release ---------------------------------------------------------

const RULES_CONFIG_YAML: &str = include_str!("bench_config.yml");

fn bench_release(quick: bool, out: Option<PathBuf>, seed: u64) -> Result<()> {
    ensure_hyperfine()?;

    let binary = build_release_binary()?;
    let sizes: &[usize] = if quick {
        &[500]
    } else {
        &[1_000, 10_000, 100_000]
    };

    // Write the shared config file once to a tempdir and point every run at it.
    let config_dir = tempfile::tempdir()?;
    let config_path = config_dir.path().join(".alint.yml");
    fs::write(&config_path, RULES_CONFIG_YAML)?;

    let mut report = String::new();
    write_header(&mut report, quick, seed)?;

    for &size in sizes {
        eprintln!("[xtask] generating tree of {size} files (seed={seed})...");
        let tree = alint_bench::tree::generate_tree(size, 4, seed)?;
        // hyperfine doesn't care about CWD; we pass the tree path as an argument.
        let target_path = tree.root();
        // Copy the config into the tree so `alint check <path>` discovers it.
        fs::copy(&config_path, target_path.join(".alint.yml"))?;

        eprintln!("[xtask] running hyperfine against {size}-file tree...");
        let md = run_hyperfine(&binary, target_path, size, quick)?;
        writeln!(&mut report, "\n### {size} files\n")?;
        writeln!(&mut report, "{md}")?;
    }

    match out {
        Some(path) => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, report)?;
            eprintln!("[xtask] wrote {}", path.display());
        }
        None => print!("{report}"),
    }
    Ok(())
}

fn ensure_hyperfine() -> Result<()> {
    match Command::new("hyperfine").arg("--version").output() {
        Ok(out) if out.status.success() => Ok(()),
        _ => bail!(
            "hyperfine not found in PATH. Install it with:\n  \
             cargo install hyperfine\n  # or apt/brew/choco install hyperfine"
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

pub(crate) fn workspace_root() -> Result<PathBuf> {
    // xtask is inside the workspace; CARGO_MANIFEST_DIR = alint/xtask; parent = workspace root.
    let manifest = env!("CARGO_MANIFEST_DIR");
    let root = Path::new(manifest)
        .parent()
        .context("xtask has no parent directory")?;
    Ok(root.to_path_buf())
}

fn run_hyperfine(binary: &Path, tree_root: &Path, size: usize, quick: bool) -> Result<String> {
    let warmup = if quick { "2" } else { "5" };
    let min_runs = if quick { "3" } else { "10" };

    let tmp_md = tempfile::NamedTempFile::new()?;
    let md_path = tmp_md.path().to_path_buf();

    let status = Command::new("hyperfine")
        .args(["--warmup", warmup, "--min-runs", min_runs])
        .arg("--command-name")
        .arg(format!("alint check (synthetic, {size} files)"))
        .arg("--export-markdown")
        .arg(&md_path)
        .arg(format!(
            "{} check {}",
            shell_quote(binary.to_str().unwrap()),
            shell_quote(tree_root.to_str().unwrap())
        ))
        .status()
        .context("invoking hyperfine")?;
    if !status.success() {
        bail!("hyperfine exited non-zero");
    }
    Ok(fs::read_to_string(&md_path)?)
}

fn shell_quote(s: &str) -> String {
    if s.chars().any(|c| c == ' ' || c == '\t') {
        format!("\"{s}\"")
    } else {
        s.to_string()
    }
}

fn write_header(report: &mut String, quick: bool, seed: u64) -> Result<()> {
    writeln!(
        report,
        "# alint bench-release results\n\n\
         **Mode:** {mode}  \n\
         **Seed:** `{seed:#x}`  \n\
         **OS:** `{os}/{arch}`  \n\
         **rustc:** `{rustc}`  \n\
         **alint git SHA:** `{sha}`  \n\
         **Generated:** {ts}  \n\n\
         Results measured with `hyperfine` on this machine. Cross-machine \
         variance is expected; see `docs/benchmarks/METHODOLOGY.md` for the \
         reproduction recipe. Do not compare absolute numbers across \
         rows in different files — compare like-for-like.",
        mode = if quick { "quick (smoke)" } else { "full" },
        seed = seed,
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        rustc = rustc_version().unwrap_or_else(|| "unknown".to_string()),
        sha = git_sha().unwrap_or_else(|| "unknown".to_string()),
        ts = now_iso(),
    )?;
    Ok(())
}

fn rustc_version() -> Option<String> {
    let out = Command::new("rustc").arg("--version").output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn git_sha() -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn now_iso() -> String {
    // Minimal ISO-ish timestamp without pulling in chrono.
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!("unix:{secs}")
}

// ---- docs-export ----------------------------------------------------------

/// Workspace-relative paths the export reads from. Centralised so a
/// `git mv` of any of these is a one-liner here, not a hunt across
/// the function body.
mod docs_paths {
    pub const SITE_DIR: &str = "docs/site";
    pub const RULES_DOC: &str = "docs/rules.md";
    pub const ARCHITECTURE_DOC: &str = "docs/design/ARCHITECTURE.md";
    pub const ROADMAP_DOC: &str = "docs/design/ROADMAP.md";
    pub const RULE_AUTHORING_DOC: &str = "docs/development/RULE-AUTHORING.md";
    pub const CHANGELOG: &str = "CHANGELOG.md";
    pub const SCHEMA_JSON: &str = "schemas/v1/config.json";
    pub const RULESETS_DIR: &str = "crates/alint-dsl/rulesets/v1";
}

fn docs_export(out: Option<PathBuf>, check: bool) -> Result<()> {
    let workspace = workspace_root()?;
    let target_dir = out.unwrap_or_else(|| workspace.join("target/docs-bundle"));

    // In check mode we still produce the bundle (so all the
    // generators run) — just under a tempdir we discard. Catches
    // missing files / bad YAML / broken --help before merge.
    let _scratch_guard;
    let target_dir = if check {
        let scratch = tempfile::tempdir().context("creating tempdir for --check")?;
        let path = scratch.path().to_path_buf();
        _scratch_guard = scratch;
        path
    } else {
        // Clean previous output so removed pages don't linger.
        if target_dir.exists() {
            fs::remove_dir_all(&target_dir)
                .with_context(|| format!("removing stale {}", target_dir.display()))?;
        }
        fs::create_dir_all(&target_dir)?;
        target_dir
    };

    eprintln!("[xtask] docs-export → {}", target_dir.display());

    // 1. Hand-written long-form prose, copied verbatim.
    copy_site_tree(&workspace, &target_dir)?;

    // 2. Verbatim copies of the existing top-level docs.
    copy_one(
        &workspace.join(docs_paths::CHANGELOG),
        &target_dir.join("changelog.md"),
        Some("Changelog"),
    )?;
    copy_one(
        &workspace.join(docs_paths::ARCHITECTURE_DOC),
        &target_dir.join("about/architecture.md"),
        Some("Architecture"),
    )?;
    copy_one(
        &workspace.join(docs_paths::ROADMAP_DOC),
        &target_dir.join("about/roadmap.md"),
        Some("Roadmap"),
    )?;
    copy_one(
        &workspace.join(docs_paths::RULE_AUTHORING_DOC),
        &target_dir.join("development/rule-authoring.md"),
        Some("Rule authoring"),
    )?;
    // Rule reference: slice docs/rules.md by H2 (= family) →
    // H3 (= rule kind) into one page per kind, plus per-family
    // overviews and a master alphabetical index. Returns a
    // kind → family-slug map used below to render kind names
    // as links from the bundled-ruleset pages.
    let kind_to_family = generate_rules_pages(&workspace, &target_dir)?;

    // 3. Per-bundled-ruleset reference page. `kind_to_family`
    //    drives the cross-links from `**kind**: <name>` →
    //    `/docs/rules/<family>/<name>/`.
    generate_bundled_ruleset_pages(&workspace, &target_dir, &kind_to_family)?;

    // 4. The JSON Schema, kept as JSON for programmatic use.
    let schema_dest = target_dir.join("configuration/schema.json");
    fs::create_dir_all(schema_dest.parent().unwrap())?;
    fs::copy(workspace.join(docs_paths::SCHEMA_JSON), &schema_dest)?;

    // 5. CLI reference, captured from the alint binary's --help.
    generate_cli_reference(&workspace, &target_dir)?;

    // 6. Manifest. Any consumer (alint.org sync script, audit
    //    tooling) reads this to know what's in the bundle.
    write_manifest(&target_dir)?;

    if check {
        eprintln!("[xtask] docs-export --check OK");
    } else {
        eprintln!("[xtask] docs-export wrote {}", target_dir.display());
    }
    Ok(())
}

/// Recursively copy `docs/site/**.md` into the bundle root. Mirror
/// the directory layout exactly — `docs/site/getting-started/foo.md`
/// → `docs-bundle/getting-started/foo.md`.
fn copy_site_tree(workspace: &Path, target_dir: &Path) -> Result<()> {
    let site_root = workspace.join(docs_paths::SITE_DIR);
    if !site_root.is_dir() {
        bail!(
            "{} is missing — Phase 2 expects hand-written docs to live here",
            site_root.display()
        );
    }
    for entry in walkdir_plain(&site_root)? {
        let md = fs::metadata(&entry)?;
        if !md.is_file() {
            continue;
        }
        let rel = entry.strip_prefix(&site_root).unwrap();
        let dest = target_dir.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&entry, &dest)
            .with_context(|| format!("copying {} → {}", entry.display(), dest.display()))?;
    }
    Ok(())
}

/// Copy one source file into the bundle. If `title` is `Some`,
/// inject a Starlight frontmatter block at the top of the
/// destination so the page renders with the desired title in the
/// Starlight chrome (the source files don't carry their own
/// frontmatter — they're project-internal docs).
fn copy_one(src: &Path, dest: &Path, title: Option<&str>) -> Result<()> {
    if !src.is_file() {
        bail!("expected file at {}", src.display());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(title) = title {
        let body = fs::read_to_string(src).with_context(|| format!("reading {}", src.display()))?;
        let stripped = strip_first_h1(&body);
        let mut out = String::new();
        let _ = writeln!(&mut out, "---");
        let _ = writeln!(&mut out, "title: {title}");
        let _ = writeln!(&mut out, "---");
        let _ = writeln!(&mut out);
        out.push_str(stripped);
        fs::write(dest, out).with_context(|| format!("writing {}", dest.display()))?;
    } else {
        fs::copy(src, dest)
            .with_context(|| format!("copying {} → {}", src.display(), dest.display()))?;
    }
    Ok(())
}

/// Strip the first top-level `# heading` line so the Starlight
/// frontmatter `title` we inject doesn't render *next to* a
/// duplicate H1 from the source file.
fn strip_first_h1(body: &str) -> &str {
    let trimmed = body.trim_start();
    if let Some(rest) = trimmed.strip_prefix("# ") {
        // Skip until end-of-line + the trailing newline.
        if let Some(idx) = rest.find('\n') {
            return rest[idx + 1..].trim_start_matches('\n');
        }
        return "";
    }
    body
}

/// Per-rule-kind pages from `docs/rules.md`.
///
/// rules.md is structured H2 (family) → H3 (one heading per
/// rule kind, sometimes covering paired/triplet kinds via a
/// slash- or comma-separated list of backtick'd names). We
/// slice into:
/// - `rules/<family-slug>/<kind>.md` — one Starlight page per
///   rule kind. Multi-kind H3s emit one page per kind; the
///   pages share the H3 body and cross-link via "See also".
/// - `rules/<family-slug>/index.md` — family overview with
///   one-line summaries linking to each kind page.
/// - `rules/index.md` — alphabetical master index of every
///   kind shipped in this build.
///
/// Two H2 sections are special-cased out of the rules tree
/// because they're concepts rather than rule kinds:
/// - "Fix operations" → `concepts/fix-operations.md`
/// - "Nested .alint.yml (monorepo layering)" →
///   `concepts/nested-configs.md`
///
/// Sections we drop entirely:
/// - "Contents" (the source's TOC; redundant with our generated
///   index)
/// - "Bundled rulesets" (per-ruleset pages already generated
///   from the YAML bodies)
///
/// Returns a `kind → family-slug` map so the bundled-ruleset
/// generator can produce links like
/// `[json_path_equals](/docs/rules/content/json_path_equals/)`.
fn generate_rules_pages(
    workspace: &Path,
    target_dir: &Path,
) -> Result<std::collections::HashMap<String, String>> {
    use std::collections::{HashMap, HashSet};

    let src = fs::read_to_string(workspace.join(docs_paths::RULES_DOC))
        .with_context(|| format!("reading {}", docs_paths::RULES_DOC))?;

    // Authoritative list of rule kinds from the registry. We
    // cross-check against this so a typo in rules.md surfaces
    // at export time, not at site render time.
    let registry = alint_rules::builtin_registry();
    let known_kinds: HashSet<String> = registry.known_kinds().map(str::to_string).collect();

    // Aliases declared in rules.md H3 titles via `(alias: \`X\`)`.
    // The registry has no concept of "alias" — both canonical and
    // alias names are registered as independent builders that
    // happen to share an implementation. We harvest the alias
    // names from rules.md so the "registered but missing" check
    // below doesn't false-positive on aliases that ARE
    // documented, just under their canonical name's heading.
    let aliases: HashSet<String> = harvest_aliases(&src);

    let rules_dir = target_dir.join("rules");
    fs::create_dir_all(&rules_dir)?;

    let mut kind_to_family: HashMap<String, String> = HashMap::new();
    let mut all_kinds: Vec<KindEntry> = Vec::new();
    let mut family_summaries: Vec<FamilySummary> = Vec::new();

    let mut family_order: u32 = 0;
    for h2 in split_h2_sections(&src) {
        let lc = h2.title.to_lowercase();
        if lc == "contents" || lc.starts_with("bundled rulesets") {
            continue;
        }
        if lc.starts_with("fix operations") {
            emit_concept_page(target_dir, "fix-operations", "Fix operations", &h2.body)?;
            continue;
        }
        if lc.starts_with("nested") {
            emit_concept_page(
                target_dir,
                "nested-configs",
                "Nested .alint.yml (monorepo layering)",
                &h2.body,
            )?;
            continue;
        }

        family_order += 1;
        let family_slug = slugify(&h2.title);
        let family_dir = rules_dir.join(&family_slug);
        fs::create_dir_all(&family_dir)?;

        let family_rules = process_family_h3s(
            &h2,
            &family_dir,
            &family_slug,
            &known_kinds,
            &mut kind_to_family,
            &mut all_kinds,
        )?;

        emit_family_index(
            &family_dir,
            &h2.title,
            family_order,
            &family_slug,
            &family_rules,
        )?;
        family_summaries.push(FamilySummary {
            title: h2.title.clone(),
            slug: family_slug.clone(),
            rule_count: family_rules.len(),
        });
    }

    // Warn about any registered kind that rules.md doesn't
    // document. Aliases (declared inline in their canonical
    // H3's `(alias: …)`) are exempt — they ride on the
    // canonical page rather than getting their own.
    for kind in &known_kinds {
        if !kind_to_family.contains_key(kind) && !aliases.contains(kind) {
            eprintln!("[xtask] WARN: rule kind '{kind}' is registered but missing from rules.md");
        }
    }

    emit_rules_master_index(&rules_dir, &all_kinds, &family_summaries)?;
    Ok(kind_to_family)
}

/// Walk every H3 in a family, emit per-rule pages, and collect
/// the family's rule list for later index generation. Split out
/// of `generate_rules_pages` because clippy's `too_many_lines`
/// flagged the original — and even ignoring that, "process one
/// family" is its own logical chunk worth naming.
fn process_family_h3s(
    h2: &H2Section,
    family_dir: &Path,
    family_slug: &str,
    known_kinds: &std::collections::HashSet<String>,
    kind_to_family: &mut std::collections::HashMap<String, String>,
    all_kinds: &mut Vec<KindEntry>,
) -> Result<Vec<RuleEntry>> {
    let mut family_rules: Vec<RuleEntry> = Vec::new();
    let mut kind_order: u32 = 0;
    for h3 in split_h3_sections(&h2.body) {
        let mut group_kinds = extract_kinds(&h3.title);
        group_kinds.retain(|k| {
            if known_kinds.contains(k) {
                true
            } else {
                eprintln!(
                    "[xtask] WARN: rules.md heading '{}' mentions unknown rule kind '{}' — skipping",
                    h3.title, k
                );
                false
            }
        });
        if group_kinds.is_empty() {
            continue;
        }
        let summary = first_sentence(&h3.body);
        for kind in &group_kinds {
            kind_order += 1;
            let siblings: Vec<&str> = group_kinds
                .iter()
                .filter(|k| *k != kind)
                .map(String::as_str)
                .collect();
            emit_rule_page(
                family_dir,
                kind,
                family_slug,
                &h2.title,
                &h3.body,
                &siblings,
                kind_order,
            )?;
            kind_to_family.insert(kind.clone(), family_slug.to_string());
            family_rules.push(RuleEntry {
                kind: kind.clone(),
                summary: summary.clone(),
            });
            all_kinds.push(KindEntry {
                kind: kind.clone(),
                family_title: h2.title.clone(),
                family_slug: family_slug.to_string(),
                summary: summary.clone(),
            });
        }
    }
    Ok(family_rules)
}

#[derive(Clone)]
struct RuleEntry {
    kind: String,
    summary: String,
}

#[derive(Clone)]
struct KindEntry {
    kind: String,
    family_title: String,
    family_slug: String,
    summary: String,
}

struct FamilySummary {
    title: String,
    slug: String,
    rule_count: usize,
}

/// Sections of a markdown document split at H3 headers (`### …`).
/// Used inside an H2 body. Anything before the first H3 is
/// dropped (it's typically a family-level intro paragraph that
/// belongs on the family index, not on any rule's page).
struct H3Section {
    title: String,
    body: String,
}

fn split_h3_sections(src: &str) -> Vec<H3Section> {
    let mut sections: Vec<H3Section> = Vec::new();
    let mut current: Option<H3Section> = None;
    for line in src.lines() {
        if let Some(rest) = line.strip_prefix("### ") {
            if let Some(prev) = current.take() {
                sections.push(prev);
            }
            current = Some(H3Section {
                title: rest.trim().to_string(),
                body: String::new(),
            });
        } else if let Some(sec) = current.as_mut() {
            sec.body.push_str(line);
            sec.body.push('\n');
        }
    }
    if let Some(prev) = current.take() {
        sections.push(prev);
    }
    sections
}

/// Extract rule-kind tokens from an H3 title. Each kind name in
/// the heading is wrapped in single backticks; aliases live
/// inside `(alias: ...)` parens. Strip the parens, then collect
/// every backtick-delimited token that looks like a rule kind.
///
/// A multi-kind heading (the structured-query family's three
/// path-equals or path-matches kinds, comma-separated and
/// individually backticked) yields one kind per backtick'd
/// token. A single-kind heading yields one. Alias declarations
/// inside parens are skipped here and harvested separately by
/// [`harvest_aliases`].
fn extract_kinds(h3_title: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut paren_depth = 0i32;
    let mut in_backtick = false;
    let mut current = String::new();
    for ch in h3_title.chars() {
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = (paren_depth - 1).max(0),
            '`' if paren_depth == 0 => {
                if in_backtick {
                    if looks_like_kind(&current) {
                        out.push(current.clone());
                    }
                    current.clear();
                }
                in_backtick = !in_backtick;
            }
            c if in_backtick && paren_depth == 0 => current.push(c),
            _ => {}
        }
    }
    out
}

fn looks_like_kind(s: &str) -> bool {
    !s.is_empty()
        && s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Scan the entire rules.md source for the alias declarations
/// `(alias: ...)` (each name in single backticks) and collect the
/// alias names. Used to suppress "registered but missing"
/// warnings for aliases that share their canonical rule's page.
fn harvest_aliases(src: &str) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let needle = "alias:";
    let mut idx = 0;
    while let Some(pos) = src[idx..].find(needle) {
        let abs = idx + pos + needle.len();
        // After "alias:", skip whitespace, then expect a backtick-
        // delimited identifier. Multiple aliases per H3 aren't
        // currently used in rules.md but we'll handle them anyway.
        let mut cursor = abs;
        let bytes = src.as_bytes();
        while cursor < bytes.len() && (bytes[cursor] as char).is_whitespace() {
            cursor += 1;
        }
        if cursor < bytes.len() && bytes[cursor] == b'`' {
            cursor += 1;
            let start = cursor;
            while cursor < bytes.len() && bytes[cursor] != b'`' && bytes[cursor] != b'\n' {
                cursor += 1;
            }
            if cursor < bytes.len() && bytes[cursor] == b'`' {
                let name = &src[start..cursor];
                if looks_like_kind(name) {
                    out.insert(name.to_string());
                }
            }
        }
        idx = abs;
    }
    out
}

/// Heuristic one-liner for sidebar / index summaries. Takes the
/// first markdown paragraph of an H3 body, strips trailing
/// whitespace, takes up to the first sentence-ending `.`. Skips
/// blank lines / fenced code at the top.
fn first_sentence(body: &str) -> String {
    let mut paragraph = String::new();
    let mut in_code_block = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }
        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(trimmed);
    }
    if let Some(idx) = paragraph.find(". ") {
        paragraph.truncate(idx + 1);
    }
    paragraph.trim().to_string()
}

/// Render one `rules/<family>/<kind>.md` page. Frontmatter
/// `title` is the bare kind name so URLs and Starlight headings
/// match what the user types in `.alint.yml`. The page body is
/// the H3's content plus a "See also" footer for paired rules.
fn emit_rule_page(
    family_dir: &Path,
    kind: &str,
    family_slug: &str,
    family_title: &str,
    body: &str,
    siblings: &[&str],
    sidebar_order: u32,
) -> Result<()> {
    let mut page = String::new();
    let _ = writeln!(&mut page, "---");
    let _ = writeln!(&mut page, "title: '{kind}'");
    let _ = writeln!(
        &mut page,
        "description: 'alint rule kind `{kind}` ({family_title} family).'"
    );
    let _ = writeln!(&mut page, "sidebar:");
    let _ = writeln!(&mut page, "  order: {sidebar_order}");
    let _ = writeln!(&mut page, "---");
    let _ = writeln!(&mut page);
    page.push_str(body.trim_start_matches('\n'));
    if !siblings.is_empty() {
        // Trim trailing newlines so the footer doesn't have a
        // gaping gap above it.
        while page.ends_with("\n\n") {
            page.pop();
        }
        if !page.ends_with('\n') {
            page.push('\n');
        }
        let _ = writeln!(&mut page);
        let _ = writeln!(&mut page, "## See also");
        let _ = writeln!(&mut page);
        for sib in siblings {
            let _ = writeln!(&mut page, "- [`{sib}`](/docs/rules/{family_slug}/{sib}/)");
        }
    }
    if !page.ends_with('\n') {
        page.push('\n');
    }
    let dest = family_dir.join(format!("{kind}.md"));
    fs::write(&dest, page).with_context(|| format!("writing {}", dest.display()))?;
    Ok(())
}

/// Family overview: one paragraph on what the family is for plus
/// a flat table-of-contents linking to each kind. alint.org
/// references this page explicitly via a `link:` "Overview"
/// item in `astro.config.mjs` (it's NOT picked up by
/// autogenerate — the Rules section uses hand-rolled
/// sub-groups, see the comment over the Rules sidebar entry).
fn emit_family_index(
    family_dir: &Path,
    family_title: &str,
    family_order: u32,
    family_slug: &str,
    rules: &[RuleEntry],
) -> Result<()> {
    let mut page = String::new();
    let _ = writeln!(&mut page, "---");
    let _ = writeln!(&mut page, "title: '{}'", escape_yaml_string(family_title));
    let _ = writeln!(
        &mut page,
        "description: 'Rule reference: the {} family.'",
        family_title.to_lowercase()
    );
    let _ = writeln!(&mut page, "sidebar:");
    let _ = writeln!(&mut page, "  order: {family_order}");
    let _ = writeln!(&mut page, "  label: '{}'", escape_yaml_string(family_title));
    let _ = writeln!(&mut page, "---");
    let _ = writeln!(&mut page);
    let _ = writeln!(
        &mut page,
        "Rule kinds in the **{family_title}** family. Each entry below has its own page with options, an example, and any auto-fix support."
    );
    let _ = writeln!(&mut page);
    for r in rules {
        let _ = writeln!(
            &mut page,
            "- [`{kind}`](/docs/rules/{family_slug}/{kind}/) — {summary}",
            kind = r.kind,
            summary = r.summary
        );
    }
    fs::write(family_dir.join("index.md"), page)?;
    Ok(())
}

/// Master `/docs/rules/` page: alphabetical index of every
/// registered rule kind. This is the canonical "where do I find
/// rule X?" landing.
fn emit_rules_master_index(
    rules_dir: &Path,
    all_kinds: &[KindEntry],
    families: &[FamilySummary],
) -> Result<()> {
    let mut sorted: Vec<&KindEntry> = all_kinds.iter().collect();
    sorted.sort_by(|a, b| a.kind.cmp(&b.kind));

    let mut page = String::new();
    let _ = writeln!(&mut page, "---");
    let _ = writeln!(&mut page, "title: Rules");
    let _ = writeln!(
        &mut page,
        "description: Every rule kind alint ships, with one-line summaries and links to family + per-rule pages."
    );
    let _ = writeln!(&mut page, "sidebar:");
    let _ = writeln!(&mut page, "  order: 1");
    let _ = writeln!(&mut page, "  label: 'Index'");
    let _ = writeln!(&mut page, "---");
    let _ = writeln!(&mut page);
    let _ = writeln!(
        &mut page,
        "alint ships {kc} rule kinds across {fc} families. Each rule is one entry in your `.alint.yml` under `rules:`.",
        kc = all_kinds.len(),
        fc = families.len()
    );
    let _ = writeln!(&mut page);
    let _ = writeln!(&mut page, "## By family");
    let _ = writeln!(&mut page);
    for f in families {
        let _ = writeln!(
            &mut page,
            "- [{title}](/docs/rules/{slug}/) — {n} rule{plural}",
            title = f.title,
            slug = f.slug,
            n = f.rule_count,
            plural = if f.rule_count == 1 { "" } else { "s" }
        );
    }
    let _ = writeln!(&mut page);
    let _ = writeln!(&mut page, "## Alphabetical");
    let _ = writeln!(&mut page);
    for k in sorted {
        let _ = writeln!(
            &mut page,
            "- [`{kind}`](/docs/rules/{family}/{kind}/) — {summary} _({family_title})_",
            kind = k.kind,
            family = k.family_slug,
            family_title = k.family_title,
            summary = k.summary
        );
    }
    fs::write(rules_dir.join("index.md"), page)?;
    Ok(())
}

/// Emit a non-rule concept page (Fix operations, Nested
/// configs). Lives under `concepts/` rather than `rules/` so
/// the rules tree is purely about rule kinds.
fn emit_concept_page(target_dir: &Path, slug: &str, title: &str, body: &str) -> Result<()> {
    let dir = target_dir.join("concepts");
    fs::create_dir_all(&dir)?;
    let mut page = String::new();
    let _ = writeln!(&mut page, "---");
    let _ = writeln!(&mut page, "title: '{}'", escape_yaml_string(title));
    let _ = writeln!(
        &mut page,
        "description: 'alint concept: {}.'",
        title.to_lowercase()
    );
    let _ = writeln!(&mut page, "---");
    let _ = writeln!(&mut page);
    page.push_str(body.trim_start_matches('\n'));
    if !page.ends_with('\n') {
        page.push('\n');
    }
    fs::write(dir.join(format!("{slug}.md")), page)?;
    Ok(())
}

/// Sections of a markdown document split at H2 headers (`## …`).
/// Anything before the first H2 is dropped (it's typically the
/// document's H1 + intro paragraph; we don't carry that into the
/// per-family pages).
struct H2Section {
    title: String,
    body: String,
}

fn split_h2_sections(src: &str) -> Vec<H2Section> {
    let mut sections: Vec<H2Section> = Vec::new();
    let mut current: Option<H2Section> = None;
    for line in src.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            if let Some(prev) = current.take() {
                sections.push(prev);
            }
            current = Some(H2Section {
                title: rest.trim().to_string(),
                body: String::new(),
            });
        } else if let Some(sec) = current.as_mut() {
            sec.body.push_str(line);
            sec.body.push('\n');
        }
    }
    if let Some(prev) = current.take() {
        sections.push(prev);
    }
    sections
}

/// URL-safe slug from a heading. Lowercases, drops any character
/// that isn't `[a-z0-9-]`, collapses runs of `-`. Adequate for
/// headings like "Security / Unicode sanity" → "security-unicode-sanity".
fn slugify(s: &str) -> String {
    let lc = s.to_lowercase();
    let mut out = String::with_capacity(lc.len());
    let mut last_dash = false;
    for ch in lc.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Quote a string safely for a single-quoted YAML scalar — only
/// `'` needs escaping (doubled). Frontmatter titles like
/// `Security / Unicode sanity` need this.
fn escape_yaml_string(s: &str) -> String {
    s.replace('\'', "''")
}

/// One markdown page per `crates/alint-dsl/rulesets/v1/**/*.yml`,
/// summarising the ruleset's rules with their level / kind / message
/// / policy URL. Slash-separated names (`hygiene/lockfiles`,
/// `ci/github-actions`) are flattened with a `-` for the bundle
/// filename so Starlight's autogen sidebar produces a flat list.
///
/// Each page now also carries:
/// - An overview parsed from the YAML's leading comment block
///   (the natural-language description the ruleset author wrote
///   above `version: 1`).
/// - A `## Source` section with a link back to the canonical YAML
///   in the alint repo plus the full file embedded as a fenced
///   code block, so readers can see the exact rule definitions
///   without leaving the docs.
fn generate_bundled_ruleset_pages(
    workspace: &Path,
    target_dir: &Path,
    kind_to_family: &std::collections::HashMap<String, String>,
) -> Result<()> {
    struct RulesetEntry {
        pretty: String,
        flat_slug: String,
        summary: String,
    }

    let rulesets_root = workspace.join(docs_paths::RULESETS_DIR);
    let bundled_dir = target_dir.join("bundled-rulesets");
    fs::create_dir_all(&bundled_dir)?;

    let mut entries: Vec<RulesetEntry> = Vec::new();

    for entry in walkdir_plain(&rulesets_root)? {
        let md = fs::metadata(&entry)?;
        if !md.is_file() {
            continue;
        }
        let ext = entry.extension().and_then(|s| s.to_str()).unwrap_or("");
        if ext != "yml" && ext != "yaml" {
            continue;
        }
        let rel = entry.strip_prefix(&rulesets_root).unwrap();
        let pretty_name = rel.with_extension("");
        let pretty_str = pretty_name.to_string_lossy().replace('\\', "/");
        let flat_slug = pretty_str.replace('/', "-");
        let flat_filename = format!("{flat_slug}.md");

        let yaml_text =
            fs::read_to_string(&entry).with_context(|| format!("reading {}", entry.display()))?;
        let yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml_text)
            .with_context(|| format!("parsing {}", entry.display()))?;

        // Repo-relative path for the source link, always forward-
        // slashed regardless of host OS so the URL is portable.
        let rel_path_str = rel.to_string_lossy().replace('\\', "/");
        let rel_repo_path = format!("{}/{}", docs_paths::RULESETS_DIR, rel_path_str);

        let overview_md = render_overview_from_comments(&yaml_text);
        let summary = first_overview_sentence(&overview_md);

        let page = render_ruleset_page(
            &pretty_str,
            &overview_md,
            &yaml_text,
            &rel_repo_path,
            &yaml,
            kind_to_family,
        );
        let dest = bundled_dir.join(&flat_filename);
        fs::write(&dest, page).with_context(|| format!("writing {}", dest.display()))?;

        entries.push(RulesetEntry {
            pretty: pretty_str,
            flat_slug,
            summary,
        });
    }

    // An index page listing every ruleset — overwrites the hand-
    // written placeholder when the sync script lays the bundle into
    // alint.org. Each entry shows a one-line summary (the first
    // sentence of the ruleset's leading comment block) so the
    // index is scannable without opening every page.
    entries.sort_by(|a, b| a.pretty.cmp(&b.pretty));
    let mut index = String::new();
    let _ = writeln!(&mut index, "---");
    let _ = writeln!(&mut index, "title: Bundled Rulesets");
    let _ = writeln!(
        &mut index,
        "description: One-line ecosystem baselines built into the alint binary."
    );
    let _ = writeln!(&mut index, "sidebar:");
    let _ = writeln!(&mut index, "  order: 1");
    let _ = writeln!(&mut index, "---");
    let _ = writeln!(&mut index);
    let _ = writeln!(
        &mut index,
        "Adopt with `extends: [alint://bundled/<name>@v1]`. Each ruleset's full rule list lives on its own page below."
    );
    let _ = writeln!(&mut index);
    let _ = writeln!(&mut index, "## Currently shipped");
    let _ = writeln!(&mut index);
    for e in &entries {
        if e.summary.is_empty() {
            let _ = writeln!(
                &mut index,
                "- [`{name}@v1`](/docs/bundled-rulesets/{slug}/)",
                name = e.pretty,
                slug = e.flat_slug
            );
        } else {
            let _ = writeln!(
                &mut index,
                "- [`{name}@v1`](/docs/bundled-rulesets/{slug}/) — {summary}",
                name = e.pretty,
                slug = e.flat_slug,
                summary = e.summary,
            );
        }
    }
    fs::write(bundled_dir.join("index.md"), index)?;

    Ok(())
}

/// GitHub repo-relative base for source-of-truth links rendered
/// into the bundled-ruleset pages. Pinned to `main` so readers
/// always land on the latest version of each ruleset; the page
/// also embeds a verbatim snapshot of the YAML below the link
/// for offline / point-in-time reference.
const ALINT_REPO_BLOB_URL: &str = "https://github.com/asamarts/alint/blob/main";

/// Render the markdown body for a single bundled ruleset. The
/// page has four sections, in order:
///
/// 1. **Overview** — the leading comment block from the YAML,
///    rendered as natural-language prose (with any inline YAML
///    code samples promoted to fenced ```yaml``` blocks for
///    syntax highlighting).
/// 2. **Adopt with** — a copy-pasteable `extends:` snippet. We
///    suppress this when the overview already contains an
///    `alint://bundled/...` reference (that's the layered-overlay
///    case where the comment author specifies a multi-ruleset
///    adoption recipe — auto-generating a single-line snippet on
///    top would be redundant and incorrect).
/// 3. **Rules** — table-of-contents-style list of every `id` in
///    the ruleset with kind / level / when / policy / message
///    pulled from the YAML. Each `kind` is a link into the rule
///    reference (`/docs/rules/<family>/<kind>/`) when
///    `kind_to_family` knows about it.
/// 4. **Source** — a permalink into the alint repo plus the full
///    YAML file embedded as a fenced code block.
///
/// `kind_to_family` is consulted to render each rule's `kind` as
/// a link into the rules tree. Kinds not in the map (e.g. a
/// brand-new kind missing from rules.md) render as plain code;
/// the rules-pages generator emits a warning in that case so the
/// gap surfaces.
fn render_ruleset_page(
    name: &str,
    overview_md: &str,
    yaml_text: &str,
    rel_repo_path: &str,
    yaml: &serde_yaml_ng::Value,
    kind_to_family: &std::collections::HashMap<String, String>,
) -> String {
    let mut out = String::new();
    let _ = writeln!(&mut out, "---");
    let _ = writeln!(&mut out, "title: '{name}@v1'");
    let _ = writeln!(
        &mut out,
        "description: Bundled alint ruleset at alint://bundled/{name}@v1."
    );
    let _ = writeln!(&mut out, "---");
    let _ = writeln!(&mut out);

    if !overview_md.is_empty() {
        out.push_str(overview_md);
        let _ = writeln!(&mut out);
        let _ = writeln!(&mut out);
    }

    // The overlay-style rulesets (e.g. monorepo/cargo-workspace)
    // already document a multi-ruleset `extends:` recipe in their
    // leading comment. Re-rendering a single-line snippet under
    // them would be both redundant and misleading, so we suppress
    // the auto-gen Adopt-with whenever the overview already
    // mentions the bundled-URI scheme.
    let overview_has_adoption = overview_md.contains("alint://bundled/");
    if !overview_has_adoption {
        let _ = writeln!(&mut out, "## Adopt with");
        let _ = writeln!(&mut out);
        let _ = writeln!(&mut out, "```yaml");
        let _ = writeln!(&mut out, "extends:");
        let _ = writeln!(&mut out, "  - alint://bundled/{name}@v1");
        let _ = writeln!(&mut out, "```");
        let _ = writeln!(&mut out);
    }

    if let Some(rules) = yaml.get("rules").and_then(|r| r.as_sequence()) {
        let _ = writeln!(&mut out, "## Rules");
        let _ = writeln!(&mut out);

        for rule in rules {
            let id = rule.get("id").and_then(|v| v.as_str()).unwrap_or("(no-id)");
            let kind = rule.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let level = rule.get("level").and_then(|v| v.as_str()).unwrap_or("");
            let when = rule.get("when").and_then(|v| v.as_str());
            let msg = rule.get("message").and_then(|v| v.as_str());
            let policy = rule.get("policy_url").and_then(|v| v.as_str());

            let _ = writeln!(&mut out, "### `{id}`");
            let _ = writeln!(&mut out);
            if !kind.is_empty() {
                let kind_md = match kind_to_family.get(kind) {
                    Some(family) => {
                        format!("[`{kind}`](/docs/rules/{family}/{kind}/)")
                    }
                    None => format!("`{kind}`"),
                };
                let _ = writeln!(&mut out, "- **kind**: {kind_md}");
            }
            if !level.is_empty() {
                let _ = writeln!(&mut out, "- **level**: `{level}`");
            }
            if let Some(when) = when {
                let _ = writeln!(&mut out, "- **when**: `{when}`");
            }
            if let Some(policy) = policy {
                let _ = writeln!(&mut out, "- **policy**: <{policy}>");
            }
            if let Some(msg) = msg {
                let _ = writeln!(&mut out);
                let _ = writeln!(&mut out, "> {}", msg.replace('\n', " "));
            }
            let _ = writeln!(&mut out);
        }
    } else {
        let _ = writeln!(&mut out, "_(no rules — this ruleset is a placeholder.)_");
        let _ = writeln!(&mut out);
    }

    let _ = writeln!(&mut out, "## Source");
    let _ = writeln!(&mut out);
    let _ = writeln!(
        &mut out,
        "The full ruleset definition is committed at \
         [`{rel_repo_path}`]({ALINT_REPO_BLOB_URL}/{rel_repo_path}) in the alint repo \
         (the snapshot below is generated verbatim from that file).",
    );
    let _ = writeln!(&mut out);
    let _ = writeln!(&mut out, "```yaml");
    out.push_str(yaml_text.trim_end_matches('\n'));
    out.push('\n');
    let _ = writeln!(&mut out, "```");
    out
}

/// Parse the leading comment block of a ruleset YAML into
/// markdown. The first line is expected to be the canonical
/// `# alint://bundled/<name>@<rev>` URI tag and is stripped.
/// Subsequent comment lines are emitted as paragraphs (preserving
/// the author's line breaks so list items render correctly) or,
/// when a paragraph starts with a 4-space indent, as a fenced
/// `yaml` code block — that's the convention authors use for
/// the "here's the `extends:` snippet" mini-blocks.
///
/// Reading stops at the first non-comment, non-blank line. By
/// convention the rule body starts right after the leading
/// comment block, so this naturally captures only the file's
/// top-of-file description.
fn render_overview_from_comments(yaml_text: &str) -> String {
    enum Block {
        Para(Vec<String>),
        Code(Vec<String>),
    }

    let mut blocks: Vec<Block> = Vec::new();
    let mut comment_started = false;
    // Treat start-of-input as a paragraph break so the very first
    // non-blank comment line opens a new block.
    let mut paragraph_break = true;

    for raw in yaml_text.lines() {
        let line = raw.trim_end();
        if line.is_empty() {
            if !comment_started {
                continue;
            }
            // A literal blank line (no `#`) ends the leading
            // comment block. Authors use blank `#` lines for
            // paragraph breaks INSIDE the block — those are
            // handled below.
            break;
        }
        if !line.starts_with('#') {
            break;
        }
        comment_started = true;

        // Strip the `#` marker and exactly one trailing space.
        let after_hash = &line[1..];
        let body = after_hash.strip_prefix(' ').unwrap_or(after_hash);

        if body.is_empty() {
            paragraph_break = true;
            continue;
        }
        // Skip the canonical `# alint://bundled/<name>@<rev>` URI
        // header — it's metadata, not prose.
        if body.starts_with("alint://bundled/") {
            continue;
        }

        // 4-space indent at the START of a block = code block.
        // Continuation lines inside an existing block keep that
        // block's kind regardless of their own indent (so
        // bulleted lists with hanging-indent continuations
        // stay in a single Para block).
        if paragraph_break {
            if let Some(rest) = body.strip_prefix("    ") {
                blocks.push(Block::Code(vec![rest.to_string()]));
            } else {
                blocks.push(Block::Para(vec![body.to_string()]));
            }
        } else {
            match blocks
                .last_mut()
                .expect("paragraph_break=false implies a current block exists")
            {
                Block::Para(lines) => lines.push(body.to_string()),
                Block::Code(lines) => {
                    // Inside a code block, dedent up to 4 spaces so
                    // the rendered code matches the opening line's
                    // visual indentation.
                    let dedented = body.strip_prefix("    ").unwrap_or(body);
                    lines.push(dedented.to_string());
                }
            }
        }
        paragraph_break = false;
    }

    let mut out = String::new();
    for (i, b) in blocks.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        match b {
            Block::Para(lines) => out.push_str(&lines.join("\n")),
            Block::Code(lines) => {
                out.push_str("```yaml\n");
                for l in lines {
                    out.push_str(l);
                    out.push('\n');
                }
                out.push_str("```");
            }
        }
    }
    out
}

/// First-sentence summary of a rendered overview, used to
/// populate the bundled-rulesets index page. Skips fenced code
/// blocks, takes the first paragraph of natural-language prose,
/// and truncates at the first sentence-ending `. ` boundary.
fn first_overview_sentence(overview_md: &str) -> String {
    let mut paragraph = String::new();
    let mut in_code = false;
    for line in overview_md.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }
        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(trimmed);
    }
    if let Some(idx) = paragraph.find(". ") {
        paragraph.truncate(idx + 1);
    }
    paragraph.trim().to_string()
}

/// Build the alint binary in release mode, then capture
/// `alint --help` and `alint <subcmd> --help` for each subcommand.
/// Each captured help text becomes its own markdown page under
/// `cli/<subcmd>.md`.
fn generate_cli_reference(workspace: &Path, target_dir: &Path) -> Result<()> {
    let bin = build_release_binary()?;

    let cli_dir = target_dir.join("cli");
    fs::create_dir_all(&cli_dir)?;

    // Top-level help → cli/index.md
    let top = run_help(&bin, &[])?;
    let mut index = String::new();
    let _ = writeln!(&mut index, "---");
    let _ = writeln!(&mut index, "title: CLI");
    let _ = writeln!(
        &mut index,
        "description: alint's subcommands and global flags, captured from the binary itself."
    );
    let _ = writeln!(&mut index, "sidebar:");
    let _ = writeln!(&mut index, "  order: 1");
    let _ = writeln!(&mut index, "---");
    let _ = writeln!(&mut index);
    let _ = writeln!(&mut index, "```");
    index.push_str(&top);
    let _ = writeln!(&mut index, "```");
    fs::write(cli_dir.join("index.md"), index)?;

    let subcmds = ["check", "fix", "list", "explain", "facts"];
    for sub in subcmds {
        let help = run_help(&bin, &[sub])?;
        let mut page = String::new();
        let _ = writeln!(&mut page, "---");
        let _ = writeln!(&mut page, "title: 'alint {sub}'");
        let _ = writeln!(
            &mut page,
            "description: '`alint {sub}` — captured from `alint {sub} --help`.'"
        );
        let _ = writeln!(&mut page, "---");
        let _ = writeln!(&mut page);
        let _ = writeln!(&mut page, "```");
        page.push_str(&help);
        let _ = writeln!(&mut page, "```");
        fs::write(cli_dir.join(format!("{sub}.md")), page)?;
    }

    // Sanity-check: workspace path exists.
    let _ = workspace;
    Ok(())
}

fn run_help(bin: &Path, subcmd_args: &[&str]) -> Result<String> {
    let mut cmd = Command::new(bin);
    cmd.args(subcmd_args).arg("--help");
    let out = cmd.output().with_context(|| format!("running {cmd:?}"))?;
    if !out.status.success() {
        bail!(
            "alint {:?} --help exited {:?}",
            subcmd_args,
            out.status.code()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn write_manifest(target_dir: &Path) -> Result<()> {
    let sha = git_sha().unwrap_or_else(|| "unknown".to_string());
    let version = env!("CARGO_PKG_VERSION");
    let now = now_iso();

    let json = format!(
        "{{\n  \"alint_version\": \"{version}\",\n  \"git_sha\": \"{sha}\",\n  \"generated_at\": \"{now}\",\n  \"format_version\": 1\n}}\n"
    );
    fs::write(target_dir.join("manifest.json"), json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_strips_uri_header_and_renders_paragraphs() {
        let yaml = "\
# alint://bundled/oss-baseline@v1
#
# A minimal OSS-hygiene baseline — what most repos follow.
# Multi-line prose stays on one paragraph.
#
# Second paragraph here.

version: 1
rules: []
";
        let out = render_overview_from_comments(yaml);
        assert!(!out.contains("alint://bundled/oss-baseline@v1"));
        assert!(out.contains("A minimal OSS-hygiene baseline"));
        assert!(out.contains("Multi-line prose stays on one paragraph."));
        assert!(out.contains("Second paragraph here."));
        // Two paragraphs separated by a blank line.
        assert!(out.contains("paragraph.\n\nSecond"));
    }

    #[test]
    fn overview_promotes_indented_block_to_fenced_yaml() {
        let yaml = "\
# alint://bundled/oss-baseline@v1
#
# Adopt it with:
#
#     extends:
#       - alint://bundled/oss-baseline@v1
#
# Trailing prose.

version: 1
";
        let out = render_overview_from_comments(yaml);
        assert!(out.contains("```yaml\nextends:\n  - alint://bundled/oss-baseline@v1\n```"));
        assert!(out.contains("Trailing prose."));
    }

    #[test]
    fn overview_keeps_bulleted_lists_with_hanging_indent() {
        // Bulleted lists with 4-space hanging-indent continuations
        // (the ci/github-actions style) must NOT be split into
        // separate code blocks. They stay as one Para block so
        // CommonMark renders them as a list with continuation.
        let yaml = "\
# alint://bundled/ci/github-actions@v1
#
# GitHub Actions hardening:
#
#   - \"Token-Permissions\" — declare scope explicitly
#     at workflow level (or narrower).
#   - \"Pinned-Dependencies\" — third-party actions pinned
#     to commit SHAs.

version: 1
";
        let out = render_overview_from_comments(yaml);
        // The hanging-indent continuation must NOT trigger a
        // code-block fence in the middle of the list.
        assert!(
            !out.contains("```yaml"),
            "got unexpected code fence in:\n{out}"
        );
        assert!(out.contains("  - \"Token-Permissions\""));
        assert!(out.contains("    at workflow level (or narrower)."));
    }

    #[test]
    fn overview_stops_at_yaml_body() {
        // Reading must stop at the first non-comment, non-blank
        // line (the YAML body).
        let yaml = "\
# alint://bundled/x@v1
#
# Description goes here.

version: 1
# This is a comment INSIDE the body, not part of the overview.
rules:
  # Inline rule comment, also not part of the overview.
  - id: foo
";
        let out = render_overview_from_comments(yaml);
        assert!(out.contains("Description goes here."));
        assert!(!out.contains("INSIDE the body"));
        assert!(!out.contains("Inline rule comment"));
    }

    #[test]
    fn overview_handles_no_leading_comments() {
        let yaml = "version: 1\nrules: []\n";
        assert_eq!(render_overview_from_comments(yaml), "");
    }

    #[test]
    fn first_overview_sentence_truncates_at_period() {
        let s =
            first_overview_sentence("Hygiene checks for Go modules. Adopt with the snippet below.");
        assert_eq!(s, "Hygiene checks for Go modules.");
    }

    #[test]
    fn first_overview_sentence_skips_code_blocks() {
        let s = first_overview_sentence(
            "Lockfile discipline: one per workspace.\n\n\
             ```yaml\nextends: []\n```\n\n\
             Second paragraph.",
        );
        assert_eq!(s, "Lockfile discipline: one per workspace.");
    }
}
