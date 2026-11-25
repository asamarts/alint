//! Load-time `{{env.X}}` variable interpolation across the DSL.
//!
//! Runs after the YAML parse but before the typed [`alint_core::Config`]
//! is built, so every string-typed config value can reference an
//! environment variable via `{{env.NAME}}`, with an optional
//! `{{env.NAME | default('fallback')}}`.
//!
//! The `vars.` and `ctx.` namespaces are deliberately left untouched
//! here: `{{vars.X}}` is resolved by the template-expansion pass in
//! `RawConfig::finalize`, and `{{ctx.X}}` is an evaluate-time
//! per-violation substitution done in the renderer. Keeping the three
//! layers distinct is a cross-cutting decision in the design.
//!
//! The env lookup is injected as `Fn(&str) -> Option<String>` (the
//! production call site passes `|n| std::env::var(n).ok()`; tests pass
//! a fake map). This keeps the crate `forbid(unsafe_code)`-compatible —
//! Rust 2024 marks `std::env::set_var` unsafe, so tests never touch the
//! real environment.
//!
//! See `docs/design/v0.11/variable_interpolation.md`.

use ayaml::Value;

/// Rule-entry keys whose values are never interpolated. Rule identity,
/// type, and severity must be stable across environments — env-driven
/// values there break audit trails and run reproducibility (see the
/// design doc's field table). Only skipped inside rule / template
/// subtrees (see [`walk`]); a `vars:` or `facts:` entry that happens
/// to be named `level` still interpolates.
const SKIP_KEYS: &[&str] = &["id", "kind", "level"];

/// Top-level keys whose subtrees are rule definitions (each element is
/// a rule spec, possibly with nested rules under `require:`). Entering
/// one turns on [`SKIP_KEYS`] suppression for the whole subtree.
const RULE_BEARING_KEYS: &[&str] = &["rules", "templates"];

/// Where in a config a value sits, which decides the keys that stay literal
/// and whether a value that took a substitution is re-typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    /// A whole config document: each top-level key decides the scope of its
    /// value.
    Document,
    /// Outside a rule (`vars:`, `facts:`, `extends:`, ...): a value that took
    /// a substitution and reads as a number or a bool is re-typed.
    Plain,
    /// Inside a rule or template entry (a [`RULE_BEARING_KEYS`] key, at any
    /// depth): [`SKIP_KEYS`] stay literal, and a value is re-typed as in
    /// [`Scope::Plain`]. Nested rules (a `for_each_dir`'s `require:`) inherit
    /// it.
    Rule,
    /// Inside version 2's `selectors:` or `relations:`, at this position: a
    /// substituted value stays text (a glob, a `JSONPath`, a pattern, a value
    /// to compare: `in: ["{{env.NODE_MAJOR}}"]` compares with `18`, not the
    /// number 18), but in a field that takes a boolean
    /// ([`crate::v2::Position`]), where one that spells a boolean reads as
    /// one (`optional: "{{env.OPTIONAL}}"`).
    Objects(crate::v2::Position),
}

impl Scope {
    /// The scope of the value under `key` of a mapping in this scope.
    fn below(self, key: Option<&str>) -> Self {
        use crate::v2::Position;
        match self {
            Self::Rule => self,
            Self::Objects(position) => Self::Objects(position.below(key)),
            Self::Document if key == Some("relations") => Self::Objects(Position::Relations),
            Self::Document if key == Some("selectors") => Self::Objects(Position::Text),
            Self::Document | Self::Plain
                if key.is_some_and(|key| RULE_BEARING_KEYS.contains(&key)) =>
            {
                Self::Rule
            }
            Self::Document | Self::Plain => Self::Plain,
        }
    }
}

/// An interpolation failure, carried back to the loader which maps it
/// onto [`alint_core::Error`] with the offending config site prepended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InterpError {
    /// `{{env.NAME}}` where `NAME` is unset (or empty) and no
    /// `| default(...)` filter was given.
    UndefinedEnv { name: String },
    /// `{{foo.X}}` where `foo` is not a known namespace.
    UnknownNamespace { namespace: String },
    /// A `{{...}}` span that did not parse (no `.`, bad filter, …).
    Malformed { span: String, reason: String },
}

impl std::fmt::Display for InterpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UndefinedEnv { name } => write!(
                f,
                "references undefined env var `{name}` and has no default. \
                 Set the env var, or use the default-value filter: \
                 `{{{{env.{name} | default('...')}}}}`"
            ),
            Self::UnknownNamespace { namespace } => {
                write!(f, "unknown namespace `{namespace}`")?;
                if looks_like_typo(namespace, "env") {
                    write!(f, " (typo for `env`?)")?;
                } else if looks_like_typo(namespace, "vars") {
                    write!(f, " (typo for `vars`?)")?;
                }
                write!(f, ". Supported namespaces: env, vars.")
            }
            Self::Malformed { span, reason } => {
                write!(f, "malformed interpolation `{span}`: {reason}")
            }
        }
    }
}

/// Outcome of interpolating one scalar string.
pub(crate) struct Interpolated {
    pub text: String,
    /// `true` if at least one `{{env.X}}` span resolved to a value
    /// (vs. the input being pure literal or carrying only deferred
    /// `{{vars.X}}` / `{{ctx.X}}` spans). The walker uses this to
    /// decide whether to re-type the result (see [`retype_scalar`]).
    pub substituted: bool,
}

/// Interpolate every `{{env.X}}` span in a YAML value tree, in place.
///
/// Recurses into sequences and mappings. Inside rule / template
/// subtrees the `id` / `kind` / `level` keys are left literal;
/// everywhere else (including `vars:` / `facts:`) every string value
/// interpolates. Non-string scalars (numbers, bools, null) are left
/// untouched. Returns the first [`InterpError`] encountered.
pub(crate) fn interpolate_value<F>(value: &mut Value, env: &F) -> Result<(), InterpError>
where
    F: Fn(&str) -> Option<String>,
{
    walk(value, env, Scope::Document, None, None)
}

/// [`interpolate_value`] of `value`, which sits in `scope` of a config, then
/// every scalar that took a substitution replaced by one of the same type
/// that differs from it ([`alint_core::mask_configuration_field`], numbers
/// moved as `numbers` names): the copy a load error is rerun on. An error
/// that recurs on it cannot quote an environment value, and one about an
/// unrelated field still recurs, because each masked value keeps the type
/// interpolation gave it.
pub(crate) fn interpolate_masked<F>(
    value: &mut Value,
    env: &F,
    numbers: alint_core::NumberMask,
    scope: Scope,
) -> Result<(), InterpError>
where
    F: Fn(&str) -> Option<String>,
{
    walk(value, env, scope, Some(numbers), None)
}

/// `scope` is where `value` sits ([`Scope`]). `field` is the key it sits
/// under (`None` for a list's item); inside a rule, it decides how the value
/// is masked ([`alint_core::mask_configuration_field`]).
fn walk<F>(
    value: &mut Value,
    env: &F,
    scope: Scope,
    mask: Option<alint_core::NumberMask>,
    field: Option<&str>,
) -> Result<(), InterpError>
where
    F: Fn(&str) -> Option<String>,
{
    match value {
        Value::String(s) => {
            let result = interpolate_scalar(s, env)?;
            let substituted = result.substituted;
            // Re-type only when the value was fully resolved (no
            // deferred `{{...}}` remains) AND interpolation actually
            // substituted something — so an interpolated integer-field
            // value validates as a number rather than a string, while
            // pre-existing literal strings are never silently re-typed.
            // A selector's or relation's value stays text, but in a field
            // that takes a boolean.
            if !result.substituted || result.text.contains("{{") {
                *s = result.text;
            } else {
                *value = match scope {
                    Scope::Objects(crate::v2::Position::Boolean) => retype_boolean(result.text),
                    Scope::Objects(crate::v2::Position::Integer) => retype_integer(result.text),
                    Scope::Objects(_) => Value::String(result.text),
                    Scope::Document | Scope::Plain | Scope::Rule => retype_scalar(result.text),
                };
            }
            if let (true, Some(numbers)) = (substituted, mask) {
                // A rule's field decides how its value is masked.
                let field = field.filter(|_| scope == Scope::Rule);
                crate::core_yaml::mask_configuration_field(
                    field.unwrap_or_default(),
                    value,
                    numbers,
                );
            }
            Ok(())
        }
        Value::Sequence(seq) => {
            // A document that is a list holds no top-level keys.
            let scope = scope.below(None);
            for v in seq.iter_mut() {
                walk(v, env, scope, mask, None)?;
            }
            Ok(())
        }
        Value::Mapping(map) => {
            for (k, v) in &mut *map {
                let key = k.as_str();
                if scope == Scope::Rule && key.is_some_and(|kk| SKIP_KEYS.contains(&kk)) {
                    continue;
                }
                // A `rules:` / `templates:` key turns the skip on for its
                // subtree; once on, it stays on.
                walk(v, env, scope.below(key), mask, key)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Interpolate a single scalar string. Pure function over the injected
/// env lookup; the walker above is the only production caller.
pub(crate) fn interpolate_scalar<F>(input: &str, env: &F) -> Result<Interpolated, InterpError>
where
    F: Fn(&str) -> Option<String>,
{
    // Fast path: no template marker at all.
    if !input.contains("{{") {
        return Ok(Interpolated {
            text: input.to_owned(),
            substituted: false,
        });
    }

    let mut out = String::with_capacity(input.len());
    let mut substituted = false;
    let mut rest = input;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else {
            // Unclosed `{{` — treat the marker as literal, mirroring
            // v0.9.21's lenient `${` handling. A regex value that
            // happens to contain `{{` must not hard-error.
            out.push_str("{{");
            rest = after;
            continue;
        };
        let inner = after[..close].trim();
        let span = &rest[open..open + 2 + close + 2];
        match resolve_span(inner, env)? {
            SpanResult::Resolved(v) => {
                out.push_str(&v);
                substituted = true;
            }
            SpanResult::Defer => out.push_str(span),
        }
        rest = &after[close + 2..];
    }
    out.push_str(rest);
    Ok(Interpolated {
        text: out,
        substituted,
    })
}

/// Whether interpolating `input` would read the environment: true when it
/// holds at least one `{{env.X}}` span (with or without a `| default(...)`
/// filter, and with any spacing the real resolver accepts). Runs the real
/// span resolver with a probe lookup, so it agrees with
/// [`interpolate_scalar`] on every boundary; a span that does not parse is
/// left to the real interpolation pass to report.
pub(crate) fn reads_env(input: &str) -> bool {
    let read = std::cell::Cell::new(false);
    let probe = |_: &str| {
        read.set(true);
        Some("x".to_owned())
    };
    let _ = interpolate_scalar(input, &probe);
    read.get()
}

/// The names `input` reads from the environment, in order. Runs the real span
/// resolver with a probe lookup, like [`reads_env`], so it agrees with
/// [`interpolate_scalar`] on every boundary.
pub(crate) fn env_names(input: &str) -> Vec<String> {
    let names = std::cell::RefCell::new(Vec::new());
    let probe = |name: &str| {
        names.borrow_mut().push(name.to_owned());
        Some("x".to_owned())
    };
    let _ = interpolate_scalar(input, &probe);
    names.into_inner()
}

enum SpanResult {
    /// An `env.X` span resolved to a value.
    Resolved(String),
    /// A `vars.X` / `ctx.X` span — re-emit verbatim for a later pass.
    Defer,
}

fn resolve_span<F>(inner: &str, env: &F) -> Result<SpanResult, InterpError>
where
    F: Fn(&str) -> Option<String>,
{
    let (ref_part, filter_part) = match inner.split_once('|') {
        Some((r, filt)) => (r.trim(), Some(filt.trim())),
        None => (inner, None),
    };
    // No `<namespace>.<name>` shape → not alint interpolation. This is
    // almost always a foreign `{{...}}` template action (Go's
    // `{{end}}` / `{{range}}`, etc.). Leave it verbatim rather than
    // erroring on the open-ended space of other template languages.
    let Some((namespace, name)) = ref_part.split_once('.') else {
        return Ok(SpanResult::Defer);
    };
    let namespace = namespace.trim();
    let name = name.trim();
    match namespace {
        // Resolved by later passes (template expansion / renderer).
        "vars" | "ctx" => Ok(SpanResult::Defer),
        "env" => {
            let default = parse_default_filter(filter_part, inner)?;
            match env(name) {
                Some(v) if !v.is_empty() => Ok(SpanResult::Resolved(v)),
                _ => default.map_or_else(
                    || {
                        Err(InterpError::UndefinedEnv {
                            name: name.to_owned(),
                        })
                    },
                    |d| Ok(SpanResult::Resolved(d)),
                ),
            }
        }
        // An unknown namespace that closely resembles `env`/`vars` is
        // almost certainly an alint typo — surface it. Anything else
        // (`{{json .}}`, `{{cookiecutter.x}}`, …) is a foreign
        // template; leave it verbatim so it reaches the external tool
        // / regex unchanged.
        other if looks_like_typo(other, "env") || looks_like_typo(other, "vars") => {
            Err(InterpError::UnknownNamespace {
                namespace: other.to_owned(),
            })
        }
        _ => Ok(SpanResult::Defer),
    }
}

/// Parse the optional `| default('value')` filter. Returns the default
/// string, or `None` when no filter was present. Anything other than a
/// well-formed `default(...)` is a [`InterpError::Malformed`].
fn parse_default_filter(filter: Option<&str>, inner: &str) -> Result<Option<String>, InterpError> {
    let Some(filter) = filter else {
        return Ok(None);
    };
    let malformed = |reason: &str| InterpError::Malformed {
        span: format!("{{{{{inner}}}}}"),
        reason: reason.to_owned(),
    };
    let Some(rest) = filter.strip_prefix("default").map(str::trim_start) else {
        return Err(malformed(&format!(
            "unknown filter `{filter}` (only `default(...)` is supported)"
        )));
    };
    let inside = rest
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .map(str::trim);
    let Some(inside) = inside else {
        return Err(malformed("filter `default` expects `default('value')`"));
    };
    strip_quotes(inside)
        .map(|v| Some(v.to_owned()))
        .ok_or_else(|| malformed("default value must be quoted: `default('value')`"))
}

/// Strip a single matched pair of `'…'` or `"…"` quotes; `None` if the
/// string is not quoted.
fn strip_quotes(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let len = s.len();
    if len >= 2
        && ((bytes[0] == b'\'' && bytes[len - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[len - 1] == b'"'))
    {
        Some(&s[1..len - 1])
    } else {
        None
    }
}

/// Re-parse a fully-resolved substitution result so `"72"` becomes a
/// number and `"true"` a bool — honouring the
/// schema-validation-after-interpolation contract. Falls back to a
/// plain string for anything that is not an unambiguous number/bool.
///
/// Known edge: a string-typed field whose env value is bare-numeric
/// (`"72"`) or bare-bool (`"true"`) re-types to `Number`/`Bool`; if the
/// field truly wants those characters as a string, the subsequent typed
/// deserialization surfaces a clear type error. This matches the design
/// doc's accepted trade-off.
fn retype_scalar(text: String) -> Value {
    // A bare number / bool literal is short; anything long is a string regardless.
    // Skip the YAML re-parse for a long value so a huge INTERPOLATED value (e.g. an
    // attacker-controlled `{{env.X}}` holding a `[[[…` flow bomb, substituted AFTER
    // the config's own flow-depth guard already ran) never reaches the parser.
    if text.len() > 1024 {
        return Value::String(text);
    }
    match alint_core::yaml::from_str::<Value>(&text) {
        Ok(v @ (Value::Number(_) | Value::Bool(_))) => v,
        _ => Value::String(text),
    }
}

/// [`retype_scalar`] for a version 2 field that takes a boolean: a value
/// that spells one reads as one, and anything else (a number too) stays the
/// text it is, for the decode to refuse as the boolean it is not.
fn retype_boolean(text: String) -> Value {
    match retype_scalar(text.clone()) {
        boolean @ Value::Bool(_) => boolean,
        _ => Value::String(text),
    }
}

fn retype_integer(text: String) -> Value {
    match text.trim().parse::<i64>() {
        Ok(n) => Value::from(n),
        Err(_) => Value::String(text),
    }
}

/// `true` if `input` is within Levenshtein distance 1 of `target`.
/// Gates the unknown-namespace *error*: only a near-miss of
/// `env`/`vars` is treated as an alint typo; everything further away
/// is assumed to be a foreign `{{...}}` template and passes through.
/// Kept conservative (distance 1) so a real foreign namespace is never
/// mistaken for a typo and made to hard-fail the load.
fn looks_like_typo(input: &str, target: &str) -> bool {
    levenshtein(input, target) <= 1
}

/// `text`'s characters, which a [`Within`] weighs other names against:
/// collected once however many names a suggestion weighs.
pub(crate) fn chars_of(text: &str) -> Vec<char> {
    let chars: Vec<char> = text.chars().collect();
    #[cfg(test)]
    DISTANCE_CHARS.with(|read| read.set(read.get() + chars.len()));
    chars
}

/// The Levenshtein distance from `wanted` to a text fed to it a piece at a
/// time, while that distance is at most `limit`. A distance within `limit`
/// needs a path that stays within `limit` cells of the table's diagonal, so
/// each character fed computes one row of `2 * limit + 1` cells, and a row
/// whose cells all exceed `limit` ends the walk: no longer text comes back
/// within it.
///
/// A config's names are untrusted, so a suggestion must cost neither the
/// product of two long names nor a long name's length once per name it is
/// weighed against: `wanted` is read once ([`chars_of`]); a name whose length
/// cannot come within `limit` is skipped before a character of it is read
/// ([`Within::may_reach`]); a name is read only as far as it can come within
/// `limit`; and a prefix that many names share (an alias) is fed once, to a
/// state each of them then continues from a clone of.
#[derive(Debug, Clone)]
pub(crate) struct Within<'w> {
    wanted: &'w [char],
    limit: usize,
    /// The characters fed so far: the row `band` holds.
    fed: usize,
    /// Row `fed`'s cells for the columns `fed - limit ..= fed + limit`; a
    /// column outside `0..=wanted.len()` holds `limit + 1`, as does any cell
    /// past `limit`.
    band: Vec<usize>,
    /// The next row, as it is computed.
    next: Vec<usize>,
}

impl<'w> Within<'w> {
    pub(crate) fn new(wanted: &'w [char], limit: usize) -> Self {
        let over = limit + 1;
        // Row 0: column `j` holds `j`.
        let band = (0..=2 * limit)
            .map(|k| match k.checked_sub(limit) {
                Some(column) if column <= wanted.len() => column.min(over),
                _ => over,
            })
            .collect();
        Self {
            wanted,
            limit,
            fed: 0,
            band,
            next: vec![over; 2 * limit + 1],
        }
    }

    /// Whether a text `bytes` long, fed next, could end within the limit: it
    /// holds between a quarter of its bytes and all of them as characters.
    pub(crate) fn may_reach(&self, bytes: usize) -> bool {
        let wanted = self.wanted.len();
        self.fed + bytes + self.limit >= wanted
            && self.fed + bytes.div_ceil(4) <= wanted + self.limit
    }

    /// Feed `text`; `false` (and the text read no further) once no text that
    /// starts with what has been fed can end within the limit.
    pub(crate) fn feed(&mut self, text: &str) -> bool {
        text.chars().all(|character| self.step(character))
    }

    /// The distance from `wanted` to the text fed, when it is within the
    /// limit.
    pub(crate) fn distance(&self) -> Option<usize> {
        // Column `wanted.len()` of row `fed`.
        let k = (self.wanted.len() + self.limit).checked_sub(self.fed)?;
        self.band
            .get(k)
            .copied()
            .filter(|distance| *distance <= self.limit)
    }

    /// Compute the row of one more character; `false` when none of its
    /// cells is within the limit.
    fn step(&mut self, character: char) -> bool {
        #[cfg(test)]
        DISTANCE_CHARS.with(|read| read.set(read.get() + 1));
        let over = self.limit + 1;
        let row = self.fed + 1;
        let mut alive = false;
        self.next.fill(over);
        for k in 0..self.next.len() {
            // Column `row - limit + k`; one left of column 0 holds `over`.
            let Some(column) = (row + k).checked_sub(self.limit) else {
                continue;
            };
            if column > self.wanted.len() {
                break;
            }
            #[cfg(test)]
            DISTANCE_CELLS.with(|cells| cells.set(cells.get() + 1));
            let cell = if column == 0 {
                row.min(over)
            } else {
                // Row `fed`'s band starts a column left of this one's, so the
                // cell above-left is at `k` and the one above at `k + 1`.
                let diagonal = self.band[k] + usize::from(self.wanted[column - 1] != character);
                let above = self.band.get(k + 1).map_or(over, |cell| cell + 1);
                let left = k.checked_sub(1).map_or(over, |left| self.next[left] + 1);
                diagonal.min(above).min(left).min(over)
            };
            self.next[k] = cell;
            alive |= cell <= self.limit;
        }
        std::mem::swap(&mut self.band, &mut self.next);
        self.fed = row;
        alive
    }
}

/// The Levenshtein distance between `a` and `b` when it is at most `limit`
/// ([`Within`]).
#[cfg(test)]
pub(crate) fn levenshtein_within(a: &str, b: &str, limit: usize) -> Option<usize> {
    let a = chars_of(a);
    let mut within = Within::new(&a, limit);
    if !within.may_reach(b.len()) || !within.feed(b) {
        return None;
    }
    within.distance()
}

#[cfg(test)]
thread_local! {
    /// The edit-distance cells [`levenshtein`] and [`Within`] computed on
    /// this thread: the tests bound the work a suggestion does.
    pub(crate) static DISTANCE_CELLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// The characters [`chars_of`] collected and [`Within`] read on this
    /// thread: a suggestion that copied or read a long name once per name it
    /// weighs it against computes few cells, and this counts what it read.
    pub(crate) static DISTANCE_CHARS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) fn levenshtein(a: &str, b: &str) -> usize {
    let a = chars_of(a);
    let b = chars_of(b);
    #[cfg(test)]
    DISTANCE_CELLS.with(|cells| cells.set(cells.get() + a.len() * b.len()));
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: std::collections::HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    fn interp(input: &str, env: &impl Fn(&str) -> Option<String>) -> Result<String, InterpError> {
        interpolate_scalar(input, env).map(|i| i.text)
    }

    #[test]
    fn literal_without_markers_passes_through() {
        let env = fake_env(&[]);
        assert_eq!(interp("origin/main", &env).unwrap(), "origin/main");
        assert_eq!(interp("src/**/*.rs", &env).unwrap(), "src/**/*.rs");
    }

    #[test]
    fn simple_env_substitution() {
        let env = fake_env(&[("ALINT_BASE_SHA", "deadbeef")]);
        assert_eq!(interp("{{env.ALINT_BASE_SHA}}", &env).unwrap(), "deadbeef");
    }

    #[test]
    fn retype_scalar_skips_long_values_without_reparse() {
        // A short bare number / bool re-types; a long value stays a String WITHOUT a
        // YAML re-parse -- so a `[[[…` flow bomb substituted from an
        // attacker-controlled `{{env.X}}` (after the config's own flow guard) can
        // never reach the parser here.
        assert!(matches!(retype_scalar("72".to_string()), Value::Number(_)));
        assert!(matches!(
            retype_scalar("true".to_string()),
            Value::Bool(true)
        ));
        let bomb = "[".repeat(200_000);
        assert!(matches!(retype_scalar(bomb), Value::String(_)));
    }

    #[test]
    fn default_used_when_var_unset() {
        let env = fake_env(&[]);
        assert_eq!(
            interp("{{env.MISSING | default('origin/main')}}", &env).unwrap(),
            "origin/main"
        );
    }

    #[test]
    fn default_ignored_when_var_set() {
        let env = fake_env(&[("TIER", "prod")]);
        assert_eq!(
            interp("{{env.TIER | default('dev')}}", &env).unwrap(),
            "prod"
        );
    }

    #[test]
    fn unset_var_without_default_errors() {
        let env = fake_env(&[]);
        let err = interp("{{env.ALINT_BASE_SHA}}", &env).unwrap_err();
        assert_eq!(
            err,
            InterpError::UndefinedEnv {
                name: "ALINT_BASE_SHA".to_owned()
            }
        );
        // Error text points the user at the default filter.
        assert!(err.to_string().contains("default('...')"), "{err}");
    }

    #[test]
    fn empty_var_treated_as_unset() {
        let env = fake_env(&[("EMPTY", "")]);
        assert_eq!(
            interp("{{env.EMPTY | default('fallback')}}", &env).unwrap(),
            "fallback"
        );
    }

    #[test]
    fn span_embedded_in_surrounding_text() {
        let env = fake_env(&[("TEAM", "payments")]);
        assert_eq!(
            interp("https://policy.{{env.TEAM}}.example.com/v1.yml", &env).unwrap(),
            "https://policy.payments.example.com/v1.yml"
        );
    }

    #[test]
    fn multiple_spans_in_one_value() {
        let env = fake_env(&[("A", "x"), ("B", "y")]);
        assert_eq!(interp("{{env.A}}-{{env.B}}", &env).unwrap(), "x-y");
    }

    #[test]
    fn vars_span_deferred_verbatim() {
        let env = fake_env(&[]);
        assert_eq!(
            interp("{{vars.threshold}}", &env).unwrap(),
            "{{vars.threshold}}"
        );
    }

    #[test]
    fn ctx_span_deferred_verbatim() {
        let env = fake_env(&[]);
        assert_eq!(
            interp("matched {{ctx.match}}", &env).unwrap(),
            "matched {{ctx.match}}"
        );
    }

    #[test]
    fn mixed_env_resolved_and_vars_deferred() {
        let env = fake_env(&[("ENV", "ci")]);
        assert_eq!(
            interp("{{env.ENV}}/{{vars.name}}", &env).unwrap(),
            "ci/{{vars.name}}"
        );
    }

    #[test]
    fn close_typo_of_known_namespace_errors_with_hint() {
        let env = fake_env(&[]);
        // `en` is Levenshtein distance 1 from `env` → flagged as a typo.
        let err = interp("{{en.CI}}", &env).unwrap_err();
        assert_eq!(
            err,
            InterpError::UnknownNamespace {
                namespace: "en".to_owned()
            }
        );
        let msg = err.to_string();
        assert!(msg.contains("typo for `env`?"), "{msg}");
        assert!(msg.contains("Supported namespaces: env, vars."), "{msg}");
    }

    #[test]
    fn foreign_namespace_passes_through_verbatim() {
        let env = fake_env(&[]);
        // Not close to env/vars → assumed foreign template, left as-is.
        assert_eq!(
            interp("{{secrets.TOKEN}}", &env).unwrap(),
            "{{secrets.TOKEN}}"
        );
    }

    #[test]
    fn go_template_command_arg_passes_through() {
        // Regression: examples/nixos-nixpkgs uses
        // `command: ["actionlint", "-format", "{{json .}}"]` — a Go
        // template, NOT alint interpolation. Must survive untouched.
        let env = fake_env(&[]);
        assert_eq!(interp("{{json .}}", &env).unwrap(), "{{json .}}");
        assert_eq!(interp("{{end}}", &env).unwrap(), "{{end}}");
        assert_eq!(interp("{{.Foo}}", &env).unwrap(), "{{.Foo}}");
    }

    #[test]
    fn dotless_span_passes_through_verbatim() {
        let env = fake_env(&[]);
        assert_eq!(interp("{{justaword}}", &env).unwrap(), "{{justaword}}");
    }

    #[test]
    fn malformed_unknown_filter() {
        let env = fake_env(&[("X", "v")]);
        let err = interp("{{env.X | upper}}", &env).unwrap_err();
        assert!(matches!(err, InterpError::Malformed { .. }), "{err:?}");
        assert!(err.to_string().contains("default(...)"), "{err}");
    }

    #[test]
    fn unclosed_marker_treated_as_literal() {
        let env = fake_env(&[]);
        assert_eq!(interp("a {{ b", &env).unwrap(), "a {{ b");
    }

    #[test]
    fn double_quoted_default_accepted() {
        let env = fake_env(&[]);
        assert_eq!(
            interp(r#"{{env.MISSING | default("x")}}"#, &env).unwrap(),
            "x"
        );
    }

    #[test]
    fn whitespace_inside_span_tolerated() {
        let env = fake_env(&[]);
        assert_eq!(
            interp("{{  env.MISSING  |  default('y')  }}", &env).unwrap(),
            "y"
        );
    }

    #[test]
    fn substituted_flag_reflects_env_resolution() {
        let env = fake_env(&[("X", "v")]);
        assert!(interpolate_scalar("{{env.X}}", &env).unwrap().substituted);
        assert!(!interpolate_scalar("plain", &env).unwrap().substituted);
        // A deferred-only value did not substitute anything here.
        assert!(!interpolate_scalar("{{vars.x}}", &env).unwrap().substituted);
    }

    #[test]
    fn walker_skips_id_kind_level_inside_rule_scope() {
        let env = fake_env(&[("X", "SUBBED")]);
        let mut v: Value = alint_core::yaml::from_str(
            "rules:\n  - id: \"{{env.X}}\"\n    kind: \"{{env.X}}\"\n    \
             level: \"{{env.X}}\"\n    paths: \"{{env.X}}\"",
        )
        .unwrap();
        interpolate_value(&mut v, &env).unwrap();
        let rule = &v.as_mapping().unwrap()["rules"].as_sequence().unwrap()[0];
        let map = rule.as_mapping().unwrap();
        // Rule identity / type / severity keep the literal template.
        assert_eq!(map["id"].as_str().unwrap(), "{{env.X}}");
        assert_eq!(map["kind"].as_str().unwrap(), "{{env.X}}");
        assert_eq!(map["level"].as_str().unwrap(), "{{env.X}}");
        // Value field is interpolated.
        assert_eq!(map["paths"].as_str().unwrap(), "SUBBED");
    }

    #[test]
    fn walker_interpolates_vars_or_facts_entry_named_like_a_rule_key() {
        // A `vars:`/`facts:` entry that happens to be named `level`
        // (or `id`/`kind`) is NOT a rule meta field — it must still
        // interpolate. Regression guard for the previously-global skip.
        let env = fake_env(&[("X", "prod")]);
        let mut v: Value =
            alint_core::yaml::from_str("vars:\n  level: \"{{env.X}}\"\n  id: \"{{env.X}}\"")
                .unwrap();
        interpolate_value(&mut v, &env).unwrap();
        let vars = v.as_mapping().unwrap()["vars"].as_mapping().unwrap();
        assert_eq!(vars["level"].as_str().unwrap(), "prod");
        assert_eq!(vars["id"].as_str().unwrap(), "prod");
    }

    #[test]
    fn retype_keeps_collection_looking_value_as_string() {
        // Only Number/Bool re-type; a resolved value that parses as a
        // YAML list or mapping must stay a string, never silently
        // become a Sequence/Mapping in a string-typed field.
        let env = fake_env(&[("L", "[a, b]"), ("M", "{k: v}")]);
        let mut v: Value =
            alint_core::yaml::from_str("a: \"{{env.L}}\"\nb: \"{{env.M}}\"").unwrap();
        interpolate_value(&mut v, &env).unwrap();
        let map = v.as_mapping().unwrap();
        assert_eq!(map["a"].as_str().unwrap(), "[a, b]");
        assert_eq!(map["b"].as_str().unwrap(), "{k: v}");
    }

    #[test]
    fn empty_string_default_resolves_to_empty() {
        let env = fake_env(&[]);
        assert_eq!(interp("{{env.MISSING | default('')}}", &env).unwrap(), "");
    }

    #[test]
    fn walker_retypes_resolved_integer_field() {
        let env = fake_env(&[]);
        let mut v: Value =
            alint_core::yaml::from_str("subject_max_length: \"{{env.MAX | default('72')}}\"")
                .unwrap();
        interpolate_value(&mut v, &env).unwrap();
        let n = v.as_mapping().unwrap()["subject_max_length"]
            .as_u64()
            .unwrap();
        assert_eq!(n, 72);
    }

    #[test]
    fn walker_retypes_resolved_bool_field() {
        let env = fake_env(&[("FLAG", "true")]);
        let mut v: Value = alint_core::yaml::from_str("if_present: \"{{env.FLAG}}\"").unwrap();
        interpolate_value(&mut v, &env).unwrap();
        assert_eq!(v.as_mapping().unwrap()["if_present"].as_bool(), Some(true));
    }

    #[test]
    fn walker_keeps_string_when_value_stays_templated() {
        let env = fake_env(&[]);
        let mut v: Value = alint_core::yaml::from_str("message: \"{{vars.x}}\"").unwrap();
        interpolate_value(&mut v, &env).unwrap();
        // Deferred vars span stays a string for the later pass.
        assert_eq!(
            v.as_mapping().unwrap()["message"].as_str().unwrap(),
            "{{vars.x}}"
        );
    }

    #[test]
    fn walker_recurses_into_sequences() {
        let env = fake_env(&[("DIR", "pkgs")]);
        let mut v: Value =
            alint_core::yaml::from_str("paths:\n  - \"{{env.DIR}}/a\"\n  - \"{{env.DIR}}/b\"")
                .unwrap();
        interpolate_value(&mut v, &env).unwrap();
        let seq = v.as_mapping().unwrap()["paths"].as_sequence().unwrap();
        assert_eq!(seq[0].as_str().unwrap(), "pkgs/a");
        assert_eq!(seq[1].as_str().unwrap(), "pkgs/b");
    }

    #[test]
    fn env_names_lists_each_name_the_resolver_reads() {
        assert_eq!(
            env_names("{{env.A}}-{{ env.B | default('x') }}{{vars.C}}{{env.A}}"),
            ["A", "B", "A"]
        );
        assert_eq!(
            env_names("plain {{vars.X}} {{ctx.Y}}"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn reads_env_matches_the_resolver() {
        assert!(reads_env("{{env.NPM_TOKEN}}"));
        assert!(reads_env("{{ env.NPM_TOKEN }}"));
        assert!(reads_env("{{  env . NPM_TOKEN | default('x') }}"));
        assert!(reads_env("prefix-{{env.A}}-suffix"));
        assert!(reads_env("{{vars.a}}{{env.B}}"));
        assert!(!reads_env("{{vars.token}}"));
        assert!(!reads_env("{{ctx.path}}"));
        assert!(!reads_env("{{end}}"));
        assert!(!reads_env("plain"));
        assert!(!reads_env("{{env.UNCLOSED"));
    }

    #[test]
    fn levenshtein_basic() {
        assert_eq!(levenshtein("env", "env"), 0);
        assert_eq!(levenshtein("evn", "env"), 2);
        assert_eq!(levenshtein("en", "env"), 1);
        // `secrets` shares an `e` with `env`, so distance is 6 not 7.
        assert_eq!(levenshtein("secrets", "env"), 6);
    }

    /// The banded distance agrees with the full one wherever that is within
    /// the limit, and is `None` past it: every pair of strings of up to five
    /// characters over a three-letter alphabet, and some longer ones.
    #[test]
    fn the_bounded_distance_is_the_distance_within_its_limit() {
        let mut words = vec![String::new()];
        let mut last = vec![String::new()];
        for _ in 0..5 {
            last = last
                .iter()
                .flat_map(|word| ['a', 'b', 'é'].map(|c| format!("{word}{c}")))
                .collect();
            words.extend(last.iter().cloned());
        }
        for a in &words {
            for b in &words {
                let distance = levenshtein(a, b);
                for limit in 0..=3 {
                    let expected = (distance <= limit).then_some(distance);
                    assert_eq!(
                        levenshtein_within(a, b, limit),
                        expected,
                        "{a:?} {b:?} {limit}"
                    );
                }
            }
        }
        let long = "selector-".repeat(30);
        for other in [
            format!("{long}x"),
            format!("x{long}"),
            format!("{}ab", &long[1..]),
            format!("{}ab", &long[1..long.len() - 1]),
            long.replacen("sel", "les", 1),
            long.replacen("sel", "les", 2),
            long.clone(),
        ] {
            let expected = Some(levenshtein(&long, &other)).filter(|distance| *distance <= 2);
            assert_eq!(levenshtein_within(&long, &other, 2), expected, "{other}");
            assert_eq!(levenshtein_within(&other, &long, 2), expected, "{other}");
        }
        assert_eq!(levenshtein_within(&long, &format!("x{long}"), 2), Some(1));
        assert_eq!(
            levenshtein_within(&long, &long.replacen("sel", "les", 2), 2),
            None
        );
    }
}
