//! Soft listing: rule kinds in the registry but absent from any
//! `xtask/src/bench/scenarios/*.yml`. Bench-scale coverage is
//! NOT a correctness requirement — perf shape is the goal there
//! — so this test always passes and just emits an `eprintln!`
//! summary of the gap. Run with `cargo test -- --nocapture` to
//! see the listing during a normal test run.
//!
//! Use it as a triage list when expanding bench-scale: kinds
//! whose dispatch shape isn't represented in any bench scenario
//! are the ones whose next regression won't be gated by
//! `bench-compare`.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

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

fn collect_kinds_from_yaml(yaml: &str, out: &mut HashSet<String>) {
    let Ok(parsed) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(yaml) else {
        return;
    };
    walk(&parsed, out);
}

fn walk(v: &serde_yaml_ng::Value, out: &mut HashSet<String>) {
    match v {
        serde_yaml_ng::Value::Mapping(m) => {
            if let Some(kind) = m
                .get(serde_yaml_ng::Value::String("kind".into()))
                .and_then(|v| v.as_str())
            {
                out.insert(canonical(kind).to_string());
            }
            for (_, child) in m {
                walk(child, out);
            }
        }
        serde_yaml_ng::Value::Sequence(seq) => {
            for child in seq {
                walk(child, out);
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
            } else if path.extension().and_then(|s| s.to_str()) == Some("yml") {
                out.push(path);
            }
        }
    }
    out
}

#[test]
fn list_rule_kinds_not_covered_by_bench_scale() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest
        .ancestors()
        .nth(2)
        .expect("workspace root from alint-e2e CARGO_MANIFEST_DIR");
    let bench_scenarios_dir = workspace_root
        .join("xtask")
        .join("src")
        .join("bench")
        .join("scenarios");
    let bundled_dir = workspace_root
        .join("crates")
        .join("alint-dsl")
        .join("rulesets")
        .join("v1");

    // Bench scenarios reference rule kinds two ways: directly
    // via `kind:` entries in their own YAML, and indirectly via
    // `extends: alint://bundled/<...>@v1` (S3's shape). Walk both
    // sources so a rule covered only via a bundled ruleset still
    // counts.
    let mut bench_kinds: HashSet<String> = HashSet::new();
    let mut bundled_used_in_bench: HashSet<String> = HashSet::new();
    for path in walk_yaml_paths(&bench_scenarios_dir) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        collect_kinds_from_yaml(&text, &mut bench_kinds);
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("- alint://bundled/") {
                let body = rest.split('@').next().unwrap_or(rest);
                bundled_used_in_bench.insert(body.to_string());
            }
        }
    }
    // Pull the rule kinds out of every bundled ruleset that any
    // bench scenario extends.
    for body in &bundled_used_in_bench {
        let path = bundled_dir.join(format!("{body}.yml"));
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        collect_kinds_from_yaml(&text, &mut bench_kinds);
    }

    let registry = alint_rules::builtin_registry();
    let alias_set: HashSet<&str> = ALIASES.iter().map(|(alias, _)| *alias).collect();
    let canonical_kinds: Vec<String> = registry
        .known_kinds()
        .filter(|k| !alias_set.contains(k))
        .map(str::to_string)
        .collect();

    let uncovered: Vec<&String> = canonical_kinds
        .iter()
        .filter(|k| !bench_kinds.contains(k.as_str()))
        .collect();

    if uncovered.is_empty() {
        eprintln!(
            "bench-scale coverage: all {} canonical rule kinds appear in at least one bench scenario.",
            canonical_kinds.len(),
        );
        return;
    }

    let mut sorted: Vec<&&String> = uncovered.iter().collect();
    sorted.sort();
    let mut report = String::new();
    let _ = writeln!(
        report,
        "[bench-coverage soft warning] {} of {} canonical rule kinds are not in any bench scenario:",
        uncovered.len(),
        canonical_kinds.len(),
    );
    for k in sorted {
        let _ = writeln!(report, "  - {k}");
    }
    let _ = writeln!(
        report,
        "  (correctness is not at risk — these are gated by alint-e2e scenarios)",
    );
    let _ = writeln!(
        report,
        "  (perf regressions of these rules' dispatch shapes won't be gated by bench-compare)",
    );
    eprintln!("\n{report}");
}
