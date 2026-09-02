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

use serde_yaml_ng::Value;

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
    walk(value, env, false)
}

/// `in_rule_scope` is `true` while descending through a rule / template
/// entry (anything under a top-level [`RULE_BEARING_KEYS`] key), where
/// [`SKIP_KEYS`] must stay literal. It latches on: once inside a rule,
/// nested rules (e.g. a `for_each_dir`'s `require:` list) inherit it.
fn walk<F>(value: &mut Value, env: &F, in_rule_scope: bool) -> Result<(), InterpError>
where
    F: Fn(&str) -> Option<String>,
{
    match value {
        Value::String(s) => {
            let result = interpolate_scalar(s, env)?;
            // Re-type only when the value was fully resolved (no
            // deferred `{{...}}` remains) AND interpolation actually
            // substituted something — so an interpolated integer-field
            // value validates as a number rather than a string, while
            // pre-existing literal strings are never silently re-typed.
            if result.substituted && !result.text.contains("{{") {
                *value = retype_scalar(result.text);
            } else {
                *s = result.text;
            }
            Ok(())
        }
        Value::Sequence(seq) => {
            for v in seq.iter_mut() {
                walk(v, env, in_rule_scope)?;
            }
            Ok(())
        }
        Value::Mapping(map) => {
            for (k, v) in &mut *map {
                let key = k.as_str();
                if in_rule_scope && key.is_some_and(|kk| SKIP_KEYS.contains(&kk)) {
                    continue;
                }
                // A top-level `rules:` / `templates:` key turns the
                // skip on for its subtree; once on, it stays on.
                let child_scope =
                    in_rule_scope || key.is_some_and(|kk| RULE_BEARING_KEYS.contains(&kk));
                walk(v, env, child_scope)?;
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
    // the config's own flow-depth guard already ran) can't be handed to libyaml to
    // chew on super-linearly.
    if text.len() > 1024 {
        return Value::String(text);
    }
    match serde_yaml_ng::from_str::<Value>(&text) {
        Ok(v @ (Value::Number(_) | Value::Bool(_))) => v,
        _ => Value::String(text),
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

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
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
        // super-linear YAML re-parse -- so a `[[[…` flow bomb substituted from an
        // attacker-controlled `{{env.X}}` (after the config's own flow guard) can
        // never reach libyaml here.
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
        let mut v: Value = serde_yaml_ng::from_str(
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
            serde_yaml_ng::from_str("vars:\n  level: \"{{env.X}}\"\n  id: \"{{env.X}}\"").unwrap();
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
        let mut v: Value = serde_yaml_ng::from_str("a: \"{{env.L}}\"\nb: \"{{env.M}}\"").unwrap();
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
            serde_yaml_ng::from_str("subject_max_length: \"{{env.MAX | default('72')}}\"").unwrap();
        interpolate_value(&mut v, &env).unwrap();
        let n = v.as_mapping().unwrap()["subject_max_length"]
            .as_u64()
            .unwrap();
        assert_eq!(n, 72);
    }

    #[test]
    fn walker_retypes_resolved_bool_field() {
        let env = fake_env(&[("FLAG", "true")]);
        let mut v: Value = serde_yaml_ng::from_str("if_present: \"{{env.FLAG}}\"").unwrap();
        interpolate_value(&mut v, &env).unwrap();
        assert_eq!(v.as_mapping().unwrap()["if_present"].as_bool(), Some(true));
    }

    #[test]
    fn walker_keeps_string_when_value_stays_templated() {
        let env = fake_env(&[]);
        let mut v: Value = serde_yaml_ng::from_str("message: \"{{vars.x}}\"").unwrap();
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
            serde_yaml_ng::from_str("paths:\n  - \"{{env.DIR}}/a\"\n  - \"{{env.DIR}}/b\"")
                .unwrap();
        interpolate_value(&mut v, &env).unwrap();
        let seq = v.as_mapping().unwrap()["paths"].as_sequence().unwrap();
        assert_eq!(seq[0].as_str().unwrap(), "pkgs/a");
        assert_eq!(seq[1].as_str().unwrap(), "pkgs/b");
    }

    #[test]
    fn levenshtein_basic() {
        assert_eq!(levenshtein("env", "env"), 0);
        assert_eq!(levenshtein("evn", "env"), 2);
        assert_eq!(levenshtein("en", "env"), 1);
        // `secrets` shares an `e` with `env`, so distance is 6 not 7.
        assert_eq!(levenshtein("secrets", "env"), 6);
    }
}
