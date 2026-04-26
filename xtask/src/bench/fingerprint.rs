//! Hardware + tool-version capture for benchmark reports.
//!
//! Cross-machine variance is the elephant in every benchmark
//! room; the fix is to record enough about the machine that
//! readers can decide whether two reports are comparable. We
//! capture: OS / arch / kernel, CPU model + cores, total RAM,
//! filesystem type of the bench-tree mount, alint + hyperfine +
//! rustc versions, git SHA, and an ISO-ish timestamp. Each is
//! best-effort: failures degrade to a `"<unknown>"` placeholder
//! rather than aborting the run.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::tools::Tool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fingerprint {
    pub os: String,
    pub arch: String,
    pub kernel: String,
    pub cpu_model: String,
    pub cpu_cores: u32,
    pub ram_gb: u32,
    pub fs_type: String,
    pub rustc: String,
    pub alint_version: String,
    pub alint_git_sha: String,
    pub hyperfine_version: String,
    /// Versions of every benchmarked tool present on the
    /// machine, keyed by `Tool::name()` (`"alint"`, `"ls-lint"`,
    /// …). Built from the `--tools` set, so a 0.5.6-shape
    /// alint-only run produces just `{"alint": "0.5.6"}` here
    /// — the field is always present, never empty when at
    /// least one tool ran. Sorted via `BTreeMap` for
    /// deterministic JSON output.
    pub tool_versions: BTreeMap<String, String>,
    pub timestamp: String,
}

/// Best-effort hardware + tool-version capture. Every
/// component degrades to `"<unknown>"` (or `0`) on failure
/// rather than aborting; benchmark publication should never
/// die just because we couldn't read `/proc/meminfo`.
pub fn capture(tools: &[Tool]) -> Fingerprint {
    let tool_versions: BTreeMap<String, String> = tools
        .iter()
        .filter_map(|t| t.detect().map(|v| (t.name().to_string(), v)))
        .collect();
    Fingerprint {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        kernel: kernel_version().unwrap_or_else(unknown),
        cpu_model: cpu_model().unwrap_or_else(unknown),
        cpu_cores: cpu_cores().unwrap_or(0),
        ram_gb: ram_gb().unwrap_or(0),
        fs_type: tmpdir_fs_type().unwrap_or_else(unknown),
        rustc: rustc_version().unwrap_or_else(unknown),
        alint_version: alint_version().unwrap_or_else(unknown),
        alint_git_sha: alint_git_sha().unwrap_or_else(unknown),
        hyperfine_version: hyperfine_version().unwrap_or_else(unknown),
        tool_versions,
        timestamp: timestamp(),
    }
}

fn unknown() -> String {
    "<unknown>".to_string()
}

// ─── Per-component helpers ───────────────────────────────────────────

fn kernel_version() -> Option<String> {
    run_capturing("uname", &["-sr"]).map(|s| s.trim().to_string())
}

fn cpu_model() -> Option<String> {
    if cfg!(target_os = "linux") {
        let cpuinfo = fs::read_to_string("/proc/cpuinfo").ok()?;
        for line in cpuinfo.lines() {
            if let Some(rest) = line.strip_prefix("model name") {
                // `/proc/cpuinfo` lines look like:
                //   model name      : AMD Ryzen 9 3900X 12-Core Processor
                // After stripping the "model name" prefix we still
                // have leading whitespace + `:` + value. Trim both.
                let value = rest
                    .trim_start_matches([' ', '\t'])
                    .trim_start_matches(':')
                    .trim();
                return Some(value.to_string());
            }
        }
        None
    } else if cfg!(target_os = "macos") {
        run_capturing("sysctl", &["-n", "machdep.cpu.brand_string"])
    } else {
        // Windows + others — best-effort via wmic on Windows.
        run_capturing("wmic", &["cpu", "get", "name", "/value"]).map(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("Name="))
                .unwrap_or(&s)
                .trim()
                .to_string()
        })
    }
}

fn cpu_cores() -> Option<u32> {
    if cfg!(target_os = "linux") {
        let cpuinfo = fs::read_to_string("/proc/cpuinfo").ok()?;
        let count = cpuinfo
            .lines()
            .filter(|l| l.starts_with("processor"))
            .count();
        u32::try_from(count).ok()
    } else if cfg!(target_os = "macos") {
        run_capturing("sysctl", &["-n", "hw.ncpu"]).and_then(|s| s.trim().parse().ok())
    } else {
        std::thread::available_parallelism()
            .ok()
            .and_then(|n| u32::try_from(n.get()).ok())
    }
}

fn ram_gb() -> Option<u32> {
    if cfg!(target_os = "linux") {
        let meminfo = fs::read_to_string("/proc/meminfo").ok()?;
        for line in meminfo.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
                return u32::try_from(kb / 1024 / 1024).ok();
            }
        }
        None
    } else if cfg!(target_os = "macos") {
        let bytes: u64 = run_capturing("sysctl", &["-n", "hw.memsize"])?
            .trim()
            .parse()
            .ok()?;
        u32::try_from(bytes / 1024 / 1024 / 1024).ok()
    } else {
        None
    }
}

fn tmpdir_fs_type() -> Option<String> {
    let tmp = std::env::temp_dir();
    tmp_fs_type_for(&tmp)
}

fn tmp_fs_type_for(p: &Path) -> Option<String> {
    if cfg!(target_os = "linux") {
        // findmnt -no FSTYPE -T <path>  → "ext4", "tmpfs", etc.
        run_capturing(
            "findmnt",
            &["-n", "-o", "FSTYPE", "-T", p.to_str().unwrap_or("/")],
        )
        .map(|s| s.trim().to_string())
    } else if cfg!(target_os = "macos") {
        // diskutil info / mount; cheaper to call `df -T` on
        // Linux but mac df doesn't have -T. Use `mount` + grep.
        let out = run_capturing("mount", &[])?;
        for line in out.lines() {
            // Format: "/dev/disk1s5 on / (apfs, local, …)"
            if line.contains(" on / ") || line.contains(" on /private/tmp ") {
                if let Some(start) = line.find(" (") {
                    let rest = &line[start + 2..];
                    if let Some(end) = rest.find(',').or_else(|| rest.find(')')) {
                        return Some(rest[..end].trim().to_string());
                    }
                }
            }
        }
        None
    } else {
        None
    }
}

fn rustc_version() -> Option<String> {
    run_capturing("rustc", &["--version"]).map(|s| s.trim().to_string())
}

/// Live workspace alint version, read at runtime from
/// Cargo.toml. Reused by `Tool::Alint::detect()` so the
/// fingerprint's `alint_version` field and
/// `tool_versions["alint"]` entry always agree — embedding the
/// version via `env!("CARGO_PKG_VERSION")` is compile-time and
/// drifts from the workspace's actual version any time xtask
/// hasn't been rebuilt since a `Cargo.toml` bump.
pub(super) fn alint_version() -> Option<String> {
    // Pull from the workspace's Cargo.toml — runs even before
    // the alint binary is built. Avoids a chicken-and-egg
    // ordering with `build_release_binary`.
    let manifest = env!("CARGO_MANIFEST_DIR");
    let workspace_cargo = Path::new(manifest).parent()?.join("Cargo.toml");
    let body = fs::read_to_string(workspace_cargo).ok()?;
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("version") {
            // version = "0.5.7"
            return Some(
                rest.trim_start_matches([' ', '='])
                    .trim_matches('"')
                    .to_string(),
            );
        }
    }
    None
}

fn alint_git_sha() -> Option<String> {
    run_capturing("git", &["rev-parse", "--short", "HEAD"]).map(|s| s.trim().to_string())
}

fn hyperfine_version() -> Option<String> {
    run_capturing("hyperfine", &["--version"]).map(|s| {
        // "hyperfine 1.18.0" → "1.18.0"
        s.trim()
            .strip_prefix("hyperfine ")
            .unwrap_or_else(|| s.trim())
            .to_string()
    })
}

fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!("unix:{secs}")
}

fn run_capturing(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}
