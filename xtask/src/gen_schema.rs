//! `xtask gen-schema` - assemble `schemas/v1/config.json` from the hand-written
//! base, replacing the `$defs/rule_*` branch of each migrated rule kind with a
//! schemars-derived schema. See ADR-0001 and docs/design/spec-driven-development.md.
//!
//! Migration is incremental: kinds not yet migrated pass through from the
//! committed base verbatim, so the published schema stays complete and valid at
//! every step. Fidelity is checked semantically (validation behavior), not by
//! byte-equality, because schemars emits cosmetic keywords (e.g. `format: uint`)
//! that the hand-written schema omits without changing what validates.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde_json::Value;

fn root_schema_path() -> Result<PathBuf> {
    Ok(crate::workspace_root()?.join("schemas/v1/config.json"))
}

fn in_crate_schema_path() -> Result<PathBuf> {
    Ok(crate::workspace_root()?.join("crates/alint-dsl/schemas/v1/config.json"))
}

/// Build the generated schema in memory: the committed base with every migrated
/// `$defs` entry replaced by its schemars-derived equivalent.
pub fn build_generated_schema() -> Result<Value> {
    let base_src = std::fs::read_to_string(root_schema_path()?)
        .context("read base schema schemas/v1/config.json")?;
    let mut schema: Value = serde_json::from_str(&base_src).context("parse base schema as JSON")?;

    let defs = schema
        .get_mut("$defs")
        .and_then(Value::as_object_mut)
        .context("base schema has no `$defs` object")?;

    for (def_name, def_schema) in alint_rules::migrated_rule_defs() {
        if !defs.contains_key(def_name) {
            bail!("migrated def `{def_name}` is not present in the base schema `$defs`");
        }
        defs.insert(def_name.to_string(), def_schema);
    }
    Ok(schema)
}

fn render(schema: &Value) -> Result<String> {
    let mut rendered = serde_json::to_string_pretty(schema)?;
    rendered.push('\n');
    Ok(rendered)
}

/// Write the generated schema to the root and in-crate copies, or (with `check`)
/// fail if either is stale.
pub fn run(check: bool) -> Result<()> {
    let generated = build_generated_schema()?;
    let rendered = render(&generated)?;

    if check {
        let root = std::fs::read_to_string(root_schema_path()?)?;
        let in_crate = std::fs::read_to_string(in_crate_schema_path()?)?;
        if root != rendered || in_crate != rendered {
            bail!(
                "schemas/v1/config.json is stale (or not yet in generated form). \
                 Run `cargo run -p xtask -- gen-schema` to regenerate and commit the result."
            );
        }
        println!("schema is up to date");
        return Ok(());
    }

    std::fs::write(root_schema_path()?, &rendered)?;
    std::fs::write(in_crate_schema_path()?, &rendered)?;
    println!("wrote schemas/v1/config.json and the in-crate copy");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_to_json(rel_path: &str) -> Value {
        let path = crate::workspace_root().unwrap().join(rel_path);
        let yaml = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).unwrap();
        serde_json::to_value(value).unwrap()
    }

    fn committed_schema() -> Value {
        let src = std::fs::read_to_string(root_schema_path().unwrap()).unwrap();
        serde_json::from_str(&src).unwrap()
    }

    // Representative file_header rule configs: the first three are valid, the
    // last two must be rejected (missing required `pattern`; `lines` below the
    // minimum of 1).
    fn file_header_cases() -> Vec<Value> {
        vec![
            serde_json::json!({"version":1,"rules":[{"id":"h","kind":"file_header","level":"error","paths":"**/*.rs","pattern":"// SPDX"}]}),
            serde_json::json!({"version":1,"rules":[{"id":"h","kind":"file_header","level":"error","paths":"**/*.rs","pattern":"// SPDX","lines":5}]}),
            serde_json::json!({"version":1,"rules":[{"id":"h","kind":"header","level":"error","paths":"**/*.rs","pattern":"// SPDX"}]}),
            serde_json::json!({"version":1,"rules":[{"id":"h","kind":"file_header","level":"error","paths":"**/*.rs"}]}),
            serde_json::json!({"version":1,"rules":[{"id":"h","kind":"file_header","level":"error","paths":"**/*.rs","pattern":"// SPDX","lines":0}]}),
        ]
    }

    #[test]
    fn generated_schema_accepts_all_kinds_fixture() {
        let schema = build_generated_schema().unwrap();
        let validator = jsonschema::validator_for(&schema).unwrap();
        let cfg = config_to_json("crates/alint-dsl/tests/fixtures/all_kinds.yaml");
        assert!(
            validator.is_valid(&cfg),
            "all_kinds.yaml must validate against the generated schema (the derived \
             file_header branch must not have narrowed acceptance)"
        );
    }

    #[test]
    fn generated_and_committed_agree_on_file_header_configs() {
        let committed = jsonschema::validator_for(&committed_schema()).unwrap();
        let generated = jsonschema::validator_for(&build_generated_schema().unwrap()).unwrap();
        for case in file_header_cases() {
            assert_eq!(
                committed.is_valid(&case),
                generated.is_valid(&case),
                "committed and generated schema disagree for: {case}"
            );
        }
    }

    #[test]
    fn generated_schema_enforces_file_header_contract() {
        let generated = jsonschema::validator_for(&build_generated_schema().unwrap()).unwrap();
        let cases = file_header_cases();
        // valid baseline accepted
        assert!(generated.is_valid(&cases[0]));
        // missing required `pattern` rejected
        assert!(!generated.is_valid(&cases[3]));
        // `lines: 0` (below minimum 1) rejected
        assert!(!generated.is_valid(&cases[4]));
    }
}
