//! Versioned end-to-end policy tests. `dir-test` generates one Rust test per
//! document in the general corpus and the co-located bundled-ruleset corpus.

use std::path::Path;

use alint_testkit::{PolicyTest, Runner, RunnerOptions, TestStatus};
use dir_test::{Fixture, dir_test};

use crate::support;

const EXPECTED_GENERAL_SCENARIOS: usize = 489;
const EXPECTED_BUNDLED_SCENARIOS: usize = 93;

#[test]
fn all_578_migrated_documents_use_the_public_v1_parser() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let roots = [
        (
            manifest.join("scenarios"),
            EXPECTED_GENERAL_SCENARIOS,
            "general",
        ),
        (
            manifest.join("../alint-dsl/ruleset-tests/v1"),
            EXPECTED_BUNDLED_SCENARIOS,
            "bundled",
        ),
    ];
    let mut total = 0;
    for (root, expected, label) in roots {
        let sources = support::policy_test_sources(&root);
        assert_eq!(
            sources.len(),
            expected,
            "{label} policy-test inventory changed; update the migration count deliberately"
        );
        total += sources.len();
        for (path, yaml) in sources {
            let test = PolicyTest::from_yaml(&yaml)
                .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
            if let Err(diagnostics) = test.validate() {
                panic!("validate {}: {diagnostics:#?}", path.display());
            }
            // `dir-test` handles the repository's `.yml` corpus one fixture per
            // Rust test. Execute any `.yaml` policy document here so package-local
            // `.yaml` support files remain discoverable but are not mistaken for
            // scenarios.
            if path.extension().and_then(|extension| extension.to_str()) == Some("yaml") {
                run_policy_test(&path, &test);
            }
        }
    }
    assert_eq!(total, 582);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/scenarios",
    glob: "**/*.yml",
)]
#[allow(clippy::needless_pass_by_value)] // Required by dir-test's macro signature.
fn scenario(fixture: Fixture<&str>) {
    run_fixture(&fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../alint-dsl/ruleset-tests/v1",
    glob: "**/*.yml",
)]
#[allow(clippy::needless_pass_by_value)] // Required by dir-test's macro signature.
fn bundled_ruleset(fixture: Fixture<&str>) {
    run_fixture(&fixture);
}

fn run_fixture(fixture: &Fixture<&str>) {
    let path = Path::new(fixture.path());
    let policy_test = PolicyTest::from_yaml(fixture.content())
        .unwrap_or_else(|error| panic!("policy test at {}: parse error: {error}", path.display()));
    run_policy_test(path, &policy_test);
}

fn run_policy_test(path: &Path, policy_test: &PolicyTest) {
    let runner = Runner::new(RunnerOptions {
        allow_spawn: true,
        allow_host_keyring: true,
        update_snapshots: false,
    });
    let run = runner.run(policy_test, path).unwrap_or_else(|error| {
        panic!(
            "policy test {:?} at {}: {error}",
            policy_test.name,
            path.display()
        )
    });
    if run.status == TestStatus::Skipped {
        eprintln!("skipping {}: {:?}", path.display(), run.mismatches);
        return;
    }
    assert!(
        run.passed(),
        "policy test {:?} at {} mismatched: {:#?}",
        policy_test.name,
        path.display(),
        run.mismatches
    );
}
