//! Coverage-rot audit: every bundled ruleset under
//! `crates/alint-dsl/rulesets/v1/**/*.yml` must be referenced by
//! at least one well-formed scenario (no violations) AND at least
//! one ill-formed scenario (≥1 violation). Without this, a
//! bundled ruleset can land or evolve without exercising both
//! its happy path and its flagging path through e2e.
//!
//! Mechanics: each bundled YAML's path → ruleset URI mapping
//! follows `alint-dsl/rulesets/v1/<...>.yml` →
//! `alint://bundled/<...>@v1` (without the file extension).
//! Each scenario YAML carries `given.config:` (a YAML string with
//! `extends: [<uri>, …]`) and `expect.violations:`. We classify
//! each scenario as "well-formed" (every expect.violations entry
//! is empty) or "ill-formed" (at least one non-empty), then
//! cross-reference: each bundled URI needs at least one of each
//! shape.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[derive(Default)]
struct Coverage {
    well_formed_in: Vec<String>,
    ill_formed_in: Vec<String>,
}

/// Translate a path under `crates/alint-dsl/rulesets/v1/` into
/// the `alint://bundled/<…>@v1` URI scenarios use to extend it.
/// `oss-baseline.yml` → `alint://bundled/oss-baseline@v1`;
/// `monorepo/cargo-workspace.yml` →
/// `alint://bundled/monorepo/cargo-workspace@v1`.
fn ruleset_uri(rel_path: &Path) -> Option<String> {
    let rel = rel_path.with_extension("");
    let body = rel.to_str()?.replace('\\', "/");
    Some(format!("alint://bundled/{body}@v1"))
}

/// Pull every `alint://bundled/...` URI referenced from this
/// scenario's `given.config:`. The config carries `extends:` as
/// a list of strings; we don't try to parse it formally — a
/// substring match against the canonical URI is enough.
fn scenario_extends_set(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- alint://bundled/") {
            let uri = format!("alint://bundled/{}", rest.trim_matches(['\'', '"']));
            out.insert(uri);
        } else if let Some(rest) = trimmed.strip_prefix("alint://bundled/") {
            // tolerate `alint://...` on a bare line, just in case
            out.insert(format!(
                "alint://bundled/{}",
                rest.trim_matches(['\'', '"', ' ', ',']),
            ));
        }
    }
    out
}

/// True if every `expect[*].violations:` block in the YAML is
/// empty. Empty list → scenario expects no violations →
/// "well-formed". Any non-empty entry → "ill-formed".
fn is_well_formed(scenario: &serde_yaml_ng::Value) -> bool {
    let Some(expect) = scenario.get("expect") else {
        return true;
    };
    let mut all_empty = true;
    walk_expect(expect, &mut all_empty);
    all_empty
}

fn walk_expect(v: &serde_yaml_ng::Value, all_empty: &mut bool) {
    match v {
        serde_yaml_ng::Value::Mapping(m) => {
            if let Some(violations) = m.get(serde_yaml_ng::Value::String("violations".into()))
                && let Some(seq) = violations.as_sequence()
                && !seq.is_empty()
            {
                *all_empty = false;
            }
            for (_, child) in m {
                walk_expect(child, all_empty);
            }
        }
        serde_yaml_ng::Value::Sequence(seq) => {
            for child in seq {
                walk_expect(child, all_empty);
            }
        }
        _ => {}
    }
}

fn walk_yaml_paths(root: &Path) -> Vec<PathBuf> {
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
            } else if path.extension().and_then(|s| s.to_str()) == Some("yml")
                || path.extension().and_then(|s| s.to_str()) == Some("yaml")
            {
                out.push(path);
            }
        }
    }
    out
}

#[test]
fn every_bundled_ruleset_has_well_formed_and_ill_formed_scenarios() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rulesets_dir = manifest
        .ancestors()
        .nth(2)
        .expect("workspace root from alint-e2e CARGO_MANIFEST_DIR")
        .join("crates")
        .join("alint-dsl")
        .join("rulesets")
        .join("v1");
    let scenarios_dir = manifest.join("scenarios");

    // Map bundled URI → coverage status.
    let mut coverage: HashMap<String, Coverage> = HashMap::new();
    let mut all_uris: Vec<String> = Vec::new();
    for path in walk_yaml_paths(&rulesets_dir) {
        let rel = path
            .strip_prefix(&rulesets_dir)
            .expect("rulesets file under rulesets dir");
        if let Some(uri) = ruleset_uri(rel) {
            all_uris.push(uri.clone());
            coverage.insert(uri, Coverage::default());
        }
    }
    all_uris.sort();

    for path in walk_yaml_paths(&scenarios_dir) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let extends = scenario_extends_set(&text);
        if extends.is_empty() {
            continue;
        }
        let Ok(scenario) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text) else {
            continue;
        };
        let well_formed = is_well_formed(&scenario);
        let scenario_name = path
            .strip_prefix(&scenarios_dir)
            .unwrap_or(&path)
            .display()
            .to_string();
        for uri in &extends {
            if let Some(c) = coverage.get_mut(uri) {
                if well_formed {
                    c.well_formed_in.push(scenario_name.clone());
                } else {
                    c.ill_formed_in.push(scenario_name.clone());
                }
            }
        }
    }

    let mut missing_well_formed: Vec<&String> = Vec::new();
    let mut missing_ill_formed: Vec<&String> = Vec::new();
    for uri in &all_uris {
        let c = coverage.get(uri).expect("coverage seeded above");
        if c.well_formed_in.is_empty() {
            missing_well_formed.push(uri);
        }
        if c.ill_formed_in.is_empty() {
            missing_ill_formed.push(uri);
        }
    }

    if missing_well_formed.is_empty() && missing_ill_formed.is_empty() {
        return;
    }

    let mut report = String::new();
    if !missing_well_formed.is_empty() {
        let _ = writeln!(
            report,
            "{} of {} bundled rulesets have no WELL-FORMED scenario \
             (expect: [{{ violations: [] }}]):",
            missing_well_formed.len(),
            all_uris.len(),
        );
        for uri in missing_well_formed {
            let _ = writeln!(report, "  - {uri}");
        }
        let _ = writeln!(
            report,
            "  (add a scenario with given.config.extends: [<uri>] and \
             a tree shape that satisfies the bundled rules)",
        );
    }
    if !missing_ill_formed.is_empty() {
        let _ = writeln!(
            report,
            "{} of {} bundled rulesets have no ILL-FORMED scenario \
             (expect: with at least one non-empty violations entry):",
            missing_ill_formed.len(),
            all_uris.len(),
        );
        for uri in missing_ill_formed {
            let _ = writeln!(report, "  - {uri}");
        }
        let _ = writeln!(
            report,
            "  (add a scenario where the bundled ruleset flags a missing \
             or malformed file with concrete expected violations)",
        );
    }
    panic!("\n{report}");
}
