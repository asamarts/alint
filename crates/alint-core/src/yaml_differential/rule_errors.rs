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
            Some(("message", "invalid type: integer `1`, expected a string"))
        );
        assert_eq!(
            ayaml_path("scope_filter.has_ancestor[1]: x"),
            Some(("scope_filter", "x"))
        );
        assert_eq!(ayaml_path("a[\"b \\\"c]: d\"][?].e-f: g"), Some(("a", "g")));
        assert_eq!(ayaml_path("a[\"b\"]: x"), Some(("a", "x")));
        // A name starts with a letter or `_`.
        assert_eq!(ayaml_path("_a: x"), Some(("_a", "x")));
        for message in [
            "1a: x",
            "-a: x",
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
}
