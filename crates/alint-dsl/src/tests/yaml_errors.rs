//! What a configuration's author sees when alint's YAML decoding refuses
//! it: the field, what goes there, and what was found.

use super::*;

/// The error from loading `text` as a repository's `.alint.yml`, with the
/// file's path left out.
fn load_error(text: &str) -> String {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join(".alint.yml");
    std::fs::write(&config, text).unwrap();
    let error = load(&config).unwrap_err().to_string();
    // A configuration the loader reads through `extends:` handling is named by
    // its canonical path (`/private/var/...` on macOS, the long form of a
    // Windows short name, without `\\?\`), others as given: replace both.
    let canonical = std::fs::canonicalize(&config)
        .unwrap()
        .display()
        .to_string();
    let canonical = canonical.strip_prefix(r"\\?\").unwrap_or(&canonical);
    error
        .replace(canonical, "<config>")
        .replace(&config.display().to_string(), "<config>")
}

/// The error from building the only rule of `text`, which loads.
fn build_error(text: &str) -> String {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join(".alint.yml");
    std::fs::write(&config, text).unwrap();
    let config = load(&config).unwrap();
    let [rule] = config.rules.as_slice() else {
        panic!("{:?}", config.rules)
    };
    test_registry().build(rule).err().unwrap().to_string()
}

const RULE: &str = "version: 1\nrules:\n  - id: a\n    kind: file_exists\n    paths: README.md\n";

const LEVELS: &str = "expected one of `error`, `warning`, `info`, `off`";

/// API-3: `level: {error: ~}` loaded as `level: error`, as ayaml reads a
/// one-entry mapping where an enum goes; the parser v0.22 used refused it,
/// and so does alint, with every value that is not a level's name.
#[test]
fn a_level_is_one_of_the_levels_names() {
    for (level, found) in [
        ("{error: ~}", "a mapping"),
        ("{error: 1}", "a mapping"),
        ("1", "integer `1`"),
        ("true", "boolean `true`"),
        ("[error]", "a sequence"),
        ("", "null"),
    ] {
        assert_eq!(
            load_error(&format!("{RULE}    level: {level}\n")),
            format!(
                "rule \"a\": could not deserialize merged rule: level: {LEVELS}, found {found}"
            ),
            "{level:?}"
        );
        // A configuration with an interpolation is decoded from its value.
        assert_eq!(
            load_error(&format!(
                "{RULE}    message: '{{{{vars.m}}}}'\n    level: {level}\nvars: {{m: x}}\n"
            )),
            format!(
                "rule \"a\": could not deserialize merged rule: level: {LEVELS}, found {found}"
            ),
            "{level:?}"
        );
        // `parse` decodes the rules with the configuration, and reports the
        // error at the value (an empty one where what follows starts).
        let error = crate::parse(&format!("{RULE}    level: {level}\n"))
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with(&format!(
                "YAML parse error: rules[0].level: {LEVELS}, found {found} at line "
            )),
            "{level:?}: {error}"
        );
    }
    // A template's rule is decoded as its instance is.
    assert_eq!(
        load_error(
            "version: 1\ntemplates:\n  - id: t\n    kind: file_exists\n    paths: x\n    \
             level: {warning: ~}\nrules:\n  - id: a\n    extends_template: t\n"
        ),
        format!("rule \"a\": could not deserialize merged rule: level: {LEVELS}, found a mapping")
    );
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join(".alint.yml");
    std::fs::write(&config, format!("{RULE}    level: warning\n")).unwrap();
    assert_eq!(
        load(&config).unwrap().rules[0].level,
        alint_core::Level::Warning
    );
}

/// YAML-1a: a rule's option of the wrong type named only its kind, as
/// `non-string YAML value`; it names the value, as v0.22.1 did.
#[test]
fn a_rule_option_of_the_wrong_type_is_named() {
    let rule = "version: 1\nrules:\n  - id: a\n    kind: file_content_forbidden\n    \
                paths: x\n    level: error\n";
    for (pattern, found) in [
        ("1", "integer `1`"),
        ("1.5", "floating point `1.5`"),
        ("true", "boolean `true`"),
        ("~", "unit value"),
        ("[1]", "sequence"),
        ("{a: b}", "map"),
    ] {
        assert_eq!(
            build_error(&format!("{rule}    pattern: {pattern}\n")),
            format!(
                "rule \"a\": invalid options: YAML parse error: pattern: invalid type: {found}, \
                 expected a string"
            ),
            "{pattern:?}"
        );
    }
    // A rule's own field too, and a variable of an interpolated configuration.
    assert_eq!(
        load_error(&format!("{RULE}    level: error\n    message: [x]\n")),
        "rule \"a\": could not deserialize merged rule: message: invalid type: sequence, \
         expected a string"
    );
    assert_eq!(
        load_error("version: 1\nvars: {a: 1, b: '{{env.HOME | default(\"x\")}}'}\nrules: []\n"),
        "<config>: YAML parse error: vars.a: invalid type: integer `1`, expected a string"
    );
}

/// YAML-4: a tag where a number or a boolean goes, in an `extends:` entry or
/// in a fact was refused as `invalid type: enum`, as not a string or a map,
/// or as enum input to an untagged enum; the error names the tag, as alint's
/// own tag errors in a rule do.
#[test]
fn a_tag_a_field_cannot_read_is_named() {
    for (text, field, tag) in [
        ("version: !!timestamp 1\n", "`version", "!!timestamp"),
        (
            "version: 1\nrespect_gitignore: !!binary true\n",
            "`respect_gitignore",
            "!!binary",
        ),
        (
            "version: 1\nextends: [!!binary ./base.yml]\n",
            "`extends[0]",
            "!!binary",
        ),
        (
            "version: 1\nextends: [!<tag:x,2000:y> ./base.yml]\n",
            "`extends[0]",
            "tag:x,2000:y",
        ),
        (
            "version: 1\nfacts:\n  - id: f\n    any_file_exists: !!binary README.md\n",
            "fact \"f\": `any_file_exists",
            "!!binary",
        ),
    ] {
        let error = load_error(text);
        assert!(
            error.contains(&format!(
                "{field}` has the YAML tag `{tag}`; YAML tags are not supported here"
            )),
            "{text:?}: {error}"
        );
    }
}

const HINT: &str = "(check this line's indentation against the lines above)";

/// YAML-1c: an indentation mistake ends the root collection; ayaml 0.2.2
/// names the parent collection when off by one, and otherwise reports content
/// after the document, to which alint adds a hint.
#[test]
fn an_indentation_mistake_gets_a_hint() {
    let off_by_one = "version: 1\nrules:\n  - id: a\n   kind: file_exists\n";
    for error in [
        load_error(off_by_one),
        crate::parse(off_by_one).unwrap_err().to_string(),
    ] {
        assert!(
            error.ends_with(
                "this line is indented differently from the sequence at line 3 column 3 at line 4 \
                 column 4"
            ),
            "{off_by_one:?}: {error}"
        );
    }
    for (text, place) in [
        (
            "version: 1\nrules:\n  - id: a\n    kind: file_exists\n  -id: b\n",
            "line 5 column 3",
        ),
        (" rules: []\nversion: 1\n", "line 2 column 1"),
    ] {
        for error in [
            load_error(text),
            crate::parse(text).unwrap_err().to_string(),
        ] {
            assert!(
                error.ends_with(&format!(
                    "unexpected content after document at {place} {HINT}"
                )),
                "{text:?}: {error}"
            );
        }
    }
    for text in ["version: 1\n---\nrules: []\n", "[1] x\n"] {
        let error = load_error(text);
        assert!(!error.contains(HINT), "{text:?}: {error}");
    }
}
