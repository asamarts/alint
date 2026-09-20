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
        Format::Json => json_::json_value_span(text, path),
        Format::Yaml => yaml_::yaml_value_span(text, path),
        // TOML is a document-rewrite format (see `document_set`), not a span
        // splice; named (not a wildcard) so a future span format is compile-forced.
        Format::Toml => None,
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
        Format::Yaml => yaml_::yaml_removal_span(text, path),
        // JSON removal is a whole-document CST rewrite (see `removal_uses_document`);
        // TOML removal goes via `document_remove` (`uses_document_rewrite`).
        Format::Json | Format::Toml => None,
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
        Format::Json => json_::json_serialize_scalar(value),
        Format::Toml => toml_::serialize_scalar(value),
        Format::Yaml => yaml_::yaml_serialize_scalar(value),
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
        Format::Json => json_::json_document_remove(text, paths),
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

/// Whether `format` has a `remove_value` implementation at all. ALL formats now do
/// (YAML via a conservative single-line block-entry line scan, JSON via the CST,
/// TOML via `toml_edit`, the rest via a span). The fixer's `can_fix` consults this
/// so `check` does not advertise an auto-fix a format can NEVER apply; a
/// DOCUMENT-dependent decline (e.g. the XML document root, a repeated-block parent,
/// or a YAML block scalar) still resolves for most inputs and stays honestly
/// advertised (the tolerated over-promise). Keep this in lockstep with the removal
/// arms of [`resolve_removal_span`] / [`document_remove`] / [`removal_uses_document`];
/// the structured-fix classification gate asserts the parity. (Kept as a predicate
/// -- not a constant `true` -- so a future format defaults to declining until it is
/// explicitly wired here, and so the gate's parity check has something to assert.)
#[must_use]
pub fn format_supports_removal(format: Format) -> bool {
    match format {
        Format::Hcl
        | Format::Xml
        | Format::Dotenv
        | Format::Ini
        | Format::Properties
        | Format::Toml
        | Format::Json
        | Format::Yaml => true,
    }
}

/// Whether `format`'s `remove_value` is a WHOLE-DOCUMENT rewrite (edit + reserialize
/// an editable CST) rather than a per-node span splice. JSON removal uses the
/// `jsonc-parser` editable CST for correct comma / comment surgery, while its SET
/// stays a span splice -- a per-OP split, so JSON is listed here but is NOT
/// [`uses_document_rewrite`] (which is all-ops, for TOML, and whose branch handles
/// TOML removal already). The fixer emits ONE `ReplaceRange` over the whole file
/// for a removal on a format listed here.
#[must_use]
pub fn removal_uses_document(format: Format) -> bool {
    matches!(format, Format::Json)
}

/// The MINIMAL `(range, content)` edit that turns `original` into `new`: the span
/// between their common byte prefix and common byte suffix, and the differing
/// middle of `new`. A whole-document rewrite (TOML / JSON removal) reserializes the
/// ENTIRE file, but typically only ONE value/member changed; emitting this minimal
/// splice (instead of `0..len` over the whole file) makes independent whole-doc
/// rules on ONE file DISJOINT, so they co-apply in a SINGLE fixpoint pass (no more
/// one-rule-per-pass serialization: no false exit-2 at >10 rules, and `--diff`
/// shows every change). Two rules touching the SAME span still overlap -> the
/// engine's overlap-skip serializes them (a genuinely conflicting config still
/// oscillates to a loud exit 2). `original[range]` replaced by `content` yields
/// exactly `new` by construction. An unchanged document yields an empty range +
/// empty content (an identity no-op the caller / engine drops).
#[must_use]
pub fn minimal_replace(original: &[u8], new: &[u8]) -> (Range<usize>, Vec<u8>) {
    let common_prefix = original.iter().zip(new).take_while(|(a, b)| a == b).count();
    // The suffix may not overlap the prefix in either buffer.
    let max_suffix = original.len().min(new.len()) - common_prefix;
    let common_suffix = original
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    let range = common_prefix..original.len() - common_suffix;
    let content = new[common_prefix..new.len() - common_suffix].to_vec();
    (range, content)
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
        // `hcl-rs` rejects a leading BOM, so strip it (as `Format::parse` does)
        // and shift the resolved span back into the ORIGINAL byte space by the
        // BOM length -- else a BOM-prefixed HCL file is advertised fixable yet
        // always skipped (HCL was the lone format not offsetting the BOM; the
        // other 7 do). `trim_start_matches` drops any run of BOMs.
        let stripped = text.trim_start_matches('\u{feff}');
        let bom_len = text.len() - stripped.len();
        let body = hcl::edit::parser::parse_body(stripped).ok()?;
        let span = resolve_in_body(&body, path).map(|t| t.value_span)?;
        Some(span.start + bom_len..span.end + bom_len)
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
        // Strip a leading BOM (hcl-rs rejects it) and offset the result back into
        // the original byte space -- see `hcl_value_span`. All line math below is
        // on the stripped text.
        let stripped = text.trim_start_matches('\u{feff}');
        let bom_len = text.len() - stripped.len();
        let body = hcl::edit::parser::parse_body(stripped).ok()?;
        let attr_span = resolve_in_body(&body, path)?.attr_span?;
        let bytes = stripped.as_bytes();
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
        let before = &stripped[line_start..attr_span.start];
        let after = &stripped[attr_span.end..line_end];
        let span =
            if before.chars().all(char::is_whitespace) && line_tail_is_blank_or_comment(after) {
                line_start..line_end
            } else {
                attr_span
            };
        Some(span.start + bom_len..span.end + bom_len)
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
/// / continued / space-separated properties are deferred. These are all
/// DOCUMENT-dependent declines, so `check`'s static `can_fix` (which cannot
/// inspect the file) may advertise such a key "fixable" while `fix` then skips it
/// -- the tolerated over-promise in the safe direction (check never claims LESS
/// than fix resolves), which the conservative resolver simply hits more often
/// here (a `\uXXXX` value is common). It never corrupts: fix skips, exit stays 1.
///
/// Properties is FLAT (a path is one `Key`), and each simple key owns its physical
/// line. The parser strips LEADING value whitespace but keeps TRAILING whitespace,
/// so the value span runs from after the separator to the END of the line. Lines
/// split on `\r\n` / bare `\r` / `\n` -- `java-properties`'s line model (see
/// `locate`), so a CR-separated neighbor is never over-deleted.
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
    ///
    /// Lines are split on `\r\n`, a bare `\r`, OR `\n` -- `java-properties`'s line
    /// model. This MUST match the parser: a bare `\r` (classic-Mac / hand-edited
    /// files) is a line terminator there, so splitting on `\n` only would see two
    /// CR-separated assignments as one line and OVER-DELETE the neighbor (the span
    /// runs to end-of-line). Byte-scanning for `\r`/`\n` is UTF-8-safe -- both are
    /// ASCII and never occur inside a multibyte char.
    fn locate(text: &str, name: &str) -> Option<Located> {
        let (body, base) = match text.strip_prefix('\u{feff}') {
            Some(rest) => (rest, '\u{feff}'.len_utf8()),
            None => (text, 0),
        };
        let bytes = body.as_bytes();
        let mut found: Option<Located> = None;
        let mut i = 0usize; // byte offset of the current line's start within `body`
        while i < bytes.len() {
            // Content runs to the next `\r` or `\n` (or EOF).
            let mut j = i;
            while j < bytes.len() && bytes[j] != b'\n' && bytes[j] != b'\r' {
                j += 1;
            }
            // Terminator: `\r\n` (2), a lone `\r`/`\n` (1), or none at EOF (0).
            let term = if j >= bytes.len() {
                0
            } else if bytes[j] == b'\r' && bytes.get(j + 1) == Some(&b'\n') {
                2
            } else {
                1
            };
            let content = &body[i..j];
            let line_start = base + i;
            let line_end = base + j + term;
            i = j + term;
            let Some(value) = value_span_in_line(content, name) else {
                continue;
            };
            if found.is_some() {
                return None; // a second occurrence -> ambiguous, decline
            }
            found = Some(Located {
                full: line_start..line_end,
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
/// The whole reserialized document is reduced to its MINIMAL changed span by the
/// fixer ([`minimal_replace`]) before it becomes a `ReplaceRange`, NOT emitted as a
/// `0..len` whole-file edit. So two whole-document rules changing DIFFERENT keys in
/// ONE file produce DISJOINT edits that co-apply in a single fixpoint pass -- like
/// the span formats -- with no false exit-2 at >10 rules and a `--diff` that shows
/// every change (follow-up 2). Two rules touching the SAME span still overlap, so a
/// genuinely conflicting config (two different values for one key) still oscillates
/// to a loud exit 2.
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

/// JSON / JSONC span resolution + value serialization via `jsonc-parser`.
///
/// `parse_to_ast` yields an AST whose every node carries a byte `range`, so
/// `set_value` is a SPAN-splice of the target value node's range (like HCL/XML):
/// disjoint edits co-apply in one pass, and JSONC comments / trailing commas
/// survive untouched (the splice never reformats). JSON is TYPED
/// (`format_leaves_are_strings` is false for it), so a numeric / bool / null
/// `equals` is fixable, and `null` is a representable value.
///
/// `remove_value` is a WHOLE-DOCUMENT rewrite via the `jsonc-parser` editable CST
/// (`json_document_remove` / [`removal_uses_document`]): the CST owns the comma /
/// trailing-comma / comment surgery, so a deletion NEVER over-deletes a sibling --
/// the trap a hand-rolled span splice falls into (the `Absent` verifier is blind to
/// eating a neighbour that still leaves the target absent). So JSON is a per-OP
/// split: span SET + document REMOVE. The check side parses via `serde_json`
/// (+ `strip_jsonc`); this spanned AST (+ the CST for removal) is the fix side's
/// view, and the two agree on standard-JSON structure.
mod json_ {
    use super::PathSeg;
    use jsonc_parser::common::Ranged;
    use jsonc_parser::{CollectOptions, CommentCollectionStrategy, ParseOptions, parse_to_ast};
    use std::ops::Range;

    /// The byte range of the value at `path` (what `set_value` overwrites). `None`
    /// if the source does not parse or the path does not resolve to a node.
    pub(super) fn json_value_span(text: &str, path: &[PathSeg]) -> Option<Range<usize>> {
        // Mirror `Format::parse`: strip leading UTF-8 BOM(s) before parsing.
        // `jsonc-parser` rejects a `\u{FEFF}` prefix as a syntax error, but the
        // check side strips it (so a BOM-prefixed file DOES fire the rule and
        // advertise a fix). Resolve against the stripped text, then shift the
        // span back into the ORIGINAL byte space by the stripped BOM length --
        // the caller splices the raw file bytes, BOM included. Without this,
        // every BOM-prefixed JSON was advertised fixable yet always skipped
        // (JSON was the outlier: dotenv/INI/properties offset it, TOML restores
        // it). `trim_start_matches` drops any run of consecutive BOMs, matching
        // `Format::parse`.
        let stripped = text.trim_start_matches('\u{feff}');
        let bom_len = text.len() - stripped.len();
        let ast = parse_to_ast(
            stripped,
            &CollectOptions {
                comments: CommentCollectionStrategy::Off,
                tokens: false,
            },
            &ParseOptions {
                allow_comments: true,
                allow_trailing_commas: true,
                allow_loose_object_property_names: false,
            },
        )
        .ok()?;
        let mut node = ast.value.as_ref()?;
        for seg in path {
            node = match seg {
                PathSeg::Key(k) => &node.as_object()?.get(k)?.value,
                PathSeg::Index(i) => node.as_array()?.elements.get(*i)?,
            };
        }
        let r = node.range();
        Some(r.start + bom_len..r.end + bom_len)
    }

    /// Serialize a scalar as JSON bytes (a number / bool bare, a string
    /// quoted + escaped, null as `null`). `None` for a non-scalar (an object /
    /// array), which the caller routes to a Suggestion.
    pub(super) fn json_serialize_scalar(value: &serde_json::Value) -> Option<Vec<u8>> {
        use serde_json::Value as J;
        match value {
            J::String(_) | J::Number(_) | J::Bool(_) | J::Null => serde_json::to_vec(value).ok(),
            J::Array(_) | J::Object(_) => None,
        }
    }

    /// Remove the object member / array element at each `path` from the JSON via
    /// the `jsonc-parser` editable CST, returning the reserialized document. Unlike
    /// `set_value` (a span splice), removal is a WHOLE-DOCUMENT rewrite: the CST
    /// owns the comma / trailing-comma / comment surgery (dprint's editor), so a
    /// deletion NEVER over-deletes a sibling -- the trap a hand-rolled span splice
    /// falls into (the `Absent` verifier is blind to eating a neighbour that still
    /// leaves the target absent). The CST round-trips byte-identically, so an
    /// unmodified region is untouched. `None` if the source does not parse or NO
    /// path resolved (nothing removed).
    pub(super) fn json_document_remove(text: &str, paths: &[Vec<PathSeg>]) -> Option<Vec<u8>> {
        use jsonc_parser::cst::CstRootNode;
        // The check side strips a leading BOM; strip before the CST parse (which
        // rejects it) and re-prepend it to the reserialized output.
        let stripped = text.trim_start_matches('\u{feff}');
        let bom = &text[..text.len() - stripped.len()];
        let root = CstRootNode::parse(
            stripped,
            &ParseOptions {
                allow_comments: true,
                allow_trailing_commas: true,
                allow_loose_object_property_names: false,
            },
        )
        .ok()?;
        // Resolve ALL target handles FIRST, then remove -- removing by handle (not
        // by re-navigating) is index-shift-safe for array elements and order-free
        // for object members. But DROP any path that is a strict DESCENDANT of
        // another matched path (a recursive query like `$..a` can match both a
        // container and a node inside it): removing the ancestor detaches the
        // descendant's CST node, and `.remove()` on the orphaned handle PANICS. The
        // ancestor's removal subsumes the descendant, so dropping it is correct.
        let targets: Vec<RemoveTarget> = paths
            .iter()
            .filter(|p| {
                !paths
                    .iter()
                    .any(|q| q.len() < p.len() && p.starts_with(q.as_slice()))
            })
            .filter_map(|p| navigate_to_removable(&root, p))
            .collect();
        if targets.is_empty() {
            return None;
        }
        for t in targets {
            t.remove();
        }
        Some(format!("{bom}{root}").into_bytes())
    }

    /// A removable CST node: an object member (removed with its comma) or an array
    /// element. Both `remove(self)` handle the surrounding separator surgery.
    enum RemoveTarget {
        Prop(jsonc_parser::cst::CstObjectProp),
        Elem(jsonc_parser::cst::CstNode),
    }

    impl RemoveTarget {
        fn remove(self) {
            match self {
                RemoveTarget::Prop(p) => p.remove(),
                RemoveTarget::Elem(n) => n.remove(),
            }
        }
    }

    /// Navigate the CST to the removable node at `path` (the object PROP for a
    /// trailing key, or the array ELEMENT for a trailing index). `None` if any
    /// segment does not resolve (declines rather than guess).
    fn navigate_to_removable(
        root: &jsonc_parser::cst::CstRootNode,
        path: &[PathSeg],
    ) -> Option<RemoveTarget> {
        let (last, parents) = path.split_last()?;
        let mut node = root.value()?;
        for seg in parents {
            node = match seg {
                PathSeg::Key(k) => node.as_object()?.get(k)?.value()?,
                PathSeg::Index(i) => node.as_array()?.elements().into_iter().nth(*i)?,
            };
        }
        Some(match last {
            PathSeg::Key(k) => RemoveTarget::Prop(node.as_object()?.get(k)?),
            PathSeg::Index(i) => {
                RemoveTarget::Elem(node.as_array()?.elements().into_iter().nth(*i)?)
            }
        })
    }
}

/// YAML span resolution + value serialization via `saphyr::MarkedYaml`.
///
/// `MarkedYaml` is a spanned YAML node tree, so `set_value` is a SPAN-splice of
/// the target scalar's range (like JSON): the span covers the full scalar token
/// (quotes included for quoted scalars, trailing comment/whitespace EXCLUDED after
/// the clamp below), so the splice never disturbs a neighbour. The check side
/// parses YAML via `serde_yaml_ng` (libyaml), which exposes no spans, hence this
/// separate spanned view; a JSON scalar is valid YAML with the same value+type
/// (YAML is a superset of JSON), so the serializer reuses `serde_json`.
///
/// CONSERVATIVE by design: for SET the resolver declines a multi-document stream, a
/// non-scalar target, an aliased value (`saphyr` leaves aliases un-expanded as
/// `Alias` nodes, so it never expands a billion-laughs bomb -- but the path then
/// resolves to a non-scalar and declines), a non-string mapping key, a BLOCK (`|` /
/// `>`) or line-folded scalar (multi-line span), and an IMPLICIT null (`x:` --
/// saphyr's zero-width span sits at the colon). `remove_value` (`yaml_removal_span`)
/// deletes a single-line block-mapping entry's whole line (saphyr is read-only, so
/// this is a hand-rolled line scan kept provably minimal -- it declines a
/// sequence-element path, a multi-line / non-scalar value, and a flow member whose
/// key does not own its line).
///
/// THREE `saphyr` 0.0.12 quirks handled here (all verified empirically + gated):
/// (1) `Marker::index` is a CHAR offset despite the `index()` rustdoc claiming
/// bytes, so spans are converted char->byte; a bump must re-verify this. (2) a
/// QUOTED scalar's `span.end` runs to end-of-line (past a trailing comment), so
/// `clamp_quoted_scalar_span` tightens it to the closing quote -- else a splice
/// silently deletes the comment (the audit's HIGH finding; a plain scalar's end is
/// already tight). (3) `saphyr` does not bound recursion, so a defensive flow-depth
/// guard mirrors the check side's `Format::parse` (which the fixer runs first
/// anyway) to keep this resolver self-safe on deeply-nested flow input. A leading
/// BOM is stripped + offset (the check side strips it but saphyr rejects it).
mod yaml_ {
    use super::PathSeg;
    use saphyr::{LoadableYamlNode, MarkedYaml, Scalar, YamlData};
    use std::ops::Range;

    /// The byte range of the scalar value at `path`. `None` if the source does not
    /// parse, is a multi-document stream, or the path does not resolve to a scalar.
    pub(super) fn yaml_value_span(text: &str, path: &[PathSeg]) -> Option<Range<usize>> {
        // Defensive DoS guard (parser-guard parity): `saphyr` is recursive-descent
        // and unbounded. The fixer runs `Format::parse` (which applies this) before
        // reaching here, but guard directly so the resolver is self-safe.
        if !crate::yaml_depth::flow_depth_within_limit(text) {
            return None;
        }
        // Strip a leading UTF-8 BOM before parsing: the check side (`Format::parse`)
        // strips it, but `saphyr` rejects a `\u{FEFF}` prefix, so a BOM file would
        // else be advertised-fixable yet always skipped. Resolve against the stripped
        // text, then shift the span back into ORIGINAL byte coordinates by the
        // stripped length (the caller splices the raw file bytes, BOM included).
        let stripped = text.trim_start_matches('\u{feff}');
        let bom_len = text.len() - stripped.len();
        let docs = MarkedYaml::load_from_str(stripped).ok()?;
        // Conservative: only a single-document stream (multi-doc `$path` resolution
        // is ambiguous).
        let [doc] = docs.as_slice() else {
            return None;
        };
        let mut node = doc;
        for seg in path {
            node = match seg {
                PathSeg::Key(k) => get_key(node, k)?,
                PathSeg::Index(i) => get_index(node, *i)?,
            };
        }
        // Only a SCALAR leaf is a set_value target (a mapping/sequence/tagged node
        // declines).
        if !matches!(node.data, YamlData::Value(_) | YamlData::Representation(..)) {
            return None;
        }
        // `saphyr`'s `Marker::index` is a CHAR offset -> convert to bytes.
        let raw = char_span_to_byte(stripped, node.span.start.index(), node.span.end.index())?;
        let at = stripped.get(raw.clone())?;
        // Decline two saphyr span shapes a flow-scalar splice cannot safely replace
        // (both are verify-gated anyway, but decline cleanly rather than emit an
        // un-appliable suggestion / trust a bogus span):
        //   - an EMPTY span: an IMPLICIT null (`x:`) -- saphyr places a zero-width
        //     span AT THE COLON, a bogus splice point (explicit `~` / `""` are fine);
        //   - a MULTI-LINE span: a block (`|` / `>`) or line-folded scalar -- the
        //     span is the block CONTENT, so splicing a flow scalar leaves the block
        //     indicator and changes meaning.
        if at.is_empty() || at.contains('\n') {
            return None;
        }
        // Decline a YAML ALIAS reference (`use: *anchor`): saphyr resolves it to the
        // anchored VALUE but leaves the span on the `*ref` text, so a splice would
        // silently DE-ALIAS it (replace the reference with a literal) -- a semantic
        // change, so decline (conservative). A plain scalar can never begin with the
        // reserved `*`; an anchor DEFINITION value (`&a 5`) has span `5` (the `&a` is
        // excluded) and stays fixable. A quoted `"*x"` string's span starts with `"`.
        if at.starts_with('*') {
            return None;
        }
        // Tighten the trailing edge of a QUOTED scalar: saphyr's span end runs to
        // END-OF-LINE (over trailing whitespace + a `#` comment), whereas a plain
        // scalar's end is tight. Without this, a splice over `"old"  # keep` DELETES
        // the comment and COMMITS (a comment is not part of the value, so re-verify
        // cannot catch it) -- the audit's HIGH silent-data-loss finding.
        let span = clamp_quoted_scalar_span(stripped, raw);
        Some(span.start + bom_len..span.end + bom_len)
    }

    /// Clamp a quoted scalar's span end to just after its closing quote. `saphyr`
    /// over-extends a quoted scalar's `span.end` to end-of-line (past trailing
    /// whitespace + a comment); a plain / block scalar's end is already tight, so
    /// its span is returned unchanged. Byte-scanning is UTF-8-safe: the quote and
    /// backslash bytes are ASCII and never collide with a multibyte continuation.
    fn clamp_quoted_scalar_span(text: &str, span: Range<usize>) -> Range<usize> {
        let bytes = text.as_bytes();
        let open = bytes[span.start];
        if open != b'"' && open != b'\'' {
            return span; // plain / block scalar: `span.end` is already tight.
        }
        let mut i = span.start + 1;
        while i < span.end {
            match bytes[i] {
                // A double-quote escape (`\"`, `\\`, `\n`, ... all ASCII) skips 2.
                b'\\' if open == b'"' => i += 2,
                b if b == open => {
                    // In a single-quoted scalar `''` is a literal quote, not a close.
                    if open == b'\'' && bytes.get(i + 1) == Some(&b'\'') {
                        i += 2;
                    } else {
                        return span.start..i + 1; // just after the closing quote
                    }
                }
                _ => i += 1,
            }
        }
        span // unterminated within the span (unreachable for a valid parsed scalar)
    }

    /// The mapping value for a STRING key `key` (declines a non-mapping node or a
    /// non-string key -- the conservative common case).
    fn get_key<'a, 'i>(node: &'a MarkedYaml<'i>, key: &str) -> Option<&'a MarkedYaml<'i>> {
        let YamlData::Mapping(m) = &node.data else {
            return None;
        };
        m.iter().find_map(|(k, v)| match &k.data {
            YamlData::Value(Scalar::String(s)) if s == key => Some(v),
            _ => None,
        })
    }

    /// The `i`th sequence element (declines a non-sequence node).
    fn get_index<'a, 'i>(node: &'a MarkedYaml<'i>, i: usize) -> Option<&'a MarkedYaml<'i>> {
        let YamlData::Sequence(seq) = &node.data else {
            return None;
        };
        seq.get(i)
    }

    /// Map a `[cs, ce)` CHAR span to a `[bs, be)` BYTE span over `text`. An offset
    /// equal to the total char count is the end of the string.
    fn char_span_to_byte(text: &str, cs: usize, ce: usize) -> Option<Range<usize>> {
        if cs > ce {
            return None;
        }
        let (mut bs, mut be) = (None, None);
        let mut count = 0usize;
        for (b, _) in text.char_indices() {
            if count == cs {
                bs = Some(b);
            }
            if count == ce {
                be = Some(b);
            }
            count += 1;
        }
        if cs == count {
            bs = Some(text.len());
        }
        if ce == count {
            be = Some(text.len());
        }
        Some(bs?..be?)
    }

    /// Serialize a scalar as YAML bytes. A JSON scalar is valid YAML with the same
    /// value + type (YAML is a superset of JSON): a number/bool renders bare, `null`
    /// as `null`, a string double-quoted + escaped -- and it re-parses to the same
    /// value so the `equals` re-verify holds. `None` for a non-scalar (routed to a
    /// Suggestion).
    pub(super) fn yaml_serialize_scalar(value: &serde_json::Value) -> Option<Vec<u8>> {
        use serde_json::Value as J;
        match value {
            J::String(_) | J::Number(_) | J::Bool(_) | J::Null => serde_json::to_vec(value).ok(),
            J::Array(_) | J::Object(_) => None,
        }
    }

    /// The byte range `remove_value` deletes for the block-mapping entry at `path`:
    /// the target key's WHOLE physical line (through its trailing newline), so the
    /// splice is a clean line removal. `saphyr` is read-only (no edit CST), so this
    /// is a hand-rolled line scan -- CONSERVATIVE to stay provably minimal (the
    /// over-deletion class): resolves ONLY when
    ///   - the last path segment is a mapping KEY (sequence-element removal declines);
    ///   - the value is a single-line SCALAR (a block `|`/`>` or nested mapping /
    ///     sequence value spans lines -> declines, else the line delete would orphan
    ///     the continuation);
    ///   - the KEY OWNS its line (only indentation precedes it) -- so a FLOW-mapping
    ///     member (`x: {a: 1, b: 2}`, key not at line start) declines rather than
    ///     delete the whole `x:` line and eat its siblings.
    ///
    /// Deleting exactly that one line removes exactly the one entry (block mappings
    /// are one entry per line); a trailing `# comment` on the line goes with it. The
    /// `Absent` re-verify catches any residual syntax breakage (e.g. a now-dangling
    /// anchor alias). A leading BOM is stripped before parse + the span offset back.
    pub(super) fn yaml_removal_span(text: &str, path: &[PathSeg]) -> Option<Range<usize>> {
        if !crate::yaml_depth::flow_depth_within_limit(text) {
            return None;
        }
        let stripped = text.trim_start_matches('\u{feff}');
        let bom_len = text.len() - stripped.len();
        let docs = MarkedYaml::load_from_str(stripped).ok()?;
        let [doc] = docs.as_slice() else {
            return None;
        };
        // The last segment must be a mapping KEY (sequence-element removal deferred).
        let (PathSeg::Key(target), parents) = path.split_last()? else {
            return None;
        };
        let mut node = doc;
        for seg in parents {
            node = match seg {
                PathSeg::Key(k) => get_key(node, k)?,
                PathSeg::Index(i) => get_index(node, *i)?,
            };
        }
        // Find the `target` (key_node, value_node) pair in the parent mapping.
        let YamlData::Mapping(m) = &node.data else {
            return None;
        };
        let (knode, vnode) = m
            .iter()
            .find(|(k, _)| matches!(&k.data, YamlData::Value(Scalar::String(s)) if s == target))?;
        // The value must be a SCALAR (a mapping/sequence value is multi-line).
        if !matches!(
            vnode.data,
            YamlData::Value(_) | YamlData::Representation(..)
        ) {
            return None;
        }
        // The entry, from the key start to the value end, must be single-line.
        let entry = char_span_to_byte(stripped, knode.span.start.index(), vnode.span.end.index())?;
        if stripped.get(entry.clone())?.contains('\n') {
            return None;
        }
        // The KEY must OWN its line: only indentation before it (excludes a flow
        // member, whose deletion would eat siblings on the same physical line).
        let line_start = stripped[..entry.start].rfind('\n').map_or(0, |i| i + 1);
        if !stripped[line_start..entry.start]
            .bytes()
            .all(|b| b == b' ' || b == b'\t')
        {
            return None;
        }
        // Delete the whole line, through its trailing newline (or to EOF).
        let line_end = stripped[entry.end..]
            .find('\n')
            .map_or(stripped.len(), |i| entry.end + i + 1);
        // The value must also be the LAST content on its physical line: the tail
        // after it may be only whitespace and/or a `#` comment. Without this, a
        // MULTI-LINE flow mapping (`x: {\n  a: 1, b: 2\n}` -- where the key starts a
        // physical line it SHARES with sibling members after the value) would have
        // the whole-line delete engulf those siblings: an over-deletion that still
        // parses, so the `Absent` re-verify is blind to it (the audit HIGH bug --
        // symmetric to the leading `line_start..entry.start` guard, mirroring HCL).
        let tail = stripped[entry.end..line_end]
            .trim_end_matches(['\r', '\n'])
            .trim_start_matches([' ', '\t']);
        if !(tail.is_empty() || tail.starts_with('#')) {
            return None;
        }
        Some(line_start + bom_len..line_end + bom_len)
    }
}

#[cfg(test)]
mod tests;
