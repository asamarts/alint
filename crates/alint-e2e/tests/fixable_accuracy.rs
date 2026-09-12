//! Per-violation fixability accuracy (close-off item 2).
//!
//! `check` tags a violation `fixable` so the user can trust `alint fix` will
//! resolve it. That promise is per-VIOLATION, not per-rule: `filename_case`
//! under `snake` flags both `myFile.rs` (which `fix` renames to `my_file.rs`)
//! and `café.rs` (which has no valid snake target, so `fix` honestly skips it).
//! Before this gate the engine tagged BOTH fixable because the rule declared a
//! fixer; now it consults [`Fixer::can_fix`] per violation.
//!
//! This drives the REAL `filename_case` rule + `FileRenameFixer` through the
//! engine (the whole chain the human renderer's unit tests stand in for) and
//! asserts the flag lands on the convertible violation only.

use std::path::Path;

use alint_core::{Engine, RuleEntry, WalkOptions, walk};
use alint_testkit::treespec::{TreeSpec, materialize};

const CONFIG: &str = "\
version: 1
rules:
  - id: snake-names
    kind: filename_case
    paths: \"**/*.rs\"
    case: snake
    level: warning
    fix:
      file_rename: {}
";

fn run_check(root: &Path) -> alint_core::Report {
    let config_path = root.join(".alint.yml");
    std::fs::write(&config_path, CONFIG).unwrap();
    let cache = alint_dsl::extends::Cache::at(root.join(".alint-cache"));
    let opts = alint_dsl::LoadOptions::with_cache(cache);
    let config = alint_dsl::load_with(&config_path, &opts).unwrap();

    let registry = alint_rules::builtin_registry();
    let mut entries: Vec<RuleEntry> = Vec::new();
    for spec in &config.rules {
        entries.push(RuleEntry::new(registry.build(spec).unwrap()));
    }
    let engine = Engine::from_entries(entries, registry);
    let index = walk(root, &WalkOptions::default()).unwrap();
    engine.run(root, &index).unwrap()
}

#[test]
fn check_tags_only_the_convertible_stem_fixable() {
    let tmp = tempfile::Builder::new()
        .prefix("alint-fixable-accuracy-")
        .tempdir()
        .unwrap();
    let root = tmp.path();
    // Two .rs files that both violate `snake`: one convertible, one not.
    let tree: TreeSpec = serde_yaml_ng::from_str("myFile.rs: \"\"\ncafé.rs: \"\"\n").unwrap();
    materialize(&tree, root).unwrap();

    let report = run_check(root);

    // One rule result, two violations. The rule DOES declare a fixer, so the
    // per-rule flag is true -- the point is that the per-violation flags differ.
    let result = report
        .results
        .iter()
        .find(|r| &*r.rule_id == "snake-names")
        .expect("snake-names produced a result");
    assert!(
        result.is_fixable,
        "the rule declares a fixer (per-rule flag)"
    );
    assert_eq!(result.violations.len(), 2, "both files are flagged");

    let fixable_of = |needle: &str| -> bool {
        result
            .violations
            .iter()
            .find(|v| {
                v.path
                    .as_deref()
                    .is_some_and(|p| p.to_string_lossy().contains(needle))
            })
            .unwrap_or_else(|| panic!("no violation for {needle}: {:?}", result.violations))
            .is_fixable
    };

    assert!(
        fixable_of("myFile"),
        "convertible stem myFile.rs must be tagged fixable"
    );
    assert!(
        !fixable_of("café"),
        "unconvertible stem café.rs must NOT be tagged fixable (fix skips it)"
    );

    // The user-facing count must match: exactly one auto-fixable violation.
    let fixable_count = report
        .results
        .iter()
        .flat_map(|r| &r.violations)
        .filter(|v| v.is_fixable)
        .count();
    assert_eq!(fixable_count, 1, "exactly one violation is auto-fixable");
}
