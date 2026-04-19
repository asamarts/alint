//! `xtask` — ancillary helpers for alint that don't belong in the shipped binary.
//!
//! Current commands:
//!
//! - `bench-release` — builds alint in release mode, generates deterministic
//!   synthetic trees, runs `hyperfine` across a tree-size × rule-count
//!   matrix, and emits a platform-fingerprinted markdown report. Used to
//!   produce the numbers published in `docs/benchmarks/<version>/`.
//! - `gen-fixture`   — materialize a synthetic tree for ad-hoc experimentation.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask", about = "alint developer helpers")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build alint in release mode and run hyperfine across a tree × rules matrix.
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::BenchRelease { quick, out, seed } => bench_release(quick, out, seed),
        Commands::GenFixture {
            files,
            depth,
            seed,
            out,
        } => gen_fixture(files, depth, seed, out),
    }
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
        .args(["build", "--release", "-p", "alint-cli"])
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
