//! `version: 2` through the loader: dispatch at every source, the decoded
//! objects and their provenance, and the refusals (design
//! `v0.23/selectors-and-constraints.md`, sections 3 and 10).

use std::path::{Path, PathBuf};

use alint_core::ConfigSourceClass;

use super::*;

const OBJECTS: &str = r#"selectors:
  manifests:
    paths: "**/package.json"
relations:
  deps:
    from: manifests
    entries: { format: json, at: "$.dependencies" }
    fields: { name: entry.key, version: entry.value }
"#;

const README_RULE: &str = "rules:\n  - id: readme\n    kind: file_exists\n    paths: README.md\n    \
                           level: warning\n";

fn write(root: &Path, relative: &str, body: &str) -> PathBuf {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, body).unwrap();
    path
}

/// `relative` as the loader names a config under the root: its path from
/// there, with the platform's separator.
fn shown(relative: &str) -> String {
    relative
        .split('/')
        .collect::<PathBuf>()
        .display()
        .to_string()
}

/// `error` without the directory `dir`, written as given or resolved (a
/// temporary directory may sit under a symlink, such as macOS's `/var`, or
/// have a Windows short name), as a load error prints a config's path.
fn without_dir(error: &str, dir: &Path) -> String {
    let resolved = dir.canonicalize().unwrap().display().to_string();
    let resolved = resolved
        .strip_prefix(r"\\?\")
        .unwrap_or(&resolved)
        .to_string();
    error
        .replace(&resolved, "")
        .replace(&dir.display().to_string(), "")
}

fn options() -> LoadOptions<'static> {
    LoadOptions::with_catalog(test_catalog())
}

fn raw(path: &Path) -> Result<RawConfig> {
    load_top_level_raw(path, &options())
}

fn load_error(path: &Path) -> String {
    load(path).expect_err("expected a load error").to_string()
}

/// The names each source that declares any declared, by its display.
fn declared(raw: &RawConfig) -> Vec<(String, ConfigSourceClass, Vec<String>)> {
    raw.v2
        .sources
        .iter()
        .filter(|source| !source.is_empty())
        .map(|source| {
            let names = source
                .selectors
                .iter()
                .map(|(name, _)| name.clone())
                .chain(source.relations.iter().map(|(name, _)| name.clone()))
                .chain(source.aliases.iter().map(|(name, _)| name.clone()))
                .collect();
            (source.source.display.clone(), source.source.class, names)
        })
        .collect()
}

#[test]
fn a_version_two_config_loads_and_keeps_its_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(
        tmp.path(),
        ".alint.yml",
        &format!("version: 2\n{OBJECTS}{README_RULE}"),
    );
    let config = load(&path).unwrap();
    assert_eq!(config.version, 2);
    assert_eq!(config.rules.len(), 1);
    assert_eq!(config.rules[0].id, "readme");
    assert_eq!(config.compatibility[0].config_version, 2);

    let raw = raw(&path).unwrap();
    assert_eq!(raw.surface, v2::Surface::V2);
    assert_eq!(
        declared(&raw),
        vec![(
            ".alint.yml".to_string(),
            ConfigSourceClass::Root,
            vec!["manifests".to_string(), "deps".to_string()]
        )]
    );
    assert_eq!(
        raw.v2.sources[0].source.locator,
        alint_core::relation::plan::SourceLocator::Local(path.canonicalize().unwrap()),
        "the source is kept as its resolved file"
    );
}

#[test]
fn a_version_two_config_reads_version_one_keys_as_version_one_does() {
    // The v1 part of a v2 document decodes through the same deserializer, so
    // an error in it reads the same, line and column included.
    let tmp = tempfile::tempdir().unwrap();
    let body = "rules:\n  - id: x\n    kind: file_exists\n    paths: [a\n";
    let one = write(tmp.path(), "one/.alint.yml", &format!("version: 1\n{body}"));
    let two = write(tmp.path(), "two/.alint.yml", &format!("version: 2\n{body}"));
    let strip = |error: String, dir: &str| without_dir(&error, &tmp.path().join(dir));
    assert_eq!(
        strip(load_error(&one), "one"),
        strip(load_error(&two), "two")
    );

    let body = "ignore: 3\nrules: []\n";
    let one = write(tmp.path(), "one/.alint.yml", &format!("version: 1\n{body}"));
    let two = write(tmp.path(), "two/.alint.yml", &format!("version: 2\n{body}"));
    let one_error = strip(load_error(&one), "one");
    assert!(one_error.contains("line 2 column 9"), "{one_error}");
    assert_eq!(one_error, strip(load_error(&two), "two"));

    // A top-level key written twice reads as version 1's decode words it,
    // `version:` itself included.
    for (one_body, two_body) in [
        ("version: 1\nversion: 2\n", "version: 2\nversion: 1\n"),
        ("version: 1\nrules: []\n", "version: 2\nrules: []\n"),
    ] {
        let one = write(
            tmp.path(),
            "one/.alint.yml",
            &format!("{one_body}rules: []\n"),
        );
        let two = write(
            tmp.path(),
            "two/.alint.yml",
            &format!("{two_body}rules: []\n"),
        );
        let one_error = strip(load_error(&one), "one");
        assert!(one_error.contains("duplicate field"), "{one_error}");
        assert_eq!(one_error, strip(load_error(&two), "two"));
    }

    // A v1 `extends:` entry error keeps its text (the v2 entry adds `as`).
    let one = write(
        tmp.path(),
        "one/.alint.yml",
        "version: 1\nextends:\n  - url: ./a.yml\n    excpet: [x]\n",
    );
    assert!(
        load_error(&one).ends_with(
            "YAML parse error: extends[0]: unknown field `excpet`, expected one of `url`, \
             `only`, `except` at line 3 column 5"
        ),
        "{}",
        load_error(&one)
    );
}

#[test]
fn a_version_one_config_is_refused_version_two_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(tmp.path(), ".alint.yml", &format!("version: 1\n{OBJECTS}"));
    assert_eq!(
        load_error(&path),
        ".alint.yml: `selectors:` needs `version: 2`; this config is version 1"
    );
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 1\nextends:\n  - url: ./base.yml\n    as: base\n",
    );
    assert_eq!(
        load_error(&path),
        ".alint.yml: `extends[0].as` needs `version: 2`; this config is version 1"
    );
}

#[test]
fn an_unknown_key_in_a_version_two_config_lists_the_version_two_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(tmp.path(), ".alint.yml", "version: 2\nrelation: {}\n");
    assert_eq!(
        load_error(&path),
        ".alint.yml: unknown key `relation` in a version 2 config; expected one of `version`, \
         `requires`, `extends`, `ignore`, `respect_gitignore`, `vars`, `facts`, `templates`, \
         `rules`, `fix_size_limit`, `nested_configs`, `limits`, `allow_out_of_root`, `baseline`, \
         `trusted_extends`, `selectors`, `relations` at line 2 column 1"
    );
}

#[test]
fn a_version_read_from_the_environment_is_version_one() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: '{{env.ALINT_V2_TEST_VERSION | default(\"2\")}}'\nrules: []\n",
    );
    assert_eq!(load_error(&path), format!(".alint.yml: {TWO_AS_WRITTEN}"));
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: '{{env.ALINT_V2_TEST_VERSION | default(\"1\")}}'\nselectors: {}\n",
    );
    assert_eq!(
        load_error(&path),
        ".alint.yml: `selectors:` needs `version: 2`; this config's `version:` is not the \
         integer `2` (a version read from the environment is version 1)"
    );
}

/// A value substituted into a selector or relation stayed text in every field
/// (V2L-7), so `optional: "{{env.B}}"` with `B=true` failed ("expected `true`
/// or `false`"). In a field that takes a boolean (a relation's or a union
/// part's `optional:`, `entries:`'s `leaves:`, a field's `sensitive:`) a value
/// that spells one reads as one; a number stays text there, and so does a
/// value that spells a boolean in any other field, even one named like those.
#[test]
fn a_value_from_the_environment_is_a_boolean_where_the_field_takes_one() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nrelations:\n  r:\n    from: { paths: '*.json' }\n    read: { json: $.a }\n    \
         optional: '{{env.B}}'\n    fields: { v: { source: value, sensitive: '{{env.B}}' } }\n  \
         e:\n    from: { paths: '*.json' }\n    entries: { format: json, at: $, leaves: \
         '{{env.B}}' }\n    fields: { k: entry.key }\n  u:\n    union:\n      - { from: { paths: \
         a.json }, read: { json: $.a }, optional: '{{env.B}}' }\n      - { from: { paths: b.json \
         }, read: { json: $.b } }\n    fields: { v: value }\nrules: []\n",
    );
    for (value, bound) in [("true", "true"), ("false", "false")] {
        let environment: &'static [(&str, &str)] = if value == "true" {
            &[("B", "true")]
        } else {
            &[("B", "false")]
        };
        load_in(&path, environment).unwrap();
        let raw = load_top_level_raw(
            &path,
            &LoadOptions {
                environment: std::sync::Arc::new(Environment(environment)),
                ..options()
            },
        )
        .unwrap();
        // What each relation decoded to, without its masked copies.
        let shown = raw.v2.sources[0]
            .relations
            .iter()
            .map(|(_, relation)| format!("{:?} {:?}\n", relation.rows, relation.fields))
            .collect::<Vec<_>>()
            .concat();
        for field in ["optional", "leaves", "sensitive"] {
            let bound_here = format!("{field}: {bound}");
            assert!(shown.contains(&bound_here), "{bound_here} in {shown}");
        }
        // The relation's `optional`, and the union's first part's (its second
        // part's is `false` unset).
        let parts = if bound == "true" { 2 } else { 3 };
        assert_eq!(
            shown.matches(&format!("optional: {bound}")).count(),
            parts,
            "{shown}"
        );
    }
    // A number is no boolean.
    let error = load_in(&path, &[("B", "1")]).unwrap_err().to_string();
    assert!(
        error.contains("expected `true` or `false`, found a string"),
        "{error}"
    );
    // `where:` names a field `optional`, whose predicates are no boolean:
    // the value stays text, as the error says.
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nrelations:\n  r:\n    from: { paths: '*.json' }\n    read: { json: $.a }\n    \
         fields: { optional: value }\n    where: { optional: '{{env.B}}' }\nrules: []\n",
    );
    let error = load_in(&path, &[("B", "true")]).unwrap_err().to_string();
    assert!(error.contains("`relations.r.where.optional`"), "{error}");
    assert!(error.ends_with("found a string"), "{error}");
}

/// What a 2 the loader did not read as version 2 is refused with.
const TWO_AS_WRITTEN: &str = "unsupported config version 2 as written: only a literal \
                              `version: 2` in a local config file selects the experimental \
                              version 2 (a version read from the environment is version 1)";

/// An unsupported version said "this build supports 1 through 1", which
/// left version 2 out, and so did a 2 read from the environment (in a root,
/// or in a fragment of a version 2 root), which the message did not tell from
/// a version this build does not know. Version 2 is named as the experiment,
/// and a 2 read from the environment is told to be written literally.
#[test]
fn an_unsupported_version_names_version_two_as_the_experiment() {
    let tmp = tempfile::tempdir().unwrap();
    for version in ["0", "3"] {
        let path = write(
            tmp.path(),
            ".alint.yml",
            &format!("version: {version}\nrules: []\n"),
        );
        assert_eq!(
            load_error(&path),
            format!(
                ".alint.yml: unsupported config version {version} (this build supports version \
                 1, and version 2 as an experiment)"
            )
        );
    }
    assert_eq!(
        crate::parse("version: 3\nrules: []\n")
            .unwrap_err()
            .to_string(),
        "<inline>: unsupported config version 3 (this build supports version 1, and version 2 \
         as an experiment)"
    );
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: '{{env.V}}'\nrules: []\n",
    );
    assert_eq!(
        load_in(&path, &[("V", "2")]).unwrap_err().to_string(),
        format!(".alint.yml: {TWO_AS_WRITTEN}")
    );
    // A fragment of a version 2 root that reads its version from the
    // environment.
    let path = write(tmp.path(), ".alint.yml", "version: 2\nrules: []\n");
    write(
        tmp.path(),
        ".alint.d/10.yml",
        "version: '{{env.V}}'\nrules: []\n",
    );
    assert_eq!(
        load_in(&path, &[("V", "2")]).unwrap_err().to_string(),
        format!("{}: {TWO_AS_WRITTEN}", shown(".alint.d/10.yml"))
    );
}

#[derive(Debug)]
struct Environment(&'static [(&'static str, &'static str)]);

impl crate::EnvironmentProvider for Environment {
    fn get(&self, name: &str) -> Option<String> {
        self.0
            .iter()
            .find(|(held, _)| *held == name)
            .map(|(_, value)| (*value).to_string())
    }
}

fn load_in(path: &Path, environment: &'static [(&'static str, &'static str)]) -> Result<Config> {
    let options = LoadOptions {
        environment: std::sync::Arc::new(Environment(environment)),
        ..options()
    };
    load_with(path, &options)
}

#[test]
fn a_version_two_value_interpolates_and_a_name_does_not() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nselectors:\n  docs:\n    paths: '{{env.DOCS}}/**'\n\
         relations:\n  titles:\n    from: docs\n    read: { regex: '{{env.TITLE}}' }\n    \
         fields: { title: value }\n",
    );
    let raw = {
        let options = LoadOptions {
            environment: std::sync::Arc::new(Environment(&[
                ("DOCS", "handbook"),
                ("TITLE", "^# (.*)"),
            ])),
            ..options()
        };
        load_top_level_raw(&path, &options).unwrap()
    };
    let source = &raw.v2.sources[0];
    assert_eq!(
        format!("{:?}", source.selectors[0].1.paths),
        format!("{:?}", alint_core::PathsSpec::Single("handbook/**".into()))
    );
    let v2::Rows::Part(part) = &source.relations[0].1.rows else {
        panic!("not a plain relation");
    };
    let v2::Reader::Read { read, .. } = &part.reader else {
        panic!("not a read");
    };
    assert_eq!(read, &v2::Read::Regex("^# (.*)".into()));

    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nrelations:\n  titles:\n    from: '{{env.SELECTOR}}'\n    \
         read: { whole_file: {} }\n    fields: { text: value }\n",
    );
    assert_eq!(
        load_in(&path, &[("SELECTOR", "docs")])
            .unwrap_err()
            .to_string(),
        ".alint.yml: `relations.titles.from`: `{{...}}` is not allowed in a name, alias or \
         reference of a version 2 config, which must name the same thing in every environment; \
         write it literally"
    );
}

/// A value substituted into a selector or relation was re-typed as it is in a
/// rule, so `in: ["{{env.NODE_MAJOR}}"]` with `NODE_MAJOR=18` held the number
/// 18 and failed to load ("expected a string"), and `matches: "{{env.X}}"`
/// with `X=5` too. A selector's or relation's value stays text; a rule's is
/// re-typed as before.
#[test]
fn a_value_from_the_environment_stays_text_in_a_selector_or_relation() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nselectors:\n  pkgs:\n    paths: '{{env.DIR}}/**'\nrelations:\n  \
         node:\n    from: pkgs\n    read: { json: $.engines.node }\n    fields: { v: value }\n    \
         where: { v: { in: ['{{env.NODE_MAJOR}}'], matches: '{{env.X}}' } }\nrules:\n  - id: \
         r\n    kind: file_max_size\n    paths: x\n    max_bytes: '{{env.NODE_MAJOR}}'\n    \
         level: error\n",
    );
    let environment = &[("DIR", "1"), ("NODE_MAJOR", "18"), ("X", "5")];
    let config = load_in(&path, environment).unwrap();
    // The rule's `max_bytes:` still reads a number.
    assert_eq!(config.rules.len(), 1);
    let raw = load_top_level_raw(
        &path,
        &LoadOptions {
            environment: std::sync::Arc::new(Environment(environment)),
            ..options()
        },
    )
    .unwrap();
    let source = &raw.v2.sources[0];
    assert_eq!(
        format!("{:?}", source.selectors[0].1.paths),
        format!("{:?}", alint_core::PathsSpec::Single("1/**".into()))
    );
    let shown = format!("{:?}", source.relations[0].1);
    assert!(shown.contains("\"18\""), "{shown}");
    assert!(shown.contains("\"5\""), "{shown}");
}

#[test]
fn a_version_two_error_leaves_out_a_value_from_the_environment() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nrelations:\n  r:\n    from: { paths: '*.json' }\n    \
         entries: { format: '{{env.FORMAT}}', at: $ }\n    fields: { k: entry.key }\n",
    );
    let error = load_in(&path, &[("FORMAT", "hunter2")])
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        ".alint.yml: `relations.r.entries.format`: unknown format `<redacted>`; expected one of \
         `json`, `yaml`, `toml` (in this build entries are read from JSON, YAML and TOML only)"
    );
    // A load the environment fed carries the value for the embedder too.
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nselectors:\n  s: { paths: '{{env.DIR}}/**' }\nrules: []\n",
    );
    let config = load_in(&path, &[("DIR", "secret-dir")]).unwrap();
    assert!(
        config
            .environment_values
            .iter()
            .any(|value| value == "secret-dir"),
        "{:?}",
        config.environment_values
    );
}

/// Values a selector or relation reads from the environment that pass the
/// decode and fail the plan check, each quoted by the error in a form the
/// whole-value scrub does not remove: a path's last segment, or a value under
/// four characters.
const PLAN_CHECK_ENVIRONMENT: &[(&str, &str)] = &[
    ("PATH_LIKE", "../canary-secret.json"),
    ("SLASHED", "q/z"),
    ("SHORT", "[q"),
    ("BOUND", "$.q"),
    ("PATTERN", "^canary"),
    ("FORMAT", "json"),
];

fn plan_check_error(tmp: &Path, body: &str) -> String {
    let path = write(tmp, ".alint.yml", &format!("version: 2\n{body}rules: []\n"));
    load_in(&path, PLAN_CHECK_ENVIRONMENT)
        .unwrap_err()
        .to_string()
}

/// A plan-check error about a selector or relation that reads the
/// environment stands only when the same check fails with the same message
/// on a copy of the plan with those values masked, the relations a join
/// copies from included; otherwise it is redacted to the object, as a rule's
/// error is.
#[test]
fn a_plan_check_error_that_could_quote_the_environment_is_redacted() {
    let tmp = tempfile::tempdir().unwrap();
    let read = "    read: { json: $.a }\n    fields: { v: value }\n";
    for (body, object) in [
        (
            "selectors:\n  s:\n    paths: '*.json'\n    scope_filter: { has_ancestor: \
             '{{env.PATH_LIKE | default(\"x\")}}' }\n"
                .to_string(),
            "selectors.s",
        ),
        (
            "selectors:\n  s:\n    paths: '*.json'\n    scope_filter: { has_ancestor: \
             ['{{env.SLASHED | default(\"x\")}}'] }\n"
                .to_string(),
            "selectors.s",
        ),
        (
            "selectors:\n  s:\n    paths: ['ok/**', '{{env.SHORT | default(\"x\")}}']\n"
                .to_string(),
            "selectors.s",
        ),
        (
            format!(
                "relations:\n  r:\n    from: {{ paths: '*.json', scope_filter: {{ has_ancestor: \
                 '{{{{env.SLASHED | default(\"x\")}}}}' }} }}\n{read}"
            ),
            "relations.r",
        ),
        (
            "relations:\n  u:\n    union:\n      - { from: { paths: '*.json' }, read: { json: \
             $.a } }\n      - { from: { paths: '{{env.SHORT | default(\"x\")}}' }, read: { \
             json: $.b } }\n    fields: { v: value }\n"
                .to_string(),
            "relations.u",
        ),
        (
            "relations:\n  r:\n    from: { paths: '*.json' }\n    read: { json: $.a }\n    \
             fields: { v: 'bound: {{env.BOUND | default(\"$.x\")}}' }\n    transform: { v: \
             [strip-root] }\n"
                .to_string(),
            "relations.r",
        ),
        (
            "relations:\n  u:\n    union:\n      - { from: { paths: '*.json' }, read: { json: \
             $.a } }\n      - { from: { paths: '*.md' }, read: { regex: 'x(.*)' } }\n    \
             fields: { v: 'bound: {{env.BOUND | default(\"$.x\")}}' }\n"
                .to_string(),
            "relations.u",
        ),
        // A join's copy is described by the field it copies, in another
        // relation.
        (
            format!(
                "relations:\n  a:\n    from: {{ paths: '*.json' }}\n    read: {{ json: $.a }}\n    \
                 fields: {{ v: value, b: 'bound: {{{{env.BOUND | default(\"$.x\")}}}}' }}\n  \
                 c:\n    from: {{ paths: '*.json' }}\n{read}  j:\n    join: {{ left: a, right: c, \
                 on: {{ v: v }}, unique: right }}\n    fields: {{ x: left.b }}\n    transform: \
                 {{ x: [strip-root] }}\n"
            ),
            "relations.j",
        ),
    ] {
        let error = plan_check_error(tmp.path(), &body);
        assert_eq!(
            error,
            format!(
                ".alint.yml: `{object}`: <redacted> (the error could quote a value read from the \
                 environment)"
            ),
            "{body}"
        );
        for fragment in ["canary", "secret", "q/z", "[q", "$.q"] {
            assert!(!error.contains(fragment), "{fragment}: {error}");
        }
    }
}

/// An error the masked copy repeats quotes no value read from the
/// environment, and reads as before: a literal beside such a value in the
/// same selector, a relation whose pattern the environment gives, and a
/// relation beside one whose value cannot be masked (no format is
/// `<redacted>`). An error about that relation itself is redacted, as there
/// is no copy to rerun it on.
#[test]
fn a_plan_check_error_the_masked_copy_repeats_reads_as_before() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(
        plan_check_error(
            tmp.path(),
            "selectors:\n  s:\n    paths: '{{env.PATH_LIKE | default(\"x\")}}'\n    \
             scope_filter: { has_ancestor: 'a/b' }\n",
        ),
        ".alint.yml: `selectors.s.scope_filter`: scope_filter.has_ancestor name \"a/b\" must be \
         a basename - no path separators.\n  hint: to match files inside a specific subtree, use \
         the selector's own `paths:` (e.g. `paths: \"airflow-core/**/*.py\"`); to match files \
         in any subtree that has this manifest, use the basename only (e.g. `has_ancestor: \
         \"b\"`)."
    );
    assert_eq!(
        plan_check_error(
            tmp.path(),
            "relations:\n  r:\n    from: { paths: '*.json' }\n    read: { json: $.a }\n    \
             fields: { v: value }\n    where: { v: { matches: '{{env.PATTERN | \
             default(\"x\")}}' } }\n    transform: { vv: [trim] }\n",
        ),
        ".alint.yml: `relations.r.transform.vv`: `vv` is not a field of relation `r`; its fields \
         are `v`"
    );
    let unmaskable = "relations:\n  a:\n    from: { paths: '*.json' }\n    entries: { format: \
                      '{{env.FORMAT | default(\"x\")}}', at: $ }\n    fields: { k: entry.key \
                      }\n";
    assert_eq!(
        plan_check_error(
            tmp.path(),
            &format!(
                "{unmaskable}  b:\n    from: {{ paths: '*.json' }}\n    read: {{ json: $.a }}\n    \
                 fields: {{ v: value }}\n    transform: {{ vv: [trim] }}\n"
            ),
        ),
        ".alint.yml: `relations.b.transform.vv`: `vv` is not a field of relation `b`; its fields \
         are `v`"
    );
    assert_eq!(
        plan_check_error(
            tmp.path(),
            &format!("{unmaskable}    transform: {{ kk: [trim] }}\n"),
        ),
        ".alint.yml: `relations.a`: <redacted> (the error could quote a value read from the \
         environment)"
    );
}

#[test]
fn a_fragment_without_a_version_inherits_version_two() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(tmp.path(), ".alint.yml", &format!("version: 2\n{OBJECTS}"));
    write(
        tmp.path(),
        ".alint.d/10-docs.yml",
        &format!(
            "selectors:\n  docs:\n    paths: docs/**\n  manifests:\n    paths: x\n{README_RULE}"
        ),
    );
    let raw = raw(&path).unwrap();
    assert_eq!(
        declared(&raw),
        vec![
            (
                ".alint.yml".to_string(),
                ConfigSourceClass::Root,
                vec!["manifests".to_string(), "deps".to_string()]
            ),
            (
                shown(".alint.d/10-docs.yml"),
                ConfigSourceClass::DropIn,
                // Each source has its own namespace: the fragment may reuse a
                // name its root declares.
                vec!["docs".to_string(), "manifests".to_string()]
            ),
        ]
    );
    assert_eq!(load(&path).unwrap().rules.len(), 1);
}

/// A config is named by its path under the top-level config's directory
/// however that directory is reached. The loader resolves each config's path,
/// so it resolves the directory too: reached through a symlink (a temporary
/// directory under macOS's `/var`, which is `/private/var`), every config was
/// named by its file name alone.
#[cfg(unix)]
#[test]
fn a_config_is_named_under_its_root_reached_through_a_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let linked = tmp.path().join("linked");
    std::os::unix::fs::symlink(&real, &linked).unwrap();
    write(&linked, "rules/base.yml", &format!("version: 2\n{OBJECTS}"));
    write(
        &linked,
        ".alint.d/10.yml",
        "selectors:\n  docs: { paths: 'docs/**' }\n",
    );
    let path = write(
        &linked,
        ".alint.yml",
        "version: 2\nextends:\n  - url: ./rules/base.yml\n    as: base\n",
    );
    let names: Vec<String> = declared(&raw(&path).unwrap())
        .into_iter()
        .map(|(name, _, _)| name)
        .collect();
    assert_eq!(names, ["rules/base.yml", ".alint.yml", ".alint.d/10.yml"]);

    std::fs::write(&path, "version: 1\nrules: []\n").unwrap();
    assert_eq!(
        load_error(&path),
        ".alint.d/10.yml: `selectors:` needs `version: 2`; this config is version 1, as its \
         root config is"
    );
    std::fs::write(
        &path,
        "version: 1\nextends: [./rules/base.yml]\nrules: []\n",
    )
    .unwrap();
    std::fs::remove_dir_all(real.join(".alint.d")).unwrap();
    assert_eq!(
        load_error(&path),
        "rules/base.yml: a version 1 config (.alint.yml) cannot extend this version 2 config; \
         give .alint.yml `version: 2`, or make rules/base.yml `version: 1`"
    );
}

#[test]
fn a_fragment_follows_a_version_one_root() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(tmp.path(), ".alint.yml", "version: 1\nrules: []\n");
    let fragment = write(tmp.path(), ".alint.d/10.yml", "selectors: {}\n");
    let shown = shown(".alint.d/10.yml");
    assert_eq!(
        load_error(&path),
        format!(
            "{shown}: `selectors:` needs `version: 2`; this config is version 1, as its root \
             config is"
        )
    );
    std::fs::write(&fragment, "version: 2\nrules: []\n").unwrap();
    assert_eq!(
        load_error(&path),
        format!("{shown}: drop-in config version 2 must equal root config version 1")
    );
}

#[test]
fn a_version_two_config_extends_local_sources_through_aliases() {
    let tmp = tempfile::tempdir().unwrap();
    let base = write(
        tmp.path(),
        "rules/base.yml",
        &format!("version: 2\n{OBJECTS}"),
    );
    write(
        tmp.path(),
        "rules/plain.yml",
        &format!("version: 1\n{README_RULE}"),
    );
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nextends:\n  - url: ./rules/base.yml\n    as: base\n  - ./rules/plain.yml\n\
         relations:\n  pinned:\n    from: base.manifests\n    read: { json: $.version }\n    \
         fields: { version: value }\n",
    );
    let raw = raw(&path).unwrap();
    assert_eq!(
        declared(&raw),
        vec![
            (
                shown("rules/base.yml"),
                ConfigSourceClass::LocalExtend,
                vec!["manifests".to_string(), "deps".to_string()]
            ),
            (
                ".alint.yml".to_string(),
                ConfigSourceClass::Root,
                vec!["pinned".to_string(), "base".to_string()]
            ),
        ]
    );
    let root = &raw.v2.sources[1];
    assert_eq!(
        root.aliases[0].1.target.as_ref(),
        Some(&alint_core::relation::plan::SourceLocator::Local(
            base.canonicalize().unwrap()
        )),
        "an alias leads to the file it extends"
    );
    let config = load(&path).unwrap();
    assert_eq!(config.version, 2);
    assert_eq!(config.rules.len(), 1);
}

#[test]
fn a_source_reached_twice_declares_its_objects_once() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "shared.yml", &format!("version: 2\n{OBJECTS}"));
    write(tmp.path(), "a.yml", "version: 2\nextends: [./shared.yml]\n");
    write(tmp.path(), "b.yml", "version: 2\nextends: [./shared.yml]\n");
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nextends: [./a.yml, ./b.yml]\n",
    );
    let raw = raw(&path).unwrap();
    assert_eq!(
        declared(&raw),
        vec![(
            "shared.yml".to_string(),
            ConfigSourceClass::LocalExtend,
            vec!["manifests".to_string(), "deps".to_string()]
        )]
    );
}

#[test]
fn a_version_one_config_cannot_extend_a_version_two_one() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "rules/base.yml",
        &format!("version: 2\n{OBJECTS}"),
    );
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 1\nextends: [./rules/base.yml]\n",
    );
    let base = Path::new("rules").join("base.yml").display().to_string();
    assert_eq!(
        load_error(&path),
        format!(
            "{base}: a version 1 config (.alint.yml) cannot extend this version 2 config; give \
             .alint.yml `version: 2`, or make {base} `version: 1`"
        )
    );
    // Deeper in a chain, the extending config is named as the chain shows it.
    write(
        tmp.path(),
        "rules/middle.yml",
        "version: 1\nextends: [./base.yml]\n",
    );
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nextends: [./rules/middle.yml]\n",
    );
    let middle = Path::new("rules").join("middle.yml").display().to_string();
    assert_eq!(
        load_error(&path),
        format!(
            "{base}: a version 1 config ({middle}) cannot extend this version 2 config; give \
             {middle} `version: 2`, or make {base} `version: 1`"
        )
    );
}

#[test]
fn a_remote_source_must_be_version_one() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = extends::Cache::at(tmp.path().join("cache"));
    let path = tmp.path().join(".alint.yml");
    let refused = [
        (
            "version: 2\nrules: []\n",
            "https://example.invalid/remote.yml: a remote `extends:` source must be `version: 1` \
             in this build; `version: 2` is accepted only in local files (the top-level config, \
             a local `extends:` target or an `.alint.d` fragment) and bundled rulesets \
             (`alint://bundled/...`)",
        ),
        (
            "version: 1\nrelations: {}\n",
            "https://example.invalid/remote.yml: `relations:` needs `version: 2`, which a remote \
             `extends:` source cannot use in this build: it must be `version: 1`",
        ),
    ];
    for (body, expected) in refused {
        let url = seed_remote(&cache, body);
        std::fs::write(&path, format!("version: 2\nextends: [\"{url}\"]\n")).unwrap();
        let error = load_with(&path, &LoadOptions::with_cache(cache.clone()))
            .unwrap_err()
            .to_string();
        assert_eq!(error, expected);
    }
    // An alias on a remote entry is refused before anything is fetched.
    std::fs::write(
        &path,
        "version: 2\nextends:\n  - url: https://example.invalid/r.yml#sha256-AAAA\n    as: r\n",
    )
    .unwrap();
    assert!(
        load_error(&path).starts_with(".alint.yml: `extends[0].as`: an alias names a local"),
        "{}",
        load_error(&path)
    );
}

#[test]
fn a_version_two_config_extends_a_version_one_remote_and_bundled_ruleset() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = extends::Cache::at(tmp.path().join("cache"));
    let url = seed_remote(&cache, &format!("version: 1\n{README_RULE}"));
    let path = tmp.path().join(".alint.yml");
    std::fs::write(
        &path,
        format!("version: 2\nextends: [\"{url}\", alint://bundled/oss-baseline@v1]\n{OBJECTS}"),
    )
    .unwrap();
    let config = load_with(&path, &LoadOptions::with_cache(cache)).unwrap();
    assert_eq!(config.version, 2);
    assert!(config.rules.iter().any(|rule| rule.id == "readme"));
    assert!(config.rules.len() > 1, "the bundled rules load too");
}

#[test]
fn a_nested_config_must_be_version_one() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nnested_configs: true\n",
    );
    let nested = write(tmp.path(), "pkg/.alint.yml", "version: 2\nrules: []\n");
    let error = load_error(&path);
    assert!(
        error.ends_with(
            ": a nested config (`nested_configs:`) must be `version: 1` in this build; \
             `version: 2` is accepted only in local files (the top-level config, a local \
             `extends:` target or an `.alint.d` fragment) and bundled rulesets \
             (`alint://bundled/...`)"
        ),
        "{error}"
    );
    assert!(error.contains("pkg"), "{error}");
    std::fs::write(&nested, "version: 1\nselectors: {}\n").unwrap();
    let error = load_error(&path);
    assert!(
        error.ends_with(
            ": `selectors:` needs `version: 2`, which a nested config (`nested_configs:`) \
             cannot use in this build: it must be `version: 1`"
        ),
        "{error}"
    );
    // A version 1 nested config under a version 2 root loads as before.
    std::fs::write(&nested, format!("version: 1\n{README_RULE}")).unwrap();
    assert_eq!(load(&path).unwrap().rules.len(), 1);
}

#[test]
fn a_parsed_string_must_be_version_one() {
    assert_eq!(
        parse("version: 2\nrules: []\n").unwrap_err().to_string(),
        "<inline>: a config parsed from a string (`alint_dsl::parse`) must be `version: 1` in \
         this build; `version: 2` is accepted only in local files (the top-level config, a \
         local `extends:` target or an `.alint.d` fragment) and bundled rulesets \
         (`alint://bundled/...`)"
    );
    assert_eq!(
        parse("version: 1\nselectors: {}\n")
            .unwrap_err()
            .to_string(),
        "<inline>: `selectors:` needs `version: 2`, which a config parsed from a string \
         (`alint_dsl::parse`) cannot use in this build: it must be `version: 1`"
    );
    assert_eq!(parse("version: 1\nrules: []\n").unwrap().version, 1);
}

#[test]
fn objects_ride_on_the_loaded_config_as_its_plan() {
    // No rule kind reads selectors and relations before the `constraint`
    // kind (phase 7): a v2 config's rules are its v1 rules, and its objects
    // are carried, resolved, on the loaded config.
    let tmp = tempfile::tempdir().unwrap();
    let with = write(
        tmp.path(),
        "a/.alint.yml",
        &format!("version: 2\n{OBJECTS}{README_RULE}"),
    );
    let without = write(
        tmp.path(),
        "b/.alint.yml",
        &format!("version: 2\n{README_RULE}"),
    );
    let (with, without) = (load(&with).unwrap(), load(&without).unwrap());
    assert_eq!(
        with.rules.iter().map(|rule| &rule.id).collect::<Vec<_>>(),
        without
            .rules
            .iter()
            .map(|rule| &rule.id)
            .collect::<Vec<_>>()
    );
    let plan = with
        .relation_plan
        .as_deref()
        .expect("a version 2 config has a plan");
    assert_eq!(plan.selectors[0].name, "manifests");
    assert_eq!(plan.relations[0].name, "deps");
    let empty = without.relation_plan.as_deref().expect("even an empty one");
    assert!(empty.selectors.is_empty() && empty.relations.is_empty());
    assert_eq!(empty.sources.len(), 1);
}

#[test]
fn bundled_version_two_ruleset_loads_aliases_overrides_and_isolates_config_files() {
    use alint_core::relation::plan::SourceLocator;

    let tmp = tempfile::tempdir().unwrap();
    let registry = alint_rules::builtin_registry();
    let options = LoadOptions::with_catalog(registry.catalog());

    // 1. Extending a bundled v2 ruleset with `as:` + overriding rule level +
    // referencing the bundled ruleset's relation from a consumer rule.
    let path = write(
        tmp.path(),
        ".alint.yml",
        r"version: 2
extends:
  - url: alint://bundled/js/workspace-deps@v2
    as: ws
rules:
  - id: js-workspace-deps-consistent-range
    level: warning
  - id: no-banned-dep
    kind: constraint
    input: ws.dependency-ranges
    assert:
      count:
        distinct: true
        field: dependency
        max: 100
    level: error
",
    );
    let config = load_with(&path, &options).expect("bundled v2 config loads");
    assert_eq!(config.version, 2);
    // `Config::config_files` must stay strictly on-disk paths (never include
    // `alint://bundled/...`), preserving fix/LSP self-edit protection.
    assert_eq!(config.config_files, vec![path.canonicalize().unwrap()]);
    let bundled_rule = config
        .rules
        .iter()
        .find(|r| r.id == "js-workspace-deps-consistent-range")
        .expect("bundled rule present");
    assert_eq!(bundled_rule.level, alint_core::Level::Warning);
    assert!(
        bundled_rule.extended_source,
        "bundled v2 rule must retain extended_source protection"
    );
    let plan = config
        .relation_plan
        .as_deref()
        .expect("bundled v2 config carries a relation plan");
    assert_eq!(plan.sources.len(), 2);
    assert_eq!(plan.sources[0].class, ConfigSourceClass::Bundled);
    assert_eq!(
        plan.sources[0].display,
        "alint://bundled/js/workspace-deps@v2"
    );
    assert_eq!(
        plan.sources[0].locator,
        SourceLocator::Bundled("alint://bundled/js/workspace-deps@v2".into())
    );
    assert_eq!(plan.sources[1].aliases.len(), 1);
    assert_eq!(plan.sources[1].aliases[0].name, "ws");
    assert_eq!(
        plan.sources[1].aliases[0].target,
        Some(alint_core::relation::plan::SourceIndex(0))
    );
    // Both the inherited bundled rule (whose `input:` came from `v2_sources`)
    // and the consumer rule referencing `ws.dependency-ranges` build cleanly.
    for spec in &config.rules {
        registry
            .build(spec)
            .unwrap_or_else(|e| panic!("rule '{}' failed to build: {e}", spec.id));
    }
}

#[test]
fn bundled_version_two_ruleset_direction_rule_and_env_redaction() {
    #[derive(Debug)]
    struct BundledEnv;
    impl crate::EnvironmentProvider for BundledEnv {
        fn get(&self, name: &str) -> Option<String> {
            (name == "BUNDLED_V2").then(|| "alint://bundled/js/workspace-deps@v2".to_string())
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    let registry = alint_rules::builtin_registry();
    let options = LoadOptions::with_catalog(registry.catalog());

    // 1. ADR-0022: a `version: 1` config extending a bundled `version: 2`
    // ruleset fails with a user config error (`Error::Other`, exit 2 — never
    // `Error::Internal` exit 3).
    let v1_path = write(
        tmp.path(),
        "v1/.alint.yml",
        "version: 1\nextends:\n  - alint://bundled/js/workspace-deps@v2\nrules: []\n",
    );
    let err = load_with(&v1_path, &options).unwrap_err();
    assert!(
        matches!(err, alint_core::Error::Other(_)),
        "expected Error::Other (exit 2), got {err:?}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("cannot extend this version 2 bundled ruleset")
            && msg.contains("`version: 2`"),
        "unexpected v1->v2 bundled error: {msg}"
    );

    // 2. Cache-hit edge: a v2 root extends BOTH the bundled v2 ruleset AND a
    // local v1 config that also tries to extend the same bundled v2 ruleset —
    // must still fail with `Error::Other` (exit 2) on the cached entry.
    write(
        tmp.path(),
        "mixed/v1-child.yml",
        "version: 1\nextends:\n  - alint://bundled/js/workspace-deps@v2\nrules: []\n",
    );
    let mixed_path = write(
        tmp.path(),
        "mixed/.alint.yml",
        "version: 2\nextends:\n  - alint://bundled/js/workspace-deps@v2\n  - ./v1-child.yml\nrules: []\n",
    );
    let err = load_with(&mixed_path, &options).unwrap_err();
    assert!(
        matches!(err, alint_core::Error::Other(_)),
        "expected Error::Other on cache-hit v1->v2 bundled edge, got {err:?}"
    );

    // 3. Environment-chosen bundled v2 ruleset redacts its display in PlanSource.
    let env_path = write(
        tmp.path(),
        "env/.alint.yml",
        "version: 2\nextends:\n  - url: '{{env.BUNDLED_V2}}'\n    as: secret_ws\nrules: []\n",
    );
    let mut env_opts = LoadOptions::with_catalog(registry.catalog());
    env_opts.environment = std::sync::Arc::new(BundledEnv);
    let env_cfg = load_with(&env_path, &env_opts).expect("env-chosen bundled v2 loads");
    let env_plan = env_cfg.relation_plan.as_deref().unwrap();
    assert_eq!(env_plan.sources[0].display, alint_core::CONFIGURATION_MASK);
    assert_eq!(
        env_plan.sources[1].aliases[0].url,
        alint_core::CONFIGURATION_MASK
    );
}
