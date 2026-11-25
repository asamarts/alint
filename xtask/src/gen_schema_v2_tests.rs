//! Unit tests of the draft version 2 schema's assembly (`gen_schema_v2.rs`).
//! Its parity with the version 2 decode is alint-dsl's to test
//! (`src/v2/schema_parity.rs`, `src/tests/version_two_schema.rs`).

use std::collections::BTreeSet;

use super::*;

fn v1() -> Value {
    crate::gen_schema::build_generated_schema().expect("the version 1 schema builds")
}

/// What the draft is derived from: the version 1 schema with the kinds of
/// version 2.
fn full() -> Value {
    crate::gen_schema::build_full_schema().expect("the full schema builds")
}

fn draft() -> Value {
    build(&full()).expect("the draft builds")
}

fn validator(schema: &Value) -> jsonschema::Validator {
    jsonschema::validator_for(schema).expect("the schema compiles")
}

/// Every `#/$defs/<name>` that `value` references.
fn references(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(name) = map
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|reference| reference.strip_prefix("#/$defs/"))
            {
                out.insert(name.to_string());
            }
            map.values().for_each(|child| references(child, out));
        }
        Value::Array(items) => items.iter().for_each(|item| references(item, out)),
        _ => {}
    }
}

#[test]
fn the_draft_names_its_release_and_says_it_is_a_draft() {
    let schema = draft();
    assert_eq!(
        schema["$id"],
        "https://raw.githubusercontent.com/asamarts/alint/v0.24.0/schemas/v2/config.json"
    );
    assert_eq!(
        schema["title"],
        "alint configuration (v2, experimental draft)"
    );
    let description = schema["description"].as_str().unwrap();
    assert!(description.starts_with("DRAFT. "), "{description}");
    assert!(
        description.contains("v0.25 at the earliest"),
        "{description}"
    );
    // The version 1 schema keeps its `$id` on `main`.
    assert_eq!(
        v1()["$id"],
        "https://raw.githubusercontent.com/asamarts/alint/main/schemas/v1/config.json"
    );
}

#[test]
fn the_draft_keeps_version_one_whole_and_adds_the_version_two_surface() {
    let v1 = v1();
    let schema = build(&v1).unwrap();
    for (name, def) in v1["$defs"].as_object().unwrap() {
        assert_eq!(&schema["$defs"][name], def, "`$defs/{name}` changed");
    }
    for (key, property) in v1["properties"].as_object().unwrap() {
        if !["version", "extends"].contains(&key.as_str()) {
            assert_eq!(&schema["properties"][key], property, "`{key}` changed");
        }
    }
    let added: BTreeSet<&str> = schema["properties"]
        .as_object()
        .unwrap()
        .keys()
        .filter(|key| v1["properties"].get(key.as_str()).is_none())
        .map(String::as_str)
        .collect();
    assert_eq!(added, BTreeSet::from(["relations", "selectors"]));
    assert_eq!(schema["required"], v1["required"]);
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["properties"]["version"]["const"], 2);
}

#[test]
fn every_reference_resolves_and_every_addition_is_used() {
    let v1 = v1();
    let schema = build(&v1).unwrap();
    let defs = schema["$defs"].as_object().unwrap();
    let mut used = BTreeSet::new();
    references(&schema, &mut used);
    for name in &used {
        assert!(defs.contains_key(name), "`#/$defs/{name}` does not resolve");
    }
    for name in defs.keys() {
        if v1["$defs"].get(name).is_none() {
            assert!(used.contains(name), "`$defs/{name}` is never referenced");
        }
    }
}

#[test]
fn the_draft_reads_version_two_and_the_version_one_schema_does_not() {
    let draft = validator(&draft());
    let v1 = validator(&v1());
    let config = |yaml: &str| -> Value {
        let value: ayaml::Value = alint_core::yaml::from_str(yaml).unwrap();
        serde_json::to_value(value).unwrap()
    };
    let objects = "selectors:\n  docs: { paths: \"docs/**\" }\nrelations:\n  titles:\n    \
                   from: docs\n    read: { regex: '^# (.*)' }\n    fields: { title: value }\n";
    let two = config(&format!("version: 2\n{objects}rules: []\n"));
    assert!(draft.is_valid(&two));
    assert!(!v1.is_valid(&two));
    let one_with_objects = config(&format!("version: 1\n{objects}"));
    assert!(!draft.is_valid(&one_with_objects));
    assert!(!v1.is_valid(&one_with_objects));
    let one = config("version: 1\nrules: []\n");
    assert!(!draft.is_valid(&one));
    assert!(v1.is_valid(&one));
    let aliased = config("version: 2\nextends:\n  - { url: ./base.yml, as: base }\n");
    assert!(draft.is_valid(&aliased));
    assert!(!v1.is_valid(&aliased));
}

#[test]
fn a_definition_the_version_one_schema_has_is_not_replaced() {
    let mut v1 = v1();
    v1["$defs"]["v2_name"] = json!({ "type": "string" });
    let error = build(&v1).expect_err("a collision").to_string();
    assert_eq!(
        error,
        "the version 1 schema already defines `$defs/v2_name`; rename the version 2 definition \
         in xtask/src/gen_schema_v2.rs"
    );
}

#[test]
fn the_draft_follows_the_version_one_id() {
    let mut v1 = v1();
    v1["$id"] = Value::from("https://example.invalid/alint/main/schemas/v1/config.json");
    assert_eq!(
        build(&v1).unwrap()["$id"],
        "https://example.invalid/alint/v0.24.0/schemas/v2/config.json"
    );
    v1["$id"] = Value::from("https://example.invalid/alint/v1.json");
    assert_eq!(
        build(&v1).unwrap_err().to_string(),
        "the version 1 schema's `$id` (https://example.invalid/alint/v1.json) does not end in \
         `/main/schemas/v1/config.json`"
    );
}

/// The draft takes a `constraint` rule, with a named or an inline `input:`,
/// at the top level only; the version 1 schema has no such kind.
#[test]
fn the_draft_takes_a_top_level_constraint_and_the_version_one_schema_none() {
    let draft = validator(&draft());
    let v1 = validator(&v1());
    let config = |yaml: &str| -> Value {
        let value: ayaml::Value = alint_core::yaml::from_str(yaml).unwrap();
        serde_json::to_value(value).unwrap()
    };
    let relation = "from: { paths: \"*/package.json\" }\n    read: { json: \"$.name\" }\n    \
                    fields: { name: value }";
    let rule = "kind: constraint\n    assert: { unique: { key: [name] } }\n    level: error";
    let named = config(&format!(
        "version: 2\nrelations:\n  names:\n    {relation}\nrules:\n  - id: c\n    {rule}\n    \
         input: names\n"
    ));
    assert!(draft.is_valid(&named), "{:?}", draft.validate(&named).err());
    let inline = config(&format!(
        "version: 2\nrules:\n  - id: c\n    {rule}\n    input:\n      {}\n",
        relation.replace("\n    ", "\n      ")
    ));
    assert!(
        draft.is_valid(&inline),
        "{:?}",
        draft.validate(&inline).err()
    );
    let nested = config(
        "version: 2\nrules:\n  - id: each\n    kind: for_each_dir\n    select: \"packages/*\"\n    \
         require:\n      - kind: constraint\n        input: names\n        assert: { unique: { \
         key: [name] } }\n    level: error\n",
    );
    assert!(!draft.is_valid(&nested));
    let one = config(&format!(
        "version: 1\nrules:\n  - id: c\n    {rule}\n    input: names\n"
    ));
    assert!(!v1.is_valid(&one));
    // An input is a name or a relation, nothing else.
    let numbered = config(&format!(
        "version: 2\nrules:\n  - id: c\n    {rule}\n    input: 3\n"
    ));
    assert!(!draft.is_valid(&numbered));
}
