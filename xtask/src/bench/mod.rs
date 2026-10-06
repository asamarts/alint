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
//! Scenarios live in `scenarios/*.yml` — five consolidated configs
//! spanning the four cost axes plus auto-fix: layout/path (S1),
//! per-file content (S2), cross-file/relational/graph (S3), the
//! realistic workspace bundle (S4), and the dedicated auto-fix pass
//! (`sfix_all`, run under `Mode::Fix`).
//!
//! Output: a per-platform, per-version directory under
//! `docs/benchmarks/macro/results/<os>-<arch>/<workspace-version>/`
//! containing a `results.json` (machine-readable) plus per-size
//! `results.md` files and an `index.md` summary. Cross-machine
//! comparisons always require like-for-like (same fingerprint) —
//! see `docs/benchmarks/METHODOLOGY.md`. Cross-version comparisons
//! walk per-version dirs; see `docs/benchmarks/HISTORY.md` for
//! the headline cross-release table.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub mod compare;
pub mod docker;
mod fingerprint;
pub mod gate;
pub mod tools;

pub use tools::Tool;

/// Embedded scenario YAMLs. Each ships in the xtask binary so
/// running on any cloned checkout produces byte-identical
/// configs without depending on workspace-relative path resolution.
const SCENARIO_S1: &str = include_str!("scenarios/s1_layout.yml");
const SCENARIO_S2: &str = include_str!("scenarios/s2_content.yml");
const SCENARIO_S3: &str = include_str!("scenarios/s3_relational.yml");
const SCENARIO_S4: &str = include_str!("scenarios/s4_workspace.yml");
const SCENARIO_SFIX: &str = include_str!("scenarios/sfix_all.yml");

/// Parameters parsed from CLI flags. Defaults pick the
/// "publish-grade run" — full size matrix (excluding 1m), all
/// scenarios, all applicable modes (`supports()` skips the
/// nonsensical cells) — so a bare `xtask bench-scale` produces a
/// committable result.
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
    /// A PRE-BUILT alint binary to benchmark instead of building from the current
    /// checkout. Used by the past-version backfill: the CURRENT harness measures an
    /// OLD alint binary (built separately from its tag). `None` = build the current
    /// checkout (the normal path).
    pub alint_binary: Option<PathBuf>,
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
    /// Axis A — layout & path (walk-bound). Filename class, existence /
    /// absence, path-metadata, and a `scope_filter` shape. The cheapest
    /// path (walker + `GlobSet`, little content read); the ls-lint / shell
    /// competitive anchor. Absorbs the old S1 / S10 + the layout half of
    /// the old S2/S4.
    S1,
    /// Axis B — per-file content. Per-file dispatch fan-out, content read,
    /// per-line kinds, and per-file structured queries: the 13 content
    /// kinds, forbidden-content patterns, and `ordered_block` /
    /// `import_gate` / `xml_path_*` over `**/*.rs` (plus one `.csproj`
    /// overlay, see `setup_overlay`). The Repolinter anchor. Absorbs the
    /// old S2/S5/S6/S12.
    S2,
    /// Axis C — cross-file, relational & graph. Whole-index build +
    /// path-index + relational fan-out + `file_graph` build/traversal +
    /// single-shot spawn. Needs the `manifest.sha256` / `.gff_target` /
    /// `.v012_editions` / `graph/` overlay (see `setup_overlay`). Absorbs
    /// the old S7/S11/S13/S14.
    S3,
    /// Axis D — workspace bundle (realistic; release anchor). The six
    /// bundled rulesets (`oss-baseline` + `rust` + `node` + `python` +
    /// `monorepo` + `cargo-workspace`) over a POLYGLOT + GIT tree, with
    /// `nested_configs` on and the two git-aware rules inline. Absorbs the
    /// old S3/S8/S9. `requires_polyglot_tree` + `requires_git_repo` route
    /// it to the git-aware polyglot generator.
    S4,
    /// Axis E — auto-fix (dedicated). `alint fix --unsafe-fixes --dry-run`
    /// exercising all 24 non-spawn fix ops over the planted `sfix/` fixture
    /// (see `setup_overlay`). The fix-mode-only scenario; `Tool::supports`
    /// runs it under `Mode::Fix` and nothing else. Replaces the old S5
    /// (4-op fix pass) and the deterministic `sfix_trim`.
    Sfix,
}

impl Scenario {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_uppercase().as_str() {
            "S1" => Ok(Self::S1),
            "S2" => Ok(Self::S2),
            "S3" => Ok(Self::S3),
            "S4" => Ok(Self::S4),
            "SFIX" | "SFIX_ALL" => Ok(Self::Sfix),
            other => bail!("unknown scenario {other:?}; expected one of S1, S2, S3, S4, SFIX"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::S1 => "S1",
            Self::S2 => "S2",
            Self::S3 => "S3",
            Self::S4 => "S4",
            Self::Sfix => "SFIX",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::S1 => {
                "Layout & path — filename class / existence / absence / metadata / scope_filter shape (walk-bound)"
            }
            Self::S2 => {
                "Per-file content — 13 content kinds + forbidden patterns + ordered_block / import_gate / xml over **/*.rs"
            }
            Self::S3 => {
                "Cross-file, relational & graph — pair / unique_by / for_each_* / registry / cross_file / pair_hash / file_graph / single-shot spawn"
            }
            Self::S4 => {
                "Workspace bundle — oss-baseline + rust + node + python + monorepo + cargo-workspace over a polyglot git tree + git-aware rules"
            }
            Self::Sfix => {
                "Auto-fix — fix --unsafe-fixes --dry-run over all 24 non-spawn fix ops on the planted sfix/ fixture"
            }
        }
    }

    pub fn config_yaml(self) -> &'static str {
        match self {
            Self::S1 => SCENARIO_S1,
            Self::S2 => SCENARIO_S2,
            Self::S3 => SCENARIO_S3,
            Self::S4 => SCENARIO_S4,
            Self::Sfix => SCENARIO_SFIX,
        }
    }

    /// True for scenarios whose tree must be the nested-polyglot
    /// shape (rust + node + python packages distributed across
    /// `crates/` + `packages/` + `apps/`). The consolidated `S4`
    /// workspace bundle stacks the bundled ecosystem rulesets over
    /// this shape (absorbing the old S9 polyglot + S10 `scope_filter`
    /// competition); combined with `requires_git_repo` it drives
    /// `bench-scale` to the git-aware polyglot generator.
    pub fn requires_polyglot_tree(self) -> bool {
        matches!(self, Self::S4)
    }

    /// True for scenarios whose tree must be a real git repo
    /// (`.git/` initialised, every file `git add`'d + committed at
    /// generation time). `S4` folds in the old S8 git overlay:
    /// `git_no_denied_paths` / `git_tracked_only` +
    /// `Engine::collect_git_tracked_if_needed` / `BlameCache` only
    /// fire inside a repo, so the git state is baked at generation
    /// (not deferred to the `--changed` setup) and the check side
    /// sees it in every mode. Combined with `requires_polyglot_tree`,
    /// run.rs selects `generate_git_nested_polyglot_monorepo`.
    pub fn requires_git_repo(self) -> bool {
        matches!(self, Self::S4)
    }

    /// Every value of the enum, in declaration order. Drives
    /// the `xtask bench-scale` "all scenarios" default + the
    /// parse-validation unit test in this module.
    #[allow(dead_code)] // exercised by `#[cfg(test)]` only today; retained for the publish-grade default.
    pub fn all() -> &'static [Scenario] {
        &[Self::S1, Self::S2, Self::S3, Self::S4, Self::Sfix]
    }

    /// Materialise this scenario's fixture overlay into the
    /// generated tree (a sibling of `tool.setup_config` — that
    /// writes the per-tool config, this writes the per-scenario
    /// data files the config references). Called once per
    /// scenario per size, paired with [`Scenario::teardown_overlay`]
    /// so the overlay never persists across scenarios that share
    /// the regular tree. No-op for S1 / S4; S2 / S3 / Sfix each
    /// write a deterministic fixture. Idempotent — re-running over
    /// an existing overlay just rewrites it.
    pub fn setup_overlay(self, root: &Path) -> Result<()> {
        match self {
            // S2 (per-file content) — one root `.csproj` for the two
            // `xml_path_*` rules (absorbed from the old S12).
            Self::S2 => std::fs::write(
                root.join("sample.csproj"),
                concat!(
                    "<Project Sdk=\"Microsoft.NET.Sdk\">",
                    "<PropertyGroup>",
                    "<TargetFramework>net8.0</TargetFramework>",
                    "</PropertyGroup>",
                    "</Project>\n",
                ),
            )
            .with_context(|| format!("writing S2 sample.csproj to {}", root.display()))?,
            // S3 (relational + graph) — the four fixtures the old
            // S11/S13/S14 each carried, now on one scenario:
            //   * manifest.sha256 — pair_hash (all-zeros hash + a token
            //     matching no real file, so every source's hash is absent).
            //   * .gff_target      — generated_file_fresh target.
            //   * .v012_editions   — the single value every member's edition
            //     unions to, so the glob-union set_equals rule stays silent.
            //   * graph/           — a 24-node cyclic file_graph (ring
            //     g{i} -> g{(i+1)%24}) so the `acyclic` rule exercises the
            //     cycle-detection DFS the empty synthetic-lorem graph never
            //     would. The one cycle is a deterministic violation.
            Self::S3 => {
                std::fs::write(
                    root.join("manifest.sha256"),
                    "0000000000000000000000000000000000000000000000000000000000000000  fixture\n",
                )
                .with_context(|| format!("writing S3 manifest.sha256 to {}", root.display()))?;
                std::fs::write(root.join(".gff_target"), b"")
                    .with_context(|| format!("writing S3 .gff_target to {}", root.display()))?;
                std::fs::write(root.join(".v012_editions"), "2024\n")
                    .with_context(|| format!("writing S3 .v012_editions to {}", root.display()))?;
                let graph = root.join("graph");
                std::fs::create_dir_all(&graph)
                    .with_context(|| format!("creating S3 graph/ in {}", root.display()))?;
                for i in 0u32..24 {
                    let next = (i + 1) % 24;
                    std::fs::write(
                        graph.join(format!("g{i:02}.rs")),
                        format!("// dep: graph/g{next:02}.rs\n"),
                    )
                    .with_context(|| format!("writing S3 graph/g{i:02}.rs"))?;
                }
            }
            // Sfix (auto-fix) — the planted `sfix/` fixture (one
            // deterministic violation per `sfix/`-scoped fix op).
            Self::Sfix => setup_sfix_fixture(root)?,
            Self::S1 | Self::S4 => {}
        }
        Ok(())
    }

    /// Remove this scenario's fixture overlay so the next
    /// scenario on the shared tree sees a pristine state.
    /// Missing paths are ignored — calling teardown without a
    /// matching setup (or twice) must not error.
    pub fn teardown_overlay(self, root: &Path) -> Result<()> {
        for rel in self.overlay_files() {
            remove_file_if_present(&root.join(rel))?;
        }
        for rel in self.overlay_dirs() {
            remove_dir_if_present(&root.join(rel))?;
        }
        Ok(())
    }

    /// Single-file overlay paths this scenario's [`Self::setup_overlay`]
    /// writes, relative to the tree root.
    fn overlay_files(self) -> &'static [&'static str] {
        match self {
            Self::S2 => &["sample.csproj"],
            Self::S3 => &["manifest.sha256", ".gff_target", ".v012_editions"],
            Self::S1 | Self::S4 | Self::Sfix => &[],
        }
    }

    /// Subtree overlay paths this scenario's [`Self::setup_overlay`] writes,
    /// relative to the tree root (removed with `remove_dir_all`).
    fn overlay_dirs(self) -> &'static [&'static str] {
        match self {
            Self::S3 => &["graph"],
            Self::Sfix => &["sfix"],
            Self::S1 | Self::S2 | Self::S4 => &[],
        }
    }
}

/// Remove a file, tolerating its absence (idempotent teardown).
fn remove_file_if_present(p: &Path) -> Result<()> {
    match std::fs::remove_file(p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("removing overlay {}", p.display())),
    }
}

/// Remove a directory tree, tolerating its absence (idempotent teardown).
fn remove_dir_if_present(p: &Path) -> Result<()> {
    match std::fs::remove_dir_all(p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("removing overlay dir {}", p.display())),
    }
}

/// Plant the `sfix/` fixture for the [`Scenario::Sfix`] auto-fix bench:
/// a deterministic violation for each of the 24 non-spawn fix ops whose
/// rule scopes to `sfix/` (the 8 whole-file content ops scope to `**/*.rs`
/// and also fire on `sfix/torture.rs`). Idempotent — every path is
/// (re)written or created. Paths a rule fires on by ABSENCE (`sfix/NOTICE`
/// for `file_create`, `sfix/adr/` for `dir_create`) are deliberately NOT
/// created.
fn setup_sfix_fixture(root: &Path) -> Result<()> {
    let sfix = root.join("sfix");
    std::fs::create_dir_all(&sfix)
        .with_context(|| format!("creating sfix/ in {}", root.display()))?;

    // torture.rs — the whole-file content-op violations (matched by the
    // `**/*.rs` content rules): a leading BOM, tab indentation, trailing
    // whitespace, a CRLF line, a 3-blank-line run, a bidi control, a
    // zero-width space, and no final newline. Plus a FIXME (replace), and
    // it lacks both a Copyright header (file_prepend) and a license footer
    // (file_append).
    let torture = concat!(
        "\u{FEFF}use std::io;\t\n",   // BOM + tab indent + trailing tab
        "fn main() {   \r\n",         // trailing spaces + CRLF
        "    // FIXME: unfinished\n", // replace target
        "\n\n\n",                     // blank-line run (collapse)
        "\tlet x = 1; \n",            // tab indent + trailing space
        "    let y = \u{202E}2;\n",   // bidi control
        "    let z = 3;\u{200B}\n",   // zero-width space
        "}",                          // no final newline
    );
    std::fs::write(sfix.join("torture.rs"), torture).with_context(|| "writing sfix/torture.rs")?;

    // BadName.rs — PascalCase filename → filename_case(snake) → file_rename.
    std::fs::write(sfix.join("BadName.rs"), "// placeholder\n")
        .with_context(|| "writing sfix/BadName.rs")?;

    // sorted.txt — a markerless ordered_block that isn't sorted → sort.
    std::fs::write(sfix.join("sorted.txt"), "charlie\nalpha\nbravo\n")
        .with_context(|| "writing sfix/sorted.txt")?;

    // CODEOWNERS — a sorted markerless ordered_block missing a required
    // line → insert_line splices `*.py @py-team` at its sorted position.
    std::fs::write(sfix.join("CODEOWNERS"), "*.rs @rust-team\n*.ts @web-team\n")
        .with_context(|| "writing sfix/CODEOWNERS")?;

    // config.yaml — wrong `env` value (set_value) + a forbidden `debug`
    // key (remove_value).
    std::fs::write(sfix.join("config.yaml"), "env: staging\ndebug: true\n")
        .with_context(|| "writing sfix/config.yaml")?;

    // build.sh — a shebang but no Copyright header (insert_header, after
    // the shebang); marked executable below → executable_bit(require:false)
    // → chmod.
    let build_sh = sfix.join("build.sh");
    std::fs::write(&build_sh, "#!/bin/sh\necho build\n")
        .with_context(|| "writing sfix/build.sh")?;

    // junk.tmp — a forbidden file (file_remove).
    std::fs::write(sfix.join("junk.tmp"), "scratch\n").with_context(|| "writing sfix/junk.tmp")?;

    // nested/Cargo.lock — a nested lockfile (relocate).
    let nested = sfix.join("nested");
    std::fs::create_dir_all(&nested).with_context(|| "creating sfix/nested/")?;
    std::fs::write(nested.join("Cargo.lock"), "# lock\n")
        .with_context(|| "writing sfix/nested/Cargo.lock")?;

    // version.toml + pkgs/*/Cargo.toml — pkg `b`'s version drifts from the
    // source (sync_from); workspace.toml registers only `a`, so `b` is
    // unregistered (create_and_register).
    std::fs::write(
        sfix.join("version.toml"),
        "[package]\nname = \"root\"\nversion = \"1.0.0\"\n",
    )
    .with_context(|| "writing sfix/version.toml")?;
    let pkgs = sfix.join("pkgs");
    for (name, ver) in [("a", "1.0.0"), ("b", "0.9.0")] {
        let dir = pkgs.join(name);
        std::fs::create_dir_all(&dir).with_context(|| format!("creating sfix/pkgs/{name}/"))?;
        std::fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"{ver}\"\n"),
        )
        .with_context(|| format!("writing sfix/pkgs/{name}/Cargo.toml"))?;
    }
    std::fs::write(
        sfix.join("workspace.toml"),
        "[workspace]\nmembers = [\"pkgs/a\"]\n",
    )
    .with_context(|| "writing sfix/workspace.toml")?;

    // Mark build.sh executable so executable_bit(require:false) fires.
    // Unix-only: Windows has no POSIX exec bit and the rule no-ops there.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&build_sh, std::fs::Permissions::from_mode(0o755))
            .with_context(|| "chmod +x sfix/build.sh")?;
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Full,
    Changed,
    /// `alint fix --unsafe-fixes --dry-run` — the auto-fix pass. Paired
    /// exclusively with [`Scenario::Sfix`] (see [`Tool::supports`]).
    /// `--dry-run` keeps the tree byte-stable across hyperfine iterations;
    /// `--unsafe-fixes` forces every op's compute + compose path (not just
    /// the Safe tier), so the bench covers the ops that default to Unsafe.
    Fix,
}

impl Mode {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "full" => Ok(Self::Full),
            "changed" => Ok(Self::Changed),
            "fix" => Ok(Self::Fix),
            other => bail!("unknown mode {other:?}; expected `full`, `changed`, or `fix`"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Changed => "changed",
            Self::Fix => "fix",
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

mod output;
mod run;

pub use run::bench_scale;

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every scenario's embedded YAML must load cleanly
    /// through `alint-dsl::load` AND every rule it declares
    /// must build through the alint-rules registry. A typo
    /// in an `include_str!` path, in a rule kind name, or in a
    /// per-kind option fails at `cargo test` time, BEFORE the
    /// publish-grade `xtask bench-scale` invocation tries to
    /// write the broken file as `.alint.yml` and hyperfine
    /// reports a runtime error halfway through.
    ///
    /// Uses `load` rather than `parse` so `extends:` resolves
    /// (S3 needs it); the registry-build pass is the same
    /// chain `alint check` walks at startup.
    #[test]
    fn every_scenario_yaml_loads_and_every_rule_builds() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let registry = alint_rules::builtin_registry();
        for &s in Scenario::all() {
            let path = tmp.path().join(format!("{}.alint.yml", s.label()));
            std::fs::write(&path, s.config_yaml())
                .unwrap_or_else(|e| panic!("writing {}: {e}", s.label()));
            let config = alint_dsl::load(&path)
                .unwrap_or_else(|e| panic!("scenario {} failed to load: {e}", s.label()));
            for spec in &config.rules {
                registry.build(spec).unwrap_or_else(|e| {
                    panic!(
                        "scenario {} rule {:?} failed to build: {e}",
                        s.label(),
                        spec.id
                    )
                });
            }
        }
    }

    /// `Scenario::all()` must enumerate every variant — the
    /// `parse` / `label` match arms cover S1..S4 + SFIX, so
    /// `all()` must too. Detects "added an enum variant, forgot
    /// to update `all()`".
    #[test]
    fn all_covers_every_parsed_label() {
        for &s in Scenario::all() {
            let parsed = Scenario::parse(s.label())
                .unwrap_or_else(|e| panic!("label {} fails to round-trip: {e}", s.label()));
            assert_eq!(parsed, s, "round-trip mismatch for {}", s.label());
        }
    }

    /// Overlay setup / teardown must be tolerant of being
    /// called on a no-op scenario AND of teardown running
    /// without a prior setup (which is what happens if a run
    /// is interrupted between the two).
    #[test]
    fn overlay_hooks_are_idempotent_and_no_op_for_legacy_scenarios() {
        let tmp = tempfile::tempdir().expect("tempdir");
        for &s in Scenario::all() {
            s.setup_overlay(tmp.path()).expect("setup");
            // calling teardown twice must succeed (the second
            // call hits the NotFound branch).
            s.teardown_overlay(tmp.path()).expect("first teardown");
            s.teardown_overlay(tmp.path()).expect("second teardown");
        }
    }
}
