//! `xtask bench-release` — the quick / publish-grade local
//! benchmark runner. See main.rs for the CLI surface.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

// ---- bench-release ---------------------------------------------------------

const RULES_CONFIG_YAML: &str = include_str!("bench_config.yml");

pub(crate) fn bench_release(quick: bool, out: Option<PathBuf>, seed: u64) -> Result<()> {
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

pub(crate) fn ensure_hyperfine() -> Result<()> {
    match Command::new("hyperfine").arg("--version").output() {
        Ok(out) if out.status.success() => Ok(()),
        _ => bail!(
            "hyperfine not found in PATH. Install it with:\n  \
             cargo install hyperfine\n  # or apt/brew/choco install hyperfine"
        ),
    }
}

pub(crate) fn build_release_binary() -> Result<PathBuf> {
    eprintln!("[xtask] cargo build --release -p alint");
    let out = Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "-p",
            "alint",
            "--message-format=json-render-diagnostics",
        ])
        .stderr(std::process::Stdio::inherit())
        .output()
        .context("invoking cargo")?;
    if !out.status.success() {
        bail!("release build failed");
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let bin = parse_cargo_artifact_executable(&stdout, "alint").with_context(
        || "cargo build --release -p alint succeeded without emitting a binary artifact path",
    )?;
    if !bin.is_file() {
        bail!("expected binary at {}", bin.display());
    }
    Ok(bin)
}

/// Extract the compiled binary executable path from `cargo build --message-format=json*`
/// stdout so `CARGO_TARGET_DIR` and `.cargo/config.toml` `build.target-dir` are honored.
pub(crate) fn parse_cargo_artifact_executable(stdout: &str, bin_name: &str) -> Option<PathBuf> {
    let mut found = None;
    for line in stdout.lines() {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if msg.get("reason").and_then(|v| v.as_str()) != Some("compiler-artifact") {
            continue;
        }
        let target = msg.get("target")?;
        if target.get("name").and_then(|v| v.as_str()) != Some(bin_name) {
            continue;
        }
        let is_bin = target
            .get("kind")
            .and_then(|v| v.as_array())
            .is_some_and(|kinds| kinds.iter().any(|k| k.as_str() == Some("bin")));
        if !is_bin {
            continue;
        }
        if let Some(exe) = msg.get("executable").and_then(|v| v.as_str()) {
            found = Some(PathBuf::from(exe));
        }
    }
    found
}

pub(crate) fn workspace_root() -> Result<PathBuf> {
    // xtask is inside the workspace; CARGO_MANIFEST_DIR = alint/xtask; parent = workspace root.
    let manifest = env!("CARGO_MANIFEST_DIR");
    let root = Path::new(manifest)
        .parent()
        .context("xtask has no parent directory")?;
    Ok(root.to_path_buf())
}

pub(crate) fn run_hyperfine(
    binary: &Path,
    tree_root: &Path,
    size: usize,
    quick: bool,
) -> Result<String> {
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

pub(crate) fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub(crate) fn write_header(report: &mut String, quick: bool, seed: u64) -> Result<()> {
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

pub(crate) fn rustc_version() -> Option<String> {
    let out = Command::new("rustc").arg("--version").output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

pub(crate) fn git_sha() -> Option<String> {
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

pub(crate) fn now_iso() -> String {
    // Minimal ISO-ish timestamp without pulling in chrono.
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!("unix:{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cargo_artifact_executable_respects_custom_target_dir() {
        let stdout = concat!(
            r#"{"reason":"compiler-artifact","target":{"name":"alint","kind":["lib"]},"executable":null}"#,
            "\n",
            r#"{"reason":"compiler-artifact","target":{"name":"alint","kind":["bin"]},"executable":"/cargo-target/release/alint"}"#,
            "\n",
            r#"{"reason":"build-finished","success":true}"#,
            "\n"
        );
        assert_eq!(
            parse_cargo_artifact_executable(stdout, "alint"),
            Some(PathBuf::from("/cargo-target/release/alint"))
        );
        assert_eq!(parse_cargo_artifact_executable(stdout, "other"), None);
    }

    #[test]
    fn shell_quote_escapes_spaces_dollar_quotes_and_metacharacters() {
        assert_eq!(shell_quote("/tmp/simple"), "'/tmp/simple'");
        assert_eq!(shell_quote("/tmp/a b/$HOME;rm"), "'/tmp/a b/$HOME;rm'");
        assert_eq!(shell_quote("it's \"quoted\""), "'it'\\''s \"quoted\"'");
    }
}
