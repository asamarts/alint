//! The structured bridge for Phase-2 `set_value` / `remove_value`: map a
//! concrete `JSONPath` location (a [`PathSeg`] list) to a byte range in the
//! original source, and serialize a scalar value as format-correct bytes
//! (auto-fix.md 5.3 / 5.4). The located fixer (in `alint-rules`) runs the
//! `JSONPath` re-query, hands us the resulting concrete path, and splices our
//! range with our bytes; the engine then re-parses and re-runs the query
//! ([`EditVerifier::Structured`](crate::rule::EditVerifier)) before committing.
//!
//! HCL (`hcl::edit`), XML (`roxmltree` node/attribute ranges), and dotenv (a
//! hand-rolled re-scan of the raw text, since the `.env` parser keeps no spans)
//! are span-capable today. Every other [`Format`] returns `None`, so its
//! `set_value` / `remove_value` declines cleanly (the violation is reported
//! unfixed) until that format's resolver lands. This is a SAFE degradation: a
//! `None` never corrupts bytes, and a wrong-but-parseable splice is still caught
//! by the engine's post-edit re-verify.
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
        Format::Dotenv => dotenv::dotenv_value_span(text, path),
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
        Format::Dotenv => dotenv::dotenv_removal_span(text, path),
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
        Format::Dotenv => dotenv::dotenv_serialize_scalar(value),
        _ => None,
    }
}

/// Whether `format`'s parse maps every leaf value to a STRING (XML, dotenv,
/// properties, INI -- none carry native numbers/booleans). On such a format a
/// non-string `equals` (`equals: 8080`) can never match the parsed string, so
/// the `*_path_equals` rule is unsatisfiable and `set_value` can never help. The
/// fixer's `can_fix` consults this so `check` does not promise an auto-fix that
/// `fix` will always decline (the `mark_fixability` / fixable-accuracy contract).
#[must_use]
pub fn format_leaves_are_strings(format: Format) -> bool {
    matches!(
        format,
        Format::Xml | Format::Dotenv | Format::Properties | Format::Ini
    )
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
    /// whitespace, or whitespace then a trailing comment -- i.e. the attribute
    /// owns the rest of its line. A `}` (single-line block close) or any other
    /// token returns `false`. A `/* .. */` block comment counts ONLY when it
    /// closes on this same line (the tail ends with `*/`); a multi-line block
    /// comment's tail would not end with `*/`, so removal stays conservative
    /// (attribute-span only) and never splits the comment.
    fn line_tail_is_blank_or_comment(after: &str) -> bool {
        let tail = after.trim_matches(char::is_whitespace);
        tail.is_empty()
            || tail.starts_with('#')
            || tail.starts_with("//")
            || (tail.starts_with("/*") && tail.ends_with("*/"))
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
        /// attribute (`name="value"`). `None` when the target cannot be removed in
        /// place -- the document ROOT element, where deleting it would empty the
        /// file to malformed XML that the `Absent` re-verify cannot catch (an
        /// empty document parses as `{}`).
        removal_span: Option<Range<usize>>,
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
        let removal = target.removal_span?;
        if target.is_attribute {
            // ` name="value"` -> also drop the single leading space separating it
            // from the previous attribute or the tag name.
            let start = removal.start;
            let trim_space = start > 0 && text.as_bytes().get(start - 1) == Some(&b' ');
            Some((if trim_space { start - 1 } else { start })..removal.end)
        } else {
            Some(element_line_span(text, removal))
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
            // The path ends at this element: set -> its text; remove -> the
            // element, UNLESS it is the document ROOT (its parent is not an
            // element). Deleting the root would empty the file to malformed XML,
            // which the Absent re-verify cannot catch (an empty doc parses as
            // `{}`), so decline removal there (-> Suggestion).
            let removable = elem.parent().is_some_and(|p| p.is_element());
            return Some(XmlTarget {
                value_span: text_span(elem),
                removal_span: removable.then(|| elem.range()),
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
                    removal_span: Some(span),
                    is_attribute: false,
                })
            }
            // `@name` -> an attribute; it cannot be navigated past.
            PathSeg::Key(k) if k.starts_with('@') => {
                if !rest.is_empty() {
                    return None;
                }
                // Match the LOCAL name (the parse flattens namespaces). If more
                // than one attribute shares the local name (`a:b` + `d:b` in
                // different namespaces), the parse's last-wins key and a
                // first-match here would disagree -> decline (R-CSTMAP).
                let name = &k[1..];
                let mut matching = elem.attributes().filter(|a| a.name() == name);
                let attr = matching.next()?;
                if matching.next().is_some() {
                    return None;
                }
                Some(XmlTarget {
                    value_span: Some(attr.range_value()),
                    removal_span: Some(attr.range()),
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
        let full = first.range();
        let raw = first.text()?;
        // The parse TRIMS a leaf's text, so replace only the trimmed content and
        // preserve surrounding whitespace. This offset math is valid ONLY when the
        // node's byte range maps 1:1 to its decoded text (`full.len() ==
        // raw.len()`): with entities (`a &amp; b`) or a CDATA section the range is
        // longer than the decoded text, so fall back to replacing the whole node
        // (padding not preserved -- a CDATA section becomes plain text -- but the
        // value is correct and the result valid). A whitespace-only leaf trims to
        // empty -> decline, like an empty element.
        if full.len() == raw.len() {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            let lead = raw.len() - raw.trim_start().len();
            return Some(full.start + lead..full.start + lead + trimmed.len());
        }
        if raw.trim().is_empty() {
            return None;
        }
        Some(full)
    }

    /// Serialize a scalar as XML character data, entity-escaping the SUPERSET
    /// `& < > " '` -- valid in BOTH element text and single- OR double-quoted
    /// attribute values, so no edit-context threading is needed -- with `\t`/`\n`/
    /// `\r` and other control chars as NUMERIC char refs (exempt from XML
    /// attribute-value normalization, so they round-trip). NOTE: a numeric/bool
    /// value never round-trips on a STRING-typed format (XML/dotenv/properties/INI
    /// parse every leaf as a string, so `equals: 8080` there is unsatisfiable --
    /// the fixer's `can_fix` reports it unfixable rather than promising a no-op).
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
                '\'' => out.push_str("&#39;"),
                // Whitespace controls as NUMERIC char refs, not raw: XML attribute-
                // value normalization turns a raw tab/newline into a space (and CR
                // into a newline), so a raw one would not round-trip through the
                // re-verify; a char ref is exempt from normalization.
                '\t' => out.push_str("&#x9;"),
                '\n' => out.push_str("&#xA;"),
                '\r' => out.push_str("&#xD;"),
                c if c.is_control() => {
                    let _ = write!(out, "&#x{:X};", c as u32);
                }
                other => out.push(other),
            }
        }
        out
    }
}

/// dotenv (`.env`) span resolution + value serialization.
///
/// The [`crate::dotenv`] parser flattens a file to `{ KEY: "value" }` of strings
/// and keeps NO byte offsets, so this re-scans the raw text to locate a key's
/// value / line (the "hand-rolled" resolver, vs HCL/XML's spanned CSTs). dotenv
/// is FLAT -- a resolved path is a SINGLE `Key` -- and each key owns exactly one
/// physical line, so `remove_value` is a whole-line delete with NONE of the
/// nested-structure over-deletion risk the HCL/XML resolvers must guard. A key
/// that appears MORE THAN ONCE (the parser's last-wins duplicate) collapses to
/// one JSON key but many lines, which a single contiguous range cannot express,
/// so the resolver DECLINES it (degrade to a Suggestion) rather than edit an
/// ambiguous occurrence.
mod dotenv {
    use super::PathSeg;
    use std::ops::Range;

    /// A located key line.
    struct Located {
        /// The whole physical line INCLUDING its trailing newline (what
        /// `remove_value` deletes).
        full: Range<usize>,
        /// The value region `set_value` overwrites.
        value: Range<usize>,
    }

    /// The byte range of the value at `path` (what `set_value` overwrites).
    pub(super) fn dotenv_value_span(text: &str, path: &[PathSeg]) -> Option<Range<usize>> {
        locate(text, single_key(path)?).map(|l| l.value)
    }

    /// The whole-line removal span at `path` (what `remove_value` deletes).
    pub(super) fn dotenv_removal_span(text: &str, path: &[PathSeg]) -> Option<Range<usize>> {
        locate(text, single_key(path)?).map(|l| l.full)
    }

    /// A dotenv path is FLAT: exactly one `Key` segment, never nested or indexed.
    fn single_key(path: &[PathSeg]) -> Option<&str> {
        match path {
            [PathSeg::Key(k)] => Some(k.as_str()),
            _ => None,
        }
    }

    /// Scan the raw text for the UNIQUE assignment line whose key is `name`,
    /// mirroring [`crate::dotenv::parse`]'s key extraction (BOM strip, `export `
    /// prefix, first `=`, trimmed key). `None` if the key is absent OR appears
    /// more than once (ambiguous last-wins duplicate -- decline).
    fn locate(text: &str, name: &str) -> Option<Located> {
        // The parser strips a leading BOM before splitting into lines; mirror
        // that and offset every span past it so ranges stay in ORIGINAL bytes.
        let (body, base) = match text.strip_prefix('\u{feff}') {
            Some(rest) => (rest, '\u{feff}'.len_utf8()),
            None => (text, 0),
        };
        let mut found: Option<Located> = None;
        let mut cursor = 0usize; // offset within `body`
        for chunk in body.split_inclusive('\n') {
            let line_start = base + cursor;
            let full = line_start..line_start + chunk.len();
            cursor += chunk.len();
            // `str::lines()` content = the chunk minus its `\n` and a preceding `\r`.
            let content = chunk
                .strip_suffix('\n')
                .map_or(chunk, |c| c.strip_suffix('\r').unwrap_or(c));
            let Some(vspan) = value_span_in_line(content, name) else {
                continue;
            };
            if found.is_some() {
                return None; // a second match -> ambiguous, decline
            }
            found = Some(Located {
                full,
                value: line_start + vspan.start..line_start + vspan.end,
            });
        }
        found
    }

    /// If `content` is an assignment line for key `name`, return the value region
    /// as a range WITHIN `content`; else `None`.
    fn value_span_in_line(content: &str, name: &str) -> Option<Range<usize>> {
        let trimmed = content.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return None;
        }
        let lead = content.len() - trimmed.len();
        // Optional `export ` prefix (only with a following space), then re-trim.
        let (after, after_off) = match trimmed.strip_prefix("export ") {
            Some(rest) => {
                let r = rest.trim_start();
                (r, "export ".len() + (rest.len() - r.len()))
            }
            None => (trimmed, 0),
        };
        let after_start = lead + after_off;
        let eq = after.find('=')?;
        if after[..eq].trim_end() != name {
            return None;
        }
        let value_raw_start = after_start + eq + 1;
        let value_raw = &content[value_raw_start..];
        let (vs, ve) = value_region(value_raw)?;
        Some(value_raw_start + vs..value_raw_start + ve)
    }

    /// The byte range (within `value_raw`, the text right after `=`) that
    /// `set_value` overwrites, mirroring [`crate::dotenv::parse_value`]'s view of
    /// the value: the whole quoted region for a quoted value, the trimmed token
    /// for an unquoted one, a zero-width point for an empty value. Replacing the
    /// WHOLE region (quotes included) lets the serializer re-decide the quoting
    /// for the new value; a trailing comment after a quoted value is preserved.
    fn value_region(value_raw: &str) -> Option<(usize, usize)> {
        let v = value_raw.trim_start();
        let vlead = value_raw.len() - v.len();
        if let Some(rest) = v.strip_prefix('\'') {
            // Single-quoted: literal to the next `'`. Region = the whole `'...'`.
            let end = rest.find('\'')?;
            Some((vlead, vlead + 1 + end + 1))
        } else if let Some(inner) = v.strip_prefix('"') {
            // Double-quoted: to the next UNESCAPED `"`. Region = the whole `"..."`.
            let mut it = inner.char_indices();
            let close = loop {
                match it.next() {
                    None => return None, // unterminated (declines; unreachable post-parse)
                    Some((_, '\\')) => {
                        it.next(); // skip the escaped char
                    }
                    Some((ci, '"')) => break ci,
                    Some(_) => {}
                }
            };
            Some((vlead, vlead + 1 + close + 1))
        } else {
            // Unquoted: an inline comment starts at ` #` (detected on the raw
            // value, so `KEY= # c` is an empty value + a comment); the value is
            // the trimmed token before it.
            let cut = value_raw.find(" #").unwrap_or(value_raw.len());
            let region = &value_raw[..cut];
            let token = region.trim();
            if token.is_empty() {
                Some((0, 0)) // empty value: a zero-width insert right after `=`
            } else {
                let start = region.len() - region.trim_start().len();
                Some((start, start + token.len()))
            }
        }
    }

    /// Serialize a scalar as a dotenv value: unquoted when it round-trips as-is,
    /// else double-quoted with the `\n \r \t \\ \"` escapes. Declines a value
    /// containing a control char dotenv cannot escape (only `\n \r \t` are
    /// representable), so `set_value` degrades to a Suggestion rather than emit a
    /// raw control byte.
    pub(super) fn dotenv_serialize_scalar(value: &serde_json::Value) -> Option<Vec<u8>> {
        use serde_json::Value;
        let s = match value {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            // Null has no natural dotenv literal; an object/array is not a scalar
            // (the caller already declines it) -- decline all three.
            Value::Null | Value::Array(_) | Value::Object(_) => return None,
        };
        if s.chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return None;
        }
        Some(if needs_double_quoting(&s) {
            let mut out = String::with_capacity(s.len() + 2);
            out.push('"');
            for c in s.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '"' => out.push_str("\\\""),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    other => out.push(other),
                }
            }
            out.push('"');
            out.into_bytes()
        } else {
            s.into_bytes()
        })
    }

    /// Whether `s` would NOT round-trip as a bare unquoted dotenv value (so it
    /// must be double-quoted): leading/trailing whitespace (the parser trims it),
    /// an inline-comment ` #`, a leading quote (would start a quoted value), or a
    /// newline / CR / tab.
    fn needs_double_quoting(s: &str) -> bool {
        s != s.trim()
            || s.contains(" #")
            || s.starts_with('\'')
            || s.starts_with('"')
            || s.contains('\n')
            || s.contains('\r')
            || s.contains('\t')
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
    fn hcl_removal_span_takes_a_same_line_block_comment_but_not_a_multiline_one() {
        // A `/* .. */` that closes on the attribute's line is removed with it.
        let src = "drop = 2 /* note */\nkeep = 1\n";
        let span = resolve_removal_span(Format::Hcl, src.as_bytes(), &[key("drop")]).unwrap();
        assert_eq!(&src[span], "drop = 2 /* note */\n");
        // A block comment that does NOT close on the line stays conservative
        // (attribute-span only) so widening can never split the comment.
        let src2 = "drop = 2 /* a\nb */\nkeep = 1\n";
        let span2 = resolve_removal_span(Format::Hcl, src2.as_bytes(), &[key("drop")]).unwrap();
        assert_eq!(&src2[span2], "drop = 2"); // just the attribute, comment intact
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
                Format::Hcl | Format::Xml | Format::Dotenv => true,
                Format::Json | Format::Yaml | Format::Toml | Format::Properties | Format::Ini => {
                    false
                }
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
        assert!(resolve_value_span(Format::Dotenv, b"A=1\n", &[key("A")]).is_some());
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
        // Apostrophe (for single-quoted attribute values) and the whitespace
        // controls as numeric char refs (so they survive XML normalization).
        assert_eq!(
            serialize_scalar(Format::Xml, &json!("a'b")).unwrap(),
            "a&#39;b".as_bytes()
        );
        assert_eq!(
            serialize_scalar(Format::Xml, &json!("a\tb\nc\rd")).unwrap(),
            "a&#x9;b&#xA;c&#xD;d".as_bytes()
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

    #[test]
    fn xml_removal_declines_the_document_root() {
        // Removing a path that resolves to the document ROOT would empty the file
        // to malformed XML that the Absent re-verify can't catch -> must decline.
        let src = "<project>\n  <version>1.0</version>\n</project>\n";
        assert!(
            resolve_removal_span(Format::Xml, src.as_bytes(), &[key("project")]).is_none(),
            "root-element removal must decline (would empty the file)"
        );
        // A non-root child is still removable.
        assert!(
            resolve_removal_span(
                Format::Xml,
                src.as_bytes(),
                &[key("project"), key("version")]
            )
            .is_some()
        );
    }

    #[test]
    fn xml_declines_a_namespaced_attribute_name_collision() {
        // Two attributes share the local name `b` (different namespaces). The
        // parse's last-wins key and a first-match resolve would disagree -> decline.
        let src = "<c xmlns:a=\"urn:a\" xmlns:d=\"urn:d\" a:b=\"AA\" d:b=\"DD\"/>";
        assert!(resolve_value_span(Format::Xml, src.as_bytes(), &[key("c"), key("@b")]).is_none());
    }

    #[test]
    fn xml_set_preserves_surrounding_whitespace() {
        // The parse trims a leaf's text; set must replace only the trimmed value,
        // keeping the padding (byte-locality).
        let src = "<v>  1.0  </v>";
        let span = resolve_value_span(Format::Xml, src.as_bytes(), &[key("v")]).unwrap();
        assert_eq!(&src[span], "1.0"); // the trimmed value, not "  1.0  "
    }

    // ---- dotenv (hand-rolled re-scan; flat KEY=value) ----

    fn denv_value(src: &str, k: &str) -> std::ops::Range<usize> {
        resolve_value_span(Format::Dotenv, src.as_bytes(), &[key(k)]).unwrap()
    }

    fn denv_removal(src: &str, k: &str) -> std::ops::Range<usize> {
        resolve_removal_span(Format::Dotenv, src.as_bytes(), &[key(k)]).unwrap()
    }

    /// Serialize `v`, splice it into `K=<here>\n`, re-parse, and return the value
    /// the parser reads back -- the round-trip invariant `set_value` relies on.
    fn denv_roundtrip(v: &serde_json::Value) -> serde_json::Value {
        let repl = serialize_scalar(Format::Dotenv, v).unwrap();
        let out = format!("K={}\n", std::str::from_utf8(&repl).unwrap());
        crate::dotenv::parse(&out).unwrap()["K"].clone()
    }

    #[test]
    fn dotenv_value_span_covers_unquoted_single_and_double_quoted() {
        let src = "A=plain\nB='lit'\nC=\"esc\\n\"\n";
        assert_eq!(&src[denv_value(src, "A")], "plain");
        assert_eq!(&src[denv_value(src, "B")], "'lit'"); // the WHOLE quoted region
        assert_eq!(&src[denv_value(src, "C")], "\"esc\\n\"");
    }

    #[test]
    fn dotenv_value_span_skips_export_and_keeps_the_inline_comment() {
        let src = "export PORT = 8080 # the port\n";
        assert_eq!(&src[denv_value(src, "PORT")], "8080"); // token only, comment kept
    }

    #[test]
    fn dotenv_value_span_for_an_empty_value_is_zero_width_after_equals() {
        let src = "EMPTY=\n";
        let span = denv_value(src, "EMPTY");
        assert_eq!(span.start, span.end); // a zero-width insertion point
        assert_eq!(&src[..span.start], "EMPTY=");
    }

    #[test]
    fn dotenv_value_span_is_offset_past_a_leading_bom() {
        let src = "\u{feff}A=hi\n";
        assert_eq!(&src[denv_value(src, "A")], "hi");
    }

    #[test]
    fn dotenv_set_can_drop_quotes_and_round_trips() {
        // `'old'` -> a simple value serializes bare, so the quotes go.
        let src = "A='old'\n";
        let span = denv_value(src, "A");
        let repl = serialize_scalar(Format::Dotenv, &json!("new")).unwrap();
        let out = format!(
            "{}{}{}",
            &src[..span.start],
            std::str::from_utf8(&repl).unwrap(),
            &src[span.end..]
        );
        assert_eq!(out, "A=new\n");
        assert_eq!(crate::dotenv::parse(&out).unwrap()["A"], json!("new"));
    }

    #[test]
    fn dotenv_removal_span_takes_the_whole_line_with_its_newline() {
        let src = "A=1\nDROP=2\nB=3\n";
        assert_eq!(&src[denv_removal(src, "DROP")], "DROP=2\n");
    }

    #[test]
    fn dotenv_removal_span_on_the_last_line_without_a_newline() {
        let src = "A=1\nDROP=2";
        let span = denv_removal(src, "DROP");
        assert_eq!(&src[span.clone()], "DROP=2");
        let out = format!("{}{}", &src[..span.start], &src[span.end..]);
        assert_eq!(out, "A=1\n"); // A=1 (and its newline) survive
    }

    #[test]
    fn dotenv_removal_span_handles_crlf_and_preserves_a_leading_bom() {
        let src = "\u{feff}A=1\r\nDROP=2\r\n";
        let span = denv_removal(src, "DROP");
        assert_eq!(&src[span.clone()], "DROP=2\r\n"); // the whole CRLF line
        let out = format!("{}{}", &src[..span.start], &src[span.end..]);
        assert_eq!(out, "\u{feff}A=1\r\n"); // BOM + first line intact
    }

    #[test]
    fn dotenv_declines_a_duplicate_key_for_both_set_and_remove() {
        // A last-wins duplicate collapses to one JSON key but two lines: one
        // contiguous range can't express it, so decline (degrade to Suggestion).
        let src = "K=1\nK=2\n";
        assert!(resolve_value_span(Format::Dotenv, src.as_bytes(), &[key("K")]).is_none());
        assert!(resolve_removal_span(Format::Dotenv, src.as_bytes(), &[key("K")]).is_none());
    }

    #[test]
    fn dotenv_declines_a_nested_or_indexed_path() {
        let src = "A=1\n";
        assert!(
            resolve_value_span(Format::Dotenv, src.as_bytes(), &[key("A"), key("B")]).is_none()
        );
        assert!(resolve_value_span(Format::Dotenv, src.as_bytes(), &[idx(0)]).is_none());
    }

    #[test]
    fn dotenv_serialize_quotes_only_when_needed() {
        let b = |v: &serde_json::Value| serialize_scalar(Format::Dotenv, v).unwrap();
        assert_eq!(b(&json!("plain")), b"plain"); // simple -> bare
        assert_eq!(b(&json!("a b")), b"a b"); // interior space -> bare
        assert_eq!(b(&json!("q\"x")), b"q\"x"); // interior quote -> bare (literal)
        assert_eq!(b(&json!(" pad ")), b"\" pad \""); // edge ws -> quote
        assert_eq!(b(&json!("a #c")), b"\"a #c\""); // ` #` comment marker -> quote
        assert_eq!(b(&json!("'x")), b"\"'x\""); // leading quote -> quote
        assert_eq!(b(&json!("l1\nl2")), b"\"l1\\nl2\""); // newline -> quote + escape
        assert_eq!(b(&json!("a\tb")), b"\"a\\tb\""); // tab -> quote + escape
    }

    #[test]
    fn dotenv_serialize_round_trips_tricky_values() {
        for v in [
            json!("plain"),
            json!("a b"),
            json!("q\"x"),     // interior quote, bare
            json!(" pad "),    // edge whitespace
            json!("a #c"),     // inline-comment marker
            json!("'quoted'"), // leading single quote
            json!("\"dq\""),   // leading double quote
            json!("back\\slash"),
            json!("l1\nl2\r\n"), // newlines + CR
            json!("tab\tsep"),
            json!(""), // empty
            json!("#leading-hash-no-space"),
        ] {
            assert_eq!(denv_roundtrip(&v), v, "value did not round-trip: {v:?}");
        }
    }

    #[test]
    fn dotenv_serialize_declines_an_unescapable_control_char() {
        assert!(serialize_scalar(Format::Dotenv, &json!("a\u{0}b")).is_none()); // NUL
        assert!(serialize_scalar(Format::Dotenv, &json!("a\u{1b}b")).is_none()); // ESC
    }
}
