//! Unit tests of the selector, relation and alias decode (`v2/decode.rs`).

use std::path::PathBuf;

use alint_core::ConfigSourceClass;
use ayaml::Value;

use super::*;
use crate::v2::ExtendsEntry;

/// Decode the `selectors:`, `relations:` and `extends:` of `yaml` as the
/// loader does for a version 2 `.alint.yml`.
fn decode(yaml: &str) -> Result<SourceObjects> {
    let document: Value = alint_core::yaml::from_str(yaml).expect("parse");
    let mut v2_keys = Vec::new();
    let mut extends: Vec<ExtendsEntry> = Vec::new();
    for (key, value) in document.as_mapping().expect("a mapping") {
        match key.as_str() {
            Some(name @ ("selectors" | "relations")) => {
                v2_keys.push((name.to_string(), value.clone()));
            }
            Some("extends") => {
                extends = crate::core_yaml::decode(value.clone()).expect("extends");
            }
            _ => {}
        }
    }
    let source = SourceId {
        display: ".alint.yml".into(),
        path: PathBuf::from("/repo/.alint.yml"),
        class: ConfigSourceClass::Root,
    };
    source_objects(source, &v2_keys, &extends)
}

fn error(yaml: &str) -> String {
    decode(yaml).expect_err("expected an error").to_string()
}

/// `error(yaml)` without the `.alint.yml: ` prefix every decode error has.
fn problem(yaml: &str) -> String {
    let message = error(yaml);
    message
        .strip_prefix(".alint.yml: ")
        .unwrap_or_else(|| panic!("no source in {message:?}"))
        .to_string()
}

fn relation<'o>(objects: &'o SourceObjects, name: &str) -> &'o Relation {
    &objects
        .relations
        .iter()
        .find(|(held, _)| held == name)
        .unwrap_or_else(|| panic!("no relation {name}"))
        .1
}

fn named(name: &str) -> Reference {
    Reference {
        alias: None,
        name: name.into(),
    }
}

/// Every form of selector, relation and alias, for the tests below.
const EVERY_FORM: &str = r##"
extends:
  - url: ./base.yml
    as: base
selectors:
  manifests:
    paths: "**/package.json"
    scope_filter: { has_ancestor: package.json }
relations:
  deps:
    from: manifests
    entries: { format: json, at: ["$.dependencies", "$.devDependencies"], leaves: true }
    fields:
      package: dir
      name: entry.key
      version: { source: entry.value, sensitive: true }
    transform: { version: [trim, semver-major] }
    where:
      name: { not_in: [left-pad, 3, null, { relation: base.banned, field: name }] }
  versions:
    from: { paths: "**/Cargo.toml" }
    read: { toml: "$.package.version" }
    optional: true
    fields: { version: value, owner: "bound: $.package.name" }
  sources:
    union:
      - { from: manifests, read: { regex: '"name": "(.*)"' } }
      - { from: manifests, read: { lines: { comment: "#" } }, optional: true }
      - { from: manifests, read: { whole_file: {} } }
    fields: { text: value }
  paired:
    join: { left: deps, right: base.versions, on: { name: version }, unique: right }
    fields: { name: left.name, version: right.version }
    where: { name: { matches: "^a", not_matches: "z$" } }
"##;

/// [`EVERY_FORM`], decoded.
fn every_form() -> SourceObjects {
    decode(EVERY_FORM).expect("decode")
}

#[test]
fn every_form_keeps_its_source_selectors_and_aliases() {
    let objects = every_form();
    assert_eq!(objects.source.display, ".alint.yml");
    assert_eq!(objects.source.class, ConfigSourceClass::Root);
    assert_eq!(
        objects.aliases,
        vec![(
            "base".to_string(),
            Alias {
                url: "./base.yml".into(),
                entry: 0,
                target: None
            }
        )]
    );
    let names: Vec<&str> = objects.selectors.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["manifests"]);
    assert!(objects.selectors[0].1.scope_filter.is_some());
    let names: Vec<&str> = objects.relations.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["deps", "versions", "sources", "paired"]);
}

#[test]
fn entries_rows_decode_with_fields_transforms_and_filters() {
    let objects = every_form();
    let deps = relation(&objects, "deps");
    let Rows::Part(Part {
        from: SelectorRef::Named(from),
        reader: Reader::Entries(entries),
    }) = &deps.rows
    else {
        panic!("{:?}", deps.rows);
    };
    assert_eq!(from, &named("manifests"));
    assert_eq!(
        entries,
        &Entries {
            format: EntriesFormat::Json,
            at: vec!["$.dependencies".into(), "$.devDependencies".into()],
            leaves: true,
        }
    );
    assert_eq!(
        deps.fields,
        vec![
            (
                "package".into(),
                Field {
                    source: FieldSource::Path(PathToken::Dir),
                    sensitive: false
                }
            ),
            (
                "name".into(),
                Field {
                    source: FieldSource::EntryKey,
                    sensitive: false
                }
            ),
            (
                "version".into(),
                Field {
                    source: FieldSource::EntryValue,
                    sensitive: true
                }
            ),
        ]
    );
    assert_eq!(
        deps.transforms,
        vec![(
            "version".into(),
            vec![Transform::Trim, Transform::SemverMajor]
        )]
    );
    let [(field, predicates)] = deps.filters.as_slice() else {
        panic!("{:?}", deps.filters);
    };
    assert_eq!(field, "name");
    let [Predicate::NotIn(members)] = predicates.as_slice() else {
        panic!("{predicates:?}");
    };
    assert_eq!(members.len(), 4);
    assert_eq!(
        members[0],
        Member::Literal(Value::String("left-pad".into()))
    );
    assert_eq!(members[2], Member::Literal(Value::Null));
    assert_eq!(
        members[3],
        Member::Relation {
            relation: Reference {
                alias: Some("base".into()),
                name: "banned".into()
            },
            field: "name".into()
        }
    );
}

#[test]
fn read_rows_decode_with_an_inline_selector() {
    let objects = every_form();
    let versions = relation(&objects, "versions");
    let Rows::Part(Part {
        from: SelectorRef::Inline(_),
        reader: Reader::Read { read, optional },
    }) = &versions.rows
    else {
        panic!("{:?}", versions.rows);
    };
    assert_eq!(read, &Read::Toml("$.package.version".into()));
    assert!(*optional);
    assert_eq!(
        versions.fields[1].1.source,
        FieldSource::Bound("$.package.name".into())
    );
}

#[test]
fn a_union_decodes_each_part() {
    let objects = every_form();
    let Rows::Union(parts) = &relation(&objects, "sources").rows else {
        panic!("not a union");
    };
    let reads: Vec<(&Read, bool)> = parts
        .iter()
        .map(|part| match &part.reader {
            Reader::Read { read, optional } => (read, *optional),
            Reader::Entries(_) => panic!("entries"),
        })
        .collect();
    assert_eq!(
        reads,
        vec![
            (&Read::Regex("\"name\": \"(.*)\"".into()), false),
            (
                &Read::Lines {
                    comment: Some("#".into())
                },
                true
            ),
            (&Read::WholeFile, false),
        ]
    );
}

#[test]
fn a_join_decodes_both_sides() {
    let objects = every_form();
    let paired = relation(&objects, "paired");
    let Rows::Join(join) = &paired.rows else {
        panic!("not a join");
    };
    assert_eq!(join.left, named("deps"));
    assert_eq!(
        join.right,
        Reference {
            alias: Some("base".into()),
            name: "versions".into()
        }
    );
    assert_eq!(join.on, vec![("name".to_string(), "version".to_string())]);
    assert_eq!(join.unique, JoinSide::Right);
    assert_eq!(
        paired.fields[1].1.source,
        FieldSource::Side(JoinSide::Right, "version".into())
    );
    assert_eq!(
        paired.filters[0].1,
        vec![
            Predicate::Matches("^a".into()),
            Predicate::NotMatches("z$".into())
        ]
    );
}

#[test]
fn a_config_without_objects_decodes_to_nothing() {
    assert!(decode("version: 2\nrules: []\n").unwrap().is_empty());
    assert!(decode("selectors: {}\nrelations: {}\n").unwrap().is_empty());
}

#[test]
fn a_name_is_an_ascii_identifier() {
    for valid in ["a", "_a", "a-b_c9", "Deps"] {
        assert!(is_name(valid), "{valid}");
    }
    for invalid in ["", "9a", "-a", "a.b", "a b", "naïve", "a/b"] {
        assert!(!is_name(invalid), "{invalid}");
    }
    assert_eq!(
        problem("selectors:\n  9lives: { paths: x }\n"),
        "`selectors.9lives`: `9lives` is not a valid name: a name starts with an ASCII letter or \
         `_` and holds only ASCII letters, digits, `_` and `-`"
    );
    assert_eq!(
        problem("relations:\n  a.b: {}\n"),
        "`relations.a.b`: `a.b` is not a valid name: a name starts with an ASCII letter or `_` \
         and holds only ASCII letters, digits, `_` and `-` (a `.` separates an alias from a name \
         in a reference)"
    );
    assert_eq!(
        problem("selectors:\n  1: { paths: x }\n"),
        "`selectors`: `1` is not a name: expected a string, found a number"
    );
    assert_eq!(
        problem("extends:\n  - url: ./a.yml\n    as: my.alias\n"),
        "`extends[0].as`: `my.alias` is not a valid name: a name starts with an ASCII letter or \
         `_` and holds only ASCII letters, digits, `_` and `-` (a `.` separates an alias from a \
         name in a reference)"
    );
}

#[test]
fn a_name_is_declared_once_per_source() {
    let shared = "a name or alias is declared once in a config (selectors, relations and \
                  `extends:` aliases share one namespace), so rename one of them";
    assert_eq!(
        problem(
            "selectors:\n  x: { paths: a }\nrelations:\n  x:\n    from: x\n    read: { whole_file: {} }\n    fields: { f: value }\n"
        ),
        format!("`relations.x`: `x` is already declared at `selectors.x`; {shared}")
    );
    assert_eq!(
        problem("extends:\n  - url: ./a.yml\n    as: a\n  - url: ./b.yml\n    as: a\n"),
        format!("`extends[1].as`: `a` is already declared at `extends[0].as`; {shared}")
    );
    assert_eq!(
        problem("extends:\n  - url: ./a.yml\n    as: x\nselectors:\n  x: { paths: a }\n"),
        format!("`extends[0].as`: `x` is already declared at `selectors.x`; {shared}")
    );
    // A YAML mapping cannot repeat a key, so a name twice in one map is the
    // parser's error before this decode runs.
    assert!(alint_core::yaml::from_str::<Value>("selectors:\n  x: {}\n  x: {}\n").is_err());
}

#[test]
fn an_alias_names_a_local_source() {
    let refusal = "an alias names a local `extends:` source, whose selectors and relations the \
                   config can then reach as `alias.name`; a bundled or remote source must be \
                   `version: 1` and declares none, so remove `as:` from this entry";
    assert_eq!(
        problem("extends:\n  - url: alint://bundled/oss-baseline@v1\n    as: oss\n"),
        format!("`extends[0].as`: {refusal}")
    );
    assert_eq!(
        problem("extends:\n  - url: https://example.invalid/r.yml#sha256-AAAA\n    as: remote\n"),
        format!("`extends[0].as`: {refusal}")
    );
    let local = decode("extends:\n  - url: ./rules/base.yml\n    as: base\n    only: [a]\n")
        .expect("decode");
    assert_eq!(local.aliases[0].1.url, "./rules/base.yml");
}

#[test]
fn a_selector_reads_paths_and_scope_filter_only() {
    assert_eq!(
        problem("selectors:\n  s: { path: x }\n"),
        "`selectors.s`: unknown key `path` in a selector; expected one of `paths`, `scope_filter`"
    );
    assert_eq!(
        problem("selectors:\n  s: {}\n"),
        "`selectors.s`: a selector needs `paths:` (a glob, a list of globs with `!` excludes, \
         or an `{ include, exclude }` map)"
    );
    assert_eq!(
        problem("selectors:\n  s: docs/**\n"),
        "`selectors.s`: expected a selector: a map with `paths:` and an optional \
         `scope_filter:`, found a string"
    );
    assert_eq!(
        problem("selectors:\n  s: { paths: { include: x, bogus: y } }\n"),
        "`selectors.s.paths`: unknown key `bogus` in `paths:`; expected one of `include`, \
         `exclude`"
    );
    assert!(
        problem("selectors:\n  s: { paths: 3 }\n").starts_with("`selectors.s.paths`: "),
        "a v1 decode error, under the selector's path"
    );
    let objects = decode(
        "selectors:\n  s:\n    paths: [\"src/**\", \"!src/gen/**\"]\n    scope_filter:\n      \
         has_ancestor: [Cargo.toml, package.json]\n",
    )
    .expect("decode");
    assert!(objects.selectors[0].1.scope_filter.is_some());
}

#[test]
fn a_selector_cannot_use_changed_since() {
    assert_eq!(
        problem(
            "selectors:\n  s:\n    paths: x\n    scope_filter: { has_ancestor: a, changed_since: main }\n"
        ),
        "`selectors.s.scope_filter.changed_since`: a selector cannot use `changed_since`: the \
         rules that read a selector judge the whole tree, under `--changed` too, so what a \
         selector picks must not depend on the diff; remove it (a selector may use \
         `has_ancestor`, `include_manifest_paths` and `exclude_manifest_paths`)"
    );
    assert_eq!(
        problem("selectors:\n  s:\n    paths: x\n    scope_filter: { has_parent: a }\n"),
        "`selectors.s.scope_filter`: unknown key `has_parent` in a selector's `scope_filter:`; \
         expected one of `has_ancestor`, `include_manifest_paths`, `exclude_manifest_paths`"
    );
}

/// `relations: { r: <body> }`, with `body` written in flow style.
fn relation_error(body: &str) -> String {
    problem(&format!("relations:\n  r: {body}\n"))
}

#[test]
fn a_relation_has_exactly_one_row_source() {
    assert_eq!(
        relation_error("{ from: s, fields: { f: path } }"),
        "`relations.r`: a relation needs exactly one of `entries:`, `read:`, `union:` and \
         `join:`; it has none"
    );
    assert_eq!(
        relation_error(
            "{ from: s, entries: { format: json, at: $ }, read: { json: $ }, fields: { f: path } }"
        ),
        "`relations.r`: a relation needs exactly one of `entries:`, `read:`, `union:` and \
         `join:`; it has `entries:` and `read:`"
    );
    assert_eq!(
        relation_error("{ union: [], join: {}, read: { json: $ }, fields: { f: path } }"),
        "`relations.r`: a relation needs exactly one of `entries:`, `read:`, `union:` and \
         `join:`; it has `read:` and `union:` and `join:`"
    );
    assert_eq!(
        relation_error("{ form: s }"),
        "`relations.r`: unknown key `form` in a relation; expected one of `from`, `entries`, \
         `read`, `optional`, `union`, `join`, `fields`, `transform`, `where`"
    );
    assert_eq!(
        relation_error("[a]"),
        "`relations.r`: expected a relation: a map with `fields:` and one of `entries:`, \
         `read:`, `union:` and `join:`, found a list"
    );
}

#[test]
fn a_relation_needs_its_parts() {
    assert_eq!(
        relation_error("{ read: { json: $ }, fields: { f: value } }"),
        "`relations.r`: `entries:` and `read:` need `from:`, a selector name or an inline \
         selector map"
    );
    assert_eq!(
        relation_error("{ from: s, read: { json: $ } }"),
        "`relations.r`: a relation needs `fields:`, a map from each field's name to its source"
    );
    assert_eq!(
        relation_error("{ from: s, read: { json: $ }, fields: {} }"),
        "`relations.r.fields`: a relation names at least one field"
    );
    assert_eq!(
        relation_error("{ from: 3, read: { json: $ }, fields: { f: value } }"),
        "`relations.r.from`: expected a selector name, or an inline selector map, found a number"
    );
    assert_eq!(
        relation_error("{ from: a.b.c, read: { json: $ }, fields: { f: value } }"),
        "`relations.r.from`: `a.b.c` is not a valid reference: write a name, or `alias.name` \
         for an object of a source this config extends with `as: alias`; a name starts with an \
         ASCII letter or `_` and holds only ASCII letters, digits, `_` and `-` (there is no \
         chained name such as `a.b.c`)"
    );
    assert_eq!(
        relation_error(
            "{ from: s, entries: { format: json, at: $ }, optional: true, fields: { f: entry.key } }"
        ),
        "`relations.r.optional`: `optional:` applies to `read:` only (an `entries:` path a \
         resource lacks yields no rows from it already); remove it"
    );
    assert_eq!(
        relation_error("{ from: s, read: { json: $ }, optional: yes, fields: { f: value } }"),
        "`relations.r.optional`: expected `true` or `false`, found a string"
    );
}

#[test]
fn a_union_has_two_or_more_parts_each_with_its_own_from() {
    assert_eq!(
        relation_error("{ union: [{ from: s, read: { json: $ } }], fields: { f: value } }"),
        "`relations.r.union`: a union needs at least two parts; a relation with one part is a \
         plain relation, so write its `from:` and `entries:` or `read:` on the relation"
    );
    assert_eq!(
        relation_error(
            "{ from: s, union: [{ from: s, read: { json: $ } }, { from: t, read: { json: $ } }], fields: { f: value } }"
        ),
        "`relations.r.from`: a relation with `union:` takes no `from:`; each part of the union \
         has its own"
    );
    assert_eq!(
        relation_error(
            "{ union: [{ from: s, read: { json: $ } }, { from: t }], fields: { f: value } }"
        ),
        "`relations.r.union[1]`: a union part needs exactly one of `entries:` and `read:`; it \
         has neither"
    );
    assert_eq!(
        relation_error(
            "{ union: [{ from: s, read: { json: $ }, entries: { format: json, at: $ } }, { from: t, read: { json: $ } }], fields: { f: value } }"
        ),
        "`relations.r.union[0]`: a union part needs exactly one of `entries:` and `read:`; it \
         has both"
    );
    assert_eq!(
        relation_error(
            "{ union: [{ from: s, read: { json: $ }, fields: {} }, { from: t, read: { json: $ } }], fields: { f: value } }"
        ),
        "`relations.r.union[0]`: unknown key `fields` in a union part; expected one of `from`, \
         `entries`, `read`, `optional`"
    );
    assert_eq!(
        relation_error("{ union: { from: s }, fields: { f: value } }"),
        "`relations.r.union`: expected a list of parts, each `{ from, entries or read, \
         optional }`, found a map"
    );
}

#[test]
fn entries_take_a_format_and_paths() {
    let entries = |entries: &str| {
        relation_error(&format!(
            "{{ from: s, entries: {entries}, fields: {{ f: entry.key }} }}"
        ))
    };
    assert_eq!(
        entries("{ format: xml, at: $ }"),
        "`relations.r.entries.format`: unknown format `xml`; expected one of `json`, `yaml`, \
         `toml` (in this build entries are read from JSON, YAML and TOML only)"
    );
    assert_eq!(
        entries("{ at: $ }"),
        "`relations.r.entries`: `entries:` needs `format:` (`json`, `yaml` or `toml`)"
    );
    assert_eq!(
        entries("{ format: json }"),
        "`relations.r.entries`: `entries:` needs `at:`, a JSONPath or a list of them selecting \
         the objects whose members become rows"
    );
    assert_eq!(
        entries("{ format: json, at: [] }"),
        "`relations.r.entries.at`: expected a JSONPath, or a non-empty list of them, found a list"
    );
    assert_eq!(
        entries("{ format: json, at: [$, 1] }"),
        "`relations.r.entries.at[1]`: expected a JSONPath, found a number"
    );
    assert_eq!(
        entries("{ format: json, at: $, leaves: 1 }"),
        "`relations.r.entries.leaves`: expected `true` or `false`, found a number"
    );
    assert_eq!(
        entries("{ format: json, at: $, depth: 1 }"),
        "`relations.r.entries`: unknown key `depth` in `entries:`; expected one of `format`, \
         `at`, `leaves`"
    );
}

#[test]
fn read_takes_one_supported_form() {
    let read = |read: &str| {
        relation_error(&format!(
            "{{ from: s, read: {read}, fields: {{ f: value }} }}"
        ))
    };
    for format in ["dotenv", "properties", "xml", "ini", "hcl"] {
        assert_eq!(
            read(&format!("{{ {format}: KEY }}")),
            format!(
                "`relations.r.read`: `read:` cannot take `{format}` in this build: a relation \
                 reads JSON, YAML and TOML values, a regex, lines or a whole file (dotenv and \
                 properties values stay redacted, and XML, INI and HCL are not readable)"
            )
        );
    }
    assert_eq!(
        read("{ json: $.a, yaml: $.b }"),
        "`relations.r.read`: `read:` takes exactly one of `json`, `yaml`, `toml`, `regex`, \
         `lines`, `whole_file`; it has several"
    );
    assert_eq!(
        read("{}"),
        "`relations.r.read`: `read:` takes exactly one of `json`, `yaml`, `toml`, `regex`, \
         `lines`, `whole_file`; it has none"
    );
    assert_eq!(
        read("{ jsonpath: $ }"),
        "`relations.r.read`: unknown key `jsonpath` in `read:`; expected one of `json`, `yaml`, \
         `toml`, `regex`, `lines`, `whole_file`"
    );
    assert_eq!(
        read("{ json: [$] }"),
        "`relations.r.read.json`: expected a JSONPath, found a list"
    );
    assert_eq!(
        read("{ whole_file: { max: 1 } }"),
        "`relations.r.read.whole_file`: `whole_file:` takes no options; write `whole_file: {}`"
    );
    assert_eq!(
        read("{ lines: { comments: x } }"),
        "`relations.r.read.lines`: unknown key `comments` in `lines:`; expected one of `comment`"
    );
    assert_eq!(
        read("{ lines: true }"),
        "`relations.r.read.lines`: expected a map: `{}` or `{ comment: \"#\" }`, found a boolean"
    );
}

#[test]
fn a_join_names_both_sides_and_its_unique_side() {
    let join =
        |join: &str, fields: &str| relation_error(&format!("{{ join: {join}, fields: {fields} }}"));
    let sides = "{ f: left.f }";
    assert_eq!(
        join("{ left: a, right: b, on: { k: k } }", sides),
        "`relations.r.join`: a join needs `unique:` (`left` or `right`), the side that holds at \
         most one row per join key; it has no default"
    );
    assert_eq!(
        join("{ left: a, right: b, on: { k: k }, unique: both }", sides),
        "`relations.r.join.unique`: unknown side `both`; expected `left` or `right`"
    );
    assert_eq!(
        join("{ left: a, right: b, on: {}, unique: left }", sides),
        "`relations.r.join.on`: a join matches on at least one field"
    );
    assert_eq!(
        join("{ right: b, on: { k: k }, unique: left }", sides),
        "`relations.r.join`: a join needs `left:`, a relation name"
    );
    assert_eq!(
        join(
            "{ left: a, right: { from: s }, on: { k: k }, unique: left }",
            sides
        ),
        "`relations.r.join.right`: expected a relation name, found a map"
    );
    assert_eq!(
        join("{ left: a, right: b, on: { k: 1 }, unique: left }", sides),
        "`relations.r.join.on.k`: expected a right field's name, found a number"
    );
    assert_eq!(
        join(
            "{ left: a, right: b, on: { k: k }, unique: left, how: inner }",
            sides
        ),
        "`relations.r.join`: unknown key `how` in a join; expected one of `left`, `right`, \
         `on`, `unique`"
    );
    assert_eq!(
        relation_error(
            "{ join: { left: a, right: b, on: { k: k }, unique: left }, optional: true, fields: { f: left.f } }"
        ),
        "`relations.r.optional`: a relation with `join:` takes no `optional:`; a join reads \
         the relations it names"
    );
}

#[test]
fn a_field_source_is_one_of_the_documented_forms() {
    let field = |source: &str| {
        relation_error(&format!(
            "{{ from: s, read: {{ json: $ }}, fields: {{ f: {source} }} }}"
        ))
    };
    assert_eq!(
        field("entry.name"),
        "`relations.r.fields.f`: `entry.name` is not a field source; expected `path`, a path \
         token (`dir`, `basename`, `stem`, `ext`, `parent_name`), `entry.key`, `entry.value`, \
         `entry.parent`, `entry.root`, `entry.path`, `value`, `bound: <JSONPath>`, or in a \
         join's output `left.<field>` or `right.<field>`"
    );
    assert_eq!(
        field("'bound:'"),
        "`relations.r.fields.f`: `bound:` needs a JSONPath, as in `bound: $.name`"
    );
    assert_eq!(
        field("{ bound: $.name }"),
        "`relations.r.fields.f`: unknown key `bound` in a field; a bound field's source is the \
         string `\"bound: <JSONPath>\"`, as in `{ source: \"bound: $.name\" }`"
    );
    assert_eq!(
        field("{ sensitive: true }"),
        "`relations.r.fields.f`: a field needs `source:`: `path`, a path token (`dir`, \
         `basename`, `stem`, `ext`, `parent_name`), `entry.key`, `entry.value`, `entry.parent`, \
         `entry.root`, `entry.path`, `value`, `bound: <JSONPath>`, or in a join's output \
         `left.<field>` or `right.<field>`"
    );
    assert_eq!(
        field("{ source: value, sensitive: 1 }"),
        "`relations.r.fields.f.sensitive`: expected `true` or `false`, found a number"
    );
    assert_eq!(
        field("[value]"),
        "`relations.r.fields.f`: expected a field source or a `{ source, sensitive }` map, \
         found a list"
    );
    assert_eq!(
        field("entry.key"),
        "`relations.r.fields.f`: `entry.*` sources need `entries:`; a `read:` row has `value`"
    );
    assert_eq!(
        field("left.f"),
        "`relations.r.fields.f`: `left.<field>` and `right.<field>` name the sides of a \
         `join:`, which this relation does not have"
    );
    assert_eq!(
        field("left.a.b"),
        "`relations.r.fields.f`: `a.b` is not a valid name: a name starts with an ASCII letter \
         or `_` and holds only ASCII letters, digits, `_` and `-` (a `.` separates an alias \
         from a name in a reference)"
    );
    assert_eq!(
        relation_error("{ from: s, entries: { format: json, at: $ }, fields: { f: value } }"),
        "`relations.r.fields.f`: `value` is a `read:` row's value; an `entries:` row has \
         `entry.value`"
    );
    assert_eq!(
        relation_error(
            "{ join: { left: a, right: b, on: { k: k }, unique: left }, fields: { f: path } }"
        ),
        "`relations.r.fields.f`: a join's output fields each name a side: write \
         `left.<field>` or `right.<field>` (nothing is copied implicitly)"
    );
    assert_eq!(
        relation_error("{ from: s, read: { json: $ }, fields: { a-b.c: value } }"),
        "`relations.r.fields.a-b.c`: `a-b.c` is not a valid name: a name starts with an ASCII \
         letter or `_` and holds only ASCII letters, digits, `_` and `-` (a `.` separates an \
         alias from a name in a reference)"
    );
}

#[test]
fn transforms_are_the_fixed_list() {
    let transform = |transform: &str| {
        relation_error(&format!(
            "{{ from: s, read: {{ json: $ }}, fields: {{ f: value }}, transform: {transform} }}"
        ))
    };
    assert_eq!(
        transform("{ f: [cldr-plurals] }"),
        "`relations.r.transform.f[0]`: `cldr-plurals` is not a transform: CLDR plural collapse \
         is `plurals: cldr`, an option of a `set` assertion"
    );
    assert_eq!(
        transform("{ f: [trim, upper] }"),
        "`relations.r.transform.f[1]`: unknown transform `upper`; expected one of `trim`, \
         `lower`, `semver-major`, `semver-minor`, `strip-root`"
    );
    assert_eq!(
        transform("{ f: [] }"),
        "`relations.r.transform.f`: expected a non-empty list of transforms (`trim`, `lower`, \
         `semver-major`, `semver-minor`, `strip-root`), found a list"
    );
    assert_eq!(
        transform("{ f: trim }"),
        "`relations.r.transform.f`: expected a non-empty list of transforms (`trim`, `lower`, \
         `semver-major`, `semver-minor`, `strip-root`), found a string"
    );
    assert_eq!(
        transform("[trim]"),
        "`relations.r.transform`: expected a map from a field's name to a list of transforms, \
         found a list"
    );
    let objects = decode(
        "relations:\n  r: { from: s, read: { json: $ }, fields: { f: value }, transform: { f: \
         [trim, lower, semver-major, semver-minor, strip-root] } }\n",
    )
    .expect("decode");
    assert_eq!(
        objects.relations[0].1.transforms[0].1,
        vec![
            Transform::Trim,
            Transform::Lower,
            Transform::SemverMajor,
            Transform::SemverMinor,
            Transform::StripRoot
        ]
    );
}

#[test]
fn a_filter_holds_known_predicates() {
    let filter = |filter: &str| {
        relation_error(&format!(
            "{{ from: s, read: {{ json: $ }}, fields: {{ f: value }}, where: {filter} }}"
        ))
    };
    assert_eq!(
        filter("{ f: { equals: x } }"),
        "`relations.r.where.f`: unknown key `equals` in a filter; expected one of `in`, \
         `not_in`, `matches`, `not_matches`"
    );
    assert_eq!(
        filter("{ f: {} }"),
        "`relations.r.where.f`: a filter needs at least one of `in`, `not_in`, `matches`, \
         `not_matches`"
    );
    assert_eq!(
        filter("{ f: { in: [] } }"),
        "`relations.r.where.f.in`: expected a non-empty list of literals and `{ relation, field \
         }` references, found a list"
    );
    assert_eq!(
        filter("{ f: { not_in: x } }"),
        "`relations.r.where.f.not_in`: expected a non-empty list of literals and `{ relation, \
         field }` references, found a string"
    );
    assert_eq!(
        filter("{ f: { in: [[a]] } }"),
        "`relations.r.where.f.in[0]`: expected a literal (null, a boolean, a number or a \
         string) or a `{ relation, field }` reference, found a list"
    );
    assert_eq!(
        filter("{ f: { in: [{ relation: r }] } }"),
        "`relations.r.where.f.in[0]`: a membership reference needs `relation:` and `field:`"
    );
    assert_eq!(
        filter("{ f: { in: [{ relation: r, field: f, where: x }] } }"),
        "`relations.r.where.f.in[0]`: unknown key `where` in a membership reference; expected \
         one of `relation`, `field`"
    );
    assert_eq!(
        filter("{ f: { matches: [x] } }"),
        "`relations.r.where.f.matches`: expected a regex, found a list"
    );
    assert_eq!(
        filter("{ f: { in: [{ relation: a.b.c, field: f }] } }"),
        "`relations.r.where.f.in[0].relation`: `a.b.c` is not a valid reference: write a name, \
         or `alias.name` for an object of a source this config extends with `as: alias`; a \
         name starts with an ASCII letter or `_` and holds only ASCII letters, digits, `_` and \
         `-` (there is no chained name such as `a.b.c`)"
    );
}

#[test]
fn a_yaml_tag_means_nothing_in_a_selector_or_relation() {
    assert_eq!(
        relation_error("!relation { from: s }"),
        "`relations.r`: a YAML tag (`!relation`) has no meaning in a selector or relation; \
         remove it"
    );
    assert_eq!(
        problem("selectors:\n  s: { paths: [a, !glob b] }\n"),
        "`selectors.s.paths[1]`: a YAML tag (`!glob`) has no meaning in a selector or \
         relation; remove it"
    );
}

#[test]
fn a_regex_compiles_and_reads_capture_group_one() {
    let unclosed = String::from("(");
    let invalid = regex::Regex::new(&unclosed).unwrap_err();
    assert_eq!(
        relation_error(
            "{ from: s, read: { json: $ }, fields: { f: value }, where: { f: { matches: '(' } } }"
        ),
        format!("`relations.r.where.f.matches`: `(` is not a valid regex: {invalid}")
    );
    assert_eq!(
        relation_error(
            "{ from: s, read: { json: $ }, fields: { f: value }, where: { f: { not_matches: '(' } } }"
        ),
        format!("`relations.r.where.f.not_matches`: `(` is not a valid regex: {invalid}")
    );
    assert_eq!(
        relation_error("{ from: s, read: { regex: '(' }, fields: { f: value } }"),
        format!("`relations.r.read.regex`: `(` is not a valid regex: {invalid}")
    );
    assert_eq!(
        relation_error("{ from: s, read: { regex: '^v.*' }, fields: { f: value } }"),
        "`relations.r.read.regex`: a `read: { regex }` takes each value from capture group 1, \
         and `^v.*` has no group; put the value in parentheses, as in `version: (.*)`"
    );
    decode("relations:\n  r: { from: s, read: { regex: '^v(.*)' }, fields: { f: value } }\n")
        .expect("a group");
    decode("relations:\n  r: { from: s, read: { regex: '^(?:v)?(.*)' }, fields: { f: value } }\n")
        .expect("a group after a non-capturing one");
}

#[test]
fn a_json_path_parses() {
    let bad =
        |query: &str| alint_core::Extract::structured(alint_core::Format::Json, query).unwrap_err();
    assert_eq!(
        relation_error("{ from: s, read: { json: '$.a[' }, fields: { f: value } }"),
        format!("`relations.r.read.json`: {}", bad("$.a["))
    );
    assert_eq!(
        relation_error("{ from: s, read: { toml: 'a' }, fields: { f: value } }"),
        format!("`relations.r.read.toml`: {}", bad("a"))
    );
    assert_eq!(
        relation_error("{ from: s, read: { yaml: $.dev-deps }, fields: { f: value } }"),
        format!(
            "`relations.r.read.yaml`: {}; JSONPath dot-notation requires identifier-shape keys \
             (RFC 9535). For dashed keys, use bracket notation: `$['dev-deps']` instead of \
             `$.dev-deps` (or `@['dev-deps']` instead of `@.dev-deps` inside a filter). See \
             `docs/development/CONFIG-AUTHORING.md` § 10.",
            bad("$.dev-deps")
        )
    );
    assert_eq!(
        relation_error(
            "{ from: s, entries: { format: json, at: '$.a[' }, fields: { f: entry.key } }"
        ),
        format!("`relations.r.entries.at`: {}", bad("$.a["))
    );
    assert_eq!(
        relation_error(
            "{ from: s, entries: { format: json, at: [$.a, '$.b['] }, fields: { f: entry.key } }"
        ),
        format!("`relations.r.entries.at[1]`: {}", bad("$.b["))
    );
    assert_eq!(
        relation_error("{ from: s, read: { json: $ }, fields: { f: 'bound: $.a[' } }"),
        format!("`relations.r.fields.f`: {}", bad("$.a["))
    );
}

/// A source declares each name in constant time, held by name: two hundred
/// thousand of them declare in a moment, where a scan of the names before
/// each took minutes. A repeat is still refused, naming the first.
#[test]
fn declaring_many_names_takes_time_linear_in_them() {
    let error = crate::tests::on_thread(8 << 20, std::time::Duration::from_secs(20), || {
        let decoder = Decoder {
            source: ".alint.yml",
        };
        let mut names = Names::default();
        for index in 0..200_000 {
            let name = format!("s{index}");
            names
                .declare(&decoder, &name, &format!("selectors.{name}"))
                .unwrap();
        }
        names
            .declare(&decoder, "s7", "relations.s7")
            .unwrap_err()
            .to_string()
    });
    assert_eq!(
        error,
        ".alint.yml: `relations.s7`: `s7` is already declared at `selectors.s7`; a name or \
         alias is declared once in a config (selectors, relations and `extends:` aliases share \
         one namespace), so rename one of them"
    );
}

/// Marking what a source's selectors and relations read from the
/// environment looks each up by name: two hundred thousand selectors and a
/// hundred thousand relations mark in a moment, where a scan per object took
/// minutes.
#[test]
fn marking_many_objects_takes_time_linear_in_them() {
    use crate::v2::model::{self, Selector};

    let marked = crate::tests::on_thread(8 << 20, std::time::Duration::from_secs(20), || {
        let mut selectors = ayaml::Mapping::new();
        let mut relations = ayaml::Mapping::new();
        let mut objects = SourceObjects {
            source: SourceId {
                display: ".alint.yml".into(),
                path: "/repo/.alint.yml".into(),
                class: alint_core::ConfigSourceClass::Root,
            },
            selectors: Vec::new(),
            relations: Vec::new(),
            aliases: Vec::new(),
            extended: Vec::new(),
        };
        let selector = Selector {
            paths: alint_core::PathsSpec::Single("a.json".into()),
            scope_filter: None,
            reads_environment: false,
            twins: model::Twins::Unneeded,
        };
        let relation = model::Relation {
            rows: model::Rows::Part(model::Part {
                from: model::SelectorRef::Named(model::Reference {
                    alias: None,
                    name: "s0".into(),
                }),
                reader: model::Reader::Read {
                    read: model::Read::Json("$.x".into()),
                    optional: false,
                },
            }),
            fields: vec![(
                "x".into(),
                model::Field {
                    source: model::FieldSource::Value,
                    sensitive: false,
                },
            )],
            transforms: Vec::new(),
            filters: Vec::new(),
            reads_environment: false,
            twins: model::Twins::Unneeded,
        };
        let reading: Value =
            alint_core::yaml::from_str("{paths: \"{{env.X | default('a')}}\"}").unwrap();
        let filtered: Value = alint_core::yaml::from_str(
            "{from: s0, read: {json: $.x}, fields: {x: value}, where: {x: {in: \
             [\"{{env.Y | default('b')}}\"]}}}",
        )
        .unwrap();
        for index in 0..200_000 {
            let name = format!("s{index}");
            selectors.insert(Value::from(name.as_str()), reading.clone());
            objects.selectors.push((name, selector.clone()));
        }
        for index in 0..100_000 {
            let name = format!("r{index}");
            relations.insert(Value::from(name.as_str()), filtered.clone());
            objects.relations.push((name, relation.clone()));
        }
        let mut document = ayaml::Mapping::new();
        document.insert("selectors".into(), Value::Mapping(selectors));
        document.insert("relations".into(), Value::Mapping(relations));
        crate::v2::Environment::of(&Value::Mapping(document)).mark(&mut objects);
        objects
    });
    assert!(marked.selectors.iter().all(|(_, s)| s.reads_environment));
    assert!(
        marked
            .relations
            .iter()
            .all(|(_, relation)| { !relation.reads_environment && relation.fields[0].1.sensitive })
    );
}

/// A `bound:` field reads a JSON, YAML or TOML document, so a relation that
/// reads the file as text cannot have one: refused when it loads, not at
/// every evaluation.
#[test]
fn a_relation_that_reads_text_has_no_bound_field() {
    for (read, form) in [
        ("{ regex: 'v(.*)' }", "regex"),
        ("{ lines: {} }", "lines"),
        ("{ whole_file: {} }", "whole_file"),
    ] {
        assert_eq!(
            problem(&format!(
                "relations:\n  r:\n    from: {{ paths: a }}\n    read: {read}\n    fields: {{ \
                 v: value, name: 'bound: $.name' }}\n"
            )),
            format!(
                "`relations.r.fields.name`: a `bound:` field reads a value from a JSON, YAML or \
                 TOML document, and a `read:` with `{form}:` reads the file as text; read the \
                 file with `entries:`, or with `read:` and `json:`, `yaml:` or `toml:`"
            ),
        );
    }
    for read in ["{ json: $.v }", "{ yaml: $.v }", "{ toml: $.v }"] {
        decode(&format!(
            "relations:\n  r:\n    from: {{ paths: a }}\n    read: {read}\n    fields: {{ v: \
             value, name: 'bound: $.name' }}\n"
        ))
        .unwrap();
    }
}

/// An empty selector filter is refused in a selector's own words, without
/// the `changed_since:` a selector cannot use; so are an empty
/// `has_ancestor:` and an empty file name, which the draft schema refuses
/// too.
#[test]
fn an_empty_selector_filter_is_refused_in_a_selectors_words() {
    for (filter, at, message) in [
        (
            "{}",
            "selectors.s.scope_filter",
            "a selector's `scope_filter:` sets at least one of `has_ancestor:`, \
             `include_manifest_paths:` and `exclude_manifest_paths:`; set one, or remove \
             `scope_filter:`",
        ),
        (
            "{ has_ancestor: [] }",
            "selectors.s.scope_filter.has_ancestor",
            "`has_ancestor:` lists no file name; list at least one, as in `has_ancestor: \
             package.json`, or remove it",
        ),
        (
            "{ has_ancestor: '' }",
            "selectors.s.scope_filter.has_ancestor",
            "`has_ancestor:` names a file with an empty string; write the file's name, as in \
             `has_ancestor: package.json`",
        ),
        (
            "{ has_ancestor: [Cargo.toml, ''] }",
            "selectors.s.scope_filter.has_ancestor",
            "`has_ancestor:` names a file with an empty string; write the file's name, as in \
             `has_ancestor: package.json`",
        ),
    ] {
        assert_eq!(
            problem(&format!(
                "selectors:\n  s: {{ paths: a, scope_filter: {filter} }}\n"
            )),
            format!("`{at}`: {message}"),
            "{filter}"
        );
    }
    // An inline selector's filter too.
    assert_eq!(
        problem(
            "relations:\n  r:\n    from: { paths: a, scope_filter: {} }\n    read: { json: $ \
             }\n    fields: { v: value }\n"
        ),
        "`relations.r.from.scope_filter`: a selector's `scope_filter:` sets at least one of \
         `has_ancestor:`, `include_manifest_paths:` and `exclude_manifest_paths:`; set one, or \
         remove `scope_filter:`"
    );
}
