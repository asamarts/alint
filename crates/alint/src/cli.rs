//! The `alint` CLI surface: the clap `Parser` / `Subcommand` types.
//!
//! Split out of `main.rs` so the argument definitions live in one place,
//! separate from command dispatch and rendering. This also keeps the
//! `rust-file-max-lines` budget honest now that every command and option
//! carries a short summary plus fuller long help.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::ALINT_LONG_VERSION;

#[derive(Parser, Debug)]
#[command(
    name = "alint",
    version,
    long_version = ALINT_LONG_VERSION,
    about = "Language-agnostic linter for repository structure, existence, naming, and content rules",
    long_about = None,
    // Cap wrapping at 100 cols so descriptions stay readable on ultra-wide
    // terminals; `wrap_help` (clap feature) does the actual terminal-width
    // wrapping, keeping each description inside its own column instead of
    // letting the terminal soft-wrap a 500-char line to column 0.
    max_term_width = 100,
)]
// Several independent boolean flags are the natural shape of the
// CLI surface — `--ascii`, `--compact`, `--fail-on-warning`,
// `--no-gitignore`. Collapsing them into a state-machine enum
// would obscure, not clarify.
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct Cli {
    /// Path to a config file.
    #[arg(long, short = 'c', global = true)]
    pub(crate) config: Vec<PathBuf>,

    /// Output format.
    #[arg(long, short = 'f', global = true, default_value = "human")]
    pub(crate) format: String,

    /// Disable .gitignore handling (overrides config).
    #[arg(long, global = true)]
    pub(crate) no_gitignore: bool,

    /// Treat warnings as errors for exit-code purposes.
    #[arg(long, global = true)]
    pub(crate) fail_on_warning: bool,

    /// List informational notes in full on stderr.
    ///
    /// Notes are non-violation findings — e.g. entries a rule skipped
    /// rather than failed on. By default only a one-line count is shown.
    #[arg(long, global = true)]
    pub(crate) show_notes: bool,

    /// When to emit ANSI color codes in human output.
    ///
    /// `auto` (the default) inspects TTY + `NO_COLOR` +
    /// `CLICOLOR_FORCE`. Only affects the `human` format; `json` /
    /// `sarif` / `github` / `markdown` / `junit` / `gitlab` /
    /// `agent` are always plain bytes.
    #[arg(
        long,
        global = true,
        value_name = "WHEN",
        default_value = "auto",
        value_parser = clap::builder::PossibleValuesParser::new(["auto", "always", "never"]),
    )]
    pub(crate) color: String,

    /// Force ASCII glyphs in human output.
    ///
    /// E.g. `x` instead of `✗`. Auto-enabled when `TERM=dumb`.
    #[arg(long, global = true)]
    pub(crate) ascii: bool,

    /// Compact one-line-per-violation human output.
    ///
    /// Suitable for piping into editors / grep / `wc -l`. Format:
    /// `path:line:col: level: rule-id: message` (the `:line:col` is
    /// omitted for findings with no specific location).
    #[arg(long, global = true)]
    pub(crate) compact: bool,

    /// Override the human-output column width.
    ///
    /// Default: detected terminal width (TTY only) or 80. Useful
    /// for reproducible captures (asciinema/screen recordings) and
    /// for piping into fixed-width log viewers. Clamped to [40, 120].
    #[arg(long, global = true, value_name = "COLS")]
    pub(crate) width: Option<usize>,

    /// Suppress per-violation `docs:` URLs in human output.
    ///
    /// Useful for narrow terminals, screen recordings, and CI logs
    /// where long URLs disrupt visual alignment. URLs remain in
    /// JSON / SARIF / GitHub / markdown output regardless.
    #[arg(long, global = true)]
    pub(crate) no_docs: bool,

    /// When to render progress on stderr for slow operations.
    ///
    /// Currently just `alint suggest`. `auto` (the default) renders
    /// when stderr is a TTY; `always` forces; `never` silences.
    /// Progress always lives on stderr — `--format` JSON output on
    /// stdout stays byte-clean.
    #[arg(
        long,
        global = true,
        value_name = "WHEN",
        default_value = "auto",
        value_parser = clap::builder::PossibleValuesParser::new(["auto", "always", "never"]),
    )]
    pub(crate) progress: String,

    /// Suppress progress and stderr summary lines.
    ///
    /// Alias for `--progress=never` plus suppression of the
    /// "found N proposals in Ts" footer that `suggest` prints.
    #[arg(long, short = 'q', global = true)]
    pub(crate) quiet: bool,

    /// Suppress violations recorded in a baseline file.
    ///
    /// See `alint baseline` to create one; only new violations are
    /// reported. Pre-existing findings are grandfathered so `check`
    /// can gate a legacy repo on new violations only. A missing or
    /// unreadable baseline is an error (never a silent no-op). The
    /// path is resolved relative to the current directory (not the
    /// checked PATH); the `baseline:` config key, by contrast,
    /// resolves against the repo root.
    #[arg(long, global = true, value_name = "FILE")]
    pub(crate) baseline: Option<PathBuf>,

    /// With `--baseline`, fail on stale baseline entries.
    ///
    /// Exits 1 when the baseline has stale entries: recorded findings
    /// that no longer fire, usually because they were fixed. Forces
    /// the committed baseline to stay exactly accurate. Off by
    /// default: fixing things never fails the build.
    #[arg(long, global = true)]
    pub(crate) strict_baseline: bool,

    /// With `--baseline`, list suppressed findings on stderr.
    ///
    /// Lists them in full, rather than just a one-line count.
    /// Parallels `--show-notes`.
    #[arg(long, global = true)]
    pub(crate) show_baselined: bool,

    /// Restrict the run to the named rule id(s).
    ///
    /// Taken from the effective config (repeatable). Other rules are
    /// skipped entirely. An id that matches no loaded rule is an error,
    /// so typos fail loudly rather than silently linting nothing.
    /// Applies to `check` and `fix` (the `agent` format emits `fix
    /// --only <rule-id>`); rejected on any other subcommand. Global, so
    /// the bare `alint --only <id>` lints the current directory like
    /// `alint check --only <id>` — to lint a different path, use the
    /// explicit form `alint check --only <id> <path>`.
    #[arg(long, global = true, value_name = "RULE_ID")]
    pub(crate) only: Vec<String>,

    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// Run linters against the current (or given) directory. Default command.
    Check {
        /// Root of the repository to lint. Defaults to the current directory.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Restrict the check to files in the working-tree diff.
        /// Without `--base`, uses
        /// `git ls-files --modified --others --exclude-standard`
        /// (right shape for pre-commit). With `--base`, uses
        /// `git diff --name-only <base>...HEAD` (right shape for
        /// PR checks). Cross-file rules (`pair`, `for_each_dir`,
        /// `every_matching_has`, `unique_by`, `dir_contains`,
        /// `dir_only_contains`) and existence rules (`file_exists`
        /// et al.) still consult the full tree by definition.
        #[arg(long)]
        changed: bool,
        /// Base ref for `--changed` (uses three-dot
        /// `<base>...HEAD`, i.e. merge-base diff). Implies
        /// `--changed`.
        #[arg(long, value_name = "REF")]
        base: Option<String>,
    },
    /// List the rules configured in this repo.
    ///
    /// Reflects THIS repo's effective config (after `extends:` resolution).
    /// To browse the full catalog of rule kinds alint ships instead, use
    /// `alint rules list`.
    List {
        /// Only rules whose kind is in this category (slug; see
        /// `alint rules categories`).
        #[arg(long)]
        category: Option<String>,
    },
    /// Show a rule's definition.
    Explain {
        /// Rule id to describe.
        rule_id: String,
    },
    /// Apply automatic fixes for violations whose rules declare one.
    Fix {
        /// Root of the repository to operate on.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Print what would be done without writing anything.
        #[arg(long)]
        dry_run: bool,
        /// Restrict the fix pass to files in the working-tree
        /// diff (see `alint check --changed`). Cross-file +
        /// existence rules still see the full tree.
        #[arg(long)]
        changed: bool,
        /// Base ref for `--changed`. Implies `--changed`.
        #[arg(long, value_name = "REF")]
        base: Option<String>,
    },
    /// Snapshot current violations so later runs fail only on new ones.
    ///
    /// Writes them to a baseline file, so a later `alint check
    /// --baseline <file>` fails only on NEW violations. The one-step
    /// way to adopt alint as a blocking gate on a legacy repo: `alint
    /// baseline` (commit it), then gate on the delta. The baseline is
    /// whole-tree; `--changed` is not accepted.
    Baseline {
        /// Root of the repository to snapshot. Defaults to the current
        /// directory.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Where to write the baseline. Default: `.alint-baseline.json`
        /// at the repo root.
        #[arg(long, value_name = "FILE")]
        output: Option<PathBuf>,
        /// Allow the regenerated baseline to grandfather violations not
        /// already present in the existing file. Without it, `alint
        /// baseline` refuses to ADD new entries (and prints a `+N / -M`
        /// summary) so re-running it to prune fixed entries can't
        /// silently accept new debt. Stale-entry removal never needs it.
        #[arg(long)]
        accept_new: bool,
    },
    /// Evaluate the config's `facts:` entries and print resolved values.
    ///
    /// A debugging aid for `when:` clauses: prints the resolved value of
    /// every `facts:` entry in the effective config.
    Facts {
        /// Root of the repository to evaluate facts against.
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Scaffold a starter `.alint.yml` for the detected ecosystem.
    ///
    /// Detects the ecosystem (and optionally workspace shape) from the
    /// repo. Refuses to overwrite an existing config — delete the
    /// existing one first if you really mean it.
    Init {
        /// Root of the repository to write the config into.
        /// Defaults to the current directory.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Detect workspace shape (Cargo `[workspace]`,
        /// pnpm-workspace.yaml, or `package.json` `workspaces`)
        /// and add the corresponding `monorepo@v1` +
        /// `monorepo/<flavor>-workspace@v1` overlays.
        /// `nested_configs: true` is set on the generated
        /// config so each subdirectory can layer its own
        /// `.alint.yml` on top.
        #[arg(long)]
        monorepo: bool,
    },
    /// Generate an `AGENTS.md` section from the active rule set.
    ///
    /// Keeps the agent's pre-prompt directives in sync with the lint
    /// config. Outputs to stdout by default; use `--output PATH` to
    /// write a file, or `--inline --output PATH` to splice between
    /// `<!-- alint:start -->` / `<!-- alint:end -->` markers.
    ExportAgentsMd {
        /// Output destination. Without `--inline`, the file is
        /// overwritten. Omit for stdout.
        #[arg(long, value_name = "PATH")]
        output: Option<PathBuf>,
        /// Splice the generated section between
        /// `<!-- alint:start -->` and `<!-- alint:end -->`
        /// markers in `--output PATH`. Markers are
        /// auto-created (with a stderr warning) when the
        /// target file lacks them.
        #[arg(long, requires = "output")]
        inline: bool,
        /// Heading text for the generated section. Default:
        /// "Lint rules enforced by alint".
        #[arg(long, value_name = "TEXT")]
        section_title: Option<String>,
        /// Include `level: info` rules. Default omits them —
        /// info-level rules are nudges, not directives.
        #[arg(long)]
        include_info: bool,
        /// Output format. `markdown` (default) is the canonical
        /// `AGENTS.md` shape; `json` is parallel to `suggest`'s
        /// JSON envelope for agent consumption.
        #[arg(
            long,
            short = 'f',
            value_name = "FORMAT",
            default_value = "markdown",
            value_parser = clap::builder::PossibleValuesParser::new(["markdown", "json"]),
        )]
        format: String,
    },
    /// Scan for antipatterns and propose rules that would catch them.
    ///
    /// Prints proposals to stdout for review — never edits the user's
    /// config. Pairs naturally with `alint init` for a smarter
    /// cold-start adoption flow.
    Suggest {
        /// Root of the repository to scan. Defaults to the
        /// current directory.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Output format. `human` (default) is colorised for
        /// terminals; `yaml` is a paste-ready config snippet;
        /// `json` is a stable shape suitable for agent
        /// consumption.
        #[arg(
            long,
            short = 'f',
            value_name = "FORMAT",
            default_value = "human",
            value_parser = clap::builder::PossibleValuesParser::new(["human", "yaml", "json"]),
        )]
        format: String,
        /// Lower bound on signal strength for proposals. `low`
        /// is broadest (helpful when prospecting); `high` is
        /// strict (only ecosystem-marker hits and equivalents).
        #[arg(
            long,
            value_name = "LEVEL",
            default_value = "medium",
            value_parser = clap::builder::PossibleValuesParser::new(["low", "medium", "high"]),
        )]
        confidence: String,
        /// Include bundled-ruleset suggestions even if the
        /// existing `.alint.yml` already extends them.
        #[arg(long)]
        include_bundled: bool,
        /// Print one-line file-level evidence under each
        /// proposal so reviewers can decide quickly.
        #[arg(long)]
        explain: bool,
    },
    /// Parse-validate an `.alint.yml` without walking the tree.
    ///
    /// Resolves `extends:`, builds every rule, and parses every
    /// `when:`, reporting any errors. For editor LSP, pre-commit
    /// hooks, and fail-fast CI steps that just want to know "is the
    /// config loadable?". Exit 0 on success; exit 1 on validation
    /// failure.
    ValidateConfig {
        /// Path to the config file to validate. Defaults to the
        /// `.alint.yml` discovered upward from the current
        /// directory (same discovery rules as `alint check`).
        path: Option<PathBuf>,
        /// Output format. `human` prints a one-line success or a
        /// rich error trace; `json` emits a stable
        /// `{"valid": bool, "rule_count": N, "config_path": ...,
        /// "error": "...?"}` envelope for editor / CI consumption.
        #[arg(
            long,
            short = 'f',
            value_name = "FORMAT",
            default_value = "human",
            value_parser = clap::builder::PossibleValuesParser::new(["human", "json"]),
        )]
        format: String,
    },
    /// Start the alint language server (LSP over stdio).
    ///
    /// Editor integrations (VS Code, Zed, Neovim, and others) spawn
    /// this and drive it via the Language Server Protocol; it is not
    /// meant to be run interactively. Publishes diagnostics for the
    /// workspace's `.alint.yml` rules on document open and save.
    Lsp,
    /// Browse the catalog of rule kinds alint ships (config-independent).
    ///
    /// Use `alint list` for the rules configured in THIS repo; `alint rules`
    /// never reads a config and works anywhere.
    Rules {
        #[command(subcommand)]
        command: RulesCommand,
    },
}

/// Subcommands of `alint rules` (catalog discovery). See ADR-0009.
#[derive(Subcommand, Debug)]
pub(crate) enum RulesCommand {
    /// List rule kinds in the catalog, optionally filtered. Reads no config.
    List {
        /// Only kinds in this category (slug, e.g. `security-unicode-sanity`;
        /// run `alint rules categories` for the list).
        #[arg(long)]
        category: Option<String>,
        /// Case-insensitive substring filter on the kind name (and its aliases).
        #[arg(long)]
        search: Option<String>,
    },
    /// List the rule categories: slug, title, and how many kinds each holds.
    Categories,
}
