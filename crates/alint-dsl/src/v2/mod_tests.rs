//! Unit tests of the version dispatch, the v2 document checks and the
//! literal-name check (`v2/mod.rs`, `v2/literal.rs`).

use super::*;

fn document(yaml: &str) -> Option<Value> {
    alint_core::yaml::from_str(yaml).ok()
}

fn dispatched(yaml: &str, origin: &Origin<'_>) -> Result<Surface> {
    dispatch(document(yaml).as_ref(), yaml, origin)
}

fn root() -> Origin<'static> {
    Origin::new(ConfigSourceClass::Root, ".alint.yml")
}

fn message(result: Result<impl std::fmt::Debug>) -> String {
    result.expect_err("expected an error").to_string()
}

#[test]
fn only_a_literal_integer_two_declares_version_two() {
    let declared = |yaml: &str| declared_version(document(yaml).as_ref(), yaml);
    assert_eq!(declared("version: 2\n"), Declared::Two);
    assert_eq!(declared("version: 2 # experimental\n"), Declared::Two);
    assert_eq!(declared("version: 1\n"), Declared::Integer("1".into()));
    assert_eq!(declared("version: 3\n"), Declared::Integer("3".into()));
    assert_eq!(declared("rules: []\n"), Declared::Absent);
    for non_literal in [
        "version: '2'\n",
        "version: \"2\"\n",
        "version: 2.0\n",
        "version: !!int 2\n",
        "anchor: &v 2\nversion: *v\n",
        "version: '{{env.V | default(\"2\")}}'\n",
        "version: [2]\n",
        "version: ~\n",
    ] {
        assert_eq!(declared(non_literal), Declared::NonLiteral, "{non_literal}");
    }
    assert_eq!(declared("- 2\n"), Declared::Unreadable);
    assert_eq!(declared("version: [\n"), Declared::Unreadable);
}

#[test]
fn a_literal_two_selects_version_two_in_each_local_source() {
    assert_eq!(dispatched("version: 2\n", &root()).unwrap(), Surface::V2);
    let extend = Origin::new(ConfigSourceClass::LocalExtend, "base.yml")
        .consumer(Some(2), Some(".alint.yml"));
    assert_eq!(dispatched("version: 2\n", &extend).unwrap(), Surface::V2);
    let fragment =
        Origin::new(ConfigSourceClass::DropIn, ".alint.d/10.yml").consumer(Some(2), None);
    assert_eq!(dispatched("version: 2\n", &fragment).unwrap(), Surface::V2);
}

#[test]
fn a_fragment_without_a_version_decodes_as_its_root() {
    let yaml = "selectors:\n  docs:\n    paths: docs/**\n";
    let under_two =
        Origin::new(ConfigSourceClass::DropIn, ".alint.d/10.yml").consumer(Some(2), None);
    assert_eq!(dispatched(yaml, &under_two).unwrap(), Surface::V2);
    let under_one =
        Origin::new(ConfigSourceClass::DropIn, ".alint.d/10.yml").consumer(Some(1), None);
    assert_eq!(
        message(dispatched(yaml, &under_one)),
        ".alint.d/10.yml: `selectors:` needs `version: 2`; this config is version 1, as its root \
         config is"
    );
    assert_eq!(dispatched("rules: []\n", &under_one).unwrap(), Surface::V1);
}

#[test]
fn a_version_one_config_is_version_one() {
    assert_eq!(
        dispatched("version: 1\nrules: []\n", &root()).unwrap(),
        Surface::V1
    );
    assert_eq!(
        dispatched("version: '{{env.V | default(\"1\")}}'\n", &root()).unwrap(),
        Surface::V1
    );
    // A document that does not parse is left to the v1 decode's error.
    assert_eq!(dispatched("version: [\n", &root()).unwrap(), Surface::V1);
}

#[test]
fn version_two_keys_in_a_version_one_config_name_the_version_they_need() {
    let cases = [
        (
            "version: 1\nselectors: {}\n",
            ".alint.yml: `selectors:` needs `version: 2`; this config is version 1",
        ),
        (
            "version: 1\nrules: []\nrelations: {}\n",
            ".alint.yml: `relations:` needs `version: 2`; this config is version 1",
        ),
        (
            "version: 1\nextends:\n  - ./a.yml\n  - url: ./b.yml\n    as: b\n",
            ".alint.yml: `extends[1].as` needs `version: 2`; this config is version 1",
        ),
        (
            "rules: []\nselectors: {}\n",
            ".alint.yml: `selectors:` needs `version: 2`; this config declares no `version:`",
        ),
        (
            "version: '2'\nselectors: {}\n",
            ".alint.yml: `selectors:` needs `version: 2`; this config's `version:` is not the \
             literal integer `2` (a version read from the environment, quoted, tagged or written \
             as an alias is version 1)",
        ),
        (
            "version: 3\nrelations: {}\n",
            ".alint.yml: `relations:` needs `version: 2`; this config is version 3",
        ),
    ];
    for (yaml, expected) in cases {
        assert_eq!(message(dispatched(yaml, &root())), expected, "{yaml}");
    }
}

#[test]
fn the_first_version_two_key_in_document_order_is_named() {
    let yaml = "version: 1\nrelations: {}\nselectors: {}\n";
    assert_eq!(
        message(dispatched(yaml, &root())),
        ".alint.yml: `relations:` needs `version: 2`; this config is version 1"
    );
}

#[test]
fn version_two_is_refused_in_each_source_that_must_be_version_one() {
    let cases = [
        (
            ConfigSourceClass::Remote,
            "https://example.invalid/r.yml",
            "a remote `extends:` source",
        ),
        (
            ConfigSourceClass::Bundled,
            "alint://bundled/x@v1",
            "a bundled ruleset",
        ),
        (
            ConfigSourceClass::Nested,
            "pkg/.alint.yml",
            "a nested config (`nested_configs:`)",
        ),
        (
            ConfigSourceClass::Inline,
            "<inline>",
            "a config parsed from a string (`alint_dsl::parse`)",
        ),
    ];
    for (class, display, noun) in cases {
        let origin = Origin::new(class, display).consumer(Some(2), Some(".alint.yml"));
        assert_eq!(
            message(dispatched("version: 2\nrules: []\n", &origin)),
            format!(
                "{display}: {noun} must be `version: 1` in this build; `version: 2` is accepted \
                 only in local files (the top-level config, a local `extends:` target or an \
                 `.alint.d` fragment)"
            ),
        );
        assert_eq!(
            message(dispatched("version: 1\nselectors: {}\n", &origin)),
            format!(
                "{display}: `selectors:` needs `version: 2`, which {noun} cannot use in this \
                 build: it must be `version: 1`"
            ),
        );
    }
}

#[test]
fn a_fragment_cannot_be_version_two_under_a_version_one_root() {
    let fragment =
        Origin::new(ConfigSourceClass::DropIn, ".alint.d/10.yml").consumer(Some(1), None);
    assert_eq!(
        message(dispatched("version: 2\n", &fragment)),
        ".alint.d/10.yml: drop-in config version 2 must equal root config version 1"
    );
}

#[test]
fn a_version_one_config_cannot_extend_a_version_two_one() {
    let extend = Origin::new(ConfigSourceClass::LocalExtend, "rules/base.yml")
        .consumer(Some(1), Some(".alint.yml"));
    assert_eq!(
        message(dispatched("version: 2\n", &extend)),
        "rules/base.yml: a version 1 config (.alint.yml) cannot extend this version 2 config; \
         give .alint.yml `version: 2`, or make rules/base.yml `version: 1`"
    );
    // A version 2 config may extend version 1 and version 2 sources.
    let from_two = Origin::new(ConfigSourceClass::LocalExtend, "rules/base.yml")
        .consumer(Some(2), Some(".alint.yml"));
    assert_eq!(dispatched("version: 1\n", &from_two).unwrap(), Surface::V1);
    assert_eq!(dispatched("version: 2\n", &from_two).unwrap(), Surface::V2);
}

fn checked(yaml: &str) -> Result<()> {
    check_document(&document(yaml).expect("parse"), ".alint.yml")
}

#[test]
fn an_unknown_key_is_reported_against_the_version_two_keys() {
    assert_eq!(
        message(checked("version: 2\nrelation: {}\n")),
        ".alint.yml: unknown key `relation` in a version 2 config; expected one of `version`, \
         `requires`, `extends`, `ignore`, `respect_gitignore`, `vars`, `facts`, `templates`, \
         `rules`, `fix_size_limit`, `nested_configs`, `allow_out_of_root`, `baseline`, \
         `trusted_extends`, `selectors`, `relations`"
    );
    assert!(checked("version: 2\nselectors: {}\nrelations: {}\nrules: []\n").is_ok());
}

#[test]
fn the_version_one_keys_are_read_from_the_v1_decode() {
    // The list follows `RawConfig`, so a key added there is a v2 key too.
    let keys = document::v1_keys();
    assert_eq!(keys.first(), Some(&"version"));
    assert!(keys.contains(&"trusted_extends"));
    assert!(!keys.contains(&"selectors"));
    assert!(!keys.contains(&"env_vars"), "a skipped field is not a key");
}

#[test]
fn an_extends_entry_is_checked_against_the_version_two_keys() {
    assert_eq!(
        message(checked(
            "version: 2\nextends:\n  - url: ./a.yml\n    excpet: [x]\n"
        )),
        ".alint.yml: `extends[0]`: unknown key `excpet` in an `extends:` entry; expected one of \
         `url`, `only`, `except`, `as`"
    );
    assert_eq!(
        message(checked(
            "version: 2\nextends:\n  - ./a.yml\n  - url: ./b.yml\n    as: [b]\n"
        )),
        ".alint.yml: `extends[1].as`: expected an alias name, found a list"
    );
    assert!(checked("version: 2\nextends:\n  - ./a.yml\n  - url: ./b.yml\n    as: b\n").is_ok());
}

/// The error for `{{...}}` at `path` of `.alint.yml`.
fn interpolated_at(path: &str) -> String {
    format!(
        ".alint.yml: `{path}`: `{{{{...}}}}` is not allowed in a name, alias or reference of a \
         version 2 config, which must name the same thing in every environment; write it \
         literally"
    )
}

#[test]
#[allow(clippy::too_many_lines)] // one table: a case per place a name is written
fn a_name_alias_or_reference_must_be_literal() {
    let env = "'{{env.N | default(\"n\")}}'";
    let cases = [
        (
            format!("selectors:\n  {env}: {{ paths: x }}\n"),
            "selectors.{{env.N | default(\"n\")}}".to_string(),
        ),
        (
            format!("relations:\n  {env}: {{}}\n"),
            "relations.{{env.N | default(\"n\")}}".to_string(),
        ),
        (
            format!("relations:\n  r:\n    from: {env}\n"),
            "relations.r.from".to_string(),
        ),
        (
            format!("relations:\n  r:\n    union:\n      - from: s\n      - from: {env}\n"),
            "relations.r.union[1].from".to_string(),
        ),
        (
            format!("relations:\n  r:\n    join: {{ left: {env} }}\n"),
            "relations.r.join.left".to_string(),
        ),
        (
            format!("relations:\n  r:\n    join: {{ right: {env} }}\n"),
            "relations.r.join.right".to_string(),
        ),
        (
            format!("relations:\n  r:\n    join: {{ on: {{ {env}: k }} }}\n"),
            "relations.r.join.on.{{env.N | default(\"n\")}}".to_string(),
        ),
        (
            format!("relations:\n  r:\n    join: {{ on: {{ k: {env} }} }}\n"),
            "relations.r.join.on.k".to_string(),
        ),
        (
            format!("relations:\n  r:\n    fields: {{ {env}: path }}\n"),
            "relations.r.fields.{{env.N | default(\"n\")}}".to_string(),
        ),
        (
            format!("relations:\n  r:\n    fields: {{ f: {env} }}\n"),
            "relations.r.fields.f".to_string(),
        ),
        (
            format!("relations:\n  r:\n    fields: {{ f: {{ source: {env} }} }}\n"),
            "relations.r.fields.f.source".to_string(),
        ),
        (
            format!("relations:\n  r:\n    transform: {{ {env}: [trim] }}\n"),
            "relations.r.transform.{{env.N | default(\"n\")}}".to_string(),
        ),
        (
            format!("relations:\n  r:\n    where: {{ {env}: {{ matches: x }} }}\n"),
            "relations.r.where.{{env.N | default(\"n\")}}".to_string(),
        ),
        (
            format!("relations:\n  r:\n    where: {{ f: {{ in: [{{ relation: {env} }}] }} }}\n"),
            "relations.r.where.f.in[0].relation".to_string(),
        ),
        (
            format!("relations:\n  r:\n    where: {{ f: {{ not_in: [{{ field: {env} }}] }} }}\n"),
            "relations.r.where.f.not_in[0].field".to_string(),
        ),
        (
            format!("extends:\n  - url: ./a.yml\n    as: {env}\n"),
            "extends[0].as".to_string(),
        ),
        (
            format!("rules:\n  - id: c\n    kind: constraint\n    input: {env}\n"),
            "rules[0].input".to_string(),
        ),
        (
            format!(
                "rules:\n  - id: c\n    kind: constraint\n    input:\n      from: s\n      \
                 fields: {{ {env}: path }}\n"
            ),
            "rules[0].input.fields.{{env.N | default(\"n\")}}".to_string(),
        ),
        (
            format!(
                "templates:\n  - id: c\n    kind: constraint\n    assert:\n      unique: \
                 {{ key: {env} }}\n"
            ),
            "templates[0].assert.unique.key".to_string(),
        ),
        (
            format!(
                "rules:\n  - id: c\n    kind: constraint\n    assert:\n      unique: \
                 {{ group_by: [a, {env}] }}\n"
            ),
            "rules[0].assert.unique.group_by[1]".to_string(),
        ),
    ];
    for (body, path) in cases {
        let yaml = format!("version: 2\n{body}");
        let document = document(&yaml).unwrap_or_else(|| panic!("parse {yaml}"));
        assert_eq!(
            message(reject_interpolated_names(&document, ".alint.yml")),
            interpolated_at(&path),
            "{yaml}"
        );
    }
}

#[test]
fn other_version_two_values_may_be_interpolated() {
    let yaml = r#"version: 2
selectors:
  docs:
    paths: '{{env.DOCS | default("docs")}}/**'
relations:
  r:
    from: { paths: '{{env.P | default("x")}}' }
    read: { regex: '{{env.RX | default("x")}}' }
    fields:
      f: 'bound: {{env.Q | default("$.name")}}'
      g: { source: 'bound: {{env.Q | default("$.name")}}', sensitive: true }
    where:
      f: { matches: '{{env.M | default("x")}}', in: ['{{env.L | default("x")}}'] }
rules:
  - id: other
    kind: file_exists
    paths: '{{env.R | default("README.md")}}'
"#;
    assert!(reject_interpolated_names(&document(yaml).unwrap(), ".alint.yml").is_ok());
}
