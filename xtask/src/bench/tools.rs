//! Tool abstraction for multi-tool benchmarks.
//!
//! Each [`Tool`] declares which (scenario, mode) combinations it
//! can meaningfully run, knows how to detect itself on `PATH`,
//! writes its own config into the bench tree, and builds the
//! hyperfine command string. The orchestrator iterates the
//! cartesian product of (tool × size × scenario × mode) and
//! skips combos where `tool.supports(scenario, mode) == false`
//! — that's how ls-lint is gated to S1 only, Repolinter (later)
//! to S2 only, etc.
//!
//! Phase 1 ships `Alint` + `LsLint`. Future phases will add
//! `GrepPipeline` (find + ripgrep baseline) and `Repolinter`
//! behind the same abstraction.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Result, bail};

use super::{Mode, Scenario};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    Alint,
    LsLint,
}

/// All tool variants in iteration order. Used by `--tools all`
/// to expand to "every known tool, skip missing." When new
/// variants are added, list them here.
pub const ALL: &[Tool] = &[Tool::Alint, Tool::LsLint];

impl Tool {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "alint" => Ok(Self::Alint),
            "ls-lint" | "lslint" => Ok(Self::LsLint),
            other => bail!("unknown tool {other:?}; expected one of alint, ls-lint, all"),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Alint => "alint",
            Self::LsLint => "ls-lint",
        }
    }

    /// True iff this tool can meaningfully run the given
    /// `(scenario, mode)`. Out-of-scope combos are skipped at
    /// the orchestrator level rather than producing zero or
    /// nonsense rows. ls-lint is filename-only and has no
    /// `--changed`-equivalent; Repolinter (when added) covers
    /// content + existence with no filename support.
    pub fn supports(self, scenario: Scenario, mode: Mode) -> bool {
        // Clippy thinks the `true` arms could be merged with
        // `|`, but keeping them split makes adding the
        // GrepPipeline / Repolinter arms in follow-up commits
        // a one-line change instead of a re-split.
        #[allow(clippy::match_same_arms)]
        match (self, scenario, mode) {
            (Self::Alint, _, _) => true,
            (Self::LsLint, Scenario::S1, Mode::Full) => true,
            (Self::LsLint, _, _) => false,
        }
    }

    /// `Some(version)` if installed and on `PATH`; `None`
    /// otherwise. Tools whose detection fails are skipped at
    /// run time (with a note on stderr); the bench harness
    /// never aborts because a competitor tool is missing.
    pub fn detect(self) -> Option<String> {
        match self {
            Self::Alint => Some(env!("CARGO_PKG_VERSION").to_string()),
            Self::LsLint => detect_via_version_flag("ls-lint", "--version"),
        }
    }

    /// Write the tool's config file into `root` for the given
    /// scenario. Idempotent — overwrites any existing copy.
    /// Called once per `(tool, size, scenario)` before the
    /// row's hyperfine runs. Tools whose config is purely
    /// CLI-arg-driven (e.g. a future grep-pipeline) just
    /// return `Ok(())`.
    pub fn setup_config(self, root: &Path, scenario: Scenario) -> Result<()> {
        match self {
            Self::Alint => {
                fs::write(root.join(".alint.yml"), scenario.config_yaml())?;
            }
            Self::LsLint => {
                debug_assert_eq!(scenario, Scenario::S1, "ls-lint only supports S1");
                fs::write(root.join(".ls-lint.yml"), LS_LINT_S1_CONFIG)?;
            }
        }
        Ok(())
    }

    /// Full shell command line handed to hyperfine for one
    /// row. Hyperfine spawns this via `sh -c`, so pipes /
    /// semicolons / globs work as a user would type them —
    /// future tools like `GrepPipeline` will exploit that;
    /// single-binary tools like alint and ls-lint just
    /// produce a `bin args...` string. `alint_bin` is the
    /// path to the locally-built alint binary; ignored by
    /// non-alint tools (which find their binary on `PATH`).
    pub fn invocation(
        self,
        alint_bin: &Path,
        tree_root: &Path,
        _scenario: Scenario,
        mode: Mode,
    ) -> String {
        let root = quote_for_shell(&tree_root.to_string_lossy());
        match self {
            Self::Alint => {
                let bin = quote_for_shell(&alint_bin.to_string_lossy());
                if mode == Mode::Changed {
                    format!("{bin} check {root} --changed")
                } else {
                    format!("{bin} check {root}")
                }
            }
            Self::LsLint => format!("ls-lint -workdir {root}"),
        }
    }
}

/// Single-quote `s` for safe inclusion in a `sh -c` command
/// line. Embedded single quotes get the standard
/// `'\''`-then-reopen trick. Used for tree-root paths so a
/// path with spaces or apostrophes round-trips cleanly.
fn quote_for_shell(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn detect_via_version_flag(program: &str, version_arg: &str) -> Option<String> {
    let out = Command::new(program).arg(version_arg).output().ok()?;
    if !out.status.success() {
        return None;
    }
    // Some tools (e.g. ripgrep) print a multi-line `--version`
    // banner. Keep just the first line so the fingerprint
    // table stays one row tall.
    let raw = String::from_utf8_lossy(&out.stdout);
    Some(raw.lines().next().unwrap_or("").trim().to_string())
}

/// Resolve a `--tools` CLI value into the actual tool set.
/// Accepts `all` (expand to every known variant), or a
/// comma-separated list of explicit tool names. Detection is
/// applied here: tools that aren't installed are dropped from
/// the returned set (stderr-logged so the user notices), so
/// the orchestrator can iterate without per-row presence
/// checks.
pub fn resolve(specs: &[String]) -> Result<Vec<Tool>> {
    let mut tools: Vec<Tool> = Vec::new();
    for spec in specs {
        if spec.trim().eq_ignore_ascii_case("all") {
            for &t in ALL {
                if !tools.contains(&t) {
                    tools.push(t);
                }
            }
        } else {
            let t = Tool::parse(spec)?;
            if !tools.contains(&t) {
                tools.push(t);
            }
        }
    }
    // Detection pass — log-and-drop missing tools so the
    // remaining matrix runs without per-row null checks.
    let mut present: Vec<Tool> = Vec::with_capacity(tools.len());
    for t in tools {
        match t.detect() {
            Some(_) => present.push(t),
            None => {
                eprintln!(
                    "[xtask] note: tool {:?} not found on PATH — skipping its rows",
                    t.name()
                );
            }
        }
    }
    if present.is_empty() {
        bail!("no requested tool is installed; nothing to bench");
    }
    Ok(present)
}

/// `.ls-lint.yml` body for scenario S1 — the same eight
/// filename rules alint S1 enforces, expressed in ls-lint's
/// extension-keyed shape. Both engines walk the tree once and
/// match each file's basename against the configured class
/// per extension; the work shapes line up cleanly.
const LS_LINT_S1_CONFIG: &str = r"# ls-lint config — S1 (filename hygiene), equivalent to xtask/src/bench/scenarios/s1_filename.yml.
ls:
  .rs: snake_case
  .tsx: PascalCase
  .ts: kebab-case
  .yaml: kebab-case
  .yml: kebab-case
  .md: regex:^[a-zA-Z0-9_.-]+$
  .json: regex:^[a-zA-Z0-9_.-]+$
  .py: snake_case

ignore:
  - vendor
  - node_modules
  - target
  - .git
";
