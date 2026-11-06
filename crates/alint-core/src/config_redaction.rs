//! Keeping secrets out of configuration errors. A rule field read from the
//! environment, or any value of a `sensitive: true` rule, may be secret; an
//! error that could quote one is shown only when a rerun on a masked copy
//! fails the same way. Values the environment supplied to the top-level
//! fields are removed from an error's text instead.

use super::{Config, PathsSpec, RuleSpec};

/// The placeholder that replaces a possibly secret configuration value.
#[doc(hidden)]
pub const CONFIGURATION_MASK: &str = "<redacted>";

/// The mask for a `JSONPath` query (a string starting with `$`), which a rule
/// parses before it reports a later error.
const JSONPATH_MASK: &str = "$['<redacted>']";

/// The message of a configuration error that could quote a secret value.
#[doc(hidden)]
pub const REDACTED_CONFIGURATION_ERROR: &str = "<redacted> (the error could quote a value read \
     from the environment or set in a `sensitive: true` rule)";

/// Whether a rule field's value may be secret in a configuration error: it was
/// read from the environment (`environment_fields`), or the rule sets
/// `sensitive: true` and the field is not part of the rule's identity (its
/// `id`, `kind`, `level`, `sensitive` and the template it instantiates).
#[doc(hidden)]
#[must_use]
pub fn configuration_field_may_be_secret(
    sensitive: bool,
    environment_fields: &std::collections::BTreeSet<String>,
    field: &str,
) -> bool {
    environment_fields.contains(field)
        || (sensitive
            && !matches!(
                field,
                "id" | "kind" | "level" | "sensitive" | "extends_template"
            ))
}

/// Which way [`mask_configuration_values`] moves a number. A masked copy stays
/// next to each number, so a range check (`max_width > 0`, a `threshold` in
/// `0.0..=1.0`) passes as it did and an unrelated error later in the rule still
/// recurs and stays readable. A value at the edge of its range fails the check
/// in one direction, so a rerun tries both ([`Self::BOTH`]).
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberMask {
    /// The next number up: the integer one higher, or the next float.
    Up,
    /// The next number down: the integer one lower, or the previous float.
    Down,
}

impl NumberMask {
    /// Both directions, in the order a rerun tries them.
    pub const BOTH: [Self; 2] = [Self::Up, Self::Down];
}

/// Replace every scalar in `value`, at any depth, with one that differs from
/// it, so an error quoting the original cannot recur on the masked copy: a
/// string becomes [`CONFIGURATION_MASK`] (one starting with `$`, a `JSONPath`,
/// becomes a `JSONPath` naming it, so the query still parses), a number the next
/// one of the same kind (in the direction `numbers` names), and a bool its
/// negation.
/// Interpolation re-types an environment value that reads as a number or a
/// bool, so those can be secret too (a 12-digit account id). Numbers and bools
/// keep their type, so an error about a type that does not quote the value
/// still recurs and stays readable. Mapping keys are names, not values, and the
/// `id`, `kind` and `level` of a nested rule (a mapping with a string `kind`)
/// are never interpolated (they are its identity), so both stay as written.
#[doc(hidden)]
pub fn mask_configuration_values(value: &mut yaml_serde::Value, numbers: NumberMask) {
    match value {
        yaml_serde::Value::String(text) => {
            let [mask, other] = if text.starts_with('$') {
                [JSONPATH_MASK, "$['<masked>']"]
            } else {
                [CONFIGURATION_MASK, "<masked>"]
            };
            *text = if text == mask { other } else { mask }.to_string();
        }
        yaml_serde::Value::Number(number) => *number = differing_number(number, numbers),
        yaml_serde::Value::Bool(flag) => *flag = !*flag,
        yaml_serde::Value::Sequence(items) => items
            .iter_mut()
            .for_each(|item| mask_configuration_values(item, numbers)),
        yaml_serde::Value::Mapping(mapping) => {
            // A nested rule (a mapping with a `kind`) keeps its identity.
            let rule = mapping
                .get("kind")
                .is_some_and(yaml_serde::Value::is_string);
            mapping
                .iter_mut()
                .filter(|(key, _)| !(rule && matches!(key.as_str(), Some("id" | "kind" | "level"))))
                .for_each(|(_, item)| mask_configuration_values(item, numbers));
        }
        yaml_serde::Value::Tagged(tagged) => mask_configuration_values(&mut tagged.value, numbers),
        yaml_serde::Value::Null => {}
    }
}

/// A number of the same kind (float or integer) that differs from `number`,
/// next to it in the direction `numbers` names, or in the other one where that
/// cannot move (the type's maximum or minimum, an infinity). A float moves by
/// the smallest step, which still prints differently: Rust prints the shortest
/// text that reads back as the same float. A NaN becomes zero.
fn differing_number(number: &yaml_serde::Number, numbers: NumberMask) -> yaml_serde::Number {
    let up = numbers == NumberMask::Up;
    if let Some(value) = number.as_u64() {
        let moved = if (up && value < u64::MAX) || value == 0 {
            value + 1
        } else {
            value - 1
        };
        yaml_serde::Number::from(moved)
    } else if let Some(value) = number.as_i64() {
        // Only a negative integer is not a `u64`.
        let moved = if up || value == i64::MIN {
            value + 1
        } else {
            value - 1
        };
        yaml_serde::Number::from(moved)
    } else {
        let value = number.as_f64().unwrap_or_default();
        let (next, other) = if up {
            (value.next_up(), value.next_down())
        } else {
            (value.next_down(), value.next_up())
        };
        let moved = if value.is_nan() {
            0.0
        } else if next.to_bits() == value.to_bits() {
            other
        } else {
            next
        };
        yaml_serde::Number::from(moved)
    }
}

impl Config {
    /// `text` with [`Self::environment_values`] removed
    /// ([`redact_environment_values`]).
    #[doc(hidden)]
    #[must_use]
    pub fn redact_environment_values<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        redact_environment_values(text, &self.environment_values)
    }
}

/// The shortest environment value [`redact_environment_values`] removes. A
/// shorter one is too common a substring to remove without garbling the rest
/// of the message, and too short to be much of a secret.
const MIN_REDACTED_ENVIRONMENT_VALUE: usize = 4;

/// `text` with each of `values` replaced by [`CONFIGURATION_MASK`], in each
/// form an error may print it: as written; trimmed (a glob parser drops
/// trailing whitespace); line by line (a regex error quotes a multi-line
/// pattern so); as a path is printed once joined or on Windows (without a
/// leading `./`, with either separator); escaped by `{:?}`; or (for a numeric
/// value, which interpolation re-types) as the number. A form inside a longer
/// word takes the whole word with it: a value written next to other text
/// (`x{{env.V}}_tmp`) is printed so, and a value such as `build` turns
/// `building` into `<redacted>` rather than `<redacted>ing`. A boolean carries
/// no secret and is left alone, and so is a form shorter than four characters.
#[doc(hidden)]
#[must_use]
pub fn redact_environment_values<'a>(
    text: &'a str,
    values: &[String],
) -> std::borrow::Cow<'a, str> {
    let mut forms: Vec<String> = Vec::new();
    for value in values {
        if matches!(value.trim(), "true" | "false") {
            continue;
        }
        let mut written_forms = vec![value.as_str(), value.trim(), value.trim_end()];
        if value.contains('\n') {
            written_forms.extend(value.lines().map(str::trim));
        }
        for written in written_forms {
            let mut bare = written;
            while let Some(rest) = bare.strip_prefix("./").or_else(|| bare.strip_prefix(".\\")) {
                bare = rest;
            }
            for path in [written, bare] {
                for printed in [
                    path.to_string(),
                    path.replace('/', "\\"),
                    path.replace('\\', "/"),
                ] {
                    let debug = format!("{printed:?}");
                    forms.push(debug[1..debug.len() - 1].to_string());
                    forms.push(printed);
                }
            }
        }
        if value.len() <= 64
            && let Ok(yaml_serde::Value::Number(number)) = yaml_serde::from_str(value)
        {
            forms.push(number.to_string());
        }
    }
    forms.retain(|form| form.chars().count() >= MIN_REDACTED_ENVIRONMENT_VALUE);
    // Longest first, so a value that contains another is removed whole.
    forms.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    forms.dedup();
    let mut redacted = std::borrow::Cow::Borrowed(text);
    for form in &forms {
        if let Some(replaced) = replace_occurrences(&redacted, form) {
            redacted = std::borrow::Cow::Owned(replaced);
        }
    }
    redacted
}

/// `text` with each occurrence of `form` replaced by [`CONFIGURATION_MASK`],
/// or `None` when there is none. An occurrence that a letter, digit or `_`
/// touches at an edge where `form` has one is widened to the whole word.
fn replace_occurrences(text: &str, form: &str) -> Option<String> {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let starts_word = form.chars().next().is_some_and(word);
    let ends_word = form.chars().next_back().is_some_and(word);
    let mut replaced = String::new();
    let mut copied = 0;
    for (found, _) in text.match_indices(form) {
        if found < copied {
            continue;
        }
        let mut start = found;
        if starts_word {
            start -= text[copied..found]
                .chars()
                .rev()
                .take_while(|&c| word(c))
                .map(char::len_utf8)
                .sum::<usize>();
        }
        let mut end = found + form.len();
        if ends_word {
            end += text[end..]
                .chars()
                .take_while(|&c| word(c))
                .map(char::len_utf8)
                .sum::<usize>();
        }
        replaced.push_str(&text[copied..start]);
        replaced.push_str(CONFIGURATION_MASK);
        copied = end;
    }
    (copied > 0).then(|| replaced + &text[copied..])
}

/// `error` with each of `values` removed from its message
/// ([`redact_environment_values`]). An error that quotes none is returned as it
/// is, and so is `FileNotInIndex`, which names only the path its caller asked
/// about; one that does keeps its kind where the kind carries the message
/// (`Internal`, `RuleConfig`) and becomes `Other` otherwise.
#[doc(hidden)]
#[must_use]
pub fn redact_environment_values_in_error(error: crate::Error, values: &[String]) -> crate::Error {
    if matches!(error, crate::Error::FileNotInIndex { .. }) {
        return error;
    }
    let message = error.to_string();
    let std::borrow::Cow::Owned(redacted) = redact_environment_values(&message, values) else {
        return error;
    };
    match error {
        crate::Error::Internal(inner) => {
            crate::Error::internal(redact_environment_values(&inner, values))
        }
        crate::Error::RuleConfig { rule_id, message } => crate::Error::rule_config(
            redact_environment_values(&rule_id, values),
            redact_environment_values(&message, values),
        ),
        _ => crate::Error::Other(redacted),
    }
}

impl RuleSpec {
    /// Fail closed when an error may quote environment-derived or explicitly
    /// sensitive configuration. `rerun` repeats the failed step on
    /// [`Self::masked_configuration`], with numbers moved up and then down
    /// ([`NumberMask::BOTH`]); the error stays readable only when a copy fails
    /// with the identical message, which therefore does not quote a masked
    /// value. Structural rule identity remains visible either way.
    #[doc(hidden)]
    pub fn redact_unless_reproduced(
        &self,
        error: crate::Error,
        mut rerun: impl FnMut(&Self) -> crate::Result<()>,
    ) -> crate::Error {
        if !self.has_sensitive_configuration() {
            return error;
        }
        let message = error.to_string();
        let reproduced = NumberMask::BOTH.into_iter().any(|numbers| {
            self.masked_configuration(numbers)
                .and_then(|masked| rerun(&masked).err())
                .is_some_and(|masked_error| masked_error.to_string() == message)
        });
        if reproduced {
            return error;
        }
        if error.is_internal() {
            crate::Error::internal(REDACTED_CONFIGURATION_ERROR)
        } else {
            crate::Error::rule_config(&self.id, REDACTED_CONFIGURATION_ERROR)
        }
    }

    /// Remove potentially sensitive detail from a `when` parser diagnostic: it
    /// quotes the condition, so it is hidden when the condition may be secret.
    #[doc(hidden)]
    pub fn redact_when_error(&self, error: impl std::fmt::Display) -> String {
        if configuration_field_may_be_secret(self.sensitive, &self.sensitive_fields, "when") {
            REDACTED_CONFIGURATION_ERROR.to_string()
        } else {
            error.to_string()
        }
    }

    /// alint-core 0.21.0's form of [`Self::redact_unless_reproduced`], kept
    /// so the 0.21.0 `alint`, `alint-lsp` and `alint-testkit` crates still
    /// build against this release: it redacts every error of a rule with
    /// possibly secret configuration, without a rerun.
    #[doc(hidden)]
    #[deprecated(since = "0.21.1", note = "use `redact_unless_reproduced`")]
    pub fn redact_configuration_error(&self, error: crate::Error) -> crate::Error {
        if !self.has_sensitive_configuration() {
            return error;
        }
        if error.is_internal() {
            crate::Error::internal(REDACTED_CONFIGURATION_ERROR)
        } else {
            crate::Error::rule_config(&self.id, REDACTED_CONFIGURATION_ERROR)
        }
    }

    /// alint-core 0.21.0's message redaction, kept for the same crates as
    /// [`Self::redact_configuration_error`]: the whole message is hidden when
    /// any field may be secret.
    #[doc(hidden)]
    #[deprecated(since = "0.21.1", note = "use `redact_when_error`")]
    pub fn redact_configuration_message(&self, error: impl std::fmt::Display) -> String {
        if self.has_sensitive_configuration() {
            REDACTED_CONFIGURATION_ERROR.to_string()
        } else {
            error.to_string()
        }
    }

    /// A copy whose possibly secret values (see
    /// [`configuration_field_may_be_secret`]) are masked
    /// ([`mask_configuration_values`], numbers moved as `numbers` says), so a
    /// step repeated on it cannot quote them. `None` when such a value sits in
    /// a typed field this cannot rewrite (`fix`, or a manifest-path
    /// `scope_filter`); the caller then fails closed.
    #[doc(hidden)]
    #[must_use]
    pub fn masked_configuration(&self, numbers: NumberMask) -> Option<Self> {
        let secret = |field: &str| {
            configuration_field_may_be_secret(self.sensitive, &self.sensitive_fields, field)
        };
        let mask = || CONFIGURATION_MASK.to_string();
        let mut masked = self.clone();
        if secret("paths") {
            masked.paths = masked.paths.map(|paths| match paths {
                PathsSpec::Single(_) => PathsSpec::Single(mask()),
                PathsSpec::Many(patterns) => {
                    PathsSpec::Many(patterns.iter().map(|_| mask()).collect())
                }
                PathsSpec::IncludeExclude { include, exclude } => PathsSpec::IncludeExclude {
                    include: include.iter().map(|_| mask()).collect(),
                    exclude: exclude.iter().map(|_| mask()).collect(),
                },
            });
        }
        for (field, value) in [
            ("message", &mut masked.message),
            ("policy_url", &mut masked.policy_url),
            ("when", &mut masked.when),
        ] {
            if secret(field) && value.is_some() {
                *value = Some(mask());
            }
        }
        if secret("fix") && masked.fix.is_some() {
            return None;
        }
        if secret("scope_filter")
            && let Some(scope) = &mut masked.scope_filter
        {
            if scope.include_manifest_paths.is_some() || scope.exclude_manifest_paths.is_some() {
                return None;
            }
            if let Some(globs) = &mut scope.has_ancestor {
                for glob in globs {
                    *glob = mask();
                }
            }
            if scope.changed_since.is_some() {
                scope.changed_since = Some(mask());
            }
        }
        for (field, value) in &mut masked.extra {
            let Some(field) = field.as_str().filter(|field| secret(field)) else {
                continue;
            };
            // A nested rule parses `when_iter:` before the rules it guards, so
            // its mask is an expression that parses too.
            if field == "when_iter"
                && let yaml_serde::Value::String(text) = value
            {
                *text = if text == "false" { "true" } else { "false" }.to_string();
                continue;
            }
            mask_configuration_values(value, numbers);
        }
        Some(masked)
    }
}
