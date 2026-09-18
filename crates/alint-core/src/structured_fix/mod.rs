//! The structured bridge for Phase-2 `set_value` / `remove_value`: map a
//! concrete `JSONPath` location (a [`PathSeg`] list) to a byte range in the
//! original source, and serialize a scalar value as format-correct bytes
//! (auto-fix.md 5.3 / 5.4). The located fixer (in `alint-rules`) runs the
//! `JSONPath` re-query, hands us the resulting concrete path, and splices our
//! range with our bytes; the engine then re-parses and re-runs the query
//! ([`EditVerifier::Structured`](crate::rule::EditVerifier)) before committing.
//!
//! HCL (`hcl::edit`), XML (`roxmltree` node/attribute ranges), and dotenv + INI +
//! properties (a hand-rolled re-scan of the raw text, since those parsers keep no
//! spans) are span-capable today; TOML is document-rewrite (`toml_edit`). Every
//! other [`Format`] returns `None`, so its
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
        Format::Properties => properties::properties_value_span(text, path),
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
        Format::Properties => properties::properties_removal_span(text, path),
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
        Format::Properties => properties::properties_serialize_scalar(value),
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

/// Java `.properties` span resolution + value serialization (CONSERVATIVE).
///
/// The parse uses `java-properties` (escape-aware; `=`/`:`/whitespace separators,
/// `\`-continuation, `\uXXXX`) and keeps no spans, so this re-scans the raw text.
/// It is DELIBERATELY conservative -- it resolves ONLY a simple single physical
/// line `key = value` / `key : value` with NO backslash (no escaped separator, no
/// continuation, no `\uXXXX`) and an EXPLICIT `=`/`:` separator, and DECLINES
/// everything else (a whitespace-only separator, a `\`-continuation or escaped
/// line, a duplicate key) so an ambiguous edit degrades to a Suggestion. Escaped
/// / continued / space-separated properties are deferred.
///
/// Properties is FLAT (a path is one `Key`), and each simple key owns its physical
/// line. The parser strips LEADING value whitespace but keeps TRAILING whitespace,
/// so the value span runs from after the separator to the END of the line.
mod properties {
    use super::PathSeg;
    use std::ops::Range;

    /// A located simple assignment line.
    struct Located {
        /// The whole physical line INCLUDING its trailing newline (removal).
        full: Range<usize>,
        /// The value span (after the separator, to end of line).
        value: Range<usize>,
    }

    pub(super) fn properties_value_span(text: &str, path: &[PathSeg]) -> Option<Range<usize>> {
        locate(text, single_key(path)?).map(|l| l.value)
    }

    pub(super) fn properties_removal_span(text: &str, path: &[PathSeg]) -> Option<Range<usize>> {
        locate(text, single_key(path)?).map(|l| l.full)
    }

    /// A properties path is FLAT: exactly one `Key` segment.
    fn single_key(path: &[PathSeg]) -> Option<&str> {
        match path {
            [PathSeg::Key(k)] => Some(k.as_str()),
            _ => None,
        }
    }

    /// Scan for the UNIQUE simple assignment line of `name`. `None` if it is
    /// absent, appears more than once, or is a complex (escaped / continued /
    /// space-separated) line the conservative resolver declines.
    fn locate(text: &str, name: &str) -> Option<Located> {
        let (body, base) = match text.strip_prefix('\u{feff}') {
            Some(rest) => (rest, '\u{feff}'.len_utf8()),
            None => (text, 0),
        };
        let mut found: Option<Located> = None;
        let mut cursor = 0usize;
        for chunk in body.split_inclusive('\n') {
            let line_start = base + cursor;
            let full = line_start..line_start + chunk.len();
            cursor += chunk.len();
            let content = chunk
                .strip_suffix('\n')
                .map_or(chunk, |c| c.strip_suffix('\r').unwrap_or(c));
            let Some(value) = value_span_in_line(content, name) else {
                continue;
            };
            if found.is_some() {
                return None; // a second occurrence -> ambiguous, decline
            }
            found = Some(Located {
                full,
                value: line_start + value.start..line_start + value.end,
            });
        }
        found
    }

    /// If `content` is a SIMPLE `key = value` / `key : value` line for `name`
    /// (no backslash, an explicit `=`/`:` separator), return the value span as a
    /// range WITHIN `content`; else `None`.
    fn value_span_in_line(content: &str, name: &str) -> Option<Range<usize>> {
        // Any backslash means an escape or a line-continuation -- too complex for
        // the conservative resolver, decline the whole line.
        if content.contains('\\') {
            return None;
        }
        let trimmed = content.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
            return None;
        }
        let lead = content.len() - trimmed.len();
        // The key runs to the first separator character (`=`, `:`, or whitespace).
        let key_end = trimmed.find(|c: char| c == '=' || c == ':' || c.is_whitespace())?;
        if trimmed[..key_end] != *name {
            return None;
        }
        // The separator must be an explicit `=`/`:` (with optional surrounding
        // whitespace); a whitespace-ONLY separator is declined.
        let after_key = &trimmed[key_end..];
        let ws1 = after_key.len() - after_key.trim_start().len();
        let after_sep = after_key.trim_start().strip_prefix(['=', ':'])?;
        let ws2 = after_sep.len() - after_sep.trim_start().len();
        // Value: leading whitespace stripped, TRAILING whitespace kept -> to the
        // end of the line. `=`/`:` are 1 ASCII byte.
        let vstart = lead + key_end + ws1 + 1 + ws2;
        Some(vstart..content.len())
    }

    /// Serialize a scalar as a RAW properties value, declining any value that
    /// would not round-trip verbatim: a control char or backslash (would need an
    /// escape, and a trailing `\` is a line-continuation), or LEADING whitespace
    /// (the parser strips it). Trailing whitespace is significant, so it is kept;
    /// `=`/`:`/`#`/`!` inside a value are literal (only line-leading matters).
    pub(super) fn properties_serialize_scalar(value: &serde_json::Value) -> Option<Vec<u8>> {
        use serde_json::Value;
        let s = match value {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Null | Value::Array(_) | Value::Object(_) => return None,
        };
        if s.starts_with(char::is_whitespace) || s.chars().any(|c| c == '\\' || c.is_control()) {
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
/// `remove` for a key) and re-serializes the WHOLE file, and the fixer emits ONE
/// `ReplaceRange` over the entire file (gated by `uses_document_rewrite`), not a
/// span splice. `toml_edit` rewrites only the mutated node's tokens, so the CHANGE
/// is surgical; the `finalize` pass restores the three details its reserialize
/// otherwise normalizes (CRLF line endings, a leading BOM, a missing trailing
/// newline) so the whole rewrite is byte-identical apart from the edit.
///
/// LIMITATION (the whole-document model's cost): each edit is a whole-file
/// `0..len` range, so TWO such edits on ONE file OVERLAP. The engine applies one
/// and re-applies the others on later fixpoint passes -- so N whole-document fix
/// rules matching ONE file need N passes. That converges (each pass makes
/// progress) and never corrupts, but a config with MORE than `MAX_PASSES` (10)
/// such rules on one file reports non-convergence (exit 2 -- re-run to continue),
/// and `--diff` / `--dry-run`, being single-pass previews, show only ONE such
/// rule's change per file (the real `fix` applies them all). Span formats
/// (HCL/XML/dotenv/INI) do not have this: their disjoint spans co-apply in one
/// pass. The proper fix -- coalescing a file's whole-document mutations across
/// rules into one edit -- is deferred.
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
        Some(finalize(text, &doc.to_string()))
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
        removed_any.then(|| finalize(text, &doc.to_string()))
    }

    /// Restore the byte-level details `toml_edit`'s reserialize normalizes away,
    /// so the whole-document rewrite stays surgical: it always emits LF, strips a
    /// leading BOM, and appends a trailing newline. Re-apply the ORIGINAL's
    /// trailing-newline presence, CRLF line endings (only when the file is
    /// UNIFORMLY CRLF -- a mixed file is left as LF rather than guessed), and BOM.
    fn finalize(original: &str, rendered: &str) -> Vec<u8> {
        let mut out = rendered.to_string();
        // Trailing newline: toml_edit always appends one; drop it if the original
        // had none (strip exactly the one it added).
        if !original.ends_with('\n') {
            if let Some(stripped) = out.strip_suffix('\n') {
                out = stripped.to_string();
            }
        }
        // Line endings: restore CRLF when the original was uniformly CRLF (the
        // rendered text is pure LF, so this cannot double an existing `\r\n`).
        if original.contains("\r\n") && !original.replace("\r\n", "").contains('\n') {
            out = out.replace('\n', "\r\n");
        }
        // BOM: toml_edit strips a leading U+FEFF; restore it.
        if original.starts_with('\u{feff}') && !out.starts_with('\u{feff}') {
            out.insert(0, '\u{feff}');
        }
        out.into_bytes()
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
mod tests;
