//! `xtask docs-export` — render the alint.org docs bundle from
//! the workspace's source-of-truth files (CHANGELOG.md,
//! docs/rules.md, bundled ruleset YAMLs, the alint --help output,
//! and per-version benchmark JSONs).
//!
//! Output lives under `target/docs-bundle/` and is consumed by
//! the alint.org sync script.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::rule_options_table::{build_kind_branch_index, load_config_schema, options_section};
use crate::{build_release_binary, git_sha, now_iso, walkdir_plain, workspace_root};

// ---- docs-export ----------------------------------------------------------

/// Every `enum Command` variant in `crates/alint/src/main.rs` MUST
/// have an entry here. The 2026-05-30 audit found that five
/// subcommands (`init`, `suggest`, `export-agents-md`,
/// `validate-config`, `lsp`) had been added to the binary over time
/// without anyone bumping this list, leaving five
/// `/docs/cli/<sub>/` URLs as live 404s. The
/// `cli_reference_subcmds_match_command_enum` test below pins this
/// list against the enum so a new subcommand can't ship without
/// its docs page following.
///
/// Names are kebab-case to match `clap`'s default conversion of the
/// `PascalCase` enum variants (`ExportAgentsMd` -> `export-agents-md`).
pub(crate) const CLI_REFERENCE_SUBCMDS: &[&str] = &[
    "check",
    "fix",
    "baseline",
    "list",
    "explain",
    "facts",
    "init",
    "suggest",
    "export-agents-md",
    "validate-config",
    "lsp",
    "rules",
];

/// Workspace-relative paths the export reads from. Centralised so a
/// `git mv` of any of these is a one-liner here, not a hunt across
/// the function body.
mod docs_paths {
    pub const SITE_DIR: &str = "docs/site";
    pub const RULES_DOC: &str = "docs/rules.md";
    pub const ARCHITECTURE_DOC: &str = "docs/design/ARCHITECTURE.md";
    pub const ROADMAP_DOC: &str = "docs/design/ROADMAP.md";
    pub const RULE_AUTHORING_DOC: &str = "docs/development/rule-authoring.md";
    pub const CHANGELOG: &str = "CHANGELOG.md";
    pub const SCHEMA_JSON: &str = "schemas/v1/config.json";
    pub const FACTS_JSON: &str = "facts.json";
    pub const ROADMAP_JSON: &str = "roadmap.json";
    pub const CRATE_GRAPH_MD: &str = "docs/design/architecture/crate-graph.md";
    pub const MODEL_DIR: &str = "docs/design/architecture/model";
    pub const RULESETS_DIR: &str = "crates/alint-dsl/rulesets/v1";
}

/// Copy the `LikeC4` architecture model (`*.c4`) into the bundle so alint.org can
/// build the interactive web-component views and re-export Mermaid. Lands under
/// `architecture-model/` in the bundle; the sync routes non-markdown files to
/// `public/_alint/`.
fn copy_c4_model(workspace: &Path, target_dir: &Path) -> Result<()> {
    let src = workspace.join(docs_paths::MODEL_DIR);
    let dest = target_dir.join("architecture-model");
    fs::create_dir_all(&dest)?;
    let mut copied = 0;
    for entry in fs::read_dir(&src).with_context(|| format!("read_dir {}", src.display()))? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("c4") {
            let name = path.file_name().context("c4 path has a file name")?;
            fs::copy(&path, dest.join(name)).with_context(|| format!("copy {}", path.display()))?;
            copied += 1;
        }
    }
    if copied == 0 {
        bail!("no .c4 files found under {}", docs_paths::MODEL_DIR);
    }
    Ok(())
}

pub(crate) fn docs_export(
    out: Option<PathBuf>,
    check: bool,
    rules_only: bool,
    released_version: Option<&str>,
) -> Result<()> {
    let workspace = workspace_root()?;
    let target_dir = out.unwrap_or_else(|| workspace.join("target/docs-bundle"));

    // The released version the per-rule pages must not document ahead of.
    // Passed explicitly by the docs-bundle rule-page bridge (which runs from
    // a main worktree, so it can't read the release tag itself); None for a
    // local/dev export, where nothing is release-gated. See ADR-0007 /
    // docs/design/v0.14/documentation-drift.md P1.
    let released = released_version.map(crate::rule_options_table::parse_version);

    // In check mode we still produce the bundle (so all the
    // generators run) — just under a tempdir we discard. Catches
    // missing files / bad YAML / broken --help before merge.
    let _scratch_guard;
    let target_dir = if check {
        let scratch = tempfile::tempdir().context("creating tempdir for --check")?;
        let path = scratch.path().to_path_buf();
        _scratch_guard = scratch;
        path
    } else {
        // Clean previous output so removed pages don't linger.
        if target_dir.exists() {
            fs::remove_dir_all(&target_dir)
                .with_context(|| format!("removing stale {}", target_dir.display()))?;
        }
        fs::create_dir_all(&target_dir)?;
        target_dir
    };

    eprintln!("[xtask] docs-export → {}", target_dir.display());

    if rules_only {
        // The docs-bundle rule-page bridge overlays ONLY the per-rule
        // reference pages from main, so generate just those and skip the rest
        // of the export — most importantly step 5 (`generate_cli_reference`),
        // which builds the alint release binary. That redundant build is the
        // bulk of the bridge's cost, and the bridge never reads its output.
        generate_rules_pages(&workspace, &target_dir, released, false)?;
        eprintln!(
            "[xtask] docs-export --rules-only wrote {}/rules",
            target_dir.display()
        );
        return Ok(());
    }

    // 1. Hand-written long-form prose. Copied verbatim, except release-gated
    //    `<!-- alint:since=X -->` blocks are stripped from .md when a released
    //    version is set (matters for the main-overlaid docs/site/reference; P-REF).
    copy_site_tree(&workspace, &target_dir, released)?;

    // 2. Verbatim copies of the existing top-level docs.
    copy_one(
        &workspace.join(docs_paths::CHANGELOG),
        &target_dir.join("changelog.md"),
        Some("Changelog"),
    )?;
    copy_one(
        &workspace.join(docs_paths::ARCHITECTURE_DOC),
        &target_dir.join("about/architecture.md"),
        Some("Architecture"),
    )?;
    crate::roadmap_generator::generate_public_roadmap(
        &workspace.join(docs_paths::ROADMAP_DOC),
        &target_dir.join("about/roadmap.md"),
        "Roadmap",
    )?;
    copy_one(
        &workspace.join(docs_paths::RULE_AUTHORING_DOC),
        &target_dir.join("development/rule-authoring.md"),
        Some("Rule authoring"),
    )?;
    // Rule reference: slice docs/rules.md by H2 (= family) →
    // H3 (= rule kind) into one page per kind, plus per-family
    // overviews and a master alphabetical index. Returns a
    // kind → family-slug map used below to render kind names
    // as links from the bundled-ruleset pages.
    let kind_to_family = generate_rules_pages(&workspace, &target_dir, released, true)?;

    // 3. Per-bundled-ruleset reference page. `kind_to_family`
    //    drives the cross-links from `**kind**: <name>` →
    //    `/docs/rules/<family>/<name>/`.
    generate_bundled_ruleset_pages(&workspace, &target_dir, &kind_to_family)?;

    // 4. The JSON Schema, kept as JSON for programmatic use.
    let schema_dest = target_dir.join("configuration/schema.json");
    fs::create_dir_all(schema_dest.parent().unwrap())?;
    fs::copy(workspace.join(docs_paths::SCHEMA_JSON), &schema_dest)?;

    // 4b. The surface-area contract (`facts.json`), shipped at the
    //     bundle root next to `manifest.json` so alint.org can render
    //     counts/catalogues from it at a stable URL. Generated +
    //     gated in-repo by `xtask gen-facts`; see docs/design/facts-json.md.
    fs::copy(
        workspace.join(docs_paths::FACTS_JSON),
        target_dir.join("facts.json"),
    )
    .with_context(|| {
        format!(
            "copy {} (run `cargo run -p xtask -- gen-facts`)",
            docs_paths::FACTS_JSON
        )
    })?;

    // 4b-2. The public-roadmap contract (`roadmap.json`), shipped at the
    //     bundle root so alint.org's /roadmap/ timeline renders the phase
    //     list from it. Generated + gated in-repo by `xtask gen-roadmap`.
    //     Unlike facts.json (pinned to the release tag), docs-bundle.yml
    //     overlays this from main, so the published plan tracks main while
    //     the surface-area counts stay pinned to what users can install.
    fs::copy(
        workspace.join(docs_paths::ROADMAP_JSON),
        target_dir.join("roadmap.json"),
    )
    .with_context(|| {
        format!(
            "copy {} (run `cargo run -p xtask -- gen-roadmap`)",
            docs_paths::ROADMAP_JSON
        )
    })?;

    // 4c. The code-extracted crate dependency graph (Mermaid), shipped
    //     as a docs page (Starlight frontmatter injected via copy_one)
    //     so alint.org renders it. Generated + gated by `xtask gen-arch`;
    //     see docs/design/architecture-as-code.md.
    copy_one(
        &workspace.join(docs_paths::CRATE_GRAPH_MD),
        &target_dir.join("about/crate-graph.md"),
        Some("Crate dependency graph"),
    )?;

    // 4d. The LikeC4 architecture model (*.c4 source). Shipped so alint.org can
    //     build the interactive system + flow views (LikeC4 web component) and
    //     re-export Mermaid. Non-markdown, so the sync routes it to
    //     public/_alint/architecture-model/. Hand-authored alint.c4 + generated
    //     *.gen.c4 (gen-model); validated by ci/scripts/likec4.sh.
    copy_c4_model(&workspace, &target_dir)?;

    // 5. CLI reference, captured from the alint binary's --help.
    generate_cli_reference(&workspace, &target_dir)?;

    // 6. Benchmark trajectory JSON. Re-renders the cross-version
    //    headline table from the per-version results.json files
    //    under `docs/benchmarks/macro/results/<arch>/` plus the
    //    CHANGELOG headlines. Consumed by alint.org's /benchmarks/
    //    page so the trajectory table refreshes on every main push
    //    instead of drifting until a maintainer hand-edits HTML.
    generate_benchmarks_trajectory(&workspace, &target_dir)?;

    // 7. Manifest. Any consumer (alint.org sync script, audit
    //    tooling) reads this to know what's in the bundle.
    write_manifest(&target_dir)?;

    if check {
        crate::family_index::check_ascii(&target_dir)?;
        crate::docs_checks::check_titles_no_backticks(&target_dir)?;
        eprintln!("[xtask] docs-export --check OK");
    } else {
        eprintln!("[xtask] docs-export wrote {}", target_dir.display());
    }
    Ok(())
}

/// Recursively copy `docs/site/**.md` into the bundle root. Mirror
/// the directory layout exactly — `docs/site/getting-started/foo.md`
/// → `docs-bundle/getting-started/foo.md`.
fn copy_site_tree(
    workspace: &Path,
    target_dir: &Path,
    released: Option<crate::rule_options_table::Version>,
) -> Result<()> {
    let site_root = workspace.join(docs_paths::SITE_DIR);
    if !site_root.is_dir() {
        bail!(
            "{} is missing — Phase 2 expects hand-written docs to live here",
            site_root.display()
        );
    }
    for entry in walkdir_plain(&site_root)? {
        let md = fs::metadata(&entry)?;
        if !md.is_file() {
            continue;
        }
        let rel = entry.strip_prefix(&site_root).unwrap();
        let dest = target_dir.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let is_markdown = entry
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e == "md" || e == "mdx");
        if released.is_some() && is_markdown {
            // Release-gate hand-written docs the same way the rule pages are:
            // strip `<!-- alint:since=X -->` blocks newer than the released
            // version. This matters for docs/site/reference/**, which the
            // docs-bundle overlays from main (the one leak vector among the
            // otherwise tag-pinned docs/site tree). A no-op for the unmarked
            // majority (the stripper fast-paths files with no marker). See
            // ADR-0007 / docs/design/v0.14/documentation-drift.md P-REF.
            let body =
                fs::read_to_string(&entry).with_context(|| format!("read {}", entry.display()))?;
            fs::write(&dest, strip_unreleased_prose(&body, released))
                .with_context(|| format!("writing {}", dest.display()))?;
        } else {
            fs::copy(&entry, &dest)
                .with_context(|| format!("copying {} → {}", entry.display(), dest.display()))?;
        }
    }
    Ok(())
}

/// Copy one source file into the bundle. If `title` is `Some`,
/// inject a Starlight frontmatter block at the top of the
/// destination so the page renders with the desired title in the
/// Starlight chrome (the source files don't carry their own
/// frontmatter — they're project-internal docs).
fn copy_one(src: &Path, dest: &Path, title: Option<&str>) -> Result<()> {
    if !src.is_file() {
        bail!("expected file at {}", src.display());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(title) = title {
        let body = fs::read_to_string(src).with_context(|| format!("reading {}", src.display()))?;
        let stripped = strip_first_h1(&body);
        let mut out = String::new();
        let _ = writeln!(&mut out, "---");
        let _ = writeln!(&mut out, "title: {title}");
        let _ = writeln!(&mut out, "---");
        let _ = writeln!(&mut out);
        out.push_str(stripped);
        fs::write(dest, out).with_context(|| format!("writing {}", dest.display()))?;
    } else {
        fs::copy(src, dest)
            .with_context(|| format!("copying {} → {}", src.display(), dest.display()))?;
    }
    Ok(())
}

/// Strip the first top-level `# heading` line so the Starlight
/// frontmatter `title` we inject doesn't render *next to* a
/// duplicate H1 from the source file.
fn strip_first_h1(body: &str) -> &str {
    let trimmed = body.trim_start();
    if let Some(rest) = trimmed.strip_prefix("# ") {
        // Skip until end-of-line + the trailing newline.
        if let Some(idx) = rest.find('\n') {
            return rest[idx + 1..].trim_start_matches('\n');
        }
        return "";
    }
    body
}

/// Per-rule-kind pages from `docs/rules.md`.
///
/// rules.md is structured H2 (family) → H3 (one heading per
/// rule kind, sometimes covering paired/triplet kinds via a
/// slash- or comma-separated list of backtick'd names). We
/// slice into:
/// - `rules/<family-slug>/<kind>.md` — one Starlight page per
///   rule kind. Multi-kind H3s emit one page per kind; the
///   pages share the H3 body and cross-link via "See also".
/// - `rules/<family-slug>/index.md` — family overview with
///   one-line summaries linking to each kind page.
/// - `rules/index.md` — alphabetical master index of every
///   kind shipped in this build.
///
/// Two H2 sections are special-cased out of the rules tree
/// because they're concepts rather than rule kinds:
/// - "Fix operations" → `concepts/fix-operations.md`
/// - "Nested .alint.yml (monorepo layering)" →
///   `concepts/nested-configs.md`
///
/// Sections we drop entirely:
/// - "Contents" (the source's TOC; redundant with our generated
///   index)
/// - "Bundled rulesets" (per-ruleset pages already generated
///   from the YAML bodies)
///
/// Returns a `kind → family-slug` map so the bundled-ruleset
/// generator can produce links like
/// `[json_path_equals](/docs/rules/content/json_path_equals/)`.
fn generate_rules_pages(
    workspace: &Path,
    target_dir: &Path,
    released: Option<crate::rule_options_table::Version>,
    capture_output: bool,
) -> Result<std::collections::HashMap<String, String>> {
    use std::collections::{HashMap, HashSet};

    let src = fs::read_to_string(workspace.join(docs_paths::RULES_DOC))
        .with_context(|| format!("reading {}", docs_paths::RULES_DOC))?;

    // Authoritative list of rule kinds from the registry. We
    // cross-check against this so a typo in rules.md surfaces
    // at export time, not at site render time.
    let registry = alint_rules::builtin_registry();
    let known_kinds: HashSet<String> = registry.known_kinds().map(str::to_string).collect();

    // Alias names declared in rules.md H3 titles via `(alias: \`X\`)`. The
    // registry now records alias->canonical too (`canonical_kind`), and the
    // gen-facts gate cross-checks the two agree; we harvest from rules.md here so
    // the "registered but missing" page check validates the documentation's own
    // alias declarations. Excludes aliases that ARE documented under their
    // canonical name's heading so that check doesn't false-positive on them.
    let aliases: HashSet<String> = harvest_aliases(&src);

    // Source of truth for the per-rule "## Options" tables: the
    // type-derived JSON Schema. Loaded once; indexed by every kind
    // spelling so aliases resolve to their canonical branch.
    let schema = load_config_schema(&workspace.join(docs_paths::SCHEMA_JSON))?;
    let kind_branches = build_kind_branch_index(&schema);

    // ADR-0014: scenarios carrying a `docs:` block render their kind's worked
    // example from the real fixture (tree + config + a live `alint check` run).
    // Empty until a kind opts in, so every other kind keeps its hand-written
    // rules.md example.
    let documented = examples::render_documented(workspace, capture_output)?;

    let rules_dir = target_dir.join("rules");
    fs::create_dir_all(&rules_dir)?;

    let mut kind_to_family: HashMap<String, String> = HashMap::new();
    let mut all_kinds: Vec<KindEntry> = Vec::new();
    let mut family_summaries: Vec<FamilySummary> = Vec::new();
    // (title, display order, slug) per family, for the second-pass family Overview
    // render (categories-based membership needs all_kinds complete first).
    let mut families_meta: Vec<(String, u32, String)> = Vec::new();
    // Per-rule H3 sections in `docs/rules.md` must contain a
    // ```yaml usage example. Accumulated across all families and
    // surfaced as a single hard failure so a docs PR sees every
    // missing example at once instead of fixing them one at a
    // time. Reflected on alint.org/docs/rules/<family>/<kind>/.
    let mut missing_examples: Vec<String> = Vec::new();
    // Rule examples whose top-level `kind:` is an alias (or otherwise not the
    // H3's canonical kind). The page is slugged by the canonical name, so the
    // example must match it. Accumulated + hard-failed alongside missing_examples.
    let mut wrong_kind_examples: Vec<String> = Vec::new();

    let mut family_order: u32 = 0;
    for h2 in split_h2_sections(&src) {
        let lc = h2.title.to_lowercase();
        if lc == "contents" || lc.starts_with("bundled rulesets") {
            continue;
        }
        if lc.starts_with("fix operations") {
            emit_concept_page(target_dir, "fix-operations", "Fix operations", &h2.body)?;
            continue;
        }
        if lc.starts_with("nested") {
            emit_concept_page(
                target_dir,
                "nested-configs",
                "Nested .alint.yml (monorepo layering)",
                &h2.body,
            )?;
            continue;
        }

        family_order += 1;
        let family_slug = slugify(&h2.title);
        let family_dir = rules_dir.join(&family_slug);
        fs::create_dir_all(&family_dir)?;

        // Populate all_kinds (with per-kind categories) + per-rule pages. The
        // family Overview pages are rendered in a second pass below, once every
        // kind's categories are known (categories-based membership).
        process_family_h3s(
            &h2,
            &family_dir,
            &family_slug,
            &known_kinds,
            &schema,
            &kind_branches,
            &mut kind_to_family,
            &mut all_kinds,
            &mut missing_examples,
            &mut wrong_kind_examples,
            &documented,
            &registry,
            released,
        )?;
        families_meta.push((h2.title.clone(), family_order, family_slug.clone()));
    }

    // Hard-fail on any registered kind that rules.md doesn't document. A new
    // kind must ship a reference page — docs-export can't generate one for an
    // undocumented kind, so it would silently ship page-less. Promoted from a
    // soft WARN to match the missing-example gate below. Aliases (declared
    // inline in their canonical H3's `(alias: …)`) are exempt. Sorted for a
    // deterministic message (`known_kinds` is a `HashSet`).
    let mut undocumented: Vec<&str> = known_kinds
        .iter()
        .filter(|k| !kind_to_family.contains_key(*k) && !aliases.contains(*k))
        .map(String::as_str)
        .collect();
    undocumented.sort_unstable();
    if !undocumented.is_empty() {
        anyhow::bail!(
            "{} registered rule kind(s) missing from docs/rules.md:\n  - {}\n\n\
             Add an H3 section (with a ```yaml example) under the right family \
             heading in docs/rules.md, or declare it as an alias on its \
             canonical rule's H3 `(alias: …)`.",
            undocumented.len(),
            undocumented.join("\n  - "),
        );
    }

    // Hard-fail the two per-rule-example docs gates (a missing yaml example, and
    // an alias / non-canonical example `kind:`). Enforced here, not as soft
    // warnings, so a regressing PR fails the docs-bundle build before it can
    // publish a broken example to alint.org.
    enforce_example_gates(&missing_examples, &wrong_kind_examples)?;
    enforce_documented_page_targets(&documented, &kind_to_family)?;

    // Family Overview pages: categories-based membership. Each family lists every
    // kind whose `**Categories:**` line includes it (by slug), not just the kinds
    // physically under its H2. At single-membership this equals the directory
    // membership; at Phase-3 multi-membership it cross-lists automatically.
    for (title, order, slug) in &families_meta {
        let rules: Vec<RuleEntry> = all_kinds
            .iter()
            .filter(|k| k.categories.iter().any(|c| c == slug))
            .map(|k| RuleEntry {
                kind: k.kind.clone(),
                summary: k.summary.clone(),
                family_slug: k.family_slug.clone(),
            })
            .collect();
        emit_family_index(&rules_dir.join(slug), title, *order, &rules)?;
        family_summaries.push(FamilySummary {
            title: title.clone(),
            slug: slug.clone(),
            rule_count: rules.len(),
        });
    }

    emit_rules_master_index(&rules_dir, &all_kinds, &family_summaries, aliases.len())?;
    Ok(kind_to_family)
}

/// Walk every H3 in a family, emit per-rule pages, and collect
/// the family's rule list for later index generation. Split out
/// of `generate_rules_pages` because clippy's `too_many_lines`
/// flagged the original — and even ignoring that, "process one
/// family" is its own logical chunk worth naming.
// Threads the read-only registry/schema context plus the four
// accumulators through one family's H3 sections; bundling these into
// a struct would obscure more than it clarifies.
#[allow(clippy::too_many_arguments)]
fn process_family_h3s(
    h2: &H2Section,
    family_dir: &Path,
    family_slug: &str,
    known_kinds: &std::collections::HashSet<String>,
    schema: &serde_json::Value,
    kind_branches: &std::collections::HashMap<String, serde_json::Value>,
    kind_to_family: &mut std::collections::HashMap<String, String>,
    all_kinds: &mut Vec<KindEntry>,
    missing_examples: &mut Vec<String>,
    wrong_kind_examples: &mut Vec<String>,
    documented: &std::collections::BTreeMap<String, Vec<examples::RenderedExample>>,
    registry: &alint_core::RuleRegistry,
    released: Option<crate::rule_options_table::Version>,
) -> Result<()> {
    let mut kind_order: u32 = 0;
    for h3 in split_h3_sections(&h2.body) {
        let mut group_kinds = extract_kinds(&h3.title);
        group_kinds.retain(|k| {
            if known_kinds.contains(k) {
                true
            } else {
                eprintln!(
                    "[xtask] WARN: rules.md heading '{}' mentions unknown rule kind '{}' — skipping",
                    h3.title, k
                );
                false
            }
        });
        if group_kinds.is_empty() {
            continue;
        }
        // Every per-rule H3 must include at least one fenced
        // ```yaml block. Surfaced collectively at the end of
        // generate_rules_pages so authors see all gaps in one
        // pass. Multi-kind headings (e.g. the structured-query
        // family's three path_equals kinds) share one body, so
        // one example per heading covers the group.
        // A kind rendered from a documented `docs:` scenario (ADR-0014) needs
        // no hand-written ```yaml block - its example is generated. Exempt an
        // H3 only when every kind it documents is covered that way.
        if !h3.body.contains("```yaml") && !group_kinds.iter().all(|k| documented.contains_key(k)) {
            missing_examples.push(format!("{} → {}", h2.title, h3.title));
        }
        // ...and a documented kind must NOT keep a hand-written CONFIG example
        // (a ```yaml block whose top-level `kind:` is the documented kind), or
        // the page double-renders the generated example alongside a stale
        // hand-written one. A non-config yaml block (e.g. a CI-workflow recipe)
        // is fine. Scan EVERY yaml block, not just the leading one: a documented
        // kind can keep a non-config recipe as its first block (git_commit_message
        // keeps a CI-workflow yaml), so a stale config re-added after it would sit
        // in a later block. Match against `documented` kind names (which are
        // canonical) after canonicalising each block kind, so a stale config using
        // the ALIAS spelling (e.g. `kind: header` under a documented `file_header`)
        // is caught too, and an incidental `kind:` in a recipe can't false-fire.
        // Atomic-swap (ADR-0014).
        if let Some(ex_kind) = example_block_kinds(&h3.body)
            .into_iter()
            .find(|k| documented.contains_key(registry.canonical_kind(k)))
        {
            anyhow::bail!(
                "docs/rules.md H3 '{}' documents `{}` via a `docs:` scenario but still \
                 contains a hand-written config example (`kind: {ex_kind}`) for it - remove \
                 the hand-written block; the example now renders from the fixture.",
                h3.title,
                registry.canonical_kind(&ex_kind),
            );
        }
        // The example must demonstrate the H3's CANONICAL kind, not an alias:
        // the generated page is slugged/titled by the canonical name, so an
        // alias in the example (e.g. `kind: header` under `file_header`) reads
        // as a mismatch. extract_kinds() returns only canonical kinds, so a
        // top-level example `kind:` outside `group_kinds` is an alias (or wrong).
        if let Some(ex_kind) = example_first_kind(&h3.body) {
            if !group_kinds.contains(&ex_kind) {
                wrong_kind_examples.push(format!(
                    "{} → {}: example uses `kind: {}`, but this H3 documents `{}`; \
                     use the canonical name (the alias still works in user configs)",
                    h2.title,
                    h3.title,
                    ex_kind,
                    group_kinds.join("` / `"),
                ));
            }
        }
        // Strip the `**Categories:**` association line (if any) from the body
        // BEFORE summarizing or rendering, so it never becomes the summary / SEO
        // description or double-renders on the page. Categories surface via
        // frontmatter + the cross-link block. Behavior-neutral until the lines
        // land in rules.md. See `categories_line` + docs/design/rule-categories.md.
        let (categories_content, clean_body) =
            crate::categories_line::split_categories_line(&h3.body);
        // Parse the association line into URL slugs for the page frontmatter
        // (the per-kind "Categories" cross-link block reads these). All kinds in
        // a multi-kind H3 share the line.
        let category_slugs: Vec<&str> = categories_content
            .as_deref()
            .map(|c| {
                c.split(',')
                    .filter_map(|t| alint_core::Category::from_title(t.trim()))
                    .map(alint_core::Category::slug)
                    .collect()
            })
            .unwrap_or_default();
        let summary = first_sentence(&clean_body);
        // Release-gate the prose the same way the Options table is gated:
        // drop `<!-- alint:since=X -->` blocks describing capability newer than
        // the released version. Computed once per H3 (a multi-kind heading's
        // siblings share it). See ADR-0007 / documentation-drift.md P1.
        let page_body = strip_unreleased_prose(&clean_body, released);
        for kind in &group_kinds {
            kind_order += 1;
            let siblings: Vec<&str> = group_kinds
                .iter()
                .filter(|k| *k != kind)
                .map(String::as_str)
                .collect();
            let options_md = kind_branches
                .get(kind)
                .map(|branch| options_section(branch, schema, released));
            emit_rule_page(
                family_dir,
                kind,
                family_slug,
                &h2.title,
                &page_body,
                &siblings,
                options_md.as_deref(),
                kind_order,
                &category_slugs,
                documented.get(kind).map(Vec::as_slice),
            )?;
            kind_to_family.insert(kind.clone(), family_slug.to_string());
            all_kinds.push(KindEntry {
                kind: kind.clone(),
                family_title: h2.title.clone(),
                family_slug: family_slug.to_string(),
                summary: summary.clone(),
                categories: category_slugs.iter().copied().map(String::from).collect(),
            });
        }
    }
    Ok(())
}

#[derive(Clone)]
pub(crate) struct RuleEntry {
    pub(crate) kind: String,
    pub(crate) summary: String,
    /// The kind's PRIMARY (canonical) family slug, which owns its page URL. A
    /// family Overview links each kind to `/docs/rules/<this>/<kind>/`, so a
    /// secondary family's page still links to the one canonical page rather than
    /// a non-existent `/docs/rules/<secondary>/<kind>/` (would 404 at Phase 3).
    pub(crate) family_slug: String,
}

#[derive(Clone)]
struct KindEntry {
    kind: String,
    family_title: String,
    family_slug: String,
    summary: String,
    /// URL slugs of every category the kind belongs to (primary first).
    categories: Vec<String>,
}

struct FamilySummary {
    title: String,
    slug: String,
    rule_count: usize,
}

/// Sections of a markdown document split at H3 headers (`### …`).
/// Used inside an H2 body. Anything before the first H3 is
/// dropped (it's typically a family-level intro paragraph that
/// belongs on the family index, not on any rule's page).
struct H3Section {
    title: String,
    body: String,
}

fn split_h3_sections(src: &str) -> Vec<H3Section> {
    let mut sections: Vec<H3Section> = Vec::new();
    let mut current: Option<H3Section> = None;
    for line in src.lines() {
        if let Some(rest) = line.strip_prefix("### ") {
            if let Some(prev) = current.take() {
                sections.push(prev);
            }
            current = Some(H3Section {
                title: rest.trim().to_string(),
                body: String::new(),
            });
        } else if let Some(sec) = current.as_mut() {
            sec.body.push_str(line);
            sec.body.push('\n');
        }
    }
    if let Some(prev) = current.take() {
        sections.push(prev);
    }
    sections
}

/// Extract rule-kind tokens from an H3 title. Each kind name in
/// the heading is wrapped in single backticks; aliases live
/// inside `(alias: ...)` parens. Strip the parens, then collect
/// every backtick-delimited token that looks like a rule kind.
///
/// A multi-kind heading (the structured-query family's three
/// path-equals or path-matches kinds, comma-separated and
/// individually backticked) yields one kind per backtick'd
/// token. A single-kind heading yields one. Alias declarations
/// inside parens are skipped here and harvested separately by
/// [`harvest_aliases`].
fn extract_kinds(h3_title: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut paren_depth = 0i32;
    let mut in_backtick = false;
    let mut current = String::new();
    for ch in h3_title.chars() {
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = (paren_depth - 1).max(0),
            '`' if paren_depth == 0 => {
                if in_backtick {
                    if looks_like_kind(&current) {
                        out.push(current.clone());
                    }
                    current.clear();
                }
                in_backtick = !in_backtick;
            }
            c if in_backtick && paren_depth == 0 => current.push(c),
            _ => {}
        }
    }
    out
}

fn looks_like_kind(s: &str) -> bool {
    !s.is_empty()
        && s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Scan the entire rules.md source for the alias declarations
/// `(alias: ...)` (each name in single backticks) and collect the
/// alias names. Used to suppress "registered but missing"
/// warnings for aliases that share their canonical rule's page.
fn harvest_aliases(src: &str) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let needle = "alias:";
    let mut idx = 0;
    while let Some(pos) = src[idx..].find(needle) {
        let abs = idx + pos + needle.len();
        // After "alias:", skip whitespace, then expect a backtick-
        // delimited identifier. Multiple aliases per H3 aren't
        // currently used in rules.md but we'll handle them anyway.
        let mut cursor = abs;
        let bytes = src.as_bytes();
        while cursor < bytes.len() && (bytes[cursor] as char).is_whitespace() {
            cursor += 1;
        }
        if cursor < bytes.len() && bytes[cursor] == b'`' {
            cursor += 1;
            let start = cursor;
            while cursor < bytes.len() && bytes[cursor] != b'`' && bytes[cursor] != b'\n' {
                cursor += 1;
            }
            if cursor < bytes.len() && bytes[cursor] == b'`' {
                let name = &src[start..cursor];
                if looks_like_kind(name) {
                    out.insert(name.to_string());
                }
            }
        }
        idx = abs;
    }
    out
}

/// Heuristic one-liner for sidebar / index summaries. Takes the
/// first markdown paragraph of an H3 body, strips trailing
/// whitespace, takes up to the first sentence-ending `.`. Skips
/// blank lines / fenced code at the top.
fn first_sentence(body: &str) -> String {
    let mut paragraph = String::new();
    let mut in_code_block = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }
        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(trimmed);
    }
    if let Some(idx) = paragraph.find(". ") {
        paragraph.truncate(idx + 1);
    }
    paragraph.trim().to_string()
}

/// Terminal-ready one-line summary of a rule kind's `docs/rules.md` prose, for
/// the in-binary docs bridge (ADR-0011). Unlike [`first_sentence`] (which feeds
/// the website and leaves markdown intact), this yields clean plaintext: the
/// opening sentence with abbreviation-aware splitting, `**bold**` / `*italic*`
/// and inline backticks stripped, em/en-dashes folded and arrows ASCII-ized,
/// whitespace collapsed, a colon/comma lead-in trimmed, and length-capped with an
/// honest `...` marker. Input is the `**Categories:**`-stripped H3 body.
pub(crate) fn kind_summary(clean_body: &str, max_chars: usize) -> String {
    let sentence = strip_markup(&guarded_first_sentence(clean_body));
    let cleaned = meta_desc_clean(&sentence, usize::MAX);
    // A colon / comma / semicolon lead-in (a sentence that introduces a list) is
    // not a self-contained one-liner; drop the trailing punctuation.
    let cleaned = cleaned.trim_end_matches([':', ';', ',', ' ']);
    if cleaned.chars().count() <= max_chars {
        return cleaned.to_string();
    }
    // Longer than one terminal line: word-cap it (leaving room for the marker),
    // drop a dangling open quote the cap may have created plus any trailing
    // punctuation, and end with an ASCII `...` so the truncation is honest.
    let capped = meta_desc_clean(cleaned, max_chars.saturating_sub(3));
    let capped: &str = match (capped.matches('"').count() % 2, capped.rfind('"')) {
        (1, Some(q)) => &capped[..q],
        _ => capped.as_str(),
    };
    let trimmed = capped.trim_end_matches([',', ';', ':', ' ', '(', '"', '\'']);
    format!("{trimmed}...")
}

/// Strip markdown emphasis (`**bold**`, `*italic*`) and fold non-ASCII arrows to
/// ASCII, so the terminal summary is plain text. A bare, unbalanced `*` (e.g. the
/// `*_equals` wildcard family) is left intact.
fn strip_markup(s: &str) -> String {
    static ITALIC: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\*([^*\n]+)\*").unwrap());
    ITALIC
        .replace_all(&s.replace("**", ""), "$1")
        .replace('→', "->")
        .replace('⇒', "=>")
        .replace('←', "<-")
        .replace('⇐', "<=")
        .replace('↔', "<->")
}

/// Like the paragraph-gather in [`first_sentence`], but ends the sentence at the
/// first `". "` whose preceding word is NOT a known abbreviation (`e.g.`, `i.e.`,
/// `vs.`, ...). Without this a clause like "differ only by case (e.g. ...)" is cut
/// at the "g." (`no_case_conflicts`); a decimal like "v1.2." is not mistaken for
/// an abbreviation and correctly ends the sentence.
fn guarded_first_sentence(body: &str) -> String {
    const ABBREV: &[&str] = &[
        "e.g", "i.e", "vs", "cf", "etc", "al", "resp", "approx", "fig", "no",
    ];
    let mut paragraph = String::new();
    let mut in_code_block = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }
        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(trimmed);
    }
    let mut cut = paragraph.len();
    let mut from = 0;
    while let Some(rel) = paragraph[from..].find(". ") {
        let dot = from + rel;
        // The word ending at this period: from the last space/`(` before it.
        let start = paragraph[..dot].rfind([' ', '(']).map_or(0, |i| i + 1);
        let word = paragraph[start..dot].to_ascii_lowercase();
        if !ABBREV.contains(&word.as_str()) {
            cut = dot + 1;
            break;
        }
        from = dot + 2;
    }
    paragraph[..cut].trim().to_string()
}

/// SERP `<meta description>` line normaliser. Strips markdown
/// inline backticks (they read as literal grave accents in a
/// search snippet), collapses whitespace, removes em/en dashes
/// (the marketing style guide bans them — substitute a period or
/// keep the clause as-is), and hard-caps the length so Google
/// doesn't truncate mid-word. Sentence-aware: if the cap lands
/// inside a sentence, back off to the previous sentence end so the
/// snippet never trails an ellipsis.
pub(crate) fn meta_desc_clean(raw: &str, max_chars: usize) -> String {
    // Drop inline-code backticks, collapse all whitespace runs.
    let despaced: String = raw.replace('`', "");
    let mut s = despaced.split_whitespace().collect::<Vec<_>>().join(" ");
    // The style guide bans em/en dashes in SERP copy. A dash
    // joining two clauses reads cleanly as a comma (keeps it one
    // clause, no broken mid-sentence capitalisation that a period
    // would introduce). A bare em/en dash with no surrounding
    // spaces collapses to a space. Intra-word hyphens stay.
    s = s.replace(" — ", ", ").replace(" – ", ", ");
    s = s.replace(" -- ", ", ");
    s = s.replace(['—', '–'], " ");
    // Tidy any ", ." / ".," artifacts the substitution can leave
    // when a dash sat next to existing punctuation.
    s = s.replace(", .", ".").replace(". ,", ".").replace(" ,", ",");
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= max_chars {
        return s;
    }
    // Over the cap: prefer cutting at the last sentence end that
    // still fits; else cut at the last word boundary that fits.
    let truncated: String = s.chars().take(max_chars).collect();
    if let Some(idx) = truncated.rfind(". ") {
        return truncated[..=idx].trim().to_string();
    }
    if let Some(idx) = truncated.rfind(' ') {
        return truncated[..idx].trim().to_string();
    }
    truncated.trim().to_string()
}

/// Build the per-rule SERP description: lead with what the rule
/// actually checks (its own first sentence — the unique value
/// prop), keep the rule kind (the search query) and family
/// (disambiguator) in the string, cap ~155 chars, no em-dashes.
/// Falls back to a family-scoped sentence when the rule body's
/// opening line is too terse to stand alone.
fn rule_meta_description(kind: &str, family_title: &str, body: &str) -> String {
    let summary = meta_desc_clean(&first_sentence(body), 140);
    let family = family_title.to_lowercase();
    let composed = if summary.len() < 25 {
        // Doc-comment opener too thin to be a useful snippet —
        // fall back to a kind + family clause (still concrete:
        // names the rule the searcher typed and where it lives).
        format!("{kind} rule in alint's {family} family.")
    } else if summary.ends_with('.') {
        format!("{summary} alint {kind} rule, {family} family.")
    } else {
        format!("{summary}. alint {kind} rule, {family} family.")
    };
    meta_desc_clean(&composed, 158)
}

/// Render one `rules/<family>/<kind>.md` page. Frontmatter
/// `title` is the bare kind name so URLs and Starlight headings
/// match what the user types in `.alint.yml`. The page body is
/// the H3's content plus a "See also" footer for paired rules.
/// A multi-kind H3 (the structured-query family's `*_path_equals` /
/// `*_path_matches` groups) shares one example body across every kind's
/// page — a single block with one rule per kind. Without help, all four
/// pages lead with the first kind's rule (`json_path_equals`), so they
/// read like un-edited clones. Bring the rule whose `kind:` matches this
/// page to the front of the first fenced block so each page leads with
/// its own kind. No-op for single-rule examples, when the page's kind
/// isn't a standalone entry, or when it's already first.
fn lead_example_with_kind(body: &str, kind: &str) -> String {
    let Some(open) = body.find("```yaml") else {
        return body.to_string();
    };
    let after_fence = open + "```yaml".len();
    let Some(nl) = body[after_fence..].find('\n') else {
        return body.to_string();
    };
    let content_start = after_fence + nl + 1;
    let Some(close_rel) = body[content_start..].find("```") else {
        return body.to_string();
    };
    let close = content_start + close_rel;

    // Rule entries are separated by blank lines.
    let entries: Vec<&str> = body[content_start..close].split("\n\n").collect();
    if entries.len() < 2 {
        return body.to_string();
    }
    let kind_line = format!("kind: {kind}");
    let Some(pos) = entries
        .iter()
        .position(|e| e.lines().any(|l| l.trim() == kind_line))
    else {
        return body.to_string();
    };
    if pos == 0 {
        return body.to_string();
    }
    let mut reordered = entries.clone();
    let chosen = reordered.remove(pos);
    reordered.insert(0, chosen);
    format!(
        "{}{}{}",
        &body[..content_start],
        reordered.join("\n\n"),
        &body[close..]
    )
}

/// The `kind:` value of the first rule entry in a rule H3's leading fenced
/// yaml example, if any. Used to assert the example demonstrates the H3's
/// canonical kind rather than an alias (the page is slugged/titled by the
/// canonical name, so an alias in the example reads as a mismatch). Returns
/// None when the H3 has no yaml example block or the block carries no
/// top-level `kind:` line.
fn example_first_kind(body: &str) -> Option<String> {
    let open = body.find("```yaml")?;
    let after_fence = open + "```yaml".len();
    let nl = body[after_fence..].find('\n')?;
    let content_start = after_fence + nl + 1;
    let close_rel = body[content_start..].find("```")?;
    for line in body[content_start..content_start + close_rel].lines() {
        if let Some(v) = line.trim().strip_prefix("kind:") {
            return Some(v.trim().to_string());
        }
    }
    None
}

/// The first top-level `kind:` of EACH fenced yaml block in a rule H3's body, in
/// document order (a block with no `kind:` contributes nothing). Unlike
/// `example_first_kind` (which reads only the leading block, to police that
/// block's canonical kind), the double-example gate must see a stale config block
/// wherever it sits - e.g. a later block after a non-config recipe. Callers
/// filter the result against the documented-kind set, so an incidental `kind:`
/// in a recipe can't false-fire.
fn example_block_kinds(body: &str) -> Vec<String> {
    let mut kinds = Vec::new();
    let mut offset = 0;
    while let Some(rel_open) = body[offset..].find("```yaml") {
        let after_fence = offset + rel_open + "```yaml".len();
        let Some(nl) = body[after_fence..].find('\n') else {
            break;
        };
        let content_start = after_fence + nl + 1;
        let Some(close_rel) = body[content_start..].find("```") else {
            break;
        };
        let block_end = content_start + close_rel;
        for line in body[content_start..block_end].lines() {
            if let Some(v) = line.trim().strip_prefix("kind:") {
                kinds.push(v.trim().to_string());
                break; // the FIRST kind: of this block only
            }
        }
        offset = block_end + 3; // step past the closing fence
    }
    kinds
}

/// Enforce the two per-rule-example docs gates and bail on the first failure:
/// `missing` = H3 sections with no fenced yaml example; `wrong_kind` = examples
/// whose top-level `kind:` is an alias (or otherwise not the H3's canonical
/// kind). Enforced (not warned) so a regressing docs/rules.md fails the
/// docs-bundle build before it can publish a broken example to alint.org.
/// Split out of `generate_rules_pages` to keep that orchestrator within the
/// clippy line budget.
fn enforce_example_gates(missing: &[String], wrong_kind: &[String]) -> Result<()> {
    if !missing.is_empty() {
        anyhow::bail!(
            "{} rule kind H3 section(s) in docs/rules.md are missing a \
             ```yaml usage example:\n  - {}\n\n\
             Each per-rule heading must include at least one fenced \
             ```yaml block before the next heading. The block becomes \
             the usage example shown on alint.org/docs/rules/<family>/<kind>/.",
            missing.len(),
            missing.join("\n  - "),
        );
    }
    if !wrong_kind.is_empty() {
        anyhow::bail!(
            "{} rule example(s) in docs/rules.md use a non-canonical `kind:` \
             (an alias, or a wrong kind):\n  - {}\n\n\
             Each rule H3's fenced yaml example must set its top-level `kind:` to \
             the H3's canonical name (the first backticked kind in the heading), \
             not an alias declared in `(alias: …)`.",
            wrong_kind.len(),
            wrong_kind.join("\n  - "),
        );
    }
    Ok(())
}

/// Every documented scenario's `docs.kind` must resolve to an emitted page, or
/// its example is silently dropped (ADR-0014).
fn enforce_documented_page_targets(
    documented: &std::collections::BTreeMap<String, Vec<examples::RenderedExample>>,
    kind_to_family: &std::collections::HashMap<String, String>,
) -> Result<()> {
    let mut orphan: Vec<&str> = documented
        .keys()
        .filter(|k| !kind_to_family.contains_key(*k))
        .map(String::as_str)
        .collect();
    orphan.sort_unstable();
    if !orphan.is_empty() {
        anyhow::bail!(
            "{} documented scenario `docs.kind` value(s) match no rule page \
             (a typo, or an alias without its own page):\n  - {}\n\n\
             Set `docs.kind` to the canonical kind whose H3 the example documents.",
            orphan.len(),
            orphan.join("\n  - "),
        );
    }
    Ok(())
}

/// Strip release-gated prose blocks from a rule's markdown body. A block
/// delimited by `<!-- alint:since=X -->` ... `<!-- /alint:since -->` is
/// dropped when `released` is `Some(v)` and `X > v` (the prose describes an
/// unreleased capability); otherwise the block content is kept. The marker
/// comments themselves are ALWAYS removed, so they never reach a published
/// page. Code fences are tracked so a marker inside a fenced example stays
/// literal. The prose analogue of the Options-table `x-since` gate
/// (ADR-0007; docs/design/v0.14/documentation-drift.md P1).
fn strip_unreleased_prose(
    body: &str,
    released: Option<crate::rule_options_table::Version>,
) -> String {
    // Fast path: almost every rule body carries no marker, so leave it
    // byte-for-byte untouched (no line-ending normalisation).
    if !body.contains("alint:since") {
        return body.to_string();
    }
    let mut out = String::with_capacity(body.len());
    let mut in_code_fence = false;
    let mut dropping = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_fence = !in_code_fence;
            if !dropping {
                out.push_str(line);
                out.push('\n');
            }
            continue;
        }
        if !in_code_fence {
            if let Some(rest) = trimmed.strip_prefix("<!-- alint:since=") {
                let ver = rest.trim().trim_end_matches("-->").trim();
                dropping =
                    released.is_some_and(|rel| crate::rule_options_table::parse_version(ver) > rel);
                continue; // never emit the marker line itself
            }
            if trimmed == "<!-- /alint:since -->" {
                dropping = false;
                continue;
            }
        }
        if !dropping {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

// Page-rendering inputs are all distinct scalars/slices; a struct
// would just relocate the argument list without simplifying callers.
#[allow(clippy::too_many_arguments)]
fn emit_rule_page(
    family_dir: &Path,
    kind: &str,
    family_slug: &str,
    family_title: &str,
    body: &str,
    siblings: &[&str],
    options_md: Option<&str>,
    sidebar_order: u32,
    category_slugs: &[&str],
    documented: Option<&[examples::RenderedExample]>,
) -> Result<()> {
    let mut page = String::new();
    let _ = writeln!(&mut page, "---");
    let _ = writeln!(&mut page, "title: '{kind}'");
    let _ = writeln!(
        &mut page,
        "description: '{}'",
        escape_yaml_string(&rule_meta_description(kind, family_title, body))
    );
    let _ = writeln!(&mut page, "sidebar:");
    let _ = writeln!(&mut page, "  order: {sidebar_order}");
    if !category_slugs.is_empty() {
        let list = category_slugs
            .iter()
            .map(|s| format!("'{s}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(&mut page, "categories: [{list}]");
    }
    let _ = writeln!(&mut page, "---");
    let _ = writeln!(&mut page);
    let body = lead_example_with_kind(body.trim_start_matches('\n'), kind);
    page.push_str(&body);
    // Authoritative options table, derived from the type-generated
    // JSON Schema (ADR-0001). Injected between the hand-written
    // prose and the "See also" footer so every rule page carries a
    // reference that can't drift from the engine's `Options` structs.
    if let Some(opts) = options_md {
        while page.ends_with('\n') {
            page.pop();
        }
        page.push_str("\n\n");
        page.push_str(opts.trim_end_matches('\n'));
        page.push('\n');
    }
    // ADR-0014: worked examples rendered from the kind's documented scenarios
    // (example repo + config + a real `alint check` run). Injected after the
    // options table, before "See also".
    if let Some(rendered) = documented {
        while page.ends_with('\n') {
            page.pop();
        }
        page.push_str("\n\n## Example\n\n");
        for ex in rendered {
            page.push_str(&ex.markdown);
        }
    }
    if !siblings.is_empty() {
        // Trim trailing newlines so the footer doesn't have a
        // gaping gap above it.
        while page.ends_with("\n\n") {
            page.pop();
        }
        if !page.ends_with('\n') {
            page.push('\n');
        }
        let _ = writeln!(&mut page);
        let _ = writeln!(&mut page, "## See also");
        let _ = writeln!(&mut page);
        for sib in siblings {
            let _ = writeln!(&mut page, "- [`{sib}`](/docs/rules/{family_slug}/{sib}/)");
        }
    }
    if !page.ends_with('\n') {
        page.push('\n');
    }
    let dest = family_dir.join(format!("{kind}.md"));
    fs::write(&dest, page).with_context(|| format!("writing {}", dest.display()))?;
    Ok(())
}

/// Family overview: one paragraph on what the family is for plus
/// a flat table-of-contents linking to each kind. alint.org
/// references this page explicitly via a `link:` "Overview"
/// item in `astro.config.mjs` (it's NOT picked up by
/// autogenerate — the Rules section uses hand-rolled
/// sub-groups, see the comment over the Rules sidebar entry).
fn emit_family_index(
    family_dir: &Path,
    family_title: &str,
    family_order: u32,
    rules: &[RuleEntry],
) -> Result<()> {
    fs::write(
        family_dir.join("index.md"),
        crate::family_index::render(family_title, family_order, rules),
    )?;
    Ok(())
}

/// Master `/docs/rules/` page: alphabetical index of every
/// registered rule kind. This is the canonical "where do I find
/// rule X?" landing.
fn emit_rules_master_index(
    rules_dir: &Path,
    all_kinds: &[KindEntry],
    families: &[FamilySummary],
    alias_count: usize,
) -> Result<()> {
    let mut sorted: Vec<&KindEntry> = all_kinds.iter().collect();
    sorted.sort_by(|a, b| a.kind.cmp(&b.kind));

    let mut page = String::new();
    let _ = writeln!(&mut page, "---");
    let _ = writeln!(&mut page, "title: Rules");
    let _ = writeln!(
        &mut page,
        "description: Every rule kind alint ships, with one-line summaries and links to family + per-rule pages."
    );
    let _ = writeln!(&mut page, "sidebar:");
    let _ = writeln!(&mut page, "  order: 1");
    let _ = writeln!(&mut page, "  label: 'Index'");
    let _ = writeln!(&mut page, "---");
    let _ = writeln!(&mut page);
    // Headline count is the canonical rule-kind figure used
    // across README / docs/site/about / schema / alint.org: the
    // {behaviors} documented rule behaviors plus {alias_count}
    // short-name aliases that ride inline on their canonical page
    // (79 = 69 + 10 as of v0.10.0; this comment intentionally
    // does not pin a number — it tracks via the format args).
    // Deriving the total from those two components keeps the
    // sentence self-consistent even if the registry/docs ever drift
    // (that drift is independently caught by the WARN loop above).
    let _ = writeln!(
        &mut page,
        "alint ships {total} rule kinds across {fc} families \
         ({behaviors} distinct rule behaviors plus {alias_count} short-name \
         aliases like `content_matches` → `file_content_matches`). Each \
         rule is one entry in your `.alint.yml` under `rules:`.",
        total = all_kinds.len() + alias_count,
        fc = families.len(),
        behaviors = all_kinds.len(),
        alias_count = alias_count
    );
    let _ = writeln!(&mut page);
    let _ = writeln!(&mut page, "## By family");
    let _ = writeln!(&mut page);
    for f in families {
        let _ = writeln!(
            &mut page,
            "- [{title}](/docs/rules/{slug}/) — {n} rule{plural}",
            title = f.title,
            slug = f.slug,
            n = f.rule_count,
            plural = if f.rule_count == 1 { "" } else { "s" }
        );
    }
    let _ = writeln!(&mut page);
    let _ = writeln!(&mut page, "## Alphabetical");
    let _ = writeln!(&mut page);
    for k in sorted {
        let _ = writeln!(
            &mut page,
            "- [`{kind}`](/docs/rules/{family}/{kind}/) — {summary} _({family_title})_",
            kind = k.kind,
            family = k.family_slug,
            family_title = k.family_title,
            summary = k.summary
        );
    }
    fs::write(rules_dir.join("index.md"), page)?;
    Ok(())
}

/// The architecture view embedded atop a generated concept page, if any.
pub(crate) fn concept_view_id(slug: &str) -> Option<&'static str> {
    match slug {
        "fix-operations" => Some("fixFlow"),
        "nested-configs" => Some("monorepoNesting"),
        _ => None,
    }
}

/// Emit a non-rule concept page (Fix operations, Nested
/// configs). Lives under `concepts/` rather than `rules/` so
/// the rules tree is purely about rule kinds.
fn emit_concept_page(target_dir: &Path, slug: &str, title: &str, body: &str) -> Result<()> {
    let dir = target_dir.join("concepts");
    fs::create_dir_all(&dir)?;
    let mut page = String::new();
    let _ = writeln!(&mut page, "---");
    let _ = writeln!(&mut page, "title: '{}'", escape_yaml_string(title));
    let _ = writeln!(
        &mut page,
        "description: 'alint concept: {}.'",
        title.to_lowercase()
    );
    let _ = writeln!(&mut page, "---");
    let _ = writeln!(&mut page);
    if let Some(view) = concept_view_id(slug) {
        let _ = writeln!(&mut page, "<likec4-view view-id=\"{view}\"></likec4-view>");
        let _ = writeln!(&mut page);
    }
    page.push_str(body.trim_start_matches('\n'));
    if !page.ends_with('\n') {
        page.push('\n');
    }
    fs::write(dir.join(format!("{slug}.md")), page)?;
    Ok(())
}

/// Sections of a markdown document split at H2 headers (`## …`).
/// Anything before the first H2 is dropped (it's typically the
/// document's H1 + intro paragraph; we don't carry that into the
/// per-family pages).
struct H2Section {
    title: String,
    body: String,
}

fn split_h2_sections(src: &str) -> Vec<H2Section> {
    let mut sections: Vec<H2Section> = Vec::new();
    let mut current: Option<H2Section> = None;
    for line in src.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            if let Some(prev) = current.take() {
                sections.push(prev);
            }
            current = Some(H2Section {
                title: rest.trim().to_string(),
                body: String::new(),
            });
        } else if let Some(sec) = current.as_mut() {
            sec.body.push_str(line);
            sec.body.push('\n');
        }
    }
    if let Some(prev) = current.take() {
        sections.push(prev);
    }
    sections
}

/// URL-safe slug from a heading. Lowercases, drops any character
/// that isn't `[a-z0-9-]`, collapses runs of `-`. Adequate for
/// headings like "Security / Unicode sanity" → "security-unicode-sanity".
pub(crate) fn slugify(s: &str) -> String {
    let lc = s.to_lowercase();
    let mut out = String::with_capacity(lc.len());
    let mut last_dash = false;
    for ch in lc.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Quote a string safely for a single-quoted YAML scalar — only
/// `'` needs escaping (doubled). Frontmatter titles like
/// `Security / Unicode sanity` need this.
pub(crate) fn escape_yaml_string(s: &str) -> String {
    s.replace('\'', "''")
}

/// One markdown page per `crates/alint-dsl/rulesets/v1/**/*.yml`,
/// summarising the ruleset's rules with their level / kind / message
/// / policy URL. Slash-separated names (`hygiene/lockfiles`,
/// `ci/github-actions`) are flattened with a `-` for the bundle
/// filename so Starlight's autogen sidebar produces a flat list.
///
/// Each page now also carries:
/// - An overview parsed from the YAML's leading comment block
///   (the natural-language description the ruleset author wrote
///   above `version: 1`).
/// - A `## Source` section with a link back to the canonical YAML
///   in the alint repo plus the full file embedded as a fenced
///   code block, so readers can see the exact rule definitions
///   without leaving the docs.
fn generate_bundled_ruleset_pages(
    workspace: &Path,
    target_dir: &Path,
    kind_to_family: &std::collections::HashMap<String, String>,
) -> Result<()> {
    struct RulesetEntry {
        pretty: String,
        flat_slug: String,
        summary: String,
    }

    let rulesets_root = workspace.join(docs_paths::RULESETS_DIR);
    let bundled_dir = target_dir.join("bundled-rulesets");
    fs::create_dir_all(&bundled_dir)?;

    let mut entries: Vec<RulesetEntry> = Vec::new();

    for entry in walkdir_plain(&rulesets_root)? {
        let md = fs::metadata(&entry)?;
        if !md.is_file() {
            continue;
        }
        let ext = entry.extension().and_then(|s| s.to_str()).unwrap_or("");
        if ext != "yml" && ext != "yaml" {
            continue;
        }
        let rel = entry.strip_prefix(&rulesets_root).unwrap();
        let pretty_name = rel.with_extension("");
        let pretty_str = pretty_name.to_string_lossy().replace('\\', "/");
        let flat_slug = pretty_str.replace('/', "-");
        let flat_filename = format!("{flat_slug}.md");

        let yaml_text =
            fs::read_to_string(&entry).with_context(|| format!("reading {}", entry.display()))?;
        let yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml_text)
            .with_context(|| format!("parsing {}", entry.display()))?;

        // Repo-relative path for the source link, always forward-
        // slashed regardless of host OS so the URL is portable.
        let rel_path_str = rel.to_string_lossy().replace('\\', "/");
        let rel_repo_path = format!("{}/{}", docs_paths::RULESETS_DIR, rel_path_str);

        let overview_md = render_overview_from_comments(&yaml_text);
        let summary = first_overview_sentence(&overview_md);

        let page = render_ruleset_page(
            &pretty_str,
            &overview_md,
            &yaml_text,
            &rel_repo_path,
            &yaml,
            kind_to_family,
        );
        let dest = bundled_dir.join(&flat_filename);
        fs::write(&dest, page).with_context(|| format!("writing {}", dest.display()))?;

        entries.push(RulesetEntry {
            pretty: pretty_str,
            flat_slug,
            summary,
        });
    }

    // An index page listing every ruleset — overwrites the hand-
    // written placeholder when the sync script lays the bundle into
    // alint.org. Each entry shows a one-line summary (the first
    // sentence of the ruleset's leading comment block) so the
    // index is scannable without opening every page.
    entries.sort_by(|a, b| a.pretty.cmp(&b.pretty));
    let mut index = String::new();
    let _ = writeln!(&mut index, "---");
    let _ = writeln!(&mut index, "title: Bundled Rulesets");
    let _ = writeln!(
        &mut index,
        "description: One-line ecosystem baselines built into the alint binary."
    );
    let _ = writeln!(&mut index, "sidebar:");
    let _ = writeln!(&mut index, "  order: 1");
    let _ = writeln!(&mut index, "---");
    let _ = writeln!(&mut index);
    let _ = writeln!(
        &mut index,
        "Adopt with `extends: [alint://bundled/<name>@v1]`. Each ruleset's full rule list lives on its own page below."
    );
    let _ = writeln!(&mut index);
    let _ = writeln!(&mut index, "## Currently shipped");
    let _ = writeln!(&mut index);
    for e in &entries {
        if e.summary.is_empty() {
            let _ = writeln!(
                &mut index,
                "- [`{name}@v1`](/docs/bundled-rulesets/{slug}/)",
                name = e.pretty,
                slug = e.flat_slug
            );
        } else {
            let _ = writeln!(
                &mut index,
                "- [`{name}@v1`](/docs/bundled-rulesets/{slug}/) — {summary}",
                name = e.pretty,
                slug = e.flat_slug,
                summary = e.summary,
            );
        }
    }
    fs::write(bundled_dir.join("index.md"), index)?;

    Ok(())
}

/// GitHub repo-relative base for source-of-truth links rendered
/// into the bundled-ruleset pages. Pinned to `main` so readers
/// always land on the latest version of each ruleset; the page
/// also embeds a verbatim snapshot of the YAML below the link
/// for offline / point-in-time reference.
const ALINT_REPO_BLOB_URL: &str = "https://github.com/asamarts/alint/blob/main";

/// Render the markdown body for a single bundled ruleset. The
/// page has four sections, in order:
///
/// 1. **Overview** — the leading comment block from the YAML,
///    rendered as natural-language prose (with any inline YAML
///    code samples promoted to fenced ```yaml``` blocks for
///    syntax highlighting).
/// 2. **Adopt with** — a copy-pasteable `extends:` snippet. We
///    suppress this when the overview already contains an
///    `alint://bundled/...` reference (that's the layered-overlay
///    case where the comment author specifies a multi-ruleset
///    adoption recipe — auto-generating a single-line snippet on
///    top would be redundant and incorrect).
/// 3. **Rules** — table-of-contents-style list of every `id` in
///    the ruleset with kind / level / when / policy / message
///    pulled from the YAML. Each `kind` is a link into the rule
///    reference (`/docs/rules/<family>/<kind>/`) when
///    `kind_to_family` knows about it.
/// 4. **Source** — a permalink into the alint repo plus the full
///    YAML file embedded as a fenced code block.
///
/// `kind_to_family` is consulted to render each rule's `kind` as
/// a link into the rules tree. Kinds not in the map (e.g. a
/// brand-new kind missing from rules.md) render as plain code;
/// the rules-pages generator emits a warning in that case so the
/// gap surfaces.
fn render_ruleset_page(
    name: &str,
    overview_md: &str,
    yaml_text: &str,
    rel_repo_path: &str,
    yaml: &serde_yaml_ng::Value,
    kind_to_family: &std::collections::HashMap<String, String>,
) -> String {
    let mut out = String::new();
    let _ = writeln!(&mut out, "---");
    let _ = writeln!(&mut out, "title: '{name}@v1'");
    // SERP description: lead with what the ruleset actually does
    // (its author-written overview, first sentence — the unique
    // value prop a searcher scanning results wants reflected
    // back), keep the ruleset name (the search query) in the
    // string, cap ~155 chars, no em-dashes. Fall back to a name-
    // scoped clause when the YAML has no leading comment block.
    let ruleset_summary = meta_desc_clean(&first_overview_sentence(overview_md), 130);
    let ruleset_desc = if ruleset_summary.len() < 25 {
        format!(
            "{name}@v1: a bundled alint ruleset. Adopt with extends: [alint://bundled/{name}@v1]."
        )
    } else if ruleset_summary.ends_with('.') {
        format!("{ruleset_summary} alint bundled ruleset {name}@v1.")
    } else {
        format!("{ruleset_summary}. alint bundled ruleset {name}@v1.")
    };
    let _ = writeln!(
        &mut out,
        "description: '{}'",
        escape_yaml_string(&meta_desc_clean(&ruleset_desc, 158))
    );
    let _ = writeln!(&mut out, "---");
    let _ = writeln!(&mut out);

    if !overview_md.is_empty() {
        out.push_str(overview_md);
        let _ = writeln!(&mut out);
        let _ = writeln!(&mut out);
    }

    // The overlay-style rulesets (e.g. monorepo/cargo-workspace)
    // already document a multi-ruleset `extends:` recipe in their
    // leading comment. Re-rendering a single-line snippet under
    // them would be both redundant and misleading, so we suppress
    // the auto-gen Adopt-with whenever the overview already
    // mentions the bundled-URI scheme.
    let overview_has_adoption = overview_md.contains("alint://bundled/");
    if !overview_has_adoption {
        let _ = writeln!(&mut out, "## Adopt with");
        let _ = writeln!(&mut out);
        let _ = writeln!(&mut out, "```yaml");
        let _ = writeln!(&mut out, "extends:");
        let _ = writeln!(&mut out, "  - alint://bundled/{name}@v1");
        let _ = writeln!(&mut out, "```");
        let _ = writeln!(&mut out);
    }

    if let Some(rules) = yaml.get("rules").and_then(|r| r.as_sequence()) {
        let _ = writeln!(&mut out, "## Rules");
        let _ = writeln!(&mut out);

        for rule in rules {
            let id = rule.get("id").and_then(|v| v.as_str()).unwrap_or("(no-id)");
            let kind = rule.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let level = rule.get("level").and_then(|v| v.as_str()).unwrap_or("");
            let when = rule.get("when").and_then(|v| v.as_str());
            let msg = rule.get("message").and_then(|v| v.as_str());
            let policy = rule.get("policy_url").and_then(|v| v.as_str());

            let _ = writeln!(&mut out, "### `{id}`");
            let _ = writeln!(&mut out);
            if !kind.is_empty() {
                let kind_md = match kind_to_family.get(kind) {
                    Some(family) => {
                        format!("[`{kind}`](/docs/rules/{family}/{kind}/)")
                    }
                    None => format!("`{kind}`"),
                };
                let _ = writeln!(&mut out, "- **kind**: {kind_md}");
            }
            if !level.is_empty() {
                let _ = writeln!(&mut out, "- **level**: `{level}`");
            }
            if let Some(when) = when {
                let _ = writeln!(&mut out, "- **when**: `{when}`");
            }
            if let Some(policy) = policy {
                let _ = writeln!(&mut out, "- **policy**: <{policy}>");
            }
            if let Some(msg) = msg {
                let _ = writeln!(&mut out);
                let _ = writeln!(&mut out, "> {}", msg.replace('\n', " "));
            }
            let _ = writeln!(&mut out);
        }
    } else {
        let _ = writeln!(&mut out, "_(no rules — this ruleset is a placeholder.)_");
        let _ = writeln!(&mut out);
    }

    let _ = writeln!(&mut out, "## Source");
    let _ = writeln!(&mut out);
    let _ = writeln!(
        &mut out,
        "The full ruleset definition is committed at \
         [`{rel_repo_path}`]({ALINT_REPO_BLOB_URL}/{rel_repo_path}) in the alint repo \
         (the snapshot below is generated verbatim from that file).",
    );
    let _ = writeln!(&mut out);
    let _ = writeln!(&mut out, "```yaml");
    out.push_str(yaml_text.trim_end_matches('\n'));
    out.push('\n');
    let _ = writeln!(&mut out, "```");
    out
}

/// Parse the leading comment block of a ruleset YAML into
/// markdown. The first line is expected to be the canonical
/// `# alint://bundled/<name>@<rev>` URI tag and is stripped.
/// Subsequent comment lines are emitted as paragraphs (preserving
/// the author's line breaks so list items render correctly) or,
/// when a paragraph starts with a 4-space indent, as a fenced
/// `yaml` code block — that's the convention authors use for
/// the "here's the `extends:` snippet" mini-blocks.
///
/// Reading stops at the first non-comment, non-blank line. By
/// convention the rule body starts right after the leading
/// comment block, so this naturally captures only the file's
/// top-of-file description.
pub(crate) fn render_overview_from_comments(yaml_text: &str) -> String {
    enum Block {
        Para(Vec<String>),
        Code(Vec<String>),
    }

    let mut blocks: Vec<Block> = Vec::new();
    let mut comment_started = false;
    // Treat start-of-input as a paragraph break so the very first
    // non-blank comment line opens a new block.
    let mut paragraph_break = true;

    for raw in yaml_text.lines() {
        let line = raw.trim_end();
        if line.is_empty() {
            if !comment_started {
                continue;
            }
            // A literal blank line (no `#`) ends the leading
            // comment block. Authors use blank `#` lines for
            // paragraph breaks INSIDE the block — those are
            // handled below.
            break;
        }
        if !line.starts_with('#') {
            break;
        }
        comment_started = true;

        // Strip the `#` marker and exactly one trailing space.
        let after_hash = &line[1..];
        let body = after_hash.strip_prefix(' ').unwrap_or(after_hash);

        if body.is_empty() {
            paragraph_break = true;
            continue;
        }
        // Skip the canonical `# alint://bundled/<name>@<rev>` URI
        // header — it's metadata, not prose.
        if body.starts_with("alint://bundled/") {
            continue;
        }

        // 4-space indent at the START of a block = code block.
        // Continuation lines inside an existing block keep that
        // block's kind regardless of their own indent (so
        // bulleted lists with hanging-indent continuations
        // stay in a single Para block).
        if paragraph_break {
            if let Some(rest) = body.strip_prefix("    ") {
                blocks.push(Block::Code(vec![rest.to_string()]));
            } else {
                blocks.push(Block::Para(vec![body.to_string()]));
            }
        } else {
            match blocks
                .last_mut()
                .expect("paragraph_break=false implies a current block exists")
            {
                Block::Para(lines) => lines.push(body.to_string()),
                Block::Code(lines) => {
                    // Inside a code block, dedent up to 4 spaces so
                    // the rendered code matches the opening line's
                    // visual indentation.
                    let dedented = body.strip_prefix("    ").unwrap_or(body);
                    lines.push(dedented.to_string());
                }
            }
        }
        paragraph_break = false;
    }

    let mut out = String::new();
    for (i, b) in blocks.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        match b {
            Block::Para(lines) => out.push_str(&lines.join("\n")),
            Block::Code(lines) => {
                out.push_str("```yaml\n");
                for l in lines {
                    out.push_str(l);
                    out.push('\n');
                }
                out.push_str("```");
            }
        }
    }
    out
}

/// First-sentence summary of a rendered overview, used to
/// populate the bundled-rulesets index page. Skips fenced code
/// blocks, takes the first paragraph of natural-language prose,
/// and truncates at the first sentence-ending `. ` boundary.
pub(crate) fn first_overview_sentence(overview_md: &str) -> String {
    let mut paragraph = String::new();
    let mut in_code = false;
    for line in overview_md.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }
        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(trimmed);
    }
    if let Some(idx) = paragraph.find(". ") {
        paragraph.truncate(idx + 1);
    }
    paragraph.trim().to_string()
}

/// The architecture flow embedded atop a generated `cli/<sub>` page, if any.
/// The architecture view embedded on a `cli/<sub>` reference page, with a
/// one-line caption. `facts` is intentionally absent: factsFlow depicts
/// fact-evaluation *plus* rule gating, but `alint facts` only evaluates and
/// prints facts, so the diagram would over-reach.
pub(crate) fn cli_view(sub: &str) -> Option<(&'static str, &'static str)> {
    match sub {
        "check" => Some(("checkFlow", "The pipeline `alint check` runs:")),
        "fix" => Some(("fixFlow", "How `alint fix` applies fixes and re-checks:")),
        "lsp" => Some(("lspFlow", "How `alint lsp` serves an editor over LSP:")),
        _ => None,
    }
}

/// The contiguous indented body of a top-level `--help` section (e.g.
/// `"Commands:"` / `"Options:"`): the indented lines after the header, up to the
/// next non-indented header. Blank lines are dropped.
fn help_section_body<'a>(help: &'a str, header: &str) -> Vec<&'a str> {
    let mut lines = help.lines();
    for line in lines.by_ref() {
        if line.trim_end() == header {
            break;
        }
    }
    let mut body = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(char::is_whitespace) {
            break; // reached the next section header
        }
        body.push(line);
    }
    body
}

/// Parse an indented clap `term  description` block (a `Commands:` or `Options:`
/// body) into `(term, description)` pairs, folding wrapped continuation lines
/// into the preceding description. clap puts each term at a shallow indent (2 or
/// 6) and wraps its description at a deeper column, which is how we tell them
/// apart.
fn parse_help_definition_list(lines: &[&str]) -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = Vec::new();
    for line in lines {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if indent <= 6 {
            let (term, desc) = trimmed.split_once("  ").unwrap_or((trimmed, ""));
            entries.push((term.trim().to_string(), desc.trim().to_string()));
        } else if let Some((_, desc)) = entries.last_mut() {
            if !desc.is_empty() {
                desc.push(' ');
            }
            desc.push_str(line.trim());
        }
    }
    entries
}

/// Render the top-level `alint --help` as a formatted CLI landing page: the
/// about blurb, the usage line, a Commands table (each linked to its subcommand
/// page), and a Global-options table. Everything is parsed from the captured
/// `--help`, so it can never drift from the binary. Returns `None` if the help
/// doesn't parse into a sane shape (no options found) so the caller falls back
/// to the raw help dump — a clap format change degrades to the old behaviour,
/// never to garbage.
fn format_top_help(help: &str) -> Option<String> {
    let commands = parse_help_definition_list(&help_section_body(help, "Commands:"));
    let options = parse_help_definition_list(&help_section_body(help, "Options:"));
    if options.is_empty() {
        return None;
    }

    let about = help
        .lines()
        .take_while(|l| !l.starts_with("Usage:"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    let usage = help
        .lines()
        .find(|l| l.starts_with("Usage:"))
        .unwrap_or_default();

    let esc = |s: &str| s.replace('|', "\\|");
    let known: std::collections::HashSet<&str> = CLI_REFERENCE_SUBCMDS.iter().copied().collect();

    let mut out = String::new();
    if !about.is_empty() {
        let _ = writeln!(&mut out, "{about}\n");
    }
    if !usage.is_empty() {
        let _ = writeln!(&mut out, "```\n{usage}\n```\n");
    }
    if !commands.is_empty() {
        let _ = writeln!(&mut out, "## Commands\n");
        let _ = writeln!(&mut out, "| Command | Description |");
        let _ = writeln!(&mut out, "| --- | --- |");
        for (cmd, desc) in &commands {
            let name = cmd.split_whitespace().next().unwrap_or(cmd);
            let cell = if known.contains(name) {
                format!("[`{cmd}`](/docs/cli/{name}/)")
            } else {
                format!("`{cmd}`")
            };
            let _ = writeln!(&mut out, "| {cell} | {} |", esc(desc));
        }
        let _ = writeln!(&mut out);
    }
    let _ = writeln!(&mut out, "## Global options\n");
    let _ = writeln!(&mut out, "These apply to every subcommand.\n");
    let _ = writeln!(&mut out, "| Flag | Description |");
    let _ = writeln!(&mut out, "| --- | --- |");
    for (flag, desc) in &options {
        let _ = writeln!(&mut out, "| `{}` | {} |", esc(flag), esc(desc));
    }
    let _ = writeln!(&mut out);
    let _ = writeln!(&mut out, "<sub>Generated from `alint --help`.</sub>");

    Some(out)
}

/// Build the alint binary in release mode, then capture
/// `alint --help` and `alint <subcmd> --help` for each subcommand.
/// The top-level help renders as a formatted landing page (`cli/index.md`);
/// each subcommand's help becomes its own page under `cli/<subcmd>.md`.
fn generate_cli_reference(workspace: &Path, target_dir: &Path) -> Result<()> {
    let bin = build_release_binary()?;

    let cli_dir = target_dir.join("cli");
    fs::create_dir_all(&cli_dir)?;

    // Top-level help → cli/index.md
    let top = run_help(&bin, &[])?;
    let mut index = String::new();
    let _ = writeln!(&mut index, "---");
    let _ = writeln!(&mut index, "title: CLI");
    let _ = writeln!(
        &mut index,
        "description: alint's subcommands and global flags, captured from the binary itself."
    );
    let _ = writeln!(&mut index, "sidebar:");
    let _ = writeln!(&mut index, "  order: 1");
    let _ = writeln!(&mut index, "---");
    let _ = writeln!(&mut index);
    // Prefer a formatted landing page (Commands + Global-options tables) parsed
    // from `--help`; fall back to the raw dump if the help doesn't parse.
    if let Some(body) = format_top_help(&top) {
        index.push_str(&body);
    } else {
        let _ = writeln!(&mut index, "```");
        index.push_str(&top);
        let _ = writeln!(&mut index, "```");
    }
    fs::write(cli_dir.join("index.md"), index)?;

    let subcmds = CLI_REFERENCE_SUBCMDS;
    for sub in subcmds {
        let help = run_help(&bin, &[sub])?;
        // SERP description: clap prints the subcommand's own one-
        // line summary as the first non-empty line of --help
        // (before the blank line and `Usage:`). Use it verbatim
        // so the snippet says what `alint <sub>` does, not "see
        // --help". Keep the command (the search query) in the
        // string; no em-dashes (style guide).
        let help_summary = meta_desc_clean(&help_first_line(&help), 120);
        let cli_desc = if help_summary.len() < 12 {
            format!("alint {sub} subcommand. CLI reference and flags for alint {sub}.")
        } else if help_summary.ends_with('.') {
            format!("{help_summary} alint {sub} CLI reference and flags.")
        } else {
            format!("{help_summary}. alint {sub} CLI reference and flags.")
        };
        let mut page = String::new();
        let _ = writeln!(&mut page, "---");
        let _ = writeln!(&mut page, "title: 'alint {sub}'");
        let _ = writeln!(
            &mut page,
            "description: '{}'",
            escape_yaml_string(&meta_desc_clean(&cli_desc, 158))
        );
        let _ = writeln!(&mut page, "---");
        let _ = writeln!(&mut page);
        if let Some((view, caption)) = cli_view(sub) {
            let _ = writeln!(&mut page, "{caption}");
            let _ = writeln!(&mut page);
            let _ = writeln!(&mut page, "<likec4-view view-id=\"{view}\"></likec4-view>");
            let _ = writeln!(&mut page);
        }
        let _ = writeln!(&mut page, "```");
        page.push_str(&help);
        let _ = writeln!(&mut page, "```");
        fs::write(cli_dir.join(format!("{sub}.md")), page)?;
    }

    // Sanity-check: workspace path exists.
    let _ = workspace;
    Ok(())
}

fn run_help(bin: &Path, subcmd_args: &[&str]) -> Result<String> {
    let mut cmd = Command::new(bin);
    cmd.args(subcmd_args).arg("--help");
    let out = cmd.output().with_context(|| format!("running {cmd:?}"))?;
    if !out.status.success() {
        bail!(
            "alint {:?} --help exited {:?}",
            subcmd_args,
            out.status.code()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// First descriptive line of a clap `--help` dump: clap prints the
/// command's about-string as the leading line(s) before the blank
/// line that precedes `Usage:`. Returns the joined about block
/// (it can wrap to a second line), skipping anything that is
/// itself a section header (`Usage:`, `Options:`, `Arguments:`,
/// `Commands:`). Used as the seed for the CLI page SERP
/// description.
fn help_first_line(help: &str) -> String {
    let mut acc = String::new();
    for line in help.lines() {
        let t = line.trim();
        if t.is_empty() {
            if !acc.is_empty() {
                break;
            }
            continue;
        }
        if t.starts_with("Usage:")
            || t.starts_with("Options:")
            || t.starts_with("Arguments:")
            || t.starts_with("Commands:")
        {
            break;
        }
        if !acc.is_empty() {
            acc.push(' ');
        }
        acc.push_str(t);
    }
    acc
}

/// Run `xtask/scripts/render-history.py --json-out <bundle>/benchmarks-trajectory.json`
/// so the bundle ships the cross-version headline-trajectory data as
/// machine-readable JSON. The script discards stdout (where the
/// regular markdown render goes) to keep this side-effect-only;
/// `bench-record.yml` is the workflow that owns HISTORY.md updates.
///
/// Python is a soft dependency — the renderer is Python because it
/// pre-dates the xtask binary. Two callers are in play:
///
/// - `docs-bundle.yml` runs on `ubuntu-latest` which ships python3
///   by default. This is the workflow that ACTUALLY ships the
///   bundle alint.org consumes, so the trajectory always lands in
///   production via this path.
/// - `ci.yml`'s Docs job runs `docs-export --check` on the
///   self-hosted `[linux, alint]` runner which lacks python3
///   (bench-record installs its own pinned interpreter rather than
///   relying on a system one). On that path we skip with a warning
///   instead of failing — the trajectory check belongs to the
///   `coverage_audit_benchmarks_trajectory` test, not docs-export.
fn generate_benchmarks_trajectory(workspace: &Path, target_dir: &Path) -> Result<()> {
    let script = workspace.join("xtask/scripts/render-history.py");
    if !script.is_file() {
        bail!("expected renderer at {}", script.display());
    }
    let out_path = target_dir.join("benchmarks-trajectory.json");
    let attempt = Command::new("python3")
        .arg(&script)
        .arg("--json-out")
        .arg(&out_path)
        // Discard the markdown render; HISTORY.md is updated by
        // bench-record.yml on tag pushes, not by docs-export.
        .stdout(std::process::Stdio::null())
        .current_dir(workspace)
        .status();
    let status = match attempt {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "[xtask] warning: python3 not on PATH; \
                 skipping benchmarks-trajectory.json. \
                 The docs-bundle.yml workflow runs on ubuntu-latest \
                 (which has python3) so production bundles still ship it."
            );
            return Ok(());
        }
        Err(e) => {
            return Err(anyhow::Error::new(e).context(format!("running {}", script.display())));
        }
    };
    if !status.success() {
        bail!("{} exited {:?}", script.display(), status.code());
    }
    if !out_path.exists() {
        bail!(
            "expected JSON at {} but renderer did not write it",
            out_path.display()
        );
    }
    Ok(())
}

fn write_manifest(target_dir: &Path) -> Result<()> {
    let sha = git_sha().unwrap_or_else(|| "unknown".to_string());
    let version = env!("CARGO_PKG_VERSION");
    let now = now_iso();
    let rule_kinds_total = counts::count_canonical_rule_kinds()?;
    let bundled_rulesets_total = counts::count_canonical_bundled_rulesets()?;
    let subcommands_total = counts::count_canonical_subcommands()?;
    let output_formats_total = counts::count_canonical_output_formats()?;
    let auto_fix_ops_total = counts::count_canonical_auto_fix_ops()?;

    // format_version BUMPED 2 -> 3 (Phase 2.6 of the drift audit) so
    // alint.org's drift gate can affirmatively detect the three new
    // count fields. The alint.org side's sync-from-alint.mjs widened
    // its accepted-versions set to {1,2,3} BEFORE this bump landed
    // so CF Pages builds don't silently fail the way they did during
    // the 2026-05-22 v2 bump.
    let json = format!(
        "{{\n  \
         \"alint_version\": \"{version}\",\n  \
         \"git_sha\": \"{sha}\",\n  \
         \"generated_at\": \"{now}\",\n  \
         \"format_version\": 3,\n  \
         \"rule_kinds_total\": {rule_kinds_total},\n  \
         \"bundled_rulesets_total\": {bundled_rulesets_total},\n  \
         \"subcommands_total\": {subcommands_total},\n  \
         \"output_formats_total\": {output_formats_total},\n  \
         \"auto_fix_ops_total\": {auto_fix_ops_total}\n\
         }}\n"
    );
    fs::write(target_dir.join("manifest.json"), json)?;
    Ok(())
}

mod examples;

mod counts;

#[cfg(test)]
mod tests;
