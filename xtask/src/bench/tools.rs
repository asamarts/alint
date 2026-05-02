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
//! Phases 1-3 ship `Alint`, `LsLint`, `GrepPipeline`, and
//! `Repolinter`.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Result, bail};

use super::{Mode, Scenario};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    Alint,
    LsLint,
    /// `find` + `ripgrep` pipelines — the small-team status
    /// quo. Universal on Unix; doesn't ship its own config so
    /// the per-scenario shell command embeds the rule set
    /// inline. Useful as a baseline for "how much does a
    /// dedicated tool actually buy you over piped one-liners?"
    GrepPipeline,
    /// Repolinter (TODO Group) — Node.js-based existence +
    /// content + structural rules. Pinned to the last
    /// pre-archive release (0.11.2, Aug 2023; repo archived
    /// 2026-02-06). Gated to (S2, Full) — its rule shape
    /// matches existence + content cleanly but has no
    /// built-in filename-class or file-size primitives, so S1
    /// and the size-check portion of S2 fall outside its
    /// remit. The bench documents that gap rather than
    /// papering over it with custom rules.
    Repolinter,
}

/// All tool variants in iteration order. Used by `--tools all`
/// to expand to "every known tool, skip missing." When new
/// variants are added, list them here.
pub const ALL: &[Tool] = &[
    Tool::Alint,
    Tool::LsLint,
    Tool::GrepPipeline,
    Tool::Repolinter,
];

impl Tool {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "alint" => Ok(Self::Alint),
            "ls-lint" | "lslint" => Ok(Self::LsLint),
            "grep" | "grep-pipeline" | "rg" => Ok(Self::GrepPipeline),
            "repolinter" => Ok(Self::Repolinter),
            other => bail!(
                "unknown tool {other:?}; expected one of alint, ls-lint, grep, repolinter, all"
            ),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Alint => "alint",
            Self::LsLint => "ls-lint",
            Self::GrepPipeline => "grep",
            Self::Repolinter => "repolinter",
        }
    }

    /// True iff this tool can meaningfully run the given
    /// `(scenario, mode)`. Out-of-scope combos are skipped at
    /// the orchestrator level rather than producing zero or
    /// nonsense rows. ls-lint is filename-only and has no
    /// `--changed`-equivalent; Repolinter (when added) covers
    /// content + existence with no filename support; the grep
    /// pipeline doesn't model the workspace bundle's
    /// cross-file rules so S3 is out of scope.
    pub fn supports(self, scenario: Scenario, mode: Mode) -> bool {
        // Clippy thinks the `true` arms could be merged with
        // `|`, but keeping them split makes adding the
        // Repolinter arm in a follow-up commit a one-line
        // change instead of a re-split.
        #[allow(clippy::match_same_arms)]
        match (self, scenario, mode) {
            (Self::Alint, _, _) => true,
            (Self::LsLint, Scenario::S1, Mode::Full) => true,
            (Self::LsLint, _, _) => false,
            (Self::GrepPipeline, Scenario::S1 | Scenario::S2, Mode::Full) => true,
            (Self::GrepPipeline, _, _) => false,
            (Self::Repolinter, Scenario::S2, Mode::Full) => true,
            (Self::Repolinter, _, _) => false,
        }
    }

    /// `Some(version)` if installed and on `PATH`; `None`
    /// otherwise. Tools whose detection fails are skipped at
    /// run time (with a note on stderr); the bench harness
    /// never aborts because a competitor tool is missing.
    pub fn detect(self) -> Option<String> {
        match self {
            // Read live from workspace Cargo.toml so this
            // matches `fingerprint::alint_version()` even when
            // xtask itself was last built before the most
            // recent version bump (env!() captures the version
            // at xtask compile time).
            Self::Alint => super::fingerprint::alint_version(),
            Self::LsLint => detect_via_version_flag("ls-lint", "--version"),
            Self::GrepPipeline => {
                // The pipeline needs both `find` (POSIX) and
                // `rg` (ripgrep) on PATH. Report the rg
                // version as the version-string handle since
                // it's the more interesting moving target;
                // `find` is essentially fixed across distros.
                detect_via_version_flag("rg", "--version")
                    .filter(|_| Command::new("find").arg("--help").output().is_ok())
            }
            Self::Repolinter => detect_via_version_flag("repolinter", "--version"),
        }
    }

    /// Write the tool's config file into `root` for the given
    /// scenario. Idempotent — overwrites any existing copy.
    /// Called once per `(tool, size, scenario)` before the
    /// row's hyperfine runs. Tools whose config is purely
    /// CLI-arg-driven (like the grep pipeline) just return
    /// `Ok(())`.
    pub fn setup_config(self, root: &Path, scenario: Scenario) -> Result<()> {
        match self {
            Self::Alint => {
                fs::write(root.join(".alint.yml"), scenario.config_yaml())?;
            }
            Self::LsLint => {
                debug_assert_eq!(scenario, Scenario::S1, "ls-lint only supports S1");
                fs::write(root.join(".ls-lint.yml"), LS_LINT_S1_CONFIG)?;
            }
            // The grep pipeline embeds its rules inline in the
            // shell command (see `grep_pipeline_*`); no
            // tool-specific config file is written.
            Self::GrepPipeline => {}
            Self::Repolinter => {
                debug_assert_eq!(scenario, Scenario::S2, "repolinter only supports S2");
                fs::write(root.join("repolinter.json"), REPOLINTER_S2_CONFIG)?;
            }
        }
        Ok(())
    }

    /// Full shell command line handed to hyperfine for one
    /// row. Hyperfine spawns this via `sh -c`, so pipes /
    /// semicolons / globs work exactly as a user would type
    /// them — important for `GrepPipeline`, which strings
    /// together multiple `find` + `rg` invocations.
    /// `alint_bin` is the path to the locally-built alint
    /// binary; ignored by non-alint tools (which find their
    /// binary on `PATH`).
    pub fn invocation(
        self,
        alint_bin: &Path,
        tree_root: &Path,
        scenario: Scenario,
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
            Self::GrepPipeline => match scenario {
                Scenario::S1 => grep_pipeline_s1(&root),
                Scenario::S2 => grep_pipeline_s2(&root),
                Scenario::S3
                | Scenario::S4
                | Scenario::S5
                | Scenario::S6
                | Scenario::S7
                | Scenario::S8
                | Scenario::S9 => {
                    unreachable!("supports() filters S3+ out for GrepPipeline")
                }
            },
            // `repolinter lint <root>` reads `repolinter.json`
            // at the tree root by default. We pass the tree
            // path positionally rather than via `-r` so a
            // run mirrors how a user would invoke it locally.
            Self::Repolinter => format!("repolinter lint {root}"),
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

/// S1 (filename hygiene) as eight `find ... | grep -vE '...'`
/// pipelines chained with `;`. Each pipeline lists files of
/// one extension and filters out names that match the case
/// pattern — the leftover lines are the would-be violations.
/// Output goes to `/dev/null` because we measure walker +
/// regex throughput, not formatting; alint and ls-lint also
/// suppress repeat output via hyperfine after the first run.
///
/// The case-class regexes are intentionally simpler than
/// alint's / ls-lint's full implementations (no Unicode
/// folding, no leading-digit handling, no extension-handling
/// nuances). The work shape — walk + per-file basename match
/// against a regex — is what the bench measures, and that
/// matches across all three tools. Documented in
/// methodology.md so readers don't read more into the numbers
/// than is there.
fn grep_pipeline_s1(root: &str) -> String {
    [
        // *.rs → snake_case
        format!("find {root} -name '*.rs' -type f -printf '%f\\n' | grep -vE '^[a-z][a-z0-9_]*\\.rs$' >/dev/null"),
        // *.tsx → PascalCase
        format!("find {root} -name '*.tsx' -type f -printf '%f\\n' | grep -vE '^[A-Z][a-zA-Z0-9]*\\.tsx$' >/dev/null"),
        // *.ts → kebab-case
        format!("find {root} -name '*.ts' -type f -printf '%f\\n' | grep -vE '^[a-z][a-z0-9-]*\\.ts$' >/dev/null"),
        // *.yaml → kebab-case
        format!("find {root} -name '*.yaml' -type f -printf '%f\\n' | grep -vE '^[a-z][a-z0-9-]*\\.yaml$' >/dev/null"),
        // *.yml → kebab-case
        format!("find {root} -name '*.yml' -type f -printf '%f\\n' | grep -vE '^[a-z][a-z0-9-]*\\.yml$' >/dev/null"),
        // *.md → broad alphanumeric
        format!("find {root} -name '*.md' -type f -printf '%f\\n' | grep -vE '^[a-zA-Z0-9_.-]+$' >/dev/null"),
        // *.json → broad alphanumeric
        format!("find {root} -name '*.json' -type f -printf '%f\\n' | grep -vE '^[a-zA-Z0-9_.-]+$' >/dev/null"),
        // *.py → snake_case
        format!("find {root} -name '*.py' -type f -printf '%f\\n' | grep -vE '^[a-z][a-z0-9_]*\\.py$' >/dev/null"),
    ]
    .join("; ")
}

/// S2 (existence + content) as eight shell commands. Layout
/// rules use `test -e` / `find ... -name`; content rules use
/// `rg` (ripgrep, parallel + Rust regex), which is what
/// small-team baselines actually reach for these days.
/// Tracks the rule-shape ratio of alint's S2: 4 layout
/// checks, 3 content checks (Rust / TS / Python forbidden
/// patterns), 1 size check.
fn grep_pipeline_s2(root: &str) -> String {
    [
        // Layout — README + LICENSE existence at root.
        format!("test -f {root}/README.md || test -f {root}/README"),
        format!(
            "test -f {root}/LICENSE || test -f {root}/LICENSE.md || test -f {root}/LICENSE.txt"
        ),
        // Layout — forbidden file extensions anywhere.
        format!("find {root} -name '*.bak' -type f >/dev/null"),
        format!("find {root} -name '*.orig' -type f >/dev/null"),
        // Content — TODO / XXX / FIXME in Rust.
        format!("rg --type rust --no-messages '\\b(TODO|XXX|FIXME)\\b' {root} >/dev/null || true"),
        // Content — `debugger;` in TS / TSX.
        format!("rg --type ts --no-messages '\\bdebugger\\s*;' {root} >/dev/null || true"),
        // Content — top-level print() in Python.
        format!("rg --type py --no-messages '^\\s*print\\s*\\(' {root} >/dev/null || true"),
        // Size — files larger than 10 MiB.
        format!("find {root} -type f -size +10M >/dev/null"),
    ]
    .join("; ")
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

/// `repolinter.json` body for scenario S2 (existence +
/// content). Maps to seven of alint's eight S2 rules:
///
/// | alint rule           | Repolinter rule       |
/// |----------------------|-----------------------|
/// | README must exist    | `file-existence`      |
/// | LICENSE must exist   | `file-existence`      |
/// | no `*.bak` files     | `file-not-exists`     |
/// | no `*.orig` files    | `file-not-exists`     |
/// | no TODO/FIXME (Rust) | `file-not-contents`   |
/// | no `debugger;` (TS)  | `file-not-contents`   |
/// | no top-level `print()` | `file-not-contents` |
/// | no files >10 MiB     | *(skipped)*           |
///
/// The size rule is dropped: Repolinter has no built-in
/// size-bounded primitive, and emulating it via a `script`
/// rule would fork Node per match and skew timings beyond
/// recognition. The methodology page documents the gap so
/// readers don't read the row as a 1:1 comparison.
const REPOLINTER_S2_CONFIG: &str = r#"{
  "$schema": "https://raw.githubusercontent.com/todogroup/repolinter/master/rulesets/schema.json",
  "version": 2,
  "axioms": {},
  "rules": {
    "readme-exists": {
      "level": "error",
      "rule": {
        "type": "file-existence",
        "options": { "globsAny": ["README", "README.md"] }
      }
    },
    "license-exists": {
      "level": "error",
      "rule": {
        "type": "file-existence",
        "options": { "globsAny": ["LICENSE", "LICENSE.md", "LICENSE.txt"] }
      }
    },
    "no-bak-files": {
      "level": "error",
      "rule": {
        "type": "file-not-exists",
        "options": { "globsAll": ["**/*.bak"] }
      }
    },
    "no-orig-files": {
      "level": "error",
      "rule": {
        "type": "file-not-exists",
        "options": { "globsAll": ["**/*.orig"] }
      }
    },
    "no-todo-rust": {
      "level": "error",
      "rule": {
        "type": "file-not-contents",
        "options": {
          "globsAll": ["**/*.rs"],
          "content": "\\b(TODO|XXX|FIXME)\\b"
        }
      }
    },
    "no-debugger-ts": {
      "level": "error",
      "rule": {
        "type": "file-not-contents",
        "options": {
          "globsAll": ["**/*.ts", "**/*.tsx"],
          "content": "\\bdebugger\\s*;"
        }
      }
    },
    "no-toplevel-print-py": {
      "level": "error",
      "rule": {
        "type": "file-not-contents",
        "options": {
          "globsAll": ["**/*.py"],
          "content": "^\\s*print\\s*\\("
        }
      }
    }
  }
}
"#;

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
