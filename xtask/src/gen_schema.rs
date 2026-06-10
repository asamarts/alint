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

    for (def_name, mut options) in alint_rules::migrated_option_schemas() {
        // Normalize derived descriptions BEFORE merging, so the merged enum
        // definitions match the committed (already-normalized) copy on re-runs;
        // otherwise the collision guard below would compare a normalized base
        // def against a freshly-derived (un-normalized) one and bail, making
        // generation non-idempotent.
        normalize_descriptions(&mut options);
        // schemars emits an option field's enum/struct type as a `$ref` to a
        // `#/$defs/<TypeName>` definition carried in the derived schema's own
        // `$defs`. Merge those definitions into the main schema's `$defs` so the
        // refs resolve (e.g. commented_out_code's `language: #/$defs/Language`).
        if let Some(extra_defs) = options.get("$defs").and_then(Value::as_object) {
            for (name, def) in extra_defs {
                // Guard against two distinct Rust types sharing one schemars def
                // name (e.g. pair_hash::Format vs structured_path::Format): a
                // silent last-write-wins would point a `$ref` at the wrong type.
                if defs.get(name).is_some_and(|existing| existing != def) {
                    bail!(
                        "schemars `$defs` collision on `{name}` while composing `{def_name}`: \
                         two distinct types share that name. Rename one with \
                         `#[schemars(rename = \"...\")]` on its Rust type."
                    );
                }
                defs.insert(name.clone(), def.clone());
            }
        }
        let base = defs
            .get(def_name)
            .with_context(|| format!("migrated def `{def_name}` is not present in `$defs`"))?
            .clone();
        defs.insert(def_name.to_string(), compose_branch(&base, &options)?);
    }

    normalize_descriptions(&mut schema);
    Ok(schema)
}

/// Collapse internal whitespace (including the `\n` schemars inserts between
/// wrapped rustdoc lines) in every `description` string, so the published schema
/// carries clean single-line prose rather than literal newlines.
fn normalize_descriptions(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if key == "description" {
                    if let Value::String(text) = child {
                        *text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    }
                } else {
                    normalize_descriptions(child);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(normalize_descriptions),
        _ => {}
    }
}

/// Compose a `$defs/rule_<kind>` branch from the schemars-derived options schema
/// and the committed base branch. The `kind` discriminator (including aliases)
/// and the `paths` property/requirement are preserved from the base (they are
/// stable and not expressible as plain Rust struct fields); the option
/// properties and their `required` set come from the derived schema, so an
/// option rename or type change in the Rust struct propagates automatically.
fn compose_branch(base: &Value, options: &Value) -> Result<Value> {
    let base_props = base.get("properties").and_then(Value::as_object);
    let base_required = base.get("required").and_then(Value::as_array);

    let mut properties = serde_json::Map::new();
    if let Some(kind) = base_props.and_then(|p| p.get("kind")) {
        properties.insert("kind".to_string(), kind.clone());
    }
    if let Some(paths) = base_props.and_then(|p| p.get("paths")) {
        properties.insert("paths".to_string(), paths.clone());
    }
    if let Some(opts) = options.get("properties").and_then(Value::as_object) {
        for (key, value) in opts {
            let mut prop = value.clone();
            // Safety net for fields schemars renders as a bare `$ref` (enums):
            // the derived prop carries neither the base `description` nor its
            // `default`, so restore both from the committed base when the derived
            // schema omits them. Keeps option docs and defaults intact during an
            // incremental rollout.
            if let Some(obj) = prop.as_object_mut() {
                let base_prop = base_props.and_then(|p| p.get(key));
                for carried in ["description", "default"] {
                    if !obj.contains_key(carried) {
                        if let Some(v) = base_prop.and_then(|p| p.get(carried)) {
                            obj.insert(carried.to_string(), v.clone());
                        }
                    }
                }
            }
            properties.insert(key.clone(), prop);
        }
    }

    let mut required: Vec<Value> = Vec::new();
    if base_required.is_some_and(|r| r.iter().any(|x| x == "paths")) {
        required.push(Value::from("paths"));
    }
    if let Some(req) = options.get("required").and_then(Value::as_array) {
        required.extend(req.iter().cloned());
    }

    // Start from the base branch so branch-level keywords the assembler does not
    // model (the rule `description`, and any branch-level `anyOf`/`oneOf` such as
    // git_commit_message's "at least one of pattern/subject_max_length") survive;
    // then overwrite only the property set and required list with the composed ones.
    let mut branch = base.clone();
    let obj = branch
        .as_object_mut()
        .expect("rule branch is a JSON object");
    obj.insert("type".to_string(), Value::from("object"));
    obj.insert("properties".to_string(), Value::Object(properties));
    if required.is_empty() {
        obj.remove("required");
    } else {
        obj.insert("required".to_string(), Value::Array(required));
    }
    assert_branch_combinators_resolve(&branch)?;
    Ok(branch)
}

/// A branch-level `anyOf`/`oneOf`/`allOf` preserved from the base hard-codes
/// property names in its sub-schemas' `required` (e.g. `git_commit_message`'s "at
/// least one of `pattern`/`subject_max_length`/`requires_body`"). If a Rust field
/// rename drops one, the constraint would silently reference a property the
/// derived schema no longer defines. Fail loudly instead.
fn assert_branch_combinators_resolve(branch: &Value) -> Result<()> {
    let props: std::collections::HashSet<&str> = branch
        .get("properties")
        .and_then(Value::as_object)
        .map(|m| m.keys().map(String::as_str).collect())
        .unwrap_or_default();
    for combinator in ["anyOf", "oneOf", "allOf"] {
        let Some(subs) = branch.get(combinator).and_then(Value::as_array) else {
            continue;
        };
        for sub in subs {
            let Some(reqs) = sub.get("required").and_then(Value::as_array) else {
                continue;
            };
            for name in reqs.iter().filter_map(Value::as_str) {
                if !props.contains(name) {
                    bail!(
                        "branch-level `{combinator}` references property `{name}` that the \
                         derived schema does not define (likely a renamed Rust field); update \
                         the base branch's `{combinator}` in schemas/v1/config.json."
                    );
                }
            }
        }
    }
    Ok(())
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
        let root_path = root_schema_path()?;
        let in_crate_path = in_crate_schema_path()?;
        let root = std::fs::read_to_string(&root_path)
            .with_context(|| format!("read {}", root_path.display()))?;
        let in_crate = std::fs::read_to_string(&in_crate_path)
            .with_context(|| format!("read {}", in_crate_path.display()))?;
        let stale: Vec<&str> = [
            (root != rendered).then_some("schemas/v1/config.json"),
            (in_crate != rendered).then_some("crates/alint-dsl/schemas/v1/config.json"),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !stale.is_empty() {
            bail!(
                "schema is stale (or not yet in generated form): {}. \
                 Run `cargo run -p xtask -- gen-schema` to regenerate and commit the result.",
                stale.join(", ")
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

    fn collect_yaml(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    collect_yaml(&path, out);
                } else if path.extension().is_some_and(|x| x == "yml" || x == "yaml") {
                    out.push(path);
                }
            }
        }
    }

    /// Broad fidelity gate: every real config in the repo (the all-kinds
    /// fixture, every bundled ruleset, every example) must get the SAME
    /// accept/reject verdict from the committed and the generated schema. These
    /// are all valid configs, so this rules out the generated schema NARROWING
    /// acceptance for any migrated kind. (Widening is caught by the rejection
    /// cases in `cargo test -p alint-dsl` against the regenerated schema.)
    #[test]
    fn generated_and_committed_agree_on_real_configs() {
        let root = crate::workspace_root().unwrap();
        let committed = jsonschema::validator_for(&committed_schema()).unwrap();
        let generated = jsonschema::validator_for(&build_generated_schema().unwrap()).unwrap();

        let mut configs = vec![root.join("crates/alint-dsl/tests/fixtures/all_kinds.yaml")];
        collect_yaml(&root.join("crates/alint-dsl/rulesets/v1"), &mut configs);
        if let Ok(entries) = std::fs::read_dir(root.join("examples")) {
            for entry in entries.flatten() {
                let cfg = entry.path().join(".alint.yml");
                if cfg.is_file() {
                    configs.push(cfg);
                }
            }
        }

        let mut checked = 0;
        for path in configs {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(value) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text) else {
                continue;
            };
            let json = serde_json::to_value(value).unwrap();
            assert_eq!(
                committed.is_valid(&json),
                generated.is_valid(&json),
                "committed vs generated schema disagree for {}",
                path.display()
            );
            checked += 1;
        }
        assert!(
            checked > 10,
            "expected many configs, only checked {checked}"
        );
    }

    #[test]
    fn migrated_option_schemas_list_is_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for (name, _) in alint_rules::migrated_option_schemas() {
            assert!(seen.insert(name), "duplicate migrated def name: {name}");
        }
        // Exercises the present-in-`$defs` check and the enum `$defs` collision
        // guard inside `build_generated_schema`.
        build_generated_schema().unwrap();
    }

    #[test]
    fn generated_schema_rejects_bad_options_on_migrated_kinds() {
        let v = jsonschema::validator_for(&build_generated_schema().unwrap()).unwrap();
        // Stray unknown option on a migrated kind (caught by the shared
        // `unevaluatedProperties: false`).
        assert!(!v.is_valid(&serde_json::json!(
            {"version":1,"rules":[{"id":"x","kind":"file_header","level":"error","paths":"a","pattern":"p","bogus":1}]}
        )));
        // Invalid enum value on a migrated enum-typed field (locks in that the
        // enum constraint survived the schemars `oneOf`/`$defs` round-trip).
        assert!(!v.is_valid(&serde_json::json!(
            {"version":1,"rules":[{"id":"x","kind":"pair_hash","level":"error","source":"a","target":"b","algorithm":"md5"}]}
        )));
        // Valid baseline accepted.
        assert!(v.is_valid(&serde_json::json!(
            {"version":1,"rules":[{"id":"x","kind":"pair_hash","level":"error","source":"a","target":"b","algorithm":"sha256"}]}
        )));
    }

    #[test]
    fn gen_schema_check_passes_on_committed_tree() {
        // The committed schema is kept in generated form, so the `--check` gate
        // must be green; this exercises `run(check=true)` (the read + compare).
        run(true).expect("gen-schema --check should pass on the committed tree");
    }
}
