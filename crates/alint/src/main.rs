//! alint — language-agnostic repository linter.
//!
//! See `docs/design/ARCHITECTURE.md` for the rule model, DSL, and execution
//! model. User-facing docs are in the root `README.md`.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use alint_core::{Engine, RuleRegistry, WalkOptions, walk};
use alint_output::{ColorChoice, Format, GlyphSet, HumanOptions};
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "alint",
    version,
    about = "Language-agnostic linter for repository structure, existence, naming, and content rules",
    long_about = None,
)]
struct Cli {
    /// Path to a config file (repeatable; later overrides earlier).
    #[arg(long, short = 'c', global = true)]
    config: Vec<PathBuf>,

    /// Output format.
    #[arg(long, short = 'f', global = true, default_value = "human")]
    format: String,

    /// Disable .gitignore handling (overrides config).
    #[arg(long, global = true)]
    no_gitignore: bool,

    /// Treat warnings as errors for exit-code purposes.
    #[arg(long, global = true)]
    fail_on_warning: bool,

    /// When to emit ANSI color codes in human output. `auto` (the
    /// default) inspects TTY + `NO_COLOR` + `CLICOLOR_FORCE`.
    /// Only affects the `human` format; `json` / `sarif` / `github`
    /// are always plain bytes.
    #[arg(
        long,
        global = true,
        value_name = "WHEN",
        default_value = "auto",
        value_parser = clap::builder::PossibleValuesParser::new(["auto", "always", "never"]),
    )]
    color: String,

    /// Force ASCII glyphs in human output (e.g. `x` instead of `✗`).
    /// Auto-enabled when `TERM=dumb`.
    #[arg(long, global = true)]
    ascii: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run linters against the current (or given) directory. Default command.
    Check {
        /// Root of the repository to lint. Defaults to the current directory.
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// List all rules loaded from the effective config.
    List,
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
    },
}

fn main() -> ExitCode {
    init_tracing();
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("alint: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt};
    let filter = EnvFilter::try_from_env("ALINT_LOG").unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = fmt().with_env_filter(filter).with_target(false).try_init();
}

fn run(mut cli: Cli) -> Result<ExitCode> {
    let command = cli.command.take().unwrap_or(Command::Check {
        path: PathBuf::from("."),
    });
    match command {
        Command::Check { path } => cmd_check(&path, &cli),
        Command::List => cmd_list(&cli),
        Command::Explain { rule_id } => cmd_explain(&rule_id, &cli),
        Command::Fix { path, dry_run } => cmd_fix(&path, dry_run, &cli),
    }
}

fn cmd_check(path: &Path, cli: &Cli) -> Result<ExitCode> {
    let loaded = load_rules(path, cli)?;
    let rule_count = loaded.entries.len();
    let engine = Engine::from_entries(loaded.entries, loaded.registry)
        .with_facts(loaded.facts)
        .with_vars(loaded.vars);

    let effective_gitignore = if cli.no_gitignore {
        false
    } else {
        loaded.respect_gitignore
    };
    let walk_opts = WalkOptions {
        respect_gitignore: effective_gitignore,
        extra_ignores: loaded.extra_ignores,
    };

    let index = walk(path, &walk_opts).context("walking repository")?;
    tracing::debug!(files = index.entries.len(), "walk complete");

    let report = engine.run(path, &index).context("running rules")?;

    let format: Format = cli.format.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    let (mut out, opts) = render_env(cli)?;
    format
        .write_with_options(&report, &mut out, opts)
        .context("writing output")?;
    out.flush().ok();

    tracing::debug!(rules = rule_count, "done");

    let exit = if report.has_errors() || (cli.fail_on_warning && report.has_warnings()) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    };
    Ok(exit)
}

fn cmd_fix(path: &Path, dry_run: bool, cli: &Cli) -> Result<ExitCode> {
    let loaded = load_rules(path, cli)?;
    let engine = Engine::from_entries(loaded.entries, loaded.registry)
        .with_facts(loaded.facts)
        .with_vars(loaded.vars)
        .with_fix_size_limit(loaded.fix_size_limit);

    let effective_gitignore = if cli.no_gitignore {
        false
    } else {
        loaded.respect_gitignore
    };
    let walk_opts = WalkOptions {
        respect_gitignore: effective_gitignore,
        extra_ignores: loaded.extra_ignores,
    };

    let index = walk(path, &walk_opts).context("walking repository")?;
    let report = engine
        .fix(path, &index, dry_run)
        .context("applying fixes")?;

    let format: Format = cli.format.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    let (mut out, opts) = render_env(cli)?;
    format
        .write_fix_with_options(&report, &mut out, opts)
        .context("writing output")?;
    out.flush().ok();

    let exit = if report.has_unfixable_errors()
        || (cli.fail_on_warning && report.has_unfixable_warnings())
    {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    };
    Ok(exit)
}

fn cmd_list(cli: &Cli) -> Result<ExitCode> {
    let loaded = load_rules(Path::new("."), cli)?;
    if loaded.entries.is_empty() {
        println!("(no rules loaded from config)");
    } else {
        for entry in &loaded.entries {
            let rule = &entry.rule;
            let gated = if entry.when.is_some() { " [when]" } else { "" };
            println!(
                "{:<8} {}{}{}",
                rule.level().as_str(),
                rule.id(),
                gated,
                rule.policy_url()
                    .map(|u| format!("  ({u})"))
                    .unwrap_or_default()
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_explain(rule_id: &str, cli: &Cli) -> Result<ExitCode> {
    let loaded = load_rules(Path::new("."), cli)?;
    let Some(entry) = loaded.entries.iter().find(|e| e.rule.id() == rule_id) else {
        bail!("no rule with id {rule_id:?} found in the effective config");
    };
    let rule = &entry.rule;
    println!("id:         {}", rule.id());
    println!("level:      {}", rule.level().as_str());
    if let Some(url) = rule.policy_url() {
        println!("policy_url: {url}");
    }
    if let Some(when) = &entry.when {
        println!("when:       {when:?}");
    }
    println!("debug:      {rule:?}");
    Ok(ExitCode::SUCCESS)
}

/// Build the stdout writer + human-format options from the
/// user's `--color` / `--ascii` flags.
///
/// The returned writer is an `anstream::AutoStream` that strips
/// ANSI SGR codes automatically when the underlying stream isn't
/// a TTY (or when `NO_COLOR` is set, or when `--color=never` was
/// passed). Formatters can therefore emit styled output
/// unconditionally.
fn render_env(
    cli: &Cli,
) -> Result<(
    anstream::AutoStream<std::io::StdoutLock<'static>>,
    HumanOptions,
)> {
    let choice: ColorChoice = cli.color.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    let stdout = io::stdout();
    let is_tty = stdout.is_terminal();
    let lock = stdout.lock();
    let stream = anstream::AutoStream::new(lock, choice.to_anstream());

    // Hyperlink detection needs a TTY to matter; piped output that
    // happens to survive (because `--color=always`) still won't be
    // rendered as a link by anything downstream.
    let hyperlinks = is_tty && supports_hyperlinks::on(supports_hyperlinks::Stream::Stdout);

    let opts = HumanOptions {
        glyphs: GlyphSet::detect(cli.ascii),
        hyperlinks,
    };
    Ok((stream, opts))
}

struct LoadedConfig {
    entries: Vec<alint_core::RuleEntry>,
    registry: RuleRegistry,
    facts: Vec<alint_core::FactSpec>,
    vars: std::collections::HashMap<String, String>,
    respect_gitignore: bool,
    extra_ignores: Vec<String>,
    fix_size_limit: Option<u64>,
}

/// Load the effective config from disk and instantiate every rule,
/// parsing any `when:` clauses into AST at build time.
fn load_rules(cwd: &Path, cli: &Cli) -> Result<LoadedConfig> {
    let config_path = if let Some(first) = cli.config.first() {
        first.clone()
    } else {
        alint_dsl::discover(cwd).ok_or_else(|| {
            anyhow::anyhow!("no .alint.yml found (searched from {})", cwd.display())
        })?
    };
    tracing::debug!(?config_path, "loading config");
    let config = alint_dsl::load(&config_path)?;

    let registry: RuleRegistry = alint_rules::builtin_registry();

    let mut entries: Vec<alint_core::RuleEntry> = Vec::with_capacity(config.rules.len());
    for spec in &config.rules {
        if matches!(spec.level, alint_core::Level::Off) {
            continue;
        }
        let rule = registry
            .build(spec)
            .with_context(|| format!("building rule {:?}", spec.id))?;
        let mut entry = alint_core::RuleEntry::new(rule);
        if let Some(when_src) = &spec.when {
            let expr = alint_core::when::parse(when_src)
                .with_context(|| format!("rule {:?}: parsing `when`", spec.id))?;
            entry = entry.with_when(expr);
        }
        entries.push(entry);
    }
    Ok(LoadedConfig {
        entries,
        registry,
        facts: config.facts,
        vars: config.vars,
        respect_gitignore: config.respect_gitignore,
        extra_ignores: config.ignore,
        fix_size_limit: config.fix_size_limit,
    })
}
