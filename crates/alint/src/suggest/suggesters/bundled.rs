//! Bundled-ruleset suggester. Takes the ecosystem detection
//! the `Scan` already computed and proposes the matching
//! bundled URIs.
//!
//! Confidence calibration:
//! - `oss-baseline@v1`: HIGH — every repo benefits.
//! - Per-language rulesets: HIGH when an ecosystem manifest is
//!   present (Cargo.toml, package.json, …); the markers are
//!   ~99% precise.
//! - Workspace overlay (`monorepo/<flavor>-workspace@v1`): HIGH
//!   when the workspace probe matched.
//! - `agent-hygiene@v1` is intentionally NOT proposed here —
//!   the antipattern suggester covers it with file-level
//!   evidence so the user sees why we're proposing it.

use crate::progress::Progress;

use crate::suggest::proposal::{Confidence, Evidence, Proposal, ProposalKind};
use crate::suggest::scan::Scan;

pub fn propose(scan: &Scan, progress: &Progress) -> Vec<Proposal> {
    progress.status("Matching bundled rulesets");
    let mut out = Vec::new();

    // oss-baseline@v1 — universally applicable.
    out.push(Proposal {
        id: "alint://bundled/oss-baseline@v1".into(),
        kind: ProposalKind::BundledRuleset {
            uri: "alint://bundled/oss-baseline@v1".into(),
        },
        confidence: Confidence::High,
        evidence: vec![Evidence {
            message: "Applies to every repository — README, LICENSE, hygiene basics.".into(),
        }],
        summary: "Universal OSS hygiene baseline.".into(),
    });

    for lang in &scan.detection.languages {
        let (uri, label, marker) = match lang {
            crate::init::Language::Rust => (
                "alint://bundled/rust@v1",
                "Rust project",
                "Cargo.toml at root",
            ),
            crate::init::Language::Node => (
                "alint://bundled/node@v1",
                "Node project",
                "package.json at root",
            ),
            crate::init::Language::Python => (
                "alint://bundled/python@v1",
                "Python project",
                "pyproject.toml / setup.py / setup.cfg at root",
            ),
            crate::init::Language::Go => ("alint://bundled/go@v1", "Go project", "go.mod at root"),
            crate::init::Language::Java => (
                "alint://bundled/java@v1",
                "Java project",
                "pom.xml / build.gradle at root",
            ),
        };
        out.push(Proposal {
            id: uri.into(),
            kind: ProposalKind::BundledRuleset { uri: uri.into() },
            confidence: Confidence::High,
            evidence: vec![Evidence {
                message: format!("Detected via {marker}."),
            }],
            summary: format!("{label} detected — extend the language ruleset."),
        });
    }

    if let Some(flavor) = scan.detection.workspace {
        let (uri, label) = match flavor {
            crate::init::WorkspaceFlavor::Cargo => (
                "alint://bundled/monorepo/cargo-workspace@v1",
                "Cargo `[workspace]`",
            ),
            crate::init::WorkspaceFlavor::Pnpm => (
                "alint://bundled/monorepo/pnpm-workspace@v1",
                "pnpm-workspace.yaml",
            ),
            crate::init::WorkspaceFlavor::Yarn => (
                "alint://bundled/monorepo/yarn-workspace@v1",
                "package.json `workspaces` field",
            ),
        };
        out.push(Proposal {
            id: "alint://bundled/monorepo@v1".into(),
            kind: ProposalKind::BundledRuleset {
                uri: "alint://bundled/monorepo@v1".into(),
            },
            confidence: Confidence::High,
            evidence: vec![Evidence {
                message: format!("Workspace detected via {label}."),
            }],
            summary: "Workspace-tier monorepo — extend the generic monorepo ruleset.".into(),
        });
        out.push(Proposal {
            id: uri.into(),
            kind: ProposalKind::BundledRuleset { uri: uri.into() },
            confidence: Confidence::High,
            evidence: vec![Evidence {
                message: format!("Workspace flavor detected via {label}."),
            }],
            summary: "Workspace-flavor overlay.".into(),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init::{Detection, Language, WorkspaceFlavor};
    use alint_core::FileIndex;

    fn scan_with(detection: Detection) -> Scan {
        Scan::for_test(detection, FileIndex::default(), Vec::new())
    }

    #[test]
    fn always_proposes_oss_baseline() {
        let scan = scan_with(Detection::default());
        let proposals = propose(&scan, &Progress::null());
        assert!(
            proposals
                .iter()
                .any(|p| p.id == "alint://bundled/oss-baseline@v1")
        );
    }

    #[test]
    fn proposes_per_language_ruleset_when_detected() {
        let scan = scan_with(Detection {
            languages: vec![Language::Rust, Language::Node],
            workspace: None,
        });
        let proposals = propose(&scan, &Progress::null());
        let ids: Vec<&str> = proposals.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"alint://bundled/rust@v1"));
        assert!(ids.contains(&"alint://bundled/node@v1"));
    }

    #[test]
    fn proposes_workspace_overlay_when_detected() {
        let scan = scan_with(Detection {
            languages: vec![Language::Rust],
            workspace: Some(WorkspaceFlavor::Cargo),
        });
        let proposals = propose(&scan, &Progress::null());
        let ids: Vec<&str> = proposals.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"alint://bundled/monorepo@v1"));
        assert!(ids.contains(&"alint://bundled/monorepo/cargo-workspace@v1"));
    }
}
