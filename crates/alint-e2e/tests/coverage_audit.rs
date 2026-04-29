//! Coverage-rot audit: every registered rule kind must have at
//! least one scenario.
//!
//! v0.8.1 surfaced the regression class where a new rule kind
//! ships without an e2e scenario — `dir_test`'s auto-discovery
//! catches missing test invocations but not missing scenarios.
//! This audit closes the gap by walking the `scenarios/` tree,
//! collecting every `kind:` value referenced, and asserting the
//! set covers the registry.
//!
//! Failure prints the missing kinds. The fix is to add a pass +
//! fail scenario under `scenarios/check/<family>/`. Newly-added
//! rule kinds show up here on the first PR that lands them.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::Path;

fn collect_kinds_from_yaml(yaml: &str, out: &mut HashSet<String>) {
    let Ok(parsed) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(yaml) else {
        return;
    };
    walk(&parsed, out);
}

/// Recursively pull every `kind: <name>` mapping pair out of the
/// scenario YAML. Scenarios may nest rules inside `require:`
/// blocks (`for_each_dir`, `every_matching_has`) — those nested
/// kinds count toward coverage too.
///
/// alint-e2e scenarios carry the actual rules inside an embedded
/// `config:` string (`given.config: |\n version: 1\n rules: ...`).
/// When we see a string value, try to re-parse it as YAML and
/// recurse — otherwise the rules buried in those strings are
/// invisible to the audit.
fn walk(value: &serde_yaml_ng::Value, out: &mut HashSet<String>) {
    match value {
        serde_yaml_ng::Value::Mapping(m) => {
            if let Some(kind) = m
                .get(serde_yaml_ng::Value::String("kind".into()))
                .and_then(|v| v.as_str())
            {
                out.insert(kind.to_string());
            }
            for (_, v) in m {
                walk(v, out);
            }
        }
        serde_yaml_ng::Value::Sequence(seq) => {
            for v in seq {
                walk(v, out);
            }
        }
        serde_yaml_ng::Value::String(s) => {
            if s.contains("kind:")
                && let Ok(inner) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(s)
            {
                walk(&inner, out);
            }
        }
        _ => {}
    }
}

fn collect_scenario_kinds(scenarios_dir: &Path) -> HashSet<String> {
    let mut found = HashSet::new();
    for entry in walkdir(scenarios_dir) {
        if entry.extension().and_then(|s| s.to_str()) != Some("yml") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&entry) else {
            continue;
        };
        collect_kinds_from_yaml(&text, &mut found);
    }
    found
}

/// Tiny std-only directory walk; saves a `walkdir` dep for one
/// test. Yields every file path under `root`.
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
            } else {
                out.push(path);
            }
        }
    }
    out
}

#[test]
fn every_registered_rule_kind_has_a_scenario() {
    let scenarios_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios");
    let scenario_kinds = collect_scenario_kinds(&scenarios_dir);

    let registry = alint_rules::builtin_registry();
    let registered: HashSet<String> = registry.known_kinds().map(str::to_string).collect();

    // Aliases register both `file_content_matches` and
    // `content_matches` to the same builder. For coverage
    // purposes, we accept either form — adding both to scenarios
    // is wasteful. Build the alias→canonical mapping inline so
    // a scenario using just one form satisfies the audit.
    let aliased: &[(&str, &str)] = &[
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
    let alias_set: HashSet<&str> = aliased.iter().map(|(alias, _)| *alias).collect();

    let canonical_registered: HashSet<&String> = registered
        .iter()
        .filter(|k| !alias_set.contains(k.as_str()))
        .collect();

    let scenario_canonical: HashSet<String> = scenario_kinds
        .iter()
        .map(|k| {
            aliased
                .iter()
                .find(|(alias, _)| alias == k)
                .map_or_else(|| k.clone(), |(_, canon)| (*canon).to_string())
        })
        .collect();

    let missing: Vec<&&String> = canonical_registered
        .iter()
        .filter(|k| !scenario_canonical.contains(k.as_str()))
        .collect();

    if !missing.is_empty() {
        let mut sorted: Vec<&&String> = missing.clone();
        sorted.sort();
        let mut bullets = String::new();
        for k in &sorted {
            let _ = writeln!(bullets, "  - {k}");
        }
        panic!(
            "scenarios/ is missing {} of {} canonical rule kinds. \
             Add a pass + fail YAML scenario under crates/alint-e2e/scenarios/check/<family>/:\n{}",
            missing.len(),
            canonical_registered.len(),
            bullets,
        );
    }
}
