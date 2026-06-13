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

/// AI-content signals forbidden in any title or blurb. Enforced in
/// `build_roadmap` (so `gen-roadmap --check` catches them on the docs-only CI
/// lane, which never runs `cargo test`) because alint.org's prose lint
/// deliberately ignores the synced `public/_alint/**` contract — the source
/// owns this invariant.
const FORBIDDEN_SIGNALS: &[char] = &[
    '\u{2014}', // em dash
    '\u{2013}', // en dash
    '\u{2018}', '\u{2019}', // single curly quotes
    '\u{201C}', '\u{201D}', // double curly quotes
];

/// Reject empty fields and any AI-content signal (em / en dash, smart quote,
/// `&mdash;`) in a phase title or blurb.
fn validate_no_ai_signals(phases: &[Phase]) -> Result<()> {
    for p in phases {
        for (field, val) in [("title", &p.title), ("blurb", &p.blurb)] {
            if val.is_empty() {
                bail!("phase {:?} has an empty {field}", p.version);
            }
            if let Some(c) = val.chars().find(|c| FORBIDDEN_SIGNALS.contains(c)) {
                bail!(
                    "phase {:?} {field} carries an AI-content signal {c:?} (em / en dash \
                     or smart quote): {val:?}. Use plain ASCII punctuation.",
                    p.version
                );
            }
            if val.contains("&mdash;") {
                bail!("phase {:?} {field} carries `&mdash;`: {val:?}", p.version);
            }
        }
    }
    Ok(())
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
    validate_no_ai_signals(&phases)?;
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
            phases.push(heading_to_phase(&heading, blurb)?);
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
fn heading_to_phase(heading: &str, blurb: String) -> Result<Phase> {
    // Versioned release heading: `v<major>.<minor>: <title>`. Treated as
    // versioned only when it starts with `v` + a digit and has a colon. A
    // version-looking heading whose version is NOT exactly two numeric
    // components (a 3-part patch like `v0.9.11`, or a typo like `v0..1`) is
    // rejected: the public roadmap is minor-version granular and the site
    // keys status on major.minor, so a patch would silently collide with its
    // minor.
    if let Some(rest) = heading.strip_prefix('v') {
        if rest.starts_with(|c: char| c.is_ascii_digit()) {
            if let Some(colon) = rest.find(':') {
                let ver = &rest[..colon];
                let parts: Vec<&str> = ver.split('.').collect();
                let is_major_minor = parts.len() == 2
                    && parts
                        .iter()
                        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
                if !is_major_minor {
                    bail!(
                        "roadmap-public heading {heading:?} has version {ver:?}; public \
                         phases must be major.minor (e.g. `v0.12`). Do not put a \
                         roadmap-public marker on a patch or malformed version."
                    );
                }
                let raw = rest[colon + 1..].trim();
                let title = raw.strip_suffix("(shipped)").map_or(raw, str::trim_end);
                return Ok(Phase {
                    version: Some(ver.to_string()),
                    title: title.to_string(),
                    kind: "release".to_string(),
                    blurb,
                });
            }
        }
    }
    // Version-less named milestone (e.g. the engineering-foundations track).
    Ok(Phase {
        version: None,
        title: heading.trim().to_string(),
        kind: "foundations".to_string(),
        blurb,
    })
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

    /// `validate_no_ai_signals` (wired into `build_roadmap`, so it runs on the
    /// docs-only `gen-roadmap --check` lane that never runs `cargo test`)
    /// rejects em dashes, smart quotes, `&mdash;`, and empty fields, and
    /// accepts clean ASCII. The committed tree being clean is covered by
    /// `gen_roadmap_check_passes_on_committed_tree` (which calls `build_roadmap`).
    #[test]
    fn validate_rejects_ai_signals_and_empty() {
        let mk = |title: &str, blurb: &str| Phase {
            version: Some("0.1".to_string()),
            title: title.to_string(),
            kind: "release".to_string(),
            blurb: blurb.to_string(),
        };
        assert!(validate_no_ai_signals(&[mk("MVP", "A clean ASCII blurb.")]).is_ok());
        assert!(validate_no_ai_signals(&[mk("MVP", "an em dash \u{2014} here")]).is_err());
        assert!(validate_no_ai_signals(&[mk("a \u{201C}smart\u{201D} title", "ok")]).is_err());
        assert!(validate_no_ai_signals(&[mk("MVP", "an en dash \u{2013} here")]).is_err());
        assert!(validate_no_ai_signals(&[mk("MVP", "and &mdash; entity")]).is_err());
        assert!(validate_no_ai_signals(&[mk("", "empty title")]).is_err());
        assert!(validate_no_ai_signals(&[mk("MVP", "")]).is_err());
    }

    /// `parse_phases` extracts versioned releases and the version-less
    /// foundations milestone, strips the `(shipped)` suffix, and keeps
    /// document order — driven by synthetic markdown, not the live file.
    #[test]
    fn parse_phases_handles_release_foundations_and_shipped_suffix() {
        let md = r#"## v0.1: MVP (shipped)
<!-- roadmap-public: blurb="first" -->

## Engineering foundations: spec-driven development
<!-- roadmap-public: blurb="second" -->

## v0.13: WASM plugins
<!-- roadmap-public: blurb="third" -->
"#;
        let phases = parse_phases(md).expect("parse");
        assert_eq!(phases.len(), 3);
        assert_eq!(phases[0].version.as_deref(), Some("0.1"));
        assert_eq!(phases[0].title, "MVP"); // (shipped) stripped
        assert_eq!(phases[0].kind, "release");
        assert_eq!(phases[1].version, None);
        assert_eq!(phases[1].kind, "foundations");
        assert_eq!(
            phases[1].title,
            "Engineering foundations: spec-driven development"
        );
        assert_eq!(phases[2].version.as_deref(), Some("0.13"));
        assert_eq!(phases[2].blurb, "third");
    }

    /// A `roadmap-public` marker with no preceding `## ` heading is an error.
    #[test]
    fn parse_phases_rejects_marker_without_heading() {
        let md = "some prose\n<!-- roadmap-public: blurb=\"orphan\" -->\n";
        assert!(parse_phases(md).is_err());
    }

    /// A versioned heading whose version is not exactly major.minor (a 3-part
    /// patch, or malformed) is rejected, so it can't silently collide with its
    /// minor on the site or render garbage. Version-less names are fine.
    #[test]
    fn heading_to_phase_rejects_non_major_minor_version() {
        assert!(heading_to_phase("v0.9.11: patch", "b".to_string()).is_err());
        assert!(heading_to_phase("v0..1: broken", "b".to_string()).is_err());
        assert!(heading_to_phase("v0.12: ok", "b".to_string()).is_ok());
        assert!(heading_to_phase("Engineering foundations: x", "b".to_string()).is_ok());
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
