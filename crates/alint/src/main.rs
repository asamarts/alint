//! alint — language-agnostic repository linter.
//!
//! See `docs/design/ARCHITECTURE.md` for the rule model, DSL, and execution
//! model. User-facing docs are in the root `README.md`.

use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use alint_output::{
    ColorChoice, sanitize_terminal as term, sanitize_terminal_multiline as term_multiline,
};
use anyhow::{Context, Result, bail};
use clap::Parser;

mod check;
mod cli;
mod config_cmd;
mod export_agents_md;
mod fix;
mod init;
mod inspect;
mod progress;
mod rules;
mod safe_write;
mod suggest;
mod test_cmd;

use check::{ChangedMode, cmd_baseline, cmd_check};
use cli::{Cli, Command, RulesCommand};
use config_cmd::{cmd_validate_config, render_env};
use fix::{FixOptions, cmd_fix};
use inspect::{ExplainOptions, cmd_explain, cmd_facts, cmd_list};

/// Long-form `alint --version` output: workspace version, git short
/// SHA, and build date. Bug reports paste this verbatim and a
/// maintainer can pinpoint the exact commit. The SHA + date come
/// from `crates/alint/build.rs` via `cargo:rustc-env`; both fall
/// back to `unknown` when built from a published tarball without
/// a git tree.
const ALINT_LONG_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("ALINT_GIT_SHA"),
    ", built ",
    env!("ALINT_BUILD_DATE"),
    ")",
);

fn main() -> ExitCode {
    init_panic_hook();
    let cli = Cli::parse();
    init_tracing(&cli.color);
    match run(cli) {
        Ok(code) => code,
        // The reader closed stdout early (`alint list | head -1`): not an
        // alint error. Exit quietly, as if the output had been consumed.
        Err(e) if is_broken_pipe(&e) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("alint: {}", terminal_error(&format!("{e:#}")));
            // Exit 3 for an internal alint error (a bug), 2 for a config /
            // CLI-usage error the user can fix (M11).
            if error_is_internal(&e) {
                ExitCode::from(3)
            } else {
                ExitCode::from(2)
            }
        }
    }
}

/// Whether the error chain carries an `alint_core::Error::Internal` — an alint
/// bug, which the CLI reports as exit code `3` rather than the `2` used for a
/// fixable config / usage error (M11).
fn error_is_internal(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<alint_core::Error>()
            .is_some_and(alint_core::Error::is_internal)
    })
}

/// Render a multi-line top-level error without letting an untrusted newline
/// escape the CLI's `alint:` record prefix.
fn terminal_error(message: &str) -> impl std::fmt::Display + '_ {
    struct Prefixed<'a>(&'a str);

    impl std::fmt::Display for Prefixed<'_> {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let safe = term_multiline(self.0);
            for (index, line) in safe.split('\n').enumerate() {
                if index > 0 {
                    formatter.write_str("\nalint: ")?;
                }
                formatter.write_str(line)?;
            }
            Ok(())
        }
    }

    Prefixed(message)
}

/// Whether the error chain carries an EPIPE: stdout's reader went away.
fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<io::Error>()
            .is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
    })
}

/// Treat a closed stdout (EPIPE, e.g. `alint check -f json | head -1`) as a
/// completed report write, so the command still returns the exit code its
/// findings call for instead of a spurious "Broken pipe" error.
fn tolerate_broken_pipe(res: io::Result<()>) -> io::Result<()> {
    match res {
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}

/// Write `text` plus a newline to stdout (locked, then flushed). A closed
/// stdout (EPIPE) is tolerated like a consumed write, so the command keeps
/// the exit code it would have had; `println!` would panic instead.
fn print_stdout_line(text: &str) -> Result<()> {
    use std::io::Write as _;
    let mut out = io::stdout().lock();
    tolerate_broken_pipe(writeln!(out, "{text}").and_then(|()| out.flush()))
        .context("writing output")
}

/// Install a custom panic hook that prints a pre-filled GitHub-issue
/// URL for the bug report. Skipped when `RUST_BACKTRACE` is set so
/// developers running with `RUST_BACKTRACE=1` keep the standard
/// backtrace path.
fn init_panic_hook() {
    if std::env::var_os("RUST_BACKTRACE").is_some() {
        return;
    }
    std::panic::set_hook(Box::new(|info| {
        let location = info.location().map_or_else(
            || "(unknown)".to_string(),
            |l| format!("{}:{}", l.file(), l.line()),
        );
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("(non-string panic payload)");
        let title = format!("alint panic: {payload}");
        let body = format!(
            "alint version: {ver}\n\
             OS: {os}\n\
             Panic location: {location}\n\
             Panic message: {payload}\n\n\
             Steps to reproduce:\n\
             1. ...\n\
             2. ...\n\
             3. ...\n\n\
             Expected behaviour:\n\n\
             Actual behaviour:\n",
            ver = ALINT_LONG_VERSION,
            os = std::env::consts::OS,
        );
        let url = format!(
            "https://github.com/asamarts/alint/issues/new?title={}&body={}",
            url_encode(&title),
            url_encode(&body),
        );
        eprintln!("\nalint crashed unexpectedly. This is a bug - please file a report:");
        eprintln!("  {url}\n");
        eprintln!("Panic: {payload}");
        eprintln!("Location: {location}");
        eprintln!("Re-run with `RUST_BACKTRACE=1` for the full backtrace.");
    }));
}

/// Minimal RFC 3986 percent-encoder for the panic-hook URL's query
/// string. Hand-rolled so the panic hook stays dependency-free —
/// pulling in `urlencoding` for one call site would expand the
/// blast radius on a code path that runs only when alint is
/// already in trouble.
fn url_encode(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        let c = *b;
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~') {
            out.push(c as char);
        } else {
            // Writing into a `String` via `write!` cannot fail — the
            // `fmt::Write` implementation on `String` is infallible.
            let _ = write!(out, "%{c:02X}");
        }
    }
    out
}

/// Whether `ALINT_LOG` diagnostics on stderr get ANSI styling: the same
/// `--color` decision the reports use, applied to stderr — `always` /
/// `CLICOLOR_FORCE` force it, `never` disables it, and `auto` requires a
/// stderr TTY and no (non-empty) `NO_COLOR`. An unparsable `--color` value
/// is treated as `auto` here (the command reports that error itself).
fn tracing_ansi(color: &str, no_color: bool, stderr_tty: bool) -> bool {
    match color.parse::<ColorChoice>().unwrap_or_default().resolve() {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => stderr_tty && !no_color,
    }
}

fn init_tracing(color: &str) {
    use tracing_subscriber::{EnvFilter, fmt};
    let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let ansi = tracing_ansi(color, no_color, io::stderr().is_terminal());
    let filter = EnvFilter::try_from_env("ALINT_LOG").unwrap_or_else(|_| EnvFilter::new("warn"));
    // Diagnostics go to stderr, never stdout: a `warn!` (e.g. an empty
    // `include_manifest_paths` set) must not corrupt `--format json`/SARIF, which
    // machine consumers read from stdout. `fmt()`'s default writer is stdout.
    let _ = fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_ansi(ansi)
        .with_writer(std::io::stderr)
        .try_init();
}

#[allow(clippy::too_many_lines)]
fn run(mut cli: Cli) -> Result<ExitCode> {
    let command = cli.command.take().unwrap_or(Command::Check {
        path: PathBuf::from("."),
        changed: false,
        base: None,
    });
    // `--only` is global so the bare default `alint --only <id>` (= check)
    // works, but it only has meaning for check/fix. On any other subcommand a
    // (possibly typo'd) `--only` was silently ignored; reject it loudly, matching
    // the flag's own "typos fail loudly" contract.
    if !cli.only.is_empty() && !matches!(command, Command::Check { .. } | Command::Fix { .. }) {
        bail!("`--only` only applies to `check` and `fix`");
    }
    if cli.as_of.is_some()
        && !matches!(
            command,
            Command::Check { .. }
                | Command::Fix { .. }
                | Command::Baseline { .. }
                | Command::Explain { .. }
                | Command::Lsp
        )
    {
        bail!("`--as-of` applies only to `check`, `fix`, `baseline`, `explain`, and `lsp`");
    }
    // `--config` is single-valued in effect: every consumer reads the first
    // entry, so a second `-c` was silently dropped while the help text wrongly
    // promised "later overrides earlier". Reject multiple rather than silently
    // pick one — layering configs is what `extends:` is for.
    if cli.config.len() > 1 {
        bail!(
            "`--config` may be given at most once (got {}); compose configs with an \
             `extends:` chain instead of repeating `-c`",
            cli.config.len()
        );
    }
    // The baseline flags only affect `check`; on every other subcommand they
    // were silently ignored, breaking the flag's own "a missing baseline is an
    // error, never a silent no-op" contract. Reject them loudly off `check`,
    // matching `--only`. (The `baseline` subcommand writes via its own
    // `--output`, not this flag.)
    // `--baseline` works on `check` AND `fix` (W4). `--strict-baseline` /
    // `--show-baselined` remain `check`-only for now (a tracked follow-up). Reject
    // loudly off their allowed commands, never a silent no-op.
    if cli.baseline.is_some() && !matches!(command, Command::Check { .. } | Command::Fix { .. }) {
        bail!(
            "`--baseline` applies only to `check` and `fix` (the `baseline` \
             subcommand writes via `--output`)"
        );
    }
    if (cli.strict_baseline || cli.show_baselined) && !matches!(command, Command::Check { .. }) {
        bail!(
            "`--strict-baseline` and `--show-baselined` apply only to `check` \
             (a baseline-aware `fix` supports `--baseline`; strict / show for \
             `fix` are not yet implemented)"
        );
    }
    // `--include-fixes` attaches `proposed_edit` to each fixable finding in
    // `check --format json`; it has no effect on any other subcommand. Reject it
    // loudly off `check` rather than silently no-op (matching `--only`/`--baseline`;
    // audit R2 LOW-1).
    if cli.include_fixes && !matches!(command, Command::Check { .. }) {
        bail!(
            "`--include-fixes` applies only to `check` (it adds `proposed_edit` to \
             the `--format json` output); it has no effect on this subcommand"
        );
    }
    match command {
        Command::Check {
            path,
            changed,
            base,
        } => cmd_check(&path, &ChangedMode::new(changed, base), &cli.only, &cli),
        Command::List { category } => cmd_list(category.as_deref(), &cli),
        Command::Explain {
            rule_id,
            trace,
            preview,
            preview_limit,
            changed,
            base,
        } => cmd_explain(
            &rule_id,
            &ExplainOptions {
                trace,
                preview_limit: preview.then_some(usize::from(preview_limit.unwrap_or(20))),
                changed: ChangedMode::new(changed, base),
            },
            &cli,
        ),
        Command::Fix {
            path,
            dry_run,
            changed,
            base,
            unsafe_fixes,
            fix_only,
            diff,
        } => cmd_fix(
            &path,
            &ChangedMode::new(changed, base),
            &FixOptions {
                dry_run,
                unsafe_fixes,
                fix_only,
                diff,
            },
            &cli.only,
            &cli,
        ),
        Command::Baseline {
            path,
            output,
            accept_new,
        } => cmd_baseline(&path, output.as_deref(), accept_new, &cli),
        Command::Facts { path } => cmd_facts(&path, &cli),
        Command::Init { path, monorepo } => {
            reject_non_human_format(&cli, "init")?;
            cmd_init(&path, monorepo)
        }
        Command::ExportAgentsMd {
            output,
            inline,
            section_title,
            include_info,
            format,
        } => cmd_export_agents_md(
            &ExportAgentsMdOptions {
                output,
                inline,
                section_title,
                include_info,
                format,
            },
            &cli,
        ),
        Command::Suggest {
            path,
            format,
            confidence,
            include_bundled,
            explain,
        } => cmd_suggest(
            &path,
            &SuggestOptions {
                format,
                confidence,
                include_bundled,
                explain,
            },
            &cli,
        ),
        Command::Test {
            paths,
            tag,
            filter,
            update_snapshots,
            allow_spawn,
            allow_host_keyring,
            format,
        } => test_cmd::run(
            &test_cmd::RunOptions {
                paths: &paths,
                tags: &tag,
                filter: filter.as_deref(),
                update_snapshots,
                allow_spawn,
                allow_host_keyring,
                format: &format,
            },
            &cli,
        ),
        Command::ValidateConfig { path, format } => cmd_validate_config(path, &format, &cli),
        Command::Lsp => {
            reject_non_human_format(&cli, "lsp")?;
            cmd_lsp(cli.show_notes, cli.as_of, cli.registry())
        }
        Command::Rules { command } => rules::run(&command, &cli),
    }
}

/// Fail loudly when a subcommand that produces no formatted report is given a
/// non-default `--format` (M13 contract: never silently ignore the flag). Used
/// by `init` and `lsp`, which — unlike `check`/`list`/`baseline`/… — take no
/// `&cli` of their own.
fn reject_non_human_format(cli: &Cli, cmd: &str) -> anyhow::Result<()> {
    if cli.format != "human" {
        anyhow::bail!(
            "`{cmd}` does not support `--format {}` - it produces no formatted report; drop `--format`",
            cli.format
        );
    }
    Ok(())
}

fn reject_non_human_baseline_format(cli: &Cli) -> anyhow::Result<()> {
    if cli.format != "human" {
        anyhow::bail!(
            "`baseline` does not support `--format {}` - it writes the baseline file and a \
             human summary; drop `--format` (use `--quiet` to silence the summary)",
            cli.format
        );
    }
    Ok(())
}

/// Start the LSP server over stdio. Blocks (running its own async
/// runtime inside `alint-lsp`) until the client sends `exit` or
/// disconnects; exits `0` after a clean `shutdown` + `exit`, `1` for an
/// `exit` without `shutdown` (per the LSP specification).
/// `--show-notes` lists informational notes on stderr after each check.
fn cmd_lsp(
    show_notes: bool,
    analysis_time: Option<alint_core::AnalysisTime>,
    registry: alint_core::RuleRegistry,
) -> Result<ExitCode> {
    let code = alint_lsp::run_stdio(alint_lsp::LspOptions {
        show_notes,
        analysis_time,
        registry: Some(registry),
    })
    .context("running language server")?;
    Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)))
}

#[derive(Debug)]
struct ExportAgentsMdOptions {
    output: Option<PathBuf>,
    inline: bool,
    section_title: Option<String>,
    include_info: bool,
    format: String,
}

fn cmd_export_agents_md(opts: &ExportAgentsMdOptions, cli: &Cli) -> Result<ExitCode> {
    use export_agents_md::{OutputFormat, RunOptions};
    let format: OutputFormat = opts
        .format
        .parse()
        .map_err(|e: String| anyhow::anyhow!(e))?;
    let section_title = opts
        .section_title
        .clone()
        .unwrap_or_else(|| "Lint rules enforced by alint".to_string());
    let run_opts = RunOptions {
        format,
        output: opts.output.clone(),
        inline: opts.inline,
        section_title,
        include_info: opts.include_info,
    };
    let registry = cli.registry();
    export_agents_md::run(
        cli.config.first().map(PathBuf::as_path),
        &run_opts,
        &registry,
    )
}

#[derive(Debug)]
struct SuggestOptions {
    format: String,
    confidence: String,
    include_bundled: bool,
    explain: bool,
}

fn cmd_suggest(path: &Path, opts: &SuggestOptions, cli: &Cli) -> Result<ExitCode> {
    use suggest::{Confidence, OutputFormat};
    let format: OutputFormat = opts
        .format
        .parse()
        .map_err(|e: String| anyhow::anyhow!(e))?;
    let confidence: Confidence = opts
        .confidence
        .parse()
        .map_err(|e: String| anyhow::anyhow!(e))?;
    let progress_mode = if cli.quiet {
        progress::ProgressMode::Never
    } else {
        cli.progress
            .parse()
            .map_err(|e: String| anyhow::anyhow!(e))?
    };
    let progress = progress::Progress::new(progress_mode);
    let (mut out, _opts) = render_env(cli)?;
    suggest::run(
        path,
        &suggest::RunOptions {
            format,
            confidence,
            include_bundled: opts.include_bundled,
            explain: opts.explain,
            quiet: cli.quiet,
            width: cli.width,
        },
        &progress,
        &mut out,
    )
}

fn cmd_init(path: &Path, monorepo: bool) -> Result<ExitCode> {
    // Refuse to overwrite an existing `.alint.yml` (or any of
    // the other names the loader recognises). The user-visible
    // contract is: `alint init` is a one-shot scaffold; if a
    // config already exists, the user knows their setup better
    // than we do.
    for name in [".alint.yml", ".alint.yaml", "alint.yml", "alint.yaml"] {
        let candidate = path.join(name);
        // `symlink_metadata`, not `is_file`: a (dangling) symlink counts as
        // existing, so `init` never writes through it to a file elsewhere.
        if std::fs::symlink_metadata(&candidate).is_ok() {
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
    safe_write::create_new(&target, body.as_bytes(), "config")?;

    let summary = init::render_summary(&detection);
    if summary.is_empty() {
        print_stdout_line(&format!(
            "Wrote {} - extends `oss-baseline@v1` only.\n  \
             No language manifests detected. Add an `extends:` line for your stack \
             (`alint://bundled/rust@v1`, `node@v1`, …) when ready.",
            term(&target.display().to_string())
        ))?;
    } else {
        print_stdout_line(&format!(
            "Wrote {} - detected: {}.\n  Run `alint check` to lint against the generated config.",
            term(&target.display().to_string()),
            summary
        ))?;
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests;
