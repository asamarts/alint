//! The `constraint` kind's options as a config writes them (design
//! `v0.23/selectors-and-constraints.md` section 6, "Syntax"): `input:` and
//! `assert:`, which holds exactly one of `equal`, `unique` and `set`.
//!
//! The draft version 2 schema's `constraint` branch is derived from these
//! types (`xtask gen-schema`), so it states the shapes the builder checks: one
//! family, one `to:` form, one `set` target of one shape, `group_by:` where a
//! group is named, `groups:` only under `equal_to:`, `plurals: cldr` only
//! against a group, and names and references by the version 2 patterns (the
//! `$defs/v2_name` and `$defs/v2_reference` the version 2 schema adds). An
//! optional key written as null is refused here, so that a null counts as
//! neither a family nor a target, as in the schema. What needs the relation
//! (whether a field exists, what it holds) is the builder's alone.

use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer};

/// An optional key that is present must hold a value: a null is refused, as
/// the schema refuses it, rather than read as an absent key.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    match Option::<T>::deserialize(deserializer)? {
        Some(value) => Ok(Some(value)),
        None => Err(serde::de::Error::custom(
            "an optional key written as null; give it a value, or leave the key out",
        )),
    }
}

/// Drop the `default: null` schemars gives an optional key: a null is
/// refused ([`present`]), so the default is the key left out, not null.
fn no_default(schema: &mut Schema) {
    schema.remove("default");
}

/// The schema of a field of the relation: a version 2 name, as the draft
/// version 2 schema defines it.
struct FieldName;

impl schemars::JsonSchema for FieldName {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "v2_name".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "$ref": "#/$defs/v2_name" })
    }
}

/// The schema of one or more field names, none twice.
struct FieldNames;

impl schemars::JsonSchema for FieldNames {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "ConstraintFieldNames".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "array",
            "items": { "$ref": "#/$defs/v2_name" },
            "minItems": 1,
            "uniqueItems": true,
        })
    }
}

/// The schema of a relation's name, bare or qualified by an `extends:`
/// alias, as the draft version 2 schema defines it.
struct Reference;

impl schemars::JsonSchema for Reference {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "v2_reference".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "$ref": "#/$defs/v2_reference" })
    }
}

/// The schema of a source row: one or more fields, each with its literal.
struct SourceRow;

impl schemars::JsonSchema for SourceRow {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "ConstraintSourceRow".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let literal = generator.subschema_for::<Literal>();
        json_schema!({
            "type": "object",
            "propertyNames": { "$ref": "#/$defs/v2_name" },
            "additionalProperties": literal,
            "minProperties": 1,
        })
    }
}

/// Options for the `constraint` kind, a version 2 kind that asserts over the
/// rows of a relation.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Options {
    /// The rows the assertion reads: a relation's name, bare or qualified by
    /// an `extends:` alias (`deps.ranges`), or an inline relation (a map, as
    /// a relation under `relations:` is written, whose values may not read
    /// the environment).
    // Read for its presence only: the loader resolved it (a name, or a map it
    // planned as an inline relation), and the plan holds the relation.
    #[allow(dead_code)]
    #[schemars(with = "serde_json::Value")]
    pub input: alint_core::yaml::Value,
    /// The assertion: exactly one of `equal`, `unique` and `set`.
    pub assert: Assert,
}

/// `assert:`: exactly one of its three keys.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ConstraintAssert")]
#[schemars(extend("minProperties" = 1, "maxProperties" = 1))]
pub(super) struct Assert {
    /// Rows agree on a field: within each group, with a named source row, or
    /// with another field of the same row.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "Equal")]
    pub equal: Option<Equal>,
    /// No two rows share a key; with `determines:`, rows that share a key
    /// agree on the determined fields.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "Unique")]
    pub unique: Option<Unique>,
    /// The set of a field's values, per group, is equal to, a subset of or
    /// disjoint from another set: a group's, or another relation's field's.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "Set")]
    pub set: Option<Set>,
}

/// `assert.equal`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ConstraintEqual")]
#[schemars(extend("not" = { "required": ["group_by", "to"] }))]
pub(super) struct Equal {
    /// The compared field.
    #[schemars(with = "FieldName")]
    pub field: String,
    /// Compare within each group of rows that share this field's value;
    /// without it, the relation is one group. Not with `to:`.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "FieldName")]
    pub group_by: Option<String>,
    /// Compare each row with one named source row (`{ row: { FIELD: literal
    /// } }`, every field holding its literal), or with another field of the
    /// same row (`{ field: G }`).
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "EqualTo")]
    pub to: Option<EqualTo>,
}

/// `assert.equal.to`: exactly one of `row` and `field`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ConstraintEqualTo")]
#[schemars(extend("minProperties" = 1, "maxProperties" = 1))]
pub(super) struct EqualTo {
    /// The source row: the one row whose fields hold these literals, every
    /// one of them.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "SourceRow")]
    pub row: Option<BTreeMap<String, Literal>>,
    /// Another field of the same row.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "FieldName")]
    pub field: Option<String>,
}

/// A literal: a string or a boolean. The loader refuses a number (a value
/// compares by the text it is written with, which the config's YAML does not
/// keep) and a null (which matches nothing).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(transparent)]
#[schemars(rename = "ConstraintLiteral")]
pub(super) struct Literal(#[schemars(with = "LiteralSchema")] pub alint_core::yaml::Value);

/// What a literal's schema accepts.
#[derive(schemars::JsonSchema)]
#[serde(untagged)]
#[schemars(rename = "ConstraintLiteralValue")]
#[allow(dead_code)] // Only its schema is used.
enum LiteralSchema {
    String(String),
    Bool(bool),
}

/// `assert.unique`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ConstraintUnique")]
pub(super) struct Unique {
    /// The key: one or more field names.
    #[schemars(with = "FieldNames")]
    pub key: Vec<String>,
    /// The fields the key determines: rows that share the key agree on them.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "FieldNames")]
    pub determines: Option<Vec<String>>,
}

/// `assert.set`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ConstraintSet")]
#[schemars(extend(
    // Exactly one other set.
    "oneOf" = [
        { "required": ["equal_to"] },
        { "required": ["subset_of"] },
        { "required": ["disjoint_from"] },
    ],
    "allOf" = [
        // A reference group is a value of `group_by:`.
        {
            "if": { "anyOf": [
                { "required": ["equal_to"], "properties": { "equal_to": { "required": ["group"] } } },
                { "required": ["subset_of"], "properties": { "subset_of": { "required": ["group"] } } },
                {
                    "required": ["disjoint_from"],
                    "properties": { "disjoint_from": { "required": ["group"] } },
                },
            ] },
            "then": { "required": ["group_by"] },
        },
        // Groups that must exist are values of `group_by:`, which only
        // `equal_to:` checks; an empty list names none.
        {
            "if": { "required": ["groups"], "properties": { "groups": { "minItems": 1 } } },
            "then": { "required": ["group_by", "equal_to"] },
        },
        // `plurals: cldr` compares two groups of one relation.
        {
            "if": { "required": ["plurals"], "properties": { "plurals": { "const": "cldr" } } },
            "then": { "properties": {
                "equal_to": { "required": ["group"] },
                "subset_of": { "required": ["group"] },
                "disjoint_from": { "required": ["group"] },
            } },
        },
    ],
))]
pub(super) struct Set {
    /// The field whose values form each group's set.
    #[schemars(with = "FieldName")]
    pub field: String,
    /// The field whose value is each row's group; without it, the relation
    /// is one group. Grouped by `path` or a path token, every resource the
    /// relation reads forms its group, rows or not, except one whose path
    /// alone the relation's `where:` rejects.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "FieldName")]
    pub group_by: Option<String>,
    /// Each group's set equals the other set.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "SetTarget")]
    pub equal_to: Option<SetTarget>,
    /// Each group's set is a subset of the other set.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "SetTarget")]
    pub subset_of: Option<SetTarget>,
    /// Each group's set shares no value with the other set.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "SetTarget")]
    pub disjoint_from: Option<SetTarget>,
    /// Groups that must exist, rows or not, with `equal_to:`: literals of
    /// the `group_by:` field, none twice. A group named only here lacks every
    /// value of the other set.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "Vec<Literal>", extend("uniqueItems" = true))]
    pub groups: Option<Vec<Literal>>,
    /// `exact` compares values as they are; `cldr` collapses CLDR plural
    /// groups of an `entry.path` field, as `key_parity` does (`{ group: }`
    /// targets only).
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "Plurals")]
    pub plurals: Option<Plurals>,
}

/// `exact` or `cldr`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "ConstraintPlurals")]
pub(super) enum Plurals {
    Exact,
    Cldr,
}

/// The other set: a group of the same relation (`{ group: literal }`), or
/// every value of another relation's field (`{ relation: R, field: F }`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ConstraintSetTarget")]
#[schemars(extend("oneOf" = [
    { "required": ["group"], "maxProperties": 1 },
    { "required": ["relation", "field"], "maxProperties": 2 },
]))]
pub(super) struct SetTarget {
    /// The reference group: the rows whose `group_by:` field holds this
    /// literal.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "Literal")]
    pub group: Option<Literal>,
    /// Another relation: its name, bare or qualified by an `extends:` alias
    /// (`ws.names`); not an inline relation.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "Reference")]
    pub relation: Option<String>,
    /// That relation's field, whose values are the other set.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "FieldName")]
    pub field: Option<String>,
}

crate::options_schema_for!(Options);
