//! Integration tests exercising `CONFIG_SCHEMA_V1` against real fixtures.
//!
//! Runs the published JSON Schema through a compliant validator to keep the
//! schema in lockstep with the actual DSL the engine accepts.

use std::fmt::Write as _;

use alint_dsl::CONFIG_SCHEMA_V1;

fn compile_schema() -> jsonschema::Validator {
    let schema_json: serde_json::Value =
        serde_json::from_str(CONFIG_SCHEMA_V1).expect("schema should be well-formed JSON");
    jsonschema::validator_for(&schema_json).expect("schema should compile")
}

fn yaml_to_json(yaml: &str) -> serde_json::Value {
    let value: ayaml::Value = alint_core::yaml::from_str(yaml).expect("yaml should parse");
    serde_json::to_value(value).expect("yaml → json should round-trip")
}

fn assert_valid(validator: &jsonschema::Validator, instance: &serde_json::Value, label: &str) {
    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|e| format!("{} at {}", e, e.instance_path()))
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

/// Every kind-specific rule option must render a non-blank Options-table cell on
/// alint.org (that table is generated from this schema). An option with no
/// `description` — inline, or on the `$ref` target it points to — ships as a
/// blank cell: the class of bug that once left 16 fields empty. This gate fails
/// the build if any rule-branch option would render blank, whether the schema is
/// hand-authored (`"description"` in the branch) or schemars-derived (a `///`
/// doc-comment on the `Options` field). The `kind` discriminator is exempt.
#[test]
fn every_rule_option_has_a_description() {
    let schema: serde_json::Value = serde_json::from_str(CONFIG_SCHEMA_V1).expect("valid JSON");
    let defs = schema["$defs"].as_object().expect("schema has $defs");

    let has_desc = |node: &serde_json::Value| {
        node.get("description")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
    };

    let mut gaps: Vec<String> = Vec::new();
    for (branch_name, branch) in defs {
        if !branch_name.starts_with("rule_") {
            continue;
        }
        let Some(props) = branch
            .get("properties")
            .and_then(serde_json::Value::as_object)
        else {
            continue;
        };
        for (prop_name, prop) in props {
            if prop_name == "kind" || has_desc(prop) {
                continue; // the discriminator, or an inline description is present
            }
            // No inline description — a `$ref` is acceptable only if its target is described.
            let ref_ok = prop
                .get("$ref")
                .and_then(serde_json::Value::as_str)
                .and_then(|r| r.rsplit('/').next())
                .and_then(|target| defs.get(target))
                .is_some_and(has_desc);
            if !ref_ok {
                gaps.push(format!("{branch_name}.{prop_name}"));
            }
        }
    }

    assert!(
        gaps.is_empty(),
        "these rule options have no description and would render as blank cells in \
         alint.org's Options tables — add one (a `///` doc-comment on the schemars \
         `Options` field, or a `\"description\"` in the hand-authored schema branch):\n  {}",
        gaps.join("\n  "),
    );
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
fn requirement_schema_matches_the_runtime_literal_grammar() {
    let validator = compile_schema();
    for accepted in ["0.0.0", "0.19.0", "18446744073709551615.0.1"] {
        let yaml = format!("version: 1\nrequires:\n  alint: '{accepted}'\nrules: []\n");
        let instance = yaml_to_json(&yaml);
        assert_valid(&validator, &instance, accepted);
        alint_core::yaml::from_str::<alint_core::Config>(&yaml).unwrap_or_else(|error| {
            panic!("runtime rejected schema-valid requirement {accepted:?}: {error}")
        });
    }

    for rejected in [
        "01.2.3",
        "1.2",
        "1.2.3.4",
        ">=1.2.3",
        "1.2.3-alpha",
        "1.2.3+build",
    ] {
        let yaml = format!("version: 1\nrequires:\n  alint: '{rejected}'\nrules: []\n");
        let instance = yaml_to_json(&yaml);
        assert_invalid(&validator, &instance, rejected);
        assert!(
            alint_core::yaml::from_str::<alint_core::Config>(&yaml).is_err(),
            "runtime accepted schema-invalid requirement {rejected:?}"
        );
    }

    for yaml in [
        "version: 1\nrequires: {}\nrules: []\n",
        "version: 1\nrequires: { alint: '0.19.0', other: x }\nrules: []\n",
        "version: 1\nrequires: { alint: [0, 19, 0] }\nrules: []\n",
    ] {
        let instance = yaml_to_json(yaml);
        assert_invalid(&validator, &instance, yaml);
        assert!(
            alint_core::yaml::from_str::<alint_core::Config>(yaml).is_err(),
            "runtime accepted {yaml:?}"
        );
    }
}

#[test]
fn accepts_every_rule_kind() {
    let validator = compile_schema();
    let yaml = include_str!("fixtures/all_kinds.yaml");
    let instance = yaml_to_json(yaml);
    assert_valid(&validator, &instance, "all_kinds.yaml fixture");
}

#[test]
fn sensitive_boolean_is_accepted_on_every_kind_and_nested_rule() {
    fn mark_rules(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(object) => {
                if object.contains_key("kind") {
                    object.insert("sensitive".into(), serde_json::Value::Bool(true));
                }
                for child in object.values_mut() {
                    mark_rules(child);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    mark_rules(item);
                }
            }
            _ => {}
        }
    }

    let mut instance = yaml_to_json(include_str!("fixtures/all_kinds.yaml"));
    mark_rules(&mut instance);
    assert_valid(
        &compile_schema(),
        &instance,
        "sensitive on every top-level and nested rule",
    );

    let config: alint_core::Config =
        serde_json::from_value(instance).expect("runtime model accepts sensitive booleans");
    let registry = alint_rules::builtin_registry();
    for spec in &config.rules {
        registry
            .build(spec)
            .unwrap_or_else(|error| panic!("{} rejected sensitive: {error}", spec.kind));
    }
}

#[test]
fn sensitive_rejects_non_boolean_values() {
    let instance = yaml_to_json(concat!(
        "version: 1\n",
        "rules:\n",
        "  - id: t\n",
        "    kind: file_exists\n",
        "    paths: a\n",
        "    sensitive: definitely\n",
        "    level: error\n",
    ));
    assert_invalid(&compile_schema(), &instance, "sensitive must be a boolean");
    assert!(
        serde_json::from_value::<alint_core::Config>(instance).is_err(),
        "runtime model must reject a non-boolean sensitive value"
    );
}

/// The `kind:` of each rule of a fixture.
fn fixture_kinds(yaml: &str) -> std::collections::BTreeSet<String> {
    let parsed: ayaml::Value = alint_core::yaml::from_str(yaml).expect("fixture parses");
    parsed
        .as_mapping()
        .and_then(|m| m.get(ayaml::Value::String("rules".into())))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|rule| {
                    rule.as_mapping()
                        .and_then(|m| m.get(ayaml::Value::String("kind".into())))
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

const ALL_KINDS: &str = include_str!("fixtures/all_kinds.yaml");
/// The kinds only a `version: 2` config may use (design
/// `v0.23/selectors-and-constraints.md` section 11).
const ALL_KINDS_V2: &str = include_str!("fixtures/all_kinds_v2.yaml");

/// Rot-prevention guard. Asserts the `all_kinds.yaml` fixture
/// covers every rule kind registered in `alint-rules` that a
/// version 1 config may use, and `all_kinds_v2.yaml` the others.
/// Without this gate, a newly-added rule kind ships with schema
/// validation only if the author manually remembers to update
/// the fixture; the v0.8 coverage audit found 18 of 66 names
/// covered, with no test catching new gaps.
///
/// Failure prints the missing kinds so the fix is to add a
/// minimal entry per kind to the fixture of its version.
#[test]
fn fixture_covers_every_registered_rule_kind() {
    let registry = alint_rules::builtin_registry();
    let registered: std::collections::BTreeSet<String> =
        registry.known_kinds().map(str::to_string).collect();
    // Each fixture holds the kinds of its version: `all_kinds.yaml` every
    // kind a version 1 config may use, `all_kinds_v2.yaml` the others.
    let needs_two = |kind: &String| registry.min_config_version(kind) > 1;
    let (two, one): (std::collections::BTreeSet<String>, _) =
        registered.iter().cloned().partition(needs_two);
    for (fixture, yaml, expected) in [
        ("all_kinds.yaml", ALL_KINDS, &one),
        ("all_kinds_v2.yaml", ALL_KINDS_V2, &two),
    ] {
        let fixture_kinds = fixture_kinds(yaml);
        let missing: Vec<&String> = expected.difference(&fixture_kinds).collect();
        let mut bullets = String::new();
        for k in &missing {
            let _ = writeln!(bullets, "  - {k}");
        }
        assert!(
            missing.is_empty(),
            "fixture `{fixture}` is missing {} of {} registered rule kinds. Add a minimal \
             entry per kind so schema validation has full coverage:\n{bullets}",
            missing.len(),
            expected.len(),
        );
        let stale: Vec<&String> = fixture_kinds.difference(expected).collect();
        let mut bullets = String::new();
        for k in &stale {
            let _ = writeln!(bullets, "  - {k}");
        }
        assert!(
            stale.is_empty(),
            "fixture `{fixture}` references {} kind(s) that are NOT registered for its \
             config version. Either remove the stale entries, register the kinds, or move \
             them to the other fixture:\n{bullets}",
            stale.len(),
        );
    }
}

/// The version 2 fixture validates against the draft version 2 schema, which
/// holds the kinds only version 2 has, and not against the version 1 schema.
#[test]
fn the_version_two_schema_accepts_every_version_two_kind() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/v2/config.json");
    // Outside the workspace (the published crate) the draft is absent.
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let schema: serde_json::Value = serde_json::from_str(&text).expect("the draft is JSON");
    let draft = jsonschema::validator_for(&schema).expect("the draft compiles");
    let instance = yaml_to_json(ALL_KINDS_V2);
    assert_valid(&draft, &instance, "all_kinds_v2.yaml fixture");
    assert_invalid(
        &compile_schema(),
        &instance,
        "all_kinds_v2.yaml under version 1",
    );
    // A version 1 config cannot use a kind of version 2.
    let one = yaml_to_json(
        "version: 1\nrules:\n  - id: c\n    kind: constraint\n    input: r\n    \
         assert: { unique: { key: [k] } }\n    level: error\n",
    );
    assert_invalid(&compile_schema(), &one, "a constraint rule under version 1");
}

/// Every registered rule kind — **including aliases** — must reject an
/// unknown option at load. Drives the loader (not just the schema) off
/// `all_kinds.yaml`, which `fixture_covers_every_registered_rule_kind`
/// guarantees holds one building entry per registered kind. Guards against
/// an alias registered via `register` instead of `register_optionless` (the
/// v0.13 `is_text` regression): the doc-example probe in alint-e2e's
/// `coverage_audit_doc_examples` misses such aliases because no documentation
/// example uses them, so only a registry-driven probe catches the divergence.
#[test]
fn every_registered_kind_rejects_an_unknown_option() {
    let registry = alint_rules::builtin_registry();
    let mut specs = Vec::new();
    for fixture in ["all_kinds.yaml", "all_kinds_v2.yaml"] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture);
        let config = alint_dsl::load_with(
            &path,
            &alint_dsl::LoadOptions::with_catalog(registry.catalog()),
        )
        .unwrap_or_else(|error| panic!("{fixture} loads: {error}"));
        specs.extend(config.rules);
    }

    let mut swallowers: Vec<String> = Vec::new();
    let mut probed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for spec in &specs {
        // Only probe specs that build cleanly, so a build failure under the
        // probe is attributable to the injected option, not a terse entry.
        if registry.build(spec).is_err() {
            continue;
        }
        let mut bogus = spec.clone();
        bogus.extra.insert("__alint_unknown_option_probe__", true);
        probed.insert(spec.kind.clone());
        if registry.build(&bogus).is_ok() {
            swallowers.push(spec.kind.clone());
        }
    }
    swallowers.sort();
    swallowers.dedup();
    assert!(
        swallowers.is_empty(),
        "rule kind(s) SILENTLY ACCEPT an unknown option (register via \
         `register_optionless` if the kind takes none, else propagate \
         `deserialize_options()`): {swallowers:?}",
    );

    // Completeness: every registered kind must have been probed, so a newly
    // added kind or alias can't slip the check by lacking a building fixture
    // entry. `fixture_covers_every_registered_rule_kind` guarantees presence;
    // this asserts the entry builds and was actually probed.
    let registered: std::collections::BTreeSet<String> =
        registry.known_kinds().map(str::to_string).collect();
    let unprobed: Vec<&String> = registered.difference(&probed).collect();
    assert!(
        unprobed.is_empty(),
        "registered kind(s) not probed for unknown-option rejection — their \
         `all_kinds.yaml` entry must build cleanly: {unprobed:?}",
    );
}

#[test]
fn accepts_dogfood_config() {
    // The repo's own `.alint.yml` lives outside this crate; load it at
    // runtime from the workspace root so the test compiles cleanly in
    // the published crate (where the workspace checkout is absent).
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.alint.yml");
    let Ok(yaml) = std::fs::read_to_string(&path) else {
        return;
    };
    let validator = compile_schema();
    let instance = yaml_to_json(&yaml);
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

/// Every fix op the engine deserializes has a schema branch, and the schema
/// agrees with serde on the op's `applicability` field and on unknown keys.
/// The hand-written `$defs/fix` once covered only 15 of the 26 ops and rejected
/// `applicability` on the hygiene ops, so schema-aware editors flagged configs
/// `alint validate-config` accepted (audit 2026-10).
#[test]
fn fix_schema_matches_every_fix_op() {
    let schema: serde_json::Value = serde_json::from_str(CONFIG_SCHEMA_V1).expect("valid JSON");
    let fix_schema = serde_json::json!({
        "$schema": schema["$schema"].clone(),
        "$ref": "#/$defs/fix",
        "$defs": schema["$defs"].clone(),
    });
    let validator = jsonschema::validator_for(&fix_schema).expect("fix schema should compile");
    let branches = schema["$defs"]["fix"]["oneOf"]
        .as_array()
        .expect("$defs/fix is a oneOf");
    let schema_ops: Vec<&str> = branches
        .iter()
        .map(|branch| {
            let branch = branch["$ref"].as_str().map_or(branch, |reference| {
                schema
                    .pointer(reference.strip_prefix('#').expect("local schema reference"))
                    .expect("fix branch reference resolves")
            });
            branch["required"][0]
                .as_str()
                .expect("fix branch requires its operation")
        })
        .collect();

    for op in alint_core::FixSpec::ALL_OP_NAMES {
        assert!(
            schema_ops.contains(op),
            "fix op `{op}` has no `$defs/fix` branch"
        );
        // The minimal body serde requires for this op.
        let base = match *op {
            "replace" => "replacement: x\n",
            "command" => "run: [x]\n",
            "file_create" | "file_prepend" | "file_append" | "insert_header" => "content: x\n",
            _ => "",
        };
        for (extra, label) in [
            ("applicability: suggestion\n", "applicability"),
            ("bogus: 1\n", "unknown key"),
        ] {
            let body = format!("{base}{extra}");
            let fix_yaml = format!("{op}:\n{}", indent(&body));
            let serde_ok = alint_core::yaml::from_str::<alint_core::FixSpec>(&fix_yaml).is_ok();
            let fix = yaml_to_json(&fix_yaml);
            assert_eq!(
                validator.is_valid(&fix),
                serde_ok,
                "schema and serde disagree on `{op}` with {label}"
            );
        }
    }
}

fn indent(s: &str) -> String {
    s.lines().fold(String::new(), |mut out, l| {
        let _ = writeln!(out, "  {l}");
        out
    })
}

#[test]
fn extends_rule_overrides_and_schema_parity_constraints() {
    let validator = compile_schema();

    // Valid partial overrides when `extends:` is non-empty (including clearing
    // `fix: null` / `scope_filter: null` and overriding kind-specific options).
    assert_valid(
        &validator,
        &yaml_to_json(
            "version: 1\nextends:\n  - alint://bundled/rust@v1\nrules:\n  - id: rust-max-lines\n    level: off\n  - id: oss-no-trailing-whitespace\n    fix: null\n    scope_filter: null\n  - id: rust-max-lines\n    max_lines: 600\n",
        ),
        "extends with partial rule overrides",
    );

    // Without `extends:`, partial overrides without `kind:` remain rejected.
    assert_invalid(
        &validator,
        &yaml_to_json("version: 1\nrules:\n  - id: rust-max-lines\n    level: off\n"),
        "standalone partial rule without kind",
    );

    // Full rules inside an `extends:` config still reject unknown options.
    assert_invalid(
        &validator,
        &yaml_to_json(
            "version: 1\nextends:\n  - alint://bundled/rust@v1\nrules:\n  - id: bad\n    kind: file_exists\n    paths: a\n    level: error\n    bogus_opt: true\n",
        ),
        "extends with full rule carrying unknown option",
    );

    // Fact IDs match `when:` lexer grammar (`[A-Za-z_][A-Za-z0-9_]*`, no hyphens).
    assert_valid(
        &validator,
        &yaml_to_json(
            "version: 1\nfacts:\n  - id: _hasRust_1\n    any_file_exists: Cargo.toml\nrules: []\n",
        ),
        "valid fact id with leading underscore and uppercase",
    );
    assert_invalid(
        &validator,
        &yaml_to_json(
            "version: 1\nfacts:\n  - id: has-rust\n    any_file_exists: Cargo.toml\nrules: []\n",
        ),
        "hyphenated fact id rejected by schema",
    );

    // `insert_header` requires exactly one of `content` / `content_from`.
    assert_invalid(
        &validator,
        &yaml_to_json(
            "version: 1\nrules:\n  - id: h\n    kind: file_header\n    paths: '*.rs'\n    pattern: '//'\n    level: error\n    fix: { insert_header: {} }\n",
        ),
        "insert_header empty body",
    );
    assert_invalid(
        &validator,
        &yaml_to_json(
            "version: 1\nrules:\n  - id: h\n    kind: file_header\n    paths: '*.rs'\n    pattern: '//'\n    level: error\n    fix: { insert_header: { content: '//', content_from: h.txt } }\n",
        ),
        "insert_header both content and content_from",
    );

    // `create_and_register` forbids setting both `content` and `content_from`.
    assert_invalid(
        &validator,
        &yaml_to_json(
            "version: 1\nrules:\n  - id: reg\n    kind: cross_file\n    source: { files: 'a.txt' }\n    targets: [{ file: Cargo.toml, extract: { toml: '$.workspace.members[*]' } }]\n    relation: registered\n    level: error\n    fix: { create_and_register: { content: x, content_from: y } }\n",
        ),
        "create_and_register both content and content_from",
    );

    // `command` fix `timeout: 0` is rejected (matches runtime `timeout >= 1`).
    assert_invalid(
        &validator,
        &yaml_to_json(
            "version: 1\nrules:\n  - id: cmd\n    kind: command\n    paths: '*.rs'\n    command: ['true']\n    level: error\n    fix: { command: { run: ['true'], timeout: 0 } }\n",
        ),
        "command fix timeout 0",
    );

    // Empty `allow_out_of_root: {}` is rejected (matches runtime `AllowOutOfRoot`).
    assert_invalid(
        &validator,
        &yaml_to_json("version: 1\nallow_out_of_root: {}\nrules: []\n"),
        "empty allow_out_of_root map",
    );
}
