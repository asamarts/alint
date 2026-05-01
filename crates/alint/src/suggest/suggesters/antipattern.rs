//! Antipattern suggester. Walks the file index for known
//! agent-hygiene leftovers and proposes
//! `extends: alint://bundled/agent-hygiene@v1` when at least
//! one is found.
//!
//! The bundled ruleset already collects these checks under one
//! umbrella; surfacing them as a single proposal (rather than
//! one per pattern) is the cleanest UX. Evidence bullets list
//! what was found so the user sees why we're suggesting it.
//!
//! Confidence: MEDIUM. Some hits are obvious (`.bak` files);
//! others are heuristic (`console.log` outside test code may be
//! intentional in a logging library). The user reviews before
//! adopting.

use std::path::Path;

use crate::progress::Progress;

use crate::suggest::proposal::{Confidence, Evidence, Proposal, ProposalKind};
use crate::suggest::scan::Scan;

const AGENT_HYGIENE_URI: &str = "alint://bundled/agent-hygiene@v1";

pub fn propose(scan: &Scan, progress: &Progress) -> Vec<Proposal> {
    let phase = progress.phase("Scanning for agent-hygiene antipatterns", None);

    let mut evidence: Vec<Evidence> = Vec::new();

    // 1. Backup-suffix files. Skip test fixtures (paths under
    // `tests/`, `fixtures/`, `__tests__/`) — those frequently
    // contain `*.bak` etc. as deliberate inputs.
    let backup_hits: Vec<&Path> = scan
        .index
        .files()
        .map(|e| e.path.as_ref())
        .filter(|p| has_backup_suffix(p))
        .filter(|p| !is_fixture_or_test_path(p))
        .collect();
    if !backup_hits.is_empty() {
        evidence.push(Evidence {
            message: format!(
                "{} backup-suffix file{} ({})",
                backup_hits.len(),
                if backup_hits.len() == 1 { "" } else { "s" },
                preview_paths(&backup_hits, 3),
            ),
        });
    }

    // 2. Scratch / planning docs at root.
    let scratch_hits: Vec<&Path> = scan
        .index
        .files()
        .map(|e| e.path.as_ref())
        .filter(|p| is_scratch_doc_at_root(p))
        .collect();
    if !scratch_hits.is_empty() {
        evidence.push(Evidence {
            message: format!(
                "{} scratch / planning doc{} at root ({})",
                scratch_hits.len(),
                if scratch_hits.len() == 1 { "" } else { "s" },
                preview_paths(&scratch_hits, 3),
            ),
        });
    }

    // 3. console.log / .debug / .trace in non-test JS / TS.
    let console_hits = scan_console_log(scan, &phase);
    if !console_hits.is_empty() {
        evidence.push(Evidence {
            message: format!(
                "{} `console.log`-style call{} in non-test source ({})",
                console_hits.len(),
                if console_hits.len() == 1 { "" } else { "s" },
                preview_paths_paths(&console_hits, 3),
            ),
        });
    }

    phase.finish("Antipattern scan complete");

    if evidence.is_empty() {
        return Vec::new();
    }

    vec![Proposal {
        id: AGENT_HYGIENE_URI.into(),
        kind: ProposalKind::BundledRuleset {
            uri: AGENT_HYGIENE_URI.into(),
        },
        confidence: Confidence::Medium,
        summary: "Agent-hygiene leftovers detected — bundled ruleset would catch them.".into(),
        evidence,
    }]
}

fn has_backup_suffix(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    // Common editor / agent backup suffixes. `.swp` is vim's,
    // `.orig` is git merge's, `~`-suffix is Emacs / GNU
    // tradition. Compare via Path::extension where possible
    // for case-insensitivity; fall back to a tilde tail check.
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    ext.eq_ignore_ascii_case("bak")
        || ext.eq_ignore_ascii_case("orig")
        || ext.eq_ignore_ascii_case("swp")
        || ext.eq_ignore_ascii_case("swo")
        || name.ends_with('~')
}

fn is_scratch_doc_at_root(path: &Path) -> bool {
    // Only flag at the repo root — `docs/PLAN.md` in a project
    // about software-engineering tooling is fine; `PLAN.md` at
    // root is the agent-leftover shape.
    if path
        .parent()
        .map(Path::as_os_str)
        .is_some_and(|s| !s.is_empty())
    {
        return false;
    }
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    matches!(
        name,
        "PLAN.md"
            | "NOTES.md"
            | "ANALYSIS.md"
            | "DRAFT.md"
            | "SCRATCH.md"
            | "WIP.md"
            | "TODO.md"
            | "PROPOSAL.md"
            | "BRAINSTORM.md"
            | "IDEAS.md"
            | "PLAN.txt"
            | "NOTES.txt"
    )
}

fn scan_console_log(scan: &Scan, phase: &crate::progress::Phase) -> Vec<std::path::PathBuf> {
    use regex::Regex;
    // Compiled once per call.
    let pattern =
        Regex::new(r"\bconsole\s*\.\s*(log|debug|trace|info)\s*\(").expect("static regex compiles");
    let mut hits = Vec::new();
    for entry in scan.text_files() {
        let path = &entry.path;
        if !is_js_or_ts(path) {
            continue;
        }
        if is_fixture_or_test_path(path) {
            continue;
        }
        phase.set_message(&format!("scanning {}", path.display()));
        let full = scan.root.join(path);
        let Ok(bytes) = std::fs::read(&full) else {
            continue;
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        if pattern.is_match(text) {
            hits.push(path.to_path_buf());
        }
    }
    hits
}

fn is_js_or_ts(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs")
    )
}

fn is_test_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.contains("/test/")
        || s.contains("/tests/")
        || s.contains("/__tests__/")
        || s.contains(".test.")
        || s.contains(".spec.")
}

/// Broader test-fixture filter applied to every antipattern
/// scan. The console-log scan already calls
/// [`is_test_path`] for its narrower per-file purpose; this
/// adds `fixtures/` and `test-fixtures/` so deliberately-
/// shaped backup / scratch files under those paths don't fire
/// false positives.
fn is_fixture_or_test_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    is_test_path(path)
        || s.contains("/fixtures/")
        || s.contains("/test-fixtures/")
        || s.contains("/snapshots/")
}

fn preview_paths(paths: &[&Path], max: usize) -> String {
    use std::fmt::Write;
    let take = paths.iter().take(max).map(|p| p.display().to_string());
    let mut joined = take.collect::<Vec<_>>().join(", ");
    if paths.len() > max {
        let _ = write!(joined, ", +{} more", paths.len() - max);
    }
    joined
}

fn preview_paths_paths(paths: &[std::path::PathBuf], max: usize) -> String {
    use std::fmt::Write;
    let take = paths.iter().take(max).map(|p| p.display().to_string());
    let mut joined = take.collect::<Vec<_>>().join(", ");
    if paths.len() > max {
        let _ = write!(joined, ", +{} more", paths.len() - max);
    }
    joined
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_backup_suffix_variants() {
        assert!(has_backup_suffix(Path::new("foo.bak")));
        assert!(has_backup_suffix(Path::new("foo.orig")));
        assert!(has_backup_suffix(Path::new(".foo.swp")));
        assert!(has_backup_suffix(Path::new("README~")));
        assert!(!has_backup_suffix(Path::new("foo.rs")));
        assert!(!has_backup_suffix(Path::new("my.bakery.ts")));
    }

    #[test]
    fn detects_scratch_doc_at_root_only() {
        assert!(is_scratch_doc_at_root(Path::new("PLAN.md")));
        assert!(is_scratch_doc_at_root(Path::new("NOTES.md")));
        assert!(is_scratch_doc_at_root(Path::new("ANALYSIS.md")));
        // Subdirectories are fine.
        assert!(!is_scratch_doc_at_root(Path::new("docs/PLAN.md")));
        assert!(!is_scratch_doc_at_root(Path::new("scripts/NOTES.md")));
        // Misc capitalisation isn't flagged — agent leftovers
        // are usually all-caps.
        assert!(!is_scratch_doc_at_root(Path::new("plan.md")));
    }

    #[test]
    fn flags_js_ts_extensions_only() {
        assert!(is_js_or_ts(Path::new("src/foo.ts")));
        assert!(is_js_or_ts(Path::new("src/foo.tsx")));
        assert!(is_js_or_ts(Path::new("src/foo.js")));
        assert!(is_js_or_ts(Path::new("src/foo.mjs")));
        assert!(!is_js_or_ts(Path::new("src/foo.rs")));
    }

    #[test]
    fn skips_test_paths() {
        assert!(is_test_path(Path::new("src/__tests__/foo.test.ts")));
        assert!(is_test_path(Path::new("packages/api/test/foo.ts")));
        assert!(is_test_path(Path::new("src/foo.spec.ts")));
        assert!(!is_test_path(Path::new("src/foo.ts")));
    }

    #[test]
    fn empty_repo_proposes_nothing() {
        let scan = Scan::for_test(
            crate::init::Detection::default(),
            alint_core::FileIndex::default(),
            Vec::new(),
        );
        let proposals = propose(&scan, &Progress::null());
        assert!(proposals.is_empty());
    }

    #[test]
    fn backup_file_alone_proposes_agent_hygiene() {
        let index = alint_core::FileIndex::from_entries(vec![alint_core::FileEntry {
            path: Path::new("README.md.bak").into(),
            is_dir: false,
            size: 0,
        }]);
        let scan = Scan::for_test(crate::init::Detection::default(), index, Vec::new());
        let proposals = propose(&scan, &Progress::null());
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].id, AGENT_HYGIENE_URI);
        assert_eq!(proposals[0].confidence, Confidence::Medium);
    }
}
