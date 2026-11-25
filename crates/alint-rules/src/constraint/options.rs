//! The `constraint` kind's options as a config writes them (design
//! `v0.23/selectors-and-constraints.md` section 6, ADR-0050 D1 to D4): `input:`
//! and `assert:`, which holds exactly one of `equal`, `unique`, `set`,
//! `reference`, `count`, and `compare`.
//!
//! The draft version 2 schema's `constraint` branch is derived from these
//! types (`xtask gen-schema`), so it states the shapes the builder checks: one
//! family, one `to:` form, one `set` target of one shape, `group_by:` where a
//! group is named, `groups:` only under `equal_to:` (or `count` with
//! `min > 0`/`exact > 0`), `plurals: cldr` only against a group, and names and
//! references by the version 2 patterns (the `$defs/v2_name` and
//! `$defs/v2_reference` the version 2 schema adds). An optional key written as
//! null is refused here, so that a null counts as neither a family nor a
//! target, as in the schema. What needs the relation (whether a field exists,
//! what it holds, arity matching) is the builder's alone.

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

/// A single field name or a non-empty list of distinct field names
/// (multi-field `group_by:`, ADR-0050 D3).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum FieldNameOrList {
    One(String),
    Many(Vec<String>),
}

impl schemars::JsonSchema for FieldNameOrList {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "ConstraintFieldNameOrList".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "oneOf": [
                { "$ref": "#/$defs/v2_name" },
                {
                    "type": "array",
                    "items": { "$ref": "#/$defs/v2_name" },
                    "minItems": 1,
                    "uniqueItems": true,
                },
            ]
        })
    }
}

/// A group key literal: a scalar literal for 1-field `group_by:`, or a
/// fixed-length array of literals (`minItems: 2`) for multi-field `group_by:`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum GroupLiteral {
    Tuple(Vec<Literal>),
    Single(Literal),
}

impl schemars::JsonSchema for GroupLiteral {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "ConstraintGroupLiteral".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let literal = generator.subschema_for::<Literal>();
        json_schema!({
            "oneOf": [
                literal,
                {
                    "type": "array",
                    "items": literal,
                    "minItems": 2,
                },
            ]
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
    /// The assertion: exactly one of `equal`, `unique`, `set`, `reference`,
    /// `count`, and `compare`.
    pub assert: Assert,
}

/// `assert:`: exactly one of its six keys.
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
    /// disjoint from another set: a group's, another relation's field's, or
    /// an inline list of values.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "Set")]
    pub set: Option<Set>,
    /// Every row's key field or composite key tuple resolves to a row in
    /// another relation (foreign key, ADR-0050 D1).
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "ReferenceAssert")]
    pub reference: Option<ReferenceAssert>,
    /// Row count or distinct-value count bounds, whole-relation or per group
    /// (ADR-0050 D2).
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "Count")]
    pub count: Option<Count>,
    /// Pairwise row comparison (`eq`, `ne`, `lt`, `le`, `gt`, `ge`) under
    /// exact, lexical, or numeric ordering (ADR-0050 D4).
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "Compare")]
    pub compare: Option<Compare>,
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
    /// Compare within each group of rows that share this field's (or fields')
    /// value; without it, the relation is one group. Not with `to:`.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "FieldNameOrList")]
    pub group_by: Option<FieldNameOrList>,
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
        // A reference group or per-group relation target requires `group_by:`.
        {
            "if": { "anyOf": [
                { "required": ["equal_to"], "properties": { "equal_to": { "required": ["group"] } } },
                { "required": ["subset_of"], "properties": { "subset_of": { "required": ["group"] } } },
                { "required": ["disjoint_from"], "properties": { "disjoint_from": { "required": ["group"] } } },
                { "required": ["equal_to"], "properties": { "equal_to": { "required": ["group_by"] } } },
                { "required": ["subset_of"], "properties": { "subset_of": { "required": ["group_by"] } } },
                { "required": ["disjoint_from"], "properties": { "disjoint_from": { "required": ["group_by"] } } },
            ] },
            "then": { "required": ["group_by"] },
        },
        // `all_target_groups:` is valid only on `equal_to:`.
        {
            "not": { "anyOf": [
                { "required": ["subset_of"], "properties": { "subset_of": { "required": ["all_target_groups"] } } },
                { "required": ["disjoint_from"], "properties": { "disjoint_from": { "required": ["all_target_groups"] } } },
            ] },
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
        // Single-field `group_by` (string or 1-item array) pairs with scalar
        // group literals; multi-field `group_by` (2+ items) pairs with arrays.
        {
            "if": {
                "required": ["group_by"],
                "properties": { "group_by": { "type": "array", "minItems": 2 } },
            },
            "then": {
                "properties": {
                    "equal_to": { "properties": { "group": { "type": "array" } } },
                    "subset_of": { "properties": { "group": { "type": "array" } } },
                    "disjoint_from": { "properties": { "group": { "type": "array" } } },
                    "groups": { "items": { "type": "array" } },
                }
            },
            "else": {
                "properties": {
                    "equal_to": { "properties": { "group": { "not": { "type": "array" } } } },
                    "subset_of": { "properties": { "group": { "not": { "type": "array" } } } },
                    "disjoint_from": { "properties": { "group": { "not": { "type": "array" } } } },
                    "groups": { "items": { "not": { "type": "array" } } },
                }
            },
        },
    ],
))]
pub(super) struct Set {
    /// The field whose values form each group's set.
    #[schemars(with = "FieldName")]
    pub field: String,
    /// The field or fields whose values form each row's group; without it, the
    /// relation is one group. Grouped by `path` or path tokens, every resource
    /// the relation reads forms its group, rows or not, except one whose path
    /// alone the relation's `where:` rejects.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "FieldNameOrList")]
    pub group_by: Option<FieldNameOrList>,
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
    /// the `group_by:` field(s), none twice. A group named only here lacks
    /// every value of the other set.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "Vec<GroupLiteral>", extend("uniqueItems" = true))]
    pub groups: Option<Vec<GroupLiteral>>,
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

/// The other set: a group of the same relation (`{ group: literal }`), an
/// inline list of literals (`{ values: [...] }`), or another relation's field
/// (`{ relation: R, field: F, group_by?: ..., all_target_groups?: bool }`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ConstraintSetTarget")]
#[schemars(extend(
    "oneOf" = [
        { "required": ["group"], "maxProperties": 1 },
        { "required": ["values"], "maxProperties": 1 },
        {
            "required": ["relation", "field"],
            "not": { "anyOf": [{ "required": ["group"] }, { "required": ["values"] }] },
        },
    ],
    "allOf" = [
        {
            "if": { "required": ["all_target_groups"] },
            "then": { "required": ["group_by"] },
        },
    ],
))]
pub(super) struct SetTarget {
    /// The reference group: the rows whose `group_by:` field(s) hold this
    /// literal (scalar for 1-field, array for multi-field).
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "GroupLiteral")]
    pub group: Option<GroupLiteral>,
    /// An inline non-empty list of allowed/expected/forbidden scalar literals
    /// of one uniform type (all strings or all booleans, ADR-0050 D3).
    #[serde(default, deserialize_with = "present")]
    #[schemars(
        transform = no_default,
        with = "Vec<Literal>",
        extend(
            "minItems" = 1,
            "oneOf" = [
                { "items": { "type": "string" } },
                { "items": { "type": "boolean" } },
            ],
        )
    )]
    pub values: Option<Vec<Literal>>,
    /// Another relation: its name, bare or qualified by an `extends:` alias
    /// (`ws.names`); not an inline relation.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "Reference")]
    pub relation: Option<String>,
    /// That relation's field, whose values are the other set.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "FieldName")]
    pub field: Option<String>,
    /// Partition the target relation by this field or fields (same arity as
    /// the input's `group_by:`) for per-group cross-relation comparison
    /// (ADR-0050 D3).
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "FieldNameOrList")]
    pub group_by: Option<FieldNameOrList>,
    /// With `equal_to:` and target `group_by:`: also report missing values for
    /// target groups absent from the input relation (default false).
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "bool")]
    pub all_target_groups: Option<bool>,
}

/// `assert.reference` (ADR-0050 D1).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ConstraintReference")]
#[schemars(extend("oneOf" = [
    { "required": ["field"], "not": { "required": ["fields"] } },
    { "required": ["fields"], "not": { "required": ["field"] } },
]))]
pub(super) struct ReferenceAssert {
    /// Single source key field (mutually exclusive with `fields:`).
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "FieldName")]
    pub field: Option<String>,
    /// Ordered composite source key fields (mutually exclusive with `field:`).
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "FieldNames")]
    pub fields: Option<Vec<String>>,
    /// Target relation and key field(s) of matching arity.
    pub to: ReferenceTo,
    /// When true, skip rows where any source key field is `Missing` (for
    /// example from an optional `bound:` or `record:` field) instead of
    /// emitting a finding (default false; explicit `Null` values still emit a
    /// finding).
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "bool")]
    pub skip_missing: Option<bool>,
}

/// `assert.reference.to`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ConstraintReferenceTo")]
#[schemars(extend("oneOf" = [
    { "required": ["field"], "not": { "required": ["fields"] } },
    { "required": ["fields"], "not": { "required": ["field"] } },
]))]
pub(super) struct ReferenceTo {
    /// Target relation name, bare or qualified by an `extends:` alias.
    #[allow(dead_code)] // Resolved through `plan.rule` references.
    #[schemars(with = "Reference")]
    pub relation: String,
    /// Single target key field (mutually exclusive with `fields:`).
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "FieldName")]
    pub field: Option<String>,
    /// Ordered composite target key fields (mutually exclusive with `field:`).
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "FieldNames")]
    pub fields: Option<Vec<String>>,
}

/// `assert.count` (ADR-0050 D2).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ConstraintCount")]
#[schemars(extend(
    "anyOf" = [
        { "required": ["min"] },
        { "required": ["max"] },
        { "required": ["exact"] },
    ],
    "not" = {
        "anyOf": [
            { "required": ["exact", "min"] },
            { "required": ["exact", "max"] },
        ]
    },
    "allOf" = [
        {
            "if": { "required": ["distinct"], "properties": { "distinct": { "const": true } } },
            "then": { "required": ["field"] },
            "else": { "not": { "required": ["field"] } },
        },
        {
            "if": { "required": ["groups"], "properties": { "groups": { "minItems": 1 } } },
            "then": {
                "required": ["group_by"],
                "anyOf": [
                    { "required": ["min"], "properties": { "min": { "minimum": 1 } } },
                    { "required": ["exact"], "properties": { "exact": { "minimum": 1 } } },
                ],
            },
        },
        {
            "if": {
                "required": ["group_by"],
                "properties": { "group_by": { "type": "array", "minItems": 2 } },
            },
            "then": { "properties": { "groups": { "items": { "type": "array" } } } },
            "else": { "properties": { "groups": { "items": { "not": { "type": "array" } } } } },
        },
    ],
))]
pub(super) struct Count {
    /// Optional grouping field or fields; omitted counts across the whole
    /// relation.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "FieldNameOrList")]
    pub group_by: Option<FieldNameOrList>,
    /// Field whose distinct non-null/present values are counted; required iff
    /// `distinct: true`, refused otherwise.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "FieldName")]
    pub field: Option<String>,
    /// Count distinct non-null values of `field:` instead of rows (default
    /// false).
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "bool")]
    pub distinct: Option<bool>,
    /// Minimum allowed count.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "u64")]
    pub min: Option<u64>,
    /// Maximum allowed count (`0` asserts emptiness and emits per-row
    /// findings).
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "u64")]
    pub max: Option<u64>,
    /// Exact required count (`0` asserts emptiness; mutually exclusive with
    /// `min:` and `max:`).
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "u64")]
    pub exact: Option<u64>,
    /// Groups that must exist (requires `group_by:` and `min > 0` or
    /// `exact > 0`).
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "Vec<GroupLiteral>", extend("uniqueItems" = true))]
    pub groups: Option<Vec<GroupLiteral>>,
}

/// `assert.compare` (ADR-0050 D4).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ConstraintCompare")]
#[schemars(extend(
    "allOf" = [
        // Under `order: exact` (or omitted): only `eq`/`ne`, and `to.value` is string/bool.
        {
            "if": {
                "anyOf": [
                    { "not": { "required": ["order"] } },
                    { "properties": { "order": { "const": "exact" } } },
                ]
            },
            "then": {
                "properties": {
                    "op": { "enum": ["eq", "ne"] },
                    "to": {
                        "properties": {
                            "value": { "type": ["string", "boolean"] },
                        }
                    },
                }
            },
        },
        // Under `order: lexical`: `to.value` must be a string.
        {
            "if": {
                "required": ["order"],
                "properties": { "order": { "const": "lexical" } },
            },
            "then": {
                "properties": {
                    "to": { "properties": { "value": { "type": "string" } } },
                }
            },
        },
        // Under `order: numeric`: `to.value` must be a number.
        {
            "if": {
                "required": ["order"],
                "properties": { "order": { "const": "numeric" } },
            },
            "then": {
                "properties": {
                    "to": { "properties": { "value": { "type": "number" } } },
                }
            },
        },
    ]
))]
pub(super) struct Compare {
    /// Left-hand field to compare.
    #[schemars(with = "FieldName")]
    pub field: String,
    /// Comparison operator: `eq`, `ne`, `lt`, `le`, `gt`, or `ge`.
    pub op: CompareOp,
    /// Right-hand target: another field of the same row, a literal value, or a
    /// named source row.
    pub to: CompareTo,
    /// Ordering domain: `exact` (default; `eq`/`ne` only), `lexical`, or
    /// `numeric`.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "CompareOrder")]
    pub order: Option<CompareOrder>,
}

/// Comparison operator for `assert.compare`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "ConstraintCompareOp")]
pub(super) enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Ordering domain for `assert.compare`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "ConstraintCompareOrder")]
pub(super) enum CompareOrder {
    #[default]
    Exact,
    Lexical,
    Numeric,
}

/// `assert.compare.to`: exactly one of `field`, `value`, and `row`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ConstraintCompareTo")]
#[schemars(extend("minProperties" = 1, "maxProperties" = 1))]
pub(super) struct CompareTo {
    /// Another field of the same row.
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "FieldName")]
    pub field: Option<String>,
    /// A literal value (string/bool for `exact`, string for `lexical`, finite
    /// number for `numeric`).
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "CompareValueSchema")]
    pub value: Option<alint_core::yaml::Value>,
    /// A named source row whose fields hold these literals (matching
    /// `equal.to.row`).
    #[serde(default, deserialize_with = "present")]
    #[schemars(transform = no_default, with = "SourceRow")]
    pub row: Option<BTreeMap<String, Literal>>,
}

#[derive(schemars::JsonSchema)]
#[serde(untagged)]
#[schemars(rename = "ConstraintCompareValue")]
#[allow(dead_code)] // Only its schema is used.
enum CompareValueSchema {
    String(String),
    Bool(bool),
    Number(f64),
}

crate::options_schema_for!(Options);
