//! Anti-drift gate for alint's dogfood config vs the shipped bundled rulesets.
//!
//! alint's own `.alint.yml` hand-copies a few rules that also ship in
//! `crates/alint-dsl/rulesets/v1/**` (same `id:`), typically to enforce them at a
//! stricter `level:` on this repo. Those copies MUST keep identical *matching
//! semantics* -- only presentation (`level`, `message`, `policy_url`, `fix`) may
//! differ. When they drift, external users who `extends:` the bundled ruleset get
//! behavior the maintainers never see on their own dogfooded repo.
//!
//! This is the gate issue #208 lacked: the bundled `gha-pin-actions-to-sha` regex
//! had drifted from the already-fixed dogfood copy, shipping a false positive on
//! local `./` action references. This test fails on exactly that kind of drift.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_yaml_ng::Value;

/// Fields that may legitimately differ between a dogfood copy and its bundled
/// twin: they change how a violation is *presented* or *fixed*, not *whether* it
/// fires.
const PRESENTATION: &[&str] = &["level", "message", "policy_url", "fix"];

/// Shared ids whose matching semantics are intentionally different (a documented
/// divergence), exempt from the equality check:
///   - `rust-sources-snake-case`: the dogfood copy is deliberately narrowed to
///     `crates/**/src/**/*.rs`, whereas the bundled `rust@v1` copy is
///     repo-agnostic (`**/src/**` plus compiler-fixture excludes that would be
///     no-ops in this tree). The divergence can only make the dogfood copy fire
///     on *fewer* files, never over-fire for external users.
const ALLOWLIST: &[&str] = &["rust-sources-snake-case"];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root from alint-e2e CARGO_MANIFEST_DIR")
        .to_path_buf()
}

fn walk_yaml(root: &Path) -> Vec<PathBuf> {
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
            } else if matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("yml" | "yaml")
            ) {
                out.push(path);
            }
        }
    }
    out
}

/// Rebuild a rule mapping without the presentation-only keys.
fn without_presentation(rule: &Value) -> Value {
    let Some(m) = rule.as_mapping() else {
        return rule.clone();
    };
    let mut out = serde_yaml_ng::Mapping::new();
    for (k, v) in m {
        if k.as_str().is_some_and(|key| PRESENTATION.contains(&key)) {
            continue;
        }
        out.insert(k.clone(), canon(v));
    }
    Value::Mapping(out)
}

/// Recursively sort mapping keys so key *order* never causes a false diff;
/// sequence order is preserved (it can be semantically meaningful).
fn canon(v: &Value) -> Value {
    match v {
        Value::Mapping(m) => {
            let mut entries: Vec<(Value, Value)> =
                m.iter().map(|(k, val)| (k.clone(), canon(val))).collect();
            entries.sort_by_key(|(k, _)| k.as_str().unwrap_or_default().to_string());
            let mut out = serde_yaml_ng::Mapping::new();
            for (k, val) in entries {
                out.insert(k, val);
            }
            Value::Mapping(out)
        }
        Value::Sequence(s) => Value::Sequence(s.iter().map(canon).collect()),
        other => other.clone(),
    }
}

/// id -> canonicalized, presentation-stripped rule, for every rule in a config
/// file's top-level `rules:` list.
fn rules_by_id(path: &Path) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return out;
    };
    let Ok(doc) = serde_yaml_ng::from_str::<Value>(&text) else {
        return out;
    };
    let Some(rules) = doc.get("rules").and_then(Value::as_sequence) else {
        return out;
    };
    for rule in rules {
        let Some(id) = rule.get("id").and_then(Value::as_str) else {
            continue;
        };
        out.insert(id.to_string(), without_presentation(rule));
    }
    out
}

#[test]
fn dogfood_rules_match_their_bundled_twins() {
    let root = workspace_root();
    let dogfood = rules_by_id(&root.join(".alint.yml"));

    let mut bundled: BTreeMap<String, Value> = BTreeMap::new();
    for path in walk_yaml(&root.join("crates/alint-dsl/rulesets/v1")) {
        for (id, rule) in rules_by_id(&path) {
            bundled.insert(id, rule);
        }
    }

    // Guard against a vacuous pass. Read/parse errors are swallowed into empty maps
    // (see `rules_by_id`), so if `.alint.yml` or the bundled rulesets ever fail to
    // parse -- or the last shared rule id is renamed away -- the drift loop below
    // would iterate nothing and pass without checking anything. Require at least one
    // non-allowlisted twin present on both sides so the gate can't silently no-op.
    let compared = dogfood
        .keys()
        .filter(|id| !ALLOWLIST.contains(&id.as_str()) && bundled.contains_key(id.as_str()))
        .count();
    assert!(
        compared > 0,
        "drift gate found zero dogfood<->bundled twins to compare -- either a config \
         failed to parse or no shared rule id remains; the gate would pass vacuously."
    );

    let mut drifts: Vec<String> = Vec::new();
    for (id, drule) in &dogfood {
        if ALLOWLIST.contains(&id.as_str()) {
            continue;
        }
        let Some(brule) = bundled.get(id) else {
            continue;
        };
        if drule != brule {
            let d = serde_yaml_ng::to_string(drule).unwrap_or_default();
            let b = serde_yaml_ng::to_string(brule).unwrap_or_default();
            drifts.push(format!(
                "  rule `{id}`:\n    .alint.yml (dogfood) matching fields:\n{}\n    bundled ruleset matching fields:\n{}",
                indent(&d),
                indent(&b),
            ));
        }
    }

    assert!(
        drifts.is_empty(),
        "\nDogfood <-> bundled rule drift: a rule hand-copied into `.alint.yml` no \
         longer matches its bundled twin's matching semantics (only \
         level/message/policy_url/fix may differ). Sync the two copies, or add the \
         id to ALLOWLIST with a comment if the divergence is intentional.\n\n{}",
        drifts.join("\n"),
    );
}

fn indent(s: &str) -> String {
    s.lines()
        .map(|l| format!("      {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}
