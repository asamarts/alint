//! Per-rule and per-family reference page generation.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use alint_testkit::PolicyDocsCase;
use anyhow::{Context, Result, bail};

use crate::rule_options_table::{build_kind_branch_index, load_config_schema, options_section};

use super::{FAIL_ONLY_KINDS, docs_paths, examples, requires_fail_only_docs};

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
pub(super) fn generate_rules_pages(
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
    // A kind only a `version: 2` config may use (`constraint`) has its branch
    // in the draft version 2 schema alone.
    let v2_schema = load_config_schema(&workspace.join(docs_paths::SCHEMA_V2_JSON))?;
    let v2_branches = build_kind_branch_index(&v2_schema);
    let schemas = [(&schema, &kind_branches), (&v2_schema, &v2_branches)];

    // ADR-0014: scenarios carrying a `docs:` block render their kind's worked
    // example from the real fixture (tree + config + a live `alint check` run).
    // Empty until a kind opts in, so every other kind keeps its hand-written
    // rules.md example.
    let documented = examples::render_documented(workspace, capture_output)?;

    let rules_dir = target_dir.join("rules");
    fs::create_dir_all(&rules_dir)?;

    let mut kind_to_family: HashMap<String, String> = HashMap::new();
    let mut all_kinds: Vec<KindEntry> = Vec::new();
    // Per family, for the second-pass family Overview render (categories-based
    // membership needs all_kinds complete first).
    let mut families_meta: Vec<FamilyMeta> = Vec::new();
    // Per-rule H3 sections in `docs/rules.md` must be backed by a documented
    // `docs:` example fixture (ADR-0014 Phase 4). Accumulated across all families
    // and surfaced as a single hard failure so a docs PR sees every missing
    // example at once instead of fixing them one at a
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
            &schemas,
            &mut kind_to_family,
            &mut all_kinds,
            &mut missing_examples,
            &mut wrong_kind_examples,
            &documented,
            &registry,
            released,
        )?;
        families_meta.push(FamilyMeta {
            title: h2.title.clone(),
            order: family_order,
            slug: family_slug,
            intro: crate::family_index::family_intro(&h2.body),
        });
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
    enforce_fail_only_allowlist_live(&documented, &registry)?;
    enforce_documented_page_targets(&documented, &kind_to_family)?;

    let family_summaries = emit_family_indexes(&rules_dir, &families_meta, &all_kinds)?;
    emit_rules_master_index(&rules_dir, &all_kinds, &family_summaries, aliases.len())?;
    Ok(kind_to_family)
}

/// One rule family as `generate_rules_pages` found it in `docs/rules.md`.
struct FamilyMeta {
    title: String,
    order: u32,
    slug: String,
    intro: Option<String>,
}

/// Family Overview pages: categories-based membership. Each family lists every
/// kind whose `**Categories:**` line includes it (by slug), not just the kinds
/// physically under its H2. At single-membership this equals the directory
/// membership; at Phase-3 multi-membership it cross-lists automatically.
fn emit_family_indexes(
    rules_dir: &Path,
    families: &[FamilyMeta],
    all_kinds: &[KindEntry],
) -> Result<Vec<FamilySummary>> {
    let mut summaries = Vec::new();
    for family in families {
        let rules: Vec<RuleEntry> = all_kinds
            .iter()
            .filter(|k| k.categories.contains(&family.slug))
            .map(|k| RuleEntry {
                kind: k.kind.clone(),
                summary: k.summary.clone(),
                family_slug: k.family_slug.clone(),
            })
            .collect();
        emit_family_index(
            &rules_dir.join(&family.slug),
            &family.title,
            family.order,
            family.intro.as_deref(),
            &rules,
        )?;
        summaries.push(FamilySummary {
            title: family.title.clone(),
            slug: family.slug.clone(),
            rule_count: rules.len(),
        });
    }
    Ok(summaries)
}

/// Walk every H3 in a family, emit per-rule pages, and collect
/// the family's rule list for later index generation. Split out
/// of `generate_rules_pages` because clippy's `too_many_lines`
/// flagged the original — and even ignoring that, "process one
/// family" is its own logical chunk worth naming.
// Threads the read-only registry/schema context plus the four
// accumulators through one family's H3 sections; bundling these into
// a struct would obscure more than it clarifies.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn process_family_h3s(
    h2: &H2Section,
    family_dir: &Path,
    family_slug: &str,
    known_kinds: &std::collections::HashSet<String>,
    schemas: &[(
        &serde_json::Value,
        &std::collections::HashMap<String, serde_json::Value>,
    )],
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
        // Phase 4 hard-flip (ADR-0014): every rule H3 MUST be backed by a
        // documented `docs:` scenario - its worked example is a real `alint check`
        // run, not a hand-written ```yaml block. The legacy "...or an inline yaml
        // example" escape is retired now that all kinds are migrated, so a new
        // rule kind can no longer ship with only a hand-written (unverified)
        // example. Multi-kind headings (e.g. the structured-query path_equals
        // kinds) must have every kind documented. Canonicalise so an alias-titled
        // token resolves to its documented (canonical) kind. Gaps are surfaced
        // collectively at the end of generate_rules_pages.
        // ADR-0014 "both states, always": every documented kind needs fail and
        // pass examples. A metadata-declared host-keyring dependency is the
        // explicit exemption because the fixture sandbox cannot construct its
        // compliant state.
        for k in &group_kinds {
            let canon = registry.canonical_kind(k);
            let cases = documented.get(canon);
            let has = |case: PolicyDocsCase| {
                cases.is_some_and(|examples| examples.iter().any(|example| example.case == case))
            };
            let missing = if !has(PolicyDocsCase::Fail) {
                Some("fail")
            } else if !has(PolicyDocsCase::Pass) && !requires_fail_only_docs(registry, canon) {
                Some("pass")
            } else {
                None
            };
            if let Some(which) = missing {
                missing_examples.push(format!(
                    "{} → {} (missing {which} example)",
                    h2.title, canon
                ));
            }
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
        if let Some(ex_kind) = example_first_kind(&h3.body)
            && !group_kinds.contains(&ex_kind)
        {
            wrong_kind_examples.push(format!(
                "{} → {}: example uses `kind: {}`, but this H3 documents `{}`; \
                     use the canonical name (the alias still works in user configs)",
                h2.title,
                h3.title,
                ex_kind,
                group_kinds.join("` / `"),
            ));
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
        // Release-gate the prose the same way the Options table is gated:
        // drop `<!-- alint:since=X -->` blocks describing capability newer than
        // the released version. Computed once per H3 (a multi-kind heading's
        // siblings share it). See ADR-0007 / documentation-drift.md P1. The
        // summary is taken from the gated body, so an unreleased opening
        // paragraph never becomes the index summary.
        let page_body = strip_unreleased_prose(&clean_body, released);
        let summary = first_sentence(&page_body);
        for kind in &group_kinds {
            kind_order += 1;
            let siblings: Vec<&str> = group_kinds
                .iter()
                .filter(|k| *k != kind)
                .map(String::as_str)
                .collect();
            // The first schema with the kind's branch: version 1's, then the
            // draft version 2 schema's.
            let options_md = schemas.iter().find_map(|(schema, branches)| {
                branches
                    .get(kind)
                    .map(|branch| options_section(branch, schema, released))
            });
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
pub(super) struct H3Section {
    pub(super) title: String,
    pub(super) body: String,
}

pub(super) fn split_h3_sections(src: &str) -> Vec<H3Section> {
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
pub(super) fn extract_kinds(h3_title: &str) -> Vec<String> {
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
/// The opening sentence of a rule's `docs/rules.md` prose: the first paragraph
/// (code blocks skipped, wrapped lines joined) cut at the first `". "` whose
/// preceding word is NOT a known abbreviation (`e.g.`, `i.e.`, `vs.`, …). The
/// abbreviation guard keeps a clause like "differ only by case (e.g. …)" from
/// being cut at the "g." (`no_case_conflicts`); a decimal like "v1.2." is not
/// mistaken for an abbreviation and correctly ends the sentence. Markdown is
/// left intact — the website rule index renders it directly; `kind_summary`
/// (terminal) and `rule_meta_description` (SERP) strip it for their surfaces.
/// A `<!-- alint:since=X -->` or `<!-- /alint:since -->` marker line is skipped,
/// so a paragraph wrapped in a since-block never puts the marker in a summary.
pub(super) fn first_sentence(body: &str) -> String {
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
        if trimmed.starts_with("<!-- alint:since=") || trimmed == "<!-- /alint:since -->" {
            continue;
        }
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

/// Drop trailing "stop words" (articles / prepositions / conjunctions) that a
/// hard word-cap can leave dangling right before the "..." marker, re-trimming
/// punctuation each pass, e.g. a summary cut after "matching the" reads better as
/// "matching". Auxiliaries ("is"/"are"/"be") are NOT dropped, since "must be"
/// reads worse as "must".
fn drop_trailing_stopwords(s: &str) -> String {
    const STOP: &[&str] = &[
        "a", "an", "the", "and", "or", "of", "to", "in", "on", "at", "for", "with", "by", "from",
        "as", "that", "this", "its", "into", "than", "per",
    ];
    const TRIM: [char; 7] = [',', ';', ':', ' ', '(', '"', '\''];
    let mut t = s.trim_end_matches(TRIM);
    while let Some(sp) = t.rfind(' ') {
        if STOP.contains(&t[sp + 1..].to_ascii_lowercase().as_str()) {
            t = t[..sp].trim_end_matches(TRIM);
        } else {
            break;
        }
    }
    t.to_string()
}

/// Terminal-ready one-line summary of a rule kind's `docs/rules.md` prose, for
/// the in-binary docs bridge (ADR-0011). Unlike [`first_sentence`] (which feeds
/// the website rule index and leaves markdown intact), this yields clean
/// plaintext: the same opening sentence, with `**bold**` / `*italic*` and inline
/// backticks stripped, em/en-dashes folded and arrows ASCII-ized, whitespace
/// collapsed, a colon/comma lead-in trimmed, and length-capped with an honest
/// `...` marker. Input is the `**Categories:**`-stripped H3 body.
pub(crate) fn kind_summary(clean_body: &str, max_chars: usize) -> String {
    let sentence = strip_markup(&first_sentence(clean_body));
    truncate_meta_summary(&sentence, max_chars)
}

/// Plain-text summary capped at a word boundary. Unlike `meta_desc_clean`'s
/// sentence-aware cap, this helper is used after the caller has already chosen
/// one sentence, so it never mistakes an abbreviation (`e.g.`) for a safe
/// stopping point. Any shortened summary ends in `...`; generated metadata
/// must not turn a clipped clause into a falsely complete sentence.
pub(super) fn truncate_meta_summary(raw: &str, max_chars: usize) -> String {
    let cleaned = meta_desc_clean(raw, usize::MAX)
        .trim_end_matches([':', ';', ',', ' '])
        .to_string();
    if cleaned.chars().count() <= max_chars {
        return cleaned;
    }

    let content_budget = max_chars.saturating_sub(3);
    let hard_cap: String = cleaned.chars().take(content_budget).collect();
    let word_cap = hard_cap
        .rfind(' ')
        .map_or(hard_cap.as_str(), |space| &hard_cap[..space]);
    let quote_balanced = match (word_cap.matches('"').count() % 2, word_cap.rfind('"')) {
        (1, Some(q)) => &word_cap[..q],
        _ => word_cap,
    };
    let balanced = tidy_truncated_tail(quote_balanced);
    let trimmed = drop_trailing_stopwords(&balanced);
    if trimmed.is_empty() {
        return ".".repeat(max_chars.min(3));
    }
    format!("{trimmed}...")
}

/// Strip markdown emphasis (`**bold**`, `*italic*`) and fold non-ASCII arrows to
/// ASCII, so the terminal summary is plain text. A bare, unbalanced `*` (e.g. the
/// `*_equals` wildcard family) is left intact.
pub(super) fn strip_markup(s: &str) -> String {
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
    // A `,`/`;`/`:` sitting directly before a period is an artifact, never real
    // prose: a backtick-wrapped term that ended in a colon (`allow:` → `allow:.`
    // once the backticks are gone) or a list lead-in that had the composed
    // "alint <kind> rule" suffix appended after its trailing colon. Collapse to
    // the period so the SERP line reads cleanly.
    s = s.replace(":.", ".").replace(";.", ".").replace(",.", ".");
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= max_chars {
        return s;
    }
    // Over the cap: prefer cutting at the last sentence end that
    // still fits; else cut at the last word boundary that fits.
    let truncated: String = s.chars().take(max_chars).collect();
    let cut = if let Some(idx) = truncated.rfind(". ") {
        &truncated[..=idx]
    } else if let Some(idx) = truncated.rfind(' ') {
        &truncated[..idx]
    } else {
        truncated.as_str()
    };
    tidy_truncated_tail(cut.trim())
}

/// Repair the tail a hard length cap can leave mid-clause. A word-boundary cut
/// inside a parenthetical strands an unclosed `(` (`… comparator (lexical /`,
/// `ordered_block`); dropping from the last unmatched `(` rebalances it, and
/// trimming trailing separators/operators — including a folded arrow half like
/// `=>` (`command_idempotent`'s `⇒`) — removes what the cut left dangling. A cut
/// at a real sentence end (trailing `.`) is already balanced and passes through
/// untouched.
fn tidy_truncated_tail(s: &str) -> String {
    let mut t = s.to_string();
    while t.matches('(').count() > t.matches(')').count() {
        let Some(p) = t.rfind('(') else { break };
        t.truncate(p);
        t = t.trim_end().to_string();
    }
    t.trim_end_matches([
        ' ', '\t', ',', ';', ':', '/', '-', '&', '|', '(', '<', '>', '=',
    ])
    .to_string()
}

/// Build the per-rule SERP description: lead with the rule kind (the search
/// query) and family (disambiguator), then explain what it checks from its own
/// first sentence. Cap at 158 characters and use no em-dashes.
/// Falls back to a family-scoped sentence when the rule body's
/// opening line is too terse to stand alone.
pub(super) fn rule_meta_description(kind: &str, family_title: &str, body: &str) -> String {
    // `first_sentence` is abbreviation-aware, so a clause like "differ only by
    // case (e.g. ...)" is not cut at the "g." (`no_case_conflicts`). A first
    // sentence that is a bare list lead-in ending in a colon
    // (`no_illegal_windows_names`: "Reject path components Windows can't
    // represent:") is trimmed before it is composed into the description.
    // `strip_markup` first, so `**bold**` / `*italic*` emphasis and unicode
    // arrows don't leak into the plain-text SERP snippet as literal `**regex**`
    // (`*_path_matches`) or `⇒` (`command_idempotent`) — the website rule index
    // keeps the markdown (it renders it), but a `<meta>` description must not.
    let mut opening = strip_markup(&first_sentence(body));
    // Multi-format structured-query sections share prose. Give every operation
    // a concise, format-specific opener so each page is useful on its own (and
    // use the correct article for initialisms such as INI, HCL, and XML).
    if let Some((format, operation)) = kind.rsplit_once("_path_") {
        let label = match format {
            "dotenv" => "dotenv".to_string(),
            "properties" => "Java properties".to_string(),
            other => other.to_uppercase(),
        };
        let article = if matches!(format, "hcl" | "ini" | "xml") {
            "an"
        } else {
            "a"
        };
        opening = match operation {
            "equals" => format!(
                "Query {article} {label} document with JSONPath and require every selected \
                 value to equal the expected value."
            ),
            "matches" => format!(
                "Query {article} {label} document with JSONPath and require every selected \
                 string to match a regular expression."
            ),
            "absent" => format!(
                "Query {article} {label} document with JSONPath and require it to match nothing."
            ),
            _ => opening,
        };
    }
    let family = family_title.to_lowercase();
    let lead = format!("alint {kind} rule ({family})");
    let opening = meta_desc_clean(&opening, usize::MAX)
        .trim_end_matches([':', ';', ','])
        .to_string();
    if opening.chars().count() < 25 {
        // Doc-comment opener too thin to be a useful snippet —
        // fall back to a kind + family clause (still concrete:
        // names the rule the searcher typed and where it lives).
        return format!("{lead}.");
    }
    // Reserve `: ` plus a final period in case the source sentence has none.
    let budget = 158usize.saturating_sub(lead.chars().count() + 3);
    let summary = truncate_meta_summary(&opening, budget);
    let punctuation = if summary.ends_with('.') || summary.ends_with('!') || summary.ends_with('?')
    {
        ""
    } else {
        "."
    };
    format!("{lead}: {summary}{punctuation}")
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
pub(super) fn lead_example_with_kind(body: &str, kind: &str) -> String {
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
pub(super) fn example_first_kind(body: &str) -> Option<String> {
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
pub(super) fn example_block_kinds(body: &str) -> Vec<String> {
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
/// `missing` = H3 sections not backed by a documented example fixture;
/// `wrong_kind` = examples whose top-level `kind:` is an alias (or otherwise not
/// the H3's canonical kind). Enforced (not warned) so a regressing docs/rules.md
/// fails the docs-bundle build before it can publish a broken example to
/// alint.org. Split out of `generate_rules_pages` to keep that orchestrator
/// within the clippy line budget.
fn enforce_example_gates(missing: &[String], wrong_kind: &[String]) -> Result<()> {
    if !missing.is_empty() {
        anyhow::bail!(
            "{} rule kind H3 section(s) in docs/rules.md are not backed by a \
             documented example fixture:\n  - {}\n\n\
             Every rule kind's worked example renders from a `docs:` scenario in \
             crates/alint-e2e/scenarios (ADR-0014, Phase 4): add a documented \
             fail+pass scenario for the kind. A hand-written ```yaml block no \
             longer satisfies this gate.",
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

/// Keep the metadata-derived fail-only exemption honest. The former hand list
/// remains an exact one-release transition check, and every derived entry must
/// be load-bearing: canonical, documented with `fail`, and without `pass`.
fn enforce_fail_only_allowlist_live(
    documented: &std::collections::BTreeMap<String, Vec<examples::RenderedExample>>,
    registry: &alint_core::RuleRegistry,
) -> Result<()> {
    let derived: std::collections::BTreeSet<&str> = registry
        .kind_metadata()
        .filter(|(_kind, meta)| {
            meta.capabilities
                .iter()
                .any(|entry| matches!(entry.capability, alint_core::Capability::HostKeyring))
        })
        .map(|(kind, _meta)| kind)
        .collect();
    let transition: std::collections::BTreeSet<&str> = FAIL_ONLY_KINDS.iter().copied().collect();
    anyhow::ensure!(
        derived == transition,
        "KindMeta::HostKeyring and the one-release FAIL_ONLY_KINDS transition list disagree: \
         metadata={derived:?}, transition={transition:?}"
    );

    let mut stale: Vec<String> = Vec::new();
    for kind in derived {
        let canon = registry.canonical_kind(kind);
        let cases = documented.get(canon);
        let has = |case: PolicyDocsCase| {
            cases.is_some_and(|examples| examples.iter().any(|example| example.case == case))
        };
        let reason = if registry.canonical_kind(kind) != kind {
            Some(format!(
                "not a canonical kind (resolves to `{}`)",
                registry.canonical_kind(kind)
            ))
        } else if !has(PolicyDocsCase::Fail) {
            Some("no documented `fail` example".to_string())
        } else if has(PolicyDocsCase::Pass) {
            Some("has a `pass` example, so the exemption is unnecessary".to_string())
        } else {
            None
        };
        if let Some(why) = reason {
            stale.push(format!("`{kind}`: {why}"));
        }
    }
    if !stale.is_empty() {
        stale.sort();
        bail!(
            "FAIL_ONLY_KINDS has {} stale entr(y/ies) - remove so the fail+pass pairing gate \
             stays un-suppressed for kinds that no longer need the exemption:\n  - {}",
            stale.len(),
            stale.join("\n  - "),
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
pub(super) fn strip_unreleased_prose(
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
    intro: Option<&str>,
    rules: &[RuleEntry],
) -> Result<()> {
    fs::write(
        family_dir.join("index.md"),
        crate::family_index::render(family_title, family_order, intro, rules),
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
    let description = match slug {
        "fix-operations" => {
            "Configure alint's automatic fix operations, applicability tiers, and execution safeguards."
        }
        "nested-configs" => {
            "Layer .alint.yml files across a monorepo with deterministic inheritance, trust boundaries, and root-relative behavior."
        }
        _ => "Understand this alint configuration concept and its repository-wide behavior.",
    };
    let _ = writeln!(
        &mut page,
        "description: '{}'",
        escape_yaml_string(description)
    );
    let _ = writeln!(&mut page, "---");
    let _ = writeln!(&mut page);
    if let Some(view) = concept_view_id(slug) {
        let _ = writeln!(&mut page, "<likec4-view view-id=\"{view}\"></likec4-view>");
        let _ = writeln!(&mut page);
    }
    page.push_str(&promote_concept_headings(body.trim_start_matches('\n')));
    if !page.ends_with('\n') {
        page.push('\n');
    }
    fs::write(dir.join(format!("{slug}.md")), page)?;
    Ok(())
}

/// A concept page is extracted from an H2 section in `docs/rules.md`; its H3
/// children become the standalone page's top-level sections. Shift headings one
/// level up while leaving fenced examples byte-for-byte unchanged.
fn promote_concept_headings(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut in_fence = false;
    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
        }
        let promoted = if !in_fence && trimmed.starts_with("### ") {
            line.replacen("### ", "## ", 1)
        } else if !in_fence && trimmed.starts_with("#### ") {
            line.replacen("#### ", "### ", 1)
        } else {
            line.to_string()
        };
        out.push_str(&promoted);
        out.push('\n');
    }
    out
}

/// Sections of a markdown document split at H2 headers (`## …`).
/// Anything before the first H2 is dropped (it's typically the
/// document's H1 + intro paragraph; we don't carry that into the
/// per-family pages).
pub(super) struct H2Section {
    pub(super) title: String,
    pub(super) body: String,
}

pub(super) fn split_h2_sections(src: &str) -> Vec<H2Section> {
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
