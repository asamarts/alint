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
fn only_a_plain_integer_two_declares_version_two() {
    let declared = |yaml: &str| declared_version(document(yaml).as_ref(), yaml);
    assert_eq!(declared("version: 2\n"), Declared::Two);
    assert_eq!(declared("version: 2 # experimental\n"), Declared::Two);
    assert_eq!(declared("{rules: [], version: 2}\n"), Declared::Two);
    assert_eq!(declared("version: 1\n"), Declared::Integer("1".into()));
    assert_eq!(declared("version: 3\n"), Declared::Integer("3".into()));
    assert_eq!(declared("rules: []\n"), Declared::Absent);
    for other_two in [
        "version: '2'\n",
        "version: \"2\"\n",
        "version: 2.0\n",
        "version: +2\n",
        "version: 0x2\n",
        "version: 0o2\n",
        "version: !!int 2\n",
        "version: !!str 2\n",
        "version: !custom 2\n",
        "version: &v 2\n",
        "anchor: &v 2\nversion: *v\n",
        "version: |\n  2\n",
    ] {
        assert_eq!(declared(other_two), Declared::OtherTwo, "{other_two}");
    }
    for non_literal in [
        "version: '1'\n",
        // A string to the parser, as `'02'` is.
        "version: 02\n",
        "version: 1.5\n",
        "version: [2]\n",
        "version: ~\n",
    ] {
        assert_eq!(declared(non_literal), Declared::NonLiteral, "{non_literal}");
    }
    assert_eq!(
        declared("version: '{{env.V | default(\"2\")}}'\n"),
        Declared::Environment
    );
    assert_eq!(declared("- 2\n"), Declared::Unreadable);
    assert_eq!(declared("version: [\n"), Declared::Unreadable);
}

/// A 2 written other than as the plain integer is an error that says what to
/// write, rather than a version 1 config of an unsupported version.
#[test]
fn another_spelling_of_two_says_to_write_a_plain_two() {
    for yaml in [
        "version: '2'\nrules: []\n",
        "version: 0x2\nselectors: {}\n",
        "version: +2\n",
        "version: !!int 2\n",
        "v: &v 2\nversion: *v\n",
    ] {
        assert_eq!(
            message(dispatched(yaml, &root())),
            ".alint.yml: `version:` is 2 written other than as the plain integer `2` (quoted, \
             tagged, through an alias, or as `+2`, `0x2` or `2.0`); write `version: 2` to select \
             version 2",
            "{yaml}"
        );
    }
    // Where version 2 may not be, that is the error.
    let remote = Origin::new(ConfigSourceClass::Remote, "https://example.invalid/r.yml");
    assert_eq!(
        message(dispatched("version: 0x2\n", &remote)),
        "https://example.invalid/r.yml: a remote `extends:` source must be `version: 1` in this \
         build; `version: 2` is accepted only in local files (the top-level config, a local \
         `extends:` target or an `.alint.d` fragment) and bundled rulesets (`alint://bundled/...`)"
    );
}

/// A version 2 source that does not parse (a duplicate key in a rule) reports
/// the parse error, not the version 1 decode's reading of its version 2 keys;
/// a version 1 one is left to the version 1 decode, as before.
#[test]
fn a_version_two_source_that_does_not_parse_reports_the_parse_error() {
    let duplicate = "version: 2\nselectors: {}\nrules:\n  - {id: a, id: b}\n";
    let error = read_document(duplicate, &root()).unwrap_err();
    assert!(matches!(error, Error::Yaml(_)), "{error:?}");
    assert!(error.to_string().contains("duplicate"), "{error}");
    let fragment =
        Origin::new(ConfigSourceClass::DropIn, ".alint.d/10.yml").consumer(Some(2), None);
    let inherited = "selectors: {}\nrules:\n  - {id: a, id: b}\n";
    assert!(matches!(
        read_document(inherited, &fragment),
        Err(Error::Yaml(_))
    ));
    for version_one in [
        "version: 1\nselectors: {}\nrules:\n  - {id: a, id: b}\n",
        "selectors: {}\nrules:\n  - {id: a, id: b}\n",
        "version: 0x2\nrules:\n  - {id: a, id: b}\n",
        "version: [\n",
    ] {
        assert!(
            read_document(version_one, &root()).unwrap().is_none(),
            "{version_one}"
        );
    }
    let remote = Origin::new(ConfigSourceClass::Remote, "https://example.invalid/r.yml");
    assert!(
        message(read_document(duplicate, &remote))
            .contains("a remote `extends:` source must be `version: 1`")
    );
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
            "version: '1'\nselectors: {}\n",
            ".alint.yml: `selectors:` needs `version: 2`; this config's `version:` is not the \
             integer `2`",
        ),
        // An `extends:` that is not a list does not hide a later key.
        (
            "version: 1\nextends:\nselectors: {}\n",
            ".alint.yml: `selectors:` needs `version: 2`; this config is version 1",
        ),
        (
            "version: 1\nextends: ./a.yml\nrelations: {}\n",
            ".alint.yml: `relations:` needs `version: 2`; this config is version 1",
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
                 `.alint.d` fragment) and bundled rulesets (`alint://bundled/...`)"
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
    // Bundled rulesets MAY be version 2 under a version 2 consumer, and are
    // refused with a user config error under a version 1 consumer (ADR-0050 D10).
    let bundled_v2 = Origin::new(
        ConfigSourceClass::Bundled,
        "alint://bundled/js/workspace-deps@v2",
    )
    .consumer(Some(2), Some(".alint.yml"));
    assert_eq!(
        dispatched("version: 2\nrules: []\n", &bundled_v2).unwrap(),
        Surface::V2
    );
    let bundled_under_v1 = Origin::new(
        ConfigSourceClass::Bundled,
        "alint://bundled/js/workspace-deps@v2",
    )
    .consumer(Some(1), Some(".alint.yml"));
    assert_eq!(
        message(dispatched("version: 2\nrules: []\n", &bundled_under_v1)),
        "alint://bundled/js/workspace-deps@v2: a version 1 config (.alint.yml) cannot extend this \
         version 2 bundled ruleset; give .alint.yml `version: 2`"
    );
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
    check_document(&document(yaml).expect("parse"), yaml, ".alint.yml")
}

#[test]
fn an_unknown_key_is_reported_against_the_version_two_keys() {
    assert_eq!(
        message(checked("version: 2\nrelation: {}\n")),
        ".alint.yml: unknown key `relation` in a version 2 config; expected one of `version`, \
         `requires`, `extends`, `ignore`, `respect_gitignore`, `vars`, `facts`, `templates`, \
         `rules`, `fix_size_limit`, `nested_configs`, `limits`, `allow_out_of_root`, `baseline`, \
         `trusted_extends`, `selectors`, `relations` at line 2 column 1"
    );
    // Block with comments, flow mapping, and non-string top-level keys report
    // exact line and column too (#566).
    let commented = message(checked("version: 2\n# comment\n  \nbogus: 1\n"));
    assert!(
        commented.ends_with("at line 4 column 1"),
        "expected line 4 column 1, got: {commented}"
    );
    let flow = message(checked("{version: 2, bogus: 1}\n"));
    assert!(
        flow.ends_with("at line 1 column 14"),
        "expected line 1 column 14 for flow mapping key, got: {flow}"
    );
    let non_string = message(checked("version: 2\n123: 1\n"));
    assert!(
        non_string.ends_with("at line 2 column 1"),
        "expected line 2 column 1 for non-string key, got: {non_string}"
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
        // A rule mapping without `kind:` (an override by id, a template's
        // instance), or with one the environment or a later override sets,
        // can end up a `constraint`.
        (
            format!("rules:\n  - id: c\n    input: {env}\n"),
            "rules[0].input".to_string(),
        ),
        (
            format!(
                "rules:\n  - id: c\n    extends_template: t\n    input: r\n    assert:\n      \
                 unique: {{ key: [{env}] }}\n"
            ),
            "rules[0].assert.unique.key[0]".to_string(),
        ),
        (
            format!("rules:\n  - id: c\n    kind: {env}\n    input: {env}\n"),
            "rules[0].input".to_string(),
        ),
        (
            format!("templates:\n  - id: c\n    kind: file_exists\n    input: {env}\n"),
            "templates[0].input".to_string(),
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

/// A text the pre-check lets keep its version 1 decode error is one the
/// dispatch reads as version 1: every spelling of a 2, a v2-only key, an
/// alias to one or an escape in one makes the loader parse it.
#[test]
fn a_text_kept_from_the_dispatch_parse_dispatches_as_version_one() {
    let fragment = Origin {
        consumer_version: Some(VERSION),
        ..Origin::new(ConfigSourceClass::DropIn, ".alint.d/a.yml")
    };
    let texts = [
        "version: 1\nnope: 1\n",
        "version: 1\nrules: 3\n",
        "version: 3\nnope: 1\n",
        "version: 10\nnope: 1\n",
        "version: 1 # 2\nnope: 1\n",
        "{version: 1, nope: 1}\n",
        "version: 1\nrules:\n  - {id: a, kind: b, message: \"bump the version first\"}\n",
        "version: 1\nrules:\n  - {id: a, kind: b, message: such as that}\n",
        "version: 1\nnope:\n\tx\n",
        "nope: 1\n",
        // Prose that reads like a key costs a parse, and no more.
        "version: 1\nrules:\n  - {id: a, kind: b, message: \"bump the version: 2 now\"}\n",
        "version: 2\nnope: 1\n",
        "# 2\nversion: 2\nnope: 1\n",
        "version: '2'\nnope: 1\n",
        "version: \"2\"\nnope: 1\n",
        "version: 2.0\nnope: 1\n",
        "version: +2\nnope: 1\n",
        "version: 0x2\nnope: 1\n",
        "version: 0o2\nnope: 1\n",
        "version: 02\nnope: 1\n",
        "version: 2e0\nnope: 1\n",
        "version: !!int 2\nnope: 1\n",
        "version:\n  2\nnope: 1\n",
        "\"version\": 2\nnope: 1\n",
        "'version' : 2\nnope: 1\n",
        "{version\n: 2, nope: 1}\n",
        "? version\n: 2\nnope: 1\n",
        "? |-\n  version\n: 2\nnope: 1\n",
        "\"versio\\x6e\": 2\nnope: 1\n",
        "\"versio\\u006e\": 2\nnope: 1\n",
        "\"versi\\\n  on\": 2\nnope: 1\n",
        "vars: {a: &v 2}\nversion: *v\nnope: 1\n",
        "vars: {a: &k version}\n*k : 2\nnope: 1\n",
        "version: 1\nselectors: {}\nnope: 1\n",
        "version: 1\nrelations: {}\nnope: 1\n",
        "version: 1\nextends:\n  - {url: ./a.yml, as: a}\nnope: 1\n",
        "version: 1\nextends:\n  - {url: ./a.yml, \"as\": a}\nnope: 1\n",
        "version: 1\n\"selector\\x73\": {}\nnope: 1\n",
    ];
    for origin in [root(), fragment] {
        for text in texts {
            if may_select_version_two(text, &origin) {
                continue;
            }
            let read = read_document(text, &origin);
            assert!(
                matches!(&read, Ok(document)
                    if dispatch(document.as_ref(), text, &origin).is_ok_and(|surface| surface == Surface::V1)),
                "{text:?} ({:?}) skips the parse, but dispatches as {:?}",
                origin.class,
                read.map(|document| dispatch(document.as_ref(), text, &origin))
            );
        }
    }
    // The texts a version 1 config is likely to fail with skip it.
    for text in &texts[..10] {
        assert!(!may_select_version_two(text, &root()), "{text:?}");
    }
    // A fragment of a version 2 root is version 2 without a word.
    assert!(may_select_version_two("nope: 1\n", &fragment));
}

/// V2L-9 (a): a null `version:` beside a v2-only key was told a version read
/// from the environment is version 1, which it is not; only one read from
/// the environment is told so.
#[test]
fn only_a_version_from_the_environment_is_told_about_the_environment() {
    for (version, why) in [
        ("~", "; this config's `version:` is not the integer `2`"),
        ("null", "; this config's `version:` is not the integer `2`"),
        ("[2]", "; this config's `version:` is not the integer `2`"),
        (
            "'{{env.V}}'",
            "; this config's `version:` is not the integer `2` (a version read from the \
             environment is version 1)",
        ),
    ] {
        assert_eq!(
            message(dispatched(
                &format!("version: {version}\nselectors: {{}}\n"),
                &root()
            )),
            format!(".alint.yml: `selectors:` needs `version: 2`{why}"),
            "{version}"
        );
    }
}

/// V2L-9 (b): a decimal that rounds to 2 as a float (`1.9999999999999999`)
/// was refused as "2 written other than as the plain integer"; it is no 2,
/// and is version 1's to refuse, as `1.5` is. A decimal that is exactly 2 is
/// still a 2 written another way.
#[test]
fn a_decimal_is_two_only_when_it_is_exactly_two() {
    let declared = |yaml: &str| declared_version(document(yaml).as_ref(), yaml);
    for near in [
        "1.9999999999999999",
        "2.0000000000000001",
        "19999999999999999e-16",
    ] {
        assert_eq!(
            declared(&format!("version: {near}\n")),
            Declared::NonLiteral,
            "{near}"
        );
    }
    for two in ["2.0", "2e0", "20e-1", "0.2e1", "+2.000", "2.", "0002.0"] {
        assert_eq!(
            declared(&format!("version: {two}\n")),
            Declared::OtherTwo,
            "{two}"
        );
    }
    assert_eq!(decimal("-0.0"), Some((String::new(), 0)));
    assert_eq!(decimal("0x2"), None);
    assert_eq!(decimal(".inf"), None);
}

/// V2L-9 (e): a v2-only key was reported before an unknown key written
/// before it; the dispatch leaves the earlier one to the version 1 decode.
#[test]
fn an_unknown_key_before_a_v2_only_key_is_reported_first() {
    assert_eq!(
        dispatched("version: 1\nnope: 1\nselectors: {}\n", &root()).unwrap(),
        Surface::V1
    );
    assert_eq!(
        message(dispatched("version: 1\nselectors: {}\nnope: 1\n", &root())),
        ".alint.yml: `selectors:` needs `version: 2`; this config is version 1"
    );
}
