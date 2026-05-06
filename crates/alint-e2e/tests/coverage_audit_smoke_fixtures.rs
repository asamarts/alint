//! v0.9.15 Phase 7 — smoke-test fixture audit.
//!
//! `coverage_audit_examples_parse.rs` (Phase 2) catches schema errors
//! at config-load time. But four catalogued pitfalls in
//! `docs/development/CONFIG-AUTHORING.md` produce runtime-semantic
//! bugs the parse audit can't see:
//!
//! - **#13** — regex `^`/`$` defaults to file-start anchoring;
//!   without `(?m)` the pattern silently never matches multi-line
//!   input.
//! - **#14** — single-quoted YAML strings don't expand `\n` to a
//!   literal newline inside regex patterns; the regex compiles into
//!   a literal `\n` two-char match that never appears in real files.
//! - **#16** — `*_path_matches` against a bool/number/null field
//!   emits a runtime "value at path is not a string" violation on
//!   every match, completely inverting the intended signal.
//! - **#17** — `*_path_equals` against a `[*]` `JSONPath` flips
//!   intent from "any element matches" to "every element must
//!   match", firing on every element that doesn't.
//!
//! This audit walks `crates/alint-e2e/fixtures/smoke/<scenario>/`
//! directories. Each scenario is a self-contained config + file tree +
//! `expected.toml` with the canonical violation counts. The audit runs
//! the engine and asserts the actuals match the expected counts; a
//! refactor that silently re-introduces any of the runtime-semantic
//! pitfalls above would change the counts and fail.
//!
//! Fixture format documented in
//! `crates/alint-e2e/fixtures/smoke/README.md`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use alint_core::{Engine, RuleEntry, WalkOptions, walk};
use alint_rules::builtin_registry;

/// Per-scenario `expected.toml` shape.
#[derive(Debug, serde::Deserialize)]
struct Expected {
    total: usize,
    #[serde(default)]
    per_rule: BTreeMap<String, usize>,
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/smoke")
}

fn discover_fixtures() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let dir = fixtures_dir();
    for entry in fs::read_dir(&dir).expect("read fixtures/smoke") {
        let path = entry.unwrap().path();
        if !path.is_dir() {
            continue;
        }
        if !path.join("alint.yml").is_file() {
            continue;
        }
        if !path.join("tree").is_dir() {
            continue;
        }
        if !path.join("expected.toml").is_file() {
            continue;
        }
        out.push(path);
    }
    out.sort();
    out
}

/// Build a single-config Engine from a fixture's `alint.yml`. Mirrors
/// the load + build path that `alint check` uses.
fn engine_for_fixture(scenario: &Path) -> Engine {
    let config = alint_dsl::load(&scenario.join("alint.yml"))
        .unwrap_or_else(|e| panic!("loading {}: {e}", scenario.display()));
    let registry = builtin_registry();
    let mut entries = Vec::with_capacity(config.rules.len());
    for spec in &config.rules {
        if matches!(spec.level, alint_core::Level::Off) {
            continue;
        }
        let rule = registry
            .build(spec)
            .unwrap_or_else(|e| panic!("building rule {} in {}: {e}", spec.id, scenario.display()));
        let mut entry = RuleEntry::new(rule);
        if let Some(when_src) = &spec.when {
            let expr = alint_core::when::parse(when_src).unwrap_or_else(|e| {
                panic!(
                    "parsing `when` for rule {} in {}: {e}",
                    spec.id,
                    scenario.display()
                )
            });
            entry = entry.with_when(expr);
        }
        entries.push(entry);
    }
    Engine::from_entries(entries, registry)
}

/// Run the engine against `tree/` and return per-rule violation
/// counts.
fn count_violations(engine: &Engine, tree_root: &Path) -> BTreeMap<String, usize> {
    let opts = WalkOptions::default();
    let index = walk(tree_root, &opts).expect("walk tree");
    let report = engine.run(tree_root, &index).expect("run engine");
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for r in &report.results {
        if r.violations.is_empty() {
            continue;
        }
        *counts.entry(r.rule_id.to_string()).or_insert(0) += r.violations.len();
    }
    counts
}

#[test]
fn every_smoke_fixture_produces_expected_violation_counts() {
    let fixtures = discover_fixtures();
    assert!(
        !fixtures.is_empty(),
        "no smoke fixtures found under {} — has the directory been wiped?",
        fixtures_dir().display(),
    );

    let mut failures: Vec<String> = Vec::new();

    for scenario in &fixtures {
        let scenario_name = scenario.file_name().and_then(|n| n.to_str()).unwrap_or("?");

        let expected_text = fs::read_to_string(scenario.join("expected.toml"))
            .unwrap_or_else(|e| panic!("read expected.toml in {scenario_name}: {e}"));
        let expected: Expected = toml::from_str(&expected_text)
            .unwrap_or_else(|e| panic!("parse expected.toml in {scenario_name}: {e}"));

        // Sanity: per-rule counts must sum to total. Catches typos in
        // the fixture metadata before they ever reach the engine.
        let per_rule_sum: usize = expected.per_rule.values().sum();
        if per_rule_sum != expected.total {
            failures.push(format!(
                "{scenario_name}: expected.toml is internally inconsistent — \
                 per_rule counts sum to {per_rule_sum} but `total = {}`",
                expected.total,
            ));
            continue;
        }

        let engine = engine_for_fixture(scenario);
        let actual_per_rule = count_violations(&engine, &scenario.join("tree"));
        let actual_total: usize = actual_per_rule.values().sum();

        if actual_total != expected.total || actual_per_rule != expected.per_rule {
            failures.push(format!(
                "{scenario_name}: expected total={} per_rule={:?}, got total={} per_rule={:?}\n  \
                 Did a refactor change the rule's runtime semantics? Pitfalls #13/#14/#16/#17 \
                 in docs/development/CONFIG-AUTHORING.md catalog the most likely classes.",
                expected.total, expected.per_rule, actual_total, actual_per_rule,
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} smoke fixture(s) failed:\n\n  - {}\n",
        failures.len(),
        failures.join("\n  - "),
    );
}
