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

use crate::{git_sha, now_iso, walkdir_plain, workspace_root};

/// One-release transition list for kinds whose compliant docs scenario requires
/// a host keyring. The operational exemption derives from `KindMeta`; this list
/// remains an exact cross-check until the v0.19 transition is removed.
const FAIL_ONLY_KINDS: &[&str] = &["git_commit_gpg_signed"];

fn requires_fail_only_docs(registry: &alint_core::RuleRegistry, kind: &str) -> bool {
    registry.meta(kind).is_some_and(|meta| {
        meta.capabilities
            .iter()
            .any(|entry| matches!(entry.capability, alint_core::Capability::HostKeyring))
    })
}

// ---- docs-export ----------------------------------------------------------

/// Every `enum Command` variant in `crates/alint/src/cli.rs` MUST
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
    "test",
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
    /// The draft version 2 schema, which alone has the kinds only a
    /// `version: 2` config may use.
    pub const SCHEMA_V2_JSON: &str = "schemas/v2/config.json";
    pub const FACTS_JSON: &str = "facts.json";
    pub const ROADMAP_JSON: &str = "roadmap.json";
    pub const CRATE_GRAPH_MD: &str = "docs/design/architecture/crate-graph.md";
    pub const MODEL_DIR: &str = "docs/design/architecture/model";
    pub const RULESETS_DIR: &str = "crates/alint-dsl/rulesets";
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

/// Whether the per-rule reference pages render each documented example's live
/// `alint check` output (ADR-0014 Phase 5). The full export and the
/// `--rules-only` docs-bundle bridge MUST pass the same value: a doc-only
/// refresh has to deploy pages byte-identical to a release-tag export, so the
/// bridge cannot omit the output a release build would emit. `--check` exercises
/// only the full path, so it would not catch the two call sites diverging — a
/// single source of truth keeps them from silently drifting.
const RULE_PAGES_CAPTURE_OUTPUT: bool = true;

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
        // The docs-bundle rule-page bridge overlays ONLY the per-rule reference
        // pages from main, so generate just those and skip the rest of the
        // export (`copy_site_tree`, `generate_cli_reference`, the arch diagrams,
        // …). It still passes RULE_PAGES_CAPTURE_OUTPUT, so each rule page's
        // `alint check` block renders from a real run of the freshly built
        // binary and a doc-only refresh deploys exactly what a release export
        // would — not a stripped-down tree+config (ADR-0014 Phase 5). One
        // release build is the cost; what --rules-only still saves is
        // `generate_cli_reference`'s SECOND build, which the bridge never reads.
        generate_rules_pages(&workspace, &target_dir, released, RULE_PAGES_CAPTURE_OUTPUT)?;
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
    let kind_to_family =
        generate_rules_pages(&workspace, &target_dir, released, RULE_PAGES_CAPTURE_OUTPUT)?;

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

    // 4c'. SERP descriptions for the pages exported verbatim from repo docs.
    for (rel, description) in exported_pages::DESCRIPTIONS {
        exported_pages::set_frontmatter_description(&target_dir.join(rel), description)?;
    }

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
        crate::docs_checks::check_no_invisible_controls(&target_dir)?;
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
        if is_markdown {
            // Release-gate hand-written docs the same way the rule pages are:
            // strip `<!-- alint:since=X -->` blocks newer than the released
            // version, and unwrap the comment markers unconditionally so no
            // sentinel comment reaches a published bundle (even when built
            // without `--released-version` from a release tag). A no-op for
            // the unmarked majority (the stripper fast-paths files with no
            // marker). See ADR-0007 / docs/design/v0.14/documentation-drift.md P-REF.
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
    let rule_kinds_total = counts::count_canonical_rule_kinds();
    let rule_aliases_total = counts::count_rule_aliases();
    let bundled_rulesets_total = counts::count_canonical_bundled_rulesets()?;
    let subcommands_total = counts::count_canonical_subcommands()?;
    let output_formats_total = counts::count_canonical_output_formats()?;
    // Infallible (reads `FixSpec::ALL_OP_NAMES`, a compile-time constant), so no `?`.
    let auto_fix_ops_total = counts::count_canonical_auto_fix_ops();

    // format_version BUMPED 3 -> 4 for the canonical-kind/alias split.
    // alint.org widens its accepted-versions set before this producer-side
    // change lands, following the compatibility-first v3 rollout.
    let json = format!(
        "{{\n  \
         \"alint_version\": \"{version}\",\n  \
         \"git_sha\": \"{sha}\",\n  \
         \"generated_at\": \"{now}\",\n  \
         \"format_version\": 4,\n  \
         \"rule_kinds_total\": {rule_kinds_total},\n  \
         \"rule_aliases_total\": {rule_aliases_total},\n  \
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
// Shared with `docs_checks::check_no_invisible_controls` so the recurrence gate
// forbids exactly the set the generator escapes (the single source of truth for
// "dangerous in a docs page").
pub(crate) use examples::is_dangerous_docs_char;

mod cli;
mod counts;
mod exported_pages;
mod rule_pages;
mod rulesets;

pub(crate) use cli::cli_view;
use cli::generate_cli_reference;
pub(crate) use rule_pages::{
    RuleEntry, concept_view_id, escape_yaml_string, kind_summary, slugify,
};
use rule_pages::{generate_rules_pages, strip_unreleased_prose};
use rulesets::generate_bundled_ruleset_pages;
#[cfg(test)]
use rulesets::ruleset_meta_description;
#[cfg(test)]
pub(crate) use rulesets::{first_overview_sentence, render_overview_from_comments};

#[cfg(test)]
use cli::format_top_help;
#[cfg(test)]
use rule_pages::{
    example_block_kinds, example_first_kind, extract_kinds, first_sentence, lead_example_with_kind,
    meta_desc_clean, rule_meta_description, split_h2_sections, split_h3_sections, strip_markup,
    truncate_meta_summary,
};

#[cfg(test)]
mod tests;
