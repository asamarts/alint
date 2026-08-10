//! `alint rules` - catalog discovery, config-independent.
//!
//! Unlike `alint list` / `alint explain` (which read THIS repo's effective
//! config), `alint rules` browses the rule kinds alint ships, from the generated
//! in-crate category bridge (`alint_rules::categories`). It never loads a config
//! and works anywhere, including outside a repo. See ADR-0009.

use std::collections::BTreeMap;
use std::io::Write;
use std::process::ExitCode;

use alint_core::Category;
use alint_output::Format;
use alint_rules::categories::{ALIAS_TO_CANONICAL, KIND_CATEGORIES};
use alint_rules::kind_docs::KIND_SUMMARIES;
use anyhow::{Result, bail};

use crate::{Cli, RulesCommand};

/// A catalog row: a canonical kind, its category slugs (primary first), and its
/// alias spellings.
struct Row {
    kind: &'static str,
    categories: Vec<&'static str>,
    aliases: Vec<&'static str>,
    summary: Option<&'static str>,
}

pub(crate) fn run(command: &RulesCommand, cli: &Cli) -> Result<ExitCode> {
    let format: Format = cli.format.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    if !matches!(format, Format::Human | Format::Json) {
        bail!(
            "`alint rules` supports only `--format human` or `--format json` (got {:?})",
            cli.format
        );
    }
    match command {
        RulesCommand::List { category, search } => {
            list(category.as_deref(), search.as_deref(), format)
        }
        RulesCommand::Categories => categories(format),
        RulesCommand::Show { kind } => show(kind, format),
    }
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

/// Resolve an alias spelling to its canonical kind (identity for a canonical or
/// unknown kind). The canonical kind is the one with a `docs/rules.md` page, so
/// deep links use it.
pub(crate) fn canonical_kind(kind: &str) -> &str {
    ALIAS_TO_CANONICAL
        .iter()
        .find(|(alias, _)| *alias == kind)
        .map_or(kind, |(_, canon)| canon)
}

/// canonical kind -> its alias spellings, sorted.
fn aliases_by_canonical() -> BTreeMap<&'static str, Vec<&'static str>> {
    let mut map: BTreeMap<&'static str, Vec<&'static str>> = BTreeMap::new();
    for (alias, canon) in ALIAS_TO_CANONICAL {
        map.entry(canon).or_default().push(alias);
    }
    for aliases in map.values_mut() {
        aliases.sort_unstable();
    }
    map
}

fn list(category: Option<&str>, search: Option<&str>, format: Format) -> Result<ExitCode> {
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
    let alias_map = aliases_by_canonical();

    let mut rows: Vec<Row> = Vec::new();
    for (kind, cats) in KIND_CATEGORIES {
        let slugs: Vec<&'static str> = cats.iter().map(|c| c.slug()).collect();
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

    let mut out = std::io::stdout().lock();
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
                cats = titles.join(", "),
            )?;
            if let Some(s) = r.summary {
                writeln!(out, "  {blank:<width$}  {s}", blank = "")?;
            }
        }
    }
    out.flush().ok();
    Ok(ExitCode::SUCCESS)
}

/// `alint rules show <kind>`: a single kind's summary, categories, aliases, and
/// docs link (ADR-0011). Accepts an alias, resolving to its canonical kind.
fn show(kind: &str, format: Format) -> Result<ExitCode> {
    let cats = categories_for_kind(kind);
    if cats.is_empty() {
        bail!("unknown rule kind {kind:?}. Run `alint rules list` for the catalog.");
    }
    let canonical = canonical_kind(kind);
    let summary = summary_for_kind(kind);
    let aliases = aliases_by_canonical()
        .get(canonical)
        .cloned()
        .unwrap_or_default();
    let family = cats.first().copied().unwrap_or_default();
    let docs = format!("https://alint.org/docs/rules/{family}/{canonical}/");

    let mut out = std::io::stdout().lock();
    if format == Format::Json {
        let doc = serde_json::json!({
            "schema_version": 1,
            "kind": "rule-definition",
            "rule_kind": canonical,
            "categories": cats,
            "aliases": aliases,
            "summary": summary,
            "docs": docs,
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
        writeln!(out, "  categories: {}", titles.join(", "))?;
        if !aliases.is_empty() {
            writeln!(out, "  aliases:    {}", aliases.join(", "))?;
        }
        writeln!(out, "  docs:       {docs}")?;
    }
    out.flush().ok();
    Ok(ExitCode::SUCCESS)
}

fn categories(format: Format) -> Result<ExitCode> {
    // Kinds per category (a kind counts once per category it belongs to).
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (_kind, cats) in KIND_CATEGORIES {
        for c in *cats {
            *counts.entry(c.slug()).or_default() += 1;
        }
    }

    let mut out = std::io::stdout().lock();
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
    out.flush().ok();
    Ok(ExitCode::SUCCESS)
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
        assert!(!cats.is_empty());
        assert_eq!(cats.first(), Some(&"security-unicode-sanity"));
        // an alias resolves to its canonical kind's categories
        assert_eq!(
            categories_for_kind("content_matches"),
            categories_for_kind("file_content_matches")
        );
        assert!(!categories_for_kind("content_matches").is_empty());
        // an unknown kind yields no categories (rather than panicking)
        assert!(categories_for_kind("does_not_exist").is_empty());
    }
}
