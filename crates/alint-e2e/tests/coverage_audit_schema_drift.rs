//! Hard audit: every rule kind registered with `RuleRegistry` (the
//! runtime source of truth) MUST have a matching dispatch entry in
//! `schemas/v1/config.json` (the editor-LSP source of truth).
//!
//! Without this audit, adding a new rule kind in alint-rules without
//! also extending the schema causes a silent drift: the rule loads
//! and runs at runtime, but configs using it surface a "value does
//! not match the kind discriminator" error in any editor with the
//! schema attached. Catches the drift at PR time.
//!
//! Aliases the registry accepts for legacy compatibility (e.g.
//! `header` for `file_header`) appear in the schema as multi-value
//! `kind: { enum: [<canonical>, <alias>] }` discriminators, so the
//! drift check below treats them as first-class. No separate
//! allowlist is needed.
//!
//! Spot-checks at the bottom verify the schema actively rejects a
//! handful of the headline pitfalls from
//! `docs/development/CONFIG-AUTHORING.md`. Those tests serve double
//! duty: drift-detector for the schema's pitfall coverage AND
//! continuously-verified evidence that the magic-comment LSP
//! workflow catches what the doc claims it catches.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use alint_rules::builtin_registry;

// Aliases used to be tracked here; the schema actually supports them
// via `kind: { enum: ["<canonical>", "<alias>"] }` discriminators
// (see e.g. `rule_file_content_matches`). The drift audit below
// pulls both single-`const` and `enum` discriminators, so aliases
// flow through automatically.

fn schema_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("schemas/v1/config.json")
}

fn parse_schema() -> serde_json::Value {
    let text = fs::read_to_string(schema_path()).expect("read schemas/v1/config.json");
    serde_json::from_str(&text).expect("parse schemas/v1/config.json")
}

/// Extract the set of rule kinds the schema's dispatch oneOf
/// recognises — i.e. every `$ref: #/$defs/rule_<kind>` in the
/// `rule_kind_dispatch` block, with the kind discriminator value(s)
/// pulled from the referenced sub-schema.
///
/// Each rule schema's `kind` discriminator is either:
/// - `{ "const": "<canonical>" }` for rules with a single name, or
/// - `{ "enum": ["<canonical>", "<alias>", …] }` for rules that
///   also accept legacy short aliases.
///
/// Both forms contribute every listed kind name to the returned set
/// so the drift audit treats aliases on the same footing as
/// canonicals (the runtime registry registers each one).
fn schema_rule_kinds(schema: &serde_json::Value) -> BTreeSet<String> {
    let dispatch = schema
        .pointer("/$defs/rule_kind_dispatch/oneOf")
        .and_then(|v| v.as_array())
        .expect("schema must have /$defs/rule_kind_dispatch/oneOf");

    let mut kinds = BTreeSet::new();
    for entry in dispatch {
        let Some(refer) = entry.get("$ref").and_then(|v| v.as_str()) else {
            continue;
        };
        let key = refer.strip_prefix("#/$defs/").unwrap_or("");
        let Some(rule_def) = schema.pointer(&format!("/$defs/{key}")) else {
            continue;
        };
        let kind_node = rule_def.pointer("/properties/kind");

        // Single-name discriminator.
        if let Some(name) = kind_node
            .and_then(|v| v.get("const"))
            .and_then(|v| v.as_str())
        {
            kinds.insert(name.to_string());
            continue;
        }

        // Multi-name discriminator (canonical + aliases).
        if let Some(arr) = kind_node
            .and_then(|v| v.get("enum"))
            .and_then(|v| v.as_array())
        {
            for v in arr {
                if let Some(name) = v.as_str() {
                    kinds.insert(name.to_string());
                }
            }
        }
    }
    kinds
}

#[test]
fn every_registered_rule_kind_has_a_schema_dispatch_entry() {
    let registry = builtin_registry();
    let registered: BTreeSet<String> = registry.known_kinds().map(str::to_string).collect();

    let schema = parse_schema();
    let in_schema = schema_rule_kinds(&schema);

    let missing: Vec<String> = registered.difference(&in_schema).cloned().collect();
    assert!(
        missing.is_empty(),
        "{} rule kinds registered with `RuleRegistry` are missing from \
         `schemas/v1/config.json`'s `rule_kind_dispatch` oneOf:\n\n  - {}\n\n\
         Add a `rule_<kind>` definition + a $ref to the dispatch block. \
         If the kind is an alias of an existing canonical form, list \
         it in the canonical's `kind` enum (see e.g. `rule_file_header`).",
        missing.len(),
        missing.join("\n  - "),
    );

    let extra: Vec<String> = in_schema.difference(&registered).cloned().collect();
    assert!(
        extra.is_empty(),
        "{} rule kinds present in `schemas/v1/config.json` but NOT \
         registered with `RuleRegistry`:\n\n  - {}\n\n\
         Likely cause: a rule was deleted from alint-rules but its \
         schema entry wasn't removed. Drop the `rule_<kind>` def + \
         the dispatch $ref in the same commit.",
        extra.len(),
        extra.join("\n  - "),
    );
}

// ─── Pitfall spot-checks ──────────────────────────────────────────────
//
// Each test loads the full v1 schema and asserts the validator
// rejects a YAML config that exhibits the named pitfall. Keeps the
// schema's "we catch this at editor-keystroke time" claim continuously
// verified.

fn validate_against_schema(yaml_doc: &serde_json::Value) -> Result<(), String> {
    let schema = parse_schema();
    let validator = jsonschema::validator_for(&schema).map_err(|e| format!("compile: {e}"))?;
    if let Err(err) = validator.validate(yaml_doc) {
        return Err(format!("{err}"));
    }
    Ok(())
}

#[test]
fn schema_rejects_argv_on_command_rule() {
    // Pitfall #1.
    let doc = serde_json::json!({
        "version": 1,
        "rules": [{
            "id": "x", "kind": "command", "level": "error",
            "paths": "**/*.sh",
            "argv": ["foo"],
        }],
    });
    assert!(
        validate_against_schema(&doc).is_err(),
        "schema should reject `argv:` on a `command` rule (pitfall #1)",
    );
}

#[test]
fn schema_rejects_secondary_on_pair_rule() {
    // Pitfall #4.
    let doc = serde_json::json!({
        "version": 1,
        "rules": [{
            "id": "x", "kind": "pair", "level": "error",
            "primary": "**/*.c", "secondary": "{dir}/{stem}.h",
        }],
    });
    assert!(
        validate_against_schema(&doc).is_err(),
        "schema should reject `secondary:` on a `pair` rule (pitfall #4 — use `partner:`)",
    );
}

#[test]
fn schema_rejects_pattern_on_file_starts_with() {
    // Pitfall #9.
    let doc = serde_json::json!({
        "version": 1,
        "rules": [{
            "id": "x", "kind": "file_starts_with", "level": "warning",
            "paths": "**/*.goml",
            "pattern": "// ",
        }],
    });
    assert!(
        validate_against_schema(&doc).is_err(),
        "schema should reject `pattern:` on `file_starts_with` (pitfall #9 — use `prefix:`)",
    );
}

#[test]
fn schema_rejects_empty_prefix_on_file_starts_with() {
    // Pitfall #15.
    let doc = serde_json::json!({
        "version": 1,
        "rules": [{
            "id": "x", "kind": "file_starts_with", "level": "warning",
            "paths": "**/*",
            "prefix": "",
        }],
    });
    assert!(
        validate_against_schema(&doc).is_err(),
        "schema should reject empty `prefix:` on `file_starts_with` (pitfall #15)",
    );
}

#[test]
fn schema_rejects_matches_on_path_equals() {
    // Pitfall #16.
    let doc = serde_json::json!({
        "version": 1,
        "rules": [{
            "id": "x", "kind": "toml_path_equals", "level": "warning",
            "paths": "Cargo.toml",
            "path": "$.package.publish",
            "matches": "^false$",
        }],
    });
    assert!(
        validate_against_schema(&doc).is_err(),
        "schema should reject `matches:` on `toml_path_equals` (pitfall #16 — use `equals:`)",
    );
}

#[test]
fn schema_accepts_canonical_command_rule() {
    let doc = serde_json::json!({
        "version": 1,
        "rules": [{
            "id": "x", "kind": "command", "level": "error",
            "paths": "**/*.sh",
            "command": ["shellcheck", "{path}"],
        }],
    });
    let result = validate_against_schema(&doc);
    assert!(
        result.is_ok(),
        "canonical `command` rule should validate cleanly: {result:?}",
    );
}

#[test]
fn schema_accepts_canonical_path_equals_rule() {
    let doc = serde_json::json!({
        "version": 1,
        "rules": [{
            "id": "x", "kind": "toml_path_equals", "level": "warning",
            "paths": "Cargo.toml",
            "path": "$.package.publish",
            "equals": false,
        }],
    });
    let result = validate_against_schema(&doc);
    assert!(
        result.is_ok(),
        "canonical `*_path_equals` with native bool should validate cleanly: {result:?}",
    );
}
