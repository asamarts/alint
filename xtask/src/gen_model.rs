//! `xtask gen-model` — emit the code-derived `LikeC4` model fragments for the
//! architecture diagrams, gated by `--check` (mirrors `gen-arch`/`gen-facts`).
//!
//! The architecture model lives in `docs/design/architecture/model/` as a
//! `LikeC4` workspace: hand-authored intent (`alint.c4`) plus these generated
//! `*.gen.c4` fragments derived from canonical sources. `likec4 validate` (CI)
//! checks the merged model; `gen-model --check` byte-gates the fragments, so
//! the code-derived diagrams can't drift from their source.
//!
//! Fragments:
//! - `rule-families.gen.c4` — the rule-kind taxonomy (families -> kinds), from
//!   `docs/rules.md` (the same `## family` / `### kind` structure that
//!   `facts.json`'s family and kind lists derive from).
//!
//! `--check` additionally gates the hand-authored model against its sources: the
//! `alint.c4` crate elements *and* their runtime edges against `cargo metadata`,
//! the `config-model.c4` Config keys against the JSON schema, and the taxonomy's
//! completeness against `all_kinds.yaml`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

const RULE_FAMILIES_C4: &str = "docs/design/architecture/model/rule-families.gen.c4";
const MODEL_C4: &str = "docs/design/architecture/model/alint.c4";
const CONFIG_MODEL_C4: &str = "docs/design/architecture/model/config-model.c4";
const RULES_MD: &str = "docs/rules.md";
const SCHEMA_JSON: &str = "schemas/v1/config.json";
const ALL_KINDS_YAML: &str = "crates/alint-dsl/tests/fixtures/all_kinds.yaml";

/// `docs/rules.md` `## ` headings that are not rule families.
const META_FAMILIES: &[&str] = &[
    "Contents",
    "Fix operations",
    "Bundled rulesets",
    "Nested `.alint.yml` (monorepo layering)",
];

struct Family {
    title: String,
    slug: String,
    kinds: Vec<String>,
}

pub fn run(check: bool) -> Result<()> {
    let root = crate::workspace_root()?;
    let md = fs::read_to_string(root.join(RULES_MD)).with_context(|| format!("read {RULES_MD}"))?;
    let families = parse_families(&md);
    if families.is_empty() {
        bail!("no rule families parsed from {RULES_MD}");
    }
    let rendered = render(&families);
    let path = root.join(RULE_FAMILIES_C4);

    if check {
        let committed =
            fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        if committed != rendered {
            bail!(
                "{RULE_FAMILIES_C4} is stale. Run `cargo run -p xtask -- gen-model` \
                 to regenerate and commit the result."
            );
        }
        check_model_crate_set(&root)?;
        check_model_crate_edges(&root)?;
        check_config_model_root_keys(&root)?;
        check_taxonomy_complete(&families, &md, &root)?;
        println!(
            "{RULE_FAMILIES_C4} is up to date; model gates pass (crate set + edges, config keys, taxonomy completeness)"
        );
        return Ok(());
    }

    fs::write(&path, &rendered).with_context(|| format!("write {}", path.display()))?;
    println!("wrote {RULE_FAMILIES_C4}");
    Ok(())
}

/// Slugify a family title into a valid `LikeC4` identifier: lowercase, each run
/// of non-alphanumerics collapses to one `_`, no leading/trailing `_`, and a
/// leading digit (or empty) is prefixed so the id is always identifier-safe.
fn slug(title: &str) -> String {
    let mut s = String::new();
    let mut prev_us = true; // seed true so a leading separator is dropped
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
            prev_us = false;
        } else if !prev_us {
            s.push('_');
            prev_us = true;
        }
    }
    let s = s.trim_matches('_').to_string();
    let leading_digit = s.as_bytes().first().is_some_and(u8::is_ascii_digit);
    if s.is_empty() || leading_digit {
        format!("f_{s}")
    } else {
        s
    }
}

/// Canonical kind name(s) from an H3 heading body (the text after `### `).
/// Handles backticked tokens, `/`-separated multi-kind headings (e.g.
/// `` `file_starts_with` / `file_ends_with` ``), and strips a trailing
/// `(alias: ...)` group (an alias rides on its canonical kind's page).
fn kinds_in_heading(heading: &str) -> Vec<String> {
    let head = heading.split("(alias").next().unwrap_or(heading);
    let mut out = Vec::new();
    let mut rest = head;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else { break };
        let token = &after[..close];
        if !token.is_empty() && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            out.push(token.to_string());
        }
        rest = &after[close + 1..];
    }
    out
}

fn parse_families(md: &str) -> Vec<Family> {
    let mut families: Vec<Family> = Vec::new();
    let mut current: Option<usize> = None;
    for line in md.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            let title = title.trim();
            if META_FAMILIES.contains(&title) {
                current = None;
            } else {
                families.push(Family {
                    title: title.to_string(),
                    slug: slug(title),
                    kinds: Vec::new(),
                });
                current = Some(families.len() - 1);
            }
        } else if let Some(h3) = line.strip_prefix("### ") {
            if let Some(idx) = current {
                for k in kinds_in_heading(h3.trim()) {
                    if !families[idx].kinds.contains(&k) {
                        families[idx].kinds.push(k);
                    }
                }
            }
        }
    }
    families
}

fn render(families: &[Family]) -> String {
    let mut out = String::new();
    let total_kinds: usize = families.iter().map(|f| f.kinds.len()).sum();
    let _ = writeln!(
        out,
        "// GENERATED by `cargo run -p xtask -- gen-model`. Do not edit by hand."
    );
    let _ = writeln!(
        out,
        "// Source: docs/rules.md (## family / ### kind). `gen-model --check` gates it."
    );
    let _ = writeln!(out, "//");
    let _ = writeln!(
        out,
        "// The rule-kind taxonomy: {} built-in rule families and the {total_kinds} canonical",
        families.len()
    );
    let _ = writeln!(
        out,
        "// rule kinds within them (aliases ride on their canonical kind). Composes into"
    );
    let _ = writeln!(
        out,
        "// the LikeC4 workspace; the 'Rule catalogue' view + Mermaid export for GitHub."
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "specification {{");
    let _ = writeln!(out, "  element catalogue");
    let _ = writeln!(out, "  element family");
    let _ = writeln!(out, "  element ruleKind");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);
    let _ = writeln!(out, "model {{");
    let _ = writeln!(
        out,
        "  ruleCatalogue = catalogue 'Rule catalogue' 'alint built-in rule kinds, grouped by family' {{"
    );
    for f in families {
        let _ = writeln!(out, "    {} = family '{}' {{", f.slug, f.title);
        for k in &f.kinds {
            let _ = writeln!(out, "      {k} = ruleKind '{k}'");
        }
        let _ = writeln!(out, "    }}");
    }
    let _ = writeln!(out, "  }}");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);
    let _ = writeln!(out, "views {{");
    let _ = writeln!(out, "  view catalogueOverview of ruleCatalogue {{");
    let _ = writeln!(out, "    title 'Rule catalogue'");
    let _ = writeln!(out, "    include *");
    let _ = writeln!(out, "  }}");
    for f in families {
        let _ = writeln!(out);
        let _ = writeln!(out, "  view family_{} of {} {{", f.slug, f.slug);
        let _ = writeln!(out, "    title '{} rules'", f.title);
        let _ = writeln!(out, "    include *");
        let _ = writeln!(out, "  }}");
    }
    let _ = writeln!(out, "}}");
    out
}

/// The crate elements hand-declared in `alint.c4` must equal the
/// `cargo metadata` workspace-member set, so a crate can't be added or
/// removed without the architecture model noticing. Mirrors `gen-arch`'s
/// `workspace.dsl` crate-consistency gate, reusing its `cargo metadata`
/// extraction.
fn check_model_crate_set(root: &Path) -> Result<()> {
    let path = root.join(MODEL_C4);
    let c4 = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let declared: BTreeSet<String> = c4.lines().filter_map(crate_name_in_decl).collect();
    let members: BTreeSet<String> = crate::arch::workspace_crates(root)?
        .into_iter()
        .map(|c| c.name)
        .collect();
    if declared != members {
        let missing: Vec<&String> = members.difference(&declared).collect();
        let extra: Vec<&String> = declared.difference(&members).collect();
        bail!(
            "{MODEL_C4} crate elements drifted from `cargo metadata`. \
             missing (add to the model): {missing:?}; extra (remove or rename): {extra:?}."
        );
    }
    Ok(())
}

/// The crate name in a `<id> = crate 'name' '...'` declaration line, if any.
fn crate_name_in_decl(line: &str) -> Option<String> {
    let after = line.split("= crate ").nth(1)?.trim_start();
    let rest = after.strip_prefix('\'')?;
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

/// The Config entity's top-level `field` elements in `config-model.c4` must
/// equal the root `properties` of the JSON schema, so a top-level config key
/// added or renamed without updating the domain map fails CI. The rest of the
/// map (the entity relationships) is structural intent that `likec4 validate`
/// checks.
fn check_config_model_root_keys(root: &Path) -> Result<()> {
    let path = root.join(CONFIG_MODEL_C4);
    let c4 = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let declared: BTreeSet<String> = c4.lines().filter_map(field_name_in_decl).collect();

    let schema_path = root.join(SCHEMA_JSON);
    let schema: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&schema_path)
            .with_context(|| format!("read {}", schema_path.display()))?,
    )
    .with_context(|| format!("parse {}", schema_path.display()))?;
    let actual: BTreeSet<String> = schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .context("schema root has no `properties` object")?
        .keys()
        .cloned()
        .collect();

    if declared != actual {
        let missing: Vec<&String> = actual.difference(&declared).collect();
        let extra: Vec<&String> = declared.difference(&actual).collect();
        bail!(
            "{CONFIG_MODEL_C4} Config fields drifted from {SCHEMA_JSON} root properties. \
             missing (add a field): {missing:?}; extra (remove): {extra:?}."
        );
    }
    Ok(())
}

/// The field label in a `<id> = field 'name'` declaration line, if any.
fn field_name_in_decl(line: &str) -> Option<String> {
    let after = line.split("= field ").nth(1)?.trim_start();
    let rest = after.strip_prefix('\'')?;
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

/// The crate-to-crate runtime edges declared in `alint.c4` must equal the
/// `cargo metadata` runtime intra-workspace dependency edges, so the component
/// view can't drift from the manifests. `check_model_crate_set` checks the
/// nodes; this checks the edges (the gap that let `alint-lsp -> alint-rules` go
/// missing in the first cut).
fn check_model_crate_edges(root: &Path) -> Result<()> {
    let path = root.join(MODEL_C4);
    let c4 = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    // The dynamic views (the `views {` block) reuse crate ids in their flow
    // steps; only the `model {` block declares dependency edges.
    let model_block = c4.split("views {").next().unwrap_or(c4.as_str());

    let id_to_name: BTreeMap<String, String> =
        model_block.lines().filter_map(crate_decl_id_name).collect();

    let declared: BTreeSet<(String, String)> = model_block
        .lines()
        .filter_map(edge_endpoints)
        .filter_map(|(src, dst)| {
            Some((id_to_name.get(&src)?.clone(), id_to_name.get(&dst)?.clone()))
        })
        .collect();

    let crates = crate::arch::workspace_crates(root)?;
    let mut actual: BTreeSet<(String, String)> = BTreeSet::new();
    for c in &crates {
        for dep in &c.deps {
            actual.insert((c.name.clone(), dep.clone()));
        }
    }

    if declared != actual {
        let missing: Vec<&(String, String)> = actual.difference(&declared).collect();
        let extra: Vec<&(String, String)> = declared.difference(&actual).collect();
        bail!(
            "{MODEL_C4} crate runtime edges drifted from `cargo metadata`. \
             missing (add the edge): {missing:?}; extra (remove): {extra:?}."
        );
    }
    Ok(())
}

/// `(id, crate_name)` from a `<id> = crate 'name' '...'` declaration line.
fn crate_decl_id_name(line: &str) -> Option<(String, String)> {
    let (lhs, rhs) = line.split_once("= crate ")?;
    let id = lhs.trim();
    let rest = rhs.trim_start().strip_prefix('\'')?;
    let end = rest.find('\'')?;
    if id.is_empty() {
        return None;
    }
    Some((id.to_string(), rest[..end].to_string()))
}

/// `(source_id, target_id)` from a `<src> -> <dst> '...'` relationship line.
/// `None` for non-relationship lines (comments, declarations) and multi-token
/// sources.
fn edge_endpoints(line: &str) -> Option<(String, String)> {
    let (lhs, rhs) = line.split_once(" -> ")?;
    let src = lhs.trim();
    let dst = rhs.trim_start().split([' ', '\t', '\'']).next()?.trim();
    if src.is_empty() || dst.is_empty() || src.split_whitespace().count() != 1 {
        return None;
    }
    Some((src.to_string(), dst.to_string()))
}

/// Every rule kind registered in `all_kinds.yaml` must be documented in
/// `docs/rules.md`, either as a canonical kind (its own H3, hence in the
/// taxonomy) or as an alias of one. A registered-but-undocumented kind would
/// silently vanish from the taxonomy, and `docs-export` only warns, so this is
/// the hard gate.
fn check_taxonomy_complete(families: &[Family], md: &str, root: &Path) -> Result<()> {
    let mut documented: BTreeSet<String> = families
        .iter()
        .flat_map(|f| f.kinds.iter().cloned())
        .collect();
    for line in md.lines() {
        if let Some(h3) = line.strip_prefix("### ") {
            documented.extend(aliases_in_heading(h3.trim()));
        }
    }
    let registered = all_kinds_yaml_kinds(root)?;
    if documented != registered {
        let undocumented: Vec<&String> = registered.difference(&documented).collect();
        let unregistered: Vec<&String> = documented.difference(&registered).collect();
        bail!(
            "rule taxonomy is incomplete vs {ALL_KINDS_YAML}. \
             undocumented (add to docs/rules.md): {undocumented:?}; \
             documented-but-unregistered: {unregistered:?}."
        );
    }
    Ok(())
}

/// Distinct `kind:` values in `all_kinds.yaml` (mirrors the gen-facts extractor).
fn all_kinds_yaml_kinds(root: &Path) -> Result<BTreeSet<String>> {
    let path = root.join(ALL_KINDS_YAML);
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    Ok(text
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix("kind:"))
        .map(|v| v.trim().trim_end_matches(',').to_string())
        .filter(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .collect())
}

/// Alias kind names from the `(alias: `x`, `y`)` group of an H3 heading body.
fn aliases_in_heading(heading: &str) -> Vec<String> {
    let Some(idx) = heading.find("(alias") else {
        return Vec::new();
    };
    let group = heading[idx..].split(')').next().unwrap_or(&heading[idx..]);
    let mut out = Vec::new();
    let mut rest = group;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else { break };
        let tok = &after[..close];
        if !tok.is_empty() && tok.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            out.push(tok.to_string());
        }
        rest = &after[close + 1..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `rule-families.gen.c4` must be regenerated + committed when the rule
    /// catalogue changes; `--check` is what CI + preflight run.
    #[test]
    fn gen_model_check_passes_on_committed_tree() {
        run(true).expect("gen-model --check should pass on the committed tree");
    }

    #[test]
    fn kinds_in_heading_handles_aliases_and_pairs() {
        assert_eq!(kinds_in_heading("`file_exists`"), vec!["file_exists"]);
        assert_eq!(
            kinds_in_heading("`file_content_matches` (alias: `content_matches`)"),
            vec!["file_content_matches"]
        );
        assert_eq!(
            kinds_in_heading("`file_starts_with` / `file_ends_with`"),
            vec!["file_starts_with", "file_ends_with"]
        );
    }

    #[test]
    fn slug_is_identifier_safe() {
        assert_eq!(slug("Existence"), "existence");
        assert_eq!(slug("Security / Unicode sanity"), "security_unicode_sanity");
        assert_eq!(slug("Plugin (tier 1)"), "plugin_tier_1");
        assert_eq!(slug("Cross-file"), "cross_file");
    }

    /// The hand-authored architecture model declares exactly the workspace
    /// crate set; a crate added or removed without updating `alint.c4` fails.
    #[test]
    fn model_crate_set_matches_cargo_metadata() {
        let root = crate::workspace_root().expect("root");
        check_model_crate_set(&root).expect("alint.c4 crate set must match cargo metadata");
    }

    #[test]
    fn crate_name_in_decl_extracts_quoted_name() {
        assert_eq!(
            crate_name_in_decl("      core = crate 'alint-core' 'engine'").as_deref(),
            Some("alint-core")
        );
        assert_eq!(crate_name_in_decl("    cli = container 'CLI'"), None);
        assert_eq!(crate_name_in_decl("  dev -> alintBin 'runs'"), None);
    }

    /// The config-DSL domain map's Config keys stay in lockstep with the JSON
    /// schema's top-level properties.
    #[test]
    fn config_model_keys_match_schema() {
        let root = crate::workspace_root().expect("root");
        check_config_model_root_keys(&root)
            .expect("config-model.c4 Config fields must match schema root properties");
    }

    #[test]
    fn field_name_in_decl_extracts_quoted_name() {
        assert_eq!(
            field_name_in_decl("      f_version = field 'version'").as_deref(),
            Some("version")
        );
        assert_eq!(field_name_in_decl("    rule = entity 'RuleSpec'"), None);
    }

    /// The hand-authored component edges stay in lockstep with the cargo
    /// metadata runtime dependency edges (the gate that catches a missing edge).
    #[test]
    fn model_crate_edges_match_cargo_metadata() {
        let root = crate::workspace_root().expect("root");
        check_model_crate_edges(&root).expect("alint.c4 crate edges must match cargo metadata");
    }

    /// Every registered rule kind is documented (canonical or alias), so none
    /// can silently vanish from the taxonomy.
    #[test]
    fn taxonomy_is_complete_vs_all_kinds() {
        let root = crate::workspace_root().expect("root");
        let md = fs::read_to_string(root.join(RULES_MD)).expect("read rules.md");
        let families = parse_families(&md);
        check_taxonomy_complete(&families, &md, &root)
            .expect("taxonomy must cover every registered kind");
    }

    #[test]
    fn edge_endpoints_parses_relationships() {
        assert_eq!(
            edge_endpoints("  alintBin -> core 'drives the engine'"),
            Some(("alintBin".to_string(), "core".to_string()))
        );
        assert_eq!(edge_endpoints("    cli = container 'CLI'"), None);
        assert_eq!(edge_endpoints("  // a comment -> not an edge"), None);
    }

    #[test]
    fn aliases_in_heading_extracts_alias_group() {
        assert_eq!(
            aliases_in_heading("`file_content_matches` (alias: `content_matches`)"),
            vec!["content_matches"]
        );
        assert_eq!(aliases_in_heading("`file_exists`"), Vec::<String>::new());
    }
}
