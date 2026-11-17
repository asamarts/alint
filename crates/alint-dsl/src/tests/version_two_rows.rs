//! Version 2 relations evaluated over walked trees: a real configuration
//! through the loader, a tree on disk, and the evaluator phase 7's
//! `constraint` kind calls (design `v0.23/selectors-and-constraints.md`,
//! sections 5 to 8 and 12). No rule reads a relation in this build, so these
//! call the evaluator directly.

use std::path::Path;

use alint_core::relation::Value;
use alint_core::relation::eval::{self, Outcome, RelationRows};
use alint_core::relation::plan::RelationId;
use alint_core::{Context, FileIndex, WalkOptions, walk};

use super::*;

fn write(root: &Path, relative: &str, body: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, body).unwrap();
}

/// A configuration loaded from a tree, and the tree walked.
struct Tree {
    dir: tempfile::TempDir,
    config: Config,
    index: FileIndex,
}

fn setup(config: &str, files: &[(&str, &str)]) -> Tree {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), ".alint.yml", config);
    for (path, body) in files {
        write(dir.path(), path, body);
    }
    let config = load(&dir.path().join(".alint.yml")).expect("the configuration loads");
    let index = walk(dir.path(), &WalkOptions::default()).unwrap();
    Tree { dir, config, index }
}

impl Tree {
    fn evaluate(&self, relation: &str) -> Outcome {
        let plan = self
            .config
            .relation_plan
            .as_ref()
            .expect("a version 2 plan");
        let id = plan
            .relations
            .iter()
            .position(|named| named.name == relation)
            .expect("a declared relation");
        eval::evaluate(
            plan,
            RelationId(id),
            &Context::new(self.dir.path(), &self.index),
        )
    }

    fn rows(&self, relation: &str) -> std::sync::Arc<RelationRows> {
        match self.evaluate(relation) {
            Ok(rows) => rows,
            Err(failures) => panic!("{relation} failed: {failures:?}"),
        }
    }

    fn failures(&self, relation: &str) -> Vec<String> {
        self.evaluate(relation)
            .expect_err("the relation fails")
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    /// The text at `span` in `path`.
    fn text(&self, path: &str, span: alint_core::ByteSpan) -> String {
        let text = std::fs::read_to_string(self.dir.path().join(path)).unwrap();
        text[span.as_range()].to_string()
    }
}

/// Each row's cells as text: a string as itself, a number as its lexeme,
/// `null`, `true`/`false`, and `<missing>` for a missing value.
fn table(rows: &RelationRows) -> Vec<Vec<String>> {
    rows.rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| match &cell.value {
                    None => "<missing>".to_string(),
                    Some(Value::String(text)) => text.to_string(),
                    Some(Value::Number(number)) => number.lexeme().to_string(),
                    Some(Value::Bool(flag)) => flag.to_string(),
                    Some(Value::Null) => "null".to_string(),
                    Some(other) => format!("{other:?}"),
                })
                .collect()
        })
        .collect()
}

fn strings(rows: &[&[&str]]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|row| row.iter().map(ToString::to_string).collect())
        .collect()
}

// Section 12's three policies, as written there less their `constraint`
// rules (phase 7).

pub(super) const DEPENDENCY_RANGES: &str = r#"version: 2
selectors:
  workspace-manifests:
    # yarnpkg-berry-01 leaves packages/docusaurus out
    paths: ["packages/*/package.json", "!packages/docusaurus/package.json"]
relations:
  workspace-names:
    from: workspace-manifests
    read: { json: "$.name" }
    fields: { name: value }
  dependency-ranges:
    from: workspace-manifests
    entries:
      format: json
      at: ["$.dependencies", "$.devDependencies", "$.optionalDependencies"]
    fields: { field: entry.parent, dependency: entry.key, range: entry.value }
    where:
      # trpc-05 skips these specifiers
      range: { not_matches: "^(catalog|npm|workspace):" }
      # jest-23 exempts @types/node; yarnpkg-berry-01 exempts workspace packages
      dependency: { not_in: ["@types/node", { relation: workspace-names, field: name }] }
"#;

const WORKSPACE: &[(&str, &str)] = &[
    (
        "packages/a/package.json",
        r#"{"name": "a", "devDependencies": {"@types/node": "^20", "jest": "^29"},
            "dependencies": {"b": "workspace:*", "react": "^18.2.0"}}"#,
    ),
    (
        "packages/b/package.json",
        r#"{"name": "b", "dependencies": {"react": "^18.3.0", "a": "^1.0.0"},
            "optionalDependencies": {"fsevents": "2.3.3"}}"#,
    ),
    (
        "packages/docusaurus/package.json",
        r#"{"name": "docs", "dependencies": {"react": "^17"}}"#,
    ),
];

#[test]
fn dependency_ranges_filter_by_pattern_and_workspace_membership() {
    let tree = setup(DEPENDENCY_RANGES, WORKSPACE);
    assert_eq!(
        table(&tree.rows("workspace-names")),
        strings(&[&["a"], &["b"]])
    );
    let ranges = tree.rows("dependency-ranges");
    assert_eq!(
        ranges
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["field", "dependency", "range"]
    );
    // `at:` paths in the order written, within each manifest in path order.
    assert_eq!(
        table(&ranges),
        strings(&[
            &["dependencies", "react", "^18.2.0"],
            &["devDependencies", "jest", "^29"],
            &["dependencies", "react", "^18.3.0"],
            &["optionalDependencies", "fsevents", "2.3.3"],
        ])
    );
    let resources: Vec<&str> = ranges.resources.iter().map(|r| r.path().as_str()).collect();
    assert_eq!(
        resources,
        ["packages/a/package.json", "packages/b/package.json"]
    );

    // Each cell keeps its resource, key path and span: the key's for a key,
    // the value's for a value.
    let row = &ranges.rows[0].cells;
    let manifest = "packages/a/package.json";
    assert_eq!(row[1].at.resource.path().as_str(), manifest);
    assert_eq!(
        row[1]
            .at
            .pointer
            .as_ref()
            .map(alint_core::relation::Pointer::render),
        Some("dependencies.react".into())
    );
    assert_eq!(tree.text(manifest, row[1].span.unwrap()), "\"react\"");
    assert_eq!(tree.text(manifest, row[2].span.unwrap()), "\"^18.2.0\"");
    assert_eq!(tree.text(manifest, row[0].span.unwrap()), "\"react\"");
}

#[test]
fn an_in_list_keeps_the_rows_another_relation_holds() {
    let config = DEPENDENCY_RANGES.replace(
        r#"      range: { not_matches: "^(catalog|npm|workspace):" }
      # jest-23 exempts @types/node; yarnpkg-berry-01 exempts workspace packages
      dependency: { not_in: ["@types/node", { relation: workspace-names, field: name }] }"#,
        "      dependency: { in: [{ relation: workspace-names, field: name }] }",
    );
    let tree = setup(&config, WORKSPACE);
    assert_eq!(
        table(&tree.rows("dependency-ranges")),
        strings(&[
            &["dependencies", "b", "workspace:*"],
            &["dependencies", "a", "^1.0.0"],
        ])
    );
}

pub(super) const LOCALE_KEYS: &str = r#"version: 2
selectors:
  locale-files: { paths: "config/locales/**/*.yml" }
relations:
  locale-keys:
    from: locale-files
    entries: { format: yaml, at: "$", leaves: true }
    fields: { locale: entry.root, key: entry.path }
    transform: { key: [strip-root] }
    where: { key: { not_matches: "^(date|devise|views)\\." } }
"#;

#[test]
fn locale_keys_strip_the_locale_and_read_past_arrays() {
    let tree = setup(
        LOCALE_KEYS,
        &[
            (
                "config/locales/en.yml",
                "en:\n  nav:\n    home: Home\n    about: About\n  date:\n    day_names: [Mo, Tu]\n  \
                 greeting: Hi\n",
            ),
            (
                "config/locales/fr.yml",
                "fr:\n  views:\n    x: y\n  nav:\n    home: Accueil\n",
            ),
        ],
    );
    let keys = tree.rows("locale-keys");
    assert_eq!(
        table(&keys),
        strings(&[
            &["en", "nav.home"],
            &["en", "nav.about"],
            &["en", "greeting"],
            &["fr", "nav.home"],
        ])
    );
    // The stripped path keeps its segments, and the cell its key's span.
    let cell = &keys.rows[3].cells[1];
    let segments = cell.path.as_ref().unwrap().segments().len();
    assert_eq!(segments, 2);
    assert_eq!(
        tree.text("config/locales/fr.yml", cell.span.unwrap()),
        "home"
    );
}

#[test]
fn strip_root_needs_one_top_level_key_per_resource() {
    let tree = setup(
        LOCALE_KEYS,
        &[("config/locales/all.yml", "en:\n  a: x\nfr:\n  a: y\n")],
    );
    assert_eq!(
        tree.failures("locale-keys"),
        [
            "config/locales/all.yml: relation `locale-keys` (.alint.yml): `strip-root` on `key` \
          needs one top-level key in each resource, but this one has `en`, `fr`; read one \
          top-level object per file, or drop `strip-root`"
        ]
    );
    let flat = tree.dir.path().join("config/locales/all.yml");
    std::fs::write(flat, "greeting: Hi\n").unwrap();
    let tree = Tree {
        index: walk(tree.dir.path(), &WalkOptions::default()).unwrap(),
        ..tree
    };
    assert_eq!(
        tree.failures("locale-keys"),
        [
            "config/locales/all.yml: relation `locale-keys` (.alint.yml): `strip-root` on `key` \
          needs every key path to have at least two segments, but `greeting` has one; read the \
          keys below the top-level key, or drop `strip-root`"
        ]
    );
}

pub(super) const NODE_PINS: &str = r#"version: 2
relations:
  node-pins:
    union:
      - { from: { paths: .nvmrc }, read: { regex: "^v?(\\S+)" } }
      - { from: { paths: package.json }, read: { json: "$.engines.node" } }
      - { from: { paths: package.json }, read: { json: "$.devEngines.runtime.version" } }
      - { from: { paths: package.json }, read: { json: "$.volta.node" } }
    fields: { source: path, version: value }
"#;

const PINNED: &[(&str, &str)] = &[
    (".nvmrc", "v20.11.1\n"),
    (
        "package.json",
        r#"{"engines": {"node": ">=20.11.1"}, "volta": {"node": "20.11.1"}}"#,
    ),
];

#[test]
fn toolchain_pins_union_a_regex_read_and_json_reads_with_a_missing_row() {
    let tree = setup(NODE_PINS, PINNED);
    let pins = tree.rows("node-pins");
    assert_eq!(
        table(&pins),
        strings(&[
            &[".nvmrc", "20.11.1"],
            &["package.json", ">=20.11.1"],
            &["package.json", "<missing>"],
            &["package.json", "20.11.1"],
        ])
    );
    // A `read:` row has no key path; its resource is its provenance.
    let missing = &pins.rows[2].cells[1];
    assert_eq!(missing.at.pointer, None);
    assert_eq!(missing.at.resource.path().as_str(), "package.json");
}

#[test]
fn an_optional_part_that_matches_nothing_gives_no_row() {
    let config = NODE_PINS.replace(
        r#"read: { json: "$.devEngines.runtime.version" } }"#,
        r#"read: { json: "$.devEngines.runtime.version" }, optional: true }"#,
    );
    let tree = setup(&config, PINNED);
    assert_eq!(
        table(&tree.rows("node-pins")),
        strings(&[
            &[".nvmrc", "20.11.1"],
            &["package.json", ">=20.11.1"],
            &["package.json", "20.11.1"],
        ])
    );
}

#[test]
fn a_union_keeps_the_order_its_parts_are_written_in() {
    let tree = setup(
        r#"version: 2
relations:
  pins:
    union:
      - { from: { paths: package.json }, read: { json: "$.volta.node" } }
      - { from: { paths: .nvmrc }, read: { lines: {} } }
    fields: { source: path, version: value }
"#,
        PINNED,
    );
    assert_eq!(
        table(&tree.rows("pins")),
        strings(&[&["package.json", "20.11.1"], &[".nvmrc", "v20.11.1"]])
    );
}

// What a relation cannot hold is an analysis failure, never a silent row.

fn relation(body: &str) -> String {
    format!("version: 2\nrelations:\n{body}")
}

const DEPS_AT: &str = r#"  deps:
    from: { paths: package.json }
    entries: { format: json, at: "$.deps" }
"#;

#[test]
fn an_at_path_that_selects_an_array_fails() {
    let tree = setup(
        &relation(&format!("{DEPS_AT}    fields: {{ name: entry.key }}\n")),
        &[("package.json", r#"{"deps": ["a"]}"#)],
    );
    assert_eq!(
        tree.failures("deps"),
        [
            "package.json: relation `deps` (.alint.yml): `at: $.deps` selects an array at `deps`; \
          `at:` selects objects, whose members become rows, so select an object (read an \
          array's or a scalar's values with `read:`)"
        ]
    );
}

#[test]
fn an_entry_value_that_is_an_object_fails_and_a_key_alone_reads() {
    let files = &[("package.json", r#"{"deps": {"a": {"v": 1}, "b": "1"}}"#)];
    let keys = setup(
        &relation(&format!("{DEPS_AT}    fields: {{ name: entry.key }}\n")),
        files,
    );
    assert_eq!(table(&keys.rows("deps")), strings(&[&["a"], &["b"]]));
    let values = setup(
        &relation(&format!(
            "{DEPS_AT}    fields: {{ name: entry.key, version: entry.value }}\n"
        )),
        files,
    );
    assert_eq!(
        values.failures("deps"),
        [
            "package.json: relation `deps` (.alint.yml): `version` binds `entry.value`, which is an \
          object at `deps.a`; a field holds a scalar (null, a boolean, a number or a string), so \
          select the members below it with `at:`, or bind only `entry.key`"
        ]
    );
}

#[test]
fn a_toml_datetime_value_fails_and_numbers_keep_their_lexemes() {
    let config = relation(
        r#"  meta:
    from: { paths: Cargo.toml }
    entries: { format: toml, at: "$.package.metadata" }
    fields: { key: entry.key, value: entry.value }
"#,
    );
    let tree = setup(
        &config,
        &[(
            "Cargo.toml",
            "[package.metadata]\nratio = 1.10\nhex = 0x1F\nreleased = 1979-05-27\n",
        )],
    );
    assert_eq!(
        tree.failures("meta"),
        [
            "Cargo.toml: relation `meta` (.alint.yml): `value` binds `entry.value`, which is a TOML \
          date or time at `package.metadata.released`; a field holds a scalar (null, a boolean, \
          a number or a string), so select the members below it with `at:`, or bind only \
          `entry.key`"
        ]
    );
    std::fs::write(
        tree.dir.path().join("Cargo.toml"),
        "[package.metadata]\nratio = 1.10\nhex = 0x1F\n",
    )
    .unwrap();
    let tree = Tree {
        index: walk(tree.dir.path(), &WalkOptions::default()).unwrap(),
        ..tree
    };
    assert_eq!(
        table(&tree.rows("meta")),
        strings(&[&["ratio", "1.10"], &["hex", "0x1F"]])
    );
}

#[test]
fn a_bound_path_must_match_once_in_each_resource() {
    let config = relation(
        r#"  owned:
    from: { paths: "*.yaml" }
    entries: { format: yaml, at: "$.deps" }
    fields: { owner: "bound: $.name", dependency: entry.key }
"#,
    );
    let tree = setup(
        &config,
        &[
            ("a.yaml", "name: a\ndeps:\n  x: '1'\n"),
            ("b.yaml", "deps:\n  y: '1'\n"),
        ],
    );
    assert_eq!(
        tree.failures("owned"),
        [
            "b.yaml: relation `owned` (.alint.yml): `owner` (`bound: $.name`) matches nothing; a \
          bound path matches exactly once in each resource the relation reads, so fix the path, \
          or narrow the selector to the files that have it"
        ]
    );
    let twice = setup(
        &config.replace("$.name", "$.names[*]"),
        &[("a.yaml", "names: [a, b]\ndeps:\n  x: '1'\n")],
    );
    assert_eq!(
        twice.failures("owned"),
        [
            "a.yaml: relation `owned` (.alint.yml): `owner` (`bound: $.names[*]`) matches more than \
          once (at `names[0]` and at `names[1]`); a bound path matches exactly once in each \
          resource the relation reads, so make it select one value"
        ]
    );
    let once = setup(
        &config,
        &[("a.yaml", "name: a\ndeps:\n  x: '1'\n  y: '2'\n")],
    );
    let rows = once.rows("owned");
    assert_eq!(table(&rows), strings(&[&["a", "x"], &["a", "y"]]));
    // A bound value's span is its value's.
    assert_eq!(
        once.text("a.yaml", rows.rows[1].cells[0].span.unwrap()),
        "a"
    );
}

const LINKED: &str = r#"version: 2
relations:
  names:
    from: { paths: "*/package.json" }
    read: { json: "$.name" }
    fields: { name: value, manifest: path }
  deps:
    from: { paths: "*/package.json" }
    entries: { format: json, at: "$.dependencies" }
    fields: { dependency: entry.key, range: entry.value }
  linked:
    join: { left: deps, right: names, on: { dependency: name }, unique: right }
    fields: { dependency: left.dependency, range: left.range, manifest: right.manifest }
"#;

#[test]
fn a_join_follows_its_non_unique_side_and_keeps_provenance() {
    let tree = setup(
        LINKED,
        &[
            (
                "a/package.json",
                r#"{"name": "a", "dependencies": {"b": "^1", "react": "^18", "c": "^3"}}"#,
            ),
            (
                "b/package.json",
                r#"{"name": "b", "dependencies": {"a": "^2"}}"#,
            ),
            ("c/package.json", r#"{"name": "c"}"#),
        ],
    );
    let linked = tree.rows("linked");
    assert_eq!(
        table(&linked),
        strings(&[
            &["b", "^1", "b/package.json"],
            &["c", "^3", "c/package.json"],
            &["a", "^2", "a/package.json"],
        ])
    );
    let row = &linked.rows[0].cells;
    assert_eq!(row[0].at.resource.path().as_str(), "a/package.json");
    assert_eq!(tree.text("a/package.json", row[1].span.unwrap()), "\"^1\"");
    assert_eq!(row[2].at.resource.path().as_str(), "b/package.json");
    assert_eq!(row[2].at.pointer, None);

    // With the other side unique, the order is the names'.
    let flipped = LINKED.replace(
        "join: { left: deps, right: names, on: { dependency: name }, unique: right }",
        "join: { left: names, right: deps, on: { name: dependency }, unique: left }",
    );
    let flipped = flipped.replace(
        "fields: { dependency: left.dependency, range: left.range, manifest: right.manifest }",
        "fields: { manifest: left.manifest, range: right.range }",
    );
    let tree = Tree {
        config: {
            write(tree.dir.path(), ".alint.yml", &flipped);
            load(&tree.dir.path().join(".alint.yml")).unwrap()
        },
        ..tree
    };
    let flipped = tree.rows("linked");
    assert_eq!(
        table(&flipped),
        strings(&[
            &["b/package.json", "^1"],
            &["c/package.json", "^3"],
            &["a/package.json", "^2"],
        ])
    );
    let resources: Vec<&str> = flipped
        .resources
        .iter()
        .map(|r| r.path().as_str())
        .collect();
    assert_eq!(
        resources,
        ["a/package.json", "b/package.json", "c/package.json"]
    );
}

#[test]
fn a_repeated_key_on_the_unique_side_fails_naming_both_rows() {
    let tree = setup(
        LINKED,
        &[
            (
                "a/package.json",
                r#"{"name": "x", "dependencies": {"x": "^1"}}"#,
            ),
            ("b/package.json", r#"{"name": "x"}"#),
        ],
    );
    assert_eq!(
        tree.failures("linked"),
        [
            "b/package.json: relation `linked` (.alint.yml): the join's unique side (right, \
          `names`) repeats the key \"x\": a/package.json and b/package.json; `unique: right` \
          needs each key once on that side, so make the key unique there, or name the other \
          side unique"
        ]
    );
}

#[test]
fn a_join_key_of_two_types_fails_naming_both_rows() {
    let config = LINKED.replace("on: { dependency: name }", "on: { range: name }");
    let tree = setup(
        &config,
        &[(
            "a/package.json",
            r#"{"name": "a", "dependencies": {"x": 1}}"#,
        )],
    );
    assert_eq!(
        tree.failures("linked"),
        [
            "a/package.json: relation `linked` (.alint.yml): the join compares `range`, a number at \
          `dependencies.x` in a/package.json, with `name`, a string in a/package.json; join keys \
          compare within one type, so transform one side, or join on fields of one type"
        ]
    );
}

#[test]
fn a_string_transform_or_pattern_on_a_number_fails() {
    let files = &[("package.json", r#"{"deps": {"a": 1}}"#)];
    let transformed = setup(
        &relation(&format!(
            "{DEPS_AT}    fields: {{ name: entry.key, version: entry.value }}\n    \
             transform: {{ version: [trim] }}\n"
        )),
        files,
    );
    assert_eq!(
        transformed.failures("deps"),
        [
            "package.json: relation `deps` (.alint.yml): `transform.version` applies `trim` to a \
          number at `deps.a`, but the string transforms apply to strings; transform a string \
          field"
        ]
    );
    let matched = setup(
        &relation(&format!(
            "{DEPS_AT}    fields: {{ name: entry.key, version: entry.value }}\n    \
             where: {{ version: {{ matches: '^1' }} }}\n"
        )),
        files,
    );
    assert_eq!(
        matched.failures("deps"),
        [
            "package.json: relation `deps` (.alint.yml): `where.version.matches` tests a number at \
          `deps.a`, but a pattern tests strings; filter a string field, or transform this one"
        ]
    );
}

#[test]
fn membership_across_types_fails_and_numbers_compare_by_lexeme() {
    let files = &[("package.json", r#"{"deps": {"a": "1", "b": "2"}}"#)];
    let crossed = setup(
        &relation(&format!(
            "{DEPS_AT}    fields: {{ name: entry.key, version: entry.value }}\n    \
             where: {{ version: {{ in: ['1', true] }} }}\n"
        )),
        files,
    );
    assert_eq!(
        crossed.failures("deps"),
        [
            "package.json: relation `deps` (.alint.yml): `where.version.in` compares a string at \
          `deps.a` in package.json with the literal true; a value is in a list only beside values \
          of its own type, so compare values of one type (transform the field, or change the \
          list)"
        ]
    );
    let kept = setup(
        &relation(&format!(
            "{DEPS_AT}    fields: {{ name: entry.key, version: entry.value }}\n    \
             where: {{ version: {{ in: ['1'] }} }}\n"
        )),
        files,
    );
    assert_eq!(table(&kept.rows("deps")), strings(&[&["a", "1"]]));
}

// Sensitivity: a value never reaches a message when its field is sensitive
// or the environment wrote what selects it.

#[test]
fn a_sensitive_field_keeps_its_values_and_keys_out_of_messages() {
    let files = &[("package.json", r#"{"deps": {"a": "1"}}"#)];
    let value = setup(
        &relation(&format!(
            "{DEPS_AT}    fields: {{ name: entry.key, version: {{ source: entry.value, \
             sensitive: true }} }}\n    where: {{ version: {{ in: ['1', true] }} }}\n"
        )),
        files,
    );
    assert_eq!(
        value.failures("deps"),
        [
            "package.json: relation `deps` (.alint.yml): `where.version.in` compares a string at \
          `deps.a` in package.json with a literal boolean; a value is in a list only beside \
          values of its own type, so compare values of one type (transform the field, or change \
          the list)"
        ]
    );
    let key = setup(
        &relation(&format!(
            "{DEPS_AT}    fields: {{ name: {{ source: entry.key, sensitive: true }}, version: \
             entry.value }}\n    where: {{ version: {{ in: ['1', true] }} }}\n"
        )),
        files,
    );
    assert_eq!(
        key.failures("deps"),
        [
            "package.json: relation `deps` (.alint.yml): `where.version.in` compares a string at a \
          key path not shown (a field holding keys is sensitive) in package.json with the \
          literal true; a value is in a list only beside values of its own type, so compare \
          values of one type (transform the field, or change the list)"
        ]
    );
}

#[test]
fn a_filter_the_environment_writes_makes_its_field_sensitive() {
    let files = &[("package.json", r#"{"deps": {"a": "1"}}"#)];
    let tree = setup(
        &relation(&format!(
            "{DEPS_AT}    fields: {{ name: entry.key, version: entry.value }}\n    \
             where: {{ version: {{ not_in: ['{{{{env.ALINT_V2_ROWS_UNSET | default(\"nine\")}}}}'] \
             }} }}\n"
        )),
        files,
    );
    let rows = tree.rows("deps");
    let shown: Vec<(&str, bool)> = rows
        .fields
        .iter()
        .map(|field| (field.name.as_str(), field.sensitive))
        .collect();
    assert_eq!(shown, [("name", false), ("version", true)]);
}

#[test]
fn a_relation_whose_selection_the_environment_writes_shows_no_detail() {
    let tree = setup(
        &relation(
            "  deps:\n    from: { paths: package.json }\n    entries: { format: json, at: \
             '{{env.ALINT_V2_ROWS_UNSET | default(\"$.deps\")}}' }\n    fields: { name: \
             entry.key }\n",
        ),
        &[("package.json", r#"{"deps": ["a"]}"#)],
    );
    let failures = tree.evaluate("deps").expect_err("the relation fails");
    assert_eq!(
        failures.iter().map(ToString::to_string).collect::<Vec<_>>(),
        [
            "package.json: relation `deps` (.alint.yml): this relation's configuration reads the \
          environment, so the detail of its analysis failure is not shown"
        ]
    );
    assert!(failures[0].sensitive);
    let violation = failures[0].to_violation("constraint-rule");
    assert!(violation.message.starts_with("sensitive evidence "));
}

// A relation no rule reads costs nothing.

#[test]
fn relations_no_rule_reads_are_never_evaluated() {
    let rules = "rules:\n  - id: names\n    kind: file_content_matches\n    paths: \
                 \"packages/*/package.json\"\n    pattern: '\"name\"'\n    level: error\n";
    let run = |config: &str| {
        let tree = setup(config, WORKSPACE);
        let mut registry = alint_core::RuleRegistry::new();
        alint_rules::register_builtin(&mut registry);
        let entries = tree
            .config
            .rules
            .iter()
            .map(|spec| alint_core::RuleEntry::new(registry.build(spec).unwrap()))
            .collect();
        let engine = alint_core::Engine::from_entries(entries, registry);
        let report = engine.run(tree.dir.path(), &tree.index).unwrap();
        // No memo and no relation cache: the run evaluated, read and
        // decoded nothing for a relation.
        assert!(!tree.index.relation_memo_is_set());
        assert!(tree.index.read_cache().is_none());
        assert!(tree.index.decode_cache().is_none());
        report
            .results
            .iter()
            .map(|result| (result.rule_id.to_string(), result.violations.len()))
            .collect::<Vec<_>>()
    };
    let with_relations = run(&format!("{DEPENDENCY_RANGES}{rules}"));
    let version_one = run(&format!("version: 1\n{rules}"));
    assert_eq!(with_relations, version_one);
    assert_eq!(with_relations, [("names".to_string(), 0)]);
}

// A selector picks files as a rule's `paths:` and `scope_filter:` do.

const SCOPED: &str = r#"version: 2
selectors:
  members:
    paths: "**/package.json"
    scope_filter:
      include_manifest_paths:
        source: package.json
        extract: { json: "$.workspaces[*]" }
relations:
  member-names:
    from: members
    read: { json: "$.name" }
    fields: { name: value, manifest: path }
  marked:
    from: { paths: "**/*.json", scope_filter: { has_ancestor: marker.txt } }
    read: { whole_file: {} }
    fields: { file: path }
"#;

#[test]
fn selectors_apply_manifest_and_ancestor_predicates() {
    let tree = setup(
        SCOPED,
        &[
            (
                "package.json",
                r#"{"name": "root", "workspaces": ["packages/a"]}"#,
            ),
            ("packages/a/package.json", r#"{"name": "a"}"#),
            ("packages/b/package.json", r#"{"name": "b"}"#),
            ("x/marker.txt", ""),
            ("x/y/a.json", "{}"),
            ("z/b.json", "{}"),
        ],
    );
    assert_eq!(
        table(&tree.rows("member-names")),
        strings(&[&["a", "packages/a/package.json"]])
    );
    assert_eq!(table(&tree.rows("marked")), strings(&[&["x/y/a.json"]]));
}

#[test]
fn a_manifest_the_selector_cannot_read_fails() {
    let tree = setup(SCOPED, &[("package.json", r#"{"workspaces": "#)]);
    assert_eq!(
        tree.failures("member-names"),
        [
            "package.json: relation `member-names` (.alint.yml): selector `members`'s \
          `scope_filter:` reads `package.json`: scope manifest extract failed: EOF while parsing a \
          value at line 1 column 15; fix `package.json`, or change the selector's \
          `scope_filter:`"
        ]
    );
}

#[test]
fn the_plan_records_what_the_environment_writes() {
    let env = |default: &str| format!("'{{{{env.ALINT_V2_ROWS_UNSET | default(\"{default}\")}}}}'");
    let config = format!(
        "version: 2\nselectors:\n  docs: {{ paths: {} }}\n  plain: {{ paths: x }}\nrelations:\n  \
         by-selector:\n    from: docs\n    read: {{ whole_file: {{}} }}\n    fields: {{ f: path }}\n  \
         by-read:\n    from: plain\n    read: {{ regex: {} }}\n    fields: {{ f: value }}\n  \
         by-filter:\n    from: plain\n    read: {{ whole_file: {{}} }}\n    fields: {{ f: value, g: \
         path }}\n    where: {{ f: {{ in: [{}] }} }}\n  untouched:\n    from: plain\n    read: {{ \
         whole_file: {{}} }}\n    fields: {{ f: value }}\n",
        env("docs/**"),
        env("(.*)"),
        env("y"),
    );
    let tree = setup(&config, &[]);
    let plan = tree.config.relation_plan.as_deref().unwrap();
    let selectors: Vec<(&str, bool)> = plan
        .selectors
        .iter()
        .map(|named| (named.name.as_str(), named.reads_environment))
        .collect();
    assert_eq!(selectors, [("docs", true), ("plain", false)]);
    let relations: Vec<(&str, bool, Vec<bool>)> = plan
        .relations
        .iter()
        .map(|named| {
            (
                named.name.as_str(),
                named.reads_environment,
                named
                    .relation
                    .fields
                    .iter()
                    .map(|(_, field)| field.sensitive)
                    .collect(),
            )
        })
        .collect();
    assert_eq!(
        relations,
        [
            ("by-selector", true, vec![false]),
            ("by-read", true, vec![false]),
            ("by-filter", false, vec![true, false]),
            ("untouched", false, vec![false]),
        ]
    );
}

/// A relation that reads one whose selection the environment writes, as a
/// join side or through a membership reference, shows no detail of its own
/// failures either: a repeated join key or a clashing value is what the
/// environment selected.
#[test]
fn a_relation_reading_one_the_environment_selects_shows_no_detail() {
    let tree = setup(
        "version: 2\nrelations:\n  secret:\n    from: { paths: b.json }\n    entries: { format: \
         json, at: '{{env.ALINT_V2_ROWS_UNSET | default(\"$.area\")}}' }\n    fields: { k: \
         entry.value }\n  plain:\n    from: { paths: c.json }\n    entries: { format: json, at: \
         $ }\n    fields: { k: entry.value }\n  joined:\n    join: { left: plain, right: secret, \
         on: { k: k }, unique: right }\n    fields: { k: left.k }\n  member:\n    from: { paths: \
         c.json }\n    entries: { format: json, at: $ }\n    fields: { v: entry.value }\n    \
         where: { v: { in: [ { relation: numbers, field: n } ] } }\n  numbers:\n    from: { \
         paths: d.json }\n    entries: { format: json, at: '{{env.ALINT_V2_ROWS_UNSET | \
         default(\"$.area\")}}' }\n    fields: { n: entry.value }\n  open:\n    join: { left: \
         plain, right: plain, on: { k: k }, unique: right }\n    fields: { k: left.k }\nrules: \
         []\n",
        &[
            ("b.json", r#"{"area": {"p": "1", "q": "1"}}"#),
            ("c.json", r#"{"x": "1", "y": "1"}"#),
            ("d.json", r#"{"area": {"n": 5}}"#),
        ],
    );
    let plan = tree.config.relation_plan.as_ref().unwrap();
    let reads: Vec<(&str, bool)> = plan
        .relations
        .iter()
        .map(|named| (named.name.as_str(), named.reads_environment))
        .collect();
    assert_eq!(
        reads,
        [
            ("secret", true),
            ("plain", false),
            ("joined", true),
            ("member", true),
            ("numbers", true),
            ("open", false)
        ]
    );
    for (relation, file) in [("joined", "b.json"), ("member", "c.json")] {
        assert_eq!(
            tree.failures(relation),
            [format!(
                "{file}: relation `{relation}` (.alint.yml): this relation's configuration reads \
                 the environment, so the detail of its analysis failure is not shown"
            )],
        );
    }
    // A relation that reads none of them still shows its detail.
    assert_eq!(
        tree.failures("open"),
        [
            "c.json: relation `open` (.alint.yml): the join's unique side (right, `plain`) repeats \
             the key \"1\": `x` in c.json and `y` in c.json; `unique: right` needs each key once \
             on that side, so make the key unique there, or name the other side unique"
        ]
    );
}
