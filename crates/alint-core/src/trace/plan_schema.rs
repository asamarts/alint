//! The schema of `rule.plan` in the experimental trace document: the resolved
//! plan `explain` shows for a rule that reads relations (the `constraint`
//! kind; design `v0.23/selectors-and-constraints.md` section 11), or
//! `<redacted>` for a rule whose `input:` is hidden.
//!
//! The CLI's `inspect_plan.rs` writes the plan; the keys and shapes here are
//! its view types', each object closed (`additionalProperties: false`), so a
//! key it renames or adds fails validation until this schema follows. The
//! names of a closed set are the configuration's own (`roles`, a filter's
//! `predicate`, an `entries:` format, a join's `unique:` side, a `read:`
//! form); free text (a field's `source`, the assertion's `text`, `cost`) is a
//! string. Every value the environment may have written is a value or
//! `<redacted>` in its place. alint's `tests/constraint_explain_cli.rs`
//! validates real plans against it, renames and adds each of their keys, and
//! checks the plans it builds name every key declared here.

use serde_json::{Map, Value, json};

const REDACTED: &str = "<redacted>";

/// `schema`, or `<redacted>` in its place.
fn or_redacted(schema: Value) -> Value {
    let mut one_of = Map::new();
    one_of.insert(
        "oneOf".into(),
        Value::Array(vec![schema, json!({ "const": REDACTED })]),
    );
    Value::Object(one_of)
}

/// An object with exactly the keys of `properties`, `required` among them.
fn closed(properties: Value, required: &[&str]) -> Value {
    let mut object = json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
    });
    object
        .as_object_mut()
        .expect("an object")
        .insert("properties".into(), properties);
    object
}

/// A place in the plan's `relations` list, by which other parts name it.
fn position() -> Value {
    json!({ "type": "integer", "minimum": 0 })
}

fn strings() -> Value {
    json!({ "type": "array", "items": { "type": "string" } })
}

/// A selector's `scope_filter:`: its gates, and the paths each
/// manifest-derived predicate resolves to.
fn scope_filter() -> Value {
    closed(
        json!({
            "has_ancestor": { "oneOf": [strings(), { "type": "null" }] },
            "changed_since": { "type": ["string", "null"] },
            "manifest_scopes": {
                "type": "array",
                "items": closed(
                    json!({
                        "predicate": {
                            "enum": ["include_manifest_paths", "exclude_manifest_paths"]
                        },
                        "source": { "type": "string" },
                        "paths": strings(),
                    }),
                    &["predicate", "source", "paths"],
                ),
            },
        }),
        &["has_ancestor", "changed_since", "manifest_scopes"],
    )
}

/// A selector: a relation's `from:`, or one the trigger lists. A named one
/// has its name; an inline one in the trigger names the relation it is part
/// of (`relation`) and, in a `union:`, its part.
fn selector() -> Value {
    closed(
        json!({
            "selector": { "type": ["string", "null"] },
            "relation": position(),
            "part": position(),
            "declared_in": { "type": "string" },
            "paths": or_redacted(closed(
                json!({ "include": strings(), "exclude": strings() }),
                &["include", "exclude"],
            )),
            "scope_filter": or_redacted(scope_filter()),
        }),
        &["selector", "declared_in", "paths"],
    )
}

/// A `read:` as the config writes it: one form (with optional `strict: true`
/// on `json:`).
fn read() -> Value {
    let query = json!({ "type": "string" });
    json!({
        "type": "object",
        "additionalProperties": false,
        "minProperties": 1,
        "maxProperties": 2,
        "if": { "required": ["strict"] },
        "then": { "required": ["json"], "minProperties": 2, "maxProperties": 2 },
        "else": { "minProperties": 1, "maxProperties": 1 },
        "properties": {
            "json": query,
            "yaml": query,
            "toml": query,
            "regex": query,
            "lines": closed(json!({ "comment": { "type": "string" } }), &[]),
            "whole_file": closed(json!({}), &[]),
            "strict": { "type": "boolean" },
        },
    })
}

fn entries_schema() -> Value {
    let mut obj = closed(
        json!({
            "format": { "enum": ["json", "yaml", "toml"] },
            "at": strings(),
            "leaves": { "type": "boolean" },
            "strict": { "type": "boolean" },
        }),
        &["format", "at", "leaves"],
    );
    let map = obj.as_object_mut().expect("object");
    map.insert("if".into(), json!({ "required": ["strict"] }));
    map.insert(
        "then".into(),
        json!({ "properties": { "format": { "const": "json" } } }),
    );
    obj
}

fn records_schema() -> Value {
    let mut obj = closed(
        json!({
            "format": { "enum": ["json", "yaml", "toml"] },
            "at": strings(),
            "strict": { "type": "boolean" },
        }),
        &["format", "at"],
    );
    let map = obj.as_object_mut().expect("object");
    map.insert("if".into(), json!({ "required": ["strict"] }));
    map.insert(
        "then".into(),
        json!({ "properties": { "format": { "const": "json" } } }),
    );
    obj
}

/// The keys of one part of a relation's rows: its selector and its reader,
/// `entries:`, `records:`, or `read:` with `optional:`.
fn part_properties() -> Value {
    json!({
        "from": selector(),
        "entries": or_redacted(entries_schema()),
        "records": or_redacted(records_schema()),
        "read": or_redacted(read()),
        "optional": or_redacted(json!({ "type": "boolean" })),
    })
}

/// A reader's keys go together: `read:` with `optional:`, and each with a
/// `from:`.
fn reader_dependencies() -> Value {
    json!({
        "entries": ["from"],
        "records": ["from"],
        "read": ["from", "optional"],
        "optional": ["from", "read"],
    })
}

/// One part of a `union:`.
fn part() -> Value {
    let mut properties = part_properties();
    properties.as_object_mut().expect("an object").insert(
        "fields".into(),
        json!({ "type": "array", "items": field() }),
    );
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": properties,
        "required": ["from"],
        "dependentRequired": reader_dependencies(),
        "oneOf": [
            { "required": ["entries"] },
            { "required": ["records"] },
            { "required": ["read"] },
        ],
    })
}

fn join() -> Value {
    closed(
        json!({
            "left": position(),
            "right": position(),
            "on": {
                "type": "array",
                "items": closed(
                    json!({ "left": { "type": "string" }, "right": { "type": "string" } }),
                    &["left", "right"],
                ),
            },
            "unique": or_redacted(json!({ "enum": ["left", "right"] })),
        }),
        &["left", "right", "on", "unique"],
    )
}

fn field() -> Value {
    closed(
        json!({
            "name": { "type": "string" },
            "source": { "type": "string" },
            "optional": { "type": "boolean" },
            "on_missing": { "enum": ["skip", "missing"] },
            "transforms": or_redacted(strings()),
            "sensitive": { "type": "boolean" },
        }),
        &["name", "source", "transforms", "sensitive"],
    )
}

/// A `where:` predicate: `in:` and `not_in:` list members (a literal,
/// `<redacted>`, or another relation's field), `matches:` and
/// `not_matches:` a pattern (or `<redacted>`).
fn filter() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "field": { "type": "string" },
            "predicate": { "enum": ["in", "not_in", "matches", "not_matches"] },
            "members": {
                "type": "array",
                "items": {
                    "oneOf": [
                        { "type": ["string", "boolean"] },
                        closed(
                            json!({ "relation": position(), "field": { "type": "string" } }),
                            &["relation", "field"],
                        ),
                    ],
                },
            },
            "pattern": { "type": "string" },
        },
        "required": ["field", "predicate"],
        "if": { "properties": { "predicate": { "enum": ["in", "not_in"] } } },
        "then": { "required": ["members"], "not": { "required": ["pattern"] } },
        "else": { "required": ["pattern"], "not": { "required": ["members"] } },
    })
}

/// A relation the rule reads: where it is declared and how it is spelled,
/// its rows (one part's keys, a `union:` of parts, or a `join:`), its fields
/// and its `where:`.
fn relation() -> Value {
    let mut properties = part_properties();
    let extra = json!({
        "id": position(),
        "name": { "type": "string" },
        "written": { "type": "array", "items": { "type": "string" }, "minItems": 1 },
        "inline": { "type": "boolean" },
        "roles": {
            "type": "array",
            "items": { "enum": ["input", "set_target", "reference_target"] },
            "uniqueItems": true,
        },
        "declared_in": { "type": "string" },
        "union": { "type": "array", "items": part(), "minItems": 1 },
        "join": join(),
        "fields": { "type": "array", "items": field() },
        "where": { "type": "array", "items": filter() },
    });
    let map = properties.as_object_mut().expect("an object");
    map.extend(extra.as_object().expect("an object").clone());
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": properties,
        "required": ["id", "name", "written", "inline", "roles", "declared_in", "fields", "where"],
        "dependentRequired": reader_dependencies(),
        "oneOf": [
            { "required": ["entries"] },
            { "required": ["records"] },
            { "required": ["read"] },
            { "required": ["union"] },
            { "required": ["join"] },
        ],
    })
}

/// The schema of `rule.plan`.
pub(super) fn plan() -> Value {
    or_redacted(closed(
        json!({
            "input": position(),
            "set_target": position(),
            "reference_target": position(),
            "assertion": or_redacted(closed(
                json!({
                    "family": {
                        "enum": ["equal", "unique", "set", "reference", "count", "compare", "satisfies"]
                    },
                    "text": { "type": "string" },
                }),
                &["family", "text"],
            )),
            "relations": { "type": "array", "items": relation(), "minItems": 1 },
            "trigger": { "type": "array", "items": selector() },
            "cost": { "type": "string" },
        }),
        &["input", "assertion", "relations", "trigger", "cost"],
    ))
}
