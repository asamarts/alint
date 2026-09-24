//! SERP descriptions for the pages `docs-export` copies verbatim from repo docs
//! (CHANGELOG, ARCHITECTURE, ROADMAP, the crate graph, the rule-authoring
//! guide). `copy_one` and the public-roadmap generator inject only a `title:`.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::escape_yaml_string;

/// `description:` frontmatter for the pages exported verbatim from repo docs
/// (`copy_one` and the public-roadmap generator inject only a `title:`).
/// Without one, Starlight falls back to the site-wide description, so these
/// pages all shared a single meta description on alint.org. Keyed by bundle
/// path.
pub(super) const DESCRIPTIONS: &[(&str, &str)] = &[
    (
        "changelog.md",
        "Every alint release, newest first: new rule kinds and rulesets, CLI and config changes, fixes, and performance notes.",
    ),
    (
        "about/architecture.md",
        "How alint works inside: the rule model, the config DSL, the single-pass execution model, the crate layout, plugins, and output formats.",
    ),
    (
        "about/roadmap.md",
        "What each alint release shipped and what is planned next, version by version, from the v0.1 MVP to the latest release.",
    ),
    (
        "about/crate-graph.md",
        "How alint's Cargo workspace crates depend on each other, drawn and tiered from the crate manifests so the graph can't drift.",
    ),
    (
        "development/rule-authoring.md",
        "The checklist for adding a rule kind, bundled ruleset, or rule-kind alias to alint so the coverage audits stay green and CI passes.",
    ),
];

/// Add `description: '<description>'` to the YAML frontmatter of a generated
/// page, after its existing keys. A page that already declares a description
/// keeps it.
pub(super) fn set_frontmatter_description(path: &Path, description: &str) -> Result<()> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let Some(rest) = text.strip_prefix("---\n") else {
        bail!(
            "{} has no frontmatter to add a description to",
            path.display()
        );
    };
    let Some(end) = rest.find("\n---\n") else {
        bail!("{} has an unterminated frontmatter block", path.display());
    };
    let (front, body) = rest.split_at(end);
    if front.lines().any(|line| line.starts_with("description:")) {
        return Ok(());
    }
    let page = format!(
        "---\n{front}\ndescription: '{}'{body}",
        escape_yaml_string(description)
    );
    fs::write(path, page).with_context(|| format!("writing {}", path.display()))
}
