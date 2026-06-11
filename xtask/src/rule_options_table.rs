//! Schema-derived per-rule options tables for `xtask docs-export`.
//!
//! The per-rule "## Options" table is generated from the type-derived
//! JSON Schema (`schemas/v1/config.json`), which itself comes from the
//! Rust `Options` structs via `xtask gen-schema` (ADR-0001). Names,
//! types, defaults and descriptions therefore flow Rust type -> schema
//! -> docs with no hand-maintained intermediate, so the published
//! reference can't drift from the engine. `gen-schema --check` (run in
//! CI + preflight) fails the build if the committed schema lags the
//! structs.
//!
//! Split out of `docs_export` so that module stays under the
//! `rust-file-max-lines` dogfood limit.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

/// Parse the committed config schema at `schema_path`
/// (`schemas/v1/config.json`).
pub(crate) fn load_config_schema(schema_path: &Path) -> Result<serde_json::Value> {
    let raw = fs::read_to_string(schema_path)
        .with_context(|| format!("reading {}", schema_path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parsing {} as JSON", schema_path.display()))
}

/// Map every rule-kind spelling (canonical *and* alias) to its
/// `$defs/rule_<canonical>` branch. Aliased kinds share a branch
/// whose `kind` is an `enum` of all spellings; canonical-only kinds
/// have a `kind` `const`. Owned clones so the borrow on `schema`
/// doesn't tie up the page-generation loop.
pub(crate) fn build_kind_branch_index(
    schema: &serde_json::Value,
) -> HashMap<String, serde_json::Value> {
    let mut index = HashMap::new();
    let Some(defs) = schema.get("$defs").and_then(|d| d.as_object()) else {
        return index;
    };
    for (name, branch) in defs {
        if !name.starts_with("rule_") {
            continue;
        }
        let Some(kind_prop) = branch.get("properties").and_then(|p| p.get("kind")) else {
            continue;
        };
        if let Some(c) = kind_prop.get("const").and_then(serde_json::Value::as_str) {
            index.insert(c.to_string(), branch.clone());
        } else if let Some(arr) = kind_prop.get("enum").and_then(|e| e.as_array()) {
            for spelling in arr.iter().filter_map(serde_json::Value::as_str) {
                index.insert(spelling.to_string(), branch.clone());
            }
        }
    }
    index
}

/// Render the `## Options` section for one rule branch. Omits the
/// universal `kind`/`paths` fields from the table (`paths` is noted
/// in the trailing common-fields line when the kind accepts it).
pub(crate) fn options_section(branch: &serde_json::Value, schema: &serde_json::Value) -> String {
    use std::collections::HashSet;

    let props = branch.get("properties").and_then(|p| p.as_object());
    let has_paths = props.is_some_and(|p| p.contains_key("paths"));
    let required: HashSet<&str> = branch
        .get("required")
        .and_then(|r| r.as_array())
        .map(|a| a.iter().filter_map(serde_json::Value::as_str).collect())
        .unwrap_or_default();

    let mut rows: Vec<(&String, &serde_json::Value)> = props
        .map(|p| {
            p.iter()
                .filter(|(k, _)| k.as_str() != "kind" && k.as_str() != "paths")
                .collect()
        })
        .unwrap_or_default();
    rows.sort_by(|a, b| a.0.cmp(b.0));

    let mut out = String::new();
    let _ = writeln!(&mut out, "## Options");
    let _ = writeln!(&mut out);
    if rows.is_empty() {
        let _ = writeln!(&mut out, "_This rule takes no kind-specific options._");
    } else {
        let _ = writeln!(
            &mut out,
            "| Option | Type | Required | Default | Description |"
        );
        let _ = writeln!(&mut out, "|---|---|---|---|---|");
        for (oname, ov) in rows {
            let ty = format!("{}{}", schema_type_label(ov, schema), constraint_suffix(ov));
            let req = if required.contains(oname.as_str()) {
                "yes"
            } else {
                ""
            };
            let default = ov.get("default").map(render_default).unwrap_or_default();
            let desc = ov
                .get("description")
                .and_then(serde_json::Value::as_str)
                .map(escape_table_cell)
                .unwrap_or_default();
            let _ = writeln!(
                &mut out,
                "| `{oname}` | {ty} | {req} | {default} | {desc} |"
            );
        }
    }
    let _ = writeln!(&mut out);
    if has_paths {
        let _ = writeln!(
            &mut out,
            "Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative."
        );
    } else {
        let _ = writeln!(
            &mut out,
            "Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative."
        );
    }
    out
}

/// Short, human-readable type label for a property schema. Enum-like
/// shapes (an inline `enum`, a bare `const`, a `$ref` to an enum def,
/// a `oneOf`/`anyOf` of those, or an `Option<Enum>`) collapse to a
/// "one of ..." clause; arrays render as `list of <item>`; a nullable
/// `type: [.., "null"]` drops the null; and structured `$ref`s fall
/// back to a humanized name (`nested rule`, `extract spec`, ...).
fn schema_type_label(prop: &serde_json::Value, schema: &serde_json::Value) -> String {
    // 1. Any enum-like shape, including `$ref`-to-enum, a mixed
    //    `oneOf` of `enum`+`const` branches, and `Option<Enum>`.
    if let Some(vals) = collect_enum_values(prop, schema, 0) {
        return enum_label(&vals);
    }
    // 2. A concrete JSON `type` keyword (object / string / array /
    //    nullable `[.., "null"]`). Checked before the union arms
    //    because a node can carry BOTH a `type` and a `oneOf`/`anyOf`
    //    *constraint* (e.g. mutually-exclusive required properties)
    //    that is not a type alternative — `type: object` wins.
    if has_concrete_type(prop) {
        return type_keyword_label(prop, schema);
    }
    // 3. A typeless union of alternative shapes: `Option<T>`
    //    (`anyOf: [T, null]`) or `string | array`. Bare-null branches
    //    (the `Option` tail) are dropped.
    if let Some(branches) = prop
        .get("anyOf")
        .or_else(|| prop.get("oneOf"))
        .and_then(|a| a.as_array())
    {
        return union_label(branches, schema);
    }
    // 4. A `$ref` to a structured (non-enum) sub-shape.
    if let Some(rf) = prop.get("$ref").and_then(serde_json::Value::as_str) {
        return humanize_ref_name(rf);
    }
    // 5. No type information at all — an untyped `serde_json::Value`
    //    option that accepts any JSON.
    "any value".to_string()
}

/// True when `prop` declares a usable JSON `type` keyword — a scalar
/// or a `[.., "null"]` array carrying at least one non-null type.
fn has_concrete_type(prop: &serde_json::Value) -> bool {
    match prop.get("type") {
        Some(serde_json::Value::String(s)) => s != "null",
        Some(serde_json::Value::Array(a)) => {
            a.iter().any(|v| v.as_str().is_some_and(|t| t != "null"))
        }
        _ => false,
    }
}

/// Render a JSON `type` keyword (scalar, or a nullable type-array)
/// to a label. Arrays recurse into `items`.
fn type_keyword_label(prop: &serde_json::Value, schema: &serde_json::Value) -> String {
    let primary = match prop.get("type") {
        Some(serde_json::Value::String(s)) => Some(s.as_str()),
        Some(serde_json::Value::Array(a)) => a
            .iter()
            .filter_map(serde_json::Value::as_str)
            .find(|t| *t != "null"),
        _ => None,
    };
    match primary {
        Some("array") => {
            let item = prop
                .get("items")
                .map_or_else(|| "value".to_string(), |i| schema_type_label(i, schema));
            format!("list of {item}")
        }
        Some("string") => "string".to_string(),
        Some("integer") => "integer".to_string(),
        Some("number") => "number".to_string(),
        Some("boolean") => "boolean".to_string(),
        Some("object") => "object".to_string(),
        Some(other) => other.to_string(),
        None => "any value".to_string(),
    }
}

/// Render an `anyOf`/`oneOf` of non-enum alternatives, dropping the
/// bare-null branch (`Option`'s tail) and de-duplicating labels.
fn union_label(branches: &[serde_json::Value], schema: &serde_json::Value) -> String {
    let labels: Vec<String> = branches
        .iter()
        .filter(|b| !is_null_branch(b))
        .map(|b| schema_type_label(b, schema))
        .collect();
    if labels.is_empty() {
        return "value".to_string();
    }
    join_unique(labels, " or ")
}

/// A schema branch that only permits JSON `null` — the tail of a
/// schemars-rendered `Option<T>` union.
fn is_null_branch(b: &serde_json::Value) -> bool {
    match b.get("type") {
        Some(serde_json::Value::String(s)) => s == "null",
        Some(serde_json::Value::Array(a)) => {
            !a.is_empty() && a.iter().all(|v| v.as_str() == Some("null"))
        }
        _ => false,
    }
}

/// Gather the full set of allowed string values when `node` is an
/// enum-like shape — an inline `enum`, a bare `const`, a `$ref` to an
/// enum def, or a `oneOf`/`anyOf` whose every non-null branch is
/// itself enum-like. schemars splits a doc-commented Rust enum into a
/// mix of `{enum: [...]}` and `{const, description}` branches, and
/// wraps `Option<Enum>` as `anyOf: [<enum>, null]` — both collapse
/// here. Returns `None` for anything that isn't a pure enumeration.
fn collect_enum_values(
    node: &serde_json::Value,
    schema: &serde_json::Value,
    depth: u8,
) -> Option<Vec<String>> {
    if depth > 5 {
        return None;
    }
    if let Some(arr) = node.get("enum").and_then(|e| e.as_array()) {
        let vals: Vec<String> = arr
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(String::from)
            .collect();
        return (vals.len() == arr.len() && !vals.is_empty()).then_some(vals);
    }
    if let Some(c) = node.get("const").and_then(serde_json::Value::as_str) {
        return Some(vec![c.to_string()]);
    }
    if let Some(rf) = node.get("$ref").and_then(serde_json::Value::as_str) {
        return resolve_local_ref(schema, rf)
            .and_then(|t| collect_enum_values(t, schema, depth + 1));
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(branches) = node.get(key).and_then(|a| a.as_array()) {
            let mut all = Vec::new();
            for b in branches {
                if is_null_branch(b) {
                    continue;
                }
                all.extend(collect_enum_values(b, schema, depth + 1)?);
            }
            return (!all.is_empty()).then_some(all);
        }
    }
    None
}

/// Render allowed values as a "one of ..." clause, with the pipe
/// separators escaped so they don't break the markdown table column.
fn enum_label(vals: &[String]) -> String {
    if vals.is_empty() {
        return "string".to_string();
    }
    let parts: Vec<String> = vals.iter().map(|s| format!("`{s}`")).collect();
    format!("one of {}", parts.join(" \\| "))
}

fn resolve_local_ref<'a>(schema: &'a serde_json::Value, rf: &str) -> Option<&'a serde_json::Value> {
    let name = rf.strip_prefix("#/$defs/")?;
    schema.get("$defs").and_then(|d| d.get(name))
}

/// Friendly label for a `$ref` to a non-enum named def. These are
/// the structured sub-shapes (`paths` is excluded upstream, so the
/// common ones are `extract_spec`, `nested_rule`, etc.).
fn humanize_ref_name(rf: &str) -> String {
    match rf.rsplit('/').next().unwrap_or(rf) {
        "string_or_string_array" => "string or list of strings".to_string(),
        "nested_rule" => "nested rule".to_string(),
        "extract_spec" => "extract spec".to_string(),
        "scope_filter" => "scope filter".to_string(),
        "fix" => "fix spec".to_string(),
        "if_present" | "template" | "fact" => "object".to_string(),
        other => other.replace('_', " "),
    }
}

/// A ` (>= N)` / ` (<= N)` / ` (N..M)` note from a property's numeric
/// `minimum`/`maximum`, so a bound the schema enforces is visible in the
/// rendered docs too (e.g. `min_lines` >= 2, `threshold` 0..1) instead of
/// surviving only in the schema an IDE reads.
fn constraint_suffix(prop: &serde_json::Value) -> String {
    // `f64` Display drops a trailing `.0` (`2.0` -> "2", `0.5` -> "0.5"),
    // so whole bounds read cleanly without a lossy integer cast.
    let num = |key: &str| prop.get(key).and_then(serde_json::Value::as_f64);
    match (num("minimum"), num("maximum")) {
        (Some(lo), Some(hi)) => format!(" ({lo}..{hi})"),
        (Some(lo), None) => format!(" (>= {lo})"),
        (None, Some(hi)) => format!(" (<= {hi})"),
        (None, None) => String::new(),
    }
}

fn render_default(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => format!("`{s}`"),
        serde_json::Value::Bool(b) => format!("`{b}`"),
        serde_json::Value::Number(n) => format!("`{n}`"),
        serde_json::Value::Array(a) if a.is_empty() => "`[]`".to_string(),
        serde_json::Value::Null => "`null`".to_string(),
        other => serde_json::to_string(other).map_or_else(|_| String::new(), |s| format!("`{s}`")),
    }
}

/// Pipes break table columns; newlines were already collapsed by
/// gen-schema's `normalize_descriptions`, but guard anyway.
fn escape_table_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// Join with a separator, dropping duplicate labels while keeping
/// first-seen order (e.g. `string or list of strings`, not
/// `string or string or ...`).
fn join_unique(labels: Vec<String>, sep: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    labels
        .into_iter()
        .filter(|l| seen.insert(l.clone()))
        .collect::<Vec<_>>()
        .join(sep)
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;

    #[test]
    fn constraint_suffix_renders_numeric_bounds() {
        assert_eq!(constraint_suffix(&json!({"minimum": 2})), " (>= 2)");
        assert_eq!(constraint_suffix(&json!({"maximum": 100})), " (<= 100)");
        assert_eq!(
            constraint_suffix(&json!({"minimum": 0.0, "maximum": 1.0})),
            " (0..1)"
        );
        assert_eq!(constraint_suffix(&json!({"type": "string"})), "");
    }

    /// A tiny schema carrying the enum `$defs` the `$ref` cases
    /// resolve against. Mirrors the real shapes schemars emits.
    fn ref_schema() -> serde_json::Value {
        json!({
            "$defs": {
                // Plain doc-less enum -> bare `enum` array.
                "Lang": { "type": "string", "enum": ["rust", "go", "python"] },
                // Doc-commented variant -> mixed `oneOf` of an `enum`
                // branch plus a `const`-with-description branch.
                "Preset": { "oneOf": [
                    { "type": "string", "enum": ["go", "rust"] },
                    { "type": "string", "const": "generic", "description": "explicit" }
                ]},
                // A structured (non-enum) sub-shape.
                "extract_spec": { "type": "object", "properties": { "regex": { "type": "string" } } }
            }
        })
    }

    #[test]
    fn type_label_renders_each_schema_shape() {
        let s = ref_schema();
        let label = |v: &serde_json::Value| schema_type_label(v, &s);

        // Scalars.
        assert_eq!(label(&json!({"type": "string"})), "string");
        assert_eq!(label(&json!({"type": "integer"})), "integer");
        assert_eq!(label(&json!({"type": "boolean"})), "boolean");

        // Option<scalar> -> nullable type-array drops the null.
        assert_eq!(label(&json!({"type": ["string", "null"]})), "string");

        // Arrays.
        assert_eq!(
            label(&json!({"type": "array", "items": {"type": "string"}})),
            "list of string"
        );

        // Inline enum and `$ref` to a plain enum def.
        assert_eq!(label(&json!({"enum": ["a", "b"]})), "one of `a` \\| `b`");
        assert_eq!(
            label(&json!({"$ref": "#/$defs/Lang"})),
            "one of `rust` \\| `go` \\| `python`"
        );

        // Mixed `oneOf` (enum branch + const branch) collapses fully.
        assert_eq!(
            label(&json!({"$ref": "#/$defs/Preset"})),
            "one of `go` \\| `rust` \\| `generic`"
        );

        // Option<Enum> -> `anyOf: [<enum ref>, null]`.
        assert_eq!(
            label(&json!({"anyOf": [{"$ref": "#/$defs/Lang"}, {"type": "null"}]})),
            "one of `rust` \\| `go` \\| `python`"
        );

        // `type: object` WITH a `oneOf` constraint stays "object" —
        // the union is a required-property rule, not a type union.
        assert_eq!(
            label(&json!({
                "type": "object",
                "oneOf": [{"required": ["file"]}, {"required": ["files"]}],
                "properties": {"file": {"type": "string"}}
            })),
            "object"
        );

        // Typeless `oneOf` of genuine alternatives -> joined union.
        assert_eq!(
            label(&json!({"oneOf": [
                {"type": "string"},
                {"type": "array", "items": {"type": "string"}}
            ]})),
            "string or list of string"
        );

        // Structured `$ref` -> humanized name.
        assert_eq!(
            label(&json!({"$ref": "#/$defs/extract_spec"})),
            "extract spec"
        );

        // Untyped `serde_json::Value` -> "any value".
        assert_eq!(label(&json!({"description": "anything"})), "any value");
    }

    #[test]
    fn options_section_excludes_universals_and_marks_required() {
        let schema = json!({});
        let branch = json!({
            "properties": {
                "kind": { "const": "demo" },
                "paths": { "$ref": "#/$defs/paths_spec" },
                "threshold": { "type": "number", "default": 0.5, "description": "Density floor." },
                "name": { "type": "string", "description": "Required name." }
            },
            "required": ["name"]
        });
        let out = options_section(&branch, &schema);

        assert!(out.starts_with("## Options"));
        // `kind` and `paths` are dropped from the table.
        assert!(!out.contains("| `kind` |"));
        assert!(!out.contains("| `paths` |"));
        // Required marker + default rendering + alphabetical order.
        assert!(out.contains("| `name` | string | yes |  | Required name. |"));
        assert!(out.contains("| `threshold` | number |  | `0.5` | Density floor. |"));
        assert!(out.find("`name`") < out.find("`threshold`"));
        // Has-`paths` branch advertises the `paths` common field.
        assert!(out.contains("Plus the common `paths`, `level`, `id`, and `when` fields."));
    }

    #[test]
    fn options_section_whole_repo_branch_has_no_options_note() {
        let schema = json!({});
        // No properties beyond `kind` -> a whole-repo rule.
        let branch = json!({ "properties": { "kind": { "const": "demo" } } });
        let out = options_section(&branch, &schema);
        assert!(out.contains("_This rule takes no kind-specific options._"));
        assert!(
            out.contains("this rule analyses the whole repository, so it takes no `paths`")
                || out.contains("This rule analyses the whole repository, so it takes no `paths`.")
        );
    }

    #[test]
    fn kind_branch_index_resolves_aliases_to_one_branch() {
        let schema = json!({
            "$defs": {
                "rule_file_header": {
                    "properties": { "kind": { "enum": ["file_header", "header"] } }
                },
                "rule_no_bom": {
                    "properties": { "kind": { "const": "no_bom" } }
                },
                // Non-rule defs are ignored.
                "paths_spec": { "type": "object" }
            }
        });
        let index = build_kind_branch_index(&schema);
        assert_eq!(index.len(), 3, "two spellings of file_header + no_bom");
        // Both spellings map to the same branch.
        assert_eq!(index.get("file_header"), index.get("header"));
        assert!(index.contains_key("no_bom"));
        assert!(!index.contains_key("paths_spec"));
    }

    /// Integration backstop: every rule branch in the *committed*
    /// schema must render a clean options table — no unclassified
    /// `value` fallback, no empty Type cell. A new rule whose option
    /// shape the renderer can't classify fails here at `cargo test`,
    /// before the broken table ever reaches alint.org.
    #[test]
    fn committed_schema_every_branch_renders_a_clean_table() {
        let root = crate::bench_release::workspace_root().expect("workspace_root");
        let schema = load_config_schema(&root.join("schemas/v1/config.json"))
            .expect("load committed schema");
        let index = build_kind_branch_index(&schema);
        assert!(index.len() > 50, "expected the full rule-kind set");

        for (kind, branch) in &index {
            let section = options_section(branch, &schema);
            assert!(section.starts_with("## Options"), "{kind}: missing heading");
            assert!(
                !section.contains("| value |"),
                "{kind}: unclassified 'value' type leaked into the table:\n{section}"
            );
            // Every data row (starts with "| `name`") must carry a
            // non-empty Type in the second column.
            for row in section.lines().filter(|l| l.starts_with("| `")) {
                let inner = row
                    .trim()
                    .strip_prefix('|')
                    .and_then(|r| r.strip_suffix('|'))
                    .unwrap_or(row);
                let cells: Vec<&str> = inner.split(" | ").map(str::trim).collect();
                assert_eq!(cells.len(), 5, "{kind}: malformed row: {row}");
                assert!(!cells[1].is_empty(), "{kind}: empty Type cell: {row}");
            }
        }
    }
}
