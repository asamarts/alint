use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::fingerprint;
#[allow(clippy::wildcard_imports)]
use super::*;

// ─── Output ──────────────────────────────────────────────────────────

pub(super) fn write_outputs(report: &Report, args: &ScaleArgs) -> Result<()> {
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

/// `docs/benchmarks/macro/results/<os>-<arch>/<workspace-version>/`
/// — the canonical committable location.
///
/// The workspace version is read from the alint binary's
/// reported version (which the harness has already established
/// via `build_release_binary` before this is called) so the
/// default tracks the workspace version as it bumps.
/// Maintainers pass `--out` to override for ad-hoc / investigation
/// runs they don't want polluting the published per-version dir.
///
/// Pre-2026-05 this was hard-coded to
/// `docs/benchmarks/v0.5/scale/<arch>/ (the pre-reorg path)`, which silently
/// overwrote the v0.5 baseline numbers on every run. Per
/// `docs/benchmarks/README.md`'s layout, results are now
/// per-version under `macro/results/`.
fn default_out_dir(fp: &fingerprint::Fingerprint) -> Result<PathBuf> {
    let workspace = workspace_root()?;
    let platform = format!("{}-{}", fp.os, fp.arch);
    let version = workspace_version(&workspace)?;
    Ok(workspace
        .join("docs")
        .join("benchmarks")
        .join("macro")
        .join("results")
        .join(platform)
        .join(format!("v{version}")))
}

/// Read `workspace.package.version` from the workspace root
/// `Cargo.toml`. Tiny inline parse — enough for the default
/// out-dir below; reaching for `cargo_metadata` here would be
/// overkill for a one-line value.
fn workspace_version(workspace: &Path) -> Result<String> {
    let manifest = std::fs::read_to_string(workspace.join("Cargo.toml"))
        .context("read workspace Cargo.toml")?;
    for line in manifest.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("version") {
            // matches `version = "..."` (workspace.package.version
            // inherited; the workspace root's first `version =` is
            // the canonical workspace version — see the v0.5+
            // workspace inheritance pattern).
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
        workspace.display()
    )
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
    let _ = writeln!(out, "**Platform:** `{}/{}`<br>", fp.os, fp.arch);
    let _ = writeln!(
        out,
        "**CPU:** `{}` ({} cores)<br>",
        fp.cpu_model, fp.cpu_cores
    );
    let _ = writeln!(out, "**RAM:** {} GB<br>", fp.ram_gb);
    let _ = writeln!(out, "**FS:** `{}`<br>", fp.fs_type);
    let _ = writeln!(out, "**rustc:** `{}`<br>", fp.rustc);
    let _ = writeln!(
        out,
        "**alint:** `{}` ({})<br>",
        fp.alint_version, fp.alint_git_sha
    );
    let _ = writeln!(out, "**hyperfine:** `{}`<br>", fp.hyperfine_version);
    if !fp.tool_versions.is_empty() {
        let listing: String = fp
            .tool_versions
            .iter()
            .map(|(name, ver)| format!("{name}=`{ver}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "**Tools:** {listing}<br>");
    }
    let _ = writeln!(out, "**Seed:** `{}`<br>", args.seed);
    let _ = writeln!(out, "**Warmup/runs:** {} / {}<br>", args.warmup, args.runs);
    let _ = writeln!(out, "**Generated:** `{}`<br>", fp.timestamp);
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md`. \
         Compare numbers like-for-like (same fingerprint), never absolutely."
    );
}
