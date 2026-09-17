//! The structured bridge for Phase-2 `set_value` / `remove_value`: map a
//! concrete `JSONPath` location (a [`PathSeg`] list) to a byte range in the
//! original source, and serialize a scalar value as format-correct bytes
//! (auto-fix.md 5.3 / 5.4). The located fixer (in `alint-rules`) runs the
//! `JSONPath` re-query, hands us the resulting concrete path, and splices our
//! range with our bytes; the engine then re-parses and re-runs the query
//! ([`EditVerifier::Structured`](crate::rule::EditVerifier)) before committing.
//!
//! HCL (`hcl::edit`) and XML (`roxmltree` node/attribute ranges) are span-capable
//! today. Every other [`Format`] returns `None`, so its `set_value` /
//! `remove_value` declines cleanly (the violation is reported unfixed) until that
//! format's resolver lands. This is a SAFE degradation: a `None` never corrupts
//! bytes, and a wrong-but-parseable splice is still caught by the engine's
//! post-edit re-verify.
//!
//! R-CSTMAP realism: the `JSONPath` is resolved over the detached, parsed
//! [`Value`](serde_json::Value); mapping its concrete path back to the CST node
//! is per-format. HCL navigates leaf attributes, unlabeled blocks, LABELED
//! blocks (the labels consume the matching path segments, so `resource "t" "n"`
//! is reached by `resource.t.n`), and OBJECT-valued attributes (`x = { a = 1 }`,
//! `x.a`). XML navigates child elements (a repeated same-name set is an array
//! reached by index, `item[1]`), `@attr` attributes, and leaf text. Where the
//! mapping is not 1:1 -- a repeated HCL block, an attribute/block key clash, a
//! quoted key, an XML mixed/empty element -- the resolver returns `None` rather
//! than guess, so an ambiguous edit degrades to a Suggestion. HCL object-member
//! removal + array indices, and XML mixed-content set, are not yet resolved.

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
        Format::Xml => xml::xml_value_span(text, path),
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
        Format::Xml => xml::xml_removal_span(text, path),
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
        Format::Xml => xml::xml_serialize_scalar(value),
        _ => None,
    }
}

/// HCL span resolution + value serialization over the `hcl::edit` CST.
mod hcl {
    use super::PathSeg;
    use hcl::edit::Span as _;
    use hcl::edit::expr::Expression;
    use hcl::edit::structure::{Attribute, Block, Body};
    use std::ops::Range;

    /// The located target of a resolved path.
    struct Target {
        /// The value's byte range (what `set_value` overwrites).
        value_span: Range<usize>,
        /// The full `key = value` attribute span, present ONLY when the target
        /// is a real leaf HCL attribute (not an object-member value).
        /// `remove_value` needs it plus the line context; it declines when this
        /// is `None` (an object member -- comma surgery is deferred).
        attr_span: Option<Range<usize>>,
    }

    /// Parse `text` into a spanned CST and return the VALUE span at `path`
    /// (what `set_value` overwrites).
    pub(super) fn hcl_value_span(text: &str, path: &[PathSeg]) -> Option<Range<usize>> {
        let body = hcl::edit::parser::parse_body(text).ok()?;
        resolve_in_body(&body, path).map(|t| t.value_span)
    }

    /// Resolve `path` within a block body to a single unambiguous target. A
    /// path segment names either an ATTRIBUTE (a leaf, or one whose value is an
    /// object we navigate into) or a BLOCK -- unlabeled, or LABELED, in which
    /// case its labels consume the next path segments (so `resource "t" "n"` is
    /// reached by `resource.t.n`). Requires EXACTLY ONE matching structure: an
    /// attribute+block key clash, or a repeated block/label, declines (`None`)
    /// so an ambiguous CST mapping degrades to a Suggestion, never a wrong
    /// splice (R-CSTMAP).
    fn resolve_in_body(body: &Body, path: &[PathSeg]) -> Option<Target> {
        let (seg, rest) = path.split_first()?;
        // Array indices (and any non-key step) are not resolved: an attribute
        // whose value is a list is left to a Suggestion.
        let PathSeg::Key(key) = seg else {
            return None;
        };
        let mut found: Option<Target> = None;
        let mut matches = 0usize;
        for structure in body {
            if let Some(attr) = structure.as_attribute() {
                if attr.key.as_str() == key {
                    matches += 1;
                    found = resolve_in_attribute(attr, rest);
                }
            } else if let Some(block) = structure.as_block() {
                if block.ident.as_str() == key && labels_match(block, rest) {
                    matches += 1;
                    found = resolve_block_body(block, &rest[block.labels.len()..]);
                }
            }
        }
        // Exactly one structure matched the key (and, for a block, its labels).
        // Two matches -- an attribute AND a block, or two blocks sharing
        // ident+labels -- is ambiguous: decline.
        if matches != 1 {
            return None;
        }
        found
    }

    /// Whether `block`'s labels match the leading `rest` path segments (so they
    /// can be consumed before recursing into the block body). An unlabeled block
    /// matches trivially.
    fn labels_match(block: &Block, rest: &[PathSeg]) -> bool {
        rest.len() >= block.labels.len()
            && block
                .labels
                .iter()
                .zip(rest)
                .all(|(label, seg)| matches!(seg, PathSeg::Key(k) if label.as_str() == k))
    }

    /// Resolve within a block's body after its labels are consumed. An empty
    /// `after` names the block itself, which has no scalar value -> decline.
    fn resolve_block_body(block: &Block, after: &[PathSeg]) -> Option<Target> {
        if after.is_empty() {
            return None;
        }
        resolve_in_body(&block.body, after)
    }

    /// Resolve within a leaf attribute. An empty `rest` names the attribute's
    /// own value; otherwise its value must be an object we navigate into (the
    /// result is an object MEMBER, so `attr_span` is `None` -- removal of a
    /// member is deferred, but `set_value` works).
    fn resolve_in_attribute(attr: &Attribute, rest: &[PathSeg]) -> Option<Target> {
        if rest.is_empty() {
            return Some(Target {
                value_span: attr.value.span()?,
                attr_span: attr.span(),
            });
        }
        Some(Target {
            value_span: object_value_span(&attr.value, rest)?,
            attr_span: None,
        })
    }

    /// Navigate an object `Expression` by `rest`, returning the target member's
    /// value span. IDENT keys only (`{ a = 1 }`); a quoted / expression key
    /// (`{ "a" = 1 }`) or an ambiguous match declines.
    fn object_value_span(expr: &Expression, rest: &[PathSeg]) -> Option<Range<usize>> {
        let object = expr.as_object()?;
        let (seg, sub) = rest.split_first()?;
        let PathSeg::Key(key) = seg else {
            return None;
        };
        let mut found = None;
        let mut matches = 0usize;
        for (k, v) in object {
            if k.as_ident().is_some_and(|i| i.as_str() == key.as_str()) {
                matches += 1;
                found = Some(v);
            }
        }
        if matches != 1 {
            return None;
        }
        let value = found?;
        if sub.is_empty() {
            value.expr().span()
        } else {
            object_value_span(value.expr(), sub)
        }
    }

    /// The byte range `remove_value` deletes for the leaf attribute `path`
    /// names. Removes the whole physical line (`key = value`, a trailing
    /// `# comment`, and the newline) when the attribute OWNS its line; when the
    /// attribute shares its line with a single-line block wrapper
    /// (`ident { key = value }`), removes ONLY the attribute span, leaving the
    /// wrapper (`ident {  }`) -- never engulfing the `{`/`}` (the single-line
    /// block over-deletion the audit found). Declines (`None`) for an object
    /// member (comma surgery deferred) or a block.
    pub(super) fn hcl_removal_span(text: &str, path: &[PathSeg]) -> Option<Range<usize>> {
        let body = hcl::edit::parser::parse_body(text).ok()?;
        let attr_span = resolve_in_body(&body, path)?.attr_span?;
        let bytes = text.as_bytes();
        let line_start = bytes[..attr_span.start]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1);
        let line_end = bytes[attr_span.end..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len(), |i| attr_span.end + i + 1);
        // The attribute OWNS its line iff only whitespace precedes it and only
        // whitespace (or a trailing `#` / `//` comment) follows it. A `{` before
        // or `}` after means a single-line block wraps it -- widening to the
        // line would delete the block, so remove just the attribute span.
        let before = &text[line_start..attr_span.start];
        let after = &text[attr_span.end..line_end];
        if before.chars().all(char::is_whitespace) && line_tail_is_blank_or_comment(after) {
            Some(line_start..line_end)
        } else {
            Some(attr_span)
        }
    }

    /// Whether the bytes from an attribute's end to the line end are only
    /// whitespace, or whitespace then a `#` / `//` line comment -- i.e. the
    /// attribute owns the rest of its line. A `}` (single-line block close) or
    /// any other token returns `false`.
    fn line_tail_is_blank_or_comment(after: &str) -> bool {
        let tail = after.trim_matches(char::is_whitespace);
        tail.is_empty() || tail.starts_with('#') || tail.starts_with("//")
    }

    /// Serialize a scalar `Value` as HCL bytes. Strings are quoted with HCL
    /// escapes (control chars as `\uXXXX`, the `${`/`%{` template markers
    /// neutralized); numbers, booleans, and null use their literal HCL forms.
    /// `None` for a non-scalar. NOTE (limitation): an integer-valued float
    /// `equals` (`2.0`) serializes to `2.0`, but `hcl-rs` re-parses that as the
    /// integer `2`, so the re-verify fails and the fix is (correctly) demoted --
    /// the same number-normalization that makes `hcl_path_equals` with
    /// `equals: 2.0` unsatisfiable in `check` too. Genuine floats (`1.5`) work.
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

    /// Escape a string for an HCL double-quoted literal: the usual
    /// `\`/`"`/`\n`/`\r`/`\t`, every OTHER control char as `\uXXXX` (so a raw
    /// NUL/ESC/DEL is never written into a text config -- the Go HCL parser
    /// Terraform uses rejects raw control bytes), and the HCL template markers
    /// `${` / `%{` neutralized (`$${` / `%%{`).
    fn hcl_escape(s: &str) -> String {
        use std::fmt::Write as _;
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
                c if c.is_control() => {
                    let _ = write!(out, "\\u{:04X}", c as u32);
                }
                other => out.push(other),
            }
        }
        out
    }
}

/// XML span resolution + value serialization over `roxmltree` node / attribute
/// byte ranges (the `positions` feature, on by default). `set_value` replaces a
/// leaf element's text or an attribute's value; `remove_value` deletes a whole
/// element or an attribute. Repeated same-name child elements (which the parse
/// maps to a JSON array) are reached by index (`items.item[0]`). Namespaces are
/// flattened to local names, matching `Format::parse`.
mod xml {
    use super::PathSeg;
    use roxmltree::{Document, Node};
    use std::ops::Range;

    /// A resolved XML target.
    struct XmlTarget {
        /// The value span `set_value` overwrites: a leaf element's text, or an
        /// attribute's value. `None` for an empty / mixed element (no single text
        /// span to replace; the caller declines a non-scalar anyway).
        value_span: Option<Range<usize>>,
        /// The node span `remove_value` deletes: the element (`<x>..</x>`) or the
        /// attribute (`name="value"`).
        removal_span: Range<usize>,
        /// Whether `removal_span` is an attribute (trim one leading space) versus
        /// an element (line-ownership widening).
        is_attribute: bool,
    }

    pub(super) fn xml_value_span(text: &str, path: &[PathSeg]) -> Option<Range<usize>> {
        let doc = Document::parse(text).ok()?;
        resolve(&doc, path)?.value_span
    }

    pub(super) fn xml_removal_span(text: &str, path: &[PathSeg]) -> Option<Range<usize>> {
        let doc = Document::parse(text).ok()?;
        let target = resolve(&doc, path)?;
        if target.is_attribute {
            // ` name="value"` -> also drop the single leading space separating it
            // from the previous attribute or the tag name.
            let start = target.removal_span.start;
            let trim_space = start > 0 && text.as_bytes().get(start - 1) == Some(&b' ');
            Some((if trim_space { start - 1 } else { start })..target.removal_span.end)
        } else {
            Some(element_line_span(text, target.removal_span))
        }
    }

    /// Widen an element's `<x>..</x>` span to its whole physical line when it
    /// OWNS the line (only whitespace before and after) -- otherwise remove just
    /// the element (it shares a line with a sibling or its parent's tags).
    fn element_line_span(text: &str, span: Range<usize>) -> Range<usize> {
        let bytes = text.as_bytes();
        let line_start = bytes[..span.start]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1);
        let line_end = bytes[span.end..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len(), |i| span.end + i + 1);
        let before = &text[line_start..span.start];
        let after = &text[span.end..line_end];
        if before.chars().all(char::is_whitespace)
            && after.trim_matches(char::is_whitespace).is_empty()
        {
            line_start..line_end
        } else {
            span
        }
    }

    /// Navigate the parsed doc by `path`; `path[0]` is the root element's tag.
    fn resolve(doc: &Document, path: &[PathSeg]) -> Option<XmlTarget> {
        let (seg, rest) = path.split_first()?;
        let PathSeg::Key(name) = seg else {
            return None;
        };
        let root = doc.root_element();
        if root.tag_name().name() != name {
            return None;
        }
        resolve_in_element(root, rest)
    }

    fn resolve_in_element(elem: Node, path: &[PathSeg]) -> Option<XmlTarget> {
        let Some((seg, rest)) = path.split_first() else {
            // The path ends at this element: set -> its text; remove -> the element.
            return Some(XmlTarget {
                value_span: text_span(elem),
                removal_span: elem.range(),
                is_attribute: false,
            });
        };
        match seg {
            // The mixed-element text pseudo-key: `set` replaces it. (Removing
            // `#text` is not meaningful, but a removal span is filled for totality.)
            PathSeg::Key(k) if k == "#text" => {
                if !rest.is_empty() {
                    return None;
                }
                let span = text_span(elem)?;
                Some(XmlTarget {
                    value_span: Some(span.clone()),
                    removal_span: span,
                    is_attribute: false,
                })
            }
            // `@name` -> an attribute; it cannot be navigated past.
            PathSeg::Key(k) if k.starts_with('@') => {
                if !rest.is_empty() {
                    return None;
                }
                let attr = elem.attributes().find(|a| a.name() == &k[1..])?;
                Some(XmlTarget {
                    value_span: Some(attr.range_value()),
                    removal_span: attr.range(),
                    is_attribute: true,
                })
            }
            // A child element by tag name. A single child is navigated directly; a
            // repeated same-name set (an array) needs the next segment as its index.
            PathSeg::Key(k) => {
                let children: Vec<Node> = elem
                    .children()
                    .filter(Node::is_element)
                    .filter(|c| c.tag_name().name() == k)
                    .collect();
                match children.as_slice() {
                    [] => None,
                    [only] => resolve_in_element(*only, rest),
                    many => {
                        let (idx, sub) = rest.split_first()?;
                        let PathSeg::Index(i) = idx else {
                            return None;
                        };
                        resolve_in_element(*many.get(*i)?, sub)
                    }
                }
            }
            PathSeg::Index(_) => None,
        }
    }

    /// The byte range of an element's text content (its single text child).
    /// `None` for an empty element, or one whose text is split by child elements
    /// (mixed content -- a partial text span is ambiguous, so decline).
    fn text_span(elem: Node) -> Option<Range<usize>> {
        if elem.children().any(|c| c.is_element()) {
            return None; // mixed content
        }
        let mut texts = elem.children().filter(Node::is_text);
        let first = texts.next()?;
        if texts.next().is_some() {
            return None; // multiple text nodes
        }
        Some(first.range())
    }

    /// Serialize a scalar as XML character data, entity-escaping the SUPERSET
    /// `& < > "` -- valid in BOTH element text and double-quoted attribute values,
    /// so no context threading is needed -- plus control chars as `&#xN;`. (A `"`
    /// in element text becomes `&quot;`: valid, if slightly noisy.)
    pub(super) fn xml_serialize_scalar(value: &serde_json::Value) -> Option<Vec<u8>> {
        use serde_json::Value;
        let raw = match value {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Null => String::new(),
            Value::Array(_) | Value::Object(_) => return None,
        };
        Some(xml_escape(&raw).into_bytes())
    }

    fn xml_escape(s: &str) -> String {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\t' | '\n' | '\r' => out.push(c),
                c if c.is_control() => {
                    let _ = write!(out, "&#x{:X};", c as u32);
                }
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

    // ---- F4: labeled blocks (the dominant Terraform shape) ----

    #[test]
    fn hcl_value_span_navigates_a_labeled_block() {
        let src = "resource \"aws_instance\" \"web\" {\n  ami = \"ami-1\"\n}\n";
        let span = resolve_value_span(
            Format::Hcl,
            src.as_bytes(),
            &[key("resource"), key("aws_instance"), key("web"), key("ami")],
        )
        .unwrap();
        assert_eq!(&src[span], "\"ami-1\"");
    }

    #[test]
    fn hcl_value_span_resolves_the_right_labeled_block_by_its_labels() {
        // Two `resource "aws_instance"` blocks -> serde nests them by label; the
        // path's labels must select `web`, not `db`.
        let src = "resource \"aws_instance\" \"web\" {\n  ami = \"WEB\"\n}\n\
                   resource \"aws_instance\" \"db\" {\n  ami = \"DB\"\n}\n";
        let span = resolve_value_span(
            Format::Hcl,
            src.as_bytes(),
            &[key("resource"), key("aws_instance"), key("web"), key("ami")],
        )
        .unwrap();
        assert_eq!(&src[span], "\"WEB\"");
    }

    #[test]
    fn hcl_value_span_declines_identical_labeled_blocks() {
        // Two blocks sharing ident AND labels are ambiguous -> decline.
        let src = "resource \"t\" \"n\" {\n  a = 1\n}\nresource \"t\" \"n\" {\n  a = 2\n}\n";
        assert!(
            resolve_value_span(
                Format::Hcl,
                src.as_bytes(),
                &[key("resource"), key("t"), key("n"), key("a")],
            )
            .is_none()
        );
    }

    // ---- F5: object-valued attributes ----

    #[test]
    fn hcl_value_span_navigates_an_object_attribute() {
        let src = "locals {\n  tags = { Team = \"core\" }\n}\n";
        let span = resolve_value_span(
            Format::Hcl,
            src.as_bytes(),
            &[key("locals"), key("tags"), key("Team")],
        )
        .unwrap();
        assert_eq!(&src[span], "\"core\"");
    }

    #[test]
    fn hcl_value_span_declines_a_quoted_object_key() {
        // A quoted/expression object key (`{ "Team" = ... }`) is not an ident
        // key, so the resolver declines rather than guess.
        let src = "tags = { \"Team\" = \"core\" }\n";
        assert!(
            resolve_value_span(Format::Hcl, src.as_bytes(), &[key("tags"), key("Team")],).is_none()
        );
    }

    // ---- A1: single-line block removal must not engulf the block ----

    #[test]
    fn hcl_removal_span_keeps_a_single_line_block_wrapper() {
        // Removing `enabled` from `settings { enabled = true }` must delete ONLY
        // the attribute, leaving the `settings {  }` wrapper -- NOT the whole
        // block (the audit's verifier-invisible over-deletion).
        let src = "settings { enabled = true }\nkeep = 1\n";
        let span = resolve_removal_span(
            Format::Hcl,
            src.as_bytes(),
            &[key("settings"), key("enabled")],
        )
        .unwrap();
        assert_eq!(&src[span.clone()], "enabled = true");
        let mut out = src.to_string();
        out.replace_range(span, "");
        assert_eq!(out, "settings {  }\nkeep = 1\n");
    }

    #[test]
    fn hcl_removal_span_takes_the_whole_line_with_a_trailing_comment() {
        // An attribute that owns its line is removed whole, including a trailing
        // comment that annotates it.
        let src = "drop = 2 # the note\nkeep = 1\n";
        let span = resolve_removal_span(Format::Hcl, src.as_bytes(), &[key("drop")]).unwrap();
        assert_eq!(&src[span.clone()], "drop = 2 # the note\n");
        let mut out = src.to_string();
        out.replace_range(span, "");
        assert_eq!(out, "keep = 1\n");
    }

    #[test]
    fn hcl_removal_span_declines_an_object_member() {
        // Removing an object MEMBER (comma/separator surgery) is deferred.
        let src = "tags = { Team = \"core\" }\n";
        assert!(
            resolve_removal_span(Format::Hcl, src.as_bytes(), &[key("tags"), key("Team")],)
                .is_none()
        );
    }

    // ---- A2: control-char escaping ----

    #[test]
    fn hcl_serialize_escapes_control_chars() {
        // A raw NUL / ESC / DEL must never be written into a text config; they
        // are emitted as `\uXXXX`.
        assert_eq!(
            serialize_scalar(Format::Hcl, &json!("a\u{0}b")).unwrap(),
            "\"a\\u0000b\"".as_bytes()
        );
        assert_eq!(
            serialize_scalar(Format::Hcl, &json!("x\u{1b}y")).unwrap(),
            "\"x\\u001By\"".as_bytes()
        );
        assert_eq!(
            serialize_scalar(Format::Hcl, &json!("z\u{7f}")).unwrap(),
            "\"z\\u007F\"".as_bytes()
        );
    }

    #[test]
    fn every_format_is_classified_for_structured_fix() {
        // A new `Format` must be consciously classified as having a structured-fix
        // resolver or explicitly not-yet-supported. The exhaustive match is
        // compile-forced, so a new variant can't silently decline forever; the
        // runtime checks pin that an unsupported format actually declines all
        // three entry points.
        for &f in Format::ALL {
            let has_resolver = match f {
                Format::Hcl | Format::Xml => true,
                Format::Json
                | Format::Yaml
                | Format::Toml
                | Format::Dotenv
                | Format::Properties
                | Format::Ini => false,
            };
            if !has_resolver {
                assert!(
                    resolve_value_span(f, b"", &[key("a")]).is_none(),
                    "unsupported {f:?} must decline set_value span resolution"
                );
                assert!(
                    resolve_removal_span(f, b"", &[key("a")]).is_none(),
                    "unsupported {f:?} must decline remove_value span resolution"
                );
                assert!(
                    serialize_scalar(f, &json!("x")).is_none(),
                    "unsupported {f:?} must decline serialization"
                );
            }
        }
        // A supported format actually resolves.
        assert!(resolve_value_span(Format::Hcl, b"a = 1\n", &[key("a")]).is_some());
    }

    // ---- XML (roxmltree spans): elements, attributes, text, siblings ----

    fn idx(i: usize) -> PathSeg {
        PathSeg::Index(i)
    }

    #[test]
    fn xml_value_span_resolves_element_text_and_attribute() {
        let src = "<config version=\"1.0\"><name>old</name></config>";
        let text =
            resolve_value_span(Format::Xml, src.as_bytes(), &[key("config"), key("name")]).unwrap();
        assert_eq!(&src[text], "old");
        let attr = resolve_value_span(
            Format::Xml,
            src.as_bytes(),
            &[key("config"), key("@version")],
        )
        .unwrap();
        assert_eq!(&src[attr], "1.0"); // the value INSIDE the quotes
    }

    #[test]
    fn xml_value_span_resolves_a_sibling_by_index() {
        // R-CSTMAP: repeated same-name children map to a JSON array; `item[1]`
        // must select the SECOND element's text, not the first.
        let src = "<items><item>a</item><item>b</item></items>";
        let span = resolve_value_span(
            Format::Xml,
            src.as_bytes(),
            &[key("items"), key("item"), idx(1)],
        )
        .unwrap();
        assert_eq!(&src[span], "b");
    }

    #[test]
    fn xml_value_span_declines_mixed_and_empty() {
        // Mixed content (text split by a child element) and an empty element have
        // no single text span -> decline.
        assert!(resolve_value_span(Format::Xml, b"<a>text<b/>more</a>", &[key("a")]).is_none());
        assert!(resolve_value_span(Format::Xml, b"<a></a>", &[key("a")]).is_none());
    }

    #[test]
    fn xml_removal_span_removes_element_and_attribute() {
        // An element on its own line -> the whole line.
        let src = "<c>\n  <drop>x</drop>\n  <keep>1</keep>\n</c>\n";
        let span =
            resolve_removal_span(Format::Xml, src.as_bytes(), &[key("c"), key("drop")]).unwrap();
        let mut out = src.to_string();
        out.replace_range(span, "");
        assert_eq!(out, "<c>\n  <keep>1</keep>\n</c>\n");
        // An inline element -> just the element (siblings on the line survive).
        let src2 = "<c><drop>x</drop><keep>1</keep></c>";
        let span2 =
            resolve_removal_span(Format::Xml, src2.as_bytes(), &[key("c"), key("drop")]).unwrap();
        assert_eq!(&src2[span2], "<drop>x</drop>");
        // An attribute -> the attribute plus its leading space.
        let src3 = "<c drop=\"x\" keep=\"1\"/>";
        let span3 =
            resolve_removal_span(Format::Xml, src3.as_bytes(), &[key("c"), key("@drop")]).unwrap();
        assert_eq!(&src3[span3], " drop=\"x\"");
    }

    #[test]
    fn xml_serialize_escapes_entities_and_controls() {
        assert_eq!(
            serialize_scalar(Format::Xml, &json!("a & b < c > d \" e")).unwrap(),
            "a &amp; b &lt; c &gt; d &quot; e".as_bytes()
        );
        assert_eq!(
            serialize_scalar(Format::Xml, &json!("x\u{0}y")).unwrap(),
            "x&#x0;y".as_bytes()
        );
        assert_eq!(
            serialize_scalar(Format::Xml, &json!(8080)).unwrap(),
            b"8080"
        );
    }
}
