//! Coverage-rot audit: rules with git-mode affordances must
//! have scenarios covering both the in-repo and outside-git
//! paths.
//!
//! Three pure-git rules (`git_blame_age`, `git_commit_message`,
//! `git_no_denied_paths`) need:
//!   - at least one scenario with `given.git: { init: true, … }`
//!     (real repo; the rule's in-repo behaviour is tested)
//!   - at least one scenario without `given.git:` (the
//!     "outside a git repo" silent-no-op path is tested)
//!
//! The "outside git" path is the one most likely to silently
//! regress — silent no-op behaviour outside a repo is exactly
//! what every git-aware rule must guarantee, and nothing else
//! catches its omission.
//!
//! `git_tracked_only:` opt-in on `file_exists` / `file_absent` /
//! `dir_exists` / `dir_absent` is covered by the existing
//! `git_tracked_only_*.yml` scenarios under `git/` — those land
//! through `coverage_audit.rs` already. This audit focuses on
//! kinds whose entire identity depends on git.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::Path;

const GIT_REQUIRED_KINDS: &[&str] = &["git_blame_age", "git_commit_message", "git_no_denied_paths"];

/// Pull every rule kind referenced in this scenario's
/// `given.config:` rules block.
fn kinds_in_scenario(scenario: &serde_yaml_ng::Value) -> HashSet<String> {
    let mut out = HashSet::new();
    let Some(given) = scenario.get("given") else {
        return out;
    };
    let Some(config) = given.get("config").and_then(|v| v.as_str()) else {
        return out;
    };
    let Ok(parsed) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(config) else {
        return out;
    };
    walk_kinds(&parsed, &mut out);
    out
}

fn walk_kinds(v: &serde_yaml_ng::Value, out: &mut HashSet<String>) {
    match v {
        serde_yaml_ng::Value::Mapping(m) => {
            if let Some(kind) = m
                .get(serde_yaml_ng::Value::String("kind".into()))
                .and_then(|v| v.as_str())
            {
                out.insert(kind.to_string());
            }
            for (_, child) in m {
                walk_kinds(child, out);
            }
        }
        serde_yaml_ng::Value::Sequence(seq) => {
            for child in seq {
                walk_kinds(child, out);
            }
        }
        _ => {}
    }
}

/// True if this scenario sets up a real git repo via
/// `given.git.init: true`. Anything else (`given.git:` absent,
/// or `init: false`) counts as "outside a git repo".
fn has_git_init(scenario: &serde_yaml_ng::Value) -> bool {
    scenario
        .get("given")
        .and_then(|g| g.get("git"))
        .and_then(|git| git.get("init"))
        .and_then(serde_yaml_ng::Value::as_bool)
        .unwrap_or(false)
}

fn walkdir(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|s| s.to_str()) == Some("yml") {
                out.push(path);
            }
        }
    }
    out
}

#[test]
fn git_required_rules_have_in_repo_and_outside_repo_scenarios() {
    let scenarios_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios");

    // (kind → (has_in_repo, has_outside_git))
    let mut coverage: std::collections::HashMap<&'static str, (bool, bool)> = GIT_REQUIRED_KINDS
        .iter()
        .map(|k| (*k, (false, false)))
        .collect();

    for path in walkdir(&scenarios_dir) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(scenario) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text) else {
            continue;
        };
        let kinds = kinds_in_scenario(&scenario);
        if kinds.is_empty() {
            continue;
        }
        let in_repo = has_git_init(&scenario);
        for &git_kind in GIT_REQUIRED_KINDS {
            if kinds.contains(git_kind)
                && let Some(entry) = coverage.get_mut(git_kind)
            {
                if in_repo {
                    entry.0 = true;
                } else {
                    entry.1 = true;
                }
            }
        }
    }

    let mut missing = Vec::new();
    let mut sorted_kinds: Vec<&&str> = coverage.keys().collect();
    sorted_kinds.sort();
    for &kind in &sorted_kinds {
        let (has_in_repo, has_outside) = coverage[kind];
        if !has_in_repo {
            missing.push(format!(
                "  - {kind} has no IN-REPO scenario (given.git.init: true)"
            ));
        }
        if !has_outside {
            missing.push(format!(
                "  - {kind} has no OUTSIDE-GIT scenario (no given.git block)"
            ));
        }
    }
    if missing.is_empty() {
        return;
    }
    let mut report = String::new();
    let _ = writeln!(
        report,
        "{} git-mode coverage gap(s) across {} git-required rule kind(s):",
        missing.len(),
        GIT_REQUIRED_KINDS.len(),
    );
    for line in missing {
        let _ = writeln!(report, "{line}");
    }
    let _ = writeln!(
        report,
        "  (in-repo: scenario uses given.git: {{ init: true, add: […], commit: true }})",
    );
    let _ = writeln!(
        report,
        "  (outside-git: same scenario shape, omit given.git — \
         rule must silent no-op)",
    );
    panic!("\n{report}");
}
