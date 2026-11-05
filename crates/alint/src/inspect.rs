//! Rule listing, facts, and explain command execution.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use alint_core::{Engine, WalkOptions, walk};
use alint_output::{
    Format, sanitize_terminal as term, sanitize_terminal_multiline as term_multiline,
};
use anyhow::{Context, Result, bail};

use super::check::{ChangedMode, exclude_baseline_from_walk};
use super::config_cmd::{LoadedConfig, load_rules, load_rules_for_explain, render_env};
use super::{Cli, rules};

#[derive(Debug)]
pub(super) struct ExplainOptions {
    pub(super) trace: bool,
    pub(super) preview_limit: Option<usize>,
    pub(super) changed: ChangedMode,
}

pub(super) fn cmd_list(category: Option<&str>, cli: &Cli) -> Result<ExitCode> {
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
            term(rule.id())
        )?;
        // Surface the rule's kind (and a fixable marker) in the human list,
        // matching what `list --format json` already carries — otherwise the
        // human inventory can't answer "what kind is this rule?".
        if !entry.kind().is_empty() {
            write!(out, "  {dim}{}{dim:#}", term(entry.kind()))?;
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
            let url = if explain_field_is_sensitive(entry, "policy_url") {
                "<redacted>"
            } else {
                url
            };
            let url = term(url);
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
                "policy_url": entry.rule.policy_url().map(|url| {
                    if explain_field_is_sensitive(entry, "policy_url") {
                        "<redacted>"
                    } else {
                        url
                    }
                }),
                "conditional": entry.when.is_some(),
                "expect_matches": entry.expect_matches(),
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

pub(super) fn cmd_facts(path: &Path, cli: &Cli) -> Result<ExitCode> {
    let loaded = load_rules(path, cli)?;
    let root = loaded.root.clone();
    let effective_gitignore = if cli.no_gitignore {
        false
    } else {
        loaded.respect_gitignore
    };
    let walk_opts = WalkOptions {
        respect_gitignore: effective_gitignore,
        extra_ignores: loaded.extra_ignores,
    };
    let index = walk(&root, &walk_opts).context("walking repository")?;
    let values =
        alint_core::evaluate_facts(&loaded.facts, &root, &index).context("evaluating facts")?;

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
pub(super) fn render_facts(
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

pub(super) fn render_facts_human(
    facts: &[alint_core::FactSpec],
    values: &alint_core::FactValues,
    out: &mut dyn Write,
) -> Result<()> {
    use alint_output::style;

    if facts.is_empty() {
        writeln!(out, "(no facts declared in config)")?;
        return Ok(());
    }
    // A fact id can come from the environment.
    let ids: Vec<_> = facts
        .iter()
        .map(|f| super::redact_environment_values(&f.id))
        .collect();
    let id_width = ids.iter().map(|id| id.len()).max().unwrap_or(0);
    let kind_width = facts.iter().map(|f| f.kind.name().len()).max().unwrap_or(0);
    let dim = style::DIM;
    for (spec, id) in facts.iter().zip(&ids) {
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
            "{:<id_width$}  {dim}{kind_name}{dim:#}{kind_pad}  {value_style}{}{value_style:#}",
            term(id),
            // A fact value can come from repo content or a `custom:` command.
            term(&value_str),
        )?;
    }
    Ok(())
}

pub(super) fn render_facts_json(
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
                // A fact id can come from the environment.
                "id": super::redact_environment_values(&spec.id),
                "kind": spec.kind.name(),
                "value": value,
            })
        })
        .collect();
    let doc = serde_json::json!({ "schema_version": 1, "kind": "facts", "facts": entries });
    writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
    Ok(())
}

pub(super) fn fact_value_display(v: &alint_core::FactValue) -> String {
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

pub(super) fn fact_value_json(v: &alint_core::FactValue) -> serde_json::Value {
    match v {
        alint_core::FactValue::Bool(b) => serde_json::Value::Bool(*b),
        alint_core::FactValue::Int(n) => serde_json::Value::Number((*n).into()),
        alint_core::FactValue::String(s) => serde_json::Value::String(s.clone()),
    }
}

fn explain_field_is_sensitive(entry: &alint_core::RuleEntry, field: &str) -> bool {
    entry
        .spec
        .as_deref()
        .is_some_and(|spec| spec.field_is_sensitive(field))
}

fn explain_repository_path(root: &Path, path: &Path) -> String {
    let relative = if path.is_absolute() {
        match path.strip_prefix(root) {
            Ok(relative) => relative,
            Err(_) => return "<outside-repository>".to_string(),
        }
    } else {
        path
    };
    if relative
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return "<outside-repository>".to_string();
    }
    relative.display().to_string().replace('\\', "/")
}

/// Render the `scope_filter:` block for `alint explain` (human format): the
/// `has_ancestor` / `changed_since` gates, and for the manifest predicates the
/// paths they resolve to (ADR-0010's legibility mitigation). No-op if unset.
fn write_scope_filter_explain(
    out: &mut impl std::io::Write,
    entry: &alint_core::RuleEntry,
    root: &Path,
) -> Result<()> {
    use alint_output::style;
    let dim = style::DIM;
    // scope_filter narrows `paths:` further; you can't read the glob and know the
    // scope, so surface each gate — and for the manifest predicates, the paths
    // they actually resolve to (ADR-0010's legibility mitigation).
    if let Some(sf) = entry.scope_filter() {
        if explain_field_is_sensitive(entry, "scope_filter") {
            writeln!(out, "{dim}scope_filter:{dim:#} <redacted>")?;
            return Ok(());
        }
        writeln!(out, "{dim}scope_filter:{dim:#}")?;
        if let Some(ha) = &sf.has_ancestor {
            writeln!(out, "  {dim}has_ancestor: {dim:#} {}", term(&ha.join(", ")))?;
        }
        if let Some(cs) = &sf.changed_since {
            writeln!(out, "  {dim}changed_since:{dim:#} {}", term(cs))?;
        }
        if (sf.include_manifest_paths.is_some() || sf.exclude_manifest_paths.is_some())
            && let Ok(filter) = alint_core::ScopeFilter::from_spec(entry.rule.id(), sf.clone())
        {
            for res in filter.resolved_manifest_sets(root) {
                let key = if res.include {
                    "include_manifest_paths"
                } else {
                    "exclude_manifest_paths"
                };
                // Forward-slash every path so `explain` reads identically on
                // Windows and Unix (matching violation paths / SARIF), not the
                // OS-native separator a bare `PathBuf` Display would emit.
                let paths = if res.paths.is_empty() {
                    "(no paths resolved)".to_string()
                } else {
                    res.paths
                        .iter()
                        .map(|p| explain_repository_path(root, p))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                // Both the manifest's own path and the paths it lists (read
                // from repo content, e.g. package.json) are untrusted.
                writeln!(
                    out,
                    "  {dim}{key}:{dim:#} {} -> {}",
                    term(&explain_repository_path(root, &res.source)),
                    term(&paths)
                )?;
            }
        }
    }
    Ok(())
}

fn write_kind_explain(
    out: &mut impl std::io::Write,
    entry: &alint_core::RuleEntry,
    kind_meta: &alint_core::KindMeta,
    show_docs: bool,
) -> Result<()> {
    use alint_output::style;

    let dim = style::DIM;
    let docs = style::DOCS;
    if !entry.kind().is_empty() {
        writeln!(out, "{dim}kind:      {dim:#} {}", term(entry.kind()))?;
        let cats = rules::categories_for_kind(entry.kind());
        if !cats.is_empty() {
            writeln!(out, "{dim}categories:{dim:#} {}", cats.join(", "))?;
        }
        if let Some(summary) = rules::description_for_kind(entry.kind()) {
            writeln!(out, "{dim}summary:   {dim:#} {summary}")?;
        }
        writeln!(
            out,
            "{dim}capabilities:{dim:#} {}",
            rules::capability_summary(kind_meta)
        )?;
        writeln!(
            out,
            "{dim}selectors:  {dim:#} {}",
            rules::selector_summary(kind_meta)
        )?;
        writeln!(
            out,
            "{dim}fix_ops:    {dim:#} {}",
            rules::fix_op_summary(kind_meta)
        )?;
        // Deep link to the kind's full docs. Family = its primary category slug
        // (equals the docs URL family segment); an alias resolves to its
        // canonical page. Suppressed by --no-docs, like the policy_url line.
        if show_docs && let Some(family) = cats.first() {
            writeln!(
                out,
                "{dim}docs:      {dim:#} {docs}https://alint.org/docs/rules/{family}/{}/{docs:#}",
                term(rules::canonical_kind(entry.kind())),
            )?;
        }
    }
    Ok(())
}

fn analysis_time_for_explain(
    root: &Path,
    entry: &alint_core::RuleEntry,
    registry: &alint_core::RuleRegistry,
    explicit: Option<alint_core::AnalysisTime>,
) -> Result<Option<alint_core::AnalysisTimeResolution>> {
    Ok(entry
        .wants_analysis_time(registry)?
        .then(|| alint_core::resolve_analysis_time(root, explicit)))
}

fn write_analysis_time_explain(
    out: &mut impl std::io::Write,
    resolution: Option<&alint_core::AnalysisTimeResolution>,
) -> Result<()> {
    use alint_core::AnalysisTimeResolution;
    use alint_output::style;

    let dim = style::DIM;
    match resolution {
        Some(AnalysisTimeResolution::Available { time, source }) => {
            writeln!(
                out,
                "{dim}analysis_time:{dim:#} {} ({source})",
                term(&time.to_string()),
            )?;
        }
        Some(AnalysisTimeResolution::Unavailable(error)) => {
            writeln!(out, "{dim}analysis_time:{dim:#} unavailable ({error})")?;
        }
        None => {}
    }
    Ok(())
}

fn analysis_time_json(
    resolution: Option<&alint_core::AnalysisTimeResolution>,
) -> Option<serde_json::Value> {
    match resolution {
        Some(alint_core::AnalysisTimeResolution::Available { time, source }) => {
            Some(serde_json::json!({
                "date": time.to_string(),
                "source": source.as_str(),
            }))
        }
        Some(alint_core::AnalysisTimeResolution::Unavailable(error)) => Some(serde_json::json!({
            "date": null,
            "source": "unavailable",
            "error": error.to_string(),
        })),
        None => None,
    }
}

fn run_explain_trace(
    loaded: &LoadedConfig,
    entry: &alint_core::RuleEntry,
    options: &ExplainOptions,
    analysis_time: Option<&alint_core::AnalysisTimeResolution>,
    use_gitignore: bool,
) -> Result<alint_core::RuleTrace> {
    let mut engine = Engine::from_entries(loaded.entries.clone(), loaded.registry.clone())
        .with_facts(loaded.facts.clone())
        .with_vars(loaded.vars.clone())
        .with_fix_size_limit(loaded.fix_size_limit);
    if let Some(resolution) = analysis_time.copied() {
        engine = engine.with_analysis_time_resolution(resolution);
    }
    if let Some(changed_paths) = options.changed.resolve(&loaded.root)? {
        engine = engine.with_changed_paths(changed_paths);
    }

    let mut extra_ignores = loaded.extra_ignores.clone();
    let configured_baseline = loaded.baseline.as_ref().map(|path| loaded.root.join(path));
    exclude_baseline_from_walk(
        &mut extra_ignores,
        &loaded.root,
        configured_baseline.as_deref(),
    );
    let index = walk(
        &loaded.root,
        &WalkOptions {
            respect_gitignore: use_gitignore && loaded.respect_gitignore,
            extra_ignores,
        },
    )
    .context("walking repository for rule trace")?;
    let trace_options = match options.preview_limit {
        Some(limit) => alint_core::TraceOptions::with_preview(limit)?,
        None => alint_core::TraceOptions::counts_only(),
    };
    engine
        .trace_rule_with_options(entry.rule.id(), &loaded.root, &index, trace_options)
        .context("tracing selected rule")
}

fn trace_wire_name(value: &impl serde::Serialize) -> Result<String> {
    Ok(serde_json::to_string(value)?.trim_matches('"').to_string())
}

fn write_explain_trace_json(document: &alint_core::RuleTraceDocument) -> Result<ExitCode> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{}", serde_json::to_string_pretty(document)?)?;
    out.flush().ok();
    Ok(ExitCode::SUCCESS)
}

fn write_trace_wrapped(
    out: &mut impl std::io::Write,
    prefix: &str,
    text: &str,
    width: usize,
) -> Result<()> {
    use alint_output::style;

    let safe = term_multiline(text);
    let indent = prefix.chars().count();
    for (index, line) in alint_output::wrap_message(&safe, indent, width)
        .into_iter()
        .enumerate()
    {
        if index == 0 {
            let dim = style::DIM;
            write!(out, "{dim}{prefix}{dim:#}")?;
        } else {
            write!(out, "{}", " ".repeat(indent))?;
        }
        writeln!(out, "{line}")?;
    }
    Ok(())
}

fn write_explain_trace_human(
    out: &mut impl std::io::Write,
    document: &alint_core::RuleTraceDocument,
    width: usize,
) -> Result<()> {
    use alint_output::style;

    let dim = style::DIM;
    writeln!(out, "{dim}trace:{dim:#}")?;
    for stage in &document.stages {
        write_explain_trace_stage(out, stage, width)?;
    }
    write_explain_trace_result(out, &document.result, width)
}

fn write_explain_trace_stage(
    out: &mut impl std::io::Write,
    stage: &alint_core::TraceStage,
    width: usize,
) -> Result<()> {
    let class = trace_wire_name(&stage.class)?;
    let status = trace_wire_name(&stage.status)?;
    let reason = trace_wire_name(&stage.reason)?;
    write_trace_wrapped(
        out,
        &format!("  {}: ", stage.ordinal),
        &format!("{class} / {} - {status} ({reason})", stage.name()),
        width,
    )?;
    let counts = &stage.counts;
    if counts.considered != 0 || counts.matched != 0 || counts.inputs != 0 || counts.values != 0 {
        write_trace_wrapped(
            out,
            "    counts: ",
            &format!(
                "considered={} matched={} inputs={} values={}",
                counts.considered, counts.matched, counts.inputs, counts.values,
            ),
            width,
        )?;
    }
    if counts.violations != 0 || counts.notes != 0 || counts.analysis_failures != 0 {
        write_trace_wrapped(
            out,
            "    findings: ",
            &format!(
                "violations={} notes={} analysis_failures={}",
                counts.violations, counts.notes, counts.analysis_failures,
            ),
            width,
        )?;
    }
    if counts.children != 0 {
        write_trace_wrapped(
            out,
            "    children: ",
            &format!(
                "total={} satisfied={} violated={} failed={} skipped={}",
                counts.children,
                counts.satisfied_children,
                counts.violated_children,
                counts.failed_children,
                counts.skipped_children,
            ),
            width,
        )?;
    }
    if let Some(preview) = stage.preview() {
        write_trace_wrapped(
            out,
            "    preview: ",
            &format!(
                "total={} shown={} omitted={} redacted={} truncated={}",
                preview.total(),
                preview.shown(),
                preview.omitted(),
                preview.redacted(),
                preview.value_truncated(),
            ),
            width,
        )?;
        for row in preview.rows() {
            let pointer = row
                .pointer()
                .map(|pointer| format!(" {pointer}"))
                .unwrap_or_default();
            write_trace_wrapped(
                out,
                "      ",
                &format!(
                    "{}{} = {}",
                    row.path().display(),
                    pointer,
                    row.display_value()
                ),
                width,
            )?;
        }
    }
    Ok(())
}

fn write_explain_trace_result(
    out: &mut impl std::io::Write,
    result: &alint_core::TraceResult,
    width: usize,
) -> Result<()> {
    let status = trace_wire_name(&result.status)?;
    write_trace_wrapped(
        out,
        "result: ",
        &format!(
            "{status} (violations={}, analysis_failures={}, notes={})",
            result.violations.len(),
            result.analysis_failures.len(),
            result.notes.len(),
        ),
        width,
    )?;
    for violation in &result.violations {
        let path = violation.path.as_deref().unwrap_or("<repository>");
        write_trace_wrapped(
            out,
            "  violation: ",
            &format!("{path}: {}", violation.message),
            width,
        )?;
    }
    for failure in &result.analysis_failures {
        let path = failure.path.as_deref().unwrap_or("<repository>");
        write_trace_wrapped(
            out,
            "  analysis_failure: ",
            &format!("{path}: {}", failure.message),
            width,
        )?;
    }
    for note in &result.notes {
        let path = note.finding.path.as_deref().unwrap_or("<repository>");
        write_trace_wrapped(
            out,
            &format!("  note[{}]: ", note.kind),
            &format!("{path}: {}", note.finding.message),
            width,
        )?;
    }
    Ok(())
}

fn build_explain_trace_document(
    loaded: &LoadedConfig,
    entry: &alint_core::RuleEntry,
    kind_meta: &alint_core::KindMeta,
    analysis_time: Option<&alint_core::AnalysisTimeResolution>,
    options: &ExplainOptions,
    no_gitignore: bool,
) -> Result<Option<alint_core::RuleTraceDocument>> {
    if !options.trace {
        return Ok(None);
    }
    let trace = run_explain_trace(loaded, entry, options, analysis_time, no_gitignore)?;
    let rule = serde_json::to_value(explain_rule_json_document(
        entry,
        kind_meta,
        analysis_time,
        &loaded.root,
        false,
    ))?;
    Ok(Some(trace.into_document(
        rule,
        alint_core::TraceExecution {
            changed: options.changed.enabled,
            base: options.changed.base.clone(),
            preview: options.preview_limit.is_some(),
            preview_limit: options.preview_limit.unwrap_or(20),
        },
        &loaded.root,
    )))
}

pub(super) fn cmd_explain(rule_id: &str, options: &ExplainOptions, cli: &Cli) -> Result<ExitCode> {
    use alint_core::Level;
    use alint_output::style;

    let loaded = load_rules_for_explain(Path::new("."), cli, options.trace.then_some(rule_id))?;
    let Some(entry) = loaded.entries.iter().find(|e| e.rule.id() == rule_id) else {
        bail!("no rule with id {rule_id:?} found in the effective config");
    };
    let rule = &entry.rule;
    let kind_meta = loaded
        .registry
        .meta(entry.kind())
        .expect("loaded rule must have capability metadata");
    let analysis_time =
        analysis_time_for_explain(&loaded.root, entry, &loaded.registry, cli.as_of)?;

    // `explain` honours --format for machine consumers, matching `list`:
    // `json` emits the rule's wire-shape; any other machine format is an
    // explicit error rather than silently printing the human block.
    let format: Format = cli.format.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    match format {
        Format::Human | Format::Json => {}
        _ => bail!(
            "`alint explain` supports only `--format human` or `--format json` (got {:?})",
            cli.format
        ),
    }
    let trace_document = build_explain_trace_document(
        &loaded,
        entry,
        kind_meta,
        analysis_time.as_ref(),
        options,
        !cli.no_gitignore,
    )?;
    if format == Format::Json {
        return match trace_document {
            Some(document) => write_explain_trace_json(&document),
            None => explain_json(entry, kind_meta, analysis_time.as_ref(), &loaded.root),
        };
    }

    let (mut out, opts) = render_env(cli)?;
    let dim = style::DIM;
    let level_style = match rule.level() {
        Level::Error => style::ERROR,
        Level::Warning => style::WARNING,
        Level::Info => style::INFO,
        Level::Off => style::DIM,
    };
    writeln!(out, "{dim}id:        {dim:#} {}", term(rule.id()))?;
    write_kind_explain(&mut out, entry, kind_meta, opts.show_docs)?;
    write_analysis_time_explain(&mut out, analysis_time.as_ref())?;
    writeln!(
        out,
        "{dim}level:     {dim:#} {level_style}{}{level_style:#}",
        rule.level().as_str(),
    )?;
    if let Some(paths) = entry.paths() {
        let rendered = if explain_field_is_sensitive(entry, "paths") {
            "<redacted>".to_string()
        } else {
            paths.render_scope()
        };
        writeln!(out, "{dim}paths:     {dim:#} {}", term(&rendered))?;
    }
    if entry.expect_matches() {
        writeln!(out, "{dim}expect_matches:{dim:#} true")?;
    }
    write_scope_filter_explain(&mut out, entry, &loaded.root)?;
    write_explain_rule_evidence(&mut out, entry, opts.show_docs)?;
    if let Some(document) = trace_document.as_ref() {
        write_explain_trace_human(&mut out, document, opts.effective_width())?;
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

fn write_explain_rule_evidence(
    out: &mut impl std::io::Write,
    entry: &alint_core::RuleEntry,
    show_docs: bool,
) -> Result<()> {
    use alint_output::style;

    let dim = style::DIM;
    let docs = style::DOCS;
    for (index, (key, value)) in entry.extra().into_iter().flatten().enumerate() {
        let field = key.as_str().unwrap_or_default();
        let key = term(field);
        let value = if explain_field_is_sensitive(entry, field) {
            "<redacted>".to_string()
        } else {
            match serde_json::to_value(value) {
                Ok(serde_json::Value::String(value)) if !value.contains(['\n', '\r']) => value,
                Ok(value) => value.to_string(),
                Err(_) => String::new(),
            }
        };
        let value = term(&value);
        if index == 0 {
            writeln!(out, "{dim}options:   {dim:#} {dim}{key}:{dim:#} {value}")?;
        } else {
            writeln!(out, "            {dim}{key}:{dim:#} {value}")?;
        }
    }
    if let Some(message) = entry.message() {
        let message = if explain_field_is_sensitive(entry, "message") {
            "<redacted>"
        } else {
            message.trim_end_matches('\n')
        };
        if !message.trim().is_empty() {
            let message = term_multiline(message).replace('\n', &format!("\n{}", " ".repeat(12)));
            writeln!(out, "{dim}message:   {dim:#} {message}")?;
        }
    }
    if show_docs && let Some(url) = entry.rule.policy_url() {
        let url = if explain_field_is_sensitive(entry, "policy_url") {
            "<redacted>"
        } else {
            url
        };
        let url = term(url);
        writeln!(out, "{dim}policy_url:{dim:#} {docs}{url}{docs:#}")?;
    }
    if let Some(condition) = entry.when_src() {
        let condition = if explain_field_is_sensitive(entry, "when") {
            "<redacted>"
        } else {
            condition
        };
        writeln!(out, "{dim}when:      {dim:#} {}", term(condition))?;
    }
    if let Some(fixer) = entry.rule.fixer() {
        let description = if explain_field_is_sensitive(entry, "fix") {
            std::borrow::Cow::Borrowed("<redacted>")
        } else {
            std::borrow::Cow::Owned(alint_core::Fixer::describe(fixer))
        };
        writeln!(
            out,
            "{dim}fix:       {dim:#} {}",
            term(description.as_ref())
        )?;
    }
    Ok(())
}

fn explain_scope_filter_json(
    entry: &alint_core::RuleEntry,
    root: &Path,
    redact_sensitive: bool,
) -> Option<serde_json::Value> {
    if redact_sensitive && explain_field_is_sensitive(entry, "scope_filter") {
        return entry
            .scope_filter()
            .map(|_| serde_json::Value::String("<redacted>".to_string()));
    }
    entry.scope_filter().map(|scope| {
        let manifest_scopes: Vec<_> =
            alint_core::ScopeFilter::from_spec(entry.rule.id(), scope.clone())
                .map(|filter| filter.resolved_manifest_sets(root))
                .unwrap_or_default()
                .into_iter()
                .map(|resolved| {
                    serde_json::json!({
                        "predicate": if resolved.include {
                            "include_manifest_paths"
                        } else {
                            "exclude_manifest_paths"
                        },
                        "source": explain_repository_path(root, &resolved.source),
                        "paths": resolved.paths
                            .iter()
                            .map(|path| explain_repository_path(root, path))
                            .collect::<Vec<_>>(),
                    })
                })
                .collect();
        serde_json::json!({
            "has_ancestor": scope.has_ancestor,
            "changed_since": scope.changed_since,
            "manifest_scopes": manifest_scopes,
        })
    })
}

/// Machine-readable rule body shared by stable static explain and the
/// experimental trace envelope. Structural ids remain visible; protected
/// fields, including environment-derived selectors, are redacted.
#[derive(serde::Serialize)]
struct ExplainRuleDocument<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    analysis_time: Option<serde_json::Value>,
    capabilities: ExplainCapabilities<'a>,
    categories: Vec<&'static str>,
    conditional: bool,
    docs: Option<String>,
    expect_matches: bool,
    fix: Option<serde_json::Value>,
    fix_ops: ExplainFixOps<'a>,
    fixable: bool,
    id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'static str>,
    level: &'static str,
    message: Option<serde_json::Value>,
    options: serde_json::Value,
    paths: Option<serde_json::Value>,
    policy_url: Option<serde_json::Value>,
    rule_kind: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema_version: Option<u32>,
    scope_filter: Option<serde_json::Value>,
    selectors: ExplainSelectors<'a>,
    summary: Option<&'static str>,
    when: Option<serde_json::Value>,
}

struct ExplainCapabilities<'a>(&'a alint_core::KindMeta);

impl serde::Serialize for ExplainCapabilities<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serde::Serialize::serialize(&self.0.serialized_capabilities(), serializer)
    }
}

struct ExplainFixOps<'a>(&'a alint_core::KindMeta);

impl serde::Serialize for ExplainFixOps<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serde::Serialize::serialize(self.0.fix_ops.as_ref(), serializer)
    }
}

struct ExplainSelectors<'a>(&'a alint_core::KindMeta);

impl serde::Serialize for ExplainSelectors<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serde::Serialize::serialize(self.0.selectors.as_ref(), serializer)
    }
}

fn explain_rule_json_document<'a>(
    entry: &'a alint_core::RuleEntry,
    kind_meta: &'a alint_core::KindMeta,
    analysis_time: Option<&alint_core::AnalysisTimeResolution>,
    root: &Path,
    stable_envelope: bool,
) -> ExplainRuleDocument<'a> {
    let sensitive = |field: &str| {
        entry
            .spec
            .as_deref()
            .is_some_and(|spec| spec.field_is_sensitive(field))
    };
    let redacted = || serde_json::Value::String("<redacted>".to_string());
    let paths = entry.paths().map(|paths| {
        if sensitive("paths") {
            serde_json::json!({ "include": ["<redacted>"], "exclude": [] })
        } else {
            let (include, exclude) = paths.include_exclude();
            serde_json::json!({ "include": include, "exclude": exclude })
        }
    });
    let options = match entry.extra() {
        Some(extra) if !extra.is_empty() => {
            let mut options = serde_json::Map::new();
            for (key, value) in extra {
                let key = key.as_str().unwrap_or_default().to_string();
                let value = if sensitive(&key) {
                    redacted()
                } else {
                    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
                };
                options.insert(key, value);
            }
            serde_json::Value::Object(options)
        }
        _ => serde_json::Value::Null,
    };
    let evidence = |field: &str, value: Option<&str>| {
        value.map(|value| {
            if sensitive(field) {
                redacted()
            } else {
                serde_json::Value::String(value.to_string())
            }
        })
    };
    let fix = entry.rule.fixer().map(|fixer| {
        if sensitive("fix") {
            redacted()
        } else {
            serde_json::Value::String(alint_core::Fixer::describe(fixer))
        }
    });
    let categories = rules::categories_for_kind(entry.kind());
    let docs = categories.first().map(|family| {
        format!(
            "https://alint.org/docs/rules/{family}/{}/",
            rules::canonical_kind(entry.kind())
        )
    });

    ExplainRuleDocument {
        analysis_time: analysis_time_json(analysis_time),
        capabilities: ExplainCapabilities(kind_meta),
        categories,
        conditional: entry.when_src().is_some(),
        docs,
        expect_matches: entry.expect_matches(),
        fix,
        fix_ops: ExplainFixOps(kind_meta),
        fixable: entry.rule.fixer().is_some(),
        id: entry.rule.id(),
        kind: stable_envelope.then_some("rule"),
        level: entry.rule.level().as_str(),
        message: evidence(
            "message",
            entry.message().filter(|message| !message.trim().is_empty()),
        ),
        options,
        paths,
        policy_url: evidence("policy_url", entry.rule.policy_url()),
        rule_kind: entry.kind(),
        schema_version: stable_envelope.then_some(1),
        scope_filter: explain_scope_filter_json(entry, root, true),
        selectors: ExplainSelectors(kind_meta),
        summary: rules::description_for_kind(entry.kind()),
        when: evidence("when", entry.when_src()),
    }
}

/// Stable machine-readable single-rule `kind: "rule"` document.
fn explain_json(
    entry: &alint_core::RuleEntry,
    kind_meta: &alint_core::KindMeta,
    analysis_time: Option<&alint_core::AnalysisTimeResolution>,
    root: &Path,
) -> Result<ExitCode> {
    let document = explain_rule_json_document(entry, kind_meta, analysis_time, root, true);
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    serde_json::to_writer_pretty(&mut out, &document)?;
    writeln!(out)?;
    out.flush().ok();
    Ok(ExitCode::SUCCESS)
}
