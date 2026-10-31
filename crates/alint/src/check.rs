//! Check and baseline command execution.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use alint_core::{Engine, WalkOptions, walk};
use alint_output::{Format, sanitize_terminal as term};
use anyhow::{Context, Result, bail};

use super::config_cmd::{load_rules, render_env, report_pattern_hints};
use super::{Cli, reject_non_human_baseline_format, safe_write, tolerate_broken_pipe};

/// Resolved `--changed` / `--base` state. `--base` implies
/// `--changed`; both together identify the diff source.
#[derive(Debug)]
pub(super) struct ChangedMode {
    pub(super) enabled: bool,
    pub(super) base: Option<String>,
}

impl ChangedMode {
    pub(super) fn new(changed_flag: bool, base: Option<String>) -> Self {
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
    pub(super) fn resolve(
        &self,
        root: &Path,
    ) -> Result<Option<std::collections::HashSet<PathBuf>>> {
        if !self.enabled {
            return Ok(None);
        }
        let set = if let Some(base) = self.base.as_deref() {
            match alint_core::git::collect_changed_paths_checked(root, base) {
                Ok(Some(set)) => set,
                Ok(None) => {
                    bail!(
                        "--changed requires a git repository (and `git` on PATH); \
                         could not inspect repository root `{}`. Run without --changed \
                         for a full check.",
                        root.display()
                    );
                }
                Err(alint_core::git::CommitRangeError::BadRange { stderr }) => {
                    bail!(
                        "--base {base:?} does not resolve to a usable commit in this clone. \
                         A shallow CI checkout usually needs `fetch-depth: 0` or an explicit \
                         `git fetch` for the base ref. Git said: {stderr}"
                    );
                }
            }
        } else {
            alint_core::git::collect_changed_paths(root, None).ok_or_else(|| {
                anyhow::anyhow!(
                    "--changed requires a git repository (and `git` on PATH); collecting \
                     staged, unstaged, and untracked paths failed at `{}`. Run without \
                     --changed for a full check.",
                    root.display()
                )
            })?
        };
        Ok(Some(set))
    }
}

/// Filter loaded rule entries to the ids named in `--only` (a no-op
/// when `only` is empty). Every `--only` id must match a loaded rule;
/// an unmatched id is an error so a typo fails loudly instead of
/// silently selecting nothing. Shared by `check` and `fix`.
pub(super) fn apply_only_filter(
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
pub(super) fn require_directory(path: &Path) -> Result<()> {
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
pub(super) fn exclude_baseline_from_walk(
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

pub(super) fn cmd_check(
    path: &Path,
    changed: &ChangedMode,
    only: &[String],
    cli: &Cli,
) -> Result<ExitCode> {
    require_directory(path)?;
    let loaded = load_rules(path, cli)?;
    let root = loaded.root.clone();
    // Path-less findings are anchored on the config file in SARIF.
    let config_rel = loaded.config_rel();
    // The `baseline:` config key, resolved against the repo root being checked.
    // The `--baseline` flag (used as given) overrides it; either one turns on
    // baseline suppression. No silent auto-detect of `.alint-baseline.json`.
    let config_baseline = loaded.baseline.as_ref().map(|b| root.join(b));
    // Resolved early (was below the walk) so the baseline file can be excluded
    // from the walk. The `--baseline` flag (used as given) overrides the key.
    let effective_baseline = cli.baseline.clone().or(config_baseline);
    let entries = apply_only_filter(loaded.entries, only)?;
    report_pattern_hints(&entries, &loaded.registry, &loaded.facts, cli);
    let rule_count = entries.len();
    let mut engine = Engine::from_entries(entries, loaded.registry)
        .with_facts(loaded.facts)
        .with_vars(loaded.vars)
        // `check --format sarif/agent/json --include-fixes` runs
        // `attach_proposed_edits`, which gates advertised fixes on the SAME
        // `fix_size_limit` as `fix`; without this it silently used the 1 MiB
        // default and advertised (or omitted) fixes `fix` would not (audit HIGH,
        // 2026-09-20). Inert for a plain `check` (no fix derivation runs).
        .with_fix_size_limit(loaded.fix_size_limit);
    if let Some(analysis_time) = cli.as_of {
        engine = engine.with_analysis_time(analysis_time);
    }
    // When a baseline is active, make the engine baseline-aware so the
    // `stage_fixes`-based proposed-edit derivation (for a fix-carrying format)
    // advertises fixes ONLY for the live (new) findings, never grandfathered ones.
    // Inert for `engine.run` (check uses no fix baseline), so a plain `check
    // --baseline` is unaffected.
    if let Some(bp) = &effective_baseline {
        engine = engine.with_fix_baseline(load_baseline(bp)?);
    }
    let changed_active = match changed.resolve(&root)? {
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
    exclude_baseline_from_walk(&mut extra_ignores, &root, effective_baseline.as_deref());
    let walk_opts = WalkOptions {
        respect_gitignore: effective_gitignore,
        extra_ignores,
    };

    let index = walk(&root, &walk_opts).context("walking repository")?;
    tracing::debug!(files = index.entries.len(), "walk complete");

    let report = engine.run(&root, &index).context("running rules")?;

    let format: Format = cli.format.parse().map_err(|e: String| anyhow::anyhow!(e))?;

    // Baseline suppression (when --baseline is in effect): grandfather
    // recorded violations, leaving only new ones to format + gate on.
    let mut strict_stale_fail = false;
    let (mut report, baseline_marks) = if let Some(baseline_path) = &effective_baseline {
        let baseline = load_baseline(baseline_path)?;
        let mut reader = FileReader::new(&root);
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

    // The machine formats that carry the concrete proposed fix: SARIF
    // (`result.fixes[]`), `agent` (always), and `json` (behind `--include-fixes`).
    // Only Safe fixes are attached (derived from the engine's single-pass compose
    // at the Safe threshold). The compute runs the fix pass in stage mode, so it is
    // scoped to a run that actually asks for fixes.
    if matches!(format, Format::Sarif | Format::Agent)
        || (format == Format::Json && cli.include_fixes)
    {
        alint_core::attach_proposed_edits(&engine, &mut report, &root, &index);
    }

    let (mut out, opts) = render_env(cli)?;
    // SARIF and JSON render baselined findings (marked / counted) so Code
    // Scanning dismisses rather than re-opens them; every other format ignores
    // the baseline and emits only the live (new) findings.
    let written = match (format, baseline_marks.as_ref()) {
        (Format::Sarif, Some(marks)) => {
            alint_output::write_sarif_for_config(&report, Some(marks), None, &config_rel, &mut out)
        }
        (Format::Sarif, None) => {
            // No baseline, but still emit the canonical `partialFingerprints` so
            // GitHub Code Scanning can correlate alerts across runs (these were
            // baseline-only before). Same fingerprints the GitLab path uses.
            let fps = report_fingerprints(&report, &root);
            alint_output::write_sarif_for_config(&report, None, Some(&fps), &config_rel, &mut out)
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
            let fps = report_fingerprints(&report, &root);
            alint_output::write_gitlab(&report, Some(&fps), &mut out)
        }
        _ => format.write_with_options(&report, &mut out, opts),
    };
    tolerate_broken_pipe(written).context("writing output")?;
    out.flush().ok();

    // Human output keeps non-violation notes on stderr so stdout stays
    // byte-clean. Machine formats carry their own lossless or summary
    // projection.
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
pub(super) fn load_baseline(path: &Path) -> Result<alint_core::baseline::Baseline> {
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
            match s.violation.report_path() {
                Some(p) => eprintln!(
                    "  baselined: {}: [{}] {}",
                    term(&p.display().to_string()),
                    term(&s.rule_id),
                    term(&s.violation.message)
                ),
                None => eprintln!(
                    "  baselined: [{}] {}",
                    term(&s.rule_id),
                    term(&s.violation.message)
                ),
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
// Baseline generation is one ordered read/filter/fingerprint/write transaction.
#[allow(clippy::too_many_lines)]
pub(super) fn cmd_baseline(
    path: &Path,
    output: Option<&Path>,
    accept_new: bool,
    cli: &Cli,
) -> Result<ExitCode> {
    use alint_core::baseline::{Baseline, FingerprintedViolation};

    require_directory(path)?;
    reject_non_human_baseline_format(cli)?;
    let loaded = load_rules(path, cli)?;
    let root = loaded.root.clone();

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
                .map_or_else(|| root.join(".alint-baseline.json"), |b| root.join(b))
        },
        Path::to_path_buf,
    );
    // A baseline path that comes from the repo (the `baseline:` key or the
    // default) is untrusted: it must stay inside the repository and not be a
    // symlink, so a crafted config / committed link can't make `alint
    // baseline` overwrite a file elsewhere. An explicit `--output` is the
    // user's own choice and may point anywhere (still never through a link).
    // A repo-derived baseline path (the `baseline:` key or the default) is
    // confined to the repository; an explicit `--output` is not. Neither may
    // be a symlink. Checked up front so a bad target fails before the run.
    safe_write::check_output(&root, &out_path, output.is_some(), "baseline")?;

    report_pattern_hints(&loaded.entries, &loaded.registry, &loaded.facts, cli);
    let mut engine = Engine::from_entries(loaded.entries, loaded.registry)
        .with_facts(loaded.facts)
        .with_vars(loaded.vars);
    if let Some(analysis_time) = cli.as_of {
        engine = engine.with_analysis_time(analysis_time);
    }
    let mut extra_ignores = loaded.extra_ignores;
    exclude_baseline_from_walk(&mut extra_ignores, &root, Some(&out_path));
    let walk_opts = WalkOptions {
        respect_gitignore: if cli.no_gitignore {
            false
        } else {
            loaded.respect_gitignore
        },
        extra_ignores,
    };
    let index = walk(&root, &walk_opts).context("walking repository")?;
    let report = engine.run(&root, &index).context("running rules")?;
    if report.total_analysis_failures() > 0 {
        let count = report.total_analysis_failures();
        eprintln!(
            "alint: refusing to write baseline: {count} analysis failure{}",
            if count == 1 { "" } else { "s" }
        );
        for result in &report.results {
            for failure in &result.analysis_failures {
                let location = failure.report_path().map_or_else(
                    || "<repo>".to_string(),
                    |path| match failure.line {
                        Some(line) => format!("{}:{line}", path.display()),
                        None => path.display().to_string(),
                    },
                );
                let message = alint_output::sanitize_terminal_multiline(&failure.message)
                    .replace('\n', "\n    ");
                eprintln!(
                    "  {}: {}: {}: {message}",
                    alint_output::sanitize_terminal(&location),
                    alint_output::sanitize_terminal(&result.rule_id),
                    failure.kind.as_str(),
                );
            }
        }
        return Ok(ExitCode::from(1));
    }

    let mut reader = FileReader::new(&root);
    let mut items: Vec<FingerprintedViolation> = Vec::new();
    for result in &report.results {
        for v in &result.violations {
            items.push(FingerprintedViolation {
                rule_id: result.rule_id.to_string(),
                path: v
                    .report_path()
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

    safe_write::write_output(
        &root,
        &out_path,
        new_baseline.to_jsonl().as_bytes(),
        output.is_some(),
        "baseline",
    )?;
    if !cli.quiet {
        let n = new_baseline.entries.len();
        let total = new_baseline.total();
        eprintln!(
            "alint: wrote {} ({n} entr{}, {total} occurrence{})",
            term(&out_path.display().to_string()),
            if n == 1 { "y" } else { "ies" },
            if total == 1 { "" } else { "s" },
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Surface non-violation notes on stderr so human stdout stays clean.
fn report_notes_to_stderr(report: &alint_core::Report, show_notes: bool) {
    let stderr = io::stderr();
    let mut stderr = stderr.lock();
    let _ = write_report_notes(report, show_notes, &mut stderr);
}

pub(super) fn write_report_notes(
    report: &alint_core::Report,
    show_notes: bool,
    w: &mut dyn Write,
) -> io::Result<()> {
    let total = report.total_notes();
    if total == 0 {
        return Ok(());
    }
    if !show_notes {
        return writeln!(
            w,
            "alint: {total} informational note(s); run with --show-notes to list."
        );
    }
    writeln!(w, "alint: {total} informational note(s):")?;
    for result in &report.results {
        for note in &result.notes {
            let label = match note
                .note_kind()
                .unwrap_or(alint_core::NoteKind::Informational)
            {
                alint_core::NoteKind::Informational => "note",
                alint_core::NoteKind::AnalysisTruncated => "analysis truncated",
            };
            match note.report_path() {
                Some(p) => writeln!(
                    w,
                    "  {label}: {}: {}",
                    term(&p.display().to_string()),
                    term(note.message_after_path(p))
                )?,
                None => writeln!(w, "  {label}: {}", term(&note.message))?,
            }
        }
    }
    Ok(())
}
