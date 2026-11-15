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
//! ayaml unnoticed. Without the feature, the variable is not read.
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

fn ayaml_json_tree(input: &str) -> Result<serde_json::Value, Error> {
    crate::yaml::ayaml_from_str_seed(input, crate::structured_format::json_seed(input))
        .map_err(Error::ayaml)
}

/// [`from_str`] through libyaml.
#[cfg(feature = "yaml-reference")]
pub(crate) fn libyaml_from_str<'de, T: Deserialize<'de>>(input: &'de str) -> Result<T, Error> {
    yaml_serde::from_str(input).map_err(Error::libyaml)
}

/// [`json_tree`] through libyaml.
#[cfg(feature = "yaml-reference")]
fn libyaml_json_tree(input: &str) -> Result<serde_json::Value, Error> {
    use serde::de::DeserializeSeed as _;
    crate::structured_format::json_seed(input)
        .deserialize(yaml_serde::Deserializer::from_str(input))
        .map_err(Error::libyaml)
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
