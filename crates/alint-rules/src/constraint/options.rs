//! The `constraint` kind's options as a config writes them (design
//! `v0.23/selectors-and-constraints.md` section 6, "Syntax"): `input:` and
//! `assert:`, which holds exactly one of `equal`, `unique` and `set`.

use std::collections::BTreeMap;

use serde::Deserialize;

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
    pub equal: Option<Equal>,
    /// No two rows share a key; with `determines:`, rows that share a key
    /// agree on the determined fields.
    pub unique: Option<Unique>,
    /// The set of a field's values, per group, is equal to, a subset of or
    /// disjoint from the set of another group.
    pub set: Option<Set>,
}

/// `assert.equal`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ConstraintEqual")]
pub(super) struct Equal {
    /// The compared field.
    pub field: String,
    /// Compare within each group of rows that share this field's value;
    /// without it, the relation is one group. Not with `to:`.
    pub group_by: Option<String>,
    /// Compare each row with one named source row (`{ row: { FIELD: literal
    /// } }`), or with another field of the same row (`{ field: G }`).
    pub to: Option<EqualTo>,
}

/// `assert.equal.to`: exactly one of `row` and `field`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ConstraintEqualTo")]
#[schemars(extend("minProperties" = 1, "maxProperties" = 1))]
pub(super) struct EqualTo {
    /// The source row: the one row whose fields hold these literals.
    pub row: Option<BTreeMap<String, Literal>>,
    /// Another field of the same row.
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
    #[schemars(length(min = 1))]
    pub key: Vec<String>,
    /// The fields the key determines: rows that share the key agree on them.
    pub determines: Option<Vec<String>>,
}

/// `assert.set`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ConstraintSet")]
pub(super) struct Set {
    /// The field whose values form each group's set.
    pub field: String,
    /// The field whose value is each row's group.
    pub group_by: Option<String>,
    /// Each group's set equals the other set.
    pub equal_to: Option<SetTarget>,
    /// Each group's set is a subset of the other set.
    pub subset_of: Option<SetTarget>,
    /// Each group's set shares no value with the other set.
    pub disjoint_from: Option<SetTarget>,
    /// Groups that must exist, rows or not. Not yet supported: only `[]`.
    pub groups: Option<Vec<serde_json::Value>>,
    /// `exact` compares values as they are; `cldr` collapses CLDR plural
    /// groups (not yet supported).
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

/// The other set: a group of the same relation (`{ group: literal }`), or a
/// field of another relation (`{ relation: R, field: F }`, not yet
/// supported).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ConstraintSetTarget")]
pub(super) struct SetTarget {
    /// The reference group: the rows whose `group_by:` field holds this
    /// literal.
    pub group: Option<Literal>,
    /// Another relation (not yet supported).
    pub relation: Option<String>,
    /// That relation's field (not yet supported).
    pub field: Option<String>,
}

crate::options_schema_for!(Options);
