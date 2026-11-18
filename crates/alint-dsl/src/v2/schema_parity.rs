//! The draft version 2 schema held to the version 2 decode, in this crate's
//! unit tests only (design `v0.23/selectors-and-constraints.md` section 11).
//!
//! `xtask gen-schema` writes `schemas/v2/config.json`, whose version 2 parts
//! are written by hand as this decode is (`xtask/src/gen_schema_v2.rs`). Three
//! checks keep the two in lockstep:
//! - [`assert_accepts`]: [`super::decode::source_objects`] calls it on every
//!   source it accepts in a unit test, so every fixture of this crate's unit
//!   tests (the decode's, the resolver's, the plan check's, the loader's and
//!   section 12's policies) is a config the schema must accept;
//! - the tests below: each object of the schema has exactly the keys the decode
//!   reads, and wherever a name, a reference, a field source, a transform, an
//!   `entries:` format, a `read:` form or a join side goes, the schema and the
//!   decode agree on every value either of them names, and on some neither does;
//! - `crate::tests::version_two_schema`: the configs the decode refuses for
//!   their shape the schema refuses too, and the ones only the loader refuses,
//!   or that JSON Schema cannot tell apart, it accepts, each with the reason.
//!
//! For what version 2 adds, the schema checks what this decode checks: keys,
//! types, names, lists of values and counts. What the loader checks once every
//! source has loaded (references, cycles, the plan check) stays the loader's.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use alint_core::{ConfigSourceClass, Error, Result};
use serde_json::Value as Json;

use super::ExtendsEntry;
use super::decode::{
    ENTRIES_KEYS, FIELD_KEYS, JOIN_KEYS, MEMBER_KEYS, NAMED_FIELD_SOURCES, PART_KEYS,
    PREDICATE_KEYS, READ_KEYS, RELATION_KEYS, ROW_SOURCES, SELECTOR_FILTER_KEYS, SELECTOR_KEYS,
    TRANSFORMS,
};
use super::document::{EXTENDS_ENTRY_KEYS, V2_ONLY_KEYS, v1_keys};
use super::model::SourceId;

/// The committed draft's text for the crate at `crate_dir`: the workspace's
/// `schemas/v2/config.json`, two directories up. `None` when it is absent
/// and the crate is not in its workspace (a `crates` directory beside a
/// `Cargo.lock`): a packaged crate, as crates.io serves it, or a vendored
/// copy (`vendor/alint-dsl`) holds no schema, and its tests skip the checks
/// against it. Absent inside the workspace, or unreadable, it is an error,
/// which a test reports.
fn draft_at(crate_dir: &Path) -> std::result::Result<Option<String>, String> {
    let workspace = crate_dir.join("..").join("..");
    let path = workspace.join("schemas").join("v2").join("config.json");
    let in_workspace = crate_dir.parent().and_then(Path::file_name) == Some("crates".as_ref())
        && workspace.join("Cargo.lock").exists();
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !in_workspace => Ok(None),
        Err(error) => Err(format!(
            "read {}: {error}; run `cargo run -p xtask -- gen-schema`",
            path.display()
        )),
    }
}

/// The committed draft, read from the workspace when a test first needs it;
/// `None` outside the workspace ([`draft_at`]).
pub(crate) fn schema() -> Option<&'static Json> {
    static SCHEMA: OnceLock<Option<Json>> = OnceLock::new();
    SCHEMA
        .get_or_init(|| {
            let text = draft_at(Path::new(env!("CARGO_MANIFEST_DIR")))
                .unwrap_or_else(|error| panic!("{error}"))?;
            Some(serde_json::from_str(&text).expect("schemas/v2/config.json is JSON"))
        })
        .as_ref()
}

/// Whether the draft is here to check against: a test of it returns early
/// when it is not.
pub(crate) fn has_schema() -> bool {
    schema().is_some()
}

fn validator() -> &'static jsonschema::Validator {
    static VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();
    VALIDATOR.get_or_init(|| {
        let schema = schema().expect("a test of the schema checks `has_schema` first");
        jsonschema::validator_for(schema).expect("schemas/v2/config.json compiles")
    })
}

/// The schema's errors for `instance`, each with where it is; none when the
/// schema accepts it.
pub(crate) fn schema_errors(instance: &Json) -> Vec<String> {
    validator()
        .iter_errors(instance)
        .map(|error| format!("{error} at `{}`", error.instance_path()))
        .collect()
}

/// `yaml` as the schema reads it: its JSON form.
pub(crate) fn json_of(yaml: &str) -> Json {
    let value: ayaml::Value = alint_core::yaml::from_str(yaml).expect("the YAML parses");
    serde_json::to_value(value).expect("the YAML has a JSON form")
}

/// Run `yaml`, an `.alint.yml` as written, through the loader's decode of a
/// version 2 source: the version dispatch, the document check (keys and
/// literal names), the decode of its version 1 keys and
/// [`super::decode::source_objects`]. Interpolation, resolution and the plan
/// check are left out; a config that is not version 2 is an error.
pub(crate) fn decode_stage(yaml: &str) -> Result<()> {
    let document: ayaml::Value =
        alint_core::yaml::from_str(yaml).map_err(|error| Error::Other(error.to_string()))?;
    let origin = super::Origin::new(ConfigSourceClass::Root, ".alint.yml");
    if super::dispatch(Some(&document), yaml, &origin)? != super::Surface::V2 {
        return Err(Error::Other(".alint.yml: not a version 2 config".into()));
    }
    super::check_document(&document, ".alint.yml")?;
    let decoded: super::Document = crate::core_yaml::decode(document)
        .map_err(|error| Error::Other(format!(".alint.yml: {error}")))?;
    let source = SourceId {
        display: ".alint.yml".into(),
        path: PathBuf::from("/repo/.alint.yml"),
        class: ConfigSourceClass::Root,
    };
    super::decode::source_objects(source, &decoded.v2_keys, &decoded.config.extends).map(drop)
}

/// Check that the schema accepts what the decode accepted: a source's
/// `selectors:` and `relations:` as decoded (after interpolation) and its
/// `extends:` entries, under `version: 2`.
pub(super) fn assert_accepts(v2_keys: &[(String, ayaml::Value)], extends: &[ExtendsEntry]) {
    if !has_schema() {
        return;
    }
    let mut document = serde_json::Map::new();
    document.insert("version".into(), Json::from(2));
    for (key, value) in v2_keys {
        let value = serde_json::to_value(value).expect("a decoded value has a JSON form");
        document.insert(key.clone(), value);
    }
    if !extends.is_empty() {
        let entries = extends.iter().map(extends_json).collect();
        document.insert("extends".into(), Json::Array(entries));
    }
    let instance = Json::Object(document);
    let errors = schema_errors(&instance);
    assert!(
        errors.is_empty(),
        "the version 2 decode accepted a source the draft schema refuses; make \
         xtask/src/gen_schema_v2.rs accept it and run `cargo run -p xtask -- gen-schema`:\n\
         {instance:#}\n{}",
        errors.join("\n")
    );
}

/// An `extends:` entry as it was written, near enough for the schema. An empty
/// `only:` or `except:` list is left out: the decode leaves those lists to the
/// loader, which refuses an empty one later, as the schema does
/// (`crate::tests::version_two_schema`).
fn extends_json(entry: &ExtendsEntry) -> Json {
    let (url, only, except) = match &entry.entry {
        alint_core::ExtendsEntry::Url(url) => (url, None, None),
        alint_core::ExtendsEntry::Filtered { url, only, except } => (
            url,
            only.as_ref().filter(|ids| !ids.is_empty()),
            except.as_ref().filter(|ids| !ids.is_empty()),
        ),
    };
    if only.is_none() && except.is_none() && entry.alias.is_none() {
        return Json::from(url.as_str());
    }
    let mut map = serde_json::Map::new();
    map.insert("url".into(), Json::from(url.as_str()));
    if let Some(only) = only {
        map.insert("only".into(), Json::from(only.clone()));
    }
    if let Some(except) = except {
        map.insert("except".into(), Json::from(except.clone()));
    }
    if let Some(alias) = &entry.alias {
        map.insert("as".into(), Json::from(alias.as_str()));
    }
    Json::Object(map)
}

fn def(name: &str) -> &'static Json {
    let def = &schema().expect("the draft")["$defs"][name];
    assert!(!def.is_null(), "the schema has no `$defs/{name}`");
    def
}

fn keys(object: &Json) -> BTreeSet<&str> {
    object["properties"]
        .as_object()
        .expect("an object with properties")
        .keys()
        .map(String::as_str)
        .collect()
}

fn set<'a>(list: &[&'a str]) -> BTreeSet<&'a str> {
    list.iter().copied().collect()
}

/// The string constants a definition lists, in a `oneOf` of `const`s or an
/// `enum`.
fn constants(def: &Json) -> BTreeSet<&str> {
    if let Some(values) = def["enum"].as_array() {
        return values.iter().filter_map(Json::as_str).collect();
    }
    def["oneOf"]
        .as_array()
        .expect("a `oneOf` of constants")
        .iter()
        .map(|form| form["const"].as_str().expect("a `const`"))
        .collect()
}

#[test]
fn every_object_has_the_keys_the_decode_reads() {
    if !has_schema() {
        return;
    }
    let top: BTreeSet<&str> = v1_keys().iter().chain(V2_ONLY_KEYS).copied().collect();
    assert_eq!(keys(schema().expect("the draft")), top);
    let extends_mapping = def("v2_extends_entry")["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .find(|form| form.get("properties").is_some())
        .expect("a mapping form");
    assert_eq!(keys(extends_mapping), set(EXTENDS_ENTRY_KEYS));
    for (name, list) in [
        ("v2_selector", SELECTOR_KEYS),
        ("v2_selector_scope_filter", SELECTOR_FILTER_KEYS),
        ("v2_relation", RELATION_KEYS),
        ("v2_union_part", PART_KEYS),
        ("v2_entries", ENTRIES_KEYS),
        ("v2_read", READ_KEYS),
        ("v2_join", JOIN_KEYS),
        ("v2_predicates", PREDICATE_KEYS),
        ("v2_membership", MEMBER_KEYS),
    ] {
        let object = def(name);
        assert_eq!(keys(object), set(list), "`$defs/{name}`");
        assert_eq!(object["additionalProperties"], false, "`$defs/{name}`");
    }
    for rows in ROW_SOURCES {
        let field = def(&format!("v2_field_{rows}"))["oneOf"][1].clone();
        assert_eq!(keys(&field), set(FIELD_KEYS), "`$defs/v2_field_{rows}`");
    }
    // One branch of a relation per row source.
    let branches: BTreeSet<&str> = def("v2_relation")["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|branch| branch["required"][0].as_str().unwrap())
        .collect();
    assert_eq!(branches, set(ROW_SOURCES));
}

#[test]
fn the_schema_lists_the_values_the_decode_takes() {
    if !has_schema() {
        return;
    }
    let transforms = &def("v2_transforms")["additionalProperties"]["items"];
    assert_eq!(constants(transforms), set(TRANSFORMS));
    let named: BTreeSet<&str> = ["v2_source_path_token", "v2_source_entry", "v2_source_value"]
        .into_iter()
        .flat_map(|name| constants(def(name)))
        .collect();
    let decoded: BTreeSet<&str> = NAMED_FIELD_SOURCES.iter().map(|(name, _)| *name).collect();
    assert_eq!(named, decoded);
}

/// `relations: { r: ... }` with rows `rows` and the fields `fields`.
fn relation(rows: &str, fields: &str) -> String {
    format!("version: 2\nrelations:\n  r:\n    {rows}\n    fields: {fields}\n")
}

/// Assert that the decode and the schema agree on each config of `configs`,
/// and that they accept at least one.
fn agree(configs: impl IntoIterator<Item = String>) {
    let mut accepted = 0;
    for config in configs {
        let decoded = decode_stage(&config);
        let errors = schema_errors(&json_of(&config));
        assert_eq!(
            decoded.is_ok(),
            errors.is_empty(),
            "the decode and the draft schema disagree on\n{config}\ndecode: {decoded:?}\n\
             schema: {errors:?}"
        );
        accepted += usize::from(decoded.is_ok());
    }
    assert!(accepted > 0, "no config was accepted");
}

/// The rows of each kind of relation.
const ROWS: &[&str] = &[
    "from: s\n    entries: { format: json, at: $ }",
    "from: s\n    read: { json: $.a }",
    "union:\n      - { from: s, read: { json: $.a } }\n      - { from: s, entries: { format: json, \
     at: $ } }",
    "join: { left: a, right: b, on: { k: k }, unique: left }",
];

#[test]
fn the_decode_and_the_schema_agree_on_each_field_source() {
    if !has_schema() {
        return;
    }
    let mut sources: Vec<String> = NAMED_FIELD_SOURCES
        .iter()
        .map(|(name, _)| (*name).to_string())
        .collect();
    sources.extend(
        [
            "bound: $.name",
            "bound:$.name",
            "bound:",
            "bound:  ",
            "left.f",
            "right.f_2",
            "left.a.b",
            "left.",
            "middle.f",
            "entry.name",
            "entry",
            "Path",
            "values",
        ]
        .map(String::from),
    );
    agree(ROWS.iter().flat_map(|rows| {
        sources.iter().flat_map(move |source| {
            [
                relation(rows, &format!("{{ f: '{source}' }}")),
                relation(rows, &format!("{{ f: {{ source: '{source}' }} }}")),
                relation(
                    rows,
                    &format!("{{ f: {{ source: '{source}', sensitive: true }} }}"),
                ),
            ]
        })
    }));
}

#[test]
fn the_decode_and_the_schema_agree_on_each_transform() {
    if !has_schema() {
        return;
    }
    let mut transforms: Vec<&str> = TRANSFORMS.to_vec();
    transforms.extend(["cldr-plurals", "upper", "Trim", "semver"]);
    agree(transforms.into_iter().map(|transform| {
        relation(
            &format!(
                "from: s\n    read: {{ json: $.a }}\n    transform: {{ f: [trim, '{transform}'] }}"
            ),
            "{ f: value }",
        )
    }));
}

#[test]
fn the_decode_and_the_schema_agree_on_formats_forms_and_sides() {
    if !has_schema() {
        return;
    }
    let formats = ["json", "yaml", "toml", "xml", "JSON", "ini"].map(|format| {
        relation(
            &format!("from: s\n    entries: {{ format: {format}, at: $ }}"),
            "{ f: entry.key }",
        )
    });
    let mut forms: Vec<String> = READ_KEYS
        .iter()
        .chain(&[
            "xml",
            "dotenv",
            "properties",
            "ini",
            "hcl",
            "jsonpath",
            "Json",
        ])
        .map(|form| {
            let operand = match *form {
                "regex" => "'(x)'",
                "lines" | "whole_file" => "{}",
                _ => "$.a",
            };
            relation(
                &format!("from: s\n    read: {{ {form}: {operand} }}"),
                "{ f: value }",
            )
        })
        .collect();
    forms.extend(formats);
    let sides = ["left", "right", "both", "Left", "none"].map(|side| {
        relation(
            &format!("join: {{ left: a, right: b, on: {{ k: k }}, unique: {side} }}"),
            "{ f: left.f }",
        )
    });
    forms.extend(sides);
    agree(forms);
}

#[test]
fn the_decode_and_the_schema_agree_on_names_and_references() {
    if !has_schema() {
        return;
    }
    let names = [
        "a", "_a", "a-b_c9", "Deps", "x1", "9a", "-a", "a.b", "'a b'", "naïve", "a/b", "''",
    ];
    let declared = names.map(|name| format!("version: 2\nselectors:\n  {name}: {{ paths: x }}\n"));
    let fields = names.map(|name| {
        relation(
            "from: s\n    read: { json: $.a }",
            &format!("{{ {name}: value }}"),
        )
    });
    let aliases =
        names.map(|name| format!("version: 2\nextends:\n  - {{ url: ./base.yml, as: {name} }}\n"));
    let references = [
        "a", "a.b", "a.b.c", "9.a", "a.", ".a", "a..b", "a-b._c", "'a b'",
    ];
    let from = references.map(|reference| {
        relation(
            &format!("from: {reference}\n    read: {{ json: $.a }}"),
            "{ f: value }",
        )
    });
    let sides = references.map(|reference| {
        relation(
            &format!("join: {{ left: {reference}, right: b, on: {{ k: k }}, unique: left }}"),
            "{ f: left.f }",
        )
    });
    agree(
        declared
            .into_iter()
            .chain(fields)
            .chain(aliases)
            .chain(from)
            .chain(sides),
    );
}

/// The draft is read from the workspace; a packaged or vendored crate, with
/// no workspace of its own around it, skips the checks against it, and a
/// workspace without the draft fails them.
#[test]
fn the_draft_is_required_in_the_workspace_and_skipped_outside_it() {
    let tmp = tempfile::tempdir().unwrap();
    let crate_dir = tmp.path().join("crates").join("alint-dsl");
    std::fs::create_dir_all(&crate_dir).unwrap();
    assert_eq!(draft_at(&crate_dir), Ok(None), "a packaged crate");
    std::fs::write(tmp.path().join("Cargo.lock"), "").unwrap();
    let vendored = tmp.path().join("vendor").join("alint-dsl");
    std::fs::create_dir_all(&vendored).unwrap();
    assert_eq!(draft_at(&vendored), Ok(None), "a vendored crate");
    let error = draft_at(&crate_dir).unwrap_err();
    assert!(
        error.contains("run `cargo run -p xtask -- gen-schema`"),
        "{error}"
    );
    let schemas = tmp.path().join("schemas").join("v2");
    std::fs::create_dir_all(&schemas).unwrap();
    std::fs::write(schemas.join("config.json"), "{}").unwrap();
    assert_eq!(draft_at(&crate_dir), Ok(Some("{}".to_string())));
}
