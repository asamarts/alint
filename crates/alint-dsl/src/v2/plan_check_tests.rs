//! Unit tests of the plan check (`v2/plan_check.rs`), through resolution as
//! the loader runs it.

use std::fmt::Write as _;
use std::path::PathBuf;

use alint_core::ConfigSourceClass;
use ayaml::Value;

use super::*;
use crate::v2::{Objects, SourceId};

/// The plan check's verdict on a top-level config declaring `yaml`'s
/// `selectors:` and `relations:`.
fn check_root(yaml: &str) -> alint_core::Result<Vec<RelationId>> {
    let document: Value = alint_core::yaml::from_str(yaml).expect("parse");
    let v2_keys: Vec<(String, Value)> = document
        .as_mapping()
        .expect("a mapping")
        .iter()
        .map(|(key, value)| (key.as_str().expect("a key").to_string(), value.clone()))
        .collect();
    let id = SourceId {
        display: ".alint.yml".into(),
        path: PathBuf::from("/repo/.alint.yml"),
        class: ConfigSourceClass::Root,
    };
    let objects = Objects {
        sources: vec![crate::v2::source_objects(id, &v2_keys, &[]).expect("decode")],
    };
    let plan = crate::v2::resolve(&objects)?.plan;
    check(&plan)
}

fn problem(yaml: &str) -> String {
    check_root(yaml)
        .expect_err("expected a plan error")
        .to_string()
}

/// Relation `a`: one field, `name`, read from JSON.
const A: &str = "  a:\n    from: { paths: a.json }\n    read: { json: $.name }\n    \
                 fields: { name: value }\n";

#[test]
fn a_selector_compiles_as_a_rule_scope_does() {
    assert_eq!(
        problem("selectors:\n  bad: { paths: 'a/[' }\n"),
        format!(
            ".alint.yml: `selectors.bad.paths`: {}",
            alint_core::Scope::from_paths_spec(&alint_core::PathsSpec::Single("a/[".into()))
                .unwrap_err()
        )
    );
    assert_eq!(
        problem(
            "relations:\n  r:\n    from: { paths: '*.json', scope_filter: { has_ancestor: \
             '../x' } }\n    read: { json: $.a }\n    fields: { a: value }\n"
        ),
        ".alint.yml: `relations.r.from.scope_filter`: scope_filter.has_ancestor name \"../x\" \
         must be a basename - no path separators.\n  hint: to match files inside a specific \
         subtree, use `paths:` on the rule's main scope (e.g. `paths: \"airflow-core/**/*.py\"`); \
         to match files in any subtree that has this manifest, use the basename only (e.g. \
         `has_ancestor: \"x\"`)."
    );
    assert_eq!(
        problem(
            "selectors:\n  s:\n    paths: '*.json'\n    scope_filter:\n      \
             include_manifest_paths: { source: .git/config, extract: { json: $.x } }\n"
        ),
        ".alint.yml: `selectors.s.scope_filter`: scope_filter manifest `source` .git/config is \
         in a Git directory; rules never read there"
    );
    assert_eq!(
        problem(
            "selectors:\n  s:\n    paths: '*.json'\n    scope_filter:\n      \
             exclude_manifest_paths: { source: ../outside.json, extract: { json: $.x } }\n"
        ),
        ".alint.yml: `selectors.s.scope_filter`: scope_filter manifest `source:` must be a \
         repo-root-relative path, got \"../outside.json\""
    );
}

#[test]
fn a_cycle_is_refused_and_named() {
    assert_eq!(
        problem(
            "relations:\n  a:\n    join: { left: b, right: c, on: { x: x }, unique: right }\n    \
             fields: { x: left.x }\n  b:\n    join: { left: a, right: c, on: { x: x }, unique: \
             right }\n    fields: { x: left.x }\n  c:\n    from: { paths: c.json }\n    \
             read: { json: $.x }\n    fields: { x: value }\n"
        ),
        ".alint.yml: `relations.a.join.left`: relation `a` reads itself: `a` -> `b` -> `a` (a \
         join's sides and a filter's `{ relation, field }` references cannot form a cycle); \
         remove one of these references"
    );
    assert_eq!(
        problem(&format!(
            "relations:\n{A}  r:\n    from: {{ paths: r.json }}\n    read: {{ json: $.x }}\n    \
             fields: {{ x: value }}\n    where:\n      x: {{ in: [y, {{ relation: r, field: x }}] \
             }}\n"
        )),
        ".alint.yml: `relations.r.where.x.in[1].relation`: relation `r` reads itself: `r` -> \
         `r` (a join's sides and a filter's `{ relation, field }` references cannot form a \
         cycle); remove one of these references"
    );
}

#[test]
fn relations_are_ordered_after_what_they_read() {
    let order = check_root(&format!(
        "relations:\n  j:\n    join: {{ left: a, right: b, on: {{ name: name }}, unique: right \
         }}\n    fields: {{ name: left.name }}\n{A}  b:\n    from: {{ paths: b.json }}\n    \
         read: {{ json: $.name }}\n    fields: {{ name: value }}\n"
    ))
    .expect("checks");
    assert_eq!(order, vec![RelationId(1), RelationId(2), RelationId(0)]);
}

#[test]
fn a_long_chain_of_joins_is_ordered_without_recursion() {
    let mut yaml = String::from(
        "relations:\n  r0:\n    from: { paths: a.json }\n    read: { json: $.x }\n    \
         fields: { x: value }\n",
    );
    for index in 1..2_000 {
        let previous = index - 1;
        write!(
            yaml,
            "  r{index}:\n    join: {{ left: r{previous}, right: r0, on: {{ x: x }}, unique: \
             right }}\n    fields: {{ x: left.x }}\n"
        )
        .expect("writing to a String cannot fail");
    }
    assert_eq!(check_root(&yaml).expect("checks").len(), 2_000);
}

#[test]
fn every_part_of_a_union_binds_every_field() {
    assert_eq!(
        problem(
            "relations:\n  r:\n    union:\n      - { from: { paths: a.json }, entries: { format: \
             json, at: $ } }\n      - { from: { paths: b.txt }, read: { lines: {} } }\n    \
             fields: { name: entry.key }\n"
        ),
        ".alint.yml: `relations.r.fields.name`: every part of a union binds every field, and \
         `union[1]` cannot bind `entry.key`: `entry.*` sources need `entries:`; a `read:` row \
         has `value`"
    );
    assert_eq!(
        problem(
            "relations:\n  r:\n    union:\n      - { from: { paths: b.txt }, read: { lines: {} } \
             }\n      - { from: { paths: a.json }, entries: { format: json, at: $ } }\n    \
             fields: { file: path, line: value }\n"
        ),
        ".alint.yml: `relations.r.fields.line`: every part of a union binds every field, and \
         `union[1]` cannot bind `value`: `value` is a `read:` row's value; an `entries:` row has \
         `entry.value`"
    );
    check_root(
        "relations:\n  r:\n    union:\n      - { from: { paths: b.txt }, read: { lines: {} } }\n      \
         - { from: { paths: c.txt }, read: { whole_file: {} } }\n    fields: { file: path, line: \
         value }\n",
    )
    .expect("both parts bind `path` and `value`");
}

/// A join of `a` with `b` (one field, `id`), with `rest` appended.
fn join(on: &str, fields: &str) -> String {
    format!(
        "relations:\n{A}  b:\n    from: {{ paths: b.json }}\n    read: {{ json: $.id }}\n    \
         fields: {{ id: value }}\n  j:\n    join: {{ left: a, right: b, on: {{ {on} }}, unique: \
         right }}\n    fields: {{ {fields} }}\n"
    )
}

#[test]
fn a_join_names_fields_of_its_sides() {
    check_root(&join("name: id", "name: left.name, id: right.id")).expect("checks");
    assert_eq!(
        problem(&join("nmae: id", "name: left.name")),
        ".alint.yml: `relations.j.join.on.nmae`: `nmae` is not a field of `a`, the join's left \
         side; its fields are `name`"
    );
    assert_eq!(
        problem(&join("name: name", "name: left.name")),
        ".alint.yml: `relations.j.join.on.name`: `name` is not a field of `b`, the join's right \
         side; its fields are `id`"
    );
    assert_eq!(
        problem(&join("name: id", "name: left.id")),
        ".alint.yml: `relations.j.fields.name`: `left.id` names no field of `a`, the join's left \
         side; its fields are `name`"
    );
    assert_eq!(
        problem(&join("name: id", "id: right.ids")),
        ".alint.yml: `relations.j.fields.id`: `right.ids` names no field of `b`, the join's right \
         side; its fields are `id`"
    );
}

#[test]
fn a_transform_and_a_filter_name_fields_of_their_relation() {
    let relation = |rest: &str| {
        format!(
            "relations:\n  r:\n    from: {{ paths: '*.json' }}\n    entries: {{ format: json, \
             at: $ }}\n    fields: {{ name: entry.key, version: entry.value }}\n{rest}"
        )
    };
    assert_eq!(
        problem(&relation("    transform: { nmae: [trim] }\n")),
        ".alint.yml: `relations.r.transform.nmae`: `nmae` is not a field of relation `r`; its \
         fields are `name`, `version`"
    );
    assert_eq!(
        problem(&relation("    where: { nmae: { matches: x } }\n")),
        ".alint.yml: `relations.r.where.nmae`: `nmae` is not a field of relation `r`; its fields \
         are `name`, `version`"
    );
    let many = "relations:\n  r:\n    from: { paths: '*.json' }\n    entries: { format: json, \
                at: $ }\n    fields: { one: entry.key, two: entry.key, three: entry.key, four: \
                entry.key, five: entry.key, six: entry.key, seven: entry.key }\n    where: { \
                thre: { matches: x } }\n";
    assert_eq!(
        problem(many),
        ".alint.yml: `relations.r.where.thre`: `thre` is not a field of relation `r`; did you \
         mean `three`?"
    );
}

#[test]
fn a_membership_reference_names_a_field_of_the_relation() {
    assert_eq!(
        problem(&format!(
            "relations:\n{A}  r:\n    from: {{ paths: r.json }}\n    read: {{ json: $.x }}\n    \
             fields: {{ x: value }}\n    where:\n      x: {{ not_in: [{{ relation: a, field: \
             names }}] }}\n"
        )),
        ".alint.yml: `relations.r.where.x.not_in[0].field`: `names` is not a field of `a`; its \
         fields are `name`"
    );
}

/// Relation `r` with `fields` (a flow map's body) and `transform:`.
fn transformed(source: &str, transforms: &str) -> String {
    format!(
        "relations:\n  r:\n    from: {{ paths: '*.json' }}\n    entries: {{ format: json, at: \
         $, leaves: true }}\n    fields: {{ f: {source} }}\n    transform: {{ f: [{transforms}] \
         }}\n"
    )
}

#[test]
fn strip_root_applies_first_to_an_entry_path() {
    check_root(&transformed("entry.path", "strip-root, lower")).expect("checks");
    assert_eq!(
        problem(&transformed("entry.key", "strip-root")),
        ".alint.yml: `relations.r.transform.f[0]`: `strip-root` drops the first segment of a key \
         path, so it applies only to a field bound to `entry.path`; `f` is bound to `entry.key`"
    );
    assert_eq!(
        problem(&transformed("entry.path", "lower, strip-root")),
        ".alint.yml: `relations.r.transform.f[1]`: `strip-root` reads the segments of the key \
         path `entry.path` binds, so it comes first among a field's transforms, and once; \
         `lower` before it leaves a string"
    );
    assert_eq!(
        problem(&transformed("entry.path", "strip-root, strip-root")),
        ".alint.yml: `relations.r.transform.f[1]`: `strip-root` reads the segments of the key \
         path `entry.path` binds, so it comes first among a field's transforms, and once; \
         `strip-root` before it leaves a string"
    );
}

#[test]
fn a_version_transform_applies_to_a_field_that_may_hold_a_version() {
    check_root(&transformed("entry.value", "trim, semver-major")).expect("checks");
    check_root(&transformed("basename", "semver-minor")).expect("a file name may be a version");
    for (source, transform) in [
        ("path", "semver-major"),
        ("dir", "semver-minor"),
        ("ext", "semver-major"),
        ("entry.path", "semver-minor"),
    ] {
        assert_eq!(
            problem(&transformed(source, transform)),
            format!(
                ".alint.yml: `relations.r.transform.f[0]`: `{transform}` reads a version, and \
                 `f` is bound to `{source}`, which never holds one; apply it to the field that \
                 holds the version"
            )
        );
    }
}

/// Relation `r`, field `f` from `source`, filtered by `in: [literal]`.
fn listing(source: &str, literal: &str) -> String {
    format!(
        "relations:\n  r:\n    from: {{ paths: '*.json' }}\n    entries: {{ format: json, at: \
         $ }}\n    fields: {{ f: {source} }}\n    where: {{ f: {{ in: [{literal}] }} }}\n"
    )
}

#[test]
fn a_literal_that_can_never_match_is_refused() {
    check_root(&listing("entry.key", "x")).expect("a string");
    check_root(&listing("entry.value", "true")).expect("a value may be a boolean");
    check_root(&listing("'bound: $.private'", "true")).expect("a bound value may be a boolean");
    assert_eq!(
        problem(&listing("entry.value", "~")),
        ".alint.yml: `relations.r.where.f.in[0]`: null is in no set, so it never matches `in` \
         and never fails `not_in`; remove it"
    );
    assert_eq!(
        problem(&listing("entry.value", "1.10")),
        ".alint.yml: `relations.r.where.f.in[0]`: a number cannot be listed in this build: a \
         value compares by the text it is written with (`1.10` is not `1.1`), which the config's \
         YAML does not keep; write it as a string (`'1.10'`) to match a string value"
    );
    assert_eq!(
        problem(&listing("entry.key", "true")),
        ".alint.yml: `relations.r.where.f.in[0]`: `f` holds strings, and a boolean never equals \
         a string; quote it (`'true'`) to match that text"
    );
    // After a transform, a value is a string.
    assert_eq!(
        problem(
            "relations:\n  r:\n    from: { paths: '*.json' }\n    entries: { format: json, at: \
             $ }\n    fields: { f: entry.value }\n    transform: { f: [trim] }\n    where: { f: \
             { not_in: [false] } }\n"
        ),
        ".alint.yml: `relations.r.where.f.not_in[0]`: `f` holds strings, and a boolean never \
         equals a string; quote it (`'false'`) to match that text"
    );
    // Through a join, to the side's field.
    assert_eq!(
        problem(&format!(
            "relations:\n{A}  j:\n    join: {{ left: a, right: a, on: {{ name: name }}, unique: \
             right }}\n    fields: {{ n: left.name }}\n    where: {{ n: {{ in: [true] }} }}\n"
        )),
        ".alint.yml: `relations.j.where.n.in[0]`: `n` holds strings, and a boolean never equals \
         a string; quote it (`'true'`) to match that text"
    );
}

/// A plan of one top-level source holding the selector `docs` and `relations`.
fn plan_of(relations: Vec<(&str, RelationDef)>) -> Plan {
    use alint_core::relation::plan::{
        Capabilities, NamedRelation, NamedSelector, PlanSource, SourceIndex,
    };
    Plan {
        sources: vec![PlanSource {
            display: ".alint.yml".into(),
            path: PathBuf::from("/repo/.alint.yml"),
            class: ConfigSourceClass::Root,
            aliases: Vec::new(),
        }],
        selectors: vec![NamedSelector {
            name: "docs".into(),
            source: SourceIndex(0),
            selector: SelectorDef {
                paths: alint_core::PathsSpec::Single("*.json".into()),
                scope_filter: None,
            },
            capabilities: Capabilities::default(),
            reads_environment: false,
        }],
        relations: relations
            .into_iter()
            .map(|(name, relation)| NamedRelation {
                name: name.into(),
                source: SourceIndex(0),
                relation,
                capabilities: Capabilities::default(),
                reads_environment: false,
            })
            .collect(),
        rules: Vec::new(),
    }
}

/// A part reading the selector `docs` with `read: { json: $.a }`.
fn json_part() -> alint_core::relation::plan::Part {
    alint_core::relation::plan::Part {
        from: SelectorRef::Named(Reached {
            id: alint_core::relation::plan::SelectorId(0),
            alias: None,
        }),
        reader: Reader::Read {
            read: alint_core::relation::plan::Read::Json("$.a".into()),
            optional: false,
        },
    }
}

/// `count` fields named `f0`, `f1`, ..., each bound to `value`.
fn value_fields(count: usize) -> Vec<(String, FieldDef)> {
    (0..count)
        .map(|index| {
            let field = FieldDef {
                source: FieldSource::Value,
                sensitive: false,
            };
            (format!("f{index}"), field)
        })
        .collect()
}

/// Each name a check looks up is one lookup: a relation of a hundred
/// thousand fields, each transformed and filtered, and a join over it that
/// copies every field, check in a moment, where a scan of the fields per
/// name took minutes.
#[test]
fn a_relation_of_many_fields_checks_in_time_linear_in_them() {
    let outcome = crate::tests::on_thread(8 << 20, std::time::Duration::from_secs(20), || {
        let fields = value_fields(100_000);
        let wide = RelationDef {
            rows: Rows::Part(json_part()),
            transforms: fields
                .iter()
                .map(|(name, _)| (name.clone(), vec![Transform::Trim]))
                .collect(),
            filters: fields
                .iter()
                .map(|(name, _)| {
                    let member = Member::Literal(alint_core::relation::Value::String("x".into()));
                    (name.clone(), vec![Predicate::In(vec![member])])
                })
                .collect(),
            fields,
        };
        let wide_side = || Reached {
            id: RelationId(0),
            alias: None,
        };
        let joined = RelationDef {
            rows: Rows::Join(alint_core::relation::plan::JoinDef {
                left: wide_side(),
                right: wide_side(),
                on: vec![("f0".into(), "f0".into())],
                unique: JoinSide::Right,
            }),
            fields: (0..100_000)
                .map(|index| {
                    let field = FieldDef {
                        source: FieldSource::Side(JoinSide::Left, format!("f{index}")),
                        sensitive: false,
                    };
                    (format!("g{index}"), field)
                })
                .collect(),
            transforms: Vec::new(),
            filters: Vec::new(),
        };
        check(&plan_of(vec![("wide", wide), ("joined", joined)])).map_err(|e| e.to_string())
    });
    assert_eq!(outcome, Ok(vec![RelationId(0), RelationId(1)]));
}

/// A union's parts are weighed against its fields by what each part reads,
/// not one by one: fifty thousand parts binding fifty thousand fields check
/// in a moment, where every part was checked against every field.
#[test]
fn a_union_of_many_parts_checks_in_time_linear_in_them() {
    let outcome = crate::tests::on_thread(8 << 20, std::time::Duration::from_secs(20), || {
        let union = RelationDef {
            rows: Rows::Union((0..50_000).map(|_| json_part()).collect()),
            fields: value_fields(50_000),
            transforms: Vec::new(),
            filters: Vec::new(),
        };
        check(&plan_of(vec![("union", union)])).map_err(|e| e.to_string())
    });
    assert_eq!(outcome, Ok(vec![RelationId(0)]));
}

/// What a field holds is found once per field of a join chain: a boolean
/// literal in each of three thousand filters on the end of a chain of three
/// hundred joins walks the chain once, where each literal walked all of it.
#[test]
fn a_join_chain_is_walked_once_for_every_literal_on_it() {
    let (joins, literals) = (300, 3000);
    let mut yaml = String::from(
        "relations:\n  r0:\n    from: { paths: a.json }\n    entries: { format: json, at: $ \
         }\n    fields: { x: entry.value }\n",
    );
    let mut previous = "r0".to_string();
    for index in 1..=joins {
        write!(
            yaml,
            "  j{index}:\n    join: {{ left: {previous}, right: r0, on: {{ x: x }}, unique: \
             right }}\n    fields: {{ x: left.x }}\n"
        )
        .unwrap();
        previous = format!("j{index}");
    }
    write!(
        yaml,
        "  t:\n    join: {{ left: {previous}, right: r0, on: {{ x: x }}, unique: right }}\n    \
         fields: {{ x: left.x }}\n    where:\n      x:\n        in:\n"
    )
    .unwrap();
    for _ in 0..literals {
        yaml.push_str("          - true\n");
    }
    HOLDS_STEPS.with(|steps| steps.set(0));
    assert!(check_root(&yaml).is_ok());
    let steps = HOLDS_STEPS.with(std::cell::Cell::get);
    // `t.x`, then each join's `x`, then `r0.x`: once each, in each of the two
    // checks `check_root` runs (the resolve's, then its own).
    assert_eq!(steps, 2 * (joins + 2));
}
