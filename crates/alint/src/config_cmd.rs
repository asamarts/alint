//! Shared output setup and configuration loading commands.

use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use alint_core::RuleRegistry;
use alint_output::{ColorChoice, Format, GlyphSet, HumanOptions, sanitize_terminal as term};
use anyhow::{Context, Result, bail};

use super::{Cli, error_is_internal, print_stdout_line, terminal_error};

/// Build the stdout writer + human-format options from the
/// user's `--color` / `--ascii` flags.
///
/// The returned writer is an `anstream::AutoStream` that strips
/// ANSI SGR codes automatically when the underlying stream isn't
/// a TTY (or when `NO_COLOR` is set, or when `--color=never` was
/// passed). Formatters can therefore emit styled output
/// unconditionally.
pub(super) fn render_env(
    cli: &Cli,
) -> Result<(
    anstream::AutoStream<std::io::StdoutLock<'static>>,
    HumanOptions,
)> {
    let choice: ColorChoice = cli.color.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    // Pre-resolve `Auto` against CLICOLOR_FORCE before handing
    // off to anstream — anstream's Auto honors NO_COLOR + TTY
    // but doesn't consult CLICOLOR_FORCE on its own.
    let choice = choice.resolve();
    let stdout = io::stdout();
    let is_tty = stdout.is_terminal();
    let lock = stdout.lock();
    let stream = anstream::AutoStream::new(lock, choice.to_anstream());

    // Hyperlink detection needs a TTY to matter; piped output that
    // happens to survive (because `--color=always`) still won't be
    // rendered as a link by anything downstream. The
    // `ALINT_FORCE_HYPERLINKS=1` escape hatch overrides both checks:
    // used by the asciinema demo capture (stdout redirected to a
    // file, so `is_tty=false`, but the cast is replayed inside an
    // OSC-8-supporting terminal emulator and should carry the
    // hyperlink semantic). Empty / `0` values do NOT force.
    let force_hyperlinks =
        std::env::var_os("ALINT_FORCE_HYPERLINKS").is_some_and(|v| !v.is_empty() && v != *"0");
    let hyperlinks = force_hyperlinks
        || (is_tty && supports_hyperlinks::on(supports_hyperlinks::Stream::Stdout));

    // Only ask the kernel for columns when we know we're on a TTY.
    // Pipes have no useful width; let the formatter fall back to
    // its DEFAULT_WIDTH constant. `--width` always wins when set
    // (reproducible captures, narrow CI, manual overrides).
    let width = cli.width.or_else(|| {
        if is_tty {
            terminal_size::terminal_size().map(|(w, _)| usize::from(w.0))
        } else {
            None
        }
    });

    let opts = HumanOptions {
        glyphs: GlyphSet::detect(cli.ascii),
        hyperlinks,
        width,
        compact: cli.compact,
        show_docs: !cli.no_docs,
    };
    Ok((stream, opts))
}

pub(super) struct LoadedConfig {
    /// Effective repository root. When config discovery walks upward, commands
    /// operate from the discovered config's directory rather than evaluating
    /// root-relative rules against the caller's nested subdirectory.
    pub(super) root: PathBuf,
    pub(super) entries: Vec<alint_core::RuleEntry>,
    pub(super) registry: RuleRegistry,
    pub(super) facts: Vec<alint_core::FactSpec>,
    pub(super) vars: std::collections::HashMap<String, String>,
    pub(super) respect_gitignore: bool,
    pub(super) extra_ignores: Vec<String>,
    pub(super) fix_size_limit: Option<u64>,
    /// The `baseline:` config key (the raw repo-root-relative path), if set.
    pub(super) baseline: Option<PathBuf>,
    /// The loaded top-level config file.
    config_path: PathBuf,
}

impl LoadedConfig {
    /// The config file relative to `root` (for anchoring path-less findings
    /// in SARIF), or `.alint.yml` when it lives outside the root.
    pub(super) fn config_rel(&self) -> PathBuf {
        let root = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());
        let config = self
            .config_path
            .canonicalize()
            .unwrap_or_else(|_| self.config_path.clone());
        config.strip_prefix(&root).map_or_else(
            |_| PathBuf::from(alint_output::DEFAULT_CONFIG_URI),
            Path::to_path_buf,
        )
    }
}

/// Load the effective config from disk and instantiate every rule,
/// parsing any `when:` clauses into AST at build time.
/// `alint validate-config <path>` — Phase 6 of v0.9.15.
///
/// Runs the same load + build + when-parse path as `check`, but
/// stops before the engine spins up. Editor LSP, pre-commit hooks,
/// and fail-fast CI steps want to know "is the config loadable?"
/// without paying for a tree walk.
///
/// Exit codes:
/// - `0` — config valid, all N rules built cleanly
/// - `1` — config invalid (load / build / when-parse error). The
///   underlying error message includes the v0.9.15 Phase 3 +
///   Phase 4 enrichments (did-you-mean, `JSONPath` dashed-key hints,
///   `&&` → `and` keyword hints, etc.)
/// - `2` — invocation error (file missing, etc.) — propagated by
///   `main`'s top-level error handler.
/// - `3` — internal error (e.g. a bundled ruleset shipped inside the binary
///   fails to parse) — classified by `error_is_internal`, distinguishing an
///   alint bug from a user-fixable config problem (M11).
pub(super) fn cmd_validate_config(
    path: Option<PathBuf>,
    format: &str,
    cli: &Cli,
) -> Result<ExitCode> {
    // Fail loudly on a format this subcommand can't render, rather than
    // silently falling through to human output (M13) — mirroring how
    // `list` / `facts` / `explain` gate their format. This catches the value
    // regardless of position (a global `--format sarif` before the subcommand
    // is merged into this `format` by clap). validate-config has only a human
    // and a json renderer.
    let fmt: Format = format.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    if !matches!(fmt, Format::Human | Format::Json) {
        bail!(
            "`alint validate-config` supports only `--format human` or `--format json` (got {format:?})"
        );
    }

    // Resolve the config path. Three sources, in priority order:
    // 1. positional `path` arg — file path or directory (most explicit;
    //    editor LSP invocations pass the YAML file directly, while
    //    `validate-config .` is a natural shorthand for "validate the
    //    config in this repo")
    // 2. `--config` global flag (carried via `cli.config`)
    // 3. discovery from the current directory (same as `check`)
    let config_path: PathBuf = if let Some(p) = path {
        if p.is_dir() {
            if let Some(found) = alint_dsl::discover(&p) {
                found
            } else {
                let err = anyhow::anyhow!(
                    "no .alint.yml found under directory {} \
                     (run `alint init` there to scaffold one)",
                    p.display()
                );
                return emit_validate_failure(&err, None, format);
            }
        } else {
            p
        }
    } else if let Some(first) = cli.config.first() {
        first.clone()
    } else if let Some(p) = alint_dsl::discover(Path::new(".")) {
        p
    } else {
        let err = anyhow::anyhow!(
            "no .alint.yml found (searched from {}) \
             (run `alint init` to scaffold one)",
            Path::new(".").display()
        );
        return emit_validate_failure(&err, None, format);
    };

    if !config_path.exists() {
        let err = anyhow::anyhow!("config file not found: {}", config_path.display());
        return emit_validate_failure(&err, Some(&config_path), format);
    }

    // Same load + build + when-parse path as `check`, with the
    // failures plumbed back as a Result for the validate handler
    // rather than aborting the run.
    match validate_config_inner(&config_path, cli) {
        Ok((rule_count, hints)) => {
            emit_validate_success(rule_count, &hints, &config_path, format, cli)
        }
        Err(e) => emit_validate_failure(&e, Some(&config_path), format),
    }
}

fn validate_config_inner(
    config_path: &Path,
    cli: &Cli,
) -> Result<(usize, Vec<alint_core::pattern_hints::ConfigHint>)> {
    let registry = cli.registry();
    let config = alint_dsl::load_with(
        config_path,
        &alint_dsl::LoadOptions::with_catalog(registry.catalog()),
    )?;
    let mut count = 0usize;
    let mut hints = Vec::new();
    for spec in &config.rules {
        if matches!(spec.level, alint_core::Level::Off) {
            continue;
        }
        let rule = registry
            .build(spec)
            .with_context(|| format!("building rule {:?}", spec.id))?;
        // Structurally validate any nested `require:` sub-rules (unknown
        // kind / option / missing field) — the cross-file iteration rules
        // build these lazily, so without this they slip past validate-config.
        rule.validate_nested(&registry)
            .map_err(|error| spec.redact_configuration_error(error))
            .with_context(|| format!("building rule {:?}", spec.id))?;
        hints.extend(alint_core::pattern_hints::rule_tree_hints(
            &registry,
            rule.as_ref(),
            Some(spec),
        ));
        if let Some(when_src) = &spec.when {
            alint_core::when::parse(when_src)
                .map_err(|error| anyhow::anyhow!(spec.redact_configuration_message(error)))
                .with_context(|| format!("rule {:?}: parsing `when`", spec.id))?;
        }
        count += 1;
    }
    hints.extend(alint_core::pattern_hints::fact_hints(&config.facts));
    Ok((count, hints))
}

fn emit_validate_success(
    rule_count: usize,
    hints: &[alint_core::pattern_hints::ConfigHint],
    config_path: &Path,
    format: &str,
    cli: &Cli,
) -> Result<ExitCode> {
    if format == "json" {
        // Pattern hints are advisory: the config is still valid.
        let warnings: Vec<String> = hints.iter().map(ToString::to_string).collect();
        let envelope = serde_json::json!({
            "valid": true,
            "rule_count": rule_count,
            "config_path": config_path.display().to_string(),
            "error": serde_json::Value::Null,
            "warnings": warnings,
        });
        print_stdout_line(&serde_json::to_string(&envelope)?)?;
    } else {
        // human format
        if !cli.quiet {
            for hint in hints {
                eprintln!("alint: warning: {}", term(&hint.to_string()));
            }
        }
        print_stdout_line(&format!(
            "✓ Config valid: {rule_count} rule(s) loaded from {}",
            term(&config_path.display().to_string())
        ))?;
    }
    Ok(ExitCode::SUCCESS)
}

fn emit_validate_failure(
    err: &anyhow::Error,
    config_path: Option<&Path>,
    format: &str,
) -> Result<ExitCode> {
    if format == "json" {
        // Render the error chain as a single string so editors get
        // the full context including did-you-mean hints.
        let chain = format!("{err:#}");
        let envelope = serde_json::json!({
            "valid": false,
            "rule_count": 0,
            "config_path": config_path.map(|p| p.display().to_string()),
            "error": chain,
        });
        print_stdout_line(&serde_json::to_string(&envelope)?)?;
    } else {
        // Human format prints to stderr to stay out of the way of
        // stdout consumers, then a one-line summary on stdout so
        // terminals show something either way.
        eprintln!("alint: {}", terminal_error(&format!("{err:#}")));
        print_stdout_line("✗ Config invalid")?;
    }
    // An internal error (an alint bug — e.g. a shipped bundled ruleset that
    // fails to parse) is not the user's config being "invalid"; surface it as
    // exit 3 to match main()'s classification, not the exit 1 used for a
    // genuinely-invalid config (M11-F1). The error text (above) already names
    // it as internal.
    if error_is_internal(err) {
        Ok(ExitCode::from(3))
    } else {
        Ok(ExitCode::from(1))
    }
}

/// Print the load-time pattern hints ([`alint_core::pattern_hints`]) for the
/// rules about to run and for the facts, unless `--quiet`. They are warnings
/// only: findings and the exit code are unaffected.
pub(super) fn report_pattern_hints(
    entries: &[alint_core::RuleEntry],
    registry: &RuleRegistry,
    facts: &[alint_core::FactSpec],
    cli: &Cli,
) {
    if cli.quiet {
        return;
    }
    let hints = entries
        .iter()
        .flat_map(|entry| entry.pattern_hints(registry))
        .chain(alint_core::pattern_hints::fact_hints(facts));
    for hint in hints {
        eprintln!("alint: warning: {}", term(&hint.to_string()));
    }
}

pub(super) fn load_rules(cwd: &Path, cli: &Cli) -> Result<LoadedConfig> {
    load_rules_for_explain(cwd, cli, None)
}

pub(super) fn load_rules_for_explain(
    cwd: &Path,
    cli: &Cli,
    include_off_rule: Option<&str>,
) -> Result<LoadedConfig> {
    let cwd_abs = std::path::absolute(cwd)
        .with_context(|| format!("resolving repository root {}", cwd.display()))?;
    let (config_path, root) = if let Some(first) = cli.config.first() {
        // An explicitly-selected config does not redefine the lint root; the
        // command's path remains authoritative.
        (first.clone(), cwd_abs)
    } else {
        let config_path = alint_dsl::discover(cwd).ok_or_else(|| {
            anyhow::anyhow!(
                "no .alint.yml found (searched from {}) \
                 (run `alint init` to scaffold one)",
                cwd.display()
            )
        })?;
        let root = config_path
            .parent()
            .map_or_else(|| cwd_abs.clone(), Path::to_path_buf);
        (config_path, root)
    };
    tracing::debug!(?config_path, "loading config");
    let registry = cli.registry();
    let config = alint_dsl::load_with(
        &config_path,
        &alint_dsl::LoadOptions::with_catalog(registry.catalog()),
    )?;

    let mut entries: Vec<alint_core::RuleEntry> = Vec::with_capacity(config.rules.len());
    for spec in &config.rules {
        if matches!(spec.level, alint_core::Level::Off)
            && include_off_rule != Some(spec.id.as_str())
        {
            continue;
        }
        let mut rule = registry
            .build(spec)
            .with_context(|| format!("building rule {:?}", spec.id))?;
        // Structurally validate nested `require:` sub-rules at load, so a
        // typo'd nested kind/option fails here rather than lazily mid-walk
        // (or never, when the selector matches no entries).
        rule.validate_nested(&registry)
            .map_err(|error| spec.redact_configuration_error(error))
            .with_context(|| format!("building rule {:?}", spec.id))?;
        // Apply the top-level `allow_out_of_root:` policy (parsed from
        // the user's own config only — never from `extends:`). A rule
        // not opted in stays confined (the setter is a no-op for every
        // kind that doesn't honor the flag).
        let allow_out_of_root = config.allow_out_of_root.allows(&spec.id, &spec.kind);
        rule.set_allow_out_of_root(allow_out_of_root);
        // Retain the whole spec (one `Arc`-wrapped clone) so `explain`/`list`
        // render every configured detail from a single source of truth. The
        // marginal cost over the old per-field copies is ~nil — `extra` was
        // already cloned here — and nothing on the check hot path reads it.
        let mut entry = alint_core::RuleEntry::new(rule)
            .with_spec(std::sync::Arc::new(spec.clone()))
            .with_allow_out_of_root(allow_out_of_root);
        if let Some(when_src) = &spec.when {
            let expr = alint_core::when::parse(when_src)
                .map_err(|error| anyhow::anyhow!(spec.redact_configuration_message(error)))
                .with_context(|| format!("rule {:?}: parsing `when`", spec.id))?;
            entry = entry.with_when(expr);
        }
        entries.push(entry);
    }
    Ok(LoadedConfig {
        root,
        entries,
        registry,
        facts: config.facts,
        vars: config.vars,
        respect_gitignore: config.respect_gitignore,
        extra_ignores: config.ignore,
        fix_size_limit: config.fix_size_limit,
        baseline: config.baseline,
        config_path,
    })
}
