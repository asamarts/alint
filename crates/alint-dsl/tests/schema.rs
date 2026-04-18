//! Integration tests exercising `CONFIG_SCHEMA_V1` against real fixtures.
//!
//! Runs the published JSON Schema through a compliant validator to keep the
//! schema in lockstep with the actual DSL the engine accepts.

use alint_dsl::CONFIG_SCHEMA_V1;

fn compile_schema() -> jsonschema::Validator {
    let schema_json: serde_json::Value =
        serde_json::from_str(CONFIG_SCHEMA_V1).expect("schema should be well-formed JSON");
    jsonschema::validator_for(&schema_json).expect("schema should compile")
}

fn yaml_to_json(yaml: &str) -> serde_json::Value {
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(yaml).expect("yaml should parse");
    serde_json::to_value(value).expect("yaml → json should round-trip")
}

fn assert_valid(validator: &jsonschema::Validator, instance: &serde_json::Value, label: &str) {
    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|e| format!("{} at {}", e, e.instance_path))
        .collect();
    if !errors.is_empty() {
        for e in &errors {
            eprintln!("schema error ({label}): {e}");
        }
        panic!("{label} did not validate against CONFIG_SCHEMA_V1");
    }
}

fn assert_invalid(validator: &jsonschema::Validator, instance: &serde_json::Value, label: &str) {
    assert!(
        !validator.is_valid(instance),
        "{label} was expected to fail schema validation but passed",
    );
}

#[test]
fn schema_is_well_formed_json() {
    let _: serde_json::Value = serde_json::from_str(CONFIG_SCHEMA_V1).expect("valid JSON");
}

#[test]
fn schema_compiles_as_draft_2020_12() {
    let _ = compile_schema();
}

#[test]
fn accepts_minimal_config() {
    let validator = compile_schema();
    let instance = yaml_to_json("version: 1\nrules: []\n");
    assert_valid(&validator, &instance, "minimal config");
}

#[test]
fn accepts_every_rule_kind() {
    let validator = compile_schema();
    let yaml = include_str!("fixtures/all_kinds.yaml");
    let instance = yaml_to_json(yaml);
    assert_valid(&validator, &instance, "all_kinds.yaml fixture");
}

#[test]
fn accepts_dogfood_config() {
    let validator = compile_schema();
    let yaml = include_str!("../../../.alint.yml");
    let instance = yaml_to_json(yaml);
    assert_valid(&validator, &instance, "repo dogfood .alint.yml");
}

#[test]
fn rejects_wrong_version() {
    let validator = compile_schema();
    let instance = yaml_to_json("version: 2\nrules: []\n");
    assert_invalid(&validator, &instance, "version: 2");
}

#[test]
fn rejects_unknown_top_level_field() {
    let validator = compile_schema();
    let yaml = "version: 1\nbogus_key: true\nrules: []\n";
    let instance = yaml_to_json(yaml);
    assert_invalid(&validator, &instance, "unknown top-level field");
}

#[test]
fn rejects_unknown_rule_kind() {
    let validator = compile_schema();
    let yaml = r"
version: 1
rules:
  - id: bogus
    kind: not_a_real_kind
    paths: foo
    level: error
";
    let instance = yaml_to_json(yaml);
    assert_invalid(&validator, &instance, "unknown rule kind");
}

#[test]
fn rejects_rule_missing_required_kind_field() {
    let validator = compile_schema();
    // file_max_size requires `max_bytes`; omit it.
    let yaml = r"
version: 1
rules:
  - id: missing-max-bytes
    kind: file_max_size
    paths: '**'
    level: warning
";
    let instance = yaml_to_json(yaml);
    assert_invalid(&validator, &instance, "file_max_size without max_bytes");
}

#[test]
fn rejects_rule_unknown_option() {
    let validator = compile_schema();
    // file_exists does not accept a `not_a_real_field` property.
    let yaml = r"
version: 1
rules:
  - id: bad
    kind: file_exists
    paths: foo
    level: error
    not_a_real_field: oops
";
    let instance = yaml_to_json(yaml);
    assert_invalid(&validator, &instance, "file_exists with stray property");
}

#[test]
fn rejects_invalid_level() {
    let validator = compile_schema();
    let yaml = r"
version: 1
rules:
  - id: bad
    kind: file_exists
    paths: foo
    level: critical
";
    let instance = yaml_to_json(yaml);
    assert_invalid(&validator, &instance, "unknown level");
}

#[test]
fn rejects_invalid_rule_id_shape() {
    let validator = compile_schema();
    let yaml = r"
version: 1
rules:
  - id: BadUpperCaseID
    kind: file_exists
    paths: foo
    level: error
";
    let instance = yaml_to_json(yaml);
    assert_invalid(&validator, &instance, "rule id with uppercase");
}
