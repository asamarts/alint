//! Whether two errors for one rule are one. Both parsers' errors at
//! [`Entry::Rule`](super::Entry::Rule) are placed at the rule's position
//! only, so the place says nothing of which field failed or why: ayaml's
//! `message: invalid type ..., expected a string` and the oracle's `invalid
//! type: integer `1`, expected field identifier` are both at the rule's
//! line. They are one when they agree on the problem and on the field.

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
    let (field, ayaml_problem) = ayaml_path(&ayaml.message)
        .map_or((None, ayaml.message.as_str()), |(field, problem)| {
            (Some(field), problem)
        });
    problem(ayaml_problem) == problem(&oracle.message)
        && field.is_none_or(|field| oracle_error_is_in(case.input, case.rule, field, oracle))
}

/// ayaml's `message` split into the first step of the path in front of it,
/// when that step is a name, as a rule's fields are, and the problem after
/// the path. A path's steps are names, quoted keys (`["a b"]`), indexes
/// (`[0]`) and keys that are collections (`[?]`), with a `.` before a name,
/// and it ends at `: `; a problem starts with words and spaces, as
/// `invalid type: `, so none reads as a path.
fn ayaml_path(message: &str) -> Option<(&str, &str)> {
    let name = |text: &str| {
        let mut chars = text.char_indices();
        match chars.next() {
            Some((_, first)) if first.is_ascii_alphabetic() || first == '_' => chars
                .find(|(_, next)| !(next.is_ascii_alphanumeric() || matches!(next, '_' | '-')))
                .map_or(text.len(), |(at, _)| at),
            _ => 0,
        }
    };
    let first = name(message);
    if first == 0 {
        return None;
    }
    let mut at = first;
    loop {
        let rest = &message[at..];
        if let Some(problem) = rest.strip_prefix(": ") {
            return Some((&message[..first], problem));
        }
        at += if let Some(after) = rest.strip_prefix('.') {
            match name(after) {
                0 => return None,
                len => 1 + len,
            }
        } else if let Some(after) = rest.strip_prefix("[\"") {
            let mut escaped = false;
            let (close, _) = after.char_indices().find(|&(_, next)| {
                let close = !escaped && next == '"';
                escaped = !escaped && next == '\\';
                close
            })?;
            if !after[close + 1..].starts_with(']') {
                return None;
            }
            2 + close + 2
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

/// ayaml's messages for an enum's value that is neither a variant's name nor
/// a tagged value, by the value's shape: the oracle words them all as
/// "invalid type: ..., expected a `Value::Tagged` enum".
const AYAML_ENUM_SHAPES: &[&str] = &[
    "enum value must be tagged or a single-entry mapping",
    "enum mapping must contain one variant",
    "enum variant key must be a string",
    "expected a unit enum variant",
];

/// A rule error's problem, with what an "invalid type" or "invalid value"
/// found left out: ayaml names it by its kind, as `non-string YAML value`,
/// and the oracle by its value, as ``integer `1` ``, so the two agree on
/// what was expected only. An enum's value that is not a variant's name is
/// one problem, however each words it (see [`AYAML_ENUM_SHAPES`]).
fn problem(text: &str) -> String {
    if AYAML_ENUM_SHAPES.contains(&text)
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
/// field and the fields the rule cannot do without, and not for the rule
/// cut down to those others alone. A field the rule cannot do without is
/// one without which the oracle says it is missing; serde reads the fields
/// in their order in the struct, so a field the rule needs ahead of the
/// failing one is among them.
///
/// It cannot tell a field the rule needs, which the oracle reads, from the
/// failing field after it: when ayaml refuses `id` and the oracle reads it
/// and refuses `kind` with the same problem, it takes the two for one.
/// That takes a difference in how the two decode `id`, which another rule
/// shows.
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
    let needed: Vec<&str> = mapping
        .keys()
        .filter_map(yaml_serde::Value::as_str)
        .filter(|key| {
            *key != field
                && error(&|other| other != *key).as_deref()
                    == Some(format!("missing field `{key}`").as_str())
        })
        .collect();
    let with = error(&|key| key == field || needed.contains(&key));
    let without = error(&|key| needed.contains(&key));
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
            ayaml_path("message: invalid type: non-string YAML value, expected a string"),
            Some((
                "message",
                "invalid type: non-string YAML value, expected a string"
            ))
        );
        assert_eq!(
            ayaml_path("scope_filter.has_ancestor[1]: x"),
            Some(("scope_filter", "x"))
        );
        assert_eq!(ayaml_path("a[\"b \\\"c]: d\"][?].e-f: g"), Some(("a", "g")));
        for message in [
            "invalid type: integer `1`, expected struct RuleSpec",
            "missing field `id`",
            "a glob string, a list of globs, or an `{ include, exclude }` map",
            "[\"1\"]: x",
            "a.: x",
            "a[x]: y",
            "a[]: y",
            "a[\"b\"x: y",
            "a",
        ] {
            assert_eq!(ayaml_path(message), None, "{message:?}");
        }
    }

    #[test]
    fn an_invalid_type_or_value_is_compared_on_what_was_expected() {
        assert_eq!(
            problem("invalid type: non-string YAML value, expected a string"),
            problem("invalid type: integer `1`, expected a string")
        );
        assert_eq!(
            problem("invalid value: string \"a, expected b\", expected a boolean"),
            "invalid value: ..., expected a boolean"
        );
        assert_ne!(
            problem("invalid type: non-string YAML value, expected a string"),
            problem("invalid type: integer `1`, expected field identifier")
        );
        assert_eq!(problem("missing field `id`"), "missing field `id`");
        for ayaml in AYAML_ENUM_SHAPES {
            assert_eq!(
                problem(ayaml),
                problem("invalid type: unit value, expected a Value::Tagged enum")
            );
        }
        assert_ne!(
            problem("invalid type: unit value, expected a Value::Tagged enum"),
            problem("invalid type: unit value, expected a string")
        );
    }
}
