//! `xtask` — ancillary helpers for alint that don't belong in the shipped binary.
//!
//! Current commands (kept in sync with the `Commands` enum below):
//!
//! - `bench-release`     — legacy v0.1 single-config hyperfine harness across a
//!   tree-size × rule-count matrix. Superseded by `bench-scale` for the
//!   scenario × size × mode matrix; retained for back-compat.
//! - `bench-scale`       — the v0.5+ benchmark matrix (scenario × size × mode)
//!   with hardware-fingerprint capture and JSON + Markdown publication.
//! - `gen-fixture`       — materialize a synthetic tree (persistent) for
//!   ad-hoc experimentation.
//! - `gen-monorepo`      — materialize a reusable 1k/10k/100k/1m monorepo
//!   tree so profiling iterations skip per-run tree-gen.
//! - `bench-compare`     — diff two criterion runs and fail when a paired
//!   bench's mean time regressed past a threshold (PR-CI perf gate).
//! - `publish-benches`   — snapshot a criterion run into
//!   `docs/benchmarks/micro/results/<os>-<arch>/<version>/` for `git add`.
//! - `docs-export`       — emit a `docs-bundle/` directory consumed by the
//!   `asamarts/alint.org` site at build time. The bundle is the canonical
//!   handoff format between the alint repo (source of truth for technical
//!   docs) and the site repo (presentation).
//! - `gen-public-roadmap` — render the public roadmap from the canonical
//!   `docs/design/ROADMAP.md` (also invoked internally by `docs-export`).
//! - `gen-schema`         — regenerate `schemas/v1/config.json` from the rule
//!   `Options` structs (schemars); `--check` gates drift.
//! - `gen-facts`          — regenerate `facts.json` (the surface-area contract)
//!   from canonical sources; `--check` gates drift.
//! - `gen-roadmap`        — regenerate `roadmap.json` (the public-roadmap
//!   contract) from `docs/design/ROADMAP.md`; `--check` gates drift.
//! - `gen-arch`           — regenerate the crate dependency graph from
//!   `cargo metadata` + check the C4 model; `--check` gates drift.
//! - `gen-model`          — regenerate the code-derived `LikeC4` model fragments
//!   (the rule-kind taxonomy, ...) for the architecture diagrams; `--check`
//!   gates drift.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

mod arch;
mod bench;
mod bench_release;
mod docs_checks;
mod docs_export;
mod facts;
mod family_index;
mod gen_mermaid;
mod gen_model;
mod gen_roadmap;
mod gen_schema;
mod roadmap_generator;
mod rule_options_table;

pub(crate) use bench_release::{build_release_binary, git_sha, now_iso, workspace_root};

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
    /// Gate a macro `results.json` for publish. Quality: per-cell
    /// within-run CV ≤ 10 % on 100k/1m only (1k/10k advisory).
    /// Regression (with `--baseline`): `min_ms` delta vs the prior
    /// release ≤ +15 % on ≥10k cells. Exits non-zero on a gating
    /// failure. Replaces the unenforced human CV eyeball in
    /// `RELEASING.md`; thresholds validated against the full
    /// corpus — see
    /// `docs/benchmarks/investigations/2026-05-bench-runner-instability/`.
    BenchGate {
        /// The run to gate (a `bench-scale` `results.json`).
        #[arg(long)]
        results: PathBuf,
        /// Prior release's `results.json` for the `min_ms`
        /// regression check. Omit to run quality-only.
        #[arg(long)]
        baseline: Option<PathBuf>,
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
        /// Generate ONLY the per-rule reference pages (`rules/`), skipping the
        /// rest of the bundle — most importantly the CLI-reference step, which
        /// builds the alint release binary. Used by the docs-bundle rule-page
        /// bridge, which overlays only those pages from main; the redundant
        /// release build was the bulk of that bridge's cost.
        #[arg(long)]
        rules_only: bool,
    },
    /// Render the public roadmap from canonical `docs/design/ROADMAP.md`,
    /// stripping `<!-- alint:internal-start -->` /
    /// `<!-- alint:internal-end -->` blocks. See
    /// `docs/design/v0.11/roadmap_generator.md`. Also invoked
    /// internally by `docs-export` during the docs-bundle build;
    /// this standalone form is for ad-hoc debugging.
    GenPublicRoadmap {
        /// Canonical ROADMAP to read. Resolved against the workspace
        /// root when relative.
        #[arg(long, default_value = "docs/design/ROADMAP.md")]
        input: PathBuf,
        /// Output path for the rendered public roadmap. Resolved
        /// against the workspace root when relative.
        #[arg(long, default_value = "target/docs-bundle/about/roadmap.md")]
        output: PathBuf,
        /// Frontmatter `title:` value injected into the output.
        #[arg(long, default_value = "Roadmap")]
        title: String,
    },
    /// Generate `schemas/v1/config.json` from Rust types (schemars) for the
    /// migrated rule kinds, passing hand-written branches through for the rest.
    /// See ADR-0001 and docs/design/spec-driven-development.md.
    GenSchema {
        /// Verify the committed schema is up to date instead of rewriting it.
        #[arg(long)]
        check: bool,
    },
    /// Regenerate `facts.json` (the surface-area contract) from canonical sources.
    GenFacts {
        /// Verify the committed `facts.json` is up to date instead of rewriting it.
        #[arg(long)]
        check: bool,
    },
    /// Regenerate `roadmap.json` (the public-roadmap contract) from ROADMAP.md.
    GenRoadmap {
        /// Verify the committed `roadmap.json` is up to date instead of rewriting it.
        #[arg(long)]
        check: bool,
    },
    /// Regenerate the crate dependency graph from `cargo metadata` + gate the C4 model.
    GenArch {
        /// Verify the committed crate graph + C4 model instead of rewriting.
        #[arg(long)]
        check: bool,
    },
    /// Regenerate the code-derived `LikeC4` model fragments (rule taxonomy, ...).
    GenModel {
        /// Verify the committed `*.gen.c4` fragments instead of rewriting them.
        #[arg(long)]
        check: bool,
    },
    /// Regenerate the GitHub-facing Mermaid diagram gallery from the `LikeC4` model.
    GenMermaid {
        /// Verify the committed `DIAGRAMS.md` instead of rewriting it.
        #[arg(long)]
        check: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::BenchRelease { quick, out, seed } => {
            bench_release::bench_release(quick, out, seed)
        }
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
        Commands::BenchGate { results, baseline } => {
            bench::gate::run(&results, baseline.as_deref())
        }
        Commands::DocsExport {
            out,
            check,
            rules_only,
        } => docs_export::docs_export(out, check, rules_only),
        Commands::GenPublicRoadmap {
            input,
            output,
            title,
        } => {
            let workspace = workspace_root()?;
            let input = if input.is_absolute() {
                input
            } else {
                workspace.join(&input)
            };
            let output = if output.is_absolute() {
                output
            } else {
                workspace.join(&output)
            };
            roadmap_generator::generate_public_roadmap(&input, &output, &title)
        }
        Commands::GenSchema { check } => gen_schema::run(check),
        Commands::GenFacts { check } => facts::run(check),
        Commands::GenRoadmap { check } => gen_roadmap::run(check),
        Commands::GenArch { check } => arch::run(check),
        Commands::GenModel { check } => gen_model::run(check),
        Commands::GenMermaid { check } => gen_mermaid::run(check),
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
pub(crate) fn workspace_root_from_xtask() -> Result<PathBuf> {
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
pub(crate) fn workspace_version_from_manifest(workspace: &Path) -> Result<String> {
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
pub(crate) fn copy_criterion_tree(from: &Path, to: &Path, trim: bool) -> Result<()> {
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
pub(crate) fn should_trim_path(rel: &Path) -> bool {
    let s = rel.to_string_lossy();
    s.contains("/report/")
        || s.starts_with("report/")
        || s.ends_with(".svg")
        || s.ends_with(".html")
        || s.contains("/change/")
}

pub(crate) fn copy_tree(from: &Path, to: &Path) -> Result<()> {
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

pub(crate) fn walkdir_plain(root: &Path) -> Result<Vec<PathBuf>> {
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

#[cfg(test)]
mod tests {
    use super::docs_export::{first_overview_sentence, render_overview_from_comments};

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
