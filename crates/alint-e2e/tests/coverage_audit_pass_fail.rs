//! Coverage-rot audit: every registered rule kind must have a
//! scenario where it fires AND a scenario where it stays silent.
//!
//! `coverage_audit.rs` (the v0.8.1 audit) only requires "≥1
//! scenario per kind". A rule can ship with only its passing
//! scenario and stay green — its failure path silently regresses.
//! This audit closes that gap by classifying each scenario as
//! "kind X fires here" / "kind X stays silent here" and asserting
//! every kind has both states across the scenario corpus.
//!
//! Mechanics: each scenario YAML carries `given.config:` (a YAML
//! string with `rules: [{id, kind, …}]`) and `expect.violations:`
//! (a list of `{rule: <id>, …}` entries). For each rule kind in a
//! scenario, we look up whether any rule of that kind appears in
//! the violations list — yes → "fires", no → "silent".

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;

#[derive(Default)]
struct Status {
    fires_in: Vec<String>,
    silent_in: Vec<String>,
}

/// Pull every `(rule_id, rule_kind)` pair out of a scenario's
/// `given.config:` YAML-string rules block. Nested
/// `require:` blocks contribute too — `for_each_dir` and friends
/// might wrap a `file_exists` whose pass/fail behaviour we want
/// counted.
fn rules_in_scenario(scenario: &serde_yaml_ng::Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(given) = scenario.get("given") else {
        return out;
    };
    let Some(config) = given.get("config").and_then(|v| v.as_str()) else {
        return out;
    };
    let Ok(parsed) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(config) else {
        return out;
    };
    walk_rules(&parsed, &mut out);
    out
}

fn walk_rules(v: &serde_yaml_ng::Value, out: &mut Vec<(String, String)>) {
    match v {
        serde_yaml_ng::Value::Mapping(m) => {
            let id = m
                .get(serde_yaml_ng::Value::String("id".into()))
                .and_then(|v| v.as_str());
            let kind = m
                .get(serde_yaml_ng::Value::String("kind".into()))
                .and_then(|v| v.as_str());
            if let (Some(id), Some(kind)) = (id, kind) {
                out.push((id.to_string(), kind.to_string()));
            }
            for (_, child) in m {
                walk_rules(child, out);
            }
        }
        serde_yaml_ng::Value::Sequence(seq) => {
            for child in seq {
                walk_rules(child, out);
            }
        }
        _ => {}
    }
}

/// Collect every `rule:` id from `expect[*].violations[*]`. A
/// rule whose id appears here was firing in this scenario.
fn fired_rule_ids(scenario: &serde_yaml_ng::Value) -> HashSet<String> {
    let mut out = HashSet::new();
    let Some(expect) = scenario.get("expect") else {
        return out;
    };
    walk_violations(expect, &mut out);
    out
}

fn walk_violations(v: &serde_yaml_ng::Value, out: &mut HashSet<String>) {
    match v {
        serde_yaml_ng::Value::Mapping(m) => {
            if let Some(id) = m
                .get(serde_yaml_ng::Value::String("rule".into()))
                .and_then(|v| v.as_str())
            {
                out.insert(id.to_string());
            }
            for (_, child) in m {
                walk_violations(child, out);
            }
        }
        serde_yaml_ng::Value::Sequence(seq) => {
            for child in seq {
                walk_violations(child, out);
            }
        }
        _ => {}
    }
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

/// Rule kinds whose firing case can't be expressed in the
/// scenario YAML format because the testkit doesn't yet
/// materialise the required filesystem primitive. Each entry
/// must point at a native Rust integration test that DOES
/// cover the firing case.
///
/// These should shrink to zero as the testkit grows
/// `mode: 0o755`, `symlink_to: <path>`, custom commit
/// messages, and `GIT_AUTHOR_DATE` overrides — at which point
/// the YAML coverage becomes feasible and the entry is
/// removed.
const NATIVE_FIRES_ALLOWLIST: &[(&str, &str)] = &[
    (
        "executable_bit",
        "crates/alint-e2e/tests/unix_metadata.rs (testkit can't write +x bits)",
    ),
    (
        "executable_has_shebang",
        "crates/alint-e2e/tests/unix_metadata.rs (testkit can't write +x bits)",
    ),
    (
        "no_symlinks",
        "crates/alint-e2e/tests/unix_metadata.rs (testkit can't materialise symlinks)",
    ),
    (
        "git_blame_age",
        "crates/alint-rules/tests/git_blame_age.rs (testkit runner doesn't backdate commits via GIT_AUTHOR_DATE)",
    ),
    (
        "git_commit_message",
        "crates/alint-rules/tests/shell_out_rules.rs (testkit runner uses a fixed commit message)",
    ),
];

/// Same alias map as `coverage_audit.rs`. Normalise before
/// recording status so a single scenario using either form
/// satisfies the audit.
const ALIASES: &[(&str, &str)] = &[
    ("content_matches", "file_content_matches"),
    ("content_forbidden", "file_content_forbidden"),
    ("header", "file_header"),
    ("footer", "file_footer"),
    ("shebang", "file_shebang"),
    ("max_size", "file_max_size"),
    ("min_size", "file_min_size"),
    ("min_lines", "file_min_lines"),
    ("max_lines", "file_max_lines"),
    ("is_text", "file_is_text"),
];

fn canonical(kind: &str) -> &str {
    ALIASES
        .iter()
        .find(|(alias, _)| *alias == kind)
        .map_or(kind, |(_, canon)| *canon)
}

// 101 lines — the test enumerates every registered rule kind,
// classifies each as having a pass scenario / fail scenario /
// both / neither, and emits a structured failure report when
// gaps exist. Splitting would obscure the single linear
// classification loop. Two lines over the threshold; allow.
#[allow(clippy::too_many_lines)]
#[test]
fn every_registered_rule_kind_has_pass_and_fail_scenarios() {
    let scenarios_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios");
    let mut status: HashMap<String, Status> = HashMap::new();

    for path in walkdir(&scenarios_dir) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(scenario) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text) else {
            continue;
        };
        let rules = rules_in_scenario(&scenario);
        if rules.is_empty() {
            continue;
        }
        let fired = fired_rule_ids(&scenario);
        let scenario_name = path
            .strip_prefix(&scenarios_dir)
            .unwrap_or(&path)
            .display()
            .to_string();

        // For each kind in this scenario, classify by whether any
        // rule of that kind appears in the fired set. A scenario
        // that doesn't reach the rule (for example because the
        // scope doesn't match anything in the tree) registers as
        // "silent" — that's correct, the rule did stay silent.
        let mut kinds_in_scenario: HashMap<String, bool> = HashMap::new();
        for (id, kind) in &rules {
            let canon = canonical(kind).to_string();
            let entry = kinds_in_scenario.entry(canon).or_insert(false);
            if fired.contains(id) {
                *entry = true;
            }
        }
        for (kind, did_fire) in kinds_in_scenario {
            let s = status.entry(kind).or_default();
            if did_fire {
                s.fires_in.push(scenario_name.clone());
            } else {
                s.silent_in.push(scenario_name.clone());
            }
        }
    }

    let registry = alint_rules::builtin_registry();
    let alias_set: HashSet<&str> = ALIASES.iter().map(|(alias, _)| *alias).collect();
    let canonical_kinds: Vec<String> = registry
        .known_kinds()
        .filter(|k| !alias_set.contains(k))
        .map(str::to_string)
        .collect();

    let native_allowlist: HashSet<&str> = NATIVE_FIRES_ALLOWLIST
        .iter()
        .map(|(kind, _)| *kind)
        .collect();

    let mut missing_fires: Vec<&String> = Vec::new();
    let mut missing_silent: Vec<&String> = Vec::new();
    for kind in &canonical_kinds {
        let s = status.get(kind);
        let fires = s.is_some_and(|s| !s.fires_in.is_empty());
        let silent = s.is_some_and(|s| !s.silent_in.is_empty());
        if !fires && !native_allowlist.contains(kind.as_str()) {
            missing_fires.push(kind);
        }
        if !silent {
            missing_silent.push(kind);
        }
    }

    if missing_fires.is_empty() && missing_silent.is_empty() {
        return;
    }

    let mut report = String::new();
    if !missing_fires.is_empty() {
        let mut sorted: Vec<&&String> = missing_fires.iter().collect();
        sorted.sort();
        let _ = writeln!(
            report,
            "{} of {} canonical rule kinds have no FIRING scenario:",
            missing_fires.len(),
            canonical_kinds.len(),
        );
        for k in sorted {
            let _ = writeln!(report, "  - {k}");
        }
        let _ = writeln!(
            report,
            "  (add a scenario where the rule emits violations: \
             expect: [{{ violations: [{{ rule: <id>, … }}] }}])",
        );
    }
    if !missing_silent.is_empty() {
        let mut sorted: Vec<&&String> = missing_silent.iter().collect();
        sorted.sort();
        let _ = writeln!(
            report,
            "{} of {} canonical rule kinds have no SILENT scenario:",
            missing_silent.len(),
            canonical_kinds.len(),
        );
        for k in sorted {
            let _ = writeln!(report, "  - {k}");
        }
        let _ = writeln!(
            report,
            "  (add a scenario where the rule stays quiet on a \
             well-formed input: expect: [{{ violations: [] }}])",
        );
    }
    panic!("\n{report}");
}
