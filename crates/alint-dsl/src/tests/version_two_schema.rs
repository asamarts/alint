//! The draft version 2 schema (`schemas/v2/config.json`) against the loader,
//! on configs chosen for where they fail (design
//! `v0.23/selectors-and-constraints.md` section 11). `v2/schema_parity.rs`
//! checks every config the unit tests decode and the vocabulary; this module
//! holds the refusals:
//! - a config the decode refuses for its shape (a wrong type, an unknown key, a
//!   count, a value off a list, a malformed name) the schema refuses too;
//! - a config the decode refuses for what JSON Schema cannot see the schema
//!   accepts, and the list says why (these are the limits the version 2 docs
//!   page lists);
//! - a config only the loader refuses, once every source has loaded (a name
//!   that does not resolve, a cycle, what the plan check finds), the schema
//!   accepts: those are the loader's alone.
//!
//! Each config is checked through the decode stage and through the whole
//! loader, in a directory that holds `base.yml` for the configs that extend it.

use serde_json::Value as Json;

use crate::v2::schema_parity::{decode_stage, json_of, schema_errors};

use super::*;

/// A version 2 source that `extends: [{ url: ./base.yml, as: base }]` reaches.
const BASE: &str = "version: 2\nrelations:\n  pins:\n    from: { paths: pins.json }\n    \
                    entries: { format: json, at: $ }\n    fields: { name: entry.key, version: \
                    entry.value }\nrules: []\n";

/// What the whole loader says about `config`, written as `.alint.yml` beside
/// `base.yml`.
fn loaded(config: &str) -> alint_core::Result<Config> {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("base.yml"), BASE).unwrap();
    let path = tmp.path().join(".alint.yml");
    std::fs::write(&path, config).unwrap();
    load(&path)
}

fn doc(body: &str) -> String {
    format!("version: 2\n{body}")
}

/// `relations: { r: <body> }`, `body` in flow style.
fn rel(body: &str) -> String {
    doc(&format!("relations:\n  r: {body}\n"))
}

/// `selectors: { s: <body> }`.
fn sel(body: &str) -> String {
    doc(&format!("selectors:\n  s: {body}\n"))
}

/// A relation reading `read:` with `fields: { f: value }` and `extra` keys.
fn read_rel(extra: &str) -> String {
    rel(&format!(
        "{{ from: s, read: {{ json: $.a }}, fields: {{ f: value }}{extra} }}"
    ))
}

/// Configs the decode refuses for their shape, each with what is wrong.
fn shape_refusals() -> Vec<(&'static str, String)> {
    let mut configs = vec![
        // The document.
        ("an unknown top-level key", doc("relation:\n  deps: {}\n")),
        ("`selectors:` that is a list", doc("selectors: [a]\n")),
        ("`relations:` that is null", doc("relations:\n")),
        (
            "a quoted version",
            "version: '2'\nselectors: {}\n".to_string(),
        ),
        (
            "version 1 with selectors",
            "version: 1\nselectors: {}\n".to_string(),
        ),
        (
            "an unknown key in an `extends:` entry",
            doc("extends:\n  - { url: ./base.yml, alias: base }\n"),
        ),
        (
            "an alias that is not a string",
            doc("extends:\n  - { url: ./base.yml, as: 3 }\n"),
        ),
        (
            "an alias on a bundled source",
            doc("extends:\n  - { url: alint://bundled/oss-baseline@v1, as: oss }\n"),
        ),
        (
            "an alias on a remote source, in any case",
            doc("extends:\n  - { url: 'HTTPS://example.invalid/r.yml#sha256-AAAA', as: r }\n"),
        ),
        // Names and references.
        (
            "an alias with a dot",
            doc("extends:\n  - { url: ./base.yml, as: my.alias }\n"),
        ),
        (
            "a name that starts with a digit",
            doc("selectors:\n  9lives: { paths: x }\n"),
        ),
        ("a name with a dot", doc("relations:\n  a.b: {}\n")),
        (
            "a name YAML reads as a number",
            doc("selectors:\n  1: { paths: x }\n"),
        ),
        ("an empty name", doc("selectors:\n  '': { paths: x }\n")),
        (
            "an interpolated name",
            doc("selectors:\n  '{{env.NAME | default(\"s\")}}': { paths: x }\n"),
        ),
        (
            "an interpolated reference",
            rel(
                "{ from: '{{env.S | default(\"s\")}}', read: { json: $.a }, fields: { f: value } }",
            ),
        ),
        (
            "a chained reference",
            rel("{ from: a.b.c, read: { json: $.a }, fields: { f: value } }"),
        ),
        // Selectors.
        ("an unknown selector key", sel("{ path: x }")),
        ("a selector without `paths:`", sel("{}")),
        ("a selector that is a string", sel("docs/**")),
        (
            "an unknown key in `paths:`",
            sel("{ paths: { include: x, bogus: y } }"),
        ),
        ("`paths:` that is a number", sel("{ paths: 3 }")),
        (
            "`changed_since` in a selector",
            sel("{ paths: x, scope_filter: { has_ancestor: a, changed_since: main } }"),
        ),
        (
            "an unknown `scope_filter:` key",
            sel("{ paths: x, scope_filter: { has_parent: a } }"),
        ),
        (
            "an inline selector without `paths:`",
            rel("{ from: {}, read: { json: $.a }, fields: { f: value } }"),
        ),
        // Relations.
        ("a relation that is a list", rel("[a]")),
        (
            "a relation without a row source",
            rel("{ from: s, fields: { f: path } }"),
        ),
        (
            "a relation with two row sources",
            rel(
                "{ from: s, entries: { format: json, at: $ }, read: { json: $ }, fields: { f: \
                    path } }",
            ),
        ),
        (
            "a relation with three row sources",
            rel("{ union: [], join: {}, read: { json: $ }, fields: { f: path } }"),
        ),
        ("an unknown relation key", read_rel(", form: s")),
        (
            "`read:` without `from:`",
            rel("{ read: { json: $ }, fields: { f: value } }"),
        ),
        (
            "a relation without `fields:`",
            rel("{ from: s, read: { json: $ } }"),
        ),
        (
            "empty `fields:`",
            rel("{ from: s, read: { json: $ }, fields: {} }"),
        ),
        (
            "`from:` that is a number",
            rel("{ from: 3, read: { json: $ }, fields: { f: value } }"),
        ),
        (
            "`optional:` beside `entries:`",
            rel(
                "{ from: s, entries: { format: json, at: $ }, optional: true, fields: { f: \
                    entry.key } }",
            ),
        ),
        (
            "`optional:` that is not a boolean",
            read_rel(", optional: yes"),
        ),
        // Unions.
        (
            "a union with `from:`",
            rel(
                "{ from: s, union: [{ from: s, read: { json: $ } }, { from: t, read: { json: $ \
                    } }], fields: { f: value } }",
            ),
        ),
        (
            "a union with `optional:`",
            rel(
                "{ optional: true, union: [{ from: s, read: { json: $ } }, { from: t, read: { \
                    json: $ } }], fields: { f: value } }",
            ),
        ),
        (
            "a union of one part",
            rel("{ union: [{ from: s, read: { json: $ } }], fields: { f: value } }"),
        ),
        (
            "a union part with neither `entries:` nor `read:`",
            rel("{ union: [{ from: s, read: { json: $ } }, { from: t }], fields: { f: value } }"),
        ),
        (
            "a union part with both `entries:` and `read:`",
            rel(
                "{ union: [{ from: s, read: { json: $ }, entries: { format: json, at: $ } }, { \
                    from: t, read: { json: $ } }], fields: { f: value } }",
            ),
        ),
        (
            "a union part with `fields:`",
            rel(
                "{ union: [{ from: s, read: { json: $ }, fields: {} }, { from: t, read: { json: \
                    $ } }], fields: { f: value } }",
            ),
        ),
        (
            "a union part without `from:`",
            rel(
                "{ union: [{ read: { json: $ } }, { from: t, read: { json: $ } }], fields: { f: \
                    value } }",
            ),
        ),
        (
            "a union part with `optional:` beside `entries:`",
            rel(
                "{ union: [{ from: s, entries: { format: json, at: $ }, optional: true }, { \
                    from: t, read: { json: $ } }], fields: { f: path } }",
            ),
        ),
        (
            "a union that is a map",
            rel("{ union: { from: s }, fields: { f: value } }"),
        ),
        // Joins.
        (
            "a join with `from:`",
            rel(
                "{ from: s, join: { left: a, right: b, on: { k: k }, unique: left }, fields: { \
                    f: left.f } }",
            ),
        ),
        (
            "a join with `optional:`",
            rel(
                "{ join: { left: a, right: b, on: { k: k }, unique: left }, optional: true, \
                    fields: { f: left.f } }",
            ),
        ),
        (
            "a join without `unique:`",
            rel("{ join: { left: a, right: b, on: { k: k } }, fields: { f: left.f } }"),
        ),
        (
            "an unknown join side",
            rel(
                "{ join: { left: a, right: b, on: { k: k }, unique: both }, fields: { f: left.f \
                    } }",
            ),
        ),
        (
            "an empty `on:`",
            rel("{ join: { left: a, right: b, on: {}, unique: left }, fields: { f: left.f } }"),
        ),
        (
            "a join without `on:`",
            rel("{ join: { left: a, right: b, unique: left }, fields: { f: left.f } }"),
        ),
        (
            "a join without `left:`",
            rel("{ join: { right: b, on: { k: k }, unique: left }, fields: { f: left.f } }"),
        ),
        (
            "a join side that is a map",
            rel(
                "{ join: { left: a, right: { from: s }, on: { k: k }, unique: left }, fields: { \
                    f: left.f } }",
            ),
        ),
        (
            "an `on:` field that is a number",
            rel(
                "{ join: { left: a, right: b, on: { k: 1 }, unique: left }, fields: { f: left.f \
                    } }",
            ),
        ),
        (
            "an `on:` field with a dot",
            rel(
                "{ join: { left: a, right: b, on: { a.b: k }, unique: left }, fields: { f: \
                    left.f } }",
            ),
        ),
        (
            "an unknown join key",
            rel(
                "{ join: { left: a, right: b, on: { k: k }, unique: left, how: inner }, fields: \
                    { f: left.f } }",
            ),
        ),
        // `entries:`.
        (
            "an unknown format",
            rel("{ from: s, entries: { format: xml, at: $ }, fields: { f: entry.key } }"),
        ),
        (
            "`entries:` without `format:`",
            rel("{ from: s, entries: { at: $ }, fields: { f: entry.key } }"),
        ),
        (
            "`entries:` without `at:`",
            rel("{ from: s, entries: { format: json }, fields: { f: entry.key } }"),
        ),
        (
            "an empty `at:` list",
            rel("{ from: s, entries: { format: json, at: [] }, fields: { f: entry.key } }"),
        ),
        (
            "an `at:` path that is a number",
            rel("{ from: s, entries: { format: json, at: [$, 1] }, fields: { f: entry.key } }"),
        ),
        (
            "`leaves:` that is a number",
            rel(
                "{ from: s, entries: { format: json, at: $, leaves: 1 }, fields: { f: entry.key \
                    } }",
            ),
        ),
        (
            "an unknown `entries:` key",
            rel(
                "{ from: s, entries: { format: json, at: $, depth: 1 }, fields: { f: entry.key } }",
            ),
        ),
        (
            "`entries:` that is a string",
            rel("{ from: s, entries: json, fields: { f: entry.key } }"),
        ),
        // `read:`.
        (
            "two `read:` forms",
            rel("{ from: s, read: { json: $.a, yaml: $.b }, fields: { f: value } }"),
        ),
        (
            "no `read:` form",
            rel("{ from: s, read: {}, fields: { f: value } }"),
        ),
        (
            "an unknown `read:` form",
            rel("{ from: s, read: { jsonpath: $ }, fields: { f: value } }"),
        ),
        (
            "a JSONPath that is a list",
            rel("{ from: s, read: { json: [$] }, fields: { f: value } }"),
        ),
        (
            "`whole_file:` with options",
            rel("{ from: s, read: { whole_file: { max: 1 } }, fields: { f: value } }"),
        ),
        (
            "`lines:` with an unknown key",
            rel("{ from: s, read: { lines: { comments: x } }, fields: { f: value } }"),
        ),
        (
            "`lines:` that is a boolean",
            rel("{ from: s, read: { lines: true }, fields: { f: value } }"),
        ),
        (
            "a `lines:` comment that is a number",
            rel("{ from: s, read: { lines: { comment: 1 } }, fields: { f: value } }"),
        ),
        (
            "`read:` that is a string",
            rel("{ from: s, read: $.a, fields: { f: value } }"),
        ),
        // Fields.
        (
            "an unknown field source",
            rel("{ from: s, read: { json: $ }, fields: { f: entry.name } }"),
        ),
        (
            "an empty `bound:`",
            rel("{ from: s, read: { json: $ }, fields: { f: 'bound:' } }"),
        ),
        (
            "a `bound:` map",
            rel("{ from: s, read: { json: $ }, fields: { f: { bound: $.name } } }"),
        ),
        (
            "a field map without `source:`",
            rel("{ from: s, read: { json: $ }, fields: { f: { sensitive: true } } }"),
        ),
        (
            "`sensitive:` that is a number",
            rel("{ from: s, read: { json: $ }, fields: { f: { source: value, sensitive: 1 } } }"),
        ),
        (
            "an unknown field map key",
            rel("{ from: s, read: { json: $ }, fields: { f: { source: value, redact: true } } }"),
        ),
        (
            "a field that is a list",
            rel("{ from: s, read: { json: $ }, fields: { f: [value] } }"),
        ),
        (
            "`entry.*` on `read:` rows",
            rel("{ from: s, read: { json: $ }, fields: { f: entry.key } }"),
        ),
        (
            "`value` on `entries:` rows",
            rel("{ from: s, entries: { format: json, at: $ }, fields: { f: value } }"),
        ),
        (
            "a side outside a join",
            rel("{ from: s, read: { json: $ }, fields: { f: left.f } }"),
        ),
        (
            "a side in a union",
            rel(
                "{ union: [{ from: s, read: { json: $ } }, { from: t, read: { json: $ } }], \
                    fields: { f: left.f } }",
            ),
        ),
        (
            "a side with a chained field",
            rel(
                "{ join: { left: a, right: b, on: { k: k }, unique: left }, fields: { f: \
                    left.a.b } }",
            ),
        ),
        (
            "a path token in a join",
            rel("{ join: { left: a, right: b, on: { k: k }, unique: left }, fields: { f: path } }"),
        ),
        (
            "a field name with a dot",
            rel("{ from: s, read: { json: $ }, fields: { a-b.c: value } }"),
        ),
        // `transform:`.
        (
            "`cldr-plurals` as a transform",
            read_rel(", transform: { f: [cldr-plurals] }"),
        ),
        (
            "an unknown transform",
            read_rel(", transform: { f: [trim, upper] }"),
        ),
        (
            "an empty transform list",
            read_rel(", transform: { f: [] }"),
        ),
        (
            "a transform that is not in a list",
            read_rel(", transform: { f: trim }"),
        ),
        (
            "`transform:` that is a list",
            read_rel(", transform: [trim]"),
        ),
        (
            "a transformed field name with a dot",
            read_rel(", transform: { a.b: [trim] }"),
        ),
        // `where:`.
        (
            "an unknown predicate",
            read_rel(", where: { f: { equals: x } }"),
        ),
        (
            "a filter without a predicate",
            read_rel(", where: { f: {} }"),
        ),
        (
            "an empty `in:` list",
            read_rel(", where: { f: { in: [] } }"),
        ),
        (
            "`not_in:` that is a string",
            read_rel(", where: { f: { not_in: x } }"),
        ),
        (
            "a list in an `in:` list",
            read_rel(", where: { f: { in: [[a]] } }"),
        ),
        (
            "a membership reference without `field:`",
            read_rel(", where: { f: { in: [{ relation: r }] } }"),
        ),
        (
            "an unknown membership reference key",
            read_rel(", where: { f: { in: [{ relation: r, field: f, where: x }] } }"),
        ),
        (
            "a membership reference with a chained relation",
            read_rel(", where: { f: { in: [{ relation: a.b.c, field: f }] } }"),
        ),
        (
            "a membership reference to a field with a dot",
            read_rel(", where: { f: { in: [{ relation: r, field: a.b }] } }"),
        ),
        (
            "`matches:` that is a list",
            read_rel(", where: { f: { matches: [x] } }"),
        ),
        ("`where:` that is a list", read_rel(", where: [f]")),
    ];
    for format in ["dotenv", "properties", "xml", "ini", "hcl"] {
        configs.push((
            "a `read:` format a relation cannot read",
            rel(&format!(
                "{{ from: s, read: {{ {format}: KEY }}, fields: {{ f: value }} }}"
            )),
        ));
    }
    configs
}

#[test]
fn the_schema_refuses_what_the_decode_refuses_for_its_shape() {
    for (what, config) in shape_refusals() {
        let decoded = decode_stage(&config);
        assert!(decoded.is_err(), "{what}: the decode accepted\n{config}");
        assert!(
            loaded(&config).is_err(),
            "{what}: the loader accepted\n{config}"
        );
        assert!(
            !schema_errors(&json_of(&config)).is_empty(),
            "{what}: the draft schema accepts what the decode refuses ({})\n{config}",
            decoded.unwrap_err()
        );
    }
}

/// Configs the decode refuses that JSON Schema cannot tell from valid ones,
/// each with what the schema cannot see. The version 2 docs page lists these.
fn unexpressed() -> Vec<(&'static str, String)> {
    let mut configs = vec![
        (
            "a name declared twice across selectors, relations and aliases",
            doc(
                "selectors:\n  x: { paths: a }\nrelations:\n  x: { from: x, read: { whole_file: \
                    {} }, fields: { f: value } }\n",
            ),
        ),
        (
            "a name declared twice across selectors, relations and aliases",
            doc("extends:\n  - { url: ./base.yml, as: x }\nselectors:\n  x: { paths: a }\n"),
        ),
        (
            "a name declared twice across selectors, relations and aliases",
            doc("extends:\n  - { url: ./base.yml, as: a }\n  - { url: ./base.yml, as: a }\n"),
        ),
        (
            "a regex that does not compile",
            read_rel(", where: { f: { matches: '(' } }"),
        ),
        (
            "a regex that does not compile",
            rel("{ from: s, read: { regex: '(' }, fields: { f: value } }"),
        ),
        (
            "a `read:` regex without a capture group",
            rel("{ from: s, read: { regex: '^v.*' }, fields: { f: value } }"),
        ),
        (
            "a JSONPath that does not parse",
            rel("{ from: s, read: { json: '$.a[' }, fields: { f: value } }"),
        ),
        (
            "a JSONPath that does not parse",
            rel("{ from: s, entries: { format: json, at: '$.a[' }, fields: { f: entry.key } }"),
        ),
        (
            "a JSONPath that does not parse",
            rel("{ from: s, read: { json: $ }, fields: { f: 'bound: $.a[' } }"),
        ),
    ];
    // JSON has no key but a string, so an editor reads `true:` as `"true":`
    // and the schema sees a valid name; the decode refuses a key YAML reads
    // as a boolean or null.
    for key in ["true", "false", "null"] {
        configs.push((
            "a map key YAML reads as a boolean or null",
            doc(&format!("selectors:\n  {key}: {{ paths: x }}\n")),
        ));
        configs.push((
            "a map key YAML reads as a boolean or null",
            read_rel(&format!(
                ", where: {{ f: {{ in: [a] }}, {key}: {{ in: [a] }} }}"
            )),
        ));
    }
    configs
}

/// `yaml` as an editor reads it, where JSON has no key but a string: each
/// boolean or null key written as its text.
fn as_an_editor_reads(yaml: &str) -> Json {
    let mut text = yaml.to_string();
    for key in ["true", "false", "null"] {
        text = text
            .replace(&format!("\n  {key}:"), &format!("\n  '{key}':"))
            .replace(&format!(", {key}:"), &format!(", '{key}':"));
    }
    json_of(&text)
}

#[test]
fn the_schema_accepts_what_it_cannot_tell_from_a_valid_config() {
    for (what, config) in unexpressed() {
        assert!(
            decode_stage(&config).is_err(),
            "{what}: the decode accepted\n{config}"
        );
        assert!(
            loaded(&config).is_err(),
            "{what}: the loader accepted\n{config}"
        );
        let errors = schema_errors(&as_an_editor_reads(&config));
        assert!(
            errors.is_empty(),
            "{what}: the draft schema now refuses this; move it to `shape_refusals` and the \
             docs page's list of what the schema cannot check\n{config}\n{errors:?}"
        );
    }
}

/// `config` with the selectors `s` and `t` declared, unless it declares
/// selectors of its own.
fn declared(config: String) -> String {
    if config.contains("\nselectors:") {
        return config;
    }
    config.replacen(
        "version: 2\n",
        "version: 2\nselectors:\n  s: { paths: s.json }\n  t: { paths: t.json }\n",
        1,
    )
}

/// Configs the decode accepts and the loader refuses once every source has
/// loaded: resolution and the plan check, which are the loader's alone. Each
/// comes with a part of the loader's error, so it is refused for that.
#[allow(clippy::too_many_lines)] // A table of cases.
fn loader_refusals() -> Vec<(&'static str, String, &'static str)> {
    let a = "  a: { from: { paths: a.json }, read: { json: $.name }, fields: { name: value } }\n";
    let join = |body: &str| doc(&format!("relations:\n{a}  r: {{ join: {body} }}\n"));
    let configs = vec![
        (
            "an unknown selector",
            rel("{ from: doc, read: { json: $.a }, fields: { f: value } }"),
            "no selector named `doc`",
        ),
        (
            "an unknown alias",
            doc(
                "extends:\n  - { url: ./base.yml, as: base }\nrelations:\n  r: { join: { left: \
                 base.pins, right: bsae.pins, on: { name: name }, unique: right }, fields: { \
                 f: left.name } }\n",
            ),
            "no `extends:` alias `bsae`",
        ),
        (
            "a qualified name the aliased config does not declare",
            doc(
                "extends:\n  - { url: ./base.yml, as: base }\nrelations:\n  r: { from: \
                 base.manifests, read: { json: $.a }, fields: { f: value } }\n",
            ),
            "declares no selector named `manifests`",
        ),
        (
            "a selector where a relation goes",
            doc(&format!(
                "selectors:\n  s: {{ paths: x }}\nrelations:\n{a}  r: {{ join: {{ left: s, \
                 right: a, on: {{ name: name }}, unique: right }}, fields: {{ f: left.name \
                 }} }}\n"
            )),
            "`s` is a selector",
        ),
        (
            "a cycle",
            doc(&format!(
                "relations:\n{a}  l: {{ join: {{ left: r, right: a, on: {{ name: name }}, \
                 unique: right }}, fields: {{ name: left.name }} }}\n  r: {{ join: {{ left: \
                 l, right: a, on: {{ name: name }}, unique: right }}, fields: {{ name: \
                 left.name }} }}\n"
            )),
            "reads itself",
        ),
        (
            "a union part that cannot bind a field",
            rel(
                "{ union: [{ from: s, read: { json: $ } }, { from: t, entries: { format: json, \
                 at: $ } }], fields: { f: value } }",
            ),
            "`value`",
        ),
        (
            "a join on a field a side lacks",
            join(
                "{ left: a, right: a, on: { nope: name }, unique: right }, fields: { f: left.name }",
            ),
            "`nope` is not a field of `a`",
        ),
        (
            "a side field the side lacks",
            join(
                "{ left: a, right: a, on: { name: name }, unique: right }, fields: { f: left.nope }",
            ),
            "`left.nope` names no field of `a`",
        ),
        (
            "a transform of a missing field",
            read_rel(", transform: { g: [trim] }"),
            "`relations.r.transform.g`",
        ),
        (
            "a filter on a missing field",
            read_rel(", where: { g: { matches: x } }"),
            "`relations.r.where.g`",
        ),
        (
            "a membership reference to a missing field",
            doc(&format!(
                "relations:\n{a}  r: {{ from: s, read: {{ json: $.a }}, fields: {{ f: value }}, \
                 where: {{ f: {{ in: [{{ relation: a, field: nope }}] }} }} }}\n"
            )),
            "`relations.r.where.f.in[0]",
        ),
        (
            "`strip-root` off an `entry.path`",
            read_rel(", transform: { f: [strip-root] }"),
            "strip-root",
        ),
        (
            "`strip-root` after another transform",
            rel(
                "{ from: s, entries: { format: json, at: $, leaves: true }, fields: { k: \
                 entry.path }, transform: { k: [trim, strip-root] } }",
            ),
            "strip-root",
        ),
        (
            "a version transform on a path",
            rel(
                "{ from: s, read: { json: $ }, fields: { f: path }, transform: { f: \
                 [semver-major] } }",
            ),
            "`relations.r.transform.f",
        ),
        (
            "a null in an `in:` list",
            read_rel(", where: { f: { in: [a, null] } }"),
            "`relations.r.where.f.in[1]`",
        ),
        (
            "a number in a `not_in:` list",
            read_rel(", where: { f: { not_in: [3] } }"),
            "`relations.r.where.f.not_in[0]`",
        ),
        (
            "a boolean for a field that holds strings",
            read_rel(", where: { f: { in: [true] } }"),
            "a boolean never equals a string",
        ),
        (
            "a selector manifest outside the repository",
            sel(
                "{ paths: x, scope_filter: { include_manifest_paths: { source: ../m.json, \
                 extract: { json: '$[*]' } } } }",
            ),
            "must be a repo-root-relative path",
        ),
    ];
    configs
        .into_iter()
        .map(|(what, config, error)| (what, declared(config), error))
        .collect()
}

#[test]
fn the_schema_leaves_resolution_and_the_plan_check_to_the_loader() {
    for (what, config, expected) in loader_refusals() {
        // The decode accepts it, so `schema_parity::assert_accepts` has
        // checked the schema accepts it too.
        decode_stage(&config).unwrap_or_else(|error| panic!("{what}: {error}\n{config}"));
        let error = loaded(&config).expect_err(what).to_string();
        assert!(
            error.contains(expected),
            "{what}: expected `{expected}` in\n{error}\n{config}"
        );
        assert!(
            schema_errors(&json_of(&config)).is_empty(),
            "{what}\n{config}"
        );
    }
}

#[test]
fn section_twelves_policies_validate_and_load() {
    for policy in [
        version_two_rows::DEPENDENCY_RANGES,
        version_two_rows::LOCALE_KEYS,
        version_two_rows::NODE_PINS,
    ] {
        let config = format!("{policy}rules: []\n");
        assert!(loaded(&config).is_ok(), "{config}");
        let errors = schema_errors(&json_of(&config));
        assert!(errors.is_empty(), "{config}\n{errors:?}");
    }
}

#[test]
fn a_config_the_loader_accepts_with_every_form_validates() {
    let tmp = tempfile::tempdir().unwrap();
    let config = "version: 2\nextends:\n  - ./base.yml\n  - { url: ./base.yml, as: base \
        }\nselectors:\n  manifests:\n    paths: { include: \"**/package.json\", \
                  exclude: [vendor/**] }\n    scope_filter: { has_ancestor: [package.json] }\n\
                  relations:\n  deps:\n    from: manifests\n    entries: { format: json, at: \
                  [\"$.dependencies\"], leaves: false }\n    fields: { name: entry.key, range: \
                  { source: entry.value, sensitive: true }, file: path, owner: 'bound: $.name' \
                  }\n    transform: { range: [trim, semver-major] }\n    where:\n      name: { \
                  not_in: [left-pad, { relation: base.pins, field: name }], matches: '^a', \
                  not_matches: 'z$' }\n  lines:\n    from: { paths: .nvmrc }\n    read: { lines: \
                  { comment: '//' } }\n    optional: true\n    fields: { v: value }\n  \
                  pins:\n    union:\n      - { from: manifests, read: { regex: '\"v\": \"(.*)\"' } \
                  }\n      - { from: { paths: x.toml }, read: { toml: $.v }, optional: true \
                  }\n      \
                  - { from: { paths: x.yml }, read: { whole_file: {} } }\n    fields: { v: value \
                  }\n  paired:\n    join: { left: deps, right: base.pins, on: { name: name }, \
                  unique: right }\n    fields: { name: left.name, wanted: { source: \
                  right.version, sensitive: true } }\nrules: []\n";
    std::fs::write(tmp.path().join("base.yml"), BASE).unwrap();
    std::fs::write(tmp.path().join(".alint.yml"), config).unwrap();
    load(&tmp.path().join(".alint.yml")).unwrap_or_else(|error| panic!("{error}"));
    let errors = schema_errors(&json_of(config));
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn an_empty_rule_list_on_an_extends_entry_is_refused_by_both() {
    // The decode leaves `only:` and `except:` to the loader, which refuses an
    // empty list when it resolves the entry, as the schema of either version
    // does.
    for list in ["only", "except"] {
        let config = doc(&format!(
            "extends:\n  - {{ url: ./base.yml, as: base, {list}: [] }}\n"
        ));
        decode_stage(&config).unwrap();
        assert_eq!(
            loaded(&config).unwrap_err().to_string(),
            format!(
                "`extends:` entry \"./base.yml\": `{list}:` is empty; list at least one rule id"
            )
        );
        assert!(!schema_errors(&json_of(&config)).is_empty(), "{config}");
        let errors = schema_errors(&json_of(&config.replace("[]", "[some-rule]")));
        assert!(errors.is_empty(), "{errors:?}");
        let one = format!("version: 1\nextends:\n  - {{ url: ./base.yml, {list}: [] }}\n");
        assert!(!jsonschema::is_valid(
            &serde_json::from_str(crate::CONFIG_SCHEMA_V1).unwrap(),
            &json_of(&one)
        ));
    }
}
