use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::*;
use crate::Level;
// These tests measure alint's layer over ayaml, so they decode text with
// ayaml whatever parser `ALINT_YAML_PARSER` chooses.
use crate::yaml::{Value, ayaml_from_str as from_str, from_value};

#[derive(Debug, Deserialize)]
struct Rule {
    level: Level,
}

#[derive(Debug, Deserialize)]
struct Pattern {
    pattern: String,
}

#[derive(Debug, Deserialize)]
struct Outer {
    inner: Inner,
}

#[derive(Debug, Deserialize)]
struct Inner {
    #[serde(default)]
    a: u32,
}

#[derive(Debug, Deserialize)]
struct Settings {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    #[allow(dead_code)]
    flag: bool,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Deserialize)]
struct Fact {
    any: OneOrMany,
}

/// `input`'s error decoded from text, and decoded from its value tree.
fn errors<T: DeserializeOwned + std::fmt::Debug>(input: &str) -> [String; 2] {
    let text = from_str::<T>(input).unwrap_err().message().to_owned();
    let value: Value = from_str(input).unwrap();
    let value = from_value::<T>(value).unwrap_err().message().to_owned();
    [text, value]
}

/// `input` decoded from text and from its value tree.
fn both<T: DeserializeOwned>(input: &str) -> [T; 2] {
    let value: Value = from_str(input).unwrap();
    [from_str(input).unwrap(), from_value(value).unwrap()]
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum One {
    Strict,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Two {
    Strict,
    Contains,
}

const LEVELS: &str = "expected one of `error`, `warning`, `info`, `off`";

const SHAPES: &str = "expected one of `empty`, `named`, `pair`, `point`";

/// API-3: a one-entry mapping where a unit-only enum goes, which ayaml reads
/// as serde's externally tagged form, is refused, as v0.22 refused it, and
/// every value that is not a variant's name says what it is.
#[test]
fn a_unit_enum_takes_only_a_variants_name() {
    let error = from_str::<Level>("{error: ~}").unwrap_err();
    assert_eq!(error.message(), format!("{LEVELS}, found a mapping"));
    let location = error.location().unwrap();
    assert_eq!((location.line(), location.column()), (1, 1));
    for (input, found) in [
        ("level: {error: ~}", "a mapping"),
        ("level: {error: 1}", "a mapping"),
        ("level:\n  paths: x", "a mapping"),
        ("level: {}", "a mapping"),
        ("level: [error]", "a sequence"),
        ("level: 1", "integer `1`"),
        ("level: -1", "integer `-1`"),
        (
            "level: 100000000000000000000000",
            "integer `100000000000000000000000`",
        ),
        ("level: 1.5", "float `1.5`"),
        ("level: true", "boolean `true`"),
        ("level: ~", "null"),
        ("level:", "null"),
        ("level: !error", "a value with the YAML tag `!error`"),
        (
            "level: !<tag:x,2000:error> x",
            "a value with the YAML tag `tag:x,2000:error`",
        ),
    ] {
        let expected = format!("level: {LEVELS}, found {found}");
        assert_eq!(
            errors::<Rule>(input),
            [expected.clone(), expected],
            "{input:?}"
        );
    }
    // The error is where the value is.
    let error = from_str::<Rule>("level: {error: ~}").unwrap_err();
    let location = error.location().unwrap();
    assert_eq!((location.line(), location.column()), (1, 8));
    // A variant's name reads as before, quoted or not, and so does an
    // unknown one's error.
    for (input, level) in [
        ("level: error", Level::Error),
        ("level: 'off'", Level::Off),
        ("level: off", Level::Off),
        ("level: !!str warning", Level::Warning),
        ("level: \"info\"", Level::Info),
    ] {
        assert_eq!(both::<Rule>(input).map(|rule| rule.level), [level; 2]);
    }
    assert_eq!(
        errors::<Rule>("level: Error"),
        [0, 1].map(|_| format!("level: unknown variant `Error`, {LEVELS}"))
    );
    // One variant, or two, read as serde lists them.
    assert_eq!(
        errors::<One>("[strict]"),
        ["expected `strict`, found a sequence"; 2]
    );
    assert_eq!(
        errors::<Two>("[strict]"),
        ["expected `strict` or `contains`, found a sequence"; 2]
    );
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Shape {
    Empty,
    Named(String),
    Pair(u32, u32),
    Point { x: u32, y: u32 },
}

/// An enum's variant with data still reads in serde's externally tagged
/// form, as a one-entry mapping or a tag; only a variant without data takes
/// its name alone.
#[test]
fn a_variant_with_data_still_reads_from_a_mapping_or_a_tag() {
    for (input, shape) in [
        ("empty", Shape::Empty),
        ("{named: x}", Shape::Named("x".into())),
        ("!named x", Shape::Named("x".into())),
        ("{pair: [1, 2]}", Shape::Pair(1, 2)),
        ("!pair [1, 2]", Shape::Pair(1, 2)),
        ("{point: {x: 1, y: 2}}", Shape::Point { x: 1, y: 2 }),
        ("point:\n  x: 1\n  y: 2\n", Shape::Point { x: 1, y: 2 }),
    ] {
        assert_eq!(both::<Shape>(input), [shape.clone(), shape], "{input:?}");
    }
    for (input, found) in [
        ("{empty: ~}", "a mapping"),
        ("{other: x}", "a mapping"),
        ("{1: x}", "a mapping"),
        ("{}", "a mapping"),
        ("!empty ~", "a value with the YAML tag `!empty`"),
        ("!other x", "a value with the YAML tag `!other`"),
        ("1", "integer `1`"),
    ] {
        let expected = format!("{SHAPES}, found {found}");
        assert_eq!(
            errors::<Shape>(input),
            [expected.clone(), expected],
            "{input:?}"
        );
    }
    let expected = "a variant with data is a mapping of one entry, the variant's name and its data";
    assert_eq!(errors::<Shape>("{named: x, pair: [1, 2]}"), [expected; 2]);
    // The data's errors are its own, with its path.
    let value: Value = from_str("{named: [x]}").unwrap();
    assert_eq!(
        from_value::<Shape>(value).unwrap_err().message(),
        "named: invalid type: sequence, expected a string"
    );
    assert_eq!(
        errors::<Shape>("{point: {x: 1}}"),
        ["point: missing field `y`"; 2]
    );
}

/// YAML-1a: a value of the wrong type where a string goes, decoded from a
/// value tree, is named, where ayaml says `non-string YAML value`. From text,
/// a string reads any scalar's text, as it did.
#[test]
fn a_type_error_names_what_was_found() {
    for (input, found) in [
        ("pattern: 1", "integer `1`"),
        ("pattern: -1", "integer `-1`"),
        (
            "pattern: 100000000000000000000000",
            "integer `100000000000000000000000`",
        ),
        ("pattern: 1.5", "floating point `1.5`"),
        ("pattern: true", "boolean `true`"),
        ("pattern: ~", "unit value"),
        ("pattern: [1]", "sequence"),
        ("pattern: {a: b}", "map"),
    ] {
        let value: Value = from_str(input).unwrap();
        assert_eq!(
            from_value::<Pattern>(value).unwrap_err().message(),
            format!("pattern: invalid type: {found}, expected a string"),
            "{input:?}"
        );
    }
    // A string, tagged or not, reads; from text, so does a number's text.
    for input in ["pattern: x", "pattern: !t x", "pattern: !!binary x"] {
        assert_eq!(both::<Pattern>(input).map(|p| p.pattern), ["x", "x"]);
    }
    assert_eq!(from_str::<Pattern>("pattern: 1").unwrap().pattern, "1");
    // The path is ayaml's, in a sequence or a map too.
    let value: Value = from_str("- pattern: x\n- pattern: 2\n").unwrap();
    assert_eq!(
        from_value::<Vec<Pattern>>(value).unwrap_err().message(),
        "[1].pattern: invalid type: integer `2`, expected a string"
    );
    let value: Value = from_str("a b: 1\n").unwrap();
    assert_eq!(
        from_value::<BTreeMap<String, String>>(value)
            .unwrap_err()
            .message(),
        "[\"a b\"]: invalid type: integer `1`, expected a string"
    );
    // A struct from a value tree names what it found, where ayaml says
    // `non-mapping YAML value`, and reads past a tag outside YAML's own.
    for (input, found) in [
        ("inner: 1", "integer `1`"),
        ("inner: x", "string"),
        ("inner: true", "boolean `true`"),
        ("inner: ~", "unit value"),
        ("inner: [1]", "sequence"),
    ] {
        let value: Value = from_str(input).unwrap();
        assert_eq!(
            from_value::<Outer>(value).unwrap_err().message(),
            format!("inner: invalid type: {found}, expected struct Inner"),
            "{input:?}"
        );
    }
    for input in [
        "inner: {a: 1}",
        "inner: !t {a: 1}",
        "inner: !<tag:x,2000:y> {a: 1}",
    ] {
        assert_eq!(both::<Outer>(input).map(|outer| outer.inner.a), [1, 1]);
    }
}

/// YAML-4: a tag a typed field cannot read is named, where ayaml's error
/// speaks of an enum, or of enum input to an untagged enum.
#[test]
fn a_tag_a_typed_field_cannot_read_is_named() {
    for (input, path, tag) in [
        ("version: !!timestamp 1", "version", "!!timestamp"),
        ("flag: !!binary true", "flag", "!!binary"),
        ("flag: !!merge true", "flag", "!!merge"),
    ] {
        let expected =
            format!("`{path}` has the YAML tag `{tag}`; YAML tags are not supported here");
        assert_eq!(
            errors::<Settings>(input),
            [expected.clone(), expected],
            "{input:?}"
        );
    }
    // The error is where ayaml put it, at the tagged value's content.
    let error = from_str::<Settings>("version: !!timestamp 1").unwrap_err();
    let location = error.location().unwrap();
    assert_eq!((location.line(), location.column()), (1, 22));
    for (input, tag) in [
        ("any: !!binary README.md", "!!binary"),
        ("any: !<tag:x,2000:y> README.md", "tag:x,2000:y"),
        ("any: !foo README.md", "!foo"),
    ] {
        let expected = format!("`any` has the YAML tag `{tag}`; YAML tags are not supported here");
        assert_eq!(
            errors::<Fact>(input),
            [expected.clone(), expected],
            "{input:?}"
        );
    }
    assert_eq!(
        from_str::<u32>("!!binary 1").unwrap_err().message(),
        "the value has the YAML tag `!!binary`; YAML tags are not supported here"
    );
    assert_eq!(
        from_str::<BTreeMap<String, u32>>("a b: !!binary 1")
            .unwrap_err()
            .message(),
        "`[\"a b\"]` has the YAML tag `!!binary`; YAML tags are not supported here"
    );
    // A tag that ayaml looks past still reads, and a YAML value keeps any tag.
    for input in ["version: !foo 1", "version: !<tag:x,2000:y> 1"] {
        assert_eq!(both::<Settings>(input).map(|s| s.version), [1, 1]);
    }
    let value: Value = from_str("a: !!binary x\nb: [!t 1]\n").unwrap();
    assert_eq!(value["a"].tag().as_deref(), Some("!!binary"));
    assert_eq!(value["b"][0].tag().as_deref(), Some("!t"));
    let value: Value = from_value(value).unwrap();
    assert_eq!(value["a"].tag().as_deref(), Some("!!binary"));
}

/// Only a tag error, in the form [`TagRefused`] writes, is reworded.
#[test]
fn only_a_tag_error_is_reworded() {
    let error = |message: &str| ayaml::Error::new(ayaml::ErrorKind::Serde, message.to_owned());
    for message in [
        "a: has the YAML tag `x y`; YAML tags are not supported here",
        "a: has the YAML tag ``; YAML tags are not supported here",
        "a: has the YAML tag `x`; YAML tags are not supported",
        "a: invalid type: enum, expected u32",
        "a has the YAML tag `x`; YAML tags are not supported here",
    ] {
        assert_eq!(finish(error(message)).message(), message);
    }
    assert_eq!(
        finish(error(
            "[\"k: has the YAML tag `y`\"].b: has the YAML tag `!x`; YAML tags are not \
             supported here"
        ))
        .message(),
        "`[\"k: has the YAML tag `y`\"].b` has the YAML tag `!x`; YAML tags are not supported \
         here"
    );
}

/// Enums with data are untagged in alint, and still read each form.
#[test]
fn an_untagged_enum_with_data_still_reads() {
    assert_eq!(
        both::<Fact>("any: x").map(|fact| fact.any),
        [OneOrMany::One("x".into()), OneOrMany::One("x".into())]
    );
    assert_eq!(
        both::<Fact>("any: [x]").map(|fact| fact.any),
        [
            OneOrMany::Many(vec!["x".into()]),
            OneOrMany::Many(vec!["x".into()])
        ]
    );
    let input = "id: a\nkind: file_exists\nlevel: warning\npaths: {include: [x], exclude: [y]}\n\
                 fix: {file_create: {content: x}}\nwhen: 'true'\n";
    for spec in both::<crate::RuleSpec>(input) {
        assert_eq!(spec.level, Level::Warning);
        assert!(matches!(
            spec.paths,
            Some(crate::PathsSpec::IncludeExclude { .. })
        ));
        assert!(spec.fix.is_some());
    }
}

const HINTED: &str = "unexpected content after document (check this line's indentation \
                      against the lines above)";

/// YAML-1c: the common indentation mistakes end the root collection, and
/// ayaml reports what follows as content after the document; alint adds a
/// hint, after the location, which stays where it was.
#[test]
fn an_indentation_mistake_gets_a_hint() {
    let rule = "version: 1\nrules:\n  - id: a\n    kind: file_exists\n";
    for (input, line, column) in [
        // A key one space less, or more, than its siblings.
        (
            "version: 1\nrules:\n  - id: a\n   kind: file_exists\n",
            4,
            4,
        ),
        (
            "version: 1\nrules:\n  - id: a\n     kind: file_exists\n",
            4,
            6,
        ),
        // A key at the entry's `-`.
        ("version: 1\nrules:\n  - id: a\n  kind: file_exists\n", 4, 3),
        // `-id` with no space.
        (&format!("{rule}  -id: b\n"), 5, 3),
        (&format!("{rule} -id: b\n"), 5, 2),
        // A top-level key indented, after or before the others.
        ("version: 1\n rules: []\n", 2, 2),
        ("version: 1\n  rules: []\n", 2, 3),
        (" rules: []\nversion: 1\n", 2, 1),
        // A comment between the two lines changes nothing.
        ("version: 1\n# c\n  rules: []\n", 3, 3),
        ("version: 1\n  # c\n  rules: []\n", 3, 3),
        // A value indented under a scalar.
        (
            &format!("{rule}    level: error\n      message: hi\n"),
            6,
            7,
        ),
    ] {
        let error = from_str::<Value>(input).unwrap_err();
        assert_eq!(error.message(), HINTED, "{input:?}");
        let location = error.location().unwrap();
        assert_eq!(
            (location.line(), location.column()),
            (line, column),
            "{input:?}"
        );
        assert_eq!(
            error.to_string(),
            format!(
                "unexpected content after document at line {line} column {column} (check \
                 this line's indentation against the lines above)"
            )
        );
        // The parser's own error is unchanged, for the harness.
        assert_eq!(
            error.into_parser().message(),
            "unexpected content after document"
        );
        // A data file's error gets it too.
        let error = crate::yaml_backend::json_tree(input).unwrap_err();
        assert_eq!(error.message(), HINTED, "{input:?}");
    }
    for input in [
        // A second document is another error.
        "version: 1\n---\nrules: []\n",
        // Content after a flow collection.
        "[1]\nx\n",
        "[1] x\n",
        "{a: 1}\n b\n",
        // Content where a key goes, at its siblings' column.
        "a: 1\n\"b\"\n",
        "a: 1\n[x]\n",
        // A tab, which ayaml says.
        "a:\n\t- b\n",
    ] {
        let error = from_str::<Value>(input).unwrap_err();
        assert!(
            !error.to_string().contains("check this line"),
            "{input:?}: {error}"
        );
    }
}

#[test]
fn a_line_s_content_starts_past_its_entries() {
    assert_eq!(content_column("a: 1"), 0);
    assert_eq!(content_column("  - id: a"), 4);
    assert_eq!(content_column("  - - x"), 6);
    assert_eq!(content_column("  -"), 2);
    assert_eq!(content_column("  -x"), 2);
    assert_eq!(content_column("-\tx"), 2);
    assert!(holds_content("  a"));
    for line in ["", "  ", "  # c", "---", "--- # c", "...", "%YAML 1.2"] {
        assert!(!holds_content(line), "{line:?}");
    }
    assert!(holds_content("---x"));
}
