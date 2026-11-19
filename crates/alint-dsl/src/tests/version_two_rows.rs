//! Version 2 relations evaluated over walked trees: a real configuration
//! through the loader, a tree on disk, and the evaluator phase 7's
//! `constraint` kind calls (design `v0.23/selectors-and-constraints.md`,
//! sections 5 to 8 and 12). These call the evaluator directly, to see its
//! rows; `constraint_families.rs` runs the rules that read them.

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
    let loaded = load(&dir.path().join(".alint.yml")).expect("the configuration loads");
    super::version_two_schema::constraint::assert_valid_when_loaded_builds(config, &loaded);
    let config = loaded;
    let index = walk(
        dir.path(),
        &WalkOptions::default().with_global_git_excludes(false),
    )
    .unwrap();
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

    /// The text at `span`, a span known without reading again, in `path`.
    fn text(&self, path: &str, span: Option<&eval::Span>) -> String {
        let span = span.and_then(eval::Span::bytes).expect("a known span");
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
    assert_eq!(tree.text(manifest, row[1].span.as_ref()), "\"react\"");
    assert_eq!(tree.text(manifest, row[2].span.as_ref()), "\"^18.2.0\"");
    assert_eq!(tree.text(manifest, row[0].span.as_ref()), "\"react\"");
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
        tree.text("config/locales/fr.yml", cell.span.as_ref()),
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
        index: walk(
            tree.dir.path(),
            &WalkOptions::default().with_global_git_excludes(false),
        )
        .unwrap(),
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

/// The same keys in every locale (`docs/site/configuration/version-2.md`):
/// each locale's keys below its top-level key, grouped by locale, or by
/// file.
const STUB_LOCALES: &str = r#"version: 2
relations:
  locale-keys:
    from: { paths: "config/locales/*.yml" }
    entries: { format: yaml, at: "$", leaves: true }
    fields: { locale: entry.root, file: stem, key: entry.path }
    transform: { key: [strip-root] }
rules:
  - id: by-locale
    kind: constraint
    input: locale-keys
    assert:
      set: { field: key, group_by: locale, equal_to: { group: en }, groups: [de, fr, it, pl] }
    level: error
  - id: by-file
    kind: constraint
    input: locale-keys
    assert: { set: { field: key, group_by: file, equal_to: { group: en } } }
    level: error
"#;

/// Under `strip-root`, a locale file whose top-level key holds nothing
/// (`fr: {}`, `it:`, `pl: []`), as 71 of Mastodon's 530 do, has no keys, as
/// an empty file has none: it lacks every key of the reference locale, and
/// the rule still reports the others' missing keys.
#[test]
fn a_stub_locale_file_has_no_keys_under_strip_root() {
    let files = &[
        ("config/locales/en.yml", "en:\n  hello: Hello\n  bye: Bye\n"),
        ("config/locales/de.yml", "de:\n  hello: Hallo\n"),
        ("config/locales/fr.yml", "fr: {}\n"),
        ("config/locales/it.yml", "it:\n"),
        ("config/locales/pl.yml", "pl: []\n"),
    ];
    let tree = setup(STUB_LOCALES, files);
    assert_eq!(
        table(&tree.rows("locale-keys")),
        strings(&[
            &["de", "de", "hello"],
            &["en", "en", "hello"],
            &["en", "en", "bye"],
        ])
    );
    let results = super::constraint_families::run(STUB_LOCALES, files);
    let missing = |group: &str, key: &str| {
        format!(
            "`locale` \"{group}\" lacks `key` \"{key}\", which `locale` \"en\" holds\n  ~ the \
             reference group's row with it: config/locales/en.yml:{}",
            if key == "hello" { "2:3" } else { "3:3" }
        )
    };
    let mut expected = vec![format!("config/locales/de.yml: {}", missing("de", "bye"))];
    for group in ["fr", "it", "pl"] {
        expected.extend([missing(group, "hello"), missing(group, "bye")]);
    }
    assert_eq!(
        super::constraint_families::findings(&results, "by-locale"),
        expected
    );
    let by_file = super::constraint_families::findings(&results, "by-file");
    for (file, keys) in [
        ("de", &["bye"][..]),
        ("fr", &["hello", "bye"]),
        ("it", &["hello", "bye"]),
        ("pl", &["hello", "bye"]),
    ] {
        for key in keys {
            let prefix =
                format!("config/locales/{file}.yml: `file` \"{file}\" lacks `key` \"{key}\"");
            assert!(
                by_file.iter().any(|finding| finding.starts_with(&prefix)),
                "{prefix}: {by_file:#?}"
            );
        }
    }
    assert_eq!(by_file.len(), 7, "{by_file:#?}");
}

/// A top-level key that holds a scalar, or a list with something in it, is
/// no stub: there are keys below it to compare or none to strip to, so
/// `strip-root` still fails on it, and a stub still counts as its file's
/// one top-level key.
#[test]
fn a_top_level_key_holding_a_value_still_fails_strip_root() {
    for (body, shown) in [
        ("fr: Bonjour\n", "`fr`"),
        ("fr: [a]\n", "`fr`"),
        ("fr: 0\n", "`fr`"),
    ] {
        let tree = setup(
            STUB_LOCALES,
            &[
                ("config/locales/en.yml", "en:\n  hello: Hello\n"),
                ("config/locales/fr.yml", body),
            ],
        );
        assert_eq!(
            tree.failures("locale-keys"),
            [format!(
                "config/locales/fr.yml: relation `locale-keys` (.alint.yml): `strip-root` on \
                 `key` needs every key path to have at least two segments, but {shown} has one; \
                 read the keys below the top-level key, or drop `strip-root`"
            )],
            "{body}"
        );
    }
    let tree = setup(
        STUB_LOCALES,
        &[("config/locales/all.yml", "en:\n  hello: Hello\nfr:\n")],
    );
    assert_eq!(
        tree.failures("locale-keys"),
        [
            "config/locales/all.yml: relation `locale-keys` (.alint.yml): `strip-root` on `key` \
             needs one top-level key in each resource, but this one has `en`, `fr`; read one \
             top-level object per file, or drop `strip-root`"
        ]
    );
}

/// A `read:` row's value keeps its byte span, so a finding about it has a
/// line and column, as one about an entry's has: a `lines:` value's trimmed
/// line, a `regex:` value's capture, and a JSON, YAML or TOML value's own
/// span; a value in an array, and a `whole_file` value, have none.
#[test]
fn a_read_value_keeps_its_place() {
    let cases = [
        (
            "{ json: \"$.*\" }",
            "a.json",
            "{\"x\": \"dup\",\n  \"y\": \"dup\"}\n",
            ("1:7", "2:8"),
        ),
        (
            "{ yaml: \"$.*\" }",
            "a.yml",
            "x: dup\ny:   'dup'\n",
            ("1:4", "2:6"),
        ),
        (
            "{ toml: \"$.*\" }",
            "a.toml",
            "x = \"dup\"\n\n y = \"dup\"\n",
            ("1:5", "3:6"),
        ),
        (
            "{ lines: {} }",
            "a.txt",
            "  dup  \n# note\n\ndup\n",
            ("1:3", "4:1"),
        ),
        (
            "{ regex: \"(?m)^v=(\\\\S+)\" }",
            "a.env",
            "v=dup\nw=1\n  v=no\nv=dup\n",
            ("1:3", "4:3"),
        ),
    ];
    for (read, file, body, (first, repeat)) in cases {
        let config = format!(
            "version: 2\nrelations:\n  values:\n    from: {{ paths: {file} }}\n    read: \
             {read}\n    fields: {{ v: value }}\nrules:\n  - id: c\n    kind: constraint\n    \
             input: values\n    assert: {{ unique: {{ key: [v] }} }}\n    level: error\n"
        );
        let results = super::constraint_families::run(&config, &[(file, body)]);
        let findings = super::constraint_families::findings(&results, "c");
        assert_eq!(findings.len(), 1, "{read}: {findings:#?}");
        let (place, related) = findings[0].split_once(": ").unwrap();
        assert_eq!(place, format!("{file}:{repeat}"), "{read}: {findings:#?}");
        assert!(
            related.ends_with(&format!("~ the first row with this key: {file}:{first}")),
            "{read}: {findings:#?}"
        );
    }
    // A value in an array, and the whole file, have no place.
    for (read, paths, files, place) in [
        (
            "{ json: \"$.a[*]\" }",
            "a.json",
            &[("a.json", "{\"a\": [\"dup\", \"dup\"]}\n")][..],
            "a.json",
        ),
        (
            "{ whole_file: {} }",
            "\"*.txt\"",
            &[("b.txt", "same\n"), ("c.txt", "same\n")][..],
            "c.txt",
        ),
    ] {
        let config = format!(
            "version: 2\nrelations:\n  values:\n    from: {{ paths: {paths} }}\n    read: \
             {read}\n    fields: {{ v: value }}\nrules:\n  - id: c\n    kind: constraint\n    \
             input: values\n    assert: {{ unique: {{ key: [v] }} }}\n    level: error\n"
        );
        let results = super::constraint_families::run(&config, files);
        let findings = super::constraint_families::findings(&results, "c");
        assert_eq!(findings.len(), 1, "{read}: {findings:#?}");
        assert!(
            findings[0].starts_with(&format!("{place}: ")),
            "{read}: {findings:#?}"
        );
    }
}

/// A `read:` finds where its values are written only for the values a
/// finding places: over 3,000 `package.json` files whose rows pass, no file
/// is walked for its values' places, and with one finding, only the file
/// it sits in is, the finding has its place, and what finding it held (the
/// document's tree and entries, several times its text) is taken from the
/// run's bytes.
#[test]
fn a_read_finds_the_places_of_only_the_values_a_finding_places() {
    let config = "version: 2\nrelations:\n  names:\n    from: { paths: \"*/package.json\" }\n    \
                  read: { json: \"$.name\" }\n    fields: { name: value, package: dir }\nrules:\n  \
                  - id: c\n    kind: constraint\n    input: names\n    assert: { equal: { field: \
                  name, to: { field: package } } }\n    level: error\n";
    // Each package's name; the findings, the walks finding their places
    // took, and the bytes the run took.
    let walked = |names: &dyn Fn(usize) -> String| {
        let files: Vec<(String, String)> = (0..3_000)
            .map(|index| {
                (
                    format!("p{index}/package.json"),
                    format!(
                        "{{\n  \"version\": \"1.0.0\",\n  \"name\": \"{}\"\n}}\n",
                        names(index)
                    ),
                )
            })
            .collect();
        let files: Vec<(&str, &str)> = files
            .iter()
            .map(|(path, body)| (path.as_str(), body.as_str()))
            .collect();
        let (dir, loaded) = super::constraint::load_tree(config, &files);
        let config = loaded.unwrap();
        let index = walk(
            dir.path(),
            &WalkOptions::default().with_global_git_excludes(false),
        )
        .unwrap();
        index.enable_relation_caches_under(&alint_core::relation::Limits::default());
        let mut registry = alint_core::RuleRegistry::new();
        alint_rules::register_builtin(&mut registry);
        let rule = registry.build(&config.rules[0]).unwrap();
        // The rule reads and places on this thread, where the walks count.
        let before = eval::read_span_walks();
        let findings = rule.evaluate(&Context::new(dir.path(), &index)).unwrap();
        let findings: Vec<String> = findings
            .iter()
            .map(|finding| {
                let path = finding.path.as_ref().unwrap();
                let path = path.display().to_string().replace('\\', "/");
                format!("{path}:{:?}:{:?}", finding.line, finding.column)
            })
            .collect();
        let taken = index.decode_cache().unwrap().bytes_taken();
        (findings, eval::read_span_walks() - before, taken)
    };
    let (findings, walks, passing) = walked(&|index| format!("p{index}"));
    assert_eq!((findings, walks), (Vec::new(), 0));
    // "other" is as long as "p1234": the two runs decode alike.
    let (findings, walks, taken) = walked(&|index| {
        if index == 1_234 {
            "other".to_string()
        } else {
            format!("p{index}")
        }
    });
    assert_eq!(findings, ["p1234/package.json:Some(3):Some(11)"]);
    assert_eq!(walks, 1);
    let text = "{\n  \"version\": \"1.0.0\",\n  \"name\": \"other\"\n}\n".len();
    assert!(taken - passing > 3 * text, "{taken} {passing} {text}");
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

/// A tree the policy passes: every pin is `.nvmrc`'s version.
const PINNED: &[(&str, &str)] = &[
    (".nvmrc", "v20.11.1\n"),
    (
        "package.json",
        r#"{"engines": {"node": "20.11.1"}, "devEngines": {"runtime": {"name": "node",
           "version": "20.11.1"}}, "volta": {"node": "20.11.1"}}"#,
    ),
];

/// A tree the policy fails twice: `engines.node` is a range, not the pinned
/// version, and `devEngines.runtime.version` is missing.
const DRIFTED: &[(&str, &str)] = &[
    (".nvmrc", "v20.11.1\n"),
    (
        "package.json",
        r#"{"engines": {"node": ">=20.11.1"}, "volta": {"node": "20.11.1"}}"#,
    ),
];

#[test]
fn toolchain_pins_that_agree_hold_one_version_in_every_row() {
    let tree = setup(NODE_PINS, PINNED);
    assert_eq!(
        table(&tree.rows("node-pins")),
        strings(&[
            &[".nvmrc", "20.11.1"],
            &["package.json", "20.11.1"],
            &["package.json", "20.11.1"],
            &["package.json", "20.11.1"],
        ])
    );
}

#[test]
fn toolchain_pins_union_a_regex_read_and_json_reads_with_a_missing_row() {
    let tree = setup(NODE_PINS, DRIFTED);
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
    let tree = setup(&config, DRIFTED);
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
        index: walk(
            tree.dir.path(),
            &WalkOptions::default().with_global_git_excludes(false),
        )
        .unwrap(),
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
    // A bound path selects a scalar, and a number outside an array.
    let files = &[("a.yaml", "meta: { x: 1 }\nnums: [1]\ndeps:\n  x: '1'\n")];
    let object = setup(&config.replace("$.name", "$.meta"), files);
    assert_eq!(
        object.failures("owned"),
        [
            "a.yaml: relation `owned` (.alint.yml): `owner` (`bound: $.meta`) selects an object \
             at `meta`; a field holds a scalar (null, a boolean, a number or a string), so bind \
             a path that selects one"
        ]
    );
    let number = setup(&config.replace("$.name", "$.nums[0]"), files);
    assert_eq!(
        number.failures("owned"),
        [
            "a.yaml: relation `owned` (.alint.yml): `owner` (`bound: $.nums[0]`) selects a \
             number in an array at `nums[0]`, whose lexeme alint does not keep (`1.10` would read \
             as `1.1`); bind an object member instead"
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
        once.text("a.yaml", rows.rows[1].cells[0].span.as_ref()),
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
    // A join row is the row of its side that is not unique: its own entry
    // is the dependency, wherever a field it copies from the other side sits.
    assert_eq!(linked.rows[0].at, row[0].at);
    assert_eq!(
        linked.rows[0]
            .at
            .pointer
            .as_ref()
            .map(alint_core::relation::Pointer::render),
        Some("dependencies.b".to_string())
    );
    assert_eq!(row[0].at.resource.path().as_str(), "a/package.json");
    assert_eq!(tree.text("a/package.json", row[1].span.as_ref()), "\"^1\"");
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
    // A key either side holds sensitive is not shown.
    for config in [
        LINKED.replace(
            "fields: { dependency: entry.key,",
            "fields: { dependency: { source: entry.key, sensitive: true },",
        ),
        LINKED.replace(
            "fields: { name: value,",
            "fields: { name: { source: value, sensitive: true },",
        ),
    ] {
        let tree = setup(
            &config,
            &[
                ("a/package.json", r#"{"name": "x"}"#),
                ("b/package.json", r#"{"name": "x"}"#),
            ],
        );
        let failures = tree.failures("linked");
        assert!(
            failures.len() == 1 && failures[0].contains("`names`) repeats a key: a/package.json"),
            "{failures:?}"
        );
    }
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
    // A string literal is named by its type too.
    let number = setup(
        &relation(&format!(
            "{DEPS_AT}    fields: {{ name: entry.key, version: {{ source: entry.value, \
             sensitive: true }} }}\n    where: {{ version: {{ in: ['1'] }} }}\n"
        )),
        &[("package.json", r#"{"deps": {"a": 1}}"#)],
    );
    assert_eq!(
        number.failures("deps"),
        [
            "package.json: relation `deps` (.alint.yml): `where.version.in` compares a number at \
             `deps.a` in package.json with a literal string; a value is in a list only beside \
             values of its own type, so compare values of one type (transform the field, or \
             change the list)"
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
    // An entry's value is read at its key, so the key is sensitive too.
    assert_eq!(shown, [("name", true), ("version", true)]);
}

/// A field shows a hidden value wherever it holds one: in a join, the copy of
/// the field an `on:` pair joins with a sensitive one (`j.y`, equal to `j.x`
/// in every row), and in a relation an `in:` list of a sensitive field
/// filters (`m.w`, with its key). Neither value reaches the failure of a
/// relation that joins on them.
#[test]
fn a_join_partner_and_a_field_a_sensitive_list_filters_are_sensitive() {
    let env = "'{{env.ALINT_V2_ROWS_UNSET | default(\"s3cret\")}}'";
    let config = format!(
        "version: 2\nrelations:\n  \
         l:\n    from: {{ paths: a.json }}\n    entries: {{ format: json, at: $ }}\n    \
         fields: {{ k: entry.value }}\n  \
         r:\n    from: {{ paths: b.json }}\n    entries: {{ format: json, at: $ }}\n    \
         fields: {{ v: entry.value }}\n  \
         j:\n    join: {{ left: l, right: r, on: {{ k: v }}, unique: right }}\n    \
         fields: {{ x: left.k, y: right.v }}\n    where: {{ x: {{ in: [{env}] }} }}\n  \
         d:\n    join: {{ left: j, right: j, on: {{ y: y }}, unique: right }}\n    \
         fields: {{ y: left.y }}\n  \
         m:\n    from: {{ paths: c.json }}\n    entries: {{ format: json, at: $ }}\n    \
         fields: {{ w: entry.value, n: entry.key }}\n    \
         where: {{ w: {{ in: [ {{ relation: j, field: x }} ] }} }}\n  \
         d2:\n    join: {{ left: m, right: m, on: {{ w: w }}, unique: right }}\n    \
         fields: {{ w: left.w }}\nrules: []\n"
    );
    let tree = setup(
        &config,
        &[
            ("a.json", r#"{"p": "s3cret", "q": "s3cret", "z": "other"}"#),
            ("b.json", r#"{"n": "s3cret", "o": "other"}"#),
            ("c.json", r#"{"e": "s3cret", "f": "s3cret", "g": "nope"}"#),
        ],
    );
    let shown = |relation: &str| -> Vec<(String, bool)> {
        tree.rows(relation)
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.sensitive))
            .collect()
    };
    let named = |name: &str| (name.to_string(), true);
    assert_eq!(shown("j"), [named("x"), named("y")]);
    assert_eq!(shown("m"), [named("w"), named("n")]);
    for relation in ["d", "d2"] {
        let failures = tree.evaluate(relation).expect_err("a key repeats");
        assert!(!failures.is_empty(), "{relation}");
        for failure in failures.iter() {
            let message = failure.to_violation("rule").message;
            assert!(!message.contains("s3cret"), "{relation}: {message}");
            assert!(
                !failure.to_string().contains("s3cret"),
                "{relation}: {failure}"
            );
        }
    }
}

/// A key is in its path, its parent's path and its root, so a sensitive
/// field of any of them makes the others sensitive, in the relation and in
/// a join that copies them; the entry's value stays shown.
#[test]
fn a_sensitive_key_makes_every_field_that_spells_it_sensitive() {
    let config = "version: 2\nrelations:\n  t:\n    from: { paths: a.json }\n    entries: { \
                  format: json, at: \"$.*\" }\n    fields:\n      k: { source: entry.key, \
                  sensitive: true }\n      p: entry.path\n      up: entry.parent\n      top: \
                  entry.root\n      v: entry.value\n      f: path\n  j:\n    join: { left: t, \
                  right: t, on: { v: v }, unique: right }\n    fields: { lp: left.p, rv: right.v \
                  }\nrules: []\n";
    let tree = setup(config, &[("a.json", r#"{"g": {"SECRET_NAME": "x"}}"#)]);
    let shown = |relation: &str| -> Vec<(String, bool)> {
        tree.rows(relation)
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.sensitive))
            .collect()
    };
    let field = |name: &str, sensitive| (name.to_string(), sensitive);
    assert_eq!(
        shown("t"),
        [
            field("k", true),
            field("p", true),
            field("up", true),
            field("top", true),
            field("v", false),
            field("f", false),
        ]
    );
    assert_eq!(shown("j"), [field("lp", true), field("rv", false)]);
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

/// A dotenv manifest's values are redacted by default, and its reader's
/// message may quote one: the failure leaves the reason out and is
/// sensitive, as the engine's gate redacts it.
#[test]
fn a_redacted_manifest_the_selector_cannot_read_hides_why() {
    let config = SCOPED.replace(
        "source: package.json\n        extract: { json: \"$.workspaces[*]\" }",
        "source: .env\n        extract: { dotenv: \"$.MEMBERS\" }",
    );
    assert_ne!(config, SCOPED);
    let tree = setup(&config, &[(".env", "MEMBERS='hunter2-secret\n")]);
    let outcome = tree.evaluate("member-names");
    let failures = outcome.expect_err("the relation fails");
    assert_eq!(failures.len(), 1);
    assert!(failures[0].sensitive);
    assert_eq!(
        failures[0].to_string(),
        ".env: relation `member-names` (.alint.yml): selector `members`'s `scope_filter:` reads \
         `.env`: it could not be read as a manifest, and the reason is not shown, since the file's \
         values are redacted by default; fix `.env`, or change the selector's `scope_filter:`"
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
    // A relation reads the environment through a selector it reads, too,
    // and through a `where:` entry, which chooses its rows; its own
    // definition does only where it holds such a value outside one.
    let relations: Vec<(&str, bool, bool, Vec<bool>)> = plan
        .relations
        .iter()
        .map(|named| {
            (
                named.name.as_str(),
                named.reads_environment,
                named.definition_reads_environment,
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
            ("by-selector", true, false, vec![false]),
            ("by-read", true, true, vec![false]),
            ("by-filter", true, false, vec![true, false]),
            ("untouched", false, false, vec![false]),
        ]
    );
}

/// A field the environment filters makes every field that shows its value
/// sensitive: an entry's key is in its path, its parent's and its root, so
/// a filter on any of an entry's fields hides all five, and a path token
/// hides the others, as two fields bound to one value hide each other.
#[test]
fn a_filter_from_the_environment_hides_every_field_that_shows_its_value() {
    let env = "'{{env.ALINT_V2_ROWS_UNSET | default(\"a\")}}'";
    let config = format!(
        "version: 2\nrelations:\n  entries:\n    from: {{ paths: a.json }}\n    entries: {{ \
         format: json, at: $ }}\n    fields: {{ key: entry.key, path: entry.path, root: \
         entry.root, parent: entry.parent, value: entry.value, file: path }}\n    where: {{ key: \
         {{ in: [{env}] }} }}\n  tokens:\n    from: {{ paths: a.json }}\n    read: {{ json: $.a \
         }}\n    fields: {{ stem: stem, path: path, v: value, w: value }}\n    where: {{ stem: \
         {{ in: [{env}] }}, w: {{ in: [{env}] }} }}\n  plain:\n    from: {{ paths: a.json }}\n    \
         entries: {{ format: json, at: $ }}\n    fields: {{ key: entry.key, path: entry.path }}\n"
    );
    let tree = setup(&config, &[]);
    let plan = tree.config.relation_plan.as_deref().unwrap();
    let sensitive: Vec<Vec<bool>> = plan
        .relations
        .iter()
        .map(|named| {
            named
                .relation
                .fields
                .iter()
                .map(|(_, field)| field.sensitive)
                .collect()
        })
        .collect();
    assert_eq!(
        sensitive,
        [
            vec![true, true, true, true, true, false],
            vec![true, true, true, true],
            vec![false, false],
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

/// A join's copy of an `entry.path` field keeps its key path's segments, so
/// `strip-root` applies to it as to the field it copies.
#[test]
fn strip_root_applies_to_a_join_copy_of_an_entry_path() {
    let tree = setup(
        "version: 2\nrelations:\n  keys:\n    from: { paths: '*.json' }\n    entries: { format: \
         json, at: $, leaves: true }\n    fields: { key: entry.key, path: entry.path }\n  \
         joined:\n    join: { left: keys, right: keys, on: { key: key }, unique: right }\n    \
         fields: { key: left.key, path: left.path }\n    transform: { path: [strip-root] }\n",
        &[(
            "en.json",
            r#"{"en": {"nav": {"home": "Home", "about": "About"}}}"#,
        )],
    );
    assert_eq!(
        table(&tree.rows("joined")),
        strings(&[&["home", "nav.home"], &["about", "nav.about"]])
    );
}

// The relation evaluator's review fixes (phase 6).

/// One relation `keys` over `file`, read with `entries:` in `format`.
fn keys_over(file: &str, format: &str, at: &str, leaves: bool, fields: &str) -> String {
    format!(
        "version: 2\nrelations:\n  keys:\n    from: {{ paths: \"{file}\" }}\n    entries: {{ \
         format: {format}, at: \"{at}\", leaves: {leaves} }}\n    fields: {fields}\n"
    )
}

#[test]
fn toml_dotted_keys_in_inline_tables_give_the_rows_json_gives() {
    let toml = "[en]\nnav = { home.title = \"Home\", about = \"About\" }\n";
    let json = r#"{"en": {"nav": {"home": {"title": "Home"}, "about": "About"}}}"#;
    let fields = "{ locale: entry.root, key: entry.path }";
    let read = |file: &str, format: &str, body: &str, at: &str, leaves: bool| {
        table(
            &setup(
                &keys_over(file, format, at, leaves, fields),
                &[(file, body)],
            )
            .rows("keys"),
        )
    };
    assert_eq!(
        read("en.toml", "toml", toml, "$", true),
        strings(&[&["en", "en.nav.home.title"], &["en", "en.nav.about"]])
    );
    assert_eq!(
        read("en.toml", "toml", toml, "$.en.nav", false),
        strings(&[&["en", "en.nav.home"], &["en", "en.nav.about"]])
    );
    for (at, leaves) in [
        ("$", true),
        ("$", false),
        ("$.en", true),
        ("$.en.nav", false),
    ] {
        assert_eq!(
            read("en.toml", "toml", toml, at, leaves),
            read("en.json", "json", json, at, leaves),
            "at {at}, leaves {leaves}"
        );
    }
    // The dotted key's table is a table of its own, not one in an array of
    // tables, and its key is no leaf.
    let dotted = "a = { b.c = \"x\" }\n";
    let members = setup(
        &keys_over("x.toml", "toml", "$.a", false, "{ key: entry.key }"),
        &[("x.toml", dotted)],
    );
    assert_eq!(table(&members.rows("keys")), strings(&[&["b"]]));
    let leaves = setup(
        &keys_over(
            "x.toml",
            "toml",
            "$",
            true,
            "{ key: entry.path, value: entry.value }",
        ),
        &[("x.toml", dotted)],
    );
    assert_eq!(table(&leaves.rows("keys")), strings(&[&["a.b.c", "x"]]));
}

const LOCALES: &str = r#"version: 2
relations:
  keys:
    from: { paths: "locales/*.yml" }
    entries: { format: yaml, at: "$", leaves: true }
    fields: { locale: entry.root, key: entry.path }
    transform: { key: [strip-root] }
"#;

#[test]
fn a_yaml_document_of_only_comments_or_null_reads_as_an_empty_one() {
    let tree = setup(
        LOCALES,
        &[
            ("locales/en.yml", "en:\n  title: Title\n"),
            ("locales/de.yml", "---\n"),
            ("locales/es.yml", "~\n"),
            ("locales/fr.yml", "# nothing translated yet\n"),
            ("locales/it.yml", ""),
            ("locales/pt.yml", "null\n"),
        ],
    );
    let keys = tree.rows("keys");
    assert_eq!(table(&keys), strings(&[&["en", "title"]]));
    assert_eq!(keys.resources.len(), 6);
    // A scalar document is not an empty one.
    let tree = setup(
        LOCALES,
        &[
            ("locales/en.yml", "en:\n  title: Title\n"),
            ("locales/fr.yml", "bonjour\n"),
        ],
    );
    assert_eq!(
        tree.failures("keys"),
        [
            "locales/fr.yml: relation `keys` (.alint.yml): `at: $` selects a string at the \
             document root; `at:` selects objects, whose members become rows, so select an \
             object (read an array's or a scalar's values with `read:`)"
        ]
    );
}

/// One relation `r` over the members of `$.a` in `x.json`, filtered.
fn filtered(fields: &str, filters: &str) -> String {
    format!(
        "version: 2\nrelations:\n  r:\n    from: {{ paths: x.json }}\n    entries: {{ format: \
         json, at: \"$.a\" }}\n    fields: {fields}\n    where: {filters}\n"
    )
}

#[test]
fn where_drops_a_row_any_predicate_is_false_for_whatever_their_order() {
    let files = &[("x.json", r#"{"a": {"keep": "1.0", "drop": null}}"#)];
    let fields = "{ k: entry.key, v: entry.value }";
    // `matches` cannot test `drop`'s null, but `in` drops the row.
    for filters in [
        "{ k: { in: [keep] }, v: { matches: \"^1\" } }",
        "{ v: { matches: \"^1\" }, k: { in: [keep] } }",
    ] {
        let tree = setup(&filtered(fields, filters), files);
        assert_eq!(
            table(&tree.rows("r")),
            strings(&[&["keep", "1.0"]]),
            "{filters}"
        );
    }
    // A row no predicate drops fails on the type error, in either order.
    for filters in [
        "{ k: { in: [keep, drop] }, v: { matches: \"^1\" } }",
        "{ v: { matches: \"^1\" }, k: { in: [keep, drop] } }",
    ] {
        let tree = setup(&filtered(fields, filters), files);
        assert_eq!(
            tree.failures("r"),
            [
                "x.json: relation `r` (.alint.yml): `where.v.matches` tests null at `a.drop`, but \
                 a pattern tests strings; filter a string field, or transform this one"
            ],
            "{filters}"
        );
    }
}

#[test]
fn a_membership_type_clash_fails_only_on_a_row_no_predicate_drops() {
    let files = &[("x.json", r#"{"a": {"p": 1, "q": "1"}}"#)];
    let fields = "{ k: entry.key, v: entry.value }";
    for filters in [
        "{ k: { not_in: [p] }, v: { in: ['1'] } }",
        "{ v: { in: ['1'] }, k: { not_in: [p] } }",
    ] {
        let tree = setup(&filtered(fields, filters), files);
        assert_eq!(table(&tree.rows("r")), strings(&[&["q", "1"]]), "{filters}");
    }
    let clash = "x.json: relation `r` (.alint.yml): `where.v.in` compares a number at `a.p` in \
                 x.json with the literal \"1\"; a value is in a list only beside values of its \
                 own type, so compare values of one type (transform the field, or change the list)";
    let tree = setup(&filtered(fields, "{ v: { in: ['1'] } }"), files);
    assert_eq!(tree.failures("r"), [clash]);
    // A row two predicates cannot test reports the first field's error,
    // whichever is written first; `q` matches no `x`, so it is dropped.
    let fields = "{ k: entry.key, v: entry.value, w: entry.value }";
    for filters in [
        "{ w: { matches: x }, v: { in: ['1'] } }",
        "{ v: { in: ['1'] }, w: { matches: x } }",
    ] {
        let tree = setup(&filtered(fields, filters), files);
        assert_eq!(tree.failures("r"), [clash], "{filters}");
    }
}

/// A list of several types names, for a value of none of them, its first
/// member in type order (a boolean, a number, then a string): here the
/// number a relation it reads holds, not its literal.
#[test]
fn a_membership_type_clash_names_the_first_member_in_type_order() {
    let config = format!(
        "{}  nums:\n    from: {{ paths: nums.json }}\n    entries: {{ format: json, at: \"$\" \
         }}\n    fields: {{ k: entry.key, n: entry.value }}\n",
        filtered(
            "{ k: entry.key, v: entry.value }",
            "{ v: { in: ['x', { relation: nums, field: n }] } }"
        )
    );
    let files = &[
        ("x.json", r#"{"a": {"p": true}}"#),
        ("nums.json", r#"{"one": 1}"#),
    ];
    let tree = setup(&config, files);
    assert_eq!(
        tree.failures("r"),
        [
            "x.json: relation `r` (.alint.yml): `where.v.in` compares a boolean at `a.p` in x.json \
             with a number at `one` in nums.json; a value is in a list only beside values of its \
             own type, so compare values of one type (transform the field, or change the list)"
        ]
    );
    // A value at the document root is named by its file alone.
    let config = "version: 2\nrelations:\n  r:\n    from: { paths: pin.json }\n    read: { json: \
                  \"$\" }\n    fields: { v: value, b: \"bound: $\" }\n    where: { b: { in: [true] \
                  } }\n";
    let tree = setup(config, &[("pin.json", "\"x\"")]);
    assert_eq!(
        tree.failures("r"),
        [
            "pin.json: relation `r` (.alint.yml): `where.b.in` compares a string in pin.json with \
             the literal true; a value is in a list only beside values of its own type, so compare \
             values of one type (transform the field, or change the list)"
        ]
    );
}

/// A `read:` that extracts nothing gives one row whose value is missing,
/// which no pattern matches and no list holds.
#[test]
fn a_missing_value_matches_no_pattern_and_is_in_no_list() {
    let files = &[("x.json", r#"{"other": "1"}"#)];
    for (filters, kept) in [
        ("{ v: { matches: '.' } }", 0),
        ("{ v: { not_matches: '.' } }", 1),
        ("{ v: { in: ['1'] } }", 0),
        ("{ v: { not_in: ['1'] } }", 1),
    ] {
        let config = format!(
            "version: 2\nrelations:\n  r:\n    from: {{ paths: x.json }}\n    read: {{ json: \
             \"$.name\" }}\n    fields: {{ v: value, p: path }}\n    where: {filters}\n"
        );
        let tree = setup(&config, files);
        let rows = tree.rows("r");
        assert_eq!(rows.rows.len(), kept, "{filters}");
        assert!(rows.rows.iter().all(|row| row.cells[0].value.is_none()));
    }
}
