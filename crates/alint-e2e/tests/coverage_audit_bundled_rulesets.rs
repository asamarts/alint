//! Ownership and behavior parity audit for bundled rulesets.
//!
//! The runtime bundle catalog is the source of truth. Every shipped revision maps
//! to exactly one directory under `crates/alint-dsl/ruleset-tests/<rev>/`, every
//! document is parsed through the public policy-test parser, and every owner has
//! at least one explicit passing fixture and one fixture with findings.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use alint_core::ExtendsEntry;
use alint_testkit::{ExpectedExit, PolicyTest, Runner, RunnerOptions};
use serde::Deserialize;

use crate::support;

#[derive(Default)]
struct Coverage {
    passing: Vec<String>,
    failing: Vec<String>,
}

#[derive(Default, Deserialize)]
struct ScenarioConfig {
    #[serde(default)]
    extends: Vec<ExtendsEntry>,
    #[serde(default)]
    rules: Vec<ayaml::Value>,
}

fn parse_policy_test(path: &Path, yaml: &str) -> PolicyTest {
    let test = PolicyTest::from_yaml(yaml)
        .unwrap_or_else(|error| panic!("parse policy test {}: {error}", path.display()));
    if let Err(diagnostics) = test.validate() {
        panic!("validate policy test {}: {diagnostics:#?}", path.display());
    }
    test
}

fn config_body(path: &Path, test: &PolicyTest) -> String {
    if let Some(config) = &test.given.config {
        return config.clone();
    }
    let relative = test
        .given
        .config_file
        .as_ref()
        .unwrap_or_else(|| panic!("{} has no config source", path.display()));
    let package_path = path
        .parent()
        .unwrap_or_else(|| panic!("{} has no parent", path.display()))
        .join(relative);
    std::fs::read_to_string(&package_path)
        .unwrap_or_else(|error| panic!("read config {}: {error}", package_path.display()))
}

fn parse_scenario_config(path: &Path, test: &PolicyTest) -> ScenarioConfig {
    let config = config_body(path, test);
    ayaml::from_str(&config)
        .unwrap_or_else(|error| panic!("parse config from {}: {error}", path.display()))
}

fn owner_key(path: &Path, tests_root: &Path) -> String {
    let relative = path
        .parent()
        .and_then(|parent| parent.strip_prefix(tests_root).ok())
        .unwrap_or_else(|| panic!("{} has no bundle owner directory", path.display()));
    relative.to_string_lossy().replace('\\', "/")
}

fn owner_uri(owner: &str) -> String {
    let (revision, name) = owner
        .split_once('/')
        .unwrap_or_else(|| panic!("bundle owner {owner:?} must start with a revision directory"));
    format!("alint://bundled/{name}@{revision}")
}

fn catalog() -> BTreeMap<String, String> {
    let mut catalog = BTreeMap::new();
    for (name, revision) in alint_dsl::bundled::catalog() {
        let owner = format!("{revision}/{name}");
        let uri = format!("alint://bundled/{name}@{revision}");
        assert_eq!(
            catalog.insert(owner.clone(), uri),
            None,
            "runtime bundle catalog contains duplicate owner {owner:?}"
        );
    }
    catalog
}

fn assert_complete_coverage(coverage: &BTreeMap<String, Coverage>) {
    let mut missing_passing = Vec::new();
    let mut missing_failing = Vec::new();
    for (uri, item) in coverage {
        if item.passing.is_empty() {
            missing_passing.push(uri);
        }
        if item.failing.is_empty() {
            missing_failing.push(uri);
        }
    }
    if missing_passing.is_empty() && missing_failing.is_empty() {
        return;
    }

    let mut report = String::new();
    if !missing_passing.is_empty() {
        let _ = writeln!(
            report,
            "{} of {} bundled rulesets have no passing policy test with an explicit `violations: []` assertion:",
            missing_passing.len(),
            coverage.len(),
        );
        for uri in missing_passing {
            let _ = writeln!(report, "  - {uri}");
        }
    }
    if !missing_failing.is_empty() {
        let _ = writeln!(
            report,
            "{} of {} bundled rulesets have no failing policy test with an explicit non-empty `violations:` assertion:",
            missing_failing.len(),
            coverage.len(),
        );
        for uri in missing_failing {
            let _ = writeln!(report, "  - {uri}");
        }
    }
    panic!("\n{report}");
}

#[cfg(unix)]
#[test]
fn ownership_discovery_ignores_support_yaml_and_directory_symlinks() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let owner = root.path().join("v1/example");
    std::fs::create_dir_all(&owner).unwrap();
    std::fs::write(owner.join("pass.yml"), "test_version: 1\n").unwrap();
    std::fs::write(owner.join("support.yaml"), "version: 1\nrules: []\n").unwrap();
    symlink("example", root.path().join("v1/linked")).unwrap();

    let sources = support::policy_test_sources(root.path());
    assert_eq!(sources.len(), 1, "{sources:?}");
    assert_eq!(
        sources[0].0.strip_prefix(root.path()).unwrap(),
        Path::new("v1/example/pass.yml")
    );
}

#[test]
fn runtime_bundle_catalog_exactly_matches_owned_pass_and_fail_tests() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root from alint-e2e CARGO_MANIFEST_DIR");
    let tests_root = workspace.join("crates/alint-dsl/ruleset-tests");
    let catalog = catalog();
    let mut actual_owners = BTreeSet::new();
    let mut coverage: BTreeMap<String, Coverage> = catalog
        .values()
        .cloned()
        .map(|uri| (uri, Coverage::default()))
        .collect();
    let mut scenario_names = BTreeMap::new();

    for (path, yaml) in support::policy_test_sources(&tests_root) {
        let relative = path
            .strip_prefix(&tests_root)
            .expect("ruleset test is below test root")
            .to_string_lossy()
            .replace('\\', "/");
        let owner = owner_key(&path, &tests_root);
        actual_owners.insert(owner.clone());
        let expected_uri = owner_uri(&owner);
        assert_eq!(
            catalog.get(&owner),
            Some(&expected_uri),
            "orphaned ruleset-test owner {owner:?} for {relative} has no shipped bundle"
        );

        let test = parse_policy_test(&path, &yaml);
        if let Some(previous) = scenario_names.insert(test.name.clone(), relative.clone()) {
            panic!(
                "duplicate bundled policy-test name {:?} in {previous} and {relative}",
                test.name
            );
        }
        let config = parse_scenario_config(&path, &test);
        assert_eq!(
            config.extends.len(),
            1,
            "{relative} must extend exactly its owning bundle"
        );
        let entry = &config.extends[0];
        assert_eq!(
            entry.url(),
            expected_uri,
            "{relative} must extend its owning bundle"
        );
        assert!(
            entry.only().is_none() && entry.except().is_none(),
            "{relative} must exercise the complete owning bundle without `only:` or `except:`"
        );

        let explicit_violation_sets: Vec<_> = test
            .expect
            .iter()
            .filter_map(|expectation| expectation.violations.as_ref())
            .collect();
        let item = coverage
            .get_mut(&expected_uri)
            .expect("catalog and owner parity checked above");
        if explicit_violation_sets
            .iter()
            .any(|violations| !violations.is_empty())
        {
            item.failing.push(relative);
        } else if !explicit_violation_sets.is_empty()
            && test
                .expect
                .iter()
                .all(|expectation| expectation.exit == ExpectedExit::Success)
        {
            item.passing.push(relative);
        }
    }

    assert_eq!(
        actual_owners,
        catalog.keys().cloned().collect(),
        "the runtime bundle catalog and owned-test directories must match exactly"
    );
    assert_complete_coverage(&coverage);
}

#[derive(Clone, Copy)]
enum CatalogBehavior {
    Only,
    Except,
    Composition,
}

#[test]
fn catalog_filtering_and_composition_run_through_the_public_runner() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cases = [
        (
            "scenarios/check/extends/bundled_extends_only_keeps_listed_rules.yml",
            CatalogBehavior::Only,
        ),
        (
            "scenarios/check/extends/bundled_extends_except_drops_rules_by_id.yml",
            CatalogBehavior::Except,
        ),
        (
            "scenarios/check/extends/partial_override_bundled.yml",
            CatalogBehavior::Composition,
        ),
    ];
    let runner = Runner::new(RunnerOptions::default());

    for (relative, behavior) in cases {
        let path = manifest.join(relative);
        let yaml = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read policy test {}: {error}", path.display()));
        let test = parse_policy_test(&path, &yaml);
        let config = parse_scenario_config(&path, &test);
        match behavior {
            CatalogBehavior::Only => assert!(
                config.extends.iter().any(|entry| entry.only().is_some()),
                "{relative} must exercise bundled `only:` filtering"
            ),
            CatalogBehavior::Except => assert!(
                config.extends.iter().any(|entry| entry.except().is_some()),
                "{relative} must exercise bundled `except:` filtering"
            ),
            CatalogBehavior::Composition => {
                assert!(!config.extends.is_empty(), "{relative}");
                assert!(!config.rules.is_empty(), "{relative}");
            }
        }
        let run = runner
            .run(&test, &path)
            .unwrap_or_else(|error| panic!("run {relative}: {error}"));
        assert!(run.passed(), "{relative}: {:#?}", run.mismatches);
    }
}

#[test]
fn every_bundle_requirement_value_reaches_production_compatibility_metadata() {
    let registry = alint_rules::builtin_registry();
    for (name, revision) in alint_dsl::bundled::catalog() {
        let spec = format!("{name}@{revision}");
        let body = alint_dsl::bundled::resolve(&spec).expect("catalog entry resolves");
        let raw: alint_core::Config = ayaml::from_str(body)
            .unwrap_or_else(|error| panic!("parse bundled ruleset {spec}: {error}"));
        let expected_requirement = raw.requires.map(|requirements| requirements.alint);

        let repository = tempfile::tempdir().expect("temporary bundle compatibility repository");
        let root = repository.path().join(".alint.yml");
        let uri = format!("alint://bundled/{spec}");
        std::fs::write(&root, format!("version: 1\nextends: [{uri}]\nrules: []\n"))
            .expect("write compatibility config");
        let loaded = alint_dsl::load_with(
            &root,
            &alint_dsl::LoadOptions::with_catalog(registry.catalog()),
        )
        .unwrap_or_else(|error| panic!("load {uri}: {error}"));
        let record = loaded
            .compatibility
            .iter()
            .find(|record| record.source == uri)
            .unwrap_or_else(|| panic!("{uri} compatibility metadata was not retained"));
        assert_eq!(record.requires_alint, expected_requirement, "{uri}");
    }
}
