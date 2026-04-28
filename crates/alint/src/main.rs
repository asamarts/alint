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

mod init;

#[derive(Parser, Debug)]
#[command(
    name = "alint",
    version,
    about = "Language-agnostic linter for repository structure, existence, naming, and content rules",
    long_about = None,
)]
// Several independent boolean flags are the natural shape of the
// CLI surface — `--ascii`, `--compact`, `--fail-on-warning`,
// `--no-gitignore`. Collapsing them into a state-machine enum
// would obscure, not clarify.
#[allow(clippy::struct_excessive_bools)]
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
    /// Only affects the `human` format; `json` / `sarif` /
    /// `github` / `markdown` / `junit` / `gitlab` / `agent` are
    /// always plain bytes.
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

    /// Compact one-line-per-violation human output, suitable for
    /// piping into editors / grep / `wc -l`. Format:
    /// `path:line:col: level: rule-id: message`.
    #[arg(long, global = true)]
    compact: bool,

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
        /// Restrict the fix pass to files in the working-tree
        /// diff (see `alint check --changed`). Cross-file +
        /// existence rules still see the full tree.
        #[arg(long)]
        changed: bool,
        /// Base ref for `--changed`. Implies `--changed`.
        #[arg(long, value_name = "REF")]
        base: Option<String>,
    },
    /// Evaluate every `facts:` entry in the effective config and
    /// print the resolved value. Debugging aid for `when:` clauses.
    Facts {
        /// Root of the repository to evaluate facts against.
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Scaffold a starter `.alint.yml` based on the repo's
    /// detected ecosystem (and optionally workspace shape).
    /// Refuses to overwrite an existing config — delete the
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
        changed: false,
        base: None,
    });
    match command {
        Command::Check {
            path,
            changed,
            base,
        } => cmd_check(&path, &ChangedMode::new(changed, base), &cli),
        Command::List => cmd_list(&cli),
        Command::Explain { rule_id } => cmd_explain(&rule_id, &cli),
        Command::Fix {
            path,
            dry_run,
            changed,
            base,
        } => cmd_fix(&path, dry_run, &ChangedMode::new(changed, base), &cli),
        Command::Facts { path } => cmd_facts(&path, &cli),
        Command::Init { path, monorepo } => cmd_init(&path, monorepo),
    }
}

fn cmd_init(path: &Path, monorepo: bool) -> Result<ExitCode> {
    // Refuse to overwrite an existing `.alint.yml` (or any of
    // the other names the loader recognises). The user-visible
    // contract is: `alint init` is a one-shot scaffold; if a
    // config already exists, the user knows their setup better
    // than we do.
    for name in [".alint.yml", ".alint.yaml", "alint.yml", "alint.yaml"] {
        let candidate = path.join(name);
        if candidate.is_file() {
            bail!(
                "{} already exists; refusing to overwrite. Delete it first if you really \
                 want to regenerate, or edit it directly.",
                candidate.display()
            );
        }
    }

    let detection = init::detect(path, monorepo);
    let body = init::render(&detection);
    let target = path.join(".alint.yml");
    std::fs::write(&target, &body).with_context(|| format!("writing {}", target.display()))?;

    let summary = init::render_summary(&detection);
    if summary.is_empty() {
        println!(
            "Wrote {} — extends `oss-baseline@v1` only.",
            target.display()
        );
        println!(
            "  No language manifests detected. Add an `extends:` line for your stack \
             (`alint://bundled/rust@v1`, `node@v1`, …) when ready."
        );
    } else {
        println!("Wrote {} — detected: {}.", target.display(), summary);
        println!("  Run `alint check` to lint against the generated config.");
    }
    Ok(ExitCode::SUCCESS)
}

/// Resolved `--changed` / `--base` state. `--base` implies
/// `--changed`; both together identify the diff source.
#[derive(Debug)]
struct ChangedMode {
    enabled: bool,
    base: Option<String>,
}

impl ChangedMode {
    fn new(changed_flag: bool, base: Option<String>) -> Self {
        // `--base=<ref>` without `--changed` is treated as if
        // `--changed` was passed. The flag is the verb; the ref
        // is its argument. Surfacing `--base` on its own as an
        // error would be pedantic.
        let enabled = changed_flag || base.is_some();
        Self { enabled, base }
    }

    /// Resolve the changed-set from git, or `None` when the user
    /// didn't ask for `--changed`. Hard-errors when the user DID
    /// ask but git can't deliver — silently falling back to a
    /// full check would violate the user's intent.
    fn resolve(&self, root: &Path) -> Result<Option<std::collections::HashSet<PathBuf>>> {
        if !self.enabled {
            return Ok(None);
        }
        let set = alint_core::git::collect_changed_paths(root, self.base.as_deref()).ok_or_else(
            || {
                let what = self.base.as_deref().map_or_else(
                    || "git ls-files --modified --others --exclude-standard".to_string(),
                    |r| format!("git diff --name-only {r}...HEAD"),
                );
                anyhow::anyhow!(
                    "--changed requires a git repository (and `git` on PATH); \
                     `{what}` failed at {}. Run without --changed for a full check.",
                    root.display()
                )
            },
        )?;
        Ok(Some(set))
    }
}

fn cmd_check(path: &Path, changed: &ChangedMode, cli: &Cli) -> Result<ExitCode> {
    let loaded = load_rules(path, cli)?;
    let rule_count = loaded.entries.len();
    let mut engine = Engine::from_entries(loaded.entries, loaded.registry)
        .with_facts(loaded.facts)
        .with_vars(loaded.vars);
    if let Some(set) = changed.resolve(path)? {
        engine = engine.with_changed_paths(set);
    }

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

fn cmd_fix(path: &Path, dry_run: bool, changed: &ChangedMode, cli: &Cli) -> Result<ExitCode> {
    let loaded = load_rules(path, cli)?;
    let mut engine = Engine::from_entries(loaded.entries, loaded.registry)
        .with_facts(loaded.facts)
        .with_vars(loaded.vars)
        .with_fix_size_limit(loaded.fix_size_limit);
    if let Some(set) = changed.resolve(path)? {
        engine = engine.with_changed_paths(set);
    }

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

fn cmd_facts(path: &Path, cli: &Cli) -> Result<ExitCode> {
    let loaded = load_rules(path, cli)?;
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
    let values =
        alint_core::evaluate_facts(&loaded.facts, path, &index).context("evaluating facts")?;

    let format: Format = cli.format.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    render_facts(&loaded.facts, &values, format, &mut out)?;
    out.flush().ok();
    Ok(ExitCode::SUCCESS)
}

/// Render the resolved fact values in the requested format. Split out
/// from `cmd_facts` so the rendering logic is unit-testable without
/// standing up a full CLI invocation.
fn render_facts(
    facts: &[alint_core::FactSpec],
    values: &alint_core::FactValues,
    format: Format,
    out: &mut dyn Write,
) -> Result<()> {
    match format {
        Format::Json => render_facts_json(facts, values, out),
        // `human` is the default; `sarif` and `github` don't have a
        // natural facts shape — fall back to human rather than
        // surface a confusing empty document.
        _ => render_facts_human(facts, values, out),
    }
}

fn render_facts_human(
    facts: &[alint_core::FactSpec],
    values: &alint_core::FactValues,
    out: &mut dyn Write,
) -> Result<()> {
    if facts.is_empty() {
        writeln!(out, "(no facts declared in config)")?;
        return Ok(());
    }
    let id_width = facts.iter().map(|f| f.id.len()).max().unwrap_or(0);
    let kind_width = facts.iter().map(|f| f.kind.name().len()).max().unwrap_or(0);
    for spec in facts {
        let value_str = values
            .get(&spec.id)
            .map_or_else(|| "(unresolved)".to_string(), fact_value_display);
        writeln!(
            out,
            "{:<id_width$}  {:<kind_width$}  {}",
            spec.id,
            spec.kind.name(),
            value_str,
        )?;
    }
    Ok(())
}

fn render_facts_json(
    facts: &[alint_core::FactSpec],
    values: &alint_core::FactValues,
    out: &mut dyn Write,
) -> Result<()> {
    let entries: Vec<serde_json::Value> = facts
        .iter()
        .map(|spec| {
            let value = values
                .get(&spec.id)
                .map_or(serde_json::Value::Null, fact_value_json);
            serde_json::json!({
                "id": spec.id,
                "kind": spec.kind.name(),
                "value": value,
            })
        })
        .collect();
    let doc = serde_json::json!({ "facts": entries });
    writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
    Ok(())
}

fn fact_value_display(v: &alint_core::FactValue) -> String {
    match v {
        alint_core::FactValue::Bool(b) => b.to_string(),
        alint_core::FactValue::Int(n) => n.to_string(),
        alint_core::FactValue::String(s) => {
            // Quote strings so an empty value doesn't render as a
            // blank column and so leading/trailing whitespace is
            // visible.
            format!("{s:?}")
        }
    }
}

fn fact_value_json(v: &alint_core::FactValue) -> serde_json::Value {
    match v {
        alint_core::FactValue::Bool(b) => serde_json::Value::Bool(*b),
        alint_core::FactValue::Int(n) => serde_json::Value::Number((*n).into()),
        alint_core::FactValue::String(s) => serde_json::Value::String(s.clone()),
    }
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

    // Only ask the kernel for columns when we know we're on a TTY.
    // Pipes have no useful width; let the formatter fall back to
    // its DEFAULT_WIDTH constant.
    let width = if is_tty {
        terminal_size::terminal_size().map(|(w, _)| usize::from(w.0))
    } else {
        None
    };

    let opts = HumanOptions {
        glyphs: GlyphSet::detect(cli.ascii),
        hyperlinks,
        width,
        compact: cli.compact,
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

#[cfg(test)]
mod tests {
    //! Unit tests for the `facts` subcommand's renderers. The full
    //! evaluation pipeline is exercised in the `trycmd` CLI
    //! snapshot tests under `tests/cli/facts-*`.

    use super::*;
    use alint_core::{FactKind, FactSpec, FactValue, FactValues, facts::OneOrMany};
    use alint_output::Format;

    fn fact_spec(id: &str, kind: FactKind) -> FactSpec {
        FactSpec {
            id: id.to_string(),
            kind,
        }
    }

    fn any_file_exists_kind(glob: &str) -> FactKind {
        FactKind::AnyFileExists {
            any_file_exists: OneOrMany::One(glob.to_string()),
        }
    }

    fn count_files_kind(glob: &str) -> FactKind {
        FactKind::CountFiles {
            count_files: glob.to_string(),
        }
    }

    fn git_branch_kind() -> FactKind {
        FactKind::GitBranch {
            git_branch: alint_core::facts::GitBranchFact {},
        }
    }

    fn render_to_string<F>(render: F) -> String
    where
        F: FnOnce(&mut dyn Write) -> Result<()>,
    {
        let mut buf = Vec::new();
        render(&mut buf).expect("render should succeed");
        String::from_utf8(buf).expect("output should be UTF-8")
    }

    #[test]
    fn fact_kind_name_covers_every_variant() {
        assert_eq!(any_file_exists_kind("X").name(), "any_file_exists");
        assert_eq!(count_files_kind("**/*.rs").name(), "count_files");
        assert_eq!(git_branch_kind().name(), "git_branch");
        assert_eq!(
            FactKind::AllFilesExist {
                all_files_exist: OneOrMany::One("X".into()),
            }
            .name(),
            "all_files_exist"
        );
        assert_eq!(
            FactKind::FileContentMatches {
                file_content_matches: alint_core::facts::FileContentMatchesFact {
                    paths: OneOrMany::One("X".into()),
                    pattern: ".".into(),
                },
            }
            .name(),
            "file_content_matches"
        );
        assert_eq!(
            FactKind::Custom {
                custom: alint_core::facts::CustomFact { argv: vec![] },
            }
            .name(),
            "custom"
        );
    }

    #[test]
    fn fact_value_display_renders_each_variant() {
        assert_eq!(fact_value_display(&FactValue::Bool(true)), "true");
        assert_eq!(fact_value_display(&FactValue::Bool(false)), "false");
        assert_eq!(fact_value_display(&FactValue::Int(0)), "0");
        assert_eq!(fact_value_display(&FactValue::Int(42)), "42");
        assert_eq!(fact_value_display(&FactValue::Int(-1)), "-1");
        // Strings are quoted so leading/trailing whitespace is visible
        // and empty strings don't render as blank columns.
        assert_eq!(
            fact_value_display(&FactValue::String("main".into())),
            "\"main\""
        );
        assert_eq!(
            fact_value_display(&FactValue::String(String::new())),
            "\"\""
        );
    }

    #[test]
    fn fact_value_json_preserves_native_types() {
        assert_eq!(
            fact_value_json(&FactValue::Bool(true)),
            serde_json::json!(true)
        );
        assert_eq!(fact_value_json(&FactValue::Int(42)), serde_json::json!(42));
        assert_eq!(
            fact_value_json(&FactValue::String("main".into())),
            serde_json::json!("main")
        );
    }

    #[test]
    fn human_render_aligns_columns_and_covers_each_value_kind() {
        let facts = vec![
            fact_spec("is_python", any_file_exists_kind("pyproject.toml")),
            fact_spec("n_rs_files", count_files_kind("**/*.rs")),
            fact_spec("branch", git_branch_kind()),
        ];
        let mut values = FactValues::new();
        values.insert("is_python".into(), FactValue::Bool(true));
        values.insert("n_rs_files".into(), FactValue::Int(42));
        values.insert("branch".into(), FactValue::String("main".into()));

        let out = render_to_string(|w| render_facts_human(&facts, &values, w));

        // Every fact id appears once, values render natively, and
        // the kind column sits between them.
        assert!(out.contains("is_python"), "output: {out}");
        assert!(out.contains("n_rs_files"), "output: {out}");
        assert!(out.contains("branch"), "output: {out}");
        assert!(out.contains("true"));
        assert!(out.contains("42"));
        assert!(out.contains("\"main\""));
        assert!(out.contains("any_file_exists"));
        assert!(out.contains("count_files"));
        assert!(out.contains("git_branch"));
        // One line per fact.
        assert_eq!(out.lines().count(), 3);
    }

    #[test]
    fn human_render_reports_no_facts_message() {
        let out = render_to_string(|w| render_facts_human(&[], &FactValues::new(), w));
        assert_eq!(out.trim(), "(no facts declared in config)");
    }

    #[test]
    fn human_render_marks_unresolved_facts_when_value_is_missing() {
        // Simulates a case where `evaluate_facts` was only partially
        // populated — shouldn't crash, should surface the gap.
        let facts = vec![fact_spec("orphan", any_file_exists_kind("X"))];
        let out = render_to_string(|w| render_facts_human(&facts, &FactValues::new(), w));
        assert!(out.contains("(unresolved)"), "output: {out}");
    }

    #[test]
    fn json_render_emits_versioned_document_shape() {
        let facts = vec![
            fact_spec("is_go", any_file_exists_kind("go.mod")),
            fact_spec("n_py", count_files_kind("**/*.py")),
        ];
        let mut values = FactValues::new();
        values.insert("is_go".into(), FactValue::Bool(false));
        values.insert("n_py".into(), FactValue::Int(5));

        let out = render_to_string(|w| render_facts_json(&facts, &values, w));
        let parsed: serde_json::Value =
            serde_json::from_str(&out).expect("render should emit valid JSON");

        let arr = parsed
            .get("facts")
            .and_then(|v| v.as_array())
            .expect("facts: [...]");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], serde_json::json!("is_go"));
        assert_eq!(arr[0]["kind"], serde_json::json!("any_file_exists"));
        assert_eq!(arr[0]["value"], serde_json::json!(false));
        assert_eq!(arr[1]["id"], serde_json::json!("n_py"));
        assert_eq!(arr[1]["kind"], serde_json::json!("count_files"));
        assert_eq!(arr[1]["value"], serde_json::json!(5));
    }

    #[test]
    fn json_render_empty_list_is_empty_array_not_null() {
        let out = render_to_string(|w| render_facts_json(&[], &FactValues::new(), w));
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["facts"], serde_json::json!([]));
    }

    #[test]
    fn json_render_missing_value_becomes_null() {
        let facts = vec![fact_spec("orphan", any_file_exists_kind("X"))];
        let out = render_to_string(|w| render_facts_json(&facts, &FactValues::new(), w));
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["facts"][0]["value"], serde_json::Value::Null);
    }

    #[test]
    fn render_facts_dispatches_on_format() {
        let facts = vec![fact_spec("is_py", any_file_exists_kind("py"))];
        let mut values = FactValues::new();
        values.insert("is_py".into(), FactValue::Bool(true));

        let human_out = render_to_string(|w| render_facts(&facts, &values, Format::Human, w));
        assert!(human_out.contains("is_py"));
        assert!(!human_out.contains("\"facts\""));

        let json_out = render_to_string(|w| render_facts(&facts, &values, Format::Json, w));
        assert!(json_out.contains("\"facts\""));

        // `sarif` and `github` fall back to the human renderer
        // rather than emitting a confusing empty document.
        let sarif_out = render_to_string(|w| render_facts(&facts, &values, Format::Sarif, w));
        assert!(sarif_out.contains("is_py"));
        assert!(!sarif_out.contains("\"facts\""));
    }
}
