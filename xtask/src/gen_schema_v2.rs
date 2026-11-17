//! The draft version 2 config schema, `schemas/v2/config.json` (design
//! `v0.23/selectors-and-constraints.md` section 11).
//!
//! `gen-schema` derives it from the generated version 1 schema, which version 2
//! accepts whole with its version 1 meaning, plus what version 2 adds: a
//! `version:` of `2`, top-level `selectors:` and `relations:`, and an `as:` on
//! an `extends:` entry. Version 2 is experimental (ADR-0034 decision 1), so the
//! schema is a draft: its title and description say so, and its `$id` names the
//! release that ships it rather than `main`, as the version 1 schema's does.
//!
//! The version 2 decode (`crates/alint-dsl/src/v2/decode.rs`) is written by
//! hand, not derived with serde, so the additions are written by hand here. Two
//! gates hold them to the decode instead:
//! - alint-dsl's unit tests (`src/v2/schema_parity.rs` and
//!   `src/tests/version_two_schema.rs`): every selector, relation and alias the
//!   decode accepts in any unit test validates against the committed schema;
//!   every object's keys and every list of values equal the decode's own lists;
//!   a corpus of configs the decode refuses for their shape is refused; and a
//!   corpus the loader alone refuses (names, cycles, what the plan check finds,
//!   what JSON Schema cannot say) is accepted, which documents the limits;
//! - alint-e2e (`tests/version_two_schema.rs`): every version 2 config of the
//!   end-to-end scenarios, the CLI fixtures and the version 2 docs page.
//!
//! The schema checks what the decode checks for the parts version 2 adds (keys,
//! types, names, lists of values and counts), and no more: the loader's plan
//! check (references, cycles, field existence, literal types) is the loader's
//! alone. What version 2 takes from version 1 (`paths:`, the `scope_filter:`
//! predicates, `extends:` entries, rules) it takes from the version 1 schema.

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Map, Value, json};

/// The release whose tag the draft's `$id` names. A release that changes the
/// draft names itself here, so an editor that cached one draft never
/// validates against another under the same `$id`.
pub(crate) const RELEASE: &str = "v0.23.0";

/// The tail of the version 1 schema's `$id` (on `main`), which the draft's
/// `$id` replaces with the release tag and its own path.
const V1_ID_TAIL: &str = "/main/schemas/v1/config.json";

/// A name: an ASCII letter or `_`, then ASCII letters, digits, `_` and `-`.
const NAME: &str = "[A-Za-z_][A-Za-z0-9_-]*";

/// The path tokens a field may take, each a string (design section 5).
const PATH_TOKENS: &[(&str, &str)] = &[
    ("path", "The file's path, relative to the repository root."),
    ("dir", "The directory that holds the file."),
    ("basename", "The file's name with its extension."),
    ("stem", "The file's name without its extension."),
    ("ext", "The file's extension, without the dot."),
    (
        "parent_name",
        "The name of the directory that holds the file.",
    ),
];

/// The `entries:` row sources.
const ENTRY_SOURCES: &[(&str, &str)] = &[
    ("entry.key", "The entry's own key."),
    (
        "entry.value",
        "The entry's value: a scalar (null, a boolean, a number with its lexeme kept, or a \
         string). An object, an array or a TOML date there is an analysis failure.",
    ),
    (
        "entry.parent",
        "The key of the object that holds the entry (`dependencies` for a member of \
         `$.dependencies`), or null when that object is the document root.",
    ),
    (
        "entry.root",
        "The first key of the entry's key path (`en` for `en.nav.home`).",
    ),
    (
        "entry.path",
        "The entry's key path from the document root (`nav.home`, `['a.b']`), which \
         `strip-root` applies to.",
    ),
];

/// The `read:` row source.
const VALUE_SOURCE: (&str, &str) = ("value", "The value a `read:` extracted, a string.");

/// The fixed transforms (ADR-0037 decision 2).
const TRANSFORMS: &[(&str, &str)] = &[
    ("trim", "Trim leading and trailing whitespace."),
    ("lower", "Lower-case the value."),
    (
        "semver-major",
        "Keep only a version's leading `MAJOR` token, as `cross_file`'s `normalize:` does.",
    ),
    (
        "semver-minor",
        "Keep only a version's leading `MAJOR.MINOR` band, as `cross_file`'s `normalize:` \
         does (`4.36-dev`, `4.36.0` and `>=4.36` all become `4.36`).",
    ),
    (
        "strip-root",
        "Drop the first segment of an `entry.path` field (`en.nav.home` becomes \
         `nav.home`). It applies to an `entry.path` field, or a join's copy of one no \
         transform has changed, first.",
    ),
];

/// The rows a relation's fields are read from, each with the field sources it
/// takes (`decode.rs` `field_fits_rows`): a join's fields name a side, an
/// `entries:` row has `entry.*`, a `read:` row has `value`, a `read:` of the
/// file as text has no `bound:` field, and which part of a `union:` binds
/// which field is the plan check's.
#[derive(Clone, Copy)]
enum RowsKind {
    Entries,
    /// A `read:` of a JSON, YAML or TOML document.
    Read,
    /// A `read:` of the file as text: `regex:`, `lines:` or `whole_file:`.
    ReadText,
    Union,
    Join,
}

/// The `read:` forms that read the file as text, which bind no `bound:`
/// field.
const TEXT_READS: [&str; 3] = ["regex", "lines", "whole_file"];

impl RowsKind {
    const ALL: [Self; 5] = [
        Self::Entries,
        Self::Read,
        Self::ReadText,
        Self::Union,
        Self::Join,
    ];

    /// The key that gives a relation these rows.
    const fn key(self) -> &'static str {
        match self {
            Self::Entries => "entries",
            Self::Read | Self::ReadText => "read",
            Self::Union => "union",
            Self::Join => "join",
        }
    }

    /// The name of these rows' definitions.
    const fn name(self) -> &'static str {
        match self {
            Self::ReadText => "read_text",
            other => other.key(),
        }
    }

    fn field_def(self) -> String {
        format!("v2_field_{}", self.name())
    }

    fn source_def(self) -> String {
        format!("v2_field_source_{}", self.name())
    }

    /// The field sources a relation with these rows takes.
    fn sources(self) -> Value {
        let path_tokens = json!({ "$ref": "#/$defs/v2_source_path_token" });
        let entry = json!({ "$ref": "#/$defs/v2_source_entry" });
        let value = json!({ "$ref": "#/$defs/v2_source_value" });
        let bound = json!({ "$ref": "#/$defs/v2_source_bound" });
        let (description, any_of) = match self {
            Self::Entries => (
                "A field source of an `entries:` relation: a path token, an `entry.*` source \
                 or `bound: <JSONPath>`.",
                vec![path_tokens, entry, bound],
            ),
            Self::Read => (
                "A field source of a `read:` relation of a JSON, YAML or TOML document: a path \
                 token, `value` or `bound: <JSONPath>`.",
                vec![path_tokens, value, bound],
            ),
            Self::ReadText => (
                "A field source of a `read:` relation that reads the file as text (`regex:`, \
                 `lines:` or `whole_file:`): a path token or `value`. A `bound:` field reads a \
                 JSON, YAML or TOML document.",
                vec![path_tokens, value],
            ),
            Self::Union => (
                "A field source of a `union:` relation: a path token, an `entry.*` source, \
                 `value` or `bound: <JSONPath>`. Every part must be able to bind it.",
                vec![path_tokens, entry, value, bound],
            ),
            Self::Join => {
                return json!({
                    "type": "string",
                    "pattern": format!("^(left|right)\\.{NAME}$"),
                    "description": "A field of a join's output: `left.<field>` or \
                        `right.<field>`, a field of that side. Nothing is copied implicitly.",
                });
            }
        };
        json!({ "anyOf": any_of, "description": description })
    }

    /// The `oneOf` branch of a relation with these rows: its own key (with
    /// `from:` for `entries:` and `read:`), none of the other row sources, and
    /// the field sources these rows take.
    fn branch(self) -> Value {
        let mut required = vec![self.key()];
        let mut excluded: Vec<&str> = Self::ALL
            .iter()
            .map(|kind| kind.key())
            .filter(|key| *key != self.key())
            .collect();
        excluded.dedup();
        match self {
            Self::Entries => {
                required.push("from");
                excluded.push("optional");
            }
            Self::Read | Self::ReadText => required.push("from"),
            Self::Union | Self::Join => excluded.extend(["from", "optional"]),
        }
        let excluded: Vec<Value> = excluded
            .into_iter()
            .map(|key| json!({ "required": [key] }))
            .collect();
        let fields = json!({
            "additionalProperties": { "$ref": format!("#/$defs/{}", self.field_def()) }
        });
        // `v2_read` holds exactly one form, so a `read:` is of one kind.
        let text: Vec<Value> = TEXT_READS
            .iter()
            .map(|form| json!({ "required": [form] }))
            .collect();
        let properties = match self {
            Self::Read => json!({ "read": { "not": { "anyOf": text } }, "fields": fields }),
            Self::ReadText => json!({ "read": { "anyOf": text }, "fields": fields }),
            _ => json!({ "fields": fields }),
        };
        json!({
            "required": required,
            "not": { "anyOf": excluded },
            "properties": properties,
        })
    }
}

/// A `oneOf` of string constants, each with its description.
fn constants(values: &[(&str, &str)], description: &str) -> Value {
    let one_of: Vec<Value> = values
        .iter()
        .map(|(value, description)| json!({ "const": value, "description": description }))
        .collect();
    json!({ "type": "string", "oneOf": one_of, "description": description })
}

/// A map keyed by names, each value matching `value`.
fn named_map(value: &Value, description: &str) -> Value {
    json!({
        "type": "object",
        "propertyNames": { "$ref": "#/$defs/v2_name" },
        "additionalProperties": value,
        "description": description,
    })
}

/// Build the draft version 2 schema from the generated version 1 `v1`.
pub(crate) fn build(v1: &Value) -> Result<Value> {
    let mut schema = v1.clone();
    let root = schema
        .as_object_mut()
        .context("the version 1 schema is not an object")?;

    let v1_id = root
        .get("$id")
        .and_then(Value::as_str)
        .context("the version 1 schema has no `$id`")?;
    let base = v1_id.strip_suffix(V1_ID_TAIL).with_context(|| {
        format!("the version 1 schema's `$id` ({v1_id}) does not end in `{V1_ID_TAIL}`")
    })?;
    let id = format!("{base}/{RELEASE}/schemas/v2/config.json");
    root.insert("$id".into(), Value::from(id));
    root.insert(
        "title".into(),
        Value::from("alint configuration (v2, experimental draft)"),
    );
    root.insert(
        "description".into(),
        Value::from(
            "DRAFT. Schema for .alint.yml configuration files that declare `version: 2`, the \
             experimental version of alint's configuration language. Version 2 may change in \
             any release until one declares it stable (v0.24 at the earliest), and this \
             draft's `$id` names the release that ships it. A version 2 config accepts every \
             version 1 key with its version 1 meaning, plus `selectors:`, `relations:` and an \
             `extends:` entry's `as:`. Reference: docs/site/configuration/version-2.md in the \
             alint repository.",
        ),
    );

    let properties = root
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .context("the version 1 schema has no `properties` object")?;
    add_properties(properties)?;

    let defs = root
        .get_mut("$defs")
        .and_then(Value::as_object_mut)
        .context("the version 1 schema has no `$defs` object")?;
    let additions = additions(defs)?;
    for (name, def) in additions {
        if defs.contains_key(&name) {
            bail!(
                "the version 1 schema already defines `$defs/{name}`; rename the version 2 \
                 definition in xtask/src/gen_schema_v2.rs"
            );
        }
        defs.insert(name, def);
    }
    Ok(schema)
}

/// Turn the version 1 `properties` into version 2's: `version: 2`, `as:` on
/// an `extends:` entry, `selectors:` and `relations:`.
fn add_properties(properties: &mut Map<String, Value>) -> Result<()> {
    for key in ["version", "extends"] {
        ensure!(
            properties.contains_key(key),
            "the version 1 schema has no `{key}` property"
        );
    }
    for key in ["selectors", "relations"] {
        ensure!(
            !properties.contains_key(key),
            "the version 1 schema already has a `{key}` property"
        );
    }
    properties.insert(
        "version".into(),
        json!({
            "const": 2,
            "description": "Config language version: the literal integer `2` (a version read \
                from the environment, quoted or written as a YAML alias is version 1). Version \
                2 is experimental.",
        }),
    );
    let extends = properties["extends"]
        .as_object_mut()
        .context("the version 1 `extends` property is not an object")?;
    extends.insert(
        "items".into(),
        json!({ "$ref": "#/$defs/v2_extends_entry" }),
    );
    let extends_description = extends
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default();
    extends.insert(
        "description".into(),
        Value::from(format!(
            "{extends_description} In a version 2 config, a local entry may also take `as: \
             <alias>`, through which this config reaches the selectors and relations that \
             source declares as `<alias>.<name>`."
        )),
    );
    properties.insert(
        "selectors".into(),
        named_map(
            &json!({ "$ref": "#/$defs/v2_selector" }),
            "Named selectors (version 2): a map from each name to the files a relation reads. \
             A config file declares a name once, across its selectors, relations and \
             `extends:` aliases.",
        ),
    );
    properties.insert(
        "relations".into(),
        named_map(
            &json!({ "$ref": "#/$defs/v2_relation" }),
            "Named relations (version 2): a map from each name to rows with named fields, \
             read from the files a selector picks. A config file declares a name once, across \
             its selectors, relations and `extends:` aliases.",
        ),
    );
    Ok(())
}

/// The definitions version 2 adds, two of them derived from the version 1
/// definitions in `v1_defs`.
fn additions(v1_defs: &Map<String, Value>) -> Result<Vec<(String, Value)>> {
    let mut defs = name_and_selector_defs(v1_defs)?;
    defs.extend(relation_defs());
    defs.extend(field_defs());
    Ok(defs)
}

/// Names, references, `extends:` entries and selectors.
fn name_and_selector_defs(v1_defs: &Map<String, Value>) -> Result<Vec<(String, Value)>> {
    Ok(vec![
        (
            "v2_name".into(),
            json!({
                "type": "string",
                "pattern": format!("^{NAME}$"),
                "description": "A name in a version 2 config (a selector, a relation, a field \
                    or an `extends:` alias): an ASCII letter or `_`, then ASCII letters, \
                    digits, `_` and `-`. It is written literally: `{{...}}` is refused.",
            }),
        ),
        (
            "v2_reference".into(),
            json!({
                "type": "string",
                "pattern": format!("^{NAME}(\\.{NAME})?$"),
                "description": "A selector or relation: a name this config file declares, or \
                    `alias.name` for one that the file an `extends:` entry gives `as: alias` \
                    declares itself (no re-export, no `a.b.c` chain).",
            }),
        ),
        ("v2_extends_entry".into(), extends_entry(v1_defs)?),
        (
            "v2_selector".into(),
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["paths"],
                "properties": {
                    "paths": {
                        "$ref": "#/$defs/paths_spec",
                        "description": "The files the selector picks, as a rule's `paths:`.",
                    },
                    "scope_filter": { "$ref": "#/$defs/v2_selector_scope_filter" },
                },
                "description": "A selector: the files a relation reads, picked as a rule's \
                    `paths:` and `scope_filter:` pick them.",
            }),
        ),
        (
            "v2_selector_scope_filter".into(),
            selector_scope_filter(v1_defs)?,
        ),
        (
            "v2_selector_ref".into(),
            json!({
                "oneOf": [
                    { "$ref": "#/$defs/v2_reference" },
                    { "$ref": "#/$defs/v2_selector" },
                ],
                "description": "The files a relation reads: a selector's name (a string is \
                    always a name, never a glob), or an inline selector map.",
            }),
        ),
    ])
}

/// A relation and the parts of one but its fields.
#[allow(clippy::too_many_lines)] // A table of JSON Schema definitions.
fn relation_defs() -> Vec<(String, Value)> {
    vec![
        ("v2_relation".into(), relation()),
        (
            "v2_union_part".into(),
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["from"],
                "properties": {
                    "from": { "$ref": "#/$defs/v2_selector_ref" },
                    "entries": { "$ref": "#/$defs/v2_entries" },
                    "read": { "$ref": "#/$defs/v2_read" },
                    "optional": optional(),
                },
                "oneOf": [
                    {
                        "required": ["entries"],
                        "not": { "anyOf": [{ "required": ["read"] }, { "required": ["optional"] }] },
                    },
                    { "required": ["read"], "not": { "required": ["entries"] } },
                ],
                "description": "One part of a `union:`: its own `from:`, and exactly one of \
                    `entries:` and `read:`.",
            }),
        ),
        (
            "v2_entries".into(),
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["format", "at"],
                "properties": {
                    "format": {
                        "enum": ["json", "yaml", "toml"],
                        "description": "How the files are decoded: JSON, YAML or TOML.",
                    },
                    "at": {
                        "oneOf": [
                            { "type": "string" },
                            { "type": "array", "minItems": 1, "items": { "type": "string" } },
                        ],
                        "description": "A JSONPath, or a non-empty list of them, selecting the \
                            objects whose members become rows, in list order. A path a file \
                            lacks yields no rows from it; one that selects an array or a \
                            scalar is an analysis failure.",
                    },
                    "leaves": {
                        "type": "boolean",
                        "default": false,
                        "description": "One row per leaf below each selected object (a key \
                            with no key below it; an array is a value) instead of one per \
                            member.",
                    },
                },
                "description": "One row per member of each object `at` selects, read with the \
                    `entry.*` field sources.",
            }),
        ),
        (
            "v2_read".into(),
            json!({
                "type": "object",
                "minProperties": 1,
                "maxProperties": 1,
                "additionalProperties": false,
                "properties": {
                    "json": {
                        "type": "string",
                        "description": "A JSONPath read from JSON: one row per string it matches.",
                    },
                    "yaml": {
                        "type": "string",
                        "description": "A JSONPath read from YAML: one row per string it matches.",
                    },
                    "toml": {
                        "type": "string",
                        "description": "A JSONPath read from TOML: one row per string it matches.",
                    },
                    "regex": {
                        "type": "string",
                        "description": "A regex: one row per match, whose value is capture group 1.",
                    },
                    "lines": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "comment": {
                                "type": "string",
                                "description": "Lines starting with this, after trimming, are \
                                    skipped. Default `#`.",
                            },
                        },
                        "description": "One row per trimmed, non-empty line.",
                    },
                    "whole_file": {
                        "type": "object",
                        "maxProperties": 0,
                        "description": "The whole file as one row. It takes no options: write \
                            `whole_file: {}`.",
                    },
                },
                "description": "Exactly one extract form, as `cross_file` reads it; the rows \
                    hold strings, read with the `value` field source. In this build a relation \
                    cannot read dotenv, properties, XML, INI or HCL.",
            }),
        ),
        (
            "v2_join".into(),
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["left", "right", "on", "unique"],
                "properties": {
                    "left": {
                        "$ref": "#/$defs/v2_reference",
                        "description": "The left relation.",
                    },
                    "right": {
                        "$ref": "#/$defs/v2_reference",
                        "description": "The right relation.",
                    },
                    "on": {
                        "type": "object",
                        "minProperties": 1,
                        "propertyNames": { "$ref": "#/$defs/v2_name" },
                        "additionalProperties": { "$ref": "#/$defs/v2_name" },
                        "description": "The join key: a map from each left field to the right \
                            field it must equal.",
                    },
                    "unique": {
                        "enum": ["left", "right"],
                        "description": "The side that holds at most one row per join key; a \
                            repeated key there is an analysis failure. It has no default.",
                    },
                },
                "description": "An inner equality join of two relations: one row per pair of \
                    rows whose `on:` fields are equal, in the order of the side that is not \
                    unique.",
            }),
        ),
    ]
}

/// Field sources, `transform:` and `where:`, and each kind of relation's
/// `fields:` entries.
#[allow(clippy::too_many_lines)] // A table of JSON Schema definitions.
fn field_defs() -> Vec<(String, Value)> {
    let mut defs: Vec<(String, Value)> = vec![
        (
            "v2_source_path_token".into(),
            constants(
                PATH_TOKENS,
                "A path token of the file the row was read from.",
            ),
        ),
        (
            "v2_source_entry".into(),
            constants(ENTRY_SOURCES, "A part of an `entries:` row."),
        ),
        (
            "v2_source_value".into(),
            constants(&[VALUE_SOURCE], "The value of a `read:` row."),
        ),
        (
            "v2_source_bound".into(),
            json!({
                "type": "string",
                "pattern": "^bound:\\s*\\S",
                "description": "`bound: <JSONPath>`: a value read once per file (a manifest's \
                    `$.name`), which must match exactly once in each file.",
            }),
        ),
        (
            "v2_transforms".into(),
            named_map(
                &json!({
                    "type": "array",
                    "minItems": 1,
                    "items": constants(TRANSFORMS, "A transform."),
                }),
                "Transforms: a map from a field's name to the transforms applied to its \
                 values, in list order, before `where:`.",
            ),
        ),
        (
            "v2_filters".into(),
            named_map(
                &json!({ "$ref": "#/$defs/v2_predicates" }),
                "Row filters: a map from a field's name to predicates that must all hold for \
                 a row to be kept. They apply after `transform:`.",
            ),
        ),
        (
            "v2_predicates".into(),
            json!({
                "type": "object",
                "minProperties": 1,
                "additionalProperties": false,
                "properties": {
                    "in": {
                        "$ref": "#/$defs/v2_members",
                        "description": "Keep a row whose value is one of these.",
                    },
                    "not_in": {
                        "$ref": "#/$defs/v2_members",
                        "description": "Keep a row whose value is none of these.",
                    },
                    "matches": {
                        "type": "string",
                        "description": "A regex a string value must match.",
                    },
                    "not_matches": {
                        "type": "string",
                        "description": "A regex a string value must not match.",
                    },
                },
                "description": "The predicates on one field, all of which must hold.",
            }),
        ),
        (
            "v2_members".into(),
            json!({
                "type": "array",
                "minItems": 1,
                "items": {
                    "oneOf": [
                        { "type": ["null", "boolean", "number", "string"] },
                        { "$ref": "#/$defs/v2_membership" },
                    ],
                },
                "description": "Literals and `{ relation, field }` references, in one list. \
                    The loader refuses a literal that can never match: null, a number, and a \
                    boolean for a field that holds strings.",
            }),
        ),
        (
            "v2_membership".into(),
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["relation", "field"],
                "properties": {
                    "relation": { "$ref": "#/$defs/v2_reference" },
                    "field": { "$ref": "#/$defs/v2_name" },
                },
                "description": "Every value another relation's field holds.",
            }),
        ),
    ];
    for kind in RowsKind::ALL {
        let source = format!("#/$defs/{}", kind.source_def());
        defs.push((
            kind.field_def(),
            json!({
                "oneOf": [
                    { "$ref": source },
                    {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["source"],
                        "properties": {
                            "source": { "$ref": source },
                            "sensitive": {
                                "type": "boolean",
                                "default": false,
                                "description": "Keep this field's values out of messages, the \
                                    JSON report and baselines.",
                            },
                        },
                    },
                ],
                "description": "A field: its source, or a `{ source, sensitive }` map.",
            }),
        ));
        defs.push((kind.source_def(), kind.sources()));
    }
    defs
}

/// `optional:`, beside a `read:`.
fn optional() -> Value {
    json!({
        "type": "boolean",
        "default": false,
        "description": "With `read:`: a file the read matches nothing in yields no row, \
            instead of one missing row.",
    })
}

/// A relation: `fields:` and exactly one of `entries:` (with `from:`), `read:`
/// (with `from:` and `optional:`), `union:` and `join:`.
fn relation() -> Value {
    let branches: Vec<Value> = RowsKind::ALL.iter().map(|kind| kind.branch()).collect();
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["fields"],
        "properties": {
            "from": { "$ref": "#/$defs/v2_selector_ref" },
            "entries": { "$ref": "#/$defs/v2_entries" },
            "read": { "$ref": "#/$defs/v2_read" },
            "optional": optional(),
            "union": {
                "type": "array",
                "minItems": 2,
                "items": { "$ref": "#/$defs/v2_union_part" },
                "description": "The rows of two or more parts, in the order written. Every \
                    part binds every field.",
            },
            "join": { "$ref": "#/$defs/v2_join" },
            "fields": {
                "type": "object",
                "minProperties": 1,
                "propertyNames": { "$ref": "#/$defs/v2_name" },
                "description": "The row's named fields: a map from each field's name to its \
                    source, or to `{ source, sensitive }`.",
            },
            "transform": { "$ref": "#/$defs/v2_transforms" },
            "where": { "$ref": "#/$defs/v2_filters" },
        },
        "oneOf": branches,
        "description": "A relation: rows with named fields. It has `fields:` and exactly one \
            of `entries:` and `read:` (each with `from:`), `union:` and `join:`, and may have \
            `transform:` and `where:`.",
    })
}

/// Version 1's `extends:` entry, whose mapping form also takes `as:`, which
/// a bundled or remote entry refuses.
fn extends_entry(v1_defs: &Map<String, Value>) -> Result<Value> {
    let mut entry = v1_defs
        .get("extends_entry")
        .context("the version 1 schema has no `$defs/extends_entry`")?
        .clone();
    let mapping = entry
        .get_mut("oneOf")
        .and_then(Value::as_array_mut)
        .and_then(|forms| {
            forms
                .iter_mut()
                .find(|form| form.pointer("/properties/url").is_some())
        })
        .and_then(Value::as_object_mut)
        .context("the version 1 `extends_entry` has no mapping form with `url`")?;
    mapping
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .context("the version 1 `extends_entry` mapping has no properties")?
        .insert(
            "as".into(),
            json!({
                "$ref": "#/$defs/v2_name",
                "description": "An alias for this local source: this config reaches the \
                    selectors and relations it declares as `<alias>.<name>`.",
            }),
        );
    // `EntryKind::of`: `https://` and `alint://bundled/` in any ASCII case.
    mapping.insert("if".into(), json!({ "required": ["as"] }));
    mapping.insert(
        "then".into(),
        json!({
            "properties": {
                "url": {
                    "not": {
                        "pattern": "^([Hh][Tt][Tt][Pp][Ss]://|[Aa][Ll][Ii][Nn][Tt]://[Bb][Uu][Nn][Dd][Ll][Ee][Dd]/)",
                    },
                },
            },
        }),
    );
    entry.as_object_mut().expect("checked above").insert(
        "description".into(),
        Value::from(
            "One extends entry. Bare string = classic form (url-only). Mapping form lets you \
             filter with `only:` / `except:` (mutually exclusive) and, for a local source, \
             name it with `as:`; a bundled or remote source must be version 1 and takes no \
             `as:`.",
        ),
    );
    Ok(entry)
}

/// Version 1's `scope_filter`, without `changed_since`: what a selector picks
/// must not depend on the diff.
fn selector_scope_filter(v1_defs: &Map<String, Value>) -> Result<Value> {
    let mut filter = v1_defs
        .get("scope_filter")
        .context("the version 1 schema has no `$defs/scope_filter`")?
        .clone();
    let object = filter
        .as_object_mut()
        .context("the version 1 `scope_filter` is not an object")?;
    let removed = object
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .context("the version 1 `scope_filter` has no properties")?
        .remove("changed_since");
    ensure!(
        removed.is_some(),
        "the version 1 `scope_filter` has no `changed_since` property"
    );
    let any_of = object
        .get_mut("anyOf")
        .and_then(Value::as_array_mut)
        .context("the version 1 `scope_filter` has no `anyOf`")?;
    let before = any_of.len();
    any_of.retain(|branch| branch != &json!({ "required": ["changed_since"] }));
    ensure!(
        any_of.len() + 1 == before,
        "the version 1 `scope_filter` `anyOf` has no `changed_since` branch"
    );
    object.insert(
        "description".into(),
        Value::from(
            "A selector's scope filter: a rule's `scope_filter:` predicates but \
             `changed_since`. `has_ancestor` admits files with a named manifest in an \
             ancestor directory; `include_manifest_paths` and `exclude_manifest_paths` admit \
             or drop files by membership in a path set a manifest declares (ADR-0010). \
             `changed_since` is refused: what a selector picks must not depend on the diff.",
        ),
    );
    Ok(filter)
}

#[cfg(test)]
#[path = "gen_schema_v2_tests.rs"]
mod tests;
