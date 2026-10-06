//! Tool abstraction for multi-tool benchmarks.
//!
//! Each [`Tool`] declares which (scenario, mode) combinations it
//! can meaningfully run, knows how to detect itself on `PATH`,
//! writes its own config into the bench tree, and builds the
//! hyperfine command string. The orchestrator iterates the
//! cartesian product of (tool × size × scenario × mode) and
//! skips combos where `tool.supports(scenario, mode) == false`
//! — that's how ls-lint is gated to S1 only, Repolinter to S2
//! only, etc.
//!
//! The harness ships `Alint`, `LsLint`, `GrepPipeline`, and
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
    /// Shell pipelines using `find`, `grep`, and (for S2) ripgrep - the small-team status
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
            "shell" | "grep" | "grep-pipeline" | "rg" => Ok(Self::GrepPipeline),
            "repolinter" => Ok(Self::Repolinter),
            other => bail!(
                "unknown tool {other:?}; expected one of alint, ls-lint, shell, repolinter, all"
            ),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Alint => "alint",
            Self::LsLint => "ls-lint",
            Self::GrepPipeline => "shell",
            Self::Repolinter => "repolinter",
        }
    }

    /// Non-zero statuses that mean a successful lint run with findings (or,
    /// for the shell S1 baseline, no unmatched filenames in its final grep).
    /// Hyperfine may ignore only these explicit codes. Repolinter also returns
    /// 1 for malformed configuration, so its planted-finding readiness probe
    /// must pass before that code is accepted for timed samples.
    pub fn hyperfine_ignored_exit_codes(self) -> &'static [i32] {
        match self {
            Self::Alint | Self::GrepPipeline | Self::Repolinter => &[1],
            Self::LsLint => &[],
        }
    }

    /// True iff this tool can meaningfully run the given
    /// `(scenario, mode)`. Out-of-scope combos are skipped at
    /// the orchestrator level rather than producing zero or
    /// nonsense rows. ls-lint is filename-only and has no
    /// `--changed`-equivalent; Repolinter covers
    /// content + existence with no filename support; the grep
    /// pipeline doesn't model the workspace bundle's
    /// cross-file rules so S3 is out of scope.
    pub fn supports(self, scenario: Scenario, mode: Mode) -> bool {
        // Keeping the arms split (rather than merging the `true`s with
        // `|`) keeps each tool's remit legible and per-tool edits local.
        #[allow(clippy::match_same_arms)]
        match (self, scenario, mode) {
            // alint: the four check scenarios run full + changed; the
            // dedicated fix scenario (SFIX) runs fix-mode ONLY — and no
            // check scenario runs fix mode. The ordering matters: the
            // SFIX/Fix true arm precedes the SFIX catch-all false arm,
            // which precedes the "any check scenario + fix" false arm,
            // which precedes the general check-scenario true arm.
            (Self::Alint, Scenario::Sfix, Mode::Fix) => true,
            (Self::Alint, Scenario::Sfix, _) => false,
            (Self::Alint, _, Mode::Fix) => false,
            (Self::Alint, _, _) => true,
            // ls-lint: filename-only, S1 + full only.
            (Self::LsLint, Scenario::S1, Mode::Full) => true,
            (Self::LsLint, _, _) => false,
            // grep pipeline: layout (S1) + content (S2), full only.
            (Self::GrepPipeline, Scenario::S1 | Scenario::S2, Mode::Full) => true,
            (Self::GrepPipeline, _, _) => false,
            // Repolinter: existence + content (S2), full only.
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
                // S1 is `find | grep`; S2 also uses ripgrep. Record every
                // executable that contributes instead of labelling the whole
                // baseline with ripgrep's version.
                let find = detect_via_version_flag("find", "--version")?;
                let grep = detect_via_version_flag("grep", "--version")?;
                let rg = detect_via_version_flag("rg", "--version")?;
                Some(format!("{find}; {grep}; {rg}"))
            }
            Self::Repolinter => {
                let repolinter = detect_via_version_flag("repolinter", "--version")?;
                let node = detect_via_version_flag("node", "--version")?;
                Some(format!("{repolinter}; node {node}"))
            }
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

    /// Remove the config staged by [`Self::setup_config`]. Generated trees are
    /// shared across tools and scenarios for a given size; cleanup prevents a
    /// previous competitor's config from changing later tools' walk inputs.
    pub fn teardown_config(self, root: &Path) -> Result<()> {
        let config = match self {
            Self::Alint => Some(".alint.yml"),
            Self::LsLint => Some(".ls-lint.yml"),
            Self::GrepPipeline => None,
            Self::Repolinter => Some("repolinter.json"),
        };
        if let Some(config) = config {
            let path = root.join(config);
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    /// Full shell command line handed to hyperfine for one
    /// row. Hyperfine spawns this via `sh -c`, so pipes /
    /// semicolons / globs work exactly as a user would type
    /// them — important for `GrepPipeline`, which uses `find`
    /// + GNU grep for S1 and `test` + `find` + ripgrep for S2.
    ///
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
                match mode {
                    Mode::Full => format!("{bin} check {root}"),
                    Mode::Changed => format!("{bin} check {root} --changed"),
                    // `--unsafe-fixes` so every op applies through compose
                    // (Unsafe ops don't downgrade to compute-only
                    // suggestions); `--dry-run` so nothing is written and
                    // the tree stays byte-stable across hyperfine runs.
                    Mode::Fix => format!("{bin} fix {root} --unsafe-fixes --dry-run"),
                }
            }
            Self::LsLint => {
                format!("ls-lint -warn -workdir {root} -config {root}/.ls-lint.yml")
            }
            Self::GrepPipeline => match scenario {
                Scenario::S1 => grep_pipeline_s1(&root, false),
                Scenario::S2 => grep_pipeline_s2(&root, false),
                Scenario::S3 | Scenario::S4 | Scenario::Sfix => {
                    unreachable!("supports() filters S3/S4/SFIX out for GrepPipeline")
                }
            },
            // `repolinter lint <root>` reads `repolinter.json`
            // at the tree root by default. We pass the tree
            // path positionally rather than via `-r` so a
            // run mirrors how a user would invoke it locally.
            Self::Repolinter => format!("repolinter lint {root}"),
        }
    }

    /// Command used by the untimed readiness check. The shell baseline's
    /// timed form suppresses findings, so its probe form keeps output visible
    /// and lets the harness prove that a planted violation was actually read.
    /// Other tools already report findings in their normal invocation.
    pub fn readiness_invocation(
        self,
        alint_bin: &Path,
        tree_root: &Path,
        scenario: Scenario,
        mode: Mode,
    ) -> String {
        if self != Self::GrepPipeline {
            return self.invocation(alint_bin, tree_root, scenario, mode);
        }

        let root = quote_for_shell(&tree_root.to_string_lossy());
        match scenario {
            Scenario::S1 => grep_pipeline_s1(&root, true),
            Scenario::S2 => grep_pipeline_s2(&root, true),
            Scenario::S3 | Scenario::S4 | Scenario::Sfix => {
                unreachable!("supports() filters S3/S4/SFIX out for GrepPipeline")
            }
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
fn grep_pipeline_s1(root: &str, emit_findings: bool) -> String {
    let sink = if emit_findings { "" } else { " >/dev/null" };
    [
        // *.rs → snake_case
        format!("find {root} -name '*.rs' -type f -printf '%f\\n' | grep -vE '^[a-z][a-z0-9_]*\\.rs$'{sink}"),
        // *.tsx → PascalCase
        format!("find {root} -name '*.tsx' -type f -printf '%f\\n' | grep -vE '^[A-Z][a-zA-Z0-9]*\\.tsx$'{sink}"),
        // *.ts → kebab-case
        format!("find {root} -name '*.ts' -type f -printf '%f\\n' | grep -vE '^[a-z][a-z0-9-]*\\.ts$'{sink}"),
        // *.yaml → kebab-case
        format!("find {root} -name '*.yaml' -type f -printf '%f\\n' | grep -vE '^[a-z][a-z0-9-]*\\.yaml$'{sink}"),
        // *.yml → kebab-case
        format!("find {root} -name '*.yml' -type f -printf '%f\\n' | grep -vE '^[a-z][a-z0-9-]*\\.yml$'{sink}"),
        // *.md → broad alphanumeric
        format!("find {root} -name '*.md' -type f -printf '%f\\n' | grep -vE '^[a-zA-Z0-9_.-]+$'{sink}"),
        // *.json → broad alphanumeric
        format!("find {root} -name '*.json' -type f -printf '%f\\n' | grep -vE '^[a-zA-Z0-9_.-]+$'{sink}"),
        // *.py → snake_case
        format!("find {root} -name '*.py' -type f -printf '%f\\n' | grep -vE '^[a-z][a-z0-9_]*\\.py$'{sink}"),
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
fn grep_pipeline_s2(root: &str, emit_findings: bool) -> String {
    let sink = if emit_findings { "" } else { " >/dev/null" };
    [
        // Layout — README + LICENSE existence at root.
        format!("test -f {root}/README.md || test -f {root}/README"),
        format!(
            "test -f {root}/LICENSE || test -f {root}/LICENSE.md || test -f {root}/LICENSE.txt"
        ),
        // Layout — forbidden file extensions anywhere.
        format!("find {root} -name '*.bak' -type f{sink}"),
        format!("find {root} -name '*.orig' -type f{sink}"),
        // Content — TODO / XXX / FIXME in Rust.
        format!("rg --type rust --no-messages '\\b(TODO|XXX|FIXME)\\b' {root}{sink} || true"),
        // Content — `debugger;` in TS / TSX.
        format!("rg --type ts --no-messages '\\bdebugger\\s*;' {root}{sink} || true"),
        // Content — top-level print() in Python.
        format!("rg --type py --no-messages '^\\s*print\\s*\\(' {root}{sink} || true"),
        // Size — files larger than 10 MiB.
        format!("find {root} -type f -size +10M{sink}"),
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

/// `.ls-lint.yml` body for scenario S1 — the eight filename rules
/// that are the ls-lint-expressible SUBSET of alint's `s1_layout`
/// scenario. Both engines walk the tree once and match each file's
/// basename against the configured class per extension; that filename
/// work lines up cleanly. `s1_layout`'s non-filename rules (existence
/// / size / `scope_filter`) have no ls-lint equivalent, so the S1
/// competitive cell compares filename-hygiene work only.
const LS_LINT_S1_CONFIG: &str = r"# ls-lint config — the filename subset of xtask/src/bench/scenarios/s1_layout.yml.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ls_lint_invocation_uses_explicit_tree_config_and_warn_mode() {
        let command = Tool::LsLint.invocation(
            Path::new("/tmp/alint"),
            Path::new("/tmp/tree with space"),
            Scenario::S1,
            Mode::Full,
        );
        assert_eq!(
            command,
            "ls-lint -warn -workdir '/tmp/tree with space' -config '/tmp/tree with space'/.ls-lint.yml"
        );
    }

    #[test]
    fn shell_readiness_invocation_keeps_findings_visible() {
        let timed = Tool::GrepPipeline.invocation(
            Path::new("/tmp/alint"),
            Path::new("/tmp/tree"),
            Scenario::S1,
            Mode::Full,
        );
        let readiness = Tool::GrepPipeline.readiness_invocation(
            Path::new("/tmp/alint"),
            Path::new("/tmp/tree"),
            Scenario::S1,
            Mode::Full,
        );
        assert!(timed.contains(">/dev/null"));
        assert!(!readiness.contains(">/dev/null"));
        assert!(readiness.contains("grep -vE"));
    }

    #[test]
    fn ignored_exit_codes_are_tool_specific() {
        assert_eq!(Tool::Alint.hyperfine_ignored_exit_codes(), &[1]);
        assert_eq!(Tool::LsLint.hyperfine_ignored_exit_codes(), &[] as &[i32]);
        assert_eq!(Tool::GrepPipeline.hyperfine_ignored_exit_codes(), &[1]);
        assert_eq!(Tool::Repolinter.hyperfine_ignored_exit_codes(), &[1]);
    }

    #[test]
    fn staged_configs_do_not_leak_between_tools() {
        let root = tempfile::tempdir().unwrap();
        let cases = [
            (Tool::Alint, Scenario::S1, ".alint.yml"),
            (Tool::LsLint, Scenario::S1, ".ls-lint.yml"),
            (Tool::Repolinter, Scenario::S2, "repolinter.json"),
        ];
        for (tool, scenario, config) in cases {
            tool.setup_config(root.path(), scenario).unwrap();
            assert!(root.path().join(config).is_file());
            tool.teardown_config(root.path()).unwrap();
            assert!(!root.path().join(config).exists());
        }
        Tool::GrepPipeline.teardown_config(root.path()).unwrap();
    }
}
