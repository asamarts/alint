//! `--docker` wrapper for `bench-scale`.
//!
//! Re-execs the bench inside the published `alint-bench` image
//! so cross-machine results are directly comparable: every
//! competitor tool's version is fixed by the image tag, the
//! Rust toolchain is fixed, and the benchmark fingerprint
//! captures `ALINT_BENCH_DOCKER=1` so reports made via this
//! path are flagged as such.
//!
//! The wrapper is intentionally thin — it doesn't reinterpret
//! the user's args, only forwards them into the container
//! verbatim. The image's entrypoint is `xtask bench-scale`,
//! so the inner invocation looks just like a host run.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::Tool;

/// Bare-bones mirror of the bench-scale CLI args. We forward
/// these into the container's `xtask bench-scale ...`
/// invocation; the inner xtask does its own parsing.
#[derive(Debug, Clone)]
pub struct ForwardedArgs {
    pub sizes: Vec<String>,
    pub include_1m: bool,
    pub scenarios: Vec<String>,
    pub modes: Vec<String>,
    pub tools: Vec<String>,
    pub warmup: u32,
    pub runs: u32,
    pub seed: u64,
    pub diff_pct: f64,
    pub out: Option<PathBuf>,
    pub quick: bool,
    pub json_only: bool,
}

/// Run `bench-scale` inside the published `alint-bench` image.
///
/// Image tag defaults to `ghcr.io/asamarts/alint-bench:<workspace
/// version>` (e.g. `:0.5.7`). Override via the
/// `ALINT_BENCH_IMAGE` environment variable for ad-hoc tags
/// (`:edge`, locally-built, etc).
pub fn run_in_docker(args: &ForwardedArgs) -> Result<()> {
    ensure_docker()?;

    // Resolve tools host-side so missing-on-PATH detection
    // produces a clean error message before we spend time
    // pulling the image. The container's xtask will redo
    // detection inside, against the image's installed set.
    super::tools::resolve(&args.tools)?;

    let workspace = crate::workspace_root()?;
    let image = std::env::var("ALINT_BENCH_IMAGE")
        .unwrap_or_else(|_| format!("ghcr.io/asamarts/alint-bench:{}", env!("CARGO_PKG_VERSION")));

    eprintln!("[xtask] bench-scale --docker → image={image}");

    let mut cmd = Command::new("docker");
    cmd.args(["run", "--rm", "--init"]);

    // Match host UID/GID so any files written under the bind
    // mount (results.json, .alint.yml inside generated trees,
    // etc.) end up owned by the user, not root. macOS Docker
    // Desktop ignores `--user` for bind mounts but doesn't
    // error on it; Linux honours it as expected. Windows is
    // out of scope: the image is linux/amd64.
    if cfg!(unix) {
        if let Some((uid, gid)) = host_uid_gid() {
            cmd.arg("--user").arg(format!("{uid}:{gid}"));
        }
    }

    cmd.arg("-v").arg(format!("{}:/work", workspace.display()));
    // Cargo target dir lives on a named volume so the host's
    // `target/` (often gigabytes of incremental artefacts) isn't
    // shadowed and the container's release rebuild persists
    // across runs.
    cmd.arg("-v").arg("alint-bench-cargo-target:/cargo-target");
    cmd.arg(&image);

    // Forwarded args. The image's ENTRYPOINT is
    //   `cargo run -p xtask --release -- bench-scale`
    // so everything after the image name flows in as
    // `bench-scale <args>`.
    cmd.args(forward(args, &workspace));

    let status = cmd.status().context("invoking docker run")?;
    if !status.success() {
        bail!("docker run failed (exit {:?})", status.code());
    }
    Ok(())
}

fn forward(args: &ForwardedArgs, workspace: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.push("--sizes".into());
    out.push(args.sizes.join(","));
    if args.include_1m {
        out.push("--include-1m".into());
    }
    out.push("--scenarios".into());
    out.push(args.scenarios.join(","));
    out.push("--modes".into());
    out.push(args.modes.join(","));
    out.push("--tools".into());
    out.push(canonicalise_tools(&args.tools));
    out.push("--warmup".into());
    out.push(args.warmup.to_string());
    out.push("--runs".into());
    out.push(args.runs.to_string());
    out.push("--seed".into());
    out.push(format!("{:#x}", args.seed));
    out.push("--diff-pct".into());
    out.push(args.diff_pct.to_string());
    if args.quick {
        out.push("--quick".into());
    }
    if args.json_only {
        out.push("--json-only".into());
    }
    if let Some(p) = &args.out {
        out.push("--out".into());
        out.push(translate_out_path(p, workspace));
    }
    out
}

/// Translate a host `--out` path into the container view. Paths
/// under the workspace are rebased onto `/work`; absolute paths
/// outside the workspace are passed through unchanged (and the
/// user is responsible for mounting them — documented in the
/// methodology page).
fn translate_out_path(p: &Path, workspace: &Path) -> String {
    if let Ok(rel) = p.strip_prefix(workspace) {
        Path::new("/work").join(rel).to_string_lossy().to_string()
    } else {
        p.to_string_lossy().to_string()
    }
}

/// `--tools` may have been entered as a single comma-separated
/// string ("alint,grep") or as a Vec produced by clap's
/// `value_delimiter = ','`. Either way, normalise to canonical
/// tool names so the in-container xtask sees a known set.
fn canonicalise_tools(specs: &[String]) -> String {
    if specs.iter().any(|s| s.eq_ignore_ascii_case("all")) {
        return "all".into();
    }
    let parsed: Vec<&'static str> = specs
        .iter()
        .filter_map(|s| Tool::parse(s).ok().map(Tool::name))
        .collect();
    parsed.join(",")
}

fn ensure_docker() -> Result<()> {
    match Command::new("docker").arg("--version").output() {
        Ok(out) if out.status.success() => Ok(()),
        _ => bail!(
            "docker not found in PATH — install Docker (or Docker Desktop) and \
             re-run, or drop --docker for a host-native bench"
        ),
    }
}

fn host_uid_gid() -> Option<(String, String)> {
    let uid = run_trim("id", &["-u"])?;
    let gid = run_trim("id", &["-g"])?;
    Some((uid, gid))
}

fn run_trim(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
