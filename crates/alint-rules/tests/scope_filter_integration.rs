//! Integration tests for the `scope_filter:` per-file gate
//! shipped in v0.9.6.
//!
//! Built against the `no_trailing_whitespace` rule because it's
//! one of the most-used per-file rules in the bundled rulesets
//! (`rust@v1`, `node@v1`, …) and exercises the engine's per-file
//! dispatch path. If `scope_filter:` is silently dropped on the
//! way through `build()`, these tests fail because the rule
//! fires on out-of-scope files that have no `Cargo.toml`
//! ancestor.
//!
//! See `docs/design/v0.9/scope-filter.md` for the gate's
//! contract and `crates/alint-core/src/scope_filter.rs` for the
//! ancestor-walk semantics.

use std::path::Path;

use alint_core::{Engine, RuleEntry, WalkOptions, walk};
use alint_rules::builtin_registry;

/// Build a single-rule engine from a YAML spec snippet. The spec
/// is parsed via `serde_yaml_ng` exactly as the loader would.
fn engine_from_yaml(yaml: &str) -> Engine {
    let spec: alint_core::RuleSpec = serde_yaml_ng::from_str(yaml).unwrap();
    let registry = builtin_registry();
    let rule = registry.build(&spec).unwrap();
    Engine::from_entries(vec![RuleEntry::new(rule)], registry)
}

/// Run the engine over `root` and return the set of relative
/// violation paths (as strings, sorted). Empty when no rule
/// fired anywhere in the tree.
fn run_and_collect_paths(engine: &Engine, root: &Path) -> Vec<String> {
    let opts = WalkOptions::default();
    let index = walk(root, &opts).unwrap();
    let report = engine.run(root, &index).unwrap();
    let mut paths: Vec<String> = report
        .results
        .iter()
        .flat_map(|r| r.violations.iter())
        .filter_map(|v| v.path.as_deref().map(|p| p.display().to_string()))
        .collect();
    paths.sort();
    paths
}

/// Materialise a polyglot tree:
///
/// ```text
///   crates/api/Cargo.toml
///   crates/api/src/main.rs        ← trailing whitespace, IN scope
///   services/web/scripts/migrate.rs ← trailing whitespace, NOT in scope
/// ```
///
/// Used by every test in this file so the in-scope vs out-of-scope
/// expectation is identical across them.
fn polyglot_tree() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("crates/api/src")).unwrap();
    std::fs::create_dir_all(root.join("services/web/scripts")).unwrap();
    std::fs::write(
        root.join("crates/api/Cargo.toml"),
        "[package]\nname='api'\n",
    )
    .unwrap();
    // Both files have trailing whitespace on line 1.
    std::fs::write(root.join("crates/api/src/main.rs"), "fn main() { } \n").unwrap();
    std::fs::write(
        root.join("services/web/scripts/migrate.rs"),
        "// not really rust \n",
    )
    .unwrap();
    tmp
}

/// Baseline: without `scope_filter:`, the rule fires on BOTH
/// files. Pins the "before" behaviour so the gated test below
/// is unambiguous about what changes when the filter is added.
#[test]
fn baseline_without_scope_filter_fires_on_every_match() {
    let tmp = polyglot_tree();
    let engine = engine_from_yaml(
        "id: nws\n\
         kind: no_trailing_whitespace\n\
         level: warning\n\
         paths: '**/*.rs'\n",
    );
    let paths = run_and_collect_paths(&engine, tmp.path());
    assert_eq!(
        paths,
        vec![
            "crates/api/src/main.rs".to_string(),
            "services/web/scripts/migrate.rs".to_string(),
        ],
        "without scope_filter both .rs files should fire",
    );
}

/// The bug under test. With `scope_filter: { has_ancestor:
/// Cargo.toml }`, the rule must ONLY fire on files inside an
/// actual Rust package. `services/web/scripts/migrate.rs` has
/// no `Cargo.toml` ancestor and must be silently skipped.
///
/// Today (v0.9.6) this fails — per-file rule builders parse
/// `spec.scope_filter` into the spec but never thread it into
/// the built rule, so `Rule::scope_filter()` returns the trait
/// default `None` and the engine's gate is a no-op. The fix is
/// to have each per-file rule build/store/expose the filter so
/// `engine.rs:417` actually filters dispatch.
#[test]
fn scope_filter_skips_files_without_ancestor_manifest() {
    let tmp = polyglot_tree();
    let engine = engine_from_yaml(
        "id: nws\n\
         kind: no_trailing_whitespace\n\
         level: warning\n\
         paths: '**/*.rs'\n\
         scope_filter:\n  has_ancestor: Cargo.toml\n",
    );
    let paths = run_and_collect_paths(&engine, tmp.path());
    assert_eq!(
        paths,
        vec!["crates/api/src/main.rs".to_string()],
        "scope_filter must skip .rs files outside any Cargo.toml subtree",
    );
}

/// Two-name `has_ancestor` list — `pyproject.toml` OR `setup.py`.
/// File inside `app/` is in scope (sibling `setup.py`); file
/// outside is not. Exercises the multi-name dispatch path.
#[test]
fn scope_filter_with_two_name_list() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::write(root.join("app/setup.py"), "# stub\n").unwrap();
    std::fs::write(root.join("app/main.py"), "x = 1 \n").unwrap();
    std::fs::write(root.join("scripts/oneoff.py"), "y = 2 \n").unwrap();

    let engine = engine_from_yaml(
        "id: nws\n\
         kind: no_trailing_whitespace\n\
         level: warning\n\
         paths: '**/*.py'\n\
         scope_filter:\n  has_ancestor:\n    - pyproject.toml\n    - setup.py\n",
    );
    let paths = run_and_collect_paths(&engine, root);
    assert_eq!(
        paths,
        vec!["app/main.py".to_string()],
        "scope_filter list must accept either name; out-of-scope file must skip",
    );
}
