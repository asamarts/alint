//! Scenario-driven end-to-end tests.
//!
//! Each YAML file under `scenarios/` becomes one `#[test]`. The
//! bodies are all identical: load the YAML, materialize the tree
//! into a tempdir, drive the steps, assert outcomes + final tree.

use alint_testkit::{Scenario, assert_scenario, run_scenario};

fn run(path: &str) {
    let yaml = std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!("could not read scenario at {path}: {e}");
    });
    let scenario = Scenario::from_yaml(&yaml)
        .unwrap_or_else(|e| panic!("scenario at {path}: parse error: {e}"));
    let run = run_scenario(&scenario)
        .unwrap_or_else(|e| panic!("scenario {:?} at {path}: run error: {e}", scenario.name));
    assert_scenario(&scenario, &run).unwrap_or_else(|e| {
        panic!(
            "scenario {:?} at {path}: assertion failed: {e}",
            scenario.name
        )
    });
}

// One #[test] per scenario file. When new scenarios land, add the
// corresponding test function here. A future commit can replace
// this hand-rolled list with `dir-test` for auto-generation.

#[test]
fn fix_file_create() {
    run("scenarios/fix/file_create.yml");
}

#[test]
fn fix_file_remove() {
    run("scenarios/fix/file_remove.yml");
}

#[test]
fn fix_file_prepend() {
    run("scenarios/fix/file_prepend.yml");
}

#[test]
fn fix_file_append() {
    run("scenarios/fix/file_append.yml");
}

#[test]
fn fix_file_rename() {
    run("scenarios/fix/file_rename.yml");
}
