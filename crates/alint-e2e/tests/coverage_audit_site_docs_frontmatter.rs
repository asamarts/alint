//! Hard audit: every `docs/site/**/*.md` page MUST have a YAML
//! frontmatter block that parses cleanly via `serde_yaml_ng::from_str`.
//!
//! Why this exists. The `docs/site/` subtree is copied verbatim into
//! the docs-bundle artifact that alint.org pulls at build time. Each
//! page's frontmatter is then parsed by Astro's frontmatter loader
//! (`js-yaml` via `safeParseFrontmatter`). Astro fails the whole
//! `astro build` on any frontmatter parse error, which fails the
//! Cloudflare Pages deploy of alint.org. Because the Cloudflare deploy
//! hook's response only confirms that the build was *accepted into the
//! queue* (returning `{"success": true, "status": "queued"}`), the
//! actual build failure surfaces only in the Cloudflare Pages dashboard
//! — invisible to GitHub Actions, invisible to the alint.org daily
//! check-pins cron, invisible to anyone not actively monitoring CF.
//!
//! Worked example: `docs/site/concepts/variable-interpolation.md` shipped
//! 2026-05-22 (commit 23d76e60) with an unquoted `description:` value
//! containing the substring `"The when: language"`. YAML parses `: `
//! inside a scalar as a mapping-key separator, so the description value
//! got reinterpreted as a malformed `when:` mapping. Result:
//! "bad indentation of a mapping entry" thrown from js-yaml at column
//! 168. **Every Cloudflare build of alint.org failed silently for the
//! next 7 days** until commit 49182bbd quoted the description.
//! Throughout that week the live alint.org served stale v0.10.2-era
//! content while alint repo's docs-bundle.yml workflow happily
//! reported success (it only verifies the deploy-hook ACCEPT, not the
//! actual build outcome).
//!
//! This test catches that whole class at PR time on the alint side. It
//! does NOT validate the body of the page — only the frontmatter — so
//! it stays fast and doesn't duplicate Astro's content-model checks.
//!
//! What it catches:
//! - Unquoted scalar values containing `:<space>` (the canonical
//!   variable-interpolation.md failure).
//! - Missing/extra `---` frontmatter delimiters.
//! - Bad indentation inside multi-line list/mapping values.
//! - Tab-vs-spaces mixing inside frontmatter.
//! - Any other malformation `serde_yaml_ng` rejects.
//!
//! What it deliberately does NOT catch:
//! - Astro/Starlight-specific frontmatter schema (e.g. `sidebar.order`
//!   must be an integer): that's a content-model concern, not a
//!   parser concern. Adding a schema check here would couple this
//!   test to Astro's evolving frontmatter conventions, which is the
//!   wrong dependency direction (alint should be Astro-agnostic).
//! - Page-body Markdown: rendering and link-resolution are Astro's
//!   responsibility. If a page renders blank or has broken anchors,
//!   that's not what this test exists to find.
//!
//! Phase 2 of the alint.org drift-audit follow-up (alint side).

use std::path::PathBuf;

use serde_yaml_ng as serde_yaml;

/// Walk `docs/site/` (relative to the alint repo root) for every `.md`
/// file. The relative path is what gets surfaced in failure messages,
/// so callers see e.g. `docs/site/concepts/variable-interpolation.md`
/// rather than an absolute CARGO_MANIFEST_DIR-rooted path.
fn collect_site_docs() -> Vec<PathBuf> {
    // CARGO_MANIFEST_DIR points at the crate (alint-e2e). The
    // `docs/site/` tree lives at the workspace root, two parents up.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root is two parents up from alint-e2e")
        .to_path_buf();
    let site = root.join("docs").join("site");
    assert!(
        site.is_dir(),
        "expected {} to exist; the site-docs tree moved or this \
         test's path computation is stale",
        site.display(),
    );
    let mut out = Vec::new();
    walk(&site, &root, &mut out);
    out.sort();
    out
}

fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            walk(&p, root, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(p.strip_prefix(root).unwrap().to_path_buf());
        }
    }
}

/// Pull the YAML block between the first two `---` delimiters. Returns
/// `None` for files that have no frontmatter at all; that's the
/// caller's policy decision (we treat it as a failure below, since every
/// site page is expected to have at minimum a `title:`).
///
/// Tolerates both LF and CRLF line endings. Git may check the site docs
/// out with CRLF on Windows, and Astro's frontmatter loader (plus YAML
/// itself) accept CRLF, so a CRLF checkout is not a real frontmatter
/// defect. Matching only `---\n` false-failed the windows-latest lane on
/// every page.
fn extract_frontmatter(source: &str) -> Option<&str> {
    let after_open = source
        .strip_prefix("---\n")
        .or_else(|| source.strip_prefix("---\r\n"))?;
    let close = after_open
        .find("\n---\n")
        .or_else(|| after_open.find("\r\n---\r\n"))?;
    Some(&after_open[..close])
}

#[test]
fn every_site_doc_has_parseable_yaml_frontmatter() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf();
    let docs = collect_site_docs();
    assert!(
        !docs.is_empty(),
        "expected at least one page under docs/site/; if the directory was \
         emptied or moved, update this test's path computation",
    );

    let mut failures: Vec<String> = Vec::new();
    for rel in &docs {
        let abs = root.join(rel);
        let src = std::fs::read_to_string(&abs).unwrap_or_else(|e| panic!("read {rel:?}: {e}"));
        let Some(fm) = extract_frontmatter(&src) else {
            failures.push(format!(
                "{}: missing `---\\n…\\n---\\n` frontmatter block at \
                 file start (Astro requires it; quote the title at minimum)",
                rel.display()
            ));
            continue;
        };
        if let Err(e) = serde_yaml::from_str::<serde_yaml::Value>(fm) {
            failures.push(format!(
                "{}: YAML frontmatter rejected by serde_yaml_ng: {}\n\
                 frontmatter source (between --- delimiters):\n{}",
                rel.display(),
                e,
                fm
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "site docs with invalid frontmatter would break alint.org's \
         Cloudflare build silently (see crate docs):\n\n{}\n\n\
         see crates/alint-e2e/tests/coverage_audit_site_docs_frontmatter.rs \
         for context and the docs/site/concepts/variable-interpolation.md \
         worked example",
        failures.join("\n\n"),
    );
}

/// Every site page declares its own `description:`. A page without one
/// gets Starlight's site-wide description, so it shares its search
/// snippet with every other such page, and two pages that declare the
/// same text have the same problem: in alint.org's Search Console
/// (2026-09) five pages shared the site-wide text. The CLI prose files
/// under `docs/site/cli/` count too: their description becomes the
/// generated page's.
#[test]
fn every_site_doc_has_its_own_description() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf();
    let mut missing: Vec<String> = Vec::new();
    let mut by_description: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for rel in collect_site_docs() {
        let src = std::fs::read_to_string(root.join(&rel)).unwrap();
        // A missing or unparseable block is reported by the test above.
        let Some(Ok(front)) =
            extract_frontmatter(&src).map(serde_yaml::from_str::<serde_yaml::Value>)
        else {
            continue;
        };
        let rel = rel.display().to_string();
        match front
            .get("description")
            .and_then(serde_yaml::Value::as_str)
            .map(str::trim)
        {
            Some(description) if !description.is_empty() => by_description
                .entry(description.to_string())
                .or_default()
                .push(rel),
            _ => missing.push(rel),
        }
    }
    let shared: Vec<String> = by_description
        .into_iter()
        .filter(|(_, pages)| pages.len() > 1)
        .map(|(description, pages)| format!("{pages:?} share {description:?}"))
        .collect();
    assert!(
        missing.is_empty() && shared.is_empty(),
        "site docs need their own `description:` (the search snippet):\n\
         without one: {missing:?}\nshared: {shared:#?}",
    );
}

#[test]
fn extract_frontmatter_recognises_well_formed_delimiters() {
    let src = "---\ntitle: hello\n---\n\nbody\n";
    assert_eq!(extract_frontmatter(src), Some("title: hello"));
}

#[test]
fn extract_frontmatter_returns_none_without_opening_delimiter() {
    let src = "no frontmatter here\n";
    assert_eq!(extract_frontmatter(src), None);
}

#[test]
fn extract_frontmatter_returns_none_without_closing_delimiter() {
    let src = "---\ntitle: hello\nbody never gets a closing delim\n";
    assert_eq!(extract_frontmatter(src), None);
}

#[test]
fn extract_frontmatter_tolerates_crlf_line_endings() {
    // Git on Windows checks the site docs out with CRLF; Astro and YAML
    // both accept it, so this audit must too. Regression: the
    // windows-latest cross-platform lane false-failed every page when
    // extract_frontmatter matched only LF (`---\n`) delimiters.
    let src = "---\r\ntitle: hello\r\nfoo: bar\r\n---\r\n\r\nbody\r\n";
    let fm = extract_frontmatter(src).expect("CRLF frontmatter should be found");
    assert_eq!(fm, "title: hello\r\nfoo: bar");
    serde_yaml::from_str::<serde_yaml::Value>(fm).expect("CRLF frontmatter should parse");
}

/// Regression specifically for the `variable-interpolation.md` failure
/// shape: unquoted scalar value containing `:<space>`. If
/// `serde_yaml_ng` ever changes its behaviour to accept this (it
/// shouldn't — the YAML 1.2 spec is unambiguous), we want to know.
#[test]
fn unquoted_scalar_with_colon_space_is_rejected_by_yaml_parser() {
    // The literal value pattern from the original failure:
    // description: ... The when: language gains ...
    let fm = "title: ok\n\
              description: lead-in text. The when: language continues.\n";
    let parsed: Result<serde_yaml::Value, _> = serde_yaml::from_str(fm);
    assert!(
        parsed.is_err(),
        "serde_yaml_ng started accepting unquoted scalars with embedded \
         `:<space>`; this test's premise (and the alint.org-side bug it \
         exists to prevent) needs revisiting"
    );
}
