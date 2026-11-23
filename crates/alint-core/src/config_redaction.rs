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
pub fn mask_configuration_values(value: &mut crate::yaml::Value, numbers: NumberMask) {
    mask_values(value.parser_mut(), numbers);
}

fn mask_values(value: &mut ayaml::Value, numbers: NumberMask) {
    match value {
        ayaml::Value::String(text) => {
            let [mask, other] = if text.starts_with('$') {
                [JSONPATH_MASK, "$['<masked>']"]
            } else {
                [CONFIGURATION_MASK, "<masked>"]
            };
            *text = if text == mask { other } else { mask }.to_string();
        }
        ayaml::Value::Number(number) => *number = differing_number(number, numbers),
        ayaml::Value::Bool(flag) => *flag = !*flag,
        ayaml::Value::Sequence(items) => {
            for item in items {
                mask_values(item, numbers);
            }
        }
        ayaml::Value::Mapping(mapping) => {
            // A nested rule (a mapping with a `kind`) keeps its identity.
            let rule = mapping.get("kind").is_some_and(ayaml::Value::is_string);
            for (key, item) in mapping.iter_mut() {
                match key.as_str() {
                    Some("id" | "kind" | "level") if rule => {}
                    Some(field) if rule => mask_field(field, item, numbers),
                    _ => mask_values(item, numbers),
                }
            }
        }
        ayaml::Value::Tagged(tagged) => mask_values(&mut tagged.value, numbers),
        ayaml::Value::Null => {}
    }
}

/// [`mask_configuration_values`] for the value of a rule's field `field`. A
/// rule parses its `when:` and `when_iter:` before the rules they guard, so a
/// string condition masks to one that parses too (`false`, or `true` where it
/// was `false`).
#[doc(hidden)]
pub fn mask_configuration_field(field: &str, value: &mut crate::yaml::Value, numbers: NumberMask) {
    mask_field(field, value.parser_mut(), numbers);
}

fn mask_field(field: &str, value: &mut ayaml::Value, numbers: NumberMask) {
    match value {
        ayaml::Value::String(text) if matches!(field, "when" | "when_iter") => {
            *text = if text == "false" { "true" } else { "false" }.to_string();
        }
        _ => mask_values(value, numbers),
    }
}

/// A number of the same kind (float or integer) that differs from `number`,
/// next to it in the direction `numbers` names, or in the other one where that
/// cannot move (the type's maximum or minimum, an infinity). A float moves by
/// the smallest step, which still prints differently: Rust prints the shortest
/// text that reads back as the same float. A NaN becomes zero.
fn differing_number(number: &ayaml::Number, numbers: NumberMask) -> ayaml::Number {
    let up = numbers == NumberMask::Up;
    if let Some(value) = number.as_u64() {
        let moved = if (up && value < u64::MAX) || value == 0 {
            value + 1
        } else {
            value - 1
        };
        ayaml::Number::from(moved)
    } else if let Some(value) = number.as_i64() {
        // Only a negative integer is not a `u64`.
        let moved = if up || value == i64::MIN {
            value + 1
        } else {
            value - 1
        };
        ayaml::Number::from(moved)
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
        ayaml::Number::from(moved)
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

/// A compiled multi-pattern matcher for [`EnvironmentRedactor`].
#[derive(Debug, Clone)]
struct CompiledRedactor {
    automaton: Option<aho_corasick::AhoCorasick>,
    /// Per-pattern `(starts_word, ends_word)` flags for word-widening.
    word_edges: Vec<(bool, bool)>,
}

impl CompiledRedactor {
    fn build(values: &[String]) -> Self {
        let forms = expand_environment_forms(values);
        if forms.is_empty() {
            return Self {
                automaton: None,
                word_edges: Vec::new(),
            };
        }
        let word_edges = forms
            .iter()
            .map(|form| {
                (
                    form.chars().next().is_some_and(is_word_char),
                    form.chars().next_back().is_some_and(is_word_char),
                )
            })
            .collect();
        let automaton = aho_corasick::AhoCorasickBuilder::new()
            .match_kind(aho_corasick::MatchKind::LeftmostLongest)
            .build(&forms)
            .ok();
        Self {
            automaton,
            word_edges,
        }
    }

    fn redact<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        let Some(ac) = &self.automaton else {
            return std::borrow::Cow::Borrowed(text);
        };
        let mut replaced = String::new();
        let mut copied = 0usize;
        let mut pending: Option<(usize, usize)> = None;
        for mat in ac.find_iter(text) {
            let found = mat.start();
            let mat_end = mat.end();
            let (starts_word, ends_word) = self.word_edges[mat.pattern().as_usize()];
            let limit = pending.map_or(copied, |(_, pend_end)| pend_end);
            let mut start = found;
            if starts_word && start > limit {
                start -= text[limit..found]
                    .chars()
                    .rev()
                    .take_while(|&c| is_word_char(c))
                    .map(char::len_utf8)
                    .sum::<usize>();
            }
            let mut end = mat_end;
            if ends_word && end > limit {
                end += text[end..]
                    .chars()
                    .take_while(|&c| is_word_char(c))
                    .map(char::len_utf8)
                    .sum::<usize>();
            }
            match pending {
                Some((pend_start, pend_end)) if start < pend_end => {
                    pending = Some((pend_start.min(start), pend_end.max(end)));
                }
                Some((pend_start, pend_end)) => {
                    replaced.push_str(&text[copied..pend_start]);
                    replaced.push_str(CONFIGURATION_MASK);
                    copied = pend_end;
                    pending = Some((start, end));
                }
                None => {
                    pending = Some((start, end));
                }
            }
        }
        if let Some((pend_start, pend_end)) = pending {
            replaced.push_str(&text[copied..pend_start]);
            replaced.push_str(CONFIGURATION_MASK);
            replaced.push_str(&text[pend_end..]);
            std::borrow::Cow::Owned(replaced)
        } else {
            std::borrow::Cow::Borrowed(text)
        }
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Expand `values` into every printable form an error may quote, keeping only
/// forms at least [`MIN_REDACTED_ENVIRONMENT_VALUE`] chars long.
fn expand_environment_forms(values: &[String]) -> Vec<String> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut forms: Vec<String> = Vec::new();
    let mut push = |form: String| {
        if form.chars().count() >= MIN_REDACTED_ENVIRONMENT_VALUE && seen.insert(form.clone()) {
            forms.push(form);
        }
    };
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
                    push(debug[1..debug.len() - 1].to_string());
                    push(printed);
                }
            }
        }
        if value.len() <= 64
            && let Ok(ayaml::Value::Number(number)) = crate::yaml::from_str(value)
        {
            push(number.to_string());
        }
    }
    forms
}

/// Set-backed, single-automaton scrubber for environment values. Recording new
/// values is O(1) per value via a `HashSet`, and scrubbing compiles one
/// Aho-Corasick automaton once so each message is scanned in linear time.
#[doc(hidden)]
#[derive(Debug, Default, Clone)]
pub struct EnvironmentRedactor {
    values: Vec<String>,
    held: std::collections::HashSet<String>,
    compiled: Option<CompiledRedactor>,
}

impl EnvironmentRedactor {
    /// An empty redactor holding no values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a compiled redactor from `values`, deduplicating via a set.
    #[must_use]
    pub fn from_values(values: &[String]) -> Self {
        let mut redactor = Self::new();
        redactor.extend(values);
        redactor.compiled = Some(CompiledRedactor::build(&redactor.values));
        redactor
    }

    /// Record `value` if not already held, invalidating the cached automaton
    /// only when `value` is new.
    pub fn insert(&mut self, value: &str) {
        if !self.held.contains(value) {
            self.held.insert(value.to_owned());
            self.values.push(value.to_owned());
            self.compiled = None;
        }
    }

    /// Record each of `values` that is not already held.
    pub fn extend(&mut self, values: &[String]) {
        for value in values {
            self.insert(value);
        }
    }

    /// Every recorded value in insertion order.
    #[must_use]
    pub fn values(&self) -> &[String] {
        &self.values
    }

    /// `text` with every recorded environment value replaced by
    /// [`CONFIGURATION_MASK`], compiling the automaton on first use if needed.
    #[must_use]
    pub fn redact_lazy<'a>(&mut self, text: &'a str) -> std::borrow::Cow<'a, str> {
        if self.values.is_empty() {
            return std::borrow::Cow::Borrowed(text);
        }
        let compiled = self
            .compiled
            .get_or_insert_with(|| CompiledRedactor::build(&self.values));
        compiled.redact(text)
    }

    /// `text` with every recorded environment value replaced by
    /// [`CONFIGURATION_MASK`]. Requires a redactor built with
    /// [`Self::from_values`] (or after [`Self::redact_lazy`]); if not yet
    /// compiled, falls back to building a temporary automaton.
    #[must_use]
    pub fn redact<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        if self.values.is_empty() {
            return std::borrow::Cow::Borrowed(text);
        }
        if let Some(compiled) = &self.compiled {
            compiled.redact(text)
        } else {
            CompiledRedactor::build(&self.values).redact(text)
        }
    }

    /// Like [`redact_environment_values_in_error`], reusing this redactor's
    /// compiled automaton across the error's message and structured fields.
    #[must_use]
    pub fn redact_error(&self, error: crate::Error) -> crate::Error {
        if self.values.is_empty() || matches!(error, crate::Error::FileNotInIndex { .. }) {
            return error;
        }
        let message = error.to_string();
        let std::borrow::Cow::Owned(redacted) = self.redact(&message) else {
            return error;
        };
        match error {
            crate::Error::Internal(inner) => crate::Error::internal(self.redact(&inner)),
            crate::Error::RuleConfig { rule_id, message } => {
                crate::Error::rule_config(self.redact(&rule_id), self.redact(&message))
            }
            _ => crate::Error::Other(redacted),
        }
    }
}

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
    if values.is_empty() {
        return std::borrow::Cow::Borrowed(text);
    }
    CompiledRedactor::build(values).redact(text)
}

/// `error` with each of `values` removed from its message
/// ([`redact_environment_values`]). An error that quotes none is returned as it
/// is, and so is `FileNotInIndex`, which names only the path its caller asked
/// about; one that does keeps its kind where the kind carries the message
/// (`Internal`, `RuleConfig`) and becomes `Other` otherwise.
#[doc(hidden)]
#[must_use]
pub fn redact_environment_values_in_error(error: crate::Error, values: &[String]) -> crate::Error {
    if values.is_empty() || matches!(error, crate::Error::FileNotInIndex { .. }) {
        return error;
    }
    EnvironmentRedactor::from_values(values).redact_error(error)
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
            mask_configuration_field(field, value, numbers);
        }
        Some(masked)
    }
}
