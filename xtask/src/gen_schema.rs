//! `xtask gen-schema` - assemble the stable config and policy-test schemas plus
//! the experimental selected-rule trace schema from their Rust models.
//!
//! Config generation applies per-kind restrictions from `KindMeta`. Every
//! canonical kind has a generated option schema. Fidelity is checked semantically
//! because schemars emits cosmetic keywords (for example `format: uint`) that do
//! not change validation behavior. A record/schema parity gate fails generation
//! if `KindMeta` and any generated branch drift.

use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde_json::Value;

fn root_schema_path() -> Result<PathBuf> {
    Ok(crate::workspace_root()?.join("schemas/v1/config.json"))
}

fn in_crate_schema_path() -> Result<PathBuf> {
    Ok(crate::workspace_root()?.join("crates/alint-dsl/schemas/v1/config.json"))
}

fn policy_test_schema_path() -> Result<PathBuf> {
    Ok(crate::workspace_root()?.join("schemas/v1/policy-test.json"))
}

fn policy_test_report_schema_path() -> Result<PathBuf> {
    Ok(crate::workspace_root()?.join("schemas/v1/policy-test-report.json"))
}
fn rule_trace_schema_path() -> Result<PathBuf> {
    Ok(crate::workspace_root()?.join("schemas/experimental/rule-trace.json"))
}

fn in_crate_rule_trace_schema_path() -> Result<PathBuf> {
    Ok(crate::workspace_root()?.join("crates/alint/schemas/experimental/rule-trace.json"))
}

/// Build the generated schema in memory: the committed descriptive base with
/// every canonical rule branch replaced by its schemars-derived equivalent.
pub fn build_generated_schema() -> Result<Value> {
    let base_src = std::fs::read_to_string(root_schema_path()?)
        .context("read base schema schemas/v1/config.json")?;
    let mut schema: Value = serde_json::from_str(&base_src).context("parse base schema as JSON")?;

    let defs = schema
        .get_mut("$defs")
        .and_then(Value::as_object_mut)
        .context("base schema has no `$defs` object")?;

    add_sensitive_common_field(defs)?;

    let mut option_schemas = alint_rules::rule_option_schemas();
    let generated_defs: BTreeSet<String> = option_schemas
        .iter()
        .map(|(name, _)| (*name).to_string())
        .collect();
    let registry = alint_rules::builtin_registry();
    let expected_defs: BTreeSet<String> = registry
        .canonical_kinds()
        .map(|kind| format!("rule_{kind}"))
        .collect();
    anyhow::ensure!(
        generated_defs == expected_defs,
        "generated option-schema coverage differs from the canonical registry.\n  missing: \
         {:?}\n  extra: {:?}",
        expected_defs
            .difference(&generated_defs)
            .collect::<Vec<_>>(),
        generated_defs
            .difference(&expected_defs)
            .collect::<Vec<_>>()
    );

    merge_fresh_option_defs(defs, &mut option_schemas)?;

    for (def_name, options) in option_schemas {
        let base = defs
            .get(def_name)
            .with_context(|| format!("rule def `{def_name}` is not present in `$defs`"))?
            .clone();
        defs.insert(def_name.to_string(), compose_branch(&base, &options)?);
    }

    apply_kind_meta_restrictions(defs)?;

    normalize_descriptions(&mut schema);
    prune_unreferenced_defs(&mut schema)?;
    Ok(schema)
}

fn add_sensitive_common_field(defs: &mut serde_json::Map<String, Value>) -> Result<()> {
    let properties = defs
        .get_mut("rule_common")
        .and_then(|value| value.get_mut("properties"))
        .and_then(Value::as_object_mut)
        .context("schema has no `$defs/rule_common/properties` object")?;
    properties.insert(
        "sensitive".to_string(),
        serde_json::json!({
            "type": "boolean",
            "default": false,
            "description": "Redact this rule's messages, baseline evidence, proposed edits, and fix detail before any public report is returned. Use for rules whose findings may contain credentials or other secrets."
        }),
    );
    Ok(())
}

/// Replace nested definitions owned by the current Rust option schemas while
/// rejecting two current types that emit the same name with different shapes.
/// The replacement is required because the committed schema is also the next
/// generation's base, so its nested definitions are expected to be stale after
/// a Rust type, constraint, or rustdoc change.
fn merge_fresh_option_defs(
    defs: &mut serde_json::Map<String, Value>,
    option_schemas: &mut [(&str, Value)],
) -> Result<()> {
    let mut fresh = serde_json::Map::new();
    for (def_name, options) in option_schemas {
        normalize_descriptions(options);
        if let Some(extra_defs) = options.get("$defs").and_then(Value::as_object) {
            for (name, def) in extra_defs {
                if fresh.get(name).is_some_and(|existing| existing != def) {
                    bail!(
                        "schemars `$defs` collision on `{name}` while composing `{def_name}`: \
                         two distinct types share that name. Rename one with \
                         `#[schemars(rename = \"...\")]` on its Rust type."
                    );
                }
                fresh.insert(name.clone(), def.clone());
            }
        }
    }
    defs.extend(fresh);
    Ok(())
}

/// Remove stale definitions left behind by the self-hosted generator. The
/// committed schema is also the next generation's descriptive base, so a Rust
/// type that is renamed or removed would otherwise leave its old `$defs` entry
/// in every future schema even though no branch references it.
fn prune_unreferenced_defs(schema: &mut Value) -> Result<()> {
    fn collect_refs(value: &Value, out: &mut std::collections::BTreeSet<String>) {
        match value {
            Value::Object(map) => {
                if let Some(name) = map
                    .get("$ref")
                    .and_then(Value::as_str)
                    .and_then(|rf| rf.strip_prefix("#/$defs/"))
                {
                    out.insert(name.to_string());
                }
                for (key, child) in map {
                    if key != "$defs" {
                        collect_refs(child, out);
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|item| collect_refs(item, out)),
            _ => {}
        }
    }

    let all_defs = schema
        .get("$defs")
        .and_then(Value::as_object)
        .context("schema has no `$defs` object")?
        .clone();
    let mut reachable = std::collections::BTreeSet::new();
    collect_refs(schema, &mut reachable);
    let mut pending: Vec<String> = reachable.iter().cloned().collect();
    while let Some(name) = pending.pop() {
        let def = all_defs
            .get(&name)
            .with_context(|| format!("schema references missing `$defs/{name}`"))?;
        let mut nested = std::collections::BTreeSet::new();
        collect_refs(def, &mut nested);
        for dependency in nested {
            if reachable.insert(dependency.clone()) {
                pending.push(dependency);
            }
        }
    }
    schema
        .get_mut("$defs")
        .and_then(Value::as_object_mut)
        .expect("checked above")
        .retain(|name, _| reachable.contains(name));
    Ok(())
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
            // `default`, so restore both from the committed descriptive base
            // when the derived schema omits them.
            if let Some(obj) = prop.as_object_mut() {
                let base_prop = base_props.and_then(|p| p.get(key));
                for carried in ["description", "default"] {
                    if !obj.contains_key(carried)
                        && let Some(v) = base_prop.and_then(|p| p.get(carried))
                    {
                        obj.insert(carried.to_string(), v.clone());
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

/// Apply the common-field surface that each canonical kind actually accepts.
/// `rule_common` stays the shared structural envelope, while these branch-local
/// constraints make the schema reject the same ignored fields as the loader.
fn apply_kind_meta_restrictions(defs: &mut serde_json::Map<String, Value>) -> Result<()> {
    factor_fix_schema(defs)?;

    let common_properties = defs
        .get("rule_common")
        .and_then(|value| value.get("properties"))
        .and_then(Value::as_object)
        .context("schema has no `$defs/rule_common/properties` object")?;
    let paths_schema = common_properties
        .get("paths")
        .context("rule_common has no `paths` property")?
        .clone();
    let scope_filter_schema = common_properties
        .get("scope_filter")
        .context("rule_common has no `scope_filter` property")?
        .clone();
    let expect_matches_schema = common_properties
        .get("expect_matches")
        .context("rule_common has no `expect_matches` property")?
        .clone();
    let full_scope_filter = defs
        .get("scope_filter")
        .context("schema has no `$defs/scope_filter`")?
        .clone();
    let full_fix = defs
        .get("fix")
        .context("schema has no `$defs/fix`")?
        .clone();

    let registry = alint_rules::builtin_registry();
    for (kind, meta) in registry.kind_metadata() {
        let def_name = format!("rule_{kind}");
        let branch = defs
            .get_mut(&def_name)
            .with_context(|| format!("registry kind `{kind}` has no `$defs/{def_name}` branch"))?;
        let branch_obj = branch
            .as_object_mut()
            .with_context(|| format!("`$defs/{def_name}` is not an object"))?;
        let properties = branch_obj
            .get_mut("properties")
            .and_then(Value::as_object_mut)
            .with_context(|| format!("`$defs/{def_name}` has no properties object"))?;
        let mut forbidden = Vec::new();

        match meta.paths {
            alint_core::PathsSupport::Required | alint_core::PathsSupport::Optional => {
                properties.insert("paths".to_string(), paths_schema.clone());
            }
            alint_core::PathsSupport::Forbidden => {
                properties.remove("paths");
                forbidden.push("paths");
            }
        }

        match &meta.scope_filter {
            alint_core::ScopeFilterSupport::Full => {
                properties.insert("scope_filter".to_string(), scope_filter_schema.clone());
            }
            alint_core::ScopeFilterSupport::AncestorOnly => {
                properties.insert(
                    "scope_filter".to_string(),
                    ancestor_only_scope_filter(&full_scope_filter)?,
                );
            }
            alint_core::ScopeFilterSupport::Rejected { .. } => {
                properties.remove("scope_filter");
                forbidden.push("scope_filter");
            }
        }

        if meta.fix_ops.is_empty() {
            properties.remove("fix");
            forbidden.push("fix");
        } else {
            properties.insert("fix".to_string(), fix_schema_for(&full_fix, &meta.fix_ops)?);
        }

        if meta.expect_matches {
            properties.insert("expect_matches".to_string(), expect_matches_schema.clone());
        } else {
            properties.remove("expect_matches");
            forbidden.push("expect_matches");
        }

        set_required(
            branch_obj,
            "paths",
            meta.paths == alint_core::PathsSupport::Required,
        );
        branch_obj.remove("not");
        if !forbidden.is_empty() {
            branch_obj.insert(
                "not".to_string(),
                serde_json::json!({
                    "anyOf": forbidden
                        .into_iter()
                        .map(|field| serde_json::json!({ "required": [field] }))
                        .collect::<Vec<_>>()
                }),
            );
        }
    }

    apply_nested_rule_restrictions(defs)?;
    assert_record_schema_parity(defs, &registry)
}

/// Reuse the canonical kind dispatch for `require:` rules, but expose only the
/// nested common-field envelope. This keeps recursive nested rules on the same
/// generated option types and `KindMeta` restrictions as top-level rules.
fn apply_nested_rule_restrictions(defs: &mut serde_json::Map<String, Value>) -> Result<()> {
    let mut common = defs
        .get("rule_common")
        .context("schema has no `$defs/rule_common` common-field base")?
        .clone();
    let common_obj = common
        .as_object_mut()
        .context("`$defs/rule_common` is not an object")?;
    let properties = common_obj
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .context("`$defs/rule_common` has no properties object")?;
    properties.retain(|name, _| {
        matches!(
            name.as_str(),
            "expect_matches"
                | "kind"
                | "message"
                | "paths"
                | "policy_url"
                | "scope_filter"
                | "sensitive"
                | "when"
        )
    });
    let expect_matches = properties
        .get_mut("expect_matches")
        .and_then(Value::as_object_mut)
        .context("`$defs/rule_common/expect_matches` is not an object")?;
    expect_matches.insert(
        "description".to_string(),
        Value::from(
            "Require this nested rule's effective file scope to match at least one file for each \
             active parent iteration. Checked after the nested `when:` gate. Unsupported for \
             existence and selector rules.",
        ),
    );
    common_obj.insert(
        "description".to_string(),
        Value::from(
            "Common fields accepted by a nested rule. `id` and `level` are synthesized from the \
             parent, and remediation or per-rule Git toggles are not supported.",
        ),
    );
    common_obj.insert("required".to_string(), serde_json::json!(["kind"]));

    defs.insert("nested_rule_common".to_string(), common);
    defs.insert(
        "nested_rule".to_string(),
        serde_json::json!({
            "description": "A nested rule inside `require:`. Its kind-specific options and common-field restrictions match the canonical rule catalog; `id` and `level` are synthesized from the parent.",
            "allOf": [
                { "$ref": "#/$defs/nested_rule_common" },
                { "$ref": "#/$defs/rule_kind_dispatch" },
                {
                    "not": {
                        "anyOf": [
                            { "required": ["id"] },
                            { "required": ["level"] },
                            { "required": ["fix"] },
                            { "required": ["git_tracked_only"] },
                            { "required": ["respect_gitignore"] }
                        ]
                    }
                }
            ],
            "unevaluatedProperties": false
        }),
    );
    Ok(())
}

fn set_required(branch: &mut serde_json::Map<String, Value>, field: &str, required: bool) {
    let mut names = branch
        .get("required")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    names.retain(|name| name.as_str() != Some(field));
    if required {
        names.insert(0, Value::from(field));
    }
    if names.is_empty() {
        branch.remove("required");
    } else {
        branch.insert("required".to_string(), Value::Array(names));
    }
}

fn ancestor_only_scope_filter(full: &Value) -> Result<Value> {
    let mut restricted = full.clone();
    let obj = restricted
        .as_object_mut()
        .context("scope_filter schema is not an object")?;
    let properties = obj
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .context("scope_filter schema has no properties object")?;
    properties.retain(|name, _| name == "has_ancestor");
    anyhow::ensure!(
        properties.contains_key("has_ancestor"),
        "scope_filter schema has no `has_ancestor` property"
    );
    obj.insert(
        "anyOf".to_string(),
        serde_json::json!([{ "required": ["has_ancestor"] }]),
    );
    obj.insert(
        "description".to_string(),
        Value::from("This rule kind accepts only `scope_filter.has_ancestor`."),
    );
    Ok(restricted)
}

/// Hoist the 26 fix-operation branches once, then reference them from both the
/// global `fix` union and per-kind subsets. Without this factoring, each kind
/// would inline its accepted operations and inflate the published schema by
/// hundreds of kilobytes.
fn factor_fix_schema(defs: &mut serde_json::Map<String, Value>) -> Result<()> {
    let branches = defs
        .get("fix")
        .and_then(|schema| schema.get("oneOf"))
        .and_then(Value::as_array)
        .context("fix schema has no `oneOf` array")?
        .clone();
    if branches
        .iter()
        .all(|branch| branch.get("$ref").and_then(Value::as_str).is_some())
    {
        for branch in &branches {
            let op = fix_op_from_branch(branch).context("invalid factored fix-op reference")?;
            anyhow::ensure!(
                defs.contains_key(&format!("fix_op_{op}")),
                "factored fix schema references missing `fix_op_{op}`"
            );
        }
        return Ok(());
    }

    let mut references = Vec::with_capacity(branches.len());
    let mut seen = BTreeSet::new();
    for branch in branches {
        let op = fix_op_from_branch(&branch).context("fix branch has no single required op")?;
        anyhow::ensure!(
            seen.insert(op.to_string()),
            "duplicate fix schema op `{op}`"
        );
        let name = format!("fix_op_{op}");
        if let Some(existing) = defs.get(&name) {
            anyhow::ensure!(
                existing == &branch,
                "schema definition `{name}` collides with the generated fix-op branch"
            );
        } else {
            defs.insert(name.clone(), branch);
        }
        references.push(serde_json::json!({ "$ref": format!("#/$defs/{name}") }));
    }
    defs.get_mut("fix")
        .and_then(|schema| schema.get_mut("oneOf"))
        .expect("checked above")
        .as_array_mut()
        .expect("checked above")
        .clone_from(&references);
    Ok(())
}

fn fix_op_from_branch(branch: &Value) -> Option<&str> {
    branch
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix("#/$defs/fix_op_"))
        .or_else(|| {
            branch
                .get("required")
                .and_then(Value::as_array)
                .and_then(|required| required.first())
                .and_then(Value::as_str)
        })
}

fn fix_schema_for(full: &Value, accepted: &[std::borrow::Cow<'static, str>]) -> Result<Value> {
    let accepted: BTreeSet<&str> = accepted.iter().map(AsRef::as_ref).collect();
    let mut restricted = full.clone();
    let branches = restricted
        .get_mut("oneOf")
        .and_then(Value::as_array_mut)
        .context("fix schema has no `oneOf` array")?;
    branches.retain(|branch| fix_op_from_branch(branch).is_some_and(|op| accepted.contains(op)));
    let found: BTreeSet<&str> = branches.iter().filter_map(fix_op_from_branch).collect();
    anyhow::ensure!(
        found == accepted,
        "KindMeta references fix ops absent from the schema: {:?}",
        accepted.difference(&found).collect::<Vec<_>>()
    );
    Ok(restricted)
}

/// Fail generation if any presence fact duplicated between `KindMeta` and the
/// JSON Schema disagrees. This is deliberately semantic: it inspects accepted
/// fields and discriminators rather than pinning schemars' incidental layout.
fn assert_record_schema_parity(
    defs: &serde_json::Map<String, Value>,
    registry: &alint_core::RuleRegistry,
) -> Result<()> {
    let expected_dispatch: BTreeSet<String> = registry
        .canonical_kinds()
        .map(|kind| format!("rule_{kind}"))
        .collect();
    let actual_dispatch: BTreeSet<String> = defs
        .get("rule_kind_dispatch")
        .and_then(|value| value.get("oneOf"))
        .and_then(Value::as_array)
        .context("rule_kind_dispatch has no `oneOf` array")?
        .iter()
        .filter_map(|branch| branch.get("$ref").and_then(Value::as_str))
        .filter_map(|reference| reference.strip_prefix("#/$defs/"))
        .map(str::to_string)
        .collect();
    anyhow::ensure!(
        actual_dispatch == expected_dispatch,
        "rule_kind_dispatch differs from the canonical registry.\n  missing: {:?}\n  extra: \
         {:?}",
        expected_dispatch
            .difference(&actual_dispatch)
            .collect::<Vec<_>>(),
        actual_dispatch
            .difference(&expected_dispatch)
            .collect::<Vec<_>>()
    );

    for (kind, meta) in registry.kind_metadata() {
        let def_name = format!("rule_{kind}");
        let branch = defs
            .get(&def_name)
            .with_context(|| format!("registry kind `{kind}` has no schema branch"))?;
        let properties = branch
            .get("properties")
            .and_then(Value::as_object)
            .with_context(|| format!("`{def_name}` has no properties object"))?;

        let mut expected_names = BTreeSet::from([kind.to_string()]);
        expected_names.extend(
            registry
                .aliases()
                .filter(|(_, canonical)| *canonical == kind)
                .map(|(alias, _)| alias.to_string()),
        );
        let actual_names = discriminator_values(
            properties
                .get("kind")
                .with_context(|| format!("`{def_name}` has no kind discriminator"))?,
        );
        anyhow::ensure!(
            actual_names == expected_names,
            "`{kind}` schema discriminator differs from registry names: expected \
             {expected_names:?}, got {actual_names:?}"
        );

        assert_record_field_parity(branch, properties, meta, kind)?;

        for selector in meta.selectors.iter() {
            anyhow::ensure!(
                schema_path_exists(defs, branch, selector.name.as_ref()),
                "`{kind}` KindMeta selector `{}` is absent from its schema",
                selector.name
            );
        }
    }
    Ok(())
}

fn assert_record_field_parity(
    branch: &Value,
    properties: &serde_json::Map<String, Value>,
    meta: &alint_core::KindMeta,
    kind: &str,
) -> Result<()> {
    use alint_core::{GitTrackedSupport, PathsSupport, ScopeFilterSupport, SinceSupport};

    assert_field_support(
        branch,
        properties,
        "paths",
        meta.paths != PathsSupport::Forbidden,
        kind,
    )?;
    anyhow::ensure!(
        is_required(branch, "paths") == (meta.paths == PathsSupport::Required),
        "`{kind}` paths requiredness differs between KindMeta and schema"
    );
    assert_field_support(
        branch,
        properties,
        "scope_filter",
        !matches!(meta.scope_filter, ScopeFilterSupport::Rejected { .. }),
        kind,
    )?;
    if meta.scope_filter == ScopeFilterSupport::AncestorOnly {
        let names: BTreeSet<&str> = properties["scope_filter"]
            .get("properties")
            .and_then(Value::as_object)
            .context("ancestor-only scope_filter has no properties")?
            .keys()
            .map(String::as_str)
            .collect();
        anyhow::ensure!(
            names == BTreeSet::from(["has_ancestor"]),
            "`{kind}` ancestor-only scope_filter schema exposes {names:?}"
        );
    }
    assert_field_support(branch, properties, "fix", !meta.fix_ops.is_empty(), kind)?;
    if !meta.fix_ops.is_empty() {
        let schema_ops = fix_ops_in_schema(&properties["fix"]);
        let record_ops: BTreeSet<&str> = meta.fix_ops.iter().map(AsRef::as_ref).collect();
        anyhow::ensure!(
            schema_ops == record_ops,
            "`{kind}` fix ops differ between KindMeta ({record_ops:?}) and schema \
             ({schema_ops:?})"
        );
    }
    assert_field_support(
        branch,
        properties,
        "expect_matches",
        meta.expect_matches,
        kind,
    )?;

    for (field, supported) in [
        (
            "git_tracked_only",
            meta.git_tracked_only != GitTrackedSupport::None,
        ),
        ("respect_gitignore", meta.respect_gitignore),
        ("root_only", meta.root_only),
        ("since", meta.since != SinceSupport::None),
    ] {
        anyhow::ensure!(
            properties.contains_key(field) == supported,
            "`{kind}` `{field}` presence differs between KindMeta and schema"
        );
    }
    anyhow::ensure!(
        is_required(branch, "since") == (meta.since == SinceSupport::Required),
        "`{kind}` since requiredness differs between KindMeta and schema"
    );
    Ok(())
}

fn discriminator_values(schema: &Value) -> BTreeSet<String> {
    if let Some(value) = schema.get("const").and_then(Value::as_str) {
        return BTreeSet::from([value.to_string()]);
    }
    schema
        .get("enum")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn assert_field_support(
    branch: &Value,
    properties: &serde_json::Map<String, Value>,
    field: &str,
    supported: bool,
    kind: &str,
) -> Result<()> {
    anyhow::ensure!(
        properties.contains_key(field) == supported,
        "`{kind}` `{field}` property presence differs between KindMeta and schema"
    );
    anyhow::ensure!(
        branch_forbids_field(branch, field) != supported,
        "`{kind}` `{field}` rejection differs between KindMeta and schema"
    );
    Ok(())
}

fn branch_forbids_field(branch: &Value, field: &str) -> bool {
    branch
        .get("not")
        .and_then(|value| value.get("anyOf"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|clause| {
            clause
                .get("required")
                .and_then(Value::as_array)
                .is_some_and(|required| required.iter().any(|name| name == field))
        })
}

fn is_required(branch: &Value, field: &str) -> bool {
    branch
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|required| required.iter().any(|name| name == field))
}

fn fix_ops_in_schema(schema: &Value) -> BTreeSet<&str> {
    schema
        .get("oneOf")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(fix_op_from_branch)
        .collect()
}

fn schema_path_exists(defs: &serde_json::Map<String, Value>, schema: &Value, path: &str) -> bool {
    let segments: Vec<&str> = path.split('.').collect();
    schema_has_segments(defs, schema, &segments, 0)
}

fn schema_has_segments(
    defs: &serde_json::Map<String, Value>,
    schema: &Value,
    segments: &[&str],
    depth: usize,
) -> bool {
    if depth > 32 {
        return false;
    }
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str)
        && let Some(name) = reference.strip_prefix("#/$defs/")
    {
        return defs
            .get(name)
            .is_some_and(|target| schema_has_segments(defs, target, segments, depth + 1));
    }
    for combinator in ["oneOf", "anyOf", "allOf"] {
        if schema
            .get(combinator)
            .and_then(Value::as_array)
            .is_some_and(|branches| {
                branches
                    .iter()
                    .any(|branch| schema_has_segments(defs, branch, segments, depth + 1))
            })
        {
            return true;
        }
    }
    let Some((segment, rest)) = segments.split_first() else {
        return true;
    };
    let (name, array) = segment
        .strip_suffix("[]")
        .map_or((*segment, false), |name| (name, true));
    let Some(property) = schema
        .get("properties")
        .and_then(Value::as_object)
        .and_then(|properties| properties.get(name))
    else {
        return false;
    };
    if array {
        array_items_have_segments(defs, property, rest, depth + 1)
    } else {
        schema_has_segments(defs, property, rest, depth + 1)
    }
}

fn array_items_have_segments(
    defs: &serde_json::Map<String, Value>,
    schema: &Value,
    segments: &[&str],
    depth: usize,
) -> bool {
    if depth > 32 {
        return false;
    }
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str)
        && let Some(name) = reference.strip_prefix("#/$defs/")
    {
        return defs
            .get(name)
            .is_some_and(|target| array_items_have_segments(defs, target, segments, depth + 1));
    }
    for combinator in ["oneOf", "anyOf", "allOf"] {
        if schema
            .get(combinator)
            .and_then(Value::as_array)
            .is_some_and(|branches| {
                branches
                    .iter()
                    .any(|branch| array_items_have_segments(defs, branch, segments, depth + 1))
            })
        {
            return true;
        }
    }
    schema
        .get("items")
        .is_some_and(|items| schema_has_segments(defs, items, segments, depth + 1))
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
    let policy_test_rendered =
        alint_testkit::policy_test_schema_json().context("render policy-test schema")?;
    let policy_test_report_rendered = alint_testkit::policy_test_report_schema_json()
        .context("render policy-test report schema")?;
    let rule_trace_rendered =
        render(&alint_core::rule_trace_schema()).context("render rule-trace schema")?;

    if check {
        let root_path = root_schema_path()?;
        let in_crate_path = in_crate_schema_path()?;
        let policy_test_path = policy_test_schema_path()?;
        let policy_test_report_path = policy_test_report_schema_path()?;
        let rule_trace_path = rule_trace_schema_path()?;
        let in_crate_rule_trace_path = in_crate_rule_trace_schema_path()?;
        let root = std::fs::read_to_string(&root_path)
            .with_context(|| format!("read {}", root_path.display()))?;
        let in_crate = std::fs::read_to_string(&in_crate_path)
            .with_context(|| format!("read {}", in_crate_path.display()))?;
        let policy_test = std::fs::read_to_string(&policy_test_path)
            .with_context(|| format!("read {}", policy_test_path.display()))?;
        let policy_test_report = std::fs::read_to_string(&policy_test_report_path)
            .with_context(|| format!("read {}", policy_test_report_path.display()))?;
        let rule_trace = std::fs::read_to_string(&rule_trace_path)
            .with_context(|| format!("read {}", rule_trace_path.display()))?;
        let in_crate_rule_trace = std::fs::read_to_string(&in_crate_rule_trace_path)
            .with_context(|| format!("read {}", in_crate_rule_trace_path.display()))?;
        let stale: Vec<&str> = [
            (root != rendered).then_some("schemas/v1/config.json"),
            (in_crate != rendered).then_some("crates/alint-dsl/schemas/v1/config.json"),
            (policy_test != policy_test_rendered).then_some("schemas/v1/policy-test.json"),
            (policy_test_report != policy_test_report_rendered)
                .then_some("schemas/v1/policy-test-report.json"),
            (rule_trace != rule_trace_rendered).then_some("schemas/experimental/rule-trace.json"),
            (in_crate_rule_trace != rule_trace_rendered)
                .then_some("crates/alint/schemas/experimental/rule-trace.json"),
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
        println!("schemas are up to date");
        return Ok(());
    }

    if let Some(parent) = rule_trace_schema_path()?.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = in_crate_rule_trace_schema_path()?.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(root_schema_path()?, &rendered)?;
    std::fs::write(in_crate_schema_path()?, &rendered)?;
    std::fs::write(policy_test_schema_path()?, &policy_test_rendered)?;
    std::fs::write(
        policy_test_report_schema_path()?,
        &policy_test_report_rendered,
    )?;
    std::fs::write(rule_trace_schema_path()?, &rule_trace_rendered)?;
    std::fs::write(in_crate_rule_trace_schema_path()?, &rule_trace_rendered)?;
    println!(
        "wrote config schema copies, schemas/v1 policy-test schemas, and \
         experimental rule-trace schema copies"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_to_json(rel_path: &str) -> Value {
        let path = crate::workspace_root().unwrap().join(rel_path);
        let yaml = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let value: ayaml::Value = ayaml::from_str(&yaml).unwrap();
        serde_json::to_value(value).unwrap()
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

    #[test]
    fn generated_schema_matches_kind_common_field_capabilities() {
        let validator = jsonschema::validator_for(&build_generated_schema().unwrap()).unwrap();
        let config = |rule| serde_json::json!({"version": 1, "rules": [rule]});

        assert!(validator.is_valid(&config(serde_json::json!({
            "id": "submodules", "kind": "no_submodules", "level": "error",
            "fix": {"file_remove": {}}
        }))));
        assert!(!validator.is_valid(&config(serde_json::json!({
            "id": "submodules", "kind": "no_submodules", "level": "error",
            "paths": ".gitmodules"
        }))));

        assert!(validator.is_valid(&config(serde_json::json!({
            "id": "exists", "kind": "file_exists", "level": "error",
            "paths": "README.md", "fix": {"file_create": {"content": ""}}
        }))));
        assert!(!validator.is_valid(&config(serde_json::json!({
            "id": "exists", "kind": "file_exists", "level": "error",
            "paths": "README.md", "scope_filter": {"exclude_query": "size > 0"}
        }))));
        assert!(!validator.is_valid(&config(serde_json::json!({
            "id": "exists", "kind": "file_exists", "level": "error",
            "paths": "README.md", "fix": {"replace": {"replacement": "x"}}
        }))));

        assert!(validator.is_valid(&config(serde_json::json!({
            "id": "case", "kind": "filename_case", "level": "error",
            "paths": "**/*.rs", "case": "snake",
            "scope_filter": {"changed_since": "HEAD~1"},
            "expect_matches": true, "fix": {"file_rename": {}}
        }))));
        assert!(!validator.is_valid(&config(serde_json::json!({
            "id": "case", "kind": "filename_case", "level": "error", "case": "snake"
        }))));

        assert!(validator.is_valid(&config(serde_json::json!({
            "id": "absent", "kind": "dir_absent", "level": "error",
            "paths": "target", "scope_filter": {"has_ancestor": "Cargo.toml"}
        }))));
        assert!(!validator.is_valid(&config(serde_json::json!({
            "id": "absent", "kind": "dir_absent", "level": "error",
            "paths": "target", "scope_filter": {"changed_since": "HEAD~1"}
        }))));

        assert!(validator.is_valid(&config(serde_json::json!({
            "id": "lines", "kind": "for_each_match", "level": "error",
            "paths": "CHANGELOG.md", "select": "^##",
            "require": {"matches": ["release"]}, "expect_matches": true
        }))));
        assert!(!validator.is_valid(&config(serde_json::json!({
            "id": "lines", "kind": "for_each_match", "level": "error",
            "paths": "CHANGELOG.md", "select": "^##",
            "require": {"matches": ["release"]}, "fix": {"replace": {"replacement": "x"}}
        }))));
    }

    #[test]
    fn generated_schema_enforces_nested_kind_contracts() {
        let validator = jsonschema::validator_for(&build_generated_schema().unwrap()).unwrap();
        let config = |nested| {
            serde_json::json!({
                "version": 1,
                "rules": [{
                    "id": "packages",
                    "kind": "for_each_dir",
                    "level": "error",
                    "select": "packages/*",
                    "require": [nested]
                }]
            })
        };

        assert!(validator.is_valid(&config(serde_json::json!({
            "kind": "file_exists", "paths": "{path}/README.md"
        }))));
        assert!(!validator.is_valid(&config(serde_json::json!({
            "kind": "dir_contains", "paths": "{path}",
            "select": "{path}", "require": "README.md"
        }))));
        assert!(!validator.is_valid(&config(serde_json::json!({
            "kind": "file_exists", "paths": "{path}/README.md",
            "fix": {"file_create": {"content": ""}}
        }))));
        for (field, value) in [
            ("id", serde_json::json!("ignored")),
            ("level", serde_json::json!("warning")),
            ("bogus", serde_json::json!(true)),
        ] {
            let mut nested = serde_json::json!({
                "kind": "file_exists", "paths": "{path}/README.md"
            });
            nested[field] = value;
            assert!(
                !validator.is_valid(&config(nested)),
                "nested rule accepted unsupported field `{field}`"
            );
        }
        assert!(!validator.is_valid(&config(serde_json::json!({
            "kind": "not_a_kind"
        }))));
    }

    #[test]
    fn rule_option_schemas_list_is_complete_and_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for (name, _) in alint_rules::rule_option_schemas() {
            assert!(seen.insert(name), "duplicate generated def name: {name}");
        }
        // Exercises the present-in-`$defs` check and the enum `$defs` collision
        // guard inside `build_generated_schema`.
        build_generated_schema().unwrap();
    }

    #[test]
    fn generated_schema_rejects_invalid_rule_options() {
        let v = jsonschema::validator_for(&build_generated_schema().unwrap()).unwrap();
        // Stray unknown option (caught by the shared
        // `unevaluatedProperties: false`).
        assert!(!v.is_valid(&serde_json::json!(
            {"version":1,"rules":[{"id":"x","kind":"file_header","level":"error","paths":"a","pattern":"p","bogus":1}]}
        )));
        // Invalid enum value: the constraint must survive the schemars
        // `oneOf`/`$defs` round-trip.
        assert!(!v.is_valid(&serde_json::json!(
            {"version":1,"rules":[{"id":"x","kind":"pair_hash","level":"error","source":"a","target":"b","algorithm":"md5"}]}
        )));
        // Valid baseline accepted.
        assert!(v.is_valid(&serde_json::json!(
            {"version":1,"rules":[{"id":"x","kind":"pair_hash","level":"error","source":"a","target":"b","algorithm":"sha256"}]}
        )));
    }

    #[test]
    fn file_graph_choice_fields_reject_null() {
        let schema = build_generated_schema().unwrap();
        for (definition, instance) in [
            (
                "FileGraphEdgesSpec",
                serde_json::json!({"from_content": null}),
            ),
            (
                "FileGraphEdgesSpec",
                serde_json::json!({"derive_target": null}),
            ),
            (
                "FileGraphRequireMap",
                serde_json::json!({"no_orphans": null}),
            ),
            ("FileGraphRequireMap", serde_json::json!({"fresh": null})),
        ] {
            let root = serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "$ref": format!("#/$defs/{definition}"),
                "$defs": schema["$defs"].clone(),
            });
            let validator = jsonschema::validator_for(&root).unwrap();
            assert!(
                !validator.is_valid(&instance),
                "{definition} accepted a null choice: {instance}"
            );
        }
    }

    #[test]
    fn unreferenced_defs_are_pruned_transitively() {
        let mut schema = serde_json::json!({
            "$ref": "#/$defs/rule",
            "$defs": {
                "rule": { "$ref": "#/$defs/used" },
                "used": { "type": "string" },
                "stale": { "$ref": "#/$defs/stale_child" },
                "stale_child": { "type": "number" }
            }
        });
        prune_unreferenced_defs(&mut schema).unwrap();
        let defs = schema["$defs"].as_object().unwrap();
        assert_eq!(defs.len(), 2);
        assert!(defs.contains_key("rule"));
        assert!(defs.contains_key("used"));
    }

    #[test]
    fn fresh_nested_defs_replace_stale_base_but_collisions_fail() {
        let mut defs = serde_json::Map::from_iter([(
            "Nested".to_string(),
            serde_json::json!({"const": "stale"}),
        )]);
        let mut fresh = [(
            "rule_example",
            serde_json::json!({"$defs": {"Nested": {"const": "current"}}}),
        )];
        merge_fresh_option_defs(&mut defs, &mut fresh).unwrap();
        assert_eq!(defs["Nested"], serde_json::json!({"const": "current"}));

        let mut conflicting = [
            (
                "rule_one",
                serde_json::json!({"$defs": {"Shared": {"type": "string"}}}),
            ),
            (
                "rule_two",
                serde_json::json!({"$defs": {"Shared": {"type": "integer"}}}),
            ),
        ];
        let err = merge_fresh_option_defs(&mut serde_json::Map::new(), &mut conflicting)
            .expect_err("distinct current types must not share one definition name");
        assert!(err.to_string().contains("collision on `Shared`"), "{err}");
    }

    #[test]
    fn gen_schema_check_passes_on_committed_tree() {
        // The committed schema is kept in generated form, so the `--check` gate
        // must be green; this exercises `run(check=true)` (the read + compare).
        run(true).expect("gen-schema --check should pass on the committed tree");
    }
}
