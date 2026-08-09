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
use clap::Parser;

mod cli;
mod export_agents_md;
mod init;
mod progress;
mod rules;
mod suggest;

use cli::{Cli, Command, RulesCommand};

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
    init_tracing();
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("alint: {e:#}");
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
        eprintln!("\nalint crashed unexpectedly. This is a bug — please file a report:");
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
            // io::Write impl on String is infallible.
            let _ = write!(out, "%{c:02X}");
        }
    }
    out
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
    // `--only` is global so the bare default `alint --only <id>` (= check)
    // works, but it only has meaning for check/fix. On any other subcommand a
    // (possibly typo'd) `--only` was silently ignored; reject it loudly, matching
    // the flag's own "typos fail loudly" contract.
    if !cli.only.is_empty() && !matches!(command, Command::Check { .. } | Command::Fix { .. }) {
        bail!("`--only` only applies to `check` and `fix`");
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
    if (cli.baseline.is_some() || cli.strict_baseline || cli.show_baselined)
        && !matches!(command, Command::Check { .. })
    {
        bail!(
            "`--baseline`, `--strict-baseline`, and `--show-baselined` apply only to \
             `check` (the `baseline` subcommand writes via `--output`)"
        );
    }
    match command {
        Command::Check {
            path,
            changed,
            base,
        } => cmd_check(&path, &ChangedMode::new(changed, base), &cli.only, &cli),
        Command::List { category } => cmd_list(category.as_deref(), &cli),
        Command::Explain { rule_id } => cmd_explain(&rule_id, &cli),
        Command::Fix {
            path,
            dry_run,
            changed,
            base,
        } => cmd_fix(
            &path,
            dry_run,
            &ChangedMode::new(changed, base),
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
        Command::ValidateConfig { path, format } => cmd_validate_config(path, &format, &cli),
        Command::Lsp => {
            reject_non_human_format(&cli, "lsp")?;
            cmd_lsp()
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
            "`{cmd}` does not support `--format {}` — it produces no formatted report; drop `--format`",
            cli.format
        );
    }
    Ok(())
}

/// Start the LSP server over stdio. Blocks (running its own async
/// runtime inside `alint-lsp`) until the client disconnects.
fn cmd_lsp() -> Result<ExitCode> {
    alint_lsp::run_stdio().context("running language server")?;
    Ok(ExitCode::SUCCESS)
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
    export_agents_md::run(cli.config.first().map(PathBuf::as_path), &run_opts)
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

/// Filter loaded rule entries to the ids named in `--only` (a no-op
/// when `only` is empty). Every `--only` id must match a loaded rule;
/// an unmatched id is an error so a typo fails loudly instead of
/// silently selecting nothing. Shared by `check` and `fix`.
fn apply_only_filter(
    entries: Vec<alint_core::RuleEntry>,
    only: &[String],
) -> Result<Vec<alint_core::RuleEntry>> {
    if only.is_empty() {
        return Ok(entries);
    }
    let wanted: std::collections::HashSet<&str> = only.iter().map(String::as_str).collect();
    let present: std::collections::HashSet<&str> = entries.iter().map(|e| e.rule.id()).collect();
    let mut missing: Vec<&str> = wanted
        .iter()
        .copied()
        .filter(|id| !present.contains(id))
        .collect();
    if !missing.is_empty() {
        missing.sort_unstable();
        bail!(
            "no rule with id {} found in the effective config (passed via --only)",
            missing
                .iter()
                .map(|id| format!("{id:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(entries
        .into_iter()
        .filter(|e| wanted.contains(e.rule.id()))
        .collect())
}

/// Reject a PATH that isn't a directory. `check`/`fix`/`baseline` operate on a
/// repository ROOT (a directory); given a single file they would walk nothing
/// and exit 0 — a silent false "all passed". Fail loudly instead.
fn require_directory(path: &Path) -> Result<()> {
    if !path.is_dir() {
        bail!(
            "{} is {}, but `check`/`fix`/`baseline` take a repository root (a \
             directory), not a single file",
            path.display(),
            if path.exists() { "a file" } else { "not found" },
        );
    }
    Ok(())
}

/// The baseline file's path relative to the lint `root`, as a walk-exclude
/// pattern — so a broad-glob content rule can't lint alint's own committed
/// JSON-Lines baseline artifact as a spurious "new" violation. Returns `None`
/// when the baseline lives outside the lint root (then it isn't walked anyway)
/// or doesn't exist yet (the missing-baseline error surfaces at load time).
fn baseline_walk_exclude(root: &Path, baseline: &Path) -> Option<String> {
    let root_abs = root.canonicalize().ok()?;
    let base_abs = baseline.canonicalize().ok()?;
    let rel = base_abs.strip_prefix(&root_abs).ok()?;
    // Root-anchored (leading `/`): the walker turns this into an override
    // `!/…` that matches ONLY the baseline at the repo root, never a same-named
    // file in a subdirectory. Without the anchor a separator-less pattern
    // matches at any depth, silently dropping a real violation in e.g.
    // `sub/.alint-baseline.json`.
    Some(format!("/{}", rel.to_string_lossy().replace('\\', "/")))
}

/// Append the [`baseline_walk_exclude`] pattern for `baseline` (when set) to
/// `extra_ignores`, so `check` / `baseline` / `fix` all keep alint's own
/// committed JSON-Lines artifact out of the walk. A broad-glob content rule
/// (`**/*.json`, `line_max_width`, `no_trailing_whitespace`, …) would otherwise
/// flag it — breaking the adopt-flow for `check`, making `baseline`
/// regeneration see the artifact as fresh debt, and letting `fix` rewrite it.
fn exclude_baseline_from_walk(
    extra_ignores: &mut Vec<String>,
    root: &Path,
    baseline: Option<&Path>,
) {
    if let Some(bp) = baseline
        && let Some(rel) = baseline_walk_exclude(root, bp)
    {
        extra_ignores.push(rel);
    }
}

fn cmd_check(path: &Path, changed: &ChangedMode, only: &[String], cli: &Cli) -> Result<ExitCode> {
    require_directory(path)?;
    let loaded = load_rules(path, cli)?;
    // The `baseline:` config key, resolved against the repo root being checked.
    // The `--baseline` flag (used as given) overrides it; either one turns on
    // baseline suppression. No silent auto-detect of `.alint-baseline.json`.
    let config_baseline = loaded.baseline.as_ref().map(|b| path.join(b));
    // Resolved early (was below the walk) so the baseline file can be excluded
    // from the walk. The `--baseline` flag (used as given) overrides the key.
    let effective_baseline = cli.baseline.clone().or(config_baseline);
    let entries = apply_only_filter(loaded.entries, only)?;
    let rule_count = entries.len();
    let mut engine = Engine::from_entries(entries, loaded.registry)
        .with_facts(loaded.facts)
        .with_vars(loaded.vars);
    let changed_active = match changed.resolve(path)? {
        Some(set) => {
            engine = engine.with_changed_paths(set);
            true
        }
        None => false,
    };

    let effective_gitignore = if cli.no_gitignore {
        false
    } else {
        loaded.respect_gitignore
    };
    // Keep alint's own baseline artifact out of the walk (see the helper).
    let mut extra_ignores = loaded.extra_ignores;
    exclude_baseline_from_walk(&mut extra_ignores, path, effective_baseline.as_deref());
    let walk_opts = WalkOptions {
        respect_gitignore: effective_gitignore,
        extra_ignores,
    };

    let index = walk(path, &walk_opts).context("walking repository")?;
    tracing::debug!(files = index.entries.len(), "walk complete");

    let report = engine.run(path, &index).context("running rules")?;

    let format: Format = cli.format.parse().map_err(|e: String| anyhow::anyhow!(e))?;

    // Baseline suppression (when --baseline is in effect): grandfather
    // recorded violations, leaving only new ones to format + gate on.
    let mut strict_stale_fail = false;
    let (report, baseline_marks) = if let Some(baseline_path) = &effective_baseline {
        let baseline = load_baseline(baseline_path)?;
        let mut reader = FileReader::new(path);
        let applied =
            alint_core::baseline::apply(&report, &baseline, |rid, v| reader.fingerprint(rid, v));
        // Stale-entry detection is only valid on a FULL run. When `--changed`
        // or `--only` restricts what was evaluated, a baseline entry for an
        // out-of-scope file/rule legitimately "doesn't fire this run" but is
        // NOT stale (the finding still exists, it just wasn't checked).
        // Reporting/failing on those red-lights the documented
        // `--changed --baseline --strict-baseline` PR-gate recipe.
        let scoped = changed_active || !only.is_empty();
        report_baseline_summary(&applied, cli, !scoped);
        if cli.strict_baseline && !scoped && !applied.stale.is_empty() {
            strict_stale_fail = true;
        }
        // Only SARIF and JSON consume the marks; building them clones every
        // suppressed finding, so skip that work for the formats that ignore
        // the baseline and emit only the live (new) findings.
        let marks =
            matches!(format, Format::Sarif | Format::Json).then(|| build_baseline_marks(&applied));
        (applied.live, marks)
    } else {
        (report, None)
    };

    let (mut out, opts) = render_env(cli)?;
    // SARIF and JSON render baselined findings (marked / counted) so Code
    // Scanning dismisses rather than re-opens them; every other format ignores
    // the baseline and emits only the live (new) findings.
    match (format, baseline_marks.as_ref()) {
        (Format::Sarif, Some(marks)) => {
            alint_output::write_sarif_with_baseline(&report, Some(marks), &mut out)
        }
        (Format::Sarif, None) => {
            // No baseline, but still emit the canonical `partialFingerprints` so
            // GitHub Code Scanning can correlate alerts across runs (these were
            // baseline-only before). Same fingerprints the GitLab path uses.
            let fps = report_fingerprints(&report, path);
            alint_output::write_sarif_with_fingerprints(&report, Some(&fps), &mut out)
        }
        (Format::Json, Some(marks)) => alint_output::write_json_with_baseline(
            &report,
            Some(marks),
            cli.show_baselined,
            &mut out,
        ),
        (Format::Gitlab, _) => {
            // Unify GitLab Code Quality's `fingerprint` with the baseline/SARIF
            // identity (was a self-contained occurrence hash): the canonical
            // per-violation `violation_fingerprint` (file-content-aware, cached
            // reads), so a finding has ONE fingerprint across GitLab, SARIF, and
            // baseline mode.
            let fps = report_fingerprints(&report, path);
            alint_output::write_gitlab(&report, Some(&fps), &mut out)
        }
        _ => format.write_with_options(&report, &mut out, opts),
    }
    .context("writing output")?;
    out.flush().ok();

    // Informational notes (non-violation findings) — surfaced on
    // stderr for the human format so stdout stays clean. JSON carries
    // them in its `notes` array instead. A one-line count by default;
    // the full list with `--show-notes`.
    if format == Format::Human {
        report_notes_to_stderr(&report, cli.show_notes);
    }

    tracing::debug!(rules = rule_count, "done");

    let exit = if report.has_errors()
        || (cli.fail_on_warning && report.has_warnings())
        || strict_stale_fail
    {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    };
    Ok(exit)
}

/// Caches file reads while fingerprinting a report's violations: the
/// line-content discriminator ([`alint_core::baseline`]) needs the
/// offending file's bytes, and a file usually carries several
/// violations.
struct FileReader<'a> {
    root: &'a Path,
    cache: std::collections::HashMap<PathBuf, Option<Vec<u8>>>,
}

impl<'a> FileReader<'a> {
    fn new(root: &'a Path) -> Self {
        Self {
            root,
            cache: std::collections::HashMap::new(),
        }
    }

    fn fingerprint(&mut self, rule_id: &str, v: &alint_core::Violation) -> String {
        let bytes = match v.path.as_ref() {
            Some(p) => self
                .cache
                .entry(p.to_path_buf())
                .or_insert_with(|| {
                    // Cap the fingerprint read (M3): a pathological >256 MiB
                    // file must not OOM the fingerprint path. Over-cap → None →
                    // the fingerprint degrades to its non-content identity
                    // rather than reading unbounded bytes.
                    let full = self.root.join(p);
                    let size = std::fs::metadata(&full).map_or(0, |m| m.len());
                    alint_core::read_capped_or_skip(&full, size)
                })
                .as_deref(),
            None => None,
        };
        alint_core::baseline::fingerprint(rule_id, v, bytes)
    }
}

/// The canonical per-violation `violation_fingerprint` for every violation in
/// `report`, indexed `[result][violation]` to align with
/// `report.results[i].violations[j]`. Reads each offending file once (cached,
/// size-capped) for the content discriminator. Shared by the GitLab and the
/// no-baseline SARIF output paths so a finding carries ONE identity across
/// GitLab, SARIF, and baseline mode.
fn report_fingerprints(report: &alint_core::Report, root: &Path) -> Vec<Vec<String>> {
    let mut reader = FileReader::new(root);
    report
        .results
        .iter()
        .map(|r| {
            r.violations
                .iter()
                .map(|v| reader.fingerprint(&r.rule_id, v))
                .collect()
        })
        .collect()
}

/// Build the per-result baseline marks the SARIF/JSON formatters consume:
/// each live violation's fingerprint (parallel to the result's `violations`)
/// and the suppressed findings grouped onto their producing rule.
fn build_baseline_marks(
    applied: &alint_core::baseline::AppliedBaseline,
) -> alint_output::BaselineMarks {
    use std::collections::HashMap;

    let mut by_rule: HashMap<&str, Vec<alint_output::SuppressedFinding>> = HashMap::new();
    for s in &applied.suppressed {
        by_rule
            .entry(s.rule_id.as_ref())
            .or_default()
            .push(alint_output::SuppressedFinding {
                violation: s.violation.clone(),
                fingerprint: s.fingerprint.clone(),
            });
    }
    let per_result = applied
        .live
        .results
        .iter()
        .enumerate()
        .map(|(i, rr)| alint_output::ResultMarks {
            live_fingerprints: applied
                .live_fingerprints
                .get(i)
                .cloned()
                .unwrap_or_default(),
            suppressed: by_rule.remove(rr.rule_id.as_ref()).unwrap_or_default(),
        })
        .collect();
    alint_output::BaselineMarks {
        per_result,
        suppressed_total: applied.suppressed_total,
    }
}

/// Load + parse a baseline file. A missing file or a parse / schema
/// error is a hard error (exit 2), never a silent "suppress nothing".
fn load_baseline(path: &Path) -> Result<alint_core::baseline::Baseline> {
    let text = std::fs::read_to_string(path).with_context(|| {
        format!(
            "reading baseline file {} (run `alint baseline` to create it)",
            path.display()
        )
    })?;
    alint_core::baseline::Baseline::load(&text)
        .map_err(|e| anyhow::anyhow!("invalid baseline file {}: {e}", path.display()))
}

/// Stderr summary for a baseline-applied run: the suppressed count (or
/// the full list with `--show-baselined`) and any stale-entry warning.
/// `report_stale` is false on a scoped (`--changed`/`--only`) run, where
/// out-of-scope entries can't be judged stale (see `cmd_check`).
fn report_baseline_summary(
    applied: &alint_core::baseline::AppliedBaseline,
    cli: &Cli,
    report_stale: bool,
) {
    if !cli.quiet && applied.suppressed_total > 0 {
        eprintln!(
            "alint: {} baselined violation(s) suppressed",
            applied.suppressed_total
        );
    }
    if cli.show_baselined {
        for s in &applied.suppressed {
            match &s.violation.path {
                Some(p) => eprintln!(
                    "  baselined: {}: [{}] {}",
                    p.display(),
                    s.rule_id,
                    s.violation.message
                ),
                None => eprintln!("  baselined: [{}] {}", s.rule_id, s.violation.message),
            }
        }
    }
    if report_stale && !cli.quiet && !applied.stale.is_empty() {
        let n = applied.stale.len();
        eprintln!(
            "alint: {n} baseline entr{} no longer fire{}; run `alint baseline` to re-tighten{}",
            if n == 1 { "y" } else { "ies" },
            if n == 1 { "s" } else { "" },
            if cli.strict_baseline {
                " (failing build: --strict-baseline)"
            } else {
                ""
            }
        );
    }
}

/// `alint baseline`: snapshot the current violations into a baseline
/// file. Whole-tree only (the subcommand doesn't accept `--changed`).
fn cmd_baseline(
    path: &Path,
    output: Option<&Path>,
    accept_new: bool,
    cli: &Cli,
) -> Result<ExitCode> {
    use alint_core::baseline::{Baseline, FingerprintedViolation};

    require_directory(path)?;
    // `baseline` writes the baseline file plus a human summary; it has no
    // machine-readable report output, so a non-default `--format` would be
    // silently ignored. Reject it rather than no-op (the same fail-loudly
    // contract `--only` follows on subcommands that don't honor it).
    if cli.format != "human" {
        anyhow::bail!(
            "`baseline` does not support `--format {}` — it writes the baseline file and a \
             human summary; drop `--format` (use `--quiet` to silence the summary)",
            cli.format
        );
    }
    let loaded = load_rules(path, cli)?;

    // Output path precedence: --output (as given) > `baseline:` config key
    // (resolved against the repo root) > the default `.alint-baseline.json`.
    // So `alint baseline` and `alint check` agree on the same file by default.
    // Resolved up front so it can be excluded from the walk below — otherwise
    // a regeneration re-lints the existing artifact and reports it as new debt.
    let out_path = output.map_or_else(
        || {
            loaded
                .baseline
                .as_ref()
                .map_or_else(|| path.join(".alint-baseline.json"), |b| path.join(b))
        },
        Path::to_path_buf,
    );

    let engine = Engine::from_entries(loaded.entries, loaded.registry)
        .with_facts(loaded.facts)
        .with_vars(loaded.vars);
    let mut extra_ignores = loaded.extra_ignores;
    exclude_baseline_from_walk(&mut extra_ignores, path, Some(&out_path));
    let walk_opts = WalkOptions {
        respect_gitignore: if cli.no_gitignore {
            false
        } else {
            loaded.respect_gitignore
        },
        extra_ignores,
    };
    let index = walk(path, &walk_opts).context("walking repository")?;
    let report = engine.run(path, &index).context("running rules")?;

    let mut reader = FileReader::new(path);
    let mut items: Vec<FingerprintedViolation> = Vec::new();
    for result in &report.results {
        for v in &result.violations {
            items.push(FingerprintedViolation {
                rule_id: result.rule_id.to_string(),
                path: v
                    .path
                    .as_ref()
                    .map(|p| p.to_string_lossy().replace('\\', "/")),
                fingerprint: reader.fingerprint(&result.rule_id, v),
                message: Some(v.message.to_string()),
            });
        }
    }
    let new_baseline =
        Baseline::from_fingerprints(Some(env!("CARGO_PKG_VERSION").to_string()), items);

    // Regeneration guard: refuse to grandfather NEW debt without
    // --accept-new. Pruning stale entries is always allowed.
    if out_path.exists() && !accept_new {
        use std::collections::HashMap;
        let existing = load_baseline(&out_path)?;
        // Compare by OCCURRENCE, not just fingerprint identity: a higher count
        // on an already-baselined finding is fresh debt too, and must not slip
        // in silently. Both baselines are dup-free (load + from_fingerprints
        // dedup), so each fingerprint maps to exactly one count.
        let old_counts: HashMap<&str, u32> = existing
            .entries
            .iter()
            .map(|e| (e.fingerprint.as_str(), e.count))
            .collect();
        let new_counts: HashMap<&str, u32> = new_baseline
            .entries
            .iter()
            .map(|e| (e.fingerprint.as_str(), e.count))
            .collect();
        let added: u64 = new_counts
            .iter()
            .map(|(fp, &c)| u64::from(c.saturating_sub(old_counts.get(fp).copied().unwrap_or(0))))
            .sum();
        let removed: u64 = old_counts
            .iter()
            .map(|(fp, &c)| u64::from(c.saturating_sub(new_counts.get(fp).copied().unwrap_or(0))))
            .sum();
        if added > 0 {
            bail!(
                "regenerating {} would grandfather {added} new violation(s) (+{added} / -{removed}); \
                 fix them, or pass --accept-new to accept them into the baseline",
                out_path.display()
            );
        }
    }

    std::fs::write(&out_path, new_baseline.to_jsonl())
        .with_context(|| format!("writing baseline {}", out_path.display()))?;
    if !cli.quiet {
        let n = new_baseline.entries.len();
        let total = new_baseline.total();
        eprintln!(
            "alint: wrote {} ({n} entr{}, {total} occurrence{})",
            out_path.display(),
            if n == 1 { "y" } else { "ies" },
            if total == 1 { "" } else { "s" },
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Surface informational notes (non-violation findings) on stderr so
/// stdout stays clean. A one-line count by default; the full
/// `path: message` list when `show_notes` is set. No output when there
/// are no notes.
fn report_notes_to_stderr(report: &alint_core::Report, show_notes: bool) {
    let total: usize = report.results.iter().map(|r| r.notes.len()).sum();
    if total == 0 {
        return;
    }
    if show_notes {
        eprintln!("alint: {total} informational note(s):");
        for result in &report.results {
            for note in &result.notes {
                match &note.path {
                    Some(p) => eprintln!("  note: {}: {}", p.display(), note.message),
                    None => eprintln!("  note: {}", note.message),
                }
            }
        }
    } else {
        eprintln!("alint: {total} informational note(s); run with --show-notes to list.");
    }
}

fn cmd_fix(
    path: &Path,
    dry_run: bool,
    changed: &ChangedMode,
    only: &[String],
    cli: &Cli,
) -> Result<ExitCode> {
    require_directory(path)?;
    // Gate the output format *before* touching the tree: `fix` mutates files,
    // so a format we can't render must fail here, not after the write. Only
    // `human`, `json`, and `markdown` have dedicated fix-report renderers. The
    // finding-oriented formats (SARIF / GitHub / JUnit / GitLab) and the
    // check-side `agent` format would otherwise degrade *silently* to human
    // output with exit 0 — a machine consumer asking for `sarif` and getting
    // human text is the same trap M13 closed for `validate-config`. Fail loudly
    // instead, and point `agent` at its real home: `check --format agent`
    // (whose per-violation `fix_command` drives the agentic fix loop; `fix`
    // itself has no agent report).
    let format: Format = cli.format.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    if !matches!(format, Format::Human | Format::Json | Format::Markdown) {
        bail!(
            "`alint fix` supports only `--format human`, `--format json`, or \
             `--format markdown` (got {fmt:?}); the SARIF/GitHub/JUnit/GitLab/agent \
             formats describe findings, not fixes — run `alint check --format {fmt}` \
             for those",
            fmt = cli.format,
        );
    }
    let loaded = load_rules(path, cli)?;
    let entries = apply_only_filter(loaded.entries, only)?;
    let mut engine = Engine::from_entries(entries, loaded.registry)
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
    // Keep the baseline artifact out of the walk so a broad content-fixer can't
    // rewrite alint's own committed JSON-Lines file (mirrors `check`/`baseline`).
    let effective_baseline = cli
        .baseline
        .clone()
        .or_else(|| loaded.baseline.as_ref().map(|b| path.join(b)));
    let mut extra_ignores = loaded.extra_ignores;
    exclude_baseline_from_walk(&mut extra_ignores, path, effective_baseline.as_deref());
    let walk_opts = WalkOptions {
        respect_gitignore: effective_gitignore,
        extra_ignores,
    };

    let index = walk(path, &walk_opts).context("walking repository")?;
    let report = engine
        .fix(path, &index, dry_run)
        .context("applying fixes")?;

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

fn cmd_list(category: Option<&str>, cli: &Cli) -> Result<ExitCode> {
    use alint_core::{Category, Level};
    use alint_output::style;

    // Validate the category slug BEFORE building the config, so a typo fails fast
    // with a clear error instead of surfacing an unrelated config-build failure.
    if let Some(slug) = category
        && Category::from_slug(slug).is_none()
    {
        let known: Vec<&str> = Category::ALL.iter().map(|c| c.slug()).collect();
        bail!(
            "unknown category {slug:?}. Known categories: {}",
            known.join(", ")
        );
    }

    let mut loaded = load_rules(Path::new("."), cli)?;
    let loaded_any = !loaded.entries.is_empty();

    // Config-scoped category filter: keep only the loaded rules whose kind is in
    // the given category, resolved through the same in-crate bridge `alint rules`
    // uses. `alint list --category X` answers "which of MY rules are X"; the
    // catalog view (all kinds alint ships) is `alint rules list --category X`.
    if let Some(slug) = category {
        loaded
            .entries
            .retain(|e| rules::categories_for_kind(e.kind()).contains(&slug));
    }

    // `list` honours --format for machine consumers: `json` emits a
    // stable rule-inventory envelope (the effective rule set, post
    // extends/overrides). Any other machine format is an explicit
    // error rather than a silent fall-through to human output.
    let format: Format = cli.format.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    match format {
        Format::Human => {}
        Format::Json => return list_json(&loaded),
        _ => bail!(
            "`alint list` supports only `--format human` or `--format json` (got {:?})",
            cli.format
        ),
    }

    let (mut out, opts) = render_env(cli)?;
    if loaded.entries.is_empty() {
        // Distinguish "config defines no rules" from "the category matched none".
        match category {
            Some(slug) if loaded_any => {
                writeln!(out, "(no loaded rules are in category '{slug}')")?;
            }
            _ => writeln!(out, "(no rules loaded from config)")?,
        }
        out.flush().ok();
        return Ok(ExitCode::SUCCESS);
    }
    let dim = style::DIM;
    let docs = style::DOCS;
    for entry in &loaded.entries {
        let rule = &entry.rule;
        let level_style = match rule.level() {
            Level::Error => style::ERROR,
            Level::Warning => style::WARNING,
            Level::Info => style::INFO,
            Level::Off => style::DIM,
        };
        let label = rule.level().as_str();
        // Pad to 8 cols *after* the SGR reset so the alignment
        // is on visible glyph count, not byte count.
        let pad = " ".repeat(8usize.saturating_sub(label.len()));
        write!(
            out,
            "{level_style}{label}{level_style:#}{pad} {}",
            rule.id()
        )?;
        // Surface the rule's kind (and a fixable marker) in the human list,
        // matching what `list --format json` already carries — otherwise the
        // human inventory can't answer "what kind is this rule?".
        if !entry.kind().is_empty() {
            write!(out, "  {dim}{}{dim:#}", entry.kind())?;
        }
        if entry.when.is_some() {
            write!(out, " {dim}[when]{dim:#}")?;
        }
        if rule.fixer().is_some() {
            write!(out, " {dim}[fix]{dim:#}")?;
        }
        if opts.show_docs
            && let Some(url) = rule.policy_url()
        {
            write!(out, "  {dim}({dim:#}{docs}{url}{docs:#}{dim}){dim:#}")?;
        }
        writeln!(out)?;
    }
    out.flush().ok();
    Ok(ExitCode::SUCCESS)
}

/// Stable machine-readable rule inventory for `alint list --format json`.
/// Carries the effective rule set (after `extends:`/overrides) so fleet
/// tooling can diff "what rules are effective here" across repos. The
/// Shares the integer `schema_version: 1` with the other JSON envelopes but is
/// a DISTINCT shape — the `kind` field discriminates so a consumer can tell a
/// rule inventory from a rule definition or a check report.
fn list_json(loaded: &LoadedConfig) -> Result<ExitCode> {
    let rules: Vec<serde_json::Value> = loaded
        .entries
        .iter()
        .map(|entry| {
            serde_json::json!({
                "id": entry.rule.id(),
                "kind": entry.kind(),
                "categories": rules::categories_for_kind(entry.kind()),
                "level": entry.rule.level().as_str(),
                "policy_url": entry.rule.policy_url(),
                "conditional": entry.when.is_some(),
                "fixable": entry.rule.fixer().is_some(),
            })
        })
        .collect();
    let doc = serde_json::json!({ "schema_version": 1, "kind": "rule-inventory", "rules": rules });
    let mut out = std::io::stdout().lock();
    writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
    out.flush().ok();
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
    // `facts` has only a human and a json shape; reject other formats rather
    // than silently degrading them to human (matching `list`/`explain`).
    match format {
        Format::Human | Format::Json => {}
        _ => bail!(
            "`alint facts` supports only `--format human` or `--format json` (got {:?})",
            cli.format
        ),
    }
    let (mut out, _opts) = render_env(cli)?;
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
    use alint_output::style;

    if facts.is_empty() {
        writeln!(out, "(no facts declared in config)")?;
        return Ok(());
    }
    let id_width = facts.iter().map(|f| f.id.len()).max().unwrap_or(0);
    let kind_width = facts.iter().map(|f| f.kind.name().len()).max().unwrap_or(0);
    let dim = style::DIM;
    for spec in facts {
        let value_str = values
            .get(&spec.id)
            .map_or_else(|| "(unresolved)".to_string(), fact_value_display);
        // The kind column is the schema/type label (always
        // dimmed); value gets a tonal cue too — `true` reads
        // as success, `false` / `(unresolved)` as muted.
        let value_style = match value_str.as_str() {
            "true" => style::SUCCESS,
            "false" | "(unresolved)" => style::DIM,
            _ => style::PATH, // typed values like strings/numbers — bold but uncolored
        };
        let kind_name = spec.kind.name();
        let kind_pad = " ".repeat(kind_width.saturating_sub(kind_name.len()));
        writeln!(
            out,
            "{:<id_width$}  {dim}{kind_name}{dim:#}{kind_pad}  {value_style}{value_str}{value_style:#}",
            spec.id,
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
    let doc = serde_json::json!({ "schema_version": 1, "kind": "facts", "facts": entries });
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
    use alint_core::Level;
    use alint_output::style;

    let loaded = load_rules(Path::new("."), cli)?;
    let Some(entry) = loaded.entries.iter().find(|e| e.rule.id() == rule_id) else {
        bail!("no rule with id {rule_id:?} found in the effective config");
    };
    let rule = &entry.rule;

    // `explain` honours --format for machine consumers, matching `list`:
    // `json` emits the rule's wire-shape; any other machine format is an
    // explicit error rather than silently printing the human block.
    let format: Format = cli.format.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    match format {
        Format::Human => {}
        Format::Json => return explain_json(entry),
        _ => bail!(
            "`alint explain` supports only `--format human` or `--format json` (got {:?})",
            cli.format
        ),
    }

    let (mut out, opts) = render_env(cli)?;
    let dim = style::DIM;
    let docs = style::DOCS;
    let level_style = match rule.level() {
        Level::Error => style::ERROR,
        Level::Warning => style::WARNING,
        Level::Info => style::INFO,
        Level::Off => style::DIM,
    };
    writeln!(out, "{dim}id:        {dim:#} {}", rule.id())?;
    if !entry.kind().is_empty() {
        writeln!(out, "{dim}kind:      {dim:#} {}", entry.kind())?;
        let cats = rules::categories_for_kind(entry.kind());
        if !cats.is_empty() {
            writeln!(out, "{dim}categories:{dim:#} {}", cats.join(", "))?;
        }
    }
    writeln!(
        out,
        "{dim}level:     {dim:#} {level_style}{}{level_style:#}",
        rule.level().as_str(),
    )?;
    if let Some(paths) = entry.paths() {
        writeln!(out, "{dim}paths:     {dim:#} {}", paths.render_scope())?;
    }
    // Kind-specific options (pattern, max_lines, ...): the first inline after
    // the `options:` label, the rest indented under the value column. A
    // spec-less entry has no options, so the flattened iterator is empty.
    for (i, (k, v)) in entry.extra().into_iter().flatten().enumerate() {
        let key = k.as_str().unwrap_or_default();
        let val = match serde_json::to_value(v) {
            // A single-line string renders bare; a multi-line string (and every
            // non-string) renders as compact JSON, so an embedded newline cannot
            // break the aligned options block.
            Ok(serde_json::Value::String(s)) if !s.contains(['\n', '\r']) => s,
            Ok(jv) => jv.to_string(),
            Err(_) => String::new(),
        };
        if i == 0 {
            writeln!(out, "{dim}options:   {dim:#} {dim}{key}:{dim:#} {val}")?;
        } else {
            writeln!(out, "            {dim}{key}:{dim:#} {val}")?;
        }
    }
    // A blank / whitespace-only message renders as absent rather than a
    // dangling `message:` label. Continuation lines indent under the value
    // column and a trailing newline (block scalars carry one) is dropped, so a
    // multi-line message stays aligned instead of wrapping back to column 0.
    if let Some(msg) = entry.message() {
        let msg = msg.trim_end_matches('\n');
        if !msg.trim().is_empty() {
            let msg = msg.replace('\n', &format!("\n{}", " ".repeat(12)));
            writeln!(out, "{dim}message:   {dim:#} {msg}")?;
        }
    }
    // v0.9.20: honour --no-docs by suppressing the policy_url line.
    // URLs remain in machine-readable formats regardless.
    if opts.show_docs
        && let Some(url) = rule.policy_url()
    {
        writeln!(out, "{dim}policy_url:{dim:#} {docs}{url}{docs:#}")?;
    }
    if let Some(when) = entry.when_src() {
        writeln!(out, "{dim}when:      {dim:#} {when}")?;
    }
    if let Some(fixer) = rule.fixer() {
        writeln!(
            out,
            "{dim}fix:       {dim:#} {}",
            alint_core::Fixer::describe(fixer)
        )?;
    }
    out.flush().ok();
    // v0.9.20: dropped the `debug: {rule:?}` line. The internal Debug
    // repr dumped per-rule-kind state (regex automaton, compiled
    // matchers, etc.) — useful for alint developers, noise for end
    // users (24+ KB for some rule kinds). Use `--format json` (here or
    // on `alint check`) if you need the wire-shape, or read the rule's
    // YAML config block.
    Ok(ExitCode::SUCCESS)
}

/// Machine-readable single-rule shape for `alint explain <id> --format
/// json`. Parallels each entry of the `list --format json` inventory.
fn explain_json(entry: &alint_core::RuleEntry) -> Result<ExitCode> {
    // `paths` normalises to `{ include, exclude }` glob lists. `rule_kind`
    // carries the rule's own kind (the envelope's `kind` is the fixed `"rule"`
    // discriminator), and `categories` matches each `list --format json` entry.
    let paths = entry.paths().map(|p| {
        let (include, exclude) = p.include_exclude();
        serde_json::json!({ "include": include, "exclude": exclude })
    });
    let options = match entry.extra() {
        Some(extra) if !extra.is_empty() => {
            serde_json::to_value(extra).unwrap_or(serde_json::Value::Null)
        }
        _ => serde_json::Value::Null,
    };
    let doc = serde_json::json!({
        "schema_version": 1,
        "kind": "rule",
        "id": entry.rule.id(),
        "rule_kind": entry.kind(),
        "categories": rules::categories_for_kind(entry.kind()),
        "level": entry.rule.level().as_str(),
        "paths": paths,
        "options": options,
        "message": entry.message().filter(|m| !m.trim().is_empty()),
        "policy_url": entry.rule.policy_url(),
        "when": entry.when_src(),
        // Both `when` and `conditional` project from the retained `when:` source
        // (`spec.when`), so within this document they can never disagree - deriving
        // both from one field keeps it self-consistent for any future caller.
        // `list --format json`'s `conditional` reads the equivalent parsed `when`.
        "conditional": entry.when_src().is_some(),
        "fixable": entry.rule.fixer().is_some(),
        "fix": entry.rule.fixer().map(alint_core::Fixer::describe),
    });
    let mut out = std::io::stdout().lock();
    writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
    out.flush().ok();
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

struct LoadedConfig {
    entries: Vec<alint_core::RuleEntry>,
    registry: RuleRegistry,
    facts: Vec<alint_core::FactSpec>,
    vars: std::collections::HashMap<String, String>,
    respect_gitignore: bool,
    extra_ignores: Vec<String>,
    fix_size_limit: Option<u64>,
    /// The `baseline:` config key (the raw repo-root-relative path), if set.
    baseline: Option<PathBuf>,
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
fn cmd_validate_config(path: Option<PathBuf>, format: &str, cli: &Cli) -> Result<ExitCode> {
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
    match validate_config_inner(&config_path) {
        Ok(rule_count) => emit_validate_success(rule_count, &config_path, format),
        Err(e) => emit_validate_failure(&e, Some(&config_path), format),
    }
}

fn validate_config_inner(config_path: &Path) -> Result<usize> {
    let config = alint_dsl::load(config_path)?;
    let registry: alint_core::RuleRegistry = alint_rules::builtin_registry();
    let mut count = 0usize;
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
            .with_context(|| format!("building rule {:?}", spec.id))?;
        if let Some(when_src) = &spec.when {
            alint_core::when::parse(when_src)
                .with_context(|| format!("rule {:?}: parsing `when`", spec.id))?;
        }
        count += 1;
    }
    Ok(count)
}

fn emit_validate_success(rule_count: usize, config_path: &Path, format: &str) -> Result<ExitCode> {
    if format == "json" {
        let envelope = serde_json::json!({
            "valid": true,
            "rule_count": rule_count,
            "config_path": config_path.display().to_string(),
            "error": serde_json::Value::Null,
        });
        println!("{}", serde_json::to_string(&envelope)?);
    } else {
        // human format
        println!(
            "✓ Config valid: {rule_count} rule(s) loaded from {}",
            config_path.display()
        );
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
        println!("{}", serde_json::to_string(&envelope)?);
    } else {
        // Human format prints to stderr to stay out of the way of
        // stdout consumers, then a one-line summary on stdout so
        // terminals show something either way.
        eprintln!("alint: {err:#}");
        println!("✗ Config invalid");
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

fn load_rules(cwd: &Path, cli: &Cli) -> Result<LoadedConfig> {
    let config_path = if let Some(first) = cli.config.first() {
        first.clone()
    } else {
        alint_dsl::discover(cwd).ok_or_else(|| {
            anyhow::anyhow!(
                "no .alint.yml found (searched from {}) \
                 (run `alint init` to scaffold one)",
                cwd.display()
            )
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
        let mut rule = registry
            .build(spec)
            .with_context(|| format!("building rule {:?}", spec.id))?;
        // Structurally validate nested `require:` sub-rules at load, so a
        // typo'd nested kind/option fails here rather than lazily mid-walk
        // (or never, when the selector matches no entries).
        rule.validate_nested(&registry)
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
        baseline: config.baseline,
    })
}

#[cfg(test)]
mod tests;
