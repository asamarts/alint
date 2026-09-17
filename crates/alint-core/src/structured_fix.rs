//! The structured bridge for Phase-2 `set_value` / `remove_value`: map a
//! concrete `JSONPath` location (a [`PathSeg`] list) to a byte range in the
//! original source, and serialize a scalar value as format-correct bytes
//! (auto-fix.md 5.3 / 5.4). The located fixer (in `alint-rules`) runs the
//! `JSONPath` re-query, hands us the resulting concrete path, and splices our
//! range with our bytes; the engine then re-parses and re-runs the query
//! ([`EditVerifier::Structured`](crate::rule::EditVerifier)) before committing.
//!
//! Only HCL is span-capable today (`hcl::edit`, a re-export of `hcl-edit`).
//! Every other [`Format`] returns `None`, so its `set_value` / `remove_value`
//! declines cleanly (the violation is reported unfixed) until that format's
//! resolver lands. This is a SAFE degradation: a `None` never corrupts bytes.
//!
//! R-CSTMAP realism: the `JSONPath` is resolved over the detached, parsed
//! [`Value`](serde_json::Value); mapping its concrete path back to the CST node
//! can be ambiguous (repeated / labeled HCL blocks map to arrays / nested
//! objects the same key would reach). Where the mapping is not 1:1 the resolver
//! returns `None` rather than guess, so an ambiguous edit degrades to a
//! Suggestion instead of splicing the wrong span.

use crate::structured_format::Format;
use std::ops::Range;

/// One concrete step of a resolved `JSONPath` location: an object key or an
/// array index. The `alint-rules` fixer converts a
/// `serde_json_path::NormalizedPath` into a `Vec<PathSeg>` so this crate need
/// not depend on the `JSONPath` library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSeg {
    /// An object member reached by name.
    Key(String),
    /// An array element reached by index.
    Index(usize),
}

/// The byte range of the *value* at `path` in `bytes` parsed as `format` -- the
/// span `set_value` overwrites. `None` when the format has no span resolver
/// yet, the path does not resolve to a single unambiguous node, or the source
/// does not parse. Never panics.
#[must_use]
pub fn resolve_value_span(format: Format, bytes: &[u8], path: &[PathSeg]) -> Option<Range<usize>> {
    let text = std::str::from_utf8(bytes).ok()?;
    match format {
        Format::Hcl => hcl::hcl_value_span(text, path),
        _ => None,
    }
}

/// The byte range `remove_value` deletes for the node at `path` -- the node and
/// its format-specific separator (for HCL, the whole `key = value` line
/// including its trailing newline). `None` under the same conditions as
/// [`resolve_value_span`].
#[must_use]
pub fn resolve_removal_span(
    format: Format,
    bytes: &[u8],
    path: &[PathSeg],
) -> Option<Range<usize>> {
    let text = std::str::from_utf8(bytes).ok()?;
    match format {
        Format::Hcl => hcl::hcl_removal_span(text, path),
        _ => None,
    }
}

/// Render `value` (a scalar: string, number, bool, or null) as `format`-correct
/// bytes to splice in place of an existing scalar. `None` for a non-scalar
/// (object / array -- routed to Suggestion by the caller) or a format without a
/// serializer yet.
#[must_use]
pub fn serialize_scalar(format: Format, value: &serde_json::Value) -> Option<Vec<u8>> {
    match format {
        Format::Hcl => hcl::hcl_serialize_scalar(value),
        _ => None,
    }
}

/// HCL span resolution + value serialization over the `hcl::edit` CST.
mod hcl {
    use super::PathSeg;
    use hcl::edit::Span as _;
    use hcl::edit::structure::Body;
    use std::ops::Range;

    /// Parse `text` into a spanned CST and return the value span at `path`.
    pub(super) fn hcl_value_span(text: &str, path: &[PathSeg]) -> Option<Range<usize>> {
        let body = hcl::edit::parser::parse_body(text).ok()?;
        value_span_in(&body, path)
    }

    /// The value span of the attribute the `path` names, navigating unlabeled
    /// blocks. Returns `None` on any ambiguity (a key matching more than one
    /// structure -- repeated blocks, an attribute-and-block clash) so an
    /// unclear mapping degrades to a Suggestion rather than a wrong splice.
    fn value_span_in(body: &Body, path: &[PathSeg]) -> Option<Range<usize>> {
        let (seg, rest) = path.split_first()?;
        // Array indices (and any non-key step) are not resolved in this first
        // HCL cut: an attribute whose value is a list/object is left to a
        // Suggestion. Keys reach attributes (leaves) and unlabeled blocks.
        let PathSeg::Key(key) = seg else {
            return None;
        };
        let mut leaf_span: Option<Range<usize>> = None;
        let mut sub_body: Option<&Body> = None;
        let mut matches = 0usize;
        for structure in body {
            if let Some(attr) = structure.as_attribute() {
                if attr.key.as_str() == key {
                    leaf_span = attr.value.span();
                    matches += 1;
                }
            } else if let Some(block) = structure.as_block() {
                // Only UNLABELED single blocks map 1:1 to an object key. A
                // labeled block (`resource "t" "n" {}`) or a repeat introduces
                // an array/label layer the flat key does not capture -> decline.
                if block.labels.is_empty() && block.ident.as_str() == key {
                    sub_body = Some(&block.body);
                    matches += 1;
                }
            }
        }
        if matches != 1 {
            return None; // absent or ambiguous
        }
        if rest.is_empty() {
            leaf_span // a leaf attribute's value
        } else {
            sub_body.and_then(|b| value_span_in(b, rest))
        }
    }

    /// The removal span for the node `path` names: the whole source line of the
    /// target attribute, including its trailing newline, so `key = value\n`
    /// vanishes cleanly. `None` on ambiguity or if the target is not a leaf
    /// attribute (removing a whole block is left to a Suggestion here).
    pub(super) fn hcl_removal_span(text: &str, path: &[PathSeg]) -> Option<Range<usize>> {
        let body = hcl::edit::parser::parse_body(text).ok()?;
        let value_span = value_span_in(&body, path)?;
        // Widen from the value span to the whole physical line: back to the
        // start of the line (after the preceding newline) and forward past the
        // trailing newline. The byte-locality golden asserts nothing outside
        // this line changes.
        let bytes = text.as_bytes();
        let line_start = bytes[..value_span.start]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1);
        let line_end = bytes[value_span.end..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len(), |i| value_span.end + i + 1);
        Some(line_start..line_end)
    }

    /// Serialize a scalar `Value` as HCL bytes. Strings are quoted with HCL
    /// escapes (including `${`/`%{` template-interpolation guards); numbers,
    /// booleans, and null use their literal HCL forms. `None` for a non-scalar.
    pub(super) fn hcl_serialize_scalar(value: &serde_json::Value) -> Option<Vec<u8>> {
        use serde_json::Value;
        let out = match value {
            Value::String(s) => format!("\"{}\"", hcl_escape(s)),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Null => "null".to_string(),
            Value::Array(_) | Value::Object(_) => return None,
        };
        Some(out.into_bytes())
    }

    /// Escape a string for an HCL double-quoted literal. Beyond the usual
    /// `\`/`"`/control escapes, the HCL template markers `${` and `%{` are
    /// neutralized (`$${` / `%%{`) so a value containing them is not
    /// reinterpreted as an interpolation/directive.
    fn hcl_escape(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '$' if chars.peek() == Some(&'{') => out.push_str("$$"),
                '%' if chars.peek() == Some(&'{') => out.push_str("%%"),
                other => out.push(other),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key(k: &str) -> PathSeg {
        PathSeg::Key(k.to_string())
    }

    #[test]
    fn hcl_value_span_finds_a_top_level_attribute() {
        let src = "region = \"us-east-1\"\n";
        let span = resolve_value_span(Format::Hcl, src.as_bytes(), &[key("region")]).unwrap();
        assert_eq!(&src[span], "\"us-east-1\"");
    }

    #[test]
    fn hcl_value_span_navigates_into_an_unlabeled_block() {
        let src = "terraform {\n  required_version = \">= 1.0\"\n}\n";
        let span = resolve_value_span(
            Format::Hcl,
            src.as_bytes(),
            &[key("terraform"), key("required_version")],
        )
        .unwrap();
        assert_eq!(&src[span], "\">= 1.0\"");
    }

    #[test]
    fn hcl_value_span_declines_a_repeated_block() {
        // Two `provider` blocks -> serde maps them to an array; the flat key
        // `provider` is ambiguous in the CST -> decline (Suggestion), never
        // splice one of them.
        let src = "provider {\n  a = 1\n}\nprovider {\n  a = 2\n}\n";
        assert!(
            resolve_value_span(Format::Hcl, src.as_bytes(), &[key("provider"), key("a")]).is_none()
        );
    }

    #[test]
    fn hcl_value_span_declines_a_labeled_block() {
        let src = "resource \"t\" \"n\" {\n  a = 1\n}\n";
        assert!(
            resolve_value_span(Format::Hcl, src.as_bytes(), &[key("resource"), key("a")]).is_none()
        );
    }

    #[test]
    fn hcl_removal_span_takes_the_whole_line() {
        let src = "keep = 1\ndrop = 2\nalso = 3\n";
        let span = resolve_removal_span(Format::Hcl, src.as_bytes(), &[key("drop")]).unwrap();
        assert_eq!(&src[span.clone()], "drop = 2\n");
        // Splicing it out leaves the neighbors untouched.
        let mut out = src.to_string();
        out.replace_range(span, "");
        assert_eq!(out, "keep = 1\nalso = 3\n");
    }

    #[test]
    fn hcl_serialize_scalar_covers_the_scalar_types() {
        assert_eq!(
            serialize_scalar(Format::Hcl, &json!("x")).unwrap(),
            b"\"x\""
        );
        assert_eq!(
            serialize_scalar(Format::Hcl, &json!(8080)).unwrap(),
            b"8080"
        );
        assert_eq!(
            serialize_scalar(Format::Hcl, &json!(true)).unwrap(),
            b"true"
        );
        assert_eq!(
            serialize_scalar(Format::Hcl, &json!(null)).unwrap(),
            b"null"
        );
        assert!(serialize_scalar(Format::Hcl, &json!({"a": 1})).is_none());
        assert!(serialize_scalar(Format::Hcl, &json!([1, 2])).is_none());
    }

    #[test]
    fn hcl_serialize_string_escapes_quotes_and_template_markers() {
        assert_eq!(
            serialize_scalar(Format::Hcl, &json!("a\"b")).unwrap(),
            b"\"a\\\"b\""
        );
        // `${` would otherwise start an interpolation.
        assert_eq!(
            serialize_scalar(Format::Hcl, &json!("${x}")).unwrap(),
            "\"$${x}\"".as_bytes()
        );
    }

    #[test]
    fn a_format_without_a_resolver_declines() {
        assert!(resolve_value_span(Format::Toml, b"a = 1\n", &[key("a")]).is_none());
        assert!(serialize_scalar(Format::Toml, &json!("x")).is_none());
    }
}
