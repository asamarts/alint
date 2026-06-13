//! `xtask gen-roadmap` — emit `roadmap.json`, the machine-readable
//! public-roadmap contract the alint.org `/roadmap/` timeline renders.
//!
//! Mirrors `gen_facts`: the phase list is derived here from
//! `docs/design/ROADMAP.md` so the marketing page can't drift from the
//! canonical roadmap. A public phase is a `## ` heading immediately
//! followed by a `<!-- roadmap-public: blurb="..." -->` marker; the
//! version and title come from the heading, the one-line blurb from the
//! marker.
//!
//! Status (shipped vs planned) is deliberately NOT stored. alint.org
//! derives it by comparing each phase's version to `facts.json`'s released
//! `alint_version`, so the timeline always agrees with the displayed
//! latest release. `roadmap.json` tracks `main` (the docs-bundle overlays
//! it from main alongside `ROADMAP.md`), whereas `facts.json` stays pinned
//! to the release tag — the same split the pipeline already draws between
//! the living plan and the released surface area.
//!
//! `run(false)` rewrites the file, `run(true)` content-diffs the committed
//! copy and fails on drift. Carries no volatile fields so it commits
//! cleanly and gates on content. The no-AI-signal invariant (no em dashes
//! or smart quotes in any title or blurb) is enforced by a test HERE
//! because alint.org's prose lint deliberately ignores the synced
//! `public/_alint/**` contract — the alint repo owns what feeds it.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::Serialize;

/// Bumped when the `roadmap.json` shape changes so the alint.org consumer
/// can pin the schema it understands (mirrors `facts.json`).
const FORMAT_VERSION: u32 = 1;
const ROADMAP_DOC: &str = "docs/design/ROADMAP.md";

#[derive(Serialize)]
struct Roadmap {
    format_version: u32,
    phases: Vec<Phase>,
}

#[derive(Serialize)]
struct Phase {
    /// `"0.12"`, `"1.0"`, etc., or `null` for a version-less milestone
    /// (the spec-driven-development foundations track).
    version: Option<String>,
    title: String,
    /// `"release"` for a versioned cut, `"foundations"` for the
    /// version-less engineering-foundations track.
    kind: String,
    blurb: String,
}

fn roadmap_path() -> Result<PathBuf> {
    Ok(crate::workspace_root()?.join("roadmap.json"))
}

fn build_roadmap() -> Result<Roadmap> {
    let root = crate::workspace_root()?;
    let path = root.join(ROADMAP_DOC);
    let md = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let phases = parse_phases(&md)?;
    if phases.is_empty() {
        bail!(
            "no `roadmap-public` markers found in {ROADMAP_DOC}; the public \
             roadmap would be empty"
        );
    }
    Ok(Roadmap {
        format_version: FORMAT_VERSION,
        phases,
    })
}

/// Public phases in document order. Each is a `## ` heading whose
/// following `<!-- roadmap-public: blurb="..." -->` marker opts it in.
fn parse_phases(md: &str) -> Result<Vec<Phase>> {
    let mut phases = Vec::new();
    let mut current_heading: Option<String> = None;
    for line in md.lines() {
        let trimmed = line.trim_end();
        if let Some(h) = trimmed.strip_prefix("## ") {
            current_heading = Some(h.trim().to_string());
        } else if let Some(blurb) = parse_marker(trimmed) {
            let heading = current_heading.clone().with_context(|| {
                format!("`roadmap-public` marker with no preceding `## ` heading: {trimmed}")
            })?;
            phases.push(heading_to_phase(&heading, blurb));
        }
    }
    Ok(phases)
}

/// `<!-- roadmap-public: blurb="<text>" -->` → `<text>`.
fn parse_marker(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("<!-- roadmap-public:")?;
    let after = rest.trim_start().strip_prefix("blurb=\"")?;
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

/// `v0.12: Real-world coverage (shipped)` → versioned phase;
/// `Engineering foundations: spec-driven development` → version-less.
fn heading_to_phase(heading: &str, blurb: String) -> Phase {
    if let Some(rest) = heading.strip_prefix('v') {
        if let Some(colon) = rest.find(':') {
            let ver = &rest[..colon];
            if !ver.is_empty()
                && ver.contains('.')
                && ver.chars().all(|c| c.is_ascii_digit() || c == '.')
            {
                let raw = rest[colon + 1..].trim();
                let title = raw.strip_suffix("(shipped)").map_or(raw, str::trim_end);
                return Phase {
                    version: Some(ver.to_string()),
                    title: title.to_string(),
                    kind: "release".to_string(),
                    blurb,
                };
            }
        }
    }
    Phase {
        version: None,
        title: heading.trim().to_string(),
        kind: "foundations".to_string(),
        blurb,
    }
}

/// Pretty JSON plus a trailing newline (git-friendly, byte-compared by
/// `--check`).
fn render(roadmap: &Roadmap) -> Result<String> {
    let mut s = serde_json::to_string_pretty(roadmap).context("serialize roadmap.json")?;
    s.push('\n');
    Ok(s)
}

pub fn run(check: bool) -> Result<()> {
    let roadmap = build_roadmap()?;
    let rendered = render(&roadmap)?;
    let path = roadmap_path()?;

    if check {
        let committed =
            fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        if committed != rendered {
            bail!(
                "roadmap.json is stale. Run `cargo run -p xtask -- gen-roadmap` to \
                 regenerate and commit the result."
            );
        }
        println!("roadmap.json is up to date");
        return Ok(());
    }

    fs::write(&path, &rendered).with_context(|| format!("write {}", path.display()))?;
    println!("wrote roadmap.json");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `roadmap.json` must be regenerated and committed when the roadmap
    /// changes — `--check` is what CI + preflight run.
    #[test]
    fn gen_roadmap_check_passes_on_committed_tree() {
        run(true).expect("gen-roadmap --check should pass on the committed tree");
    }

    /// No AI-content signals (em dashes, en dashes, smart quotes) in any
    /// title or blurb. Enforced here because alint.org's prose lint ignores
    /// the synced `public/_alint/**` contract, so the source owns it. Also
    /// guards against an empty field.
    #[test]
    fn no_ai_content_signals_in_any_field() {
        const FORBIDDEN: &[char] = &[
            '\u{2014}', // em dash
            '\u{2013}', // en dash
            '\u{2018}', '\u{2019}', // single curly quotes
            '\u{201C}', '\u{201D}', // double curly quotes
        ];
        let roadmap = build_roadmap().expect("build roadmap");
        for p in &roadmap.phases {
            for (field, val) in [("title", &p.title), ("blurb", &p.blurb)] {
                assert!(
                    !val.contains(FORBIDDEN),
                    "phase {:?} {field} carries an em dash / en dash / smart quote \
                     (AI-content signal): {val:?}",
                    p.version
                );
                assert!(
                    !val.contains("&mdash;"),
                    "phase {:?} {field} carries &mdash;: {val:?}",
                    p.version
                );
                assert!(
                    !val.is_empty(),
                    "phase {:?} has an empty {field}",
                    p.version
                );
            }
        }
    }

    /// alint.org derives shipped/planned by comparing each phase's version
    /// to `facts.json`'s released version, so the released version itself
    /// must be a public phase. Catches shipping a release whose `##`
    /// heading was never given a `roadmap-public` marker.
    #[test]
    fn released_version_is_a_public_phase() {
        let v = env!("CARGO_PKG_VERSION"); // workspace version, e.g. "0.12.0"
        let mut it = v.split('.');
        let major_minor = format!(
            "{}.{}",
            it.next().expect("major"),
            it.next().expect("minor")
        );
        let roadmap = build_roadmap().expect("build roadmap");
        assert!(
            roadmap
                .phases
                .iter()
                .any(|p| p.version.as_deref() == Some(major_minor.as_str())),
            "released version {major_minor} is not a roadmap-public phase; add a \
             `<!-- roadmap-public: ... -->` marker under its `## ` heading in {ROADMAP_DOC}"
        );
    }

    /// Versioned phases appear in non-decreasing version order (document
    /// order is timeline order); the version-less foundations entry is
    /// skipped. Every phase carries a kind.
    #[test]
    fn versioned_phases_are_monotonic() {
        let roadmap = build_roadmap().expect("build roadmap");
        let key = |p: &Phase| -> Option<(u32, u32)> {
            let v = p.version.as_ref()?;
            let mut it = v.split('.');
            Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
        };
        let mut last: Option<(u32, u32)> = None;
        for p in &roadmap.phases {
            assert!(
                p.kind == "release" || p.kind == "foundations",
                "unexpected kind {:?}",
                p.kind
            );
            if let Some(k) = key(p) {
                if let Some(prev) = last {
                    assert!(
                        k >= prev,
                        "phase versions out of order: {prev:?} then {k:?}"
                    );
                }
                last = Some(k);
            }
        }
    }
}
