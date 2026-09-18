//! The structured bridge for Phase-2 `set_value` / `remove_value`: map a
//! concrete `JSONPath` location (a [`PathSeg`] list) to a byte range in the
//! original source, and serialize a scalar value as format-correct bytes
//! (auto-fix.md 5.3 / 5.4). The located fixer (in `alint-rules`) runs the
//! `JSONPath` re-query, hands us the resulting concrete path, and splices our
//! range with our bytes; the engine then re-parses and re-runs the query
//! ([`EditVerifier::Structured`](crate::rule::EditVerifier)) before committing.
//!
//! HCL (`hcl::edit`), XML (`roxmltree` node/attribute ranges), and dotenv + INI
//! (a hand-rolled re-scan of the raw text, since those parsers keep no spans) are
//! span-capable today. Every other [`Format`] returns `None`, so its
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
        Format::Ini => ini::ini_value_span(text, path),
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
        Format::Ini => ini::ini_removal_span(text, path),
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
        Format::Ini => ini::ini_serialize_scalar(value),
        Format::Toml => toml_::serialize_scalar(value),
        _ => None,
    }
}

/// Whether `format`'s fixer rewrites the WHOLE document via a round-trip CST
/// (TOML / `toml_edit`) instead of splicing a value/removal span. Such a format
/// emits a single [`FixEdit::ReplaceRange`](crate::rule::FixEdit) over the entire
/// file; `toml_edit` round-trips byte-identically, so only the mutated node
/// actually changes and the diff stays surgical.
#[must_use]
pub fn uses_document_rewrite(format: Format) -> bool {
    matches!(format, Format::Toml)
}

/// Whole-document `set_value` for a [`uses_document_rewrite`] format: set the
/// scalar at `path` (decor-preserving) and return the FULL new document bytes.
/// `None` when the format has no rewriter, the source does not parse, the path
/// does not resolve to a scalar, or the value is not representable. Never panics.
#[must_use]
pub fn document_set(
    format: Format,
    bytes: &[u8],
    path: &[PathSeg],
    want: &serde_json::Value,
) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(bytes).ok()?;
    match format {
        Format::Toml => toml_::document_set(text, path, want),
        _ => None,
    }
}

/// Whole-document `remove_value` for a [`uses_document_rewrite`] format: remove
/// every node at `paths` in ONE rewrite and return the FULL new document bytes.
/// `None` when the format has no rewriter, the source does not parse, or NOTHING
/// could be removed (so the fixer declines rather than emit a no-op edit).
#[must_use]
pub fn document_remove(format: Format, bytes: &[u8], paths: &[Vec<PathSeg>]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(bytes).ok()?;
    match format {
        Format::Toml => toml_::document_remove(text, paths),
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

/// INI / `.cfg` span resolution + value serialization.
///
/// Like dotenv, the [`crate::ini`] parser keeps no byte offsets, so this
/// re-scans the raw text, mirroring the parser (BOM strip, `str::lines()`,
/// leading `[section]` scope, earliest `=`/`:` split, `trim`ed literal value,
/// full-line `;`/`#` comments, configparser indentation-continuation). A path is
/// a GLOBAL key `[Key(k)]` or a SECTION key `[Key(section), Key(key)]`; anything
/// deeper or indexed (an array element of a duplicated key) declines.
///
/// `remove_value` handles a multi-line CONTINUATION value: it deletes the key's
/// line through its LAST continuation line (sweeping the transparent blank/comment
/// lines interleaved between continuations, which must go too or a deeper line is
/// orphaned), but never a trailing blank after the last continuation nor a
/// following key. `set_value`, by contrast, declines a multi-line value
/// (collapsing several value lines into one scalar is a different, deferred edit)
/// -- it resolves only a single-line value token.
///
/// It DECLINES (degrade to a Suggestion, never a wrong edit): a duplicate key
/// (collapsed to an array), a whole section (`$['sec']` names the object), and any
/// indexed / array-element path. Array-element edits are deferred.
mod ini {
    use super::PathSeg;
    use std::ops::Range;

    /// A located assignment of the target key.
    struct Located {
        /// The removal span: the key's own line for a single-line value, EXTENDED
        /// through the last continuation line for a multi-line value. Interspersed
        /// transparent blank/comment lines fall inside it (a later continuation
        /// extends past them); trailing blanks after the last continuation do not
        /// (nothing extends onto them), so `remove_value` cannot orphan a deeper
        /// line nor sweep a following key.
        full: Range<usize>,
        /// The value token on the key's own line -- `Some` ONLY for a single-line
        /// value. `None` once the value continues onto further lines, so
        /// `set_value` declines a multi-line value (overwriting it with one scalar
        /// is a different, deferred edit).
        value: Option<Range<usize>>,
    }

    pub(super) fn ini_value_span(text: &str, path: &[PathSeg]) -> Option<Range<usize>> {
        let (section, key) = target(path)?;
        locate(text, section, key).and_then(|l| l.value)
    }

    pub(super) fn ini_removal_span(text: &str, path: &[PathSeg]) -> Option<Range<usize>> {
        let (section, key) = target(path)?;
        locate(text, section, key).map(|l| l.full)
    }

    /// A resolvable INI path: a GLOBAL key (one segment) or a SECTION key (two).
    /// An `Index` (an array element of a duplicated key) or a deeper path is not
    /// resolved.
    fn target(path: &[PathSeg]) -> Option<(Option<&str>, &str)> {
        match path {
            [PathSeg::Key(k)] => Some((None, k.as_str())),
            [PathSeg::Key(s), PathSeg::Key(k)] => Some((Some(s.as_str()), k.as_str())),
            _ => None,
        }
    }

    /// Scan for the UNIQUE assignment of `key` in scope `section` (`None` = the
    /// global pre-section scope), mirroring [`crate::ini::parse`] line for line.
    /// `None` if it is absent OR appears more than once (a duplicate array --
    /// decline). A single match resolves; its `full` span covers any continuation
    /// lines (so `remove_value` deletes the whole multi-line value), while its
    /// `value` is `None` for a multi-line value (so `set_value` declines it).
    fn locate(text: &str, want_section: Option<&str>, want_key: &str) -> Option<Located> {
        let (body, base) = match text.strip_prefix('\u{feff}') {
            Some(rest) => (rest, '\u{feff}'.len_utf8()),
            None => (text, 0),
        };
        let mut matches: Vec<Located> = Vec::new();
        let mut section: Option<String> = None;
        // The current key's indent, and its index in `matches` when it is the
        // target (so a following continuation line extends its removal span and
        // marks it multi-line). `None` after a section header or a separator-less
        // line resets the current key.
        let mut cur: Option<(usize, Option<usize>)> = None;
        let mut cursor = 0usize;
        for chunk in body.split_inclusive('\n') {
            let line_start = base + cursor;
            let line_end = line_start + chunk.len();
            cursor += chunk.len();
            let content = chunk
                .strip_suffix('\n')
                .map_or(chunk, |c| c.strip_suffix('\r').unwrap_or(c));
            let indent = content
                .chars()
                .take_while(|&c| c == ' ' || c == '\t')
                .count();
            let line = content.trim();
            // Blank / full-line comment: transparent -- does not reset the key AND
            // does not itself extend a removal span (only a real continuation line
            // does, which sweeps any such lines that precede it).
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            // Continuation (checked BEFORE the section header, as the parser does):
            // a deeper-indented line is value text, not a section or key.
            if let Some((cur_indent, match_idx)) = cur {
                if indent > cur_indent {
                    if let Some(idx) = match_idx {
                        matches[idx].value = None; // multi-line -> set_value declines
                        matches[idx].full.end = line_end; // extend removal through here
                    }
                    continue; // `cur` unchanged: even deeper lines still continue
                }
            }
            // Section header: `[` .. LAST `]`.
            if let Some(rest) = line.strip_prefix('[') {
                if let Some(name) = rest.strip_suffix(']') {
                    section = Some(name.trim().to_string());
                }
                cur = None;
                continue;
            }
            // `key = value` / `key : value`: earliest separator wins.
            let Some(sep) = line.find(['=', ':']) else {
                cur = None; // separator-less line (a parse error upstream); reset
                continue;
            };
            let key = line[..sep].trim();
            let is_match = section.as_deref() == want_section && key == want_key;
            if is_match {
                // Byte ranges (absolute). `line` is `content` trimmed; its offset
                // within content is the leading-whitespace width.
                let lead = content.len() - content.trim_start().len();
                let raw_val = &line[sep + 1..];
                let vlead = raw_val.len() - raw_val.trim_start().len();
                let vtok = raw_val.trim();
                let vstart = line_start + lead + sep + 1 + vlead;
                matches.push(Located {
                    full: line_start..line_end,
                    value: Some(vstart..vstart + vtok.len()),
                });
                cur = Some((indent, Some(matches.len() - 1)));
            } else {
                cur = Some((indent, None));
            }
        }
        // Exactly one occurrence (a duplicate is an array -> decline).
        if matches.len() == 1 {
            matches.into_iter().next()
        } else {
            None
        }
    }

    /// Serialize a scalar as a literal INI value (quotes / `;` / `#` / `=` / `:`
    /// are all written verbatim -- INI has NO escaping). Declines a value that
    /// cannot round-trip or would corrupt the file:
    /// - EDGE whitespace -- the parser trims it, so it would not read back;
    /// - a CONTROL char other than a tab -- a newline would need a continuation,
    ///   and every other control (NUL / ESC / BEL / VT / FF ...) would be written
    ///   RAW (no escaping), a portability + terminal-escape-injection hazard (the
    ///   `equals` value can come from an untrusted `extends:`'d ruleset). This
    ///   matches the dotenv serializer's stance and routes through the shared
    ///   `set_value_is_statically_applicable` predicate, so `check` and `fix`
    ///   agree. A tab is benign single-line whitespace (interior tabs survive;
    ///   an edge tab is already caught above), so it is allowed.
    pub(super) fn ini_serialize_scalar(value: &serde_json::Value) -> Option<Vec<u8>> {
        use serde_json::Value;
        let s = match value {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Null | Value::Array(_) | Value::Object(_) => return None,
        };
        if s != s.trim() || s.chars().any(|c| c.is_control() && c != '\t') {
            return None;
        }
        Some(s.into_bytes())
    }
}

/// TOML structured edits via the `toml_edit` round-trip CST.
///
/// Unlike the hand-rolled resolvers, `toml_edit`'s `DocumentMut` DESPANS on parse
/// (its `.span()` all return `None`), so a value/removal-span splice is not
/// possible. Instead this EDITS the parsed document (a decor-preserving `set`, a
/// `remove` for a key) and re-serializes the WHOLE file: `toml_edit` round-trips
/// byte-identically and rewrites ONLY the mutated node, so the result is as
/// surgical as a splice while the library owns all formatting / comment / removal
/// surgery. The fixer emits ONE `ReplaceRange` over the entire file (gated by
/// `uses_document_rewrite`), not a span splice.
///
/// TOML is a TYPED format (`format_leaves_are_strings` is false for it), so a
/// numeric / bool / datetime `equals` is fixable, unlike the string-leaf formats.
/// Only KEY navigation is resolved; an `Index` (array-element) path declines, and
/// a non-scalar target (a table / array) is declined by the caller (`is_scalar`).
mod toml_ {
    use super::PathSeg;
    use toml_edit::{DocumentMut, Item, Value};

    /// The TOML rendering of a scalar, or `None` for a value TOML cannot hold.
    /// The representability oracle for the fixer's static-applicability check;
    /// `document_set` converts the same way.
    pub(super) fn serialize_scalar(value: &serde_json::Value) -> Option<Vec<u8>> {
        Some(json_to_value(value)?.to_string().into_bytes())
    }

    /// Set the scalar at `path`, preserving the existing value's decor
    /// (surrounding whitespace + a trailing comment), and return the whole new
    /// document. `None` to decline: no parse, path unresolved or not an existing
    /// scalar value, or the value is unrepresentable.
    pub(super) fn document_set(
        text: &str,
        path: &[PathSeg],
        want: &serde_json::Value,
    ) -> Option<Vec<u8>> {
        let mut doc = text.parse::<DocumentMut>().ok()?;
        let new_val = json_to_value(want)?;
        let val = navigate_mut(doc.as_item_mut(), path)?.as_value_mut()?;
        let decor = val.decor().clone();
        *val = new_val;
        *val.decor_mut() = decor;
        Some(doc.to_string().into_bytes())
    }

    /// Remove every key in `paths` in one rewrite; the whole new document, or
    /// `None` if NONE could be removed (so the fixer declines rather than emit a
    /// no-op whole-file edit).
    pub(super) fn document_remove(text: &str, paths: &[Vec<PathSeg>]) -> Option<Vec<u8>> {
        let mut doc = text.parse::<DocumentMut>().ok()?;
        let mut removed_any = false;
        for path in paths {
            if remove_one(doc.as_item_mut(), path) {
                removed_any = true;
            }
        }
        removed_any.then(|| doc.to_string().into_bytes())
    }

    /// Navigate down `path` by KEY at each step. `None` on a missing key, a
    /// non-table step, or an `Index` (array-element paths are not resolved).
    fn navigate_mut<'a>(item: &'a mut Item, path: &[PathSeg]) -> Option<&'a mut Item> {
        let mut cur = item;
        for seg in path {
            let PathSeg::Key(k) = seg else { return None };
            cur = cur.as_table_like_mut()?.get_mut(k)?;
        }
        Some(cur)
    }

    /// Remove the final key of `path` from its parent table; whether a node went.
    /// Declines an empty path or an `Index` final segment.
    fn remove_one(root: &mut Item, path: &[PathSeg]) -> bool {
        let Some((PathSeg::Key(key), parents)) = path.split_last() else {
            return false;
        };
        navigate_mut(root, parents)
            .and_then(Item::as_table_like_mut)
            .and_then(|t| t.remove(key))
            .is_some()
    }

    /// Convert a JSON scalar to a TYPED `toml_edit::Value`. `None` for null (TOML
    /// has no null), a non-scalar, or a u64 above `i64::MAX` (TOML integers are
    /// i64-only) -- declining beats a lossy float.
    fn json_to_value(value: &serde_json::Value) -> Option<Value> {
        use serde_json::Value as J;
        Some(match value {
            J::String(s) => Value::from(s.as_str()),
            J::Bool(b) => Value::from(*b),
            J::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::from(i)
                } else if n.is_f64() {
                    Value::from(n.as_f64()?)
                } else {
                    return None;
                }
            }
            J::Null | J::Array(_) | J::Object(_) => return None,
        })
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
        // JSON has no structured-fix support yet (TOML, once also unsupported, is
        // now a document-rewrite format -- see the classification gate).
        assert!(resolve_value_span(Format::Json, b"{\"a\": 1}", &[key("a")]).is_none());
        assert!(serialize_scalar(Format::Json, &json!("x")).is_none());
        assert!(!uses_document_rewrite(Format::Json));
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
        // A new `Format` must be consciously classified into ONE of three
        // structured-fix regimes. The exhaustive match is compile-forced, so a new
        // variant can't silently decline forever; the runtime checks pin the
        // classification (a span resolver resolves + is not document-rewrite; a
        // document-rewrite format declines spans but serializes + rewrites; an
        // unsupported format declines everything).
        enum Regime {
            Span,
            Document,
            Unsupported,
        }
        for &f in Format::ALL {
            let regime = match f {
                Format::Hcl | Format::Xml | Format::Dotenv | Format::Ini => Regime::Span,
                Format::Toml => Regime::Document,
                Format::Json | Format::Yaml | Format::Properties => Regime::Unsupported,
            };
            match regime {
                Regime::Span => {
                    assert!(
                        !uses_document_rewrite(f),
                        "span-resolver {f:?} must not also be document-rewrite"
                    );
                }
                Regime::Document => {
                    assert!(uses_document_rewrite(f), "{f:?} must be document-rewrite");
                    // A document-rewrite format does NOT splice a span.
                    assert!(
                        resolve_value_span(f, b"", &[key("a")]).is_none(),
                        "document-rewrite {f:?} must decline span resolution"
                    );
                    assert!(
                        serialize_scalar(f, &json!("x")).is_some(),
                        "document-rewrite {f:?} must serialize a representable scalar"
                    );
                }
                Regime::Unsupported => {
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
                    assert!(
                        !uses_document_rewrite(f),
                        "unsupported {f:?} must not be document-rewrite"
                    );
                }
            }
        }
        // A span format actually resolves; the document format actually rewrites.
        assert!(resolve_value_span(Format::Hcl, b"a = 1\n", &[key("a")]).is_some());
        assert!(resolve_value_span(Format::Dotenv, b"A=1\n", &[key("A")]).is_some());
        assert!(resolve_value_span(Format::Ini, b"a = 1\n", &[key("a")]).is_some());
        assert!(document_set(Format::Toml, b"a = 1\n", &[key("a")], &json!(2)).is_some());
        assert!(document_remove(Format::Toml, b"a = 1\n", &[vec![key("a")]]).is_some());
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

    #[test]
    fn dotenv_value_span_is_byte_accurate_with_multibyte_neighbors() {
        // A multibyte key AND value: the span must land on exact byte boundaries
        // so the splice touches only the value (café=☕, with a plain sibling).
        let src = "café=☕\nPORT=1\n";
        let span = resolve_value_span(Format::Dotenv, src.as_bytes(), &[key("café")]).unwrap();
        assert_eq!(&src[span], "☕");
    }

    #[test]
    fn dotenv_a_commented_occurrence_does_not_trigger_the_duplicate_decline() {
        // A `#`-comment mentioning the key is skipped by the scan, so a SINGLE
        // real assignment still resolves -- no false "ambiguous duplicate" decline.
        let src = "# DBURL=commented-out\nDBURL=real\n";
        let removal =
            resolve_removal_span(Format::Dotenv, src.as_bytes(), &[key("DBURL")]).unwrap();
        assert_eq!(&src[removal], "DBURL=real\n");
        assert!(resolve_value_span(Format::Dotenv, src.as_bytes(), &[key("DBURL")]).is_some());
    }

    // ---- INI (hand-rolled re-scan; 2-level sections, continuation-aware) ----

    fn ini_value(src: &str, path: &[PathSeg]) -> std::ops::Range<usize> {
        resolve_value_span(Format::Ini, src.as_bytes(), path).unwrap()
    }

    #[test]
    fn ini_value_span_covers_global_and_section_keys() {
        let src = "root = true\n[server]\nport = 8080\n";
        assert_eq!(&src[ini_value(src, &[key("root")])], "true");
        assert_eq!(&src[ini_value(src, &[key("server"), key("port")])], "8080");
    }

    #[test]
    fn ini_removal_span_is_the_whole_key_line() {
        let src = "[s]\nkeep = 1\ndrop = 2\n";
        let span =
            resolve_removal_span(Format::Ini, src.as_bytes(), &[key("s"), key("drop")]).unwrap();
        assert_eq!(&src[span], "drop = 2\n");
    }

    #[test]
    fn ini_colon_separator_keeps_the_literal_value_including_inline_comment() {
        // The earliest `=`/`:` splits; an inline `;` is literal VALUE text (only a
        // full-line `;`/`#` is a comment), so the whole token is the value.
        let src = "[db]\nurl : http://h:5432 ; note\n";
        assert_eq!(
            &src[ini_value(src, &[key("db"), key("url")])],
            "http://h:5432 ; note"
        );
    }

    #[test]
    fn ini_set_declines_a_multiline_value_but_remove_takes_the_whole_block() {
        // `deps`'s value spans continuation lines. `set_value` DECLINES (collapsing
        // several value lines into one scalar is a deferred edit); `remove_value`
        // deletes the key line THROUGH the last continuation line.
        let src = "[tox]\ndeps =\n    pytest\n    mock\nother = 1\n";
        assert!(
            resolve_value_span(Format::Ini, src.as_bytes(), &[key("tox"), key("deps")]).is_none()
        );
        let span =
            resolve_removal_span(Format::Ini, src.as_bytes(), &[key("tox"), key("deps")]).unwrap();
        assert_eq!(&src[span], "deps =\n    pytest\n    mock\n");
        // a single-line sibling in the same section still resolves for set.
        assert!(
            resolve_value_span(Format::Ini, src.as_bytes(), &[key("tox"), key("other")]).is_some()
        );
    }

    #[test]
    fn ini_multiline_removal_sweeps_interspersed_blanks_and_comments_only() {
        // A blank and a comment BETWEEN continuations are inside the value block and
        // MUST go (else the deeper `mock` line is orphaned); a trailing blank AFTER
        // the last continuation is NOT swept, and the following key is untouched.
        let src = "[tox]\ndeps =\n    pytest\n\n# note\n    mock\n\nother = 1\n";
        let span =
            resolve_removal_span(Format::Ini, src.as_bytes(), &[key("tox"), key("deps")]).unwrap();
        assert_eq!(
            &src[span.clone()],
            "deps =\n    pytest\n\n# note\n    mock\n"
        );
        // What survives re-parses cleanly (no orphaned continuation).
        let out = format!("{}{}", &src[..span.start], &src[span.end..]);
        assert_eq!(out, "[tox]\n\nother = 1\n");
    }

    #[test]
    fn ini_multiline_removal_runs_to_eof_without_a_trailing_newline() {
        let src = "[tox]\ndeps =\n    pytest\n    mock";
        let span =
            resolve_removal_span(Format::Ini, src.as_bytes(), &[key("tox"), key("deps")]).unwrap();
        assert_eq!(&src[span.clone()], "deps =\n    pytest\n    mock");
        assert_eq!(&src[..span.start], "[tox]\n");
    }

    #[test]
    fn ini_declines_a_duplicate_key_even_across_repeated_sections() {
        let same = "[s]\nk = 1\nk = 2\n";
        assert!(
            resolve_removal_span(Format::Ini, same.as_bytes(), &[key("s"), key("k")]).is_none()
        );
        let split = "[s]\nk = 1\n[o]\nx = 0\n[s]\nk = 2\n";
        assert!(
            resolve_removal_span(Format::Ini, split.as_bytes(), &[key("s"), key("k")]).is_none()
        );
    }

    #[test]
    fn ini_isolates_a_global_key_from_a_same_named_section_key() {
        let src = "name = global\n[sec]\nname = sectioned\n";
        assert_eq!(&src[ini_value(src, &[key("name")])], "global");
        assert_eq!(
            &src[ini_value(src, &[key("sec"), key("name")])],
            "sectioned"
        );
    }

    #[test]
    fn ini_resolves_a_key_under_a_repeated_section_header() {
        let src = "[s]\na = 1\n[o]\nx = 9\n[s]\nb = old\n";
        assert_eq!(&src[ini_value(src, &[key("s"), key("b")])], "old");
    }

    #[test]
    fn ini_declines_a_whole_section_and_an_indexed_path() {
        let src = "[drop]\nk = 1\n";
        // `$['drop']` names the section OBJECT -- removal declines (deferred).
        assert!(resolve_removal_span(Format::Ini, src.as_bytes(), &[key("drop")]).is_none());
        // an array-element (`Index`) path declines.
        assert!(
            resolve_value_span(
                Format::Ini,
                src.as_bytes(),
                &[key("drop"), key("k"), idx(0)]
            )
            .is_none()
        );
    }

    #[test]
    fn ini_value_span_is_offset_past_a_bom() {
        let src = "\u{feff}[s]\nk = v\n";
        assert_eq!(&src[ini_value(src, &[key("s"), key("k")])], "v");
    }

    #[test]
    fn ini_serialize_is_literal_and_declines_unrepresentable_values() {
        // Literal: quotes / `;` / `:` kept verbatim, no escaping.
        assert_eq!(
            serialize_scalar(Format::Ini, &json!("http://h ; x")).unwrap(),
            b"http://h ; x"
        );
        assert_eq!(serialize_scalar(Format::Ini, &json!("")).unwrap(), b"");
        // Edge whitespace (the parser trims it) and newlines (which would need a
        // continuation) cannot round-trip -> decline.
        assert!(serialize_scalar(Format::Ini, &json!(" pad ")).is_none());
        assert!(serialize_scalar(Format::Ini, &json!("a\nb")).is_none());
        assert!(serialize_scalar(Format::Ini, &json!("a\rb")).is_none());
        // Any other control char would be written RAW (no escaping) -> decline
        // (NUL / ESC / vertical tab), matching dotenv; an interior tab is allowed.
        assert!(serialize_scalar(Format::Ini, &json!("a\u{0}b")).is_none());
        assert!(serialize_scalar(Format::Ini, &json!("a\u{1b}b")).is_none());
        assert!(serialize_scalar(Format::Ini, &json!("a\u{0b}b")).is_none());
        assert_eq!(
            serialize_scalar(Format::Ini, &json!("a\tb")).unwrap(),
            b"a\tb"
        );
    }

    // ---- TOML (toml_edit whole-document rewrite; TYPED) ----

    fn toml_set(src: &str, path: &[PathSeg], want: &serde_json::Value) -> String {
        String::from_utf8(document_set(Format::Toml, src.as_bytes(), path, want).unwrap()).unwrap()
    }

    fn toml_remove(src: &str, paths: &[Vec<PathSeg>]) -> String {
        String::from_utf8(document_remove(Format::Toml, src.as_bytes(), paths).unwrap()).unwrap()
    }

    #[test]
    fn toml_set_preserves_decor_and_is_typed() {
        let src = "[server]\nport = 8080  # keep\nname = \"old\"\n";
        // A numeric value renders bare (TOML is typed), and the trailing comment +
        // alignment whitespace (the value's decor) survive.
        assert_eq!(
            toml_set(src, &[key("server"), key("port")], &json!(9090)),
            "[server]\nport = 9090  # keep\nname = \"old\"\n"
        );
        assert_eq!(
            toml_set(src, &[key("server"), key("name")], &json!("new")),
            "[server]\nport = 8080  # keep\nname = \"new\"\n"
        );
        // A float stays a float; a bool renders bare.
        assert_eq!(toml_set("x = 1\n", &[key("x")], &json!(1.5)), "x = 1.5\n");
        assert_eq!(
            toml_set("x = true\n", &[key("x")], &json!(false)),
            "x = false\n"
        );
    }

    #[test]
    fn toml_set_navigates_nested_and_declines_non_scalar_missing_or_null() {
        assert_eq!(
            toml_set("[a.b]\nc = 1\n", &[key("a"), key("b"), key("c")], &json!(2)),
            "[a.b]\nc = 2\n"
        );
        let src = "[a.b]\nc = 1\n";
        // A table target is not a scalar value -> decline.
        assert!(document_set(Format::Toml, src.as_bytes(), &[key("a")], &json!("x")).is_none());
        // A missing key -> decline.
        assert!(
            document_set(
                Format::Toml,
                src.as_bytes(),
                &[key("a"), key("b"), key("z")],
                &json!(1)
            )
            .is_none()
        );
        // Null is unrepresentable in TOML -> decline.
        assert!(
            document_set(
                Format::Toml,
                src.as_bytes(),
                &[key("a"), key("b"), key("c")],
                &json!(null)
            )
            .is_none()
        );
    }

    #[test]
    fn toml_remove_deletes_key_table_and_inline_member() {
        // A scalar key: the line goes.
        assert_eq!(
            toml_remove("[p]\nkeep = 1\ndrop = 2\n", &[vec![key("p"), key("drop")]]),
            "[p]\nkeep = 1\n"
        );
        // A whole table -- toml_edit removes it cleanly (safe, unlike the
        // hand-rolled INI/XML section/root cases, which had to decline).
        assert_eq!(
            toml_remove("[keep]\nx = 1\n[drop]\ny = 2\n", &[vec![key("drop")]]),
            "[keep]\nx = 1\n"
        );
        // An inline-table member.
        assert_eq!(
            toml_remove("x = { a = 1, b = 2 }\n", &[vec![key("x"), key("a")]]),
            "x = { b = 2 }\n"
        );
    }

    #[test]
    fn toml_remove_returns_none_when_nothing_removed() {
        // A missing key removes nothing -> None (the fixer declines, never a
        // whole-file no-op edit).
        assert!(document_remove(Format::Toml, b"a = 1\n", &[vec![key("z")]]).is_none());
        // An array-element (`Index`) path is not resolved.
        assert!(
            document_remove(Format::Toml, b"a = [1, 2]\n", &[vec![key("a"), idx(0)]]).is_none()
        );
    }

    #[test]
    fn toml_serialize_scalar_is_typed_and_declines_null_and_nonscalar() {
        assert_eq!(
            serialize_scalar(Format::Toml, &json!(8080)).unwrap(),
            b"8080"
        );
        assert_eq!(
            serialize_scalar(Format::Toml, &json!(true)).unwrap(),
            b"true"
        );
        assert_eq!(
            serialize_scalar(Format::Toml, &json!("x")).unwrap(),
            b"\"x\""
        );
        assert!(serialize_scalar(Format::Toml, &json!(null)).is_none());
        assert!(serialize_scalar(Format::Toml, &json!({"k": 1})).is_none());
    }
}
