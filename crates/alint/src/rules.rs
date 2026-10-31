//! `alint rules` - catalog discovery, config-independent.
//!
//! Unlike `alint list` / `alint explain` (which read THIS repo's effective
//! config), `alint rules` browses the rule kinds the registry can build: the
//! built-in catalog, or the registry an embedder injected through
//! `Cli::registry`. Categories and summaries come from the generated in-crate
//! bridges (`alint_rules::categories`, `alint_rules::kind_docs`), so a kind they
//! do not describe is listed by name with no category. It never loads a config
//! and works anywhere, including outside a repo. See ADR-0009.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Write;
use std::process::ExitCode;

use alint_core::{
    Capability, CapabilityCondition, Category, GitOperation, KindMeta, OptionPredicate,
    RuleRegistry, SelectorType,
};
use alint_output::Format;
use alint_rules::categories::{ALIAS_TO_CANONICAL, KIND_CATEGORIES};
use alint_rules::kind_docs::{KIND_DESCRIPTIONS, KIND_SUMMARIES};
use anyhow::{Result, bail};

use crate::{Cli, RulesCommand};

/// A catalog row: a canonical kind, its category slugs (primary first), and its
/// alias spellings.
struct Row<'a> {
    kind: &'a str,
    categories: Vec<&'static str>,
    aliases: Vec<&'a str>,
    summary: Option<&'static str>,
}

pub(crate) struct CapabilitySummary<'a>(&'a KindMeta);

pub(crate) struct SelectorSummary<'a>(&'a KindMeta);

pub(crate) struct FixOpSummary<'a>(&'a KindMeta);
pub(crate) struct TraceSummary<'a>(&'a KindMeta);

pub(crate) fn capability_summary(meta: &KindMeta) -> CapabilitySummary<'_> {
    CapabilitySummary(meta)
}

pub(crate) fn selector_summary(meta: &KindMeta) -> SelectorSummary<'_> {
    SelectorSummary(meta)
}

pub(crate) fn fix_op_summary(meta: &KindMeta) -> FixOpSummary<'_> {
    FixOpSummary(meta)
}

pub(crate) fn trace_summary(meta: &KindMeta) -> TraceSummary<'_> {
    TraceSummary(meta)
}

impl fmt::Display for CapabilitySummary<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut capabilities = self.0.effective_capabilities().peekable();
        if capabilities.peek().is_none() {
            return f.write_str("none");
        }
        for (index, entry) in capabilities.enumerate() {
            if index != 0 {
                f.write_str(", ")?;
            }
            match &entry.capability {
                Capability::SpawnUserCommand => f.write_str("spawn_user_command")?,
                Capability::Git { operations } => {
                    f.write_str("git(")?;
                    for (op_index, op) in operations.iter().enumerate() {
                        if op_index != 0 {
                            f.write_str(", ")?;
                        }
                        f.write_str(git_operation_name(*op))?;
                    }
                    f.write_str(")")?;
                }
                Capability::HostKeyring => f.write_str("host_keyring")?,
                Capability::EnvRead => f.write_str("env_read")?,
                Capability::EnvExport => f.write_str("env_export")?,
                Capability::AnalysisTime => f.write_str("analysis_time")?,
                Capability::OutOfRootRead => f.write_str("out_of_root_read")?,
                Capability::WritesDuringCheck => f.write_str("writes_during_check")?,
                Capability::UnixOnly => f.write_str("unix_only")?,
            }
            match &entry.condition {
                CapabilityCondition::Always => {}
                CapabilityCondition::OptionSet { option } => {
                    write!(f, " [when {option} is set]")?;
                }
                CapabilityCondition::OptionMatches { option, predicate } => {
                    write!(
                        f,
                        " [when {option} {}]",
                        match predicate {
                            OptionPredicate::ContainsLegacyEnvReference =>
                                "contains a legacy environment reference",
                        }
                    )?;
                }
                CapabilityCondition::FixOpConfigured { op } => {
                    write!(f, " [when fix.{op} is configured]")?;
                }
                CapabilityCondition::CommonFieldSet { field } => {
                    write!(f, " [when {field} is set]")?;
                }
            }
        }
        Ok(())
    }
}

impl fmt::Display for SelectorSummary<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.selectors.is_empty() {
            return f.write_str("none");
        }
        for (index, selector) in self.0.selectors.iter().enumerate() {
            if index != 0 {
                f.write_str(", ")?;
            }
            write!(
                f,
                "{} ({}{})",
                selector.name,
                selector_type_name(selector.selector_type),
                if selector.rerootable {
                    ", rerootable"
                } else {
                    ""
                }
            )?;
        }
        Ok(())
    }
}

impl fmt::Display for FixOpSummary<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.fix_ops.is_empty() {
            return f.write_str("none");
        }
        for (index, op) in self.0.fix_ops.iter().enumerate() {
            if index != 0 {
                f.write_str(", ")?;
            }
            f.write_str(op)?;
        }
        Ok(())
    }
}

impl fmt::Display for TraceSummary<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("required=")?;
        write_trace_stages(f, self.0.trace.required_stages.as_ref())?;
        let optional = self
            .0
            .trace
            .allowed_stages
            .iter()
            .copied()
            .filter(|stage| !self.0.trace.required_stages.contains(stage));
        f.write_str("; optional=")?;
        let mut optional = optional.peekable();
        if optional.peek().is_none() {
            f.write_str("none")?;
        } else {
            for (index, stage) in optional.enumerate() {
                if index != 0 {
                    f.write_str(",")?;
                }
                f.write_str(trace_stage_name(stage))?;
            }
        }
        write!(
            f,
            "; preview={}; nested={}",
            self.0.trace.emits_preview_rows, self.0.trace.nested_evaluation
        )
    }
}

fn write_trace_stages(
    f: &mut fmt::Formatter<'_>,
    stages: &[alint_core::TraceStageClass],
) -> fmt::Result {
    for (index, stage) in stages.iter().copied().enumerate() {
        if index != 0 {
            f.write_str(",")?;
        }
        f.write_str(trace_stage_name(stage))?;
    }
    Ok(())
}

const fn trace_stage_name(stage: alint_core::TraceStageClass) -> &'static str {
    match stage {
        alint_core::TraceStageClass::Gate => "gate",
        alint_core::TraceStageClass::Selection => "selection",
        alint_core::TraceStageClass::Extraction => "extraction",
        alint_core::TraceStageClass::Cardinality => "cardinality",
        alint_core::TraceStageClass::Assertion => "assertion",
        alint_core::TraceStageClass::Nested => "nested",
        alint_core::TraceStageClass::Outcome => "outcome",
    }
}

const fn git_operation_name(operation: GitOperation) -> &'static str {
    match operation {
        GitOperation::TrackedSet => "tracked_set",
        GitOperation::Blame => "blame",
        GitOperation::Diff => "diff",
        GitOperation::Log => "log",
        GitOperation::VerifyCommit => "verify_commit",
        GitOperation::Untrack => "untrack",
    }
}

const fn selector_type_name(selector_type: SelectorType) -> &'static str {
    match selector_type {
        SelectorType::Glob => "glob",
        SelectorType::GlobList => "glob_list",
        SelectorType::BasenameGlob => "basename_glob",
        SelectorType::LiteralPath => "literal_path",
        SelectorType::PathTemplate => "path_template",
        SelectorType::Regex => "regex",
        SelectorType::DiffGlob => "diff_glob",
        SelectorType::Extract => "extract",
    }
}

pub(crate) fn run(command: &RulesCommand, cli: &Cli) -> Result<ExitCode> {
    let format: Format = cli.format.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    if !matches!(format, Format::Human | Format::Json) {
        bail!(
            "`alint rules` supports only `--format human` or `--format json` (got {:?})",
            cli.format
        );
    }
    let registry = cli.registry();
    let mut out = std::io::stdout().lock();
    match command {
        RulesCommand::List { category, search } => list(
            &mut out,
            &registry,
            category.as_deref(),
            search.as_deref(),
            format,
        )?,
        RulesCommand::Categories => categories(&mut out, &registry, format)?,
        RulesCommand::Show { kind } => show(&mut out, &registry, kind, format, !cli.no_docs)?,
    }
    out.flush().ok();
    Ok(ExitCode::SUCCESS)
}

/// The category slugs of a rule KIND (resolving an alias to its canonical kind
/// first), primary first. Empty for an unknown kind. Shared with
/// `alint list --category` so the two commands map kinds identically.
pub(crate) fn categories_for_kind(kind: &str) -> Vec<&'static str> {
    let canonical = ALIAS_TO_CANONICAL
        .iter()
        .find(|(alias, _)| *alias == kind)
        .map_or(kind, |(_, canon)| canon);
    KIND_CATEGORIES
        .iter()
        .find(|(k, _)| *k == canonical)
        .map_or_else(Vec::new, |(_, cats)| {
            cats.iter().map(|c| c.slug()).collect()
        })
}

/// The one-line summary of a rule KIND (resolving an alias to its canonical kind
/// first), or `None` for an unknown kind or an empty summary. Sourced from the
/// generated docs bridge (ADR-0011); shared by `alint explain` and `alint rules`.
pub(crate) fn summary_for_kind(kind: &str) -> Option<&'static str> {
    KIND_SUMMARIES
        .iter()
        .find(|(k, _)| *k == canonical_kind(kind))
        .map(|(_, s)| *s)
        .filter(|s| !s.is_empty())
}

/// The full, cleaned opening sentence for detail surfaces such as `rules show`
/// and `explain`; list/search surfaces use the capped summary above.
pub(crate) fn description_for_kind(kind: &str) -> Option<&'static str> {
    KIND_DESCRIPTIONS
        .iter()
        .find(|(k, _)| *k == canonical_kind(kind))
        .map(|(_, s)| *s)
        .filter(|s| !s.is_empty())
}

/// Resolve an alias spelling to its canonical kind (identity for a canonical or
/// unknown kind). The canonical kind is the one with a `docs/rules.md` page, so
/// deep links use it.
pub(crate) fn canonical_kind(kind: &str) -> &str {
    ALIAS_TO_CANONICAL
        .iter()
        .find(|(alias, _)| *alias == kind)
        .map_or(kind, |(_, canon)| canon)
}

/// canonical kind -> its alias spellings in `registry`, sorted.
fn aliases_by_canonical(registry: &RuleRegistry) -> BTreeMap<&str, Vec<&str>> {
    let mut map: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (alias, canon) in registry.aliases() {
        map.entry(canon).or_default().push(alias);
    }
    for aliases in map.values_mut() {
        aliases.sort_unstable();
    }
    map
}

/// The registry's canonical kinds with their category slugs: the kinds the
/// generated catalog describes, in its order, then any other registered kind
/// (an embedder's own) by name, with no category.
fn catalog_kinds(registry: &RuleRegistry) -> Vec<(&str, Vec<&'static str>)> {
    let mut kinds: Vec<(&str, Vec<&'static str>)> = KIND_CATEGORIES
        .iter()
        .filter(|(kind, _)| registry.meta(kind).is_some())
        .map(|(kind, cats)| (*kind, cats.iter().map(|c| c.slug()).collect()))
        .collect();
    let mut uncatalogued: Vec<&str> = registry
        .canonical_kinds()
        .filter(|kind| !KIND_CATEGORIES.iter().any(|(known, _)| known == kind))
        .collect();
    uncatalogued.sort_unstable();
    kinds.extend(uncatalogued.into_iter().map(|kind| (kind, Vec::new())));
    kinds
}

pub(crate) fn list(
    out: &mut dyn Write,
    registry: &RuleRegistry,
    category: Option<&str>,
    search: Option<&str>,
    format: Format,
) -> Result<()> {
    // Validate the category slug up front: a clear error beats a silent empty list.
    if let Some(slug) = category
        && Category::from_slug(slug).is_none()
    {
        let known: Vec<&str> = Category::ALL.iter().map(|c| c.slug()).collect();
        bail!(
            "unknown category {slug:?}. Known categories: {}",
            known.join(", ")
        );
    }
    let needle = search.map(str::to_lowercase);
    let alias_map = aliases_by_canonical(registry);

    let mut rows: Vec<Row> = Vec::new();
    for (kind, slugs) in catalog_kinds(registry) {
        if let Some(slug) = category
            && !slugs.contains(&slug)
        {
            continue;
        }
        let aliases = alias_map.get(kind).cloned().unwrap_or_default();
        let summary = summary_for_kind(kind);
        if let Some(term) = &needle {
            let hit = kind.to_lowercase().contains(term)
                || aliases.iter().any(|a| a.to_lowercase().contains(term))
                || summary.is_some_and(|s| s.to_lowercase().contains(term));
            if !hit {
                continue;
            }
        }
        rows.push(Row {
            kind,
            categories: slugs,
            aliases,
            summary,
        });
    }

    if format == Format::Json {
        let rules: Vec<_> = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "kind": r.kind,
                    "categories": r.categories,
                    "aliases": r.aliases,
                    "summary": r.summary,
                })
            })
            .collect();
        let doc = serde_json::json!({
            "schema_version": 1,
            "kind": "rule-catalog",
            "rules": rules,
        });
        writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
    } else {
        let n = rows.len();
        writeln!(
            out,
            "Rule catalog ({n} kind{})\n",
            if n == 1 { "" } else { "s" }
        )?;
        let width = rows.iter().map(|r| r.kind.len()).max().unwrap_or(0);
        for r in &rows {
            let titles: Vec<&str> = r
                .categories
                .iter()
                .filter_map(|s| Category::from_slug(s))
                .map(Category::title)
                .collect();
            let alias_note = if r.aliases.is_empty() {
                String::new()
            } else {
                format!("  (alias: {})", r.aliases.join(", "))
            };
            writeln!(
                out,
                "  {kind:<width$}  {cats}{alias_note}",
                kind = r.kind,
                cats = category_titles(&titles),
            )?;
            if let Some(s) = r.summary {
                writeln!(out, "  {blank:<width$}  {s}", blank = "")?;
            }
        }
    }
    Ok(())
}

/// Category titles for human output; a kind the generated catalog does not
/// describe has none.
fn category_titles(titles: &[&str]) -> String {
    if titles.is_empty() {
        "uncategorized".to_string()
    } else {
        titles.join(", ")
    }
}

/// `alint rules show <kind>`: one kind's summary, categories, aliases,
/// capabilities, accepted selectors, fix operations, and docs link.
/// Accepts an alias, resolving to its canonical kind. The kind must be in
/// `registry`; categories, the summary, and the docs link exist only for kinds
/// the generated catalog describes.
pub(crate) fn show(
    out: &mut dyn Write,
    registry: &RuleRegistry,
    kind: &str,
    format: Format,
    show_docs: bool,
) -> Result<()> {
    let canonical = registry.canonical_kind(kind);
    let Some(meta) = registry.meta(canonical) else {
        bail!("unknown rule kind {kind:?}. Run `alint rules list` for the catalog.");
    };
    let cats = categories_for_kind(canonical);
    let summary = description_for_kind(canonical);
    let aliases = aliases_by_canonical(registry)
        .remove(canonical)
        .unwrap_or_default();
    let docs = cats
        .first()
        .map(|family| format!("https://alint.org/docs/rules/{family}/{canonical}/"));

    if format == Format::Json {
        let doc = serde_json::json!({
            "schema_version": 1,
            "kind": "rule-definition",
            "rule_kind": canonical,
            "categories": cats,
            "aliases": aliases,
            "summary": summary,
            "docs": docs,
            "capabilities": meta.serialized_capabilities(),
            "selectors": meta.selectors.as_ref(),
            "fix_ops": meta.fix_ops.as_ref(),
            "trace": &meta.trace,
        });
        writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
    } else {
        let titles: Vec<&str> = cats
            .iter()
            .filter_map(|s| Category::from_slug(s))
            .map(Category::title)
            .collect();
        writeln!(out, "{canonical}")?;
        if let Some(s) = summary {
            writeln!(out, "  {s}")?;
        }
        writeln!(out, "  categories: {}", category_titles(&titles))?;
        if !aliases.is_empty() {
            writeln!(out, "  aliases:    {}", aliases.join(", "))?;
        }
        writeln!(out, "  capabilities: {}", capability_summary(meta))?;
        writeln!(out, "  selectors:    {}", selector_summary(meta))?;
        writeln!(out, "  fix_ops:      {}", fix_op_summary(meta))?;
        writeln!(out, "  trace:        {}", trace_summary(meta))?;
        if show_docs && let Some(docs) = &docs {
            writeln!(out, "  docs:       {docs}")?;
        }
    }
    Ok(())
}

pub(crate) fn categories(
    out: &mut dyn Write,
    registry: &RuleRegistry,
    format: Format,
) -> Result<()> {
    // Kinds per category (a kind counts once per category it belongs to).
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (_kind, cats) in catalog_kinds(registry) {
        for slug in cats {
            *counts.entry(slug).or_default() += 1;
        }
    }

    if format == Format::Json {
        let cats: Vec<_> = Category::ALL
            .iter()
            .map(|c| {
                serde_json::json!({
                    "slug": c.slug(),
                    "title": c.title(),
                    "order": c.order(),
                    "kinds": counts.get(c.slug()).copied().unwrap_or(0),
                })
            })
            .collect();
        let doc = serde_json::json!({
            "schema_version": 1,
            "kind": "rule-categories",
            "categories": cats,
        });
        writeln!(out, "{}", serde_json::to_string_pretty(&doc)?)?;
    } else {
        writeln!(out, "Rule categories ({})\n", Category::ALL.len())?;
        let sw = Category::ALL
            .iter()
            .map(|c| c.slug().len())
            .max()
            .unwrap_or(0);
        let tw = Category::ALL
            .iter()
            .map(|c| c.title().len())
            .max()
            .unwrap_or(0);
        for c in Category::ALL {
            let n = counts.get(c.slug()).copied().unwrap_or(0);
            writeln!(
                out,
                "  {slug:<sw$}  {title:<tw$}  {n} kind{plural}",
                slug = c.slug(),
                title = c.title(),
                plural = if n == 1 { "" } else { "s" },
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_for_kind_resolves_canonical_and_alias() {
        // A canonical kind resolves to its categories, its family (primary) FIRST.
        // Exact membership is gated by `gen-categories --check` against the bridge, so
        // this test pins only resolution + the primary-first invariant — a many-to-many
        // flip (a kind gaining a secondary) must not churn it.
        let cats = categories_for_kind("no_bidi_controls");
        assert_ne!(cats, [] as [&str; 0]);
        assert_eq!(cats.first(), Some(&"security-unicode-sanity"));
        // an alias resolves to its canonical kind's categories
        assert_eq!(
            categories_for_kind("content_matches"),
            categories_for_kind("file_content_matches")
        );
        assert_ne!(categories_for_kind("content_matches"), [] as [&str; 0]);
        // an unknown kind yields no categories (rather than panicking)
        assert_eq!(categories_for_kind("does_not_exist"), [] as [&str; 0]);
    }
}
