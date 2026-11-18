//! Whether two errors for one rule are one. Both parsers' errors at
//! [`Entry::Rule`](super::Entry::Rule) are placed at the rule's position
//! only, so the place says nothing of which field failed or why: ayaml's
//! `message: invalid type ..., expected a string` and the oracle's `invalid
//! type: integer `1`, expected field identifier` are both at the rule's
//! line. They are one when they agree on the problem and on the field.

use std::borrow::Cow;

use super::{Case, Rejection, guarded};

/// Whether ayaml's and the oracle's errors for the rule of `case` are one:
/// the same problem (see [`problem`]), in the same field. ayaml puts the
/// path to the value in front of its message, as `message: `; the oracle
/// gives none, and serde builds its errors for a rule's fields from the
/// entries it gathered for the flattened options, out of reach of any path.
/// So the oracle's field is the one its error comes and goes with (see
/// [`oracle_error_is_in`]).
pub(super) fn same_rule_error(case: &Case<'_>) -> bool {
    let (Err(ayaml), Err(oracle)) = (&case.ayaml, &case.oracle) else {
        return false;
    };
    if let Some(field) = tag_refusal_field(&ayaml.message) {
        return oracle_reads_a_tag(&oracle.message)
            && oracle_error_is_in(case.input, case.rule, &field, oracle);
    }
    let (field, ayaml_problem) = ayaml_path(&ayaml.message)
        .map_or((None, ayaml.message.as_str()), |(field, problem)| {
            (Some(field), problem)
        });
    (problem(ayaml_problem) == problem(&oracle.message)
        || tag_as_variant(ayaml_problem, &oracle.message))
        && field.is_none_or(|field| oracle_error_is_in(case.input, case.rule, &field, oracle))
}

/// The rule's field that alint's tag error names, as `command` in
/// `` `command` has the YAML tag `!t`; YAML tags are not supported here ``
/// (see [`crate::yaml_decode`]), which puts the path in backticks in front.
/// The field is the key the path's first step names (see [`ayaml_path`]).
fn tag_refusal_field(message: &str) -> Option<String> {
    let path = message
        .strip_prefix('`')?
        .strip_suffix("`; YAML tags are not supported here")?
        .split_once("` has the YAML tag `")?
        .0;
    ayaml_path(&format!("{path}: x")).map(|(field, _)| field.into_owned())
}

/// Whether the oracle's error is its own for a tagged value where a value
/// with no tag goes: it reads a tag as an enum's, so it refuses one where an
/// untagged enum goes (a string or a list, as `command:` takes), or names
/// an enum as the type it found.
fn oracle_reads_a_tag(message: &str) -> bool {
    message == "untagged and internally tagged enums do not support enum input"
        || message.starts_with("invalid type: enum, expected ")
}

/// Whether alint's error for a tagged value where an enum's name goes, as
/// ``expected one of `a`, `b`, found a value with the YAML tag `!t` ``, and
/// the oracle's error are one: the oracle reads the local tag `!t` as the
/// name of a variant, `` unknown variant `t`, expected one of `a`, `b` ``,
/// with the same variants expected.
fn tag_as_variant(ayaml: &str, oracle: &str) -> bool {
    let Some((expected, tag)) = ayaml
        .strip_prefix("expected ")
        .and_then(|rest| rest.strip_suffix('`'))
        .and_then(|rest| rest.split_once(", found a value with the YAML tag `"))
    else {
        return false;
    };
    let Some(name) = tag
        .strip_prefix('!')
        .filter(|name| !name.starts_with(['!', '<']))
    else {
        return false;
    };
    oracle == format!("unknown variant `{name}`, expected {expected}")
}

/// The length of the name at the start of `text`: a letter or `_`, then
/// letters, digits, `_` and `-`; 0 when `text` starts with none.
fn name_len(text: &str) -> usize {
    let mut chars = text.char_indices();
    match chars.next() {
        Some((_, first)) if first.is_ascii_alphabetic() || first == '_' => chars
            .find(|(_, next)| !(next.is_ascii_alphanumeric() || matches!(next, '_' | '-')))
            .map_or(text.len(), |(at, _)| at),
        _ => 0,
    }
}

/// ayaml's `message` split into the key that the first step of the path in
/// front of it names, when that step is a name or a quoted key, as a step
/// to a rule's field or option is, and the problem after the path. A path's
/// steps are names, quoted keys (`["a b"]`), indexes (`[0]`) and keys that
/// are collections (`[?]`), with a `.` before a name, and it ends at `: `; a
/// problem starts with words and spaces, as `invalid type: `, so none reads
/// as a path. ayaml quotes a key that is not a name, as the merge key `<<`
/// in `["<<"].pattern[1]`.
fn ayaml_path(message: &str) -> Option<(Cow<'_, str>, &str)> {
    let name = name_len;
    let (first, mut at) = match name(message) {
        0 => {
            let len = quoted_key_len(message)?;
            (unquoted(&message[2..len - 2])?, len)
        }
        len => (Cow::Borrowed(&message[..len]), len),
    };
    loop {
        let rest = &message[at..];
        if let Some(problem) = rest.strip_prefix(": ") {
            return Some((first, problem));
        }
        at += if let Some(after) = rest.strip_prefix('.') {
            match name(after) {
                0 => return None,
                len => 1 + len,
            }
        } else if rest.starts_with("[\"") {
            quoted_key_len(rest)?
        } else {
            let after = rest.strip_prefix('[')?;
            let close = after.find(']')?;
            let step = &after[..close];
            if step != "?" && (step.is_empty() || !step.bytes().all(|byte| byte.is_ascii_digit())) {
                return None;
            }
            1 + close + 1
        };
    }
}

/// The length of the quoted key at the start of `text`, as `["a \"b"]`,
/// brackets and quotes included, when it starts with one.
fn quoted_key_len(text: &str) -> Option<usize> {
    let after = text.strip_prefix("[\"")?;
    let mut escaped = false;
    let (close, _) = after.char_indices().find(|&(_, next)| {
        let close = !escaped && next == '"';
        escaped = !escaped && next == '\\';
        close
    })?;
    after[close + 1..].starts_with(']').then_some(2 + close + 2)
}

/// The key that `quoted`, the text between a quoted key's quotes, names:
/// ayaml puts a `\` before a `"` or a `\`, and writes a character that
/// could hide text, as a control, as `\u{..}` in hexadecimal. A key of more
/// than 64 characters, which ayaml cuts short with `…`, then names no key
/// of the rule's but one written so.
fn unquoted(quoted: &str) -> Option<Cow<'_, str>> {
    if !quoted.contains('\\') {
        return Some(Cow::Borrowed(quoted));
    }
    let mut key = String::with_capacity(quoted.len());
    let mut chars = quoted.chars();
    while let Some(next) = chars.next() {
        if next != '\\' {
            key.push(next);
            continue;
        }
        match chars.next()? {
            escaped @ ('"' | '\\') => key.push(escaped),
            'u' => {
                let rest = chars.as_str().strip_prefix('{')?;
                let (hex, after) = rest.split_once('}')?;
                key.push(char::from_u32(u32::from_str_radix(hex, 16).ok()?)?);
                chars = after.chars();
            }
            _ => return None,
        }
    }
    Some(Cow::Owned(key))
}

/// Whether `text` is alint's error for an enum's value that is not a
/// variant's name, as `expected one of `a`, `b`, found a mapping` (see
/// [`crate::yaml_decode`]).
fn ayaml_enum_shape(text: &str) -> bool {
    text.starts_with("expected `") || text.starts_with("expected one of `")
}

/// A rule error's problem, with what an "invalid type" or "invalid value"
/// found left out: alint names a string where a struct goes without quoting
/// it, as `invalid type: string`, and the oracle quotes it, so the two agree
/// on what was expected only. An enum's value that is not a variant's name
/// is one problem, which alint words as `expected one of ..., found ...` and
/// the oracle as `invalid type: ..., expected a Value::Tagged enum`.
fn problem(text: &str) -> String {
    if (ayaml_enum_shape(text) && text.contains(", found "))
        || (text.starts_with("invalid type: ") && text.ends_with(", expected a Value::Tagged enum"))
    {
        return "an enum's value that is not a variant's name".to_string();
    }
    for prefix in ["invalid type: ", "invalid value: "] {
        if text.starts_with(prefix)
            && let Some((_, expected)) = text.rsplit_once(", expected ")
        {
            return format!("{prefix}..., expected {expected}");
        }
    }
    text.to_string()
}

/// Whether the oracle's error for the rule at index `rule` of `input` is in
/// its field `field`: the oracle gives it for the rule cut down to that
/// field alone, and not for the rule with no field. serde reports a field's
/// error as it reads the field's entry, before it finds a field the rule
/// needs missing, so the field alone shows its error, as `id` alone shows
/// one of its own, not `kind`'s.
fn oracle_error_is_in(input: &str, rule: usize, field: &str, oracle: &Rejection) -> bool {
    let Some(mapping) = oracle_rule(input, rule) else {
        return false;
    };
    let error = |keep: &dyn Fn(&str) -> bool| {
        let cut: yaml_serde::Mapping = mapping
            .iter()
            .filter(|(key, _)| key.as_str().is_some_and(keep))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        // A panic is an error with a message of its own.
        let (result, _) = guarded(|| {
            yaml_serde::from_value::<crate::RuleSpec>(yaml_serde::Value::Mapping(cut))
                .map_err(|error| super::oracle_rejection(&error))
        });
        result.err().map(|rejection| rejection.message)
    };
    let with = error(&|key| key == field);
    let without = error(&|_| false);
    with.as_ref() == Some(&oracle.message) && without.as_ref() != Some(&oracle.message)
}

/// The oracle's value of the rule at index `rule` of `rules:` in `input`,
/// when it is a mapping.
fn oracle_rule(input: &str, rule: usize) -> Option<yaml_serde::Mapping> {
    let (value, _) = guarded(|| {
        yaml_serde::from_str::<yaml_serde::Value>(input)
            .map_err(|error| super::oracle_rejection(&error))
    });
    value
        .ok()?
        .get("rules")?
        .as_sequence()?
        .get(rule)?
        .as_mapping()
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_the_names_indexes_and_keys_before_the_problem() {
        assert_eq!(
            ayaml_path("message: invalid type: integer `1`, expected a string"),
            Some((
                "message".into(),
                "invalid type: integer `1`, expected a string"
            ))
        );
        assert_eq!(
            ayaml_path("scope_filter.has_ancestor[1]: x"),
            Some(("scope_filter".into(), "x"))
        );
        assert_eq!(
            ayaml_path("a[\"b \\\"c]: d\"][?].e-f: g"),
            Some(("a".into(), "g"))
        );
        assert_eq!(ayaml_path("a[\"b\"]: x"), Some(("a".into(), "x")));
        // A name starts with a letter or `_`.
        assert_eq!(ayaml_path("_a: x"), Some(("_a".into(), "x")));
        // A first step that is a quoted key names the key, unescaped.
        assert_eq!(
            ayaml_path("[\"<<\"].pattern[1]: x"),
            Some(("<<".into(), "x"))
        );
        assert_eq!(ayaml_path("[\"1\"]: x"), Some(("1".into(), "x")));
        assert_eq!(
            ayaml_path("[\"a: \\\"b\\\\\\u{1b}\"][0]: x"),
            Some(("a: \"b\\\u{1b}".into(), "x"))
        );
        for message in [
            "1a: x",
            "-a: x",
            "invalid type: integer `1`, expected struct RuleSpec",
            "missing field `id`",
            "a glob string, a list of globs, or an `{ include, exclude }` map",
            "[0]: x",
            "[?]: x",
            "[\"a\": x",
            "[\"a\\q\"]: x",
            "[\"a\\u{d800}\"]: x",
            "a.: x",
            "a[x]: y",
            "a[]: y",
            "a[\"b\"x: y",
            "a",
        ] {
            assert_eq!(ayaml_path(message), None, "{message:?}");
        }
    }

    /// ayaml quotes a key that is not a name in its path, which the path's
    /// first step reads back as the key, but for one it cut short.
    #[test]
    fn a_quoted_first_step_reads_back_as_ayamls_key() {
        let field = |written: &str| {
            let input = format!("{{{written}: [x]}}");
            let error =
                crate::yaml::ayaml_from_str::<std::collections::BTreeMap<String, String>>(&input)
                    .unwrap_err()
                    .into_parser();
            ayaml_path(error.message()).map(|(field, _)| field.into_owned())
        };
        for (written, key) in [
            ("'<<'", "<<"),
            ("'1'", "1"),
            ("\"a: \\\"b\\\\\"", "a: \"b\\"),
            ("\"\\e\\N\\L\\x7f é\"", "\u{1b}\u{85}\u{2028}\u{7f} é"),
        ] {
            assert_eq!(field(written).as_deref(), Some(key), "{written:?}");
        }
        let long = "k".repeat(65);
        assert_ne!(field(&long).as_deref(), Some(long.as_str()));
    }

    #[test]
    fn an_invalid_type_or_value_is_compared_on_what_was_expected() {
        assert_eq!(
            problem("invalid type: string, expected struct ScopeFilterSpec"),
            problem("invalid type: string \"x\", expected struct ScopeFilterSpec")
        );
        assert_eq!(
            problem("invalid value: string \"a, expected b\", expected a boolean"),
            "invalid value: ..., expected a boolean"
        );
        assert_ne!(
            problem("invalid type: integer `1`, expected a string"),
            problem("invalid type: integer `1`, expected field identifier")
        );
        assert_eq!(problem("missing field `id`"), "missing field `id`");
        for ayaml in [
            "expected one of `error`, `warning`, `info`, `off`, found a mapping",
            "expected `a` or `b`, found null",
            "expected `a`, found integer `1`",
        ] {
            assert_eq!(
                problem(ayaml),
                problem("invalid type: unit value, expected a Value::Tagged enum")
            );
        }
        // An unknown variant's name both word alike, so it stays as it is,
        // and so does a message that names no value found.
        assert_ne!(
            problem("unknown variant `x`, expected `a` or `b`"),
            problem("invalid type: unit value, expected a Value::Tagged enum")
        );
        assert_ne!(
            problem("expected one of `a`, `b`, `c`"),
            problem("invalid type: unit value, expected a Value::Tagged enum")
        );
        assert_ne!(
            problem("invalid type: unit value, expected a Value::Tagged enum"),
            problem("invalid type: unit value, expected a string")
        );
    }

    #[test]
    fn a_tag_refusal_names_the_first_step_of_its_path() {
        let refused = |path: &str| {
            format!("`{path}` has the YAML tag `!t`; YAML tags are not supported here")
        };
        let field = |path: &str| tag_refusal_field(&refused(path));
        assert_eq!(field("command").as_deref(), Some("command"));
        assert_eq!(field("paths[0]").as_deref(), Some("paths"));
        assert_eq!(
            field("scope_filter.has_ancestor").as_deref(),
            Some("scope_filter")
        );
        assert_eq!(field("[\"<<\"].pattern[1]").as_deref(), Some("<<"));
        assert_eq!(field("[\"<<\"]").as_deref(), Some("<<"));
        for message in [
            refused("1a"),
            refused(""),
            "command has the YAML tag `!t`; YAML tags are not supported here".to_string(),
            "`command` has the YAML tag `!t`".to_string(),
            "level: expected one of `a`, found a value with the YAML tag `!t`".to_string(),
        ] {
            assert_eq!(tag_refusal_field(&message), None, "{message:?}");
        }
        assert!(oracle_reads_a_tag(
            "untagged and internally tagged enums do not support enum input"
        ));
        assert!(oracle_reads_a_tag("invalid type: enum, expected a string"));
        assert!(!oracle_reads_a_tag(
            "invalid type: integer `1`, expected a string"
        ));
        assert!(!oracle_reads_a_tag(
            "unknown variant `t`, expected `a` or `b`"
        ));
    }

    #[test]
    fn a_local_tag_where_an_enum_goes_is_the_oracles_unknown_variant() {
        let ayaml = "expected one of `error`, `warning`, found a value with the YAML tag `!t`";
        assert!(tag_as_variant(
            ayaml,
            "unknown variant `t`, expected one of `error`, `warning`"
        ));
        assert!(tag_as_variant(
            "expected `a` or `b`, found a value with the YAML tag `!x-y`",
            "unknown variant `x-y`, expected `a` or `b`"
        ));
        // Another variant, other variants expected, or a tag that is not a
        // local one's name are not the same error.
        assert!(!tag_as_variant(
            ayaml,
            "unknown variant `u`, expected one of `error`, `warning`"
        ));
        assert!(!tag_as_variant(
            ayaml,
            "unknown variant `t`, expected one of `error`"
        ));
        assert!(!tag_as_variant(
            "expected `a`, found a value with the YAML tag `!!str`",
            "unknown variant `!str`, expected `a`"
        ));
        assert!(!tag_as_variant(
            "expected `a`, found a mapping",
            "unknown variant `t`, expected `a`"
        ));
    }

    /// The cases the long runs of seeds 2, 3, 10 and 12 found once alint
    /// named a tag in its own words: a tag on a value where an untagged enum
    /// goes, on an element of one, and on a level; and those of seeds 5 and
    /// 9 without specials, a tag in the value of a merge key, which ayaml
    /// does not merge into the rule, and whose path starts with the quoted
    /// key `["<<"]`.
    #[test]
    fn a_tagged_value_in_a_rule_is_one_error() {
        for (rule, field) in [
            (
                "{<<: {pattern: [a, !t b]}, id: a, kind: file_exists, paths: a, level: error}",
                "`[\"<<\"].pattern[1]`",
            ),
            (
                "{<<: {!t k: v}, id: a, kind: file_exists, paths: a, level: error}",
                "`[\"<<\"]`",
            ),
            (
                "{id: a, kind: command, command: !t [x], paths: a, level: error}",
                "command",
            ),
            (
                "{id: a, kind: file_exists, paths: [a, !t b], level: error}",
                "paths[1]",
            ),
            (
                "{id: a, kind: file_exists, paths: a, level: !t error}",
                "level",
            ),
        ] {
            let input = format!("version: 1\nrules:\n  - {rule}\n");
            let (case, _) = super::super::compare_at(&input, super::super::Entry::Rule, 0);
            assert_eq!(
                case.class,
                super::super::Class::RejectedAtSameLocation,
                "{case:?}"
            );
            assert!(
                case.ayaml.as_ref().unwrap_err().message.contains(field),
                "{case:?}"
            );
        }
    }
}
