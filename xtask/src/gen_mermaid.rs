//! `xtask gen-mermaid` — assemble the GitHub-facing architecture diagram
//! gallery (`docs/design/architecture/DIAGRAMS.md`) from the `LikeC4` model,
//! gated by `--check` (mirrors `gen-model`/`gen-arch`).
//!
//! The interactive, themed diagrams live on alint.org via the `<likec4-view>`
//! web component, which GitHub's markdown sanitizer strips. So for readers of
//! the `.md` files on GitHub we render the same views as native Mermaid
//! flowcharts, generated reproducibly from the one model: `likec4 gen mermaid`
//! emits one `.mmd` per view, and this command assembles the curated set into a
//! single document. The curation + section order + headings come from the
//! alint.org gallery page (`docs/site/about/architecture-diagrams.md`), so the
//! GitHub and alint.org galleries can't drift apart; the diagram bodies come
//! from the model. `gen-mermaid --check` byte-gates the committed result.
//!
//! Like `likec4 validate` (`ci/scripts/likec4.sh`), this needs Node/`npx`. The
//! self-hosted CI runner ships Node 22 (since #90), so `--check` is a live,
//! enforced byte-gate there; only a local dev box without Node falls back to a
//! loud SKIP. The likec4 version is pinned for reproducibility.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

const LIKEC4_VERSION: &str = "1.58.0";
const MODEL_DIR: &str = "docs/design/architecture/model";
const GALLERY: &str = "docs/site/about/architecture-diagrams.md";
const DIAGRAMS_MD: &str = "docs/design/architecture/DIAGRAMS.md";
/// Scratch dir for the per-view `.mmd` files (under `target/`, gitignored).
const OUT_DIR: &str = "target/xtask/mermaid";

pub fn run(check: bool) -> Result<()> {
    let root = crate::workspace_root()?;

    if !npx_available() {
        if check {
            println!(
                "[gen-mermaid] WARN: Node/npx not found - SKIPPING {DIAGRAMS_MD} freshness check."
            );
            println!(
                "[gen-mermaid] Install Node >= 20 to run this gate locally (CI's runner already has it)."
            );
            return Ok(());
        }
        bail!("gen-mermaid needs Node/npx (for `likec4 gen mermaid`); install Node >= 20");
    }

    let sections = gallery_sections(
        &fs::read_to_string(root.join(GALLERY)).with_context(|| format!("read {GALLERY}"))?,
    );
    if sections.is_empty() {
        bail!("no `## heading` + <likec4-view> sections found in {GALLERY}");
    }

    let out_dir = root.join(OUT_DIR);
    let _ = fs::remove_dir_all(&out_dir);
    fs::create_dir_all(&out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    generate_mmd(&root, &out_dir)?;

    let mut rendered_sections: Vec<(String, String)> = Vec::with_capacity(sections.len());
    for (heading, view_id) in &sections {
        let mmd = out_dir.join(format!("{view_id}.mmd"));
        let text = fs::read_to_string(&mmd).with_context(|| {
            format!(
                "read {} (view '{view_id}' from {GALLERY} has no model view?)",
                mmd.display()
            )
        })?;
        rendered_sections.push((heading.clone(), classic_mermaid(&strip_frontmatter(&text))));
    }
    let rendered = render_doc(&rendered_sections);

    let path = root.join(DIAGRAMS_MD);
    if check {
        let committed =
            fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        if committed != rendered {
            bail!(
                "{DIAGRAMS_MD} is stale. Run `cargo run -p xtask -- gen-mermaid` to regenerate \
                 and commit the result."
            );
        }
        println!("{DIAGRAMS_MD} is up to date ({} diagrams)", sections.len());
        return Ok(());
    }
    fs::write(&path, &rendered).with_context(|| format!("write {}", path.display()))?;
    println!("wrote {DIAGRAMS_MD} ({} diagrams)", sections.len());
    Ok(())
}

/// Is `npx` on PATH (i.e. can we run `likec4`)?
fn npx_available() -> bool {
    Command::new("npx")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// `likec4 gen mermaid <model> -o <out_dir>` — one `<viewId>.mmd` per view.
fn generate_mmd(root: &Path, out_dir: &Path) -> Result<()> {
    let status = Command::new("npx")
        .args([
            "-y",
            &format!("likec4@{LIKEC4_VERSION}"),
            "gen",
            "mermaid",
            // Pin the wasm layouter. likec4 auto-detects a container and there
            // flips `--use-dot` to true (shelling out to a `dot` binary the
            // slim runner image doesn't ship) — which fails, and would anyway
            // diverge from the committed wasm-laid-out diagrams. `--no-use-dot`
            // forces wasm everywhere (local + CI), so the gate is reproducible
            // and dependency-free. See docs/design/architecture-diagrams.md.
            "--no-use-dot",
            MODEL_DIR,
            "-o",
        ])
        .arg(out_dir)
        .current_dir(root)
        .status()
        .context("spawn `npx likec4 gen mermaid`")?;
    if !status.success() {
        bail!("`likec4 gen mermaid` failed (exit {:?})", status.code());
    }
    Ok(())
}

/// Ordered `(heading, view-id)` pairs from the gallery: each `## heading`
/// followed by its `<likec4-view view-id="...">`. This is the curation source
/// for the GitHub gallery, so it mirrors the alint.org one exactly.
fn gallery_sections(md: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut heading: Option<String> = None;
    for line in md.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            heading = Some(h.trim().to_string());
        } else if let (Some(h), Some(id)) = (heading.as_ref(), view_id_in_line(line)) {
            out.push((h.clone(), id));
        }
    }
    out
}

/// The `view-id="..."` value in a line, if any.
fn view_id_in_line(line: &str) -> Option<String> {
    let after = line.split_once("view-id=\"")?.1;
    let end = after.find('"')?;
    let id = &after[..end];
    (!id.is_empty()).then(|| id.to_string())
}

/// Drop a leading `---\ntitle: ...\n---\n` Mermaid frontmatter block (the
/// model's view title; we use the gallery heading instead), returning the
/// trimmed flowchart body.
fn strip_frontmatter(text: &str) -> String {
    let body = text
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map_or(text, |(_, body)| body);
    body.trim().to_string()
}

/// likec4 1.58 emits the Mermaid v11.3 node-metadata syntax
/// (`ID@{ shape: rectangle, label: "X" }`). GitHub and other renderers lag the
/// latest Mermaid, so rewrite each node to the classic `ID["X"]` rectangle,
/// which renders on Mermaid 8.x+. Every emitted node is a rectangle; any other
/// shape (none today) would be left in the newer syntax.
fn classic_mermaid(body: &str) -> String {
    body.replace("@{ shape: rectangle, label: ", "[")
        .replace("\" }", "\"]")
}

fn render_doc(sections: &[(String, String)]) -> String {
    let mut out = String::new();
    out.push_str(
        "<!-- GENERATED by `cargo run -p xtask -- gen-mermaid`. Do not edit by hand. -->\n",
    );
    out.push_str("# alint architecture diagrams\n\n");
    out.push_str(
        "These flowcharts are generated from the single LikeC4 architecture model \
         (`docs/design/architecture/model/`) and render natively on GitHub. The same \
         model powers the interactive, themed versions on alint.org:\n",
    );
    out.push_str("<https://alint.org/docs/about/architecture-diagrams/>\n\n");
    out.push_str(
        "Regenerate with `cargo run -p xtask -- gen-mermaid` (or `--check` to gate); \
         pinned to likec4 ",
    );
    out.push_str(LIKEC4_VERSION);
    out.push_str(".\n");
    for (heading, body) in sections {
        out.push_str("\n## ");
        out.push_str(heading);
        out.push_str("\n\n```mermaid\n");
        out.push_str(body.trim_end());
        out.push_str("\n```\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gallery_sections_pairs_headings_with_views() {
        let md = "intro\n## System context\n\n<likec4-view view-id=\"index\"></likec4-view>\n\n\
                  ## Containers\ncaption\n<likec4-view view-id=\"containers\"></likec4-view>\n";
        assert_eq!(
            gallery_sections(md),
            vec![
                ("System context".to_string(), "index".to_string()),
                ("Containers".to_string(), "containers".to_string()),
            ]
        );
    }

    #[test]
    fn strip_frontmatter_drops_title_block() {
        let mmd = "---\ntitle: \"alint check\"\n---\ngraph LR\n  A --> B\n";
        assert_eq!(strip_frontmatter(mmd), "graph LR\n  A --> B");
        // No frontmatter: returned as-is (trimmed).
        assert_eq!(
            strip_frontmatter("graph LR\n  A --> B\n"),
            "graph LR\n  A --> B"
        );
    }

    #[test]
    fn classic_mermaid_downgrades_node_syntax() {
        let v11 = "graph LR\n  Dev@{ shape: rectangle, label: \"Developer\" }\n  \
                   Eng@{ shape: rectangle, label: \"CI / build\" }";
        assert_eq!(
            classic_mermaid(v11),
            "graph LR\n  Dev[\"Developer\"]\n  Eng[\"CI / build\"]"
        );
    }

    #[test]
    fn view_id_in_line_extracts_attr() {
        assert_eq!(
            view_id_in_line("<likec4-view view-id=\"checkFlow\"></likec4-view>").as_deref(),
            Some("checkFlow")
        );
        assert_eq!(view_id_in_line("## Heading"), None);
    }
}
