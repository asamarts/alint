//! The draft version 2 schema's `constraint` branch against the loader and the
//! kind's builder (design `v0.23/selectors-and-constraints.md` sections 6 and
//! 11). The branch is derived from the kind's option types
//! (`alint-rules/src/constraint/options.rs`), and these tests hold it to what
//! the builder accepts:
//! - a sweep over the shapes of each family, with each optional key left out,
//!   written, or written as null, in which the schema accepts a config exactly
//!   when it loads and builds;
//! - the refusals the audit's findings name, each refused by both;
//! - what only the builder can check, because it needs the relation (whether a
//!   field exists, what it holds) or compares two values of the config, which
//!   the schema accepts, each with why.

use crate::v2::schema_parity::{has_schema, json_of, schema_errors};

use super::super::*;

/// Three relations: `deps` (a manifest's dependencies, with a `bound:` field
/// that can hold a boolean), `keys` (a locale file's key paths, rooted at the
/// locale) and `names` (a value read from each file).
const RELATIONS: &str = "version: 2\nrelations:\n  deps:\n    from: { paths: \
     \"packages/*/package.json\" }\n    entries: { format: json, at: \"$.dependencies\" }\n    \
     fields: { dependency: entry.key, range: entry.value, at: path, flag: 'bound: $.private' \
     }\n  keys:\n    from: { paths: \"locales/*.json\" }\n    entries: { format: json, at: \
     \"$\", leaves: true }\n    fields: { locale: entry.root, key: entry.path, name: entry.key \
     }\n    transform: { key: [strip-root] }\n  names:\n    from: { paths: \"names/*.json\" \
     }\n    read: { json: $.name }\n    fields: { name: value }\n";

/// A config whose one rule, `c`, reads `input` with `assert`, a flow mapping.
fn config(input: &str, assert: &str) -> String {
    format!(
        "{RELATIONS}rules:\n  - id: c\n    kind: constraint\n    level: error\n    input: \
         {input}\n    assert: {assert}\n"
    )
}

/// Whether `config` loads and every rule of it builds, or the first error.
fn builds(config: &str) -> std::result::Result<(), String> {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(".alint.yml");
    std::fs::write(&path, config).unwrap();
    let loaded = load(&path).map_err(|error| error.to_string())?;
    for spec in &loaded.rules {
        test_registry()
            .build(spec)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// The schema's errors for `config`.
fn refusals(config: &str) -> Vec<String> {
    schema_errors(&json_of(config))
}

/// Check that every `constraint` rule of `config` that builds validates: the
/// whole config against the draft, so the rule's `assert:` and `input:` meet
/// their branch. A config that does not build is the builder's to refuse.
pub(crate) fn assert_valid_when_it_builds(config: &str) {
    if !has_schema() || builds(config).is_err() {
        return;
    }
    assert_valid(config);
}

/// [`assert_valid_when_it_builds`] for `text` that a test has loaded as
/// `loaded`, beside the files it extends: when it declares a `constraint`
/// rule and every rule of it builds, `text` validates. `load_tree` and the
/// row tests' `setup` call it, so every `constraint` config the crate's tests
/// load is checked against the draft.
pub(crate) fn assert_valid_when_loaded_builds(text: &str, loaded: &Config) {
    if !has_schema()
        || !loaded.rules.iter().any(|spec| spec.kind == "constraint")
        || loaded
            .rules
            .iter()
            .any(|spec| test_registry().build(spec).is_err())
    {
        return;
    }
    assert_valid(text);
}

/// `config` validates against the draft.
fn assert_valid(config: &str) {
    let errors = refusals(config);
    assert!(
        errors.is_empty(),
        "a `constraint` rule builds that the draft schema refuses; change \
         alint-rules/src/constraint/options.rs and run `cargo run -p xtask -- gen-schema`:\n\
         {config}\n{errors:#?}"
    );
}

/// `key: value` in a flow mapping when `value` is written; `~` is null.
fn entry(key: &str, value: Option<&str>) -> Option<String> {
    value.map(|value| format!("{key}: {value}"))
}

fn flow(entries: impl IntoIterator<Item = Option<String>>) -> String {
    let written: Vec<String> = entries.into_iter().flatten().collect();
    format!("{{ {} }}", written.join(", "))
}

/// The sweep: `(input, assert)` for each shape of each family.
fn sweep() -> Vec<(&'static str, String)> {
    let mut cases = Vec::new();
    families(&mut cases);
    equal(&mut cases);
    unique(&mut cases);
    set(&mut cases);
    cases
}

/// The family keys, each left out, written or null.
fn families(cases: &mut Vec<(&'static str, String)>) {
    let family = [None, Some("~")];
    for equal in family.iter().copied().chain([Some("{ field: range }")]) {
        for unique in family
            .iter()
            .copied()
            .chain([Some("{ key: [dependency] }")])
        {
            for set in family.iter().copied().chain([Some(
                "{ field: dependency, subset_of: { relation: names, field: name } }",
            )]) {
                cases.push((
                    "deps",
                    flow([
                        entry("equal", equal),
                        entry("unique", unique),
                        entry("set", set),
                    ]),
                ));
            }
        }
    }
}

/// `equal`: `group_by:` and each form of `to:`.
fn equal(cases: &mut Vec<(&'static str, String)>) {
    for group_by in [None, Some("dependency"), Some("~"), Some("'not a name'")] {
        for to in [
            None,
            Some("~"),
            Some("{}"),
            Some("{ row: { dependency: react } }"),
            Some("{ row: { dependency: react, at: x }, }"),
            Some("{ row: { flag: true } }"),
            Some("{ row: {} }"),
            Some("{ row: ~ }"),
            Some("{ row: { dependency: ~ } }"),
            Some("{ row: { 'not a name': x } }"),
            Some("{ field: at }"),
            Some("{ field: ~ }"),
            Some("{ field: 'not a name' }"),
            Some("{ row: { dependency: react }, field: at }"),
            Some("{ row: { dependency: react }, field: ~ }"),
        ] {
            let equal = flow([
                Some("field: range".to_string()),
                entry("group_by", group_by),
                entry("to", to),
            ]);
            cases.push(("deps", format!("{{ equal: {equal} }}")));
        }
    }
}

/// `unique`: `key:` and `determines:`.
fn unique(cases: &mut Vec<(&'static str, String)>) {
    for key in [
        "[dependency]",
        "[dependency, at]",
        "[]",
        "~",
        "[dependency, dependency]",
        "['not a name']",
    ] {
        for determines in [
            None,
            Some("~"),
            Some("[]"),
            Some("[range]"),
            Some("[range, flag]"),
            Some("[range, range]"),
        ] {
            let unique = flow([entry("key", Some(key)), entry("determines", determines)]);
            cases.push(("deps", format!("{{ unique: {unique} }}")));
        }
    }
}

/// `set`: `group_by:`, the target keys, each target's shape, `groups:` and
/// `plurals:`.
fn set(cases: &mut Vec<(&'static str, String)>) {
    let shapes = [
        "{ group: en }",
        "{ relation: names, field: name }",
        "{ relation: names }",
        "{ field: name }",
        "{}",
        "~",
        "{ group: ~ }",
        "{ group: en, field: ~ }",
        "{ group: en, relation: names, field: name }",
    ];
    let targets: [&[&str]; 5] = [
        &[],
        &["equal_to"],
        &["subset_of"],
        &["disjoint_from"],
        &["equal_to", "subset_of"],
    ];
    for group_by in [None, Some("locale"), Some("~")] {
        for keys in targets {
            for shape in if keys.is_empty() {
                &shapes[..1]
            } else {
                &shapes[..]
            } {
                for groups in [None, Some("[]"), Some("[fr]"), Some("~"), Some("[fr, fr]")] {
                    for plurals in [None, Some("exact"), Some("cldr"), Some("~")] {
                        let mut entries =
                            vec![Some("field: key".to_string()), entry("group_by", group_by)];
                        entries.extend(keys.iter().map(|key| entry(key, Some(shape))));
                        entries.push(entry("groups", groups));
                        entries.push(entry("plurals", plurals));
                        cases.push(("keys", format!("{{ set: {} }}", flow(entries))));
                    }
                }
            }
        }
    }
}

/// The draft's rule schema (`$defs/rule`, which every entry of `rules:`
/// meets), to validate one rule at a time.
fn rule_validator() -> jsonschema::Validator {
    let draft = crate::v2::schema_parity::schema().expect("checked by `has_schema`");
    let mut rule = serde_json::Map::new();
    rule.insert("$schema".into(), draft["$schema"].clone());
    rule.insert("$ref".into(), "#/$defs/rule".into());
    rule.insert("$defs".into(), draft["$defs"].clone());
    jsonschema::validator_for(&serde_json::Value::Object(rule)).expect("the rule schema compiles")
}

/// The builder's and the schema's verdicts on each of `cases`, written as the
/// rules `c0`, `c1`, ... of one config: whether each builds, and whether the
/// schema accepts each rule (with its errors when it does not). The config
/// must load, so a case the loader refuses before any rule builds does not
/// belong here.
fn verdicts(cases: &[(&str, String)]) -> Vec<(std::result::Result<(), String>, Vec<String>)> {
    let mut config = format!("{RELATIONS}rules:\n");
    for (index, (input, assert)) in cases.iter().enumerate() {
        let _ = write!(
            config,
            "  - id: c{index}\n    kind: constraint\n    level: error\n    input: {input}\n    \
             assert: {assert}\n"
        );
    }
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(".alint.yml");
    std::fs::write(&path, &config).unwrap();
    let loaded = load(&path).unwrap_or_else(|error| panic!("the sweep loads: {error}"));
    assert_eq!(loaded.rules.len(), cases.len());
    let document = json_of(&config);
    let rules = document["rules"].as_array().expect("the sweep's rules");
    let validator = rule_validator();
    loaded
        .rules
        .iter()
        .zip(rules)
        .map(|(spec, rule)| {
            let built = test_registry()
                .build(spec)
                .map(drop)
                .map_err(|error| error.to_string());
            // The errors only where the two disagree: listing them is the
            // slow part of a refusal.
            let errors = if validator.is_valid(rule) {
                Vec::new()
            } else if built.is_err() {
                vec!["refused".to_string()]
            } else {
                validator
                    .iter_errors(rule)
                    .map(|error| format!("{error} at `{}`", error.instance_path()))
                    .collect()
            };
            (built, errors)
        })
        .collect()
}

#[test]
fn the_schema_accepts_a_constraint_exactly_when_it_builds() {
    if !has_schema() {
        return;
    }
    let cases = sweep();
    let mut disagreements = Vec::new();
    let mut built = 0;
    for ((_, assert), (loader, schema)) in cases.iter().zip(verdicts(&cases)) {
        built += usize::from(loader.is_ok());
        if loader.is_ok() != schema.is_empty() {
            disagreements.push(format!(
                "assert: {assert}\n  builder: {}\n  schema: {}",
                loader.err().unwrap_or_else(|| "builds".into()),
                schema.first().map_or("accepts", String::as_str)
            ));
        }
    }
    assert!(
        disagreements.is_empty(),
        "the draft schema and the builder disagree on {} of {} configs:\n{}",
        disagreements.len(),
        cases.len(),
        disagreements.join("\n")
    );
    // The sweep holds both verdicts in numbers, so it compares something.
    assert!(
        built > 50 && cases.len() - built > 500,
        "{built} of {}",
        cases.len()
    );
}

/// The audit's findings, each a config both the builder and the schema
/// refuse: `(finding, input, assert)`.
fn refused_by_both() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        // A: a family key or a `to:` form written as null counts as written.
        ("A1", "deps", "{ unique: { key: [dependency] }, equal: }"),
        (
            "A2",
            "deps",
            "{ equal: { field: range, to: { field: at, row: } } }",
        ),
        // B: `set` with no other set, or two.
        ("B", "keys", "{ set: { field: key, group_by: locale } }"),
        (
            "B",
            "keys",
            "{ set: { field: key, group_by: locale, equal_to: { group: en }, disjoint_from: { \
             group: de } } }",
        ),
        // C: a target of no shape, or of two.
        (
            "C",
            "keys",
            "{ set: { field: key, group_by: locale, equal_to: {} } }",
        ),
        (
            "C",
            "keys",
            "{ set: { field: key, subset_of: { relation: names } } }",
        ),
        (
            "C",
            "keys",
            "{ set: { field: key, group_by: locale, subset_of: { group: en, relation: names } } }",
        ),
        // D: a relation that is not a name or `alias.name`.
        (
            "D",
            "keys",
            "{ set: { field: key, subset_of: { relation: a.b.c, field: name } } }",
        ),
        // E: a field name that is not a name.
        ("E", "deps", "{ equal: { field: 'range!' } }"),
        ("E", "deps", "{ unique: { key: ['{{x}}'] } }"),
        // F: counts and repeats.
        (
            "F",
            "deps",
            "{ unique: { key: [dependency], determines: [] } }",
        ),
        ("F", "deps", "{ unique: { key: [range, range] } }"),
        (
            "F",
            "deps",
            "{ unique: { key: [dependency], determines: [range, range] } }",
        ),
        (
            "F",
            "keys",
            "{ set: { field: key, group_by: locale, equal_to: { group: en }, groups: [fr, fr] } \
             }",
        ),
        ("F", "deps", "{ equal: { field: range, to: { row: {} } } }"),
        // G: options that need, or exclude, another.
        (
            "G",
            "deps",
            "{ equal: { field: range, group_by: dependency, to: { field: at } } }",
        ),
        (
            "G",
            "keys",
            "{ set: { field: key, equal_to: { group: en } } }",
        ),
        (
            "G",
            "keys",
            "{ set: { field: key, group_by: locale, subset_of: { group: en }, groups: [fr] } }",
        ),
        (
            "G",
            "keys",
            "{ set: { field: key, equal_to: { relation: names, field: name }, groups: [fr] } }",
        ),
        (
            "G",
            "keys",
            "{ set: { field: key, group_by: locale, equal_to: { relation: names, field: name }, \
             plurals: cldr } }",
        ),
    ]
}

#[test]
fn the_audits_shapes_are_refused_by_both() {
    if !has_schema() {
        return;
    }
    for (finding, input, assert) in refused_by_both() {
        let config = config(input, assert);
        assert!(
            builds(&config).is_err(),
            "{finding}: the builder accepts\n{config}"
        );
        assert!(
            !refusals(&config).is_empty(),
            "{finding}: the draft schema accepts\n{config}"
        );
    }
}

/// What the builder alone refuses, with why the schema cannot: `(why, input,
/// assert)`. The version 2 docs page says the schema leaves these to alint.
fn builder_only() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (
            "a field the relation does not have",
            "deps",
            "{ equal: { field: version } }",
        ),
        (
            "a relation no config declares",
            "keys",
            "{ set: { field: key, subset_of: { relation: missing, field: name } } }",
        ),
        (
            "`group_by:` naming the compared field",
            "deps",
            "{ equal: { field: range, group_by: range } }",
        ),
        (
            "`to: { field: }` naming the compared field",
            "deps",
            "{ equal: { field: range, to: { field: range } } }",
        ),
        (
            "`determines:` naming a field of the key",
            "deps",
            "{ unique: { key: [dependency], determines: [dependency] } }",
        ),
        (
            "a boolean for a field that holds strings",
            "deps",
            "{ equal: { field: range, to: { row: { dependency: true } } } }",
        ),
        (
            "`groups:` naming the reference group",
            "keys",
            "{ set: { field: key, group_by: locale, equal_to: { group: en }, groups: [en] } }",
        ),
        (
            "`plurals: cldr` on a field that is not an `entry.path`",
            "keys",
            "{ set: { field: name, group_by: locale, equal_to: { group: en }, plurals: cldr } }",
        ),
    ]
}

#[test]
fn the_schema_leaves_what_needs_the_relation_to_the_builder() {
    if !has_schema() {
        return;
    }
    for (why, input, assert) in builder_only() {
        let config = config(input, assert);
        assert!(
            builds(&config).is_err(),
            "{why}: the builder accepts\n{config}"
        );
        let errors = refusals(&config);
        assert!(
            errors.is_empty(),
            "{why}: the draft schema now refuses this; move it to `refused_by_both` and the docs \
             page's list\n{config}\n{errors:?}"
        );
    }
}

#[test]
fn every_constraint_the_policies_and_fixtures_write_validates() {
    if !has_schema() {
        return;
    }
    // Each family in each form, as the docs and the tests write them.
    for (input, assert) in [
        (
            "deps",
            "{ unique: { key: [dependency], determines: [range] } }",
        ),
        ("deps", "{ equal: { field: range, group_by: dependency } }"),
        (
            "deps",
            "{ equal: { field: range, to: { row: { dependency: react, at: x } } } }",
        ),
        ("deps", "{ equal: { field: range, to: { field: at } } }"),
        (
            "keys",
            "{ set: { field: key, group_by: locale, equal_to: { group: en }, groups: [fr, de], \
             plurals: cldr } }",
        ),
        (
            "keys",
            "{ set: { field: key, disjoint_from: { relation: names, field: name } } }",
        ),
        (
            "{ from: { paths: x.json }, read: { json: $.v }, fields: { v: value } }",
            "{ unique: { key: [v] } }",
        ),
    ] {
        let config = config(input, assert);
        assert!(builds(&config).is_ok(), "{config}");
        assert_valid_when_it_builds(&config);
    }
}
