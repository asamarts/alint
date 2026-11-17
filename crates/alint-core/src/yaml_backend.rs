//! The parser behind [`crate::yaml`]'s decoding (ADR-0041).
//!
//! ayaml is the parser. A build with `alint-core`'s `yaml-reference`
//! feature, which no release enables, can choose another for testing with
//! `ALINT_YAML_PARSER`, read once per process:
//! - `ayaml`, or unset: ayaml;
//! - `libyaml`: libyaml, through `yaml_serde` 0.10.7, the parser v0.22 used;
//! - `compare`: ayaml, whose result the caller gets, checked against libyaml
//!   and saphyr; a difference that nothing explains panics (see `compare`).
//!
//! Any other value panics, so that a misspelled setting cannot fall back to
//! ayaml unnoticed. Without the feature, the variable is not read. The
//! feature is for alint's own tests, and a crate that depends on alint-core
//! must not enable it: the variable then chooses the parser of every decode
//! in the process, for every crate in it.
//!
//! libyaml has none of the limits ayaml decodes under, and expands an alias
//! bomb in full, so neither mode lets it read an input past them (see
//! [`libyaml_limits`]).
//!
//! Only decoding from text goes through a backend: [`crate::yaml::from_value`],
//! [`crate::yaml::to_value`] and [`crate::yaml::to_string`] work on ayaml's
//! data model, which alint's own types hide (ADR-0038). The pre-parse guards
//! of [`crate::yaml_depth`] measure what ayaml reads, so they decode with
//! ayaml whatever the backend.

use serde::Deserialize;

use crate::yaml::Error;

#[cfg(feature = "yaml-reference")]
mod compare;

/// The parser a decode goes through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Parser {
    Ayaml,
    #[cfg(feature = "yaml-reference")]
    Libyaml,
    #[cfg(feature = "yaml-reference")]
    Compare,
}

/// The variable that chooses the parser, in a build with the feature.
#[cfg(feature = "yaml-reference")]
pub(crate) const PARSER_VARIABLE: &str = "ALINT_YAML_PARSER";

/// The parser this process decodes with.
pub(crate) fn parser() -> Parser {
    #[cfg(feature = "yaml-reference")]
    {
        static PARSER: std::sync::OnceLock<Parser> = std::sync::OnceLock::new();
        *PARSER.get_or_init(|| parser_named(std::env::var(PARSER_VARIABLE).ok().as_deref()))
    }
    #[cfg(not(feature = "yaml-reference"))]
    Parser::Ayaml
}

/// The parser `ALINT_YAML_PARSER` names, unset when `None`.
///
/// # Panics
///
/// On a value that names no parser.
#[cfg(feature = "yaml-reference")]
pub(crate) fn parser_named(value: Option<&str>) -> Parser {
    match value {
        None | Some("" | "ayaml") => Parser::Ayaml,
        Some("libyaml") => Parser::Libyaml,
        Some("compare") => Parser::Compare,
        Some(other) => {
            panic!("{PARSER_VARIABLE}={other:?} names no parser: use ayaml, libyaml or compare")
        }
    }
}

/// [`crate::yaml::from_str`] through the chosen parser.
pub(crate) fn from_str<'de, T: Deserialize<'de>>(input: &'de str) -> Result<T, Error> {
    match parser() {
        Parser::Ayaml => crate::yaml::ayaml_from_str(input),
        #[cfg(feature = "yaml-reference")]
        Parser::Libyaml => libyaml_from_str(input),
        #[cfg(feature = "yaml-reference")]
        Parser::Compare => compare::from_str(input),
    }
}

/// The `serde_json` tree the structured-format rules query, through the
/// chosen parser (see [`crate::structured_format::json_seed`]).
pub(crate) fn json_tree(input: &str) -> Result<serde_json::Value, Error> {
    match parser() {
        Parser::Ayaml => ayaml_json_tree(input),
        #[cfg(feature = "yaml-reference")]
        Parser::Libyaml => libyaml_json_tree(input),
        #[cfg(feature = "yaml-reference")]
        Parser::Compare => compare::json_tree(input),
    }
}

/// The tree of `json_tree` through ayaml, whatever parser the build chose.
pub fn ayaml_json_tree(input: &str) -> Result<serde_json::Value, Error> {
    crate::yaml::ayaml_from_str_seed(input, crate::structured_format::json_seed(input))
        .map_err(|error| Error::from_text(input, error))
}

/// [`from_str`] through libyaml, within the limits a decode through ayaml
/// runs under (see [`libyaml_limits`]).
#[cfg(feature = "yaml-reference")]
pub(crate) fn libyaml_from_str<'de, T: Deserialize<'de>>(input: &'de str) -> Result<T, Error> {
    libyaml_limits(input)?;
    libyaml_decode(input)
}

/// [`from_str`] through libyaml, with no limit checked first: for compare
/// mode, which checks them itself.
#[cfg(feature = "yaml-reference")]
pub(crate) fn libyaml_decode<'de, T: Deserialize<'de>>(input: &'de str) -> Result<T, Error> {
    before_libyaml_decode();
    yaml_serde::from_str(input).map_err(Error::libyaml)
}

/// The tree of `json_tree` through libyaml, within the limits a decode
/// through ayaml runs under.
#[cfg(feature = "yaml-reference")]
pub fn libyaml_json_tree(input: &str) -> Result<serde_json::Value, Error> {
    use serde::de::DeserializeSeed as _;
    libyaml_limits(input)?;
    before_libyaml_decode();
    crate::structured_format::json_seed(input)
        .deserialize(yaml_serde::Deserializer::from_str(input))
        .map_err(Error::libyaml)
}

/// Whether libyaml may read `input`: the limits ayaml decodes under, which
/// libyaml has none of, checked first with alint's own guards. The input cap;
/// flow nesting, on which libyaml's tokenizer is super-linear; and the
/// alias-expansion budget, charged as libyaml replays aliases, since libyaml
/// reads some inputs differently from ayaml and would expand such a bomb in
/// full (see [`libyaml_expansion_within_limit`]). The
/// error is one of libyaml's, as a decode would give.
#[cfg(feature = "yaml-reference")]
pub(crate) fn libyaml_limits(input: &str) -> Result<(), Error> {
    use serde::de::Error as _;
    if input.len() > crate::MAX_STRUCTURED_BYTES {
        return Err(Error::libyaml(yaml_serde::Error::custom(format!(
            "the input is longer than the {} bytes alint reads",
            crate::MAX_STRUCTURED_BYTES
        ))));
    }
    if !crate::yaml_depth::flow_depth_within_limit(input) {
        return Err(Error::libyaml(yaml_serde::Error::custom(format!(
            "YAML flow nesting exceeds the maximum supported depth ({})",
            crate::yaml_depth::MAX_YAML_FLOW_DEPTH
        ))));
    }
    libyaml_expansion_within_limit(input).map_err(Error::libyaml)
}

/// The pass of [`crate::yaml_depth::expansion_within_limit`] as libyaml reads `text`, for the
/// test-only reference backend (ADR-0041): the same budget, charged as
/// `yaml_serde` replays aliases. libyaml has no budget of its own, and reads
/// some text differently from ayaml, so text ayaml refuses for its syntax can
/// be an alias bomb to libyaml (a byte order mark before `---`). The error is
/// libyaml's, once the budget is exceeded; any other error falls through, as
/// in [`crate::yaml_depth::expansion_within_limit`].
#[cfg(feature = "yaml-reference")]
fn libyaml_expansion_within_limit(text: &str) -> Result<(), yaml_serde::Error> {
    use serde::de::DeserializeSeed as _;
    // Without a `*`, there is no alias to replay.
    if !text.as_bytes().contains(&b'*') {
        return Ok(());
    }
    let remaining = std::cell::Cell::new(crate::yaml_depth::expansion_budget(text.len()));
    let exceeded = std::cell::Cell::new(false);
    let seed = crate::yaml_depth::NodeBudget {
        remaining: &remaining,
        exceeded: &exceeded,
    };
    match seed.deserialize(yaml_serde::Deserializer::from_str(text)) {
        Err(error) if exceeded.get() => Err(error),
        _ => Ok(()),
    }
}

/// Called before every full decode through libyaml: the reference backend's
/// and the differential harness's. A test can forbid one on its thread, to
/// show that an input never reaches libyaml without waiting for libyaml to
/// expand it.
#[cfg(any(test, feature = "yaml-reference"))]
pub(crate) fn before_libyaml_decode() {
    #[cfg(test)]
    assert!(
        !LIBYAML_FORBIDDEN.with(std::cell::Cell::get),
        "a full decode through libyaml, which this test forbids"
    );
}

#[cfg(test)]
thread_local! {
    /// Whether this thread's test forbids a full decode through libyaml.
    static LIBYAML_FORBIDDEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `message` without each ` at line L column C` and ` at position P` that
/// libyaml's errors put after their problem and context.
#[cfg(any(test, feature = "yaml-reference"))]
pub(crate) fn without_marks(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(at) = rest.find(" at ") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 4..];
        let numbers = |text: &str, words: &[&str]| -> Option<usize> {
            let mut text = text;
            let mut len = 0;
            for word in words {
                let stripped = text.strip_prefix(word)?;
                let digits = stripped.len()
                    - stripped
                        .trim_start_matches(|ch: char| ch.is_ascii_digit())
                        .len();
                if digits == 0 {
                    return None;
                }
                len += word.len() + digits;
                text = &stripped[digits..];
            }
            Some(len)
        };
        if let Some(len) =
            numbers(after, &["line ", " column "]).or_else(|| numbers(after, &["position "]))
        {
            rest = &after[len..];
        } else {
            out.push_str(" at ");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "yaml-reference"))]
    #[test]
    fn without_the_feature_ayaml_decodes() {
        assert_eq!(parser(), Parser::Ayaml);
    }

    #[test]
    fn the_json_tree_is_the_chosen_parsers() {
        assert_eq!(
            json_tree("a: [1, x]\nb: ~\n").unwrap(),
            serde_json::json!({"a": [1, "x"], "b": null})
        );
        assert!(json_tree("a: [1\n").is_err());
    }

    #[cfg(feature = "yaml-reference")]
    #[test]
    fn the_variable_names_a_parser() {
        assert_eq!(parser_named(None), Parser::Ayaml);
        assert_eq!(parser_named(Some("")), Parser::Ayaml);
        assert_eq!(parser_named(Some("ayaml")), Parser::Ayaml);
        assert_eq!(parser_named(Some("libyaml")), Parser::Libyaml);
        assert_eq!(parser_named(Some("compare")), Parser::Compare);
        // A misspelled setting must not fall back to ayaml unnoticed.
        for other in ["libYAML", "saphyr", " compare", "yaml_serde", "0"] {
            let payload = std::panic::catch_unwind(|| parser_named(Some(other))).unwrap_err();
            let message = payload.downcast_ref::<String>().unwrap();
            assert!(
                message.contains("names no parser: use ayaml, libyaml or compare"),
                "{message}"
            );
        }
    }

    #[cfg(feature = "yaml-reference")]
    #[test]
    fn libyaml_reads_what_both_read_alike_as_ayaml_does() {
        let input = "a: [1, x]\nb: {c: ~, d: 'q'}\n";
        let ayaml: crate::yaml::Value = crate::yaml::ayaml_from_str(input).unwrap();
        let libyaml: crate::yaml::Value = libyaml_from_str(input).unwrap();
        assert_eq!(ayaml, libyaml);
        assert_eq!(
            ayaml_json_tree(input).unwrap(),
            libyaml_json_tree(input).unwrap()
        );
        // And its own reading where the two differ: NEL, which YAML 1.2 made
        // content, is a line break to libyaml, which makes this plain scalar
        // an implicit key without a `:` (`yaml-1.2-line-breaks`).
        let nel = "a: x\u{85}y\n";
        let ayaml: crate::yaml::Value = crate::yaml::ayaml_from_str(nel).unwrap();
        assert_eq!(ayaml["a"].as_str(), Some("x\u{85}y"));
        assert!(libyaml_from_str::<crate::yaml::Value>(nel).is_err());
    }

    /// Run `f` with a full decode through libyaml forbidden on this thread.
    #[cfg(feature = "yaml-reference")]
    fn forbidding_libyaml<R>(f: impl FnOnce() -> R) -> R {
        struct Allow;
        impl Drop for Allow {
            fn drop(&mut self) {
                LIBYAML_FORBIDDEN.with(|forbidden| forbidden.set(false));
            }
        }
        LIBYAML_FORBIDDEN.with(|forbidden| forbidden.set(true));
        let _allow = Allow;
        f()
    }

    /// Inputs past a limit, each with what libyaml mode's error says. The
    /// first is the alias bomb of alint-dsl's
    /// `bom_prefixed_config_bombs_are_rejected_before_they_expand`: ayaml
    /// refuses it for its syntax (the byte order mark takes a column, so the
    /// quote on line 2 never closes), while libyaml reads line 1 as a mapping
    /// whose 50,000 aliases each replay a 1,000-entry sequence, which took 3.6
    /// GB and minutes in compare mode. ayaml refuses the others for its alias
    /// budget, its depth and its input cap.
    #[cfg(feature = "yaml-reference")]
    fn past_a_limit() -> Vec<(String, &'static str)> {
        let entries = "1,".repeat(1000);
        let aliases = "*a,".repeat(50_000);
        vec![
            (
                format!(
                    "\u{feff}--- 'k: {{a: &a [{entries}], b: [{aliases}], \
                     c: \"{{{{env.HOME}}}}\"}}\n'\n"
                ),
                "alias expansion exceeds",
            ),
            (
                format!("a: &a [{entries}]\nb: [{aliases}]\n"),
                "alias expansion exceeds",
            ),
            (
                format!("a: {}1{}\n", "[".repeat(5000), "]".repeat(5000)),
                "flow nesting exceeds",
            ),
            (
                "#".repeat(crate::MAX_STRUCTURED_BYTES + 1),
                "longer than the 33554432 bytes",
            ),
        ]
    }

    /// Neither compare mode nor libyaml mode lets libyaml decode an input past
    /// a limit: a full decode panics here, before libyaml expands anything.
    #[cfg(feature = "yaml-reference")]
    #[test]
    fn no_input_past_a_limit_reaches_libyaml() {
        for (input, refusal) in past_a_limit() {
            forbidding_libyaml(|| {
                // Compare mode returns ayaml's error, and compares nothing.
                assert!(compare::from_str::<crate::yaml::Value>(&input).is_err());
                assert!(compare::json_tree(&input).is_err());
                // libyaml mode refuses it, as ayaml's limits would.
                let error = libyaml_from_str::<crate::yaml::Value>(&input).unwrap_err();
                assert!(error.message().contains(refusal), "{error}");
                let error = libyaml_json_tree(&input).unwrap_err();
                assert!(error.message().contains(refusal), "{error}");
            });
        }
        // Within the limits, both read as before.
        let input = "a: &a [1, 2]\nb: [*a, *a]\n";
        let value: crate::yaml::Value = libyaml_from_str(input).unwrap();
        assert_eq!(
            value["b"].as_sequence().map(crate::yaml::Sequence::len),
            Some(2)
        );
        assert!(libyaml_json_tree(input).is_ok());
        assert!(libyaml_limits(input).is_ok());
    }

    #[cfg(feature = "yaml-reference")]
    #[test]
    fn a_libyaml_error_reads_as_an_ayaml_one() {
        let error = libyaml_from_str::<crate::yaml::Value>("a: [1\nb: 2\n").unwrap_err();
        let location = error.location().unwrap();
        assert_eq!((location.line(), location.column()), (2, 2));
        // The message leaves out the places that libyaml's Display adds,
        // which Display keeps.
        assert!(!error.message().contains(" at line "), "{error}");
        assert!(error.to_string().contains(" at line "), "{error}");
        assert_eq!(without_marks(&error.to_string()), error.message());
        let payload =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| error.into_parser()))
                .unwrap_err();
        let message = payload.downcast_ref::<String>().unwrap();
        assert!(
            message.starts_with("a libyaml error, not ayaml's"),
            "{message}"
        );
    }
}
