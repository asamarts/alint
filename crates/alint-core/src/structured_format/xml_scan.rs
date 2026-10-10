//! The XML pre-scan that bounds nesting depth, per-element attribute width and
//! namespace-resolution cost BEFORE `roxmltree::Document::parse` runs, plus the
//! limits it enforces.

/// Maximum XML element-nesting depth `xml_to_value` will
/// descend. Real config/manifest XML (`.csproj`, `pom.xml`, …)
/// is a handful of levels deep; 128 is far beyond any real
/// manifest yet far below the recursion depth that would
/// overflow the stack. A document nested deeper is rejected as a
/// parse error (one per-file violation via the existing
/// parse-error path) rather than recursed into — a crafted or
/// accidental deeply-nested file must never abort the run. Unlike
/// HCL, XML parses on the CALLING thread (a rayon worker, ~2 MB
/// stack by Rust's std-thread default). roxmltree recurses ~1
/// frame per element; measured overflow is around depth ~350 on a
/// 2 MB debug stack (the realistic worker) and ~175 on a
/// constrained 1 MB stack, with far deeper limits in release
/// (~3100 on 2 MB). 128 keeps a ~2.7x margin on the 2 MB worker
/// (still ~1.4x even on a 1 MB stack), matching the JSON recursion
/// limit and the `when`-parser's calibration. The other formats'
/// parsers carry their own internal recursion limits; this is the
/// XML arm's equivalent.
pub const MAX_XML_DEPTH: usize = 128;

/// The analytically-safe ceiling for [`MAX_XML_DEPTH`], enforced at compile time
/// below. roxmltree recurses ~1 frame per element; the smallest stack alint
/// realistically parses XML on is a ~2 MiB rayon worker, which overflows (debug)
/// around ~350 elements deep, so ~half of that keeps a >=2x margin. This ceiling is
/// SCOPED to that 2 MiB production worker; the shipping `MAX_XML_DEPTH = 128` also
/// survives a constrained 1 MiB stack (~1.4x), but a raise all the way to this
/// ceiling would erode that non-production 1 MiB margin to ~1.1x (fine on 2 MiB) --
/// so treat a bump toward 160 as 2-MiB-only. Raising `MAX_XML_DEPTH` PAST this risks
/// a stack-overflow process-abort even on the 2 MiB worker -- and the depth tests
/// only exercise the REJECTION path (they run on a >=2 MiB harness stack where even
/// 300-deep survives), so a careless re-widening would pass every runtime test. This
/// static bound is the real guard.
// Rust 1.88's dead-code analysis does not count this use from an unnamed const;
// keep the MSRV build warning-free without widening the internal API.
#[allow(dead_code)]
const SAFE_MAX_XML_DEPTH: usize = 160;
const _: () = assert!(
    MAX_XML_DEPTH <= SAFE_MAX_XML_DEPTH,
    "MAX_XML_DEPTH exceeds its analytically-safe ceiling -- a deeply-nested XML \
     file could overflow the rayon-worker stack inside roxmltree parsing (SIGABRT)."
);

/// Maximum attributes on a single XML element that `xml_to_value` will accept.
/// roxmltree 0.20 validates per-element attribute UNIQUENESS in O(n^2) -- each new
/// attribute is compared against every prior attribute on the same element -- so a
/// single element bearing tens of thousands of distinct attributes turns a tiny
/// file into MINUTES of parse time (`<r a0=".." a1=".." …/>`: ~64 K attrs ≈ 96 s,
/// clean quadratic), an algorithmic-complexity `DoS` that NO nesting guard catches
/// (all the depth guards bound height, not width). Bounding attributes per element
/// makes total parse cost linear in the input: the aggregate work is
/// `sum(k_i^2) <= cap * sum(k_i) = cap * total_attrs`, and `total_attrs` is bounded
/// by the `MAX_ANALYZE_BYTES` (256 MiB) read cap, so the whole document is O(bytes).
/// The cap ALSO sets the constant: at 256 a crafted attribute-dense file parses at
/// roughly benign-XML speed (measured ~1.5x a same-size ordinary file, vs ~5x at
/// 1024), so it no longer costs meaningfully more than any other file of its size --
/// unlike HCL, XML has no format-specific byte cap (real XML data files can be large
/// and must not false-error), so the per-element cap is the sole width bound and is
/// kept tight. 256 is still ~5x beyond even an attribute-heavy real element (an
/// `MSBuild` `<Csc>`/`<Vbc>` task, the widest common case, exposes ~40; SVG/`.csproj`
/// nodes have far fewer) -- XML expresses repetition with child ELEMENTS, not
/// hundreds of attributes on one tag. roxmltree can't be bumped to fix this (0.21
/// stack-overflows on nesting; pinned at 0.20). An over-cap element is rejected as
/// one ordinary per-file parse-error violation.
pub(super) const MAX_XML_ATTRS_PER_ELEMENT: usize = 256;

/// Maximum DISTINCT namespace prefixes (incl. the default namespace) in scope at
/// any element. roxmltree 0.20 resolves namespaces per element that declares
/// any `xmlns`: it copies each of the parent's in-scope bindings after a linear
/// "already redeclared?" scan, so one such element costs O(P^2) in the P
/// bindings in scope, and every prefixed name lookup costs O(P). Unbounded, the
/// depth x attribute caps still allow P = 128 x 256: 100 nested elements each
/// declaring 256 prefixes, then ten `<x xmlns:z="u"/>` leaves (448 KB) took
/// 32-40 s to parse. Real documents bind far fewer -- a Word `document.xml` root
/// declares ~30-40, SVG / XAML / SOAP / `pom.xml` / `.csproj` a handful -- so 128
/// is ~3x the heaviest real case.
const MAX_XML_NAMESPACES_IN_SCOPE: usize = 128;

/// Budget for roxmltree's whole-document namespace-resolution work, in units of
/// its inner-loop steps (each declaring element with P bindings inherited and D
/// of its own costs ~(P + 1) x (P + D)). The in-scope cap bounds ONE element, but
/// a max-size file can repeat the declaring element ~1.5M times: a root binding
/// 64 prefixes over 1.5M `<x xmlns:z="u"/>` children took 9.4 s (vs 0.4 s
/// without the `xmlns`). Measured ~1.5 ns per unit, so 512M units bounds the
/// overhead to ~0.75 s, while a large generated SOAP / .NET-serializer document
/// (an `xmlns` on every element, a few bindings in scope: ~30 units each) stays
/// orders of magnitude below it.
const MAX_XML_NAMESPACE_WORK: usize = 512 * 1024 * 1024;

/// Conservatively bound the raw XML's element-nesting DEPTH and per-element
/// attribute WIDTH BEFORE `roxmltree::Document::parse` sees it, in one linear scan.
/// `Document::parse` descends recursively per element and overflows the stack —
/// **aborting the whole process** — on deeply-nested input (tens of thousands of
/// levels); the `element_to_value` [`MAX_XML_DEPTH`] guard is post-parse, so it
/// only catches depths the parser already survived. It also validates attribute
/// uniqueness in O(n^2) per element (see [`MAX_XML_ATTRS_PER_ELEMENT`]), a separate
/// wall-clock `DoS`, and resolves namespaces in O(P^2) per declaring element (see
/// [`MAX_XML_NAMESPACES_IN_SCOPE`] / [`MAX_XML_NAMESPACE_WORK`]), a third. A
/// cheap linear pre-scan rejects an over-deep, over-wide or namespace-heavy
/// document here (as one ordinary per-file parse-error violation) so a crafted or
/// accidental `<a><a>…` / `<r a0.. a1..>` file can never abort or hang the run.
/// Comment / CDATA / PI regions are skipped so their contents don't count toward
/// depth or attributes. `Ok(())` when within both limits.
///
/// **Lexer parity with roxmltree (0.20) is the security invariant.** Any region
/// the scan treats as opaque must end exactly where roxmltree's tokenizer ends it;
/// if the scan ends it EARLIER, a `</a>` roxmltree sees as PI / comment text is
/// counted as a close, cancels a real `<a>`, and an arbitrarily deep document
/// reaches the recursive parse (`<a><?p ></a>?>` x 100 000 aborted the process).
/// So each boundary mirrors the tokenizer: a PI ends at `?>` (searched after the
/// `<?`), a comment at `-->` searched AFTER `<!--` (so `<!--->` does not close
/// it), CDATA at `]]>`, and only the leading `<?xml …?>` declaration honors quotes
/// (its pseudo-attribute values may hold `?>`). Everywhere else the scan fails
/// closed: a `<!` that is neither comment nor CDATA (a DOCTYPE, rejected because
/// DTDs are disabled, or an unknown token) is stepped over by two bytes only, so
/// nothing after it is hidden; and a tag scan stops at any `<` even inside a
/// quoted value (roxmltree rejects `<` there), so a runaway quote can't swallow
/// the markup that follows. Hidden-close fixtures are probed against the real
/// parser in the tests.
pub(super) fn xml_within_parse_limits(text: &str) -> std::result::Result<(), String> {
    xml_within_limits(text, MAX_XML_DEPTH, MAX_XML_NAMESPACES_IN_SCOPE)
}

/// [`xml_within_parse_limits`] with explicit depth and in-scope-namespace
/// ceilings, so the parity property tests can probe the scan at the REAL
/// parser's depth / namespace count.
fn xml_within_limits(
    text: &str,
    max_depth: usize,
    max_namespaces: usize,
) -> std::result::Result<(), String> {
    /// Byte offset just past the first `needle` at or after `from`, or EOF.
    fn skip_past(text: &str, from: usize, needle: &str) -> usize {
        text.get(from..)
            .and_then(|tail| tail.find(needle))
            .map_or(text.len(), |p| from + p + needle.len())
    }
    let bytes = text.as_bytes();
    let mut pos = 0usize;
    if bytes.starts_with(b"\xEF\xBB\xBF") {
        pos = 3;
    }
    // The XML declaration (`<?xml version=".." ?>`), recognized exactly where
    // roxmltree does (the very start, after an optional BOM): skip to `?>`
    // OUTSIDE quotes, since its pseudo-attribute values are quoted strings.
    if bytes[pos..].starts_with(b"<?xml ") {
        let mut i = pos + 6;
        let mut quote: Option<u8> = None;
        while i < bytes.len() {
            let ch = bytes[i];
            if ch == b'<' {
                // Never legal inside the declaration: stop and re-examine it.
                break;
            }
            match quote {
                Some(q) if ch == q => quote = None,
                None if ch == b'"' || ch == b'\'' => quote = Some(ch),
                None if bytes[i..].starts_with(b"?>") => {
                    i += 2;
                    break;
                }
                Some(_) | None => {}
            }
            i += 1;
        }
        pos = i;
    }
    let mut depth = 0usize;
    let mut ns = NamespaceScope {
        max_in_scope: max_namespaces,
        ..NamespaceScope::default()
    };
    let mut decls: Vec<&[u8]> = Vec::new();
    while pos < bytes.len() {
        if bytes[pos] != b'<' {
            pos += 1;
            continue;
        }
        let rest = &bytes[pos..];
        if rest.starts_with(b"</") {
            depth = depth.saturating_sub(1);
            ns.close();
            pos += 2;
        } else if rest.starts_with(b"<!--") {
            pos = skip_past(text, pos + 4, "-->");
        } else if rest.starts_with(b"<![CDATA[") {
            pos = skip_past(text, pos + 9, "]]>");
        } else if rest.starts_with(b"<?") {
            pos = skip_past(text, pos + 2, "?>");
        } else if rest.starts_with(b"<!") {
            // DOCTYPE (DTDs are disabled) or an unknown `<!` token: roxmltree
            // rejects the document right here. Step over `<!` only, so nothing
            // after it is hidden from the scan.
            pos += 2;
        } else {
            let tag = rest;
            let (end, closed) = scan_start_tag(tag, &mut decls)?;
            // Self-closing `<tag/>` opens and closes, so it adds no depth. An
            // unterminated tag conservatively counts as an open.
            let self_closing = closed && end >= 2 && tag[end - 1] == b'/';
            ns.open(&decls)?;
            if self_closing {
                ns.close();
            } else {
                depth += 1;
                if depth > max_depth {
                    return Err(format!(
                        "XML nesting exceeds the maximum supported depth ({max_depth})"
                    ));
                }
            }
            // Past the `>`; or, when the scan stopped AT a `<`, re-examine it.
            pos += if closed { end + 1 } else { end };
        }
    }
    Ok(())
}

/// Scan a start tag (`tag` begins at its `<`) for [`xml_within_limits`]: returns
/// the offset where the scan stopped and whether that is the closing `>`, and
/// fills `decls` with the tag's namespace declarations.
///
/// Finds the closing `>` respecting quoted attribute values (a `>` inside
/// `"…"`/`'…'` isn't the tag end). Counts attributes by the `=` signs OUTSIDE
/// quotes: XML requires quoted values, so each attribute contributes exactly one
/// unquoted `=`, and a `=` inside a value is skipped with the quote run. A `<`
/// ends the scan even inside quotes (roxmltree rejects it there), so a runaway
/// quote cannot hide the elements after it. Namespace declarations are the
/// attributes NAMED `xmlns` / `xmlns:prefix`: the name is read back from each
/// unquoted `=` (XML allows blanks around it).
fn scan_start_tag<'a>(
    tag: &'a [u8],
    decls: &mut Vec<&'a [u8]>,
) -> std::result::Result<(usize, bool), String> {
    let mut end = 1usize;
    let mut quote: Option<u8> = None;
    let mut attrs = 0usize;
    decls.clear();
    while end < tag.len() {
        let ch = tag[end];
        if ch == b'<' {
            break;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
        } else if ch == b'"' || ch == b'\'' {
            quote = Some(ch);
        } else if ch == b'=' {
            attrs += 1;
            // Bail the instant the cap is exceeded, so a pathological single
            // tag (up to `MAX_ANALYZE_BYTES`) can't even make the pre-scan read
            // to its end -- work stays bounded by the cap, not the tag size.
            if attrs > MAX_XML_ATTRS_PER_ELEMENT {
                return Err(format!(
                    "an XML element has more than the maximum supported number of \
                     attributes ({MAX_XML_ATTRS_PER_ELEMENT})"
                ));
            }
            if let Some(prefix) = xmlns_prefix(&tag[..end]) {
                decls.push(prefix);
            }
        } else if ch == b'>' {
            return Ok((end, true));
        }
        end += 1;
    }
    Ok((end, false))
}

/// The namespace prefix (`b""` for the default namespace) when the attribute
/// whose `=` ends `tag_to_eq` is a namespace declaration (`xmlns` /
/// `xmlns:prefix`), read back over optional blanks and the name.
fn xmlns_prefix(tag_to_eq: &[u8]) -> Option<&[u8]> {
    let is_blank = |c: u8| matches!(c, b' ' | b'\t' | b'\r' | b'\n');
    let name_end = tag_to_eq.iter().rposition(|&c| !is_blank(c))? + 1;
    let name_start = tag_to_eq[..name_end]
        .iter()
        .rposition(|&c| is_blank(c) || matches!(c, b'"' | b'\'' | b'<' | b'/' | b'='))
        .map_or(0, |i| i + 1);
    let name = &tag_to_eq[name_start..name_end];
    if name == b"xmlns" {
        Some(b"")
    } else {
        name.strip_prefix(b"xmlns:")
    }
}

/// The namespace bindings in scope during [`xml_within_limits`]'s scan, mirroring
/// what roxmltree 0.20 keeps per element: the DISTINCT prefixes bound by the open
/// elements (a redeclared prefix shadows, it doesn't add), and the cumulative
/// cost of its per-declaring-element resolution (see
/// [`MAX_XML_NAMESPACE_WORK`]).
#[derive(Default)]
struct NamespaceScope<'a> {
    /// Open-element bindings per prefix (a redeclaration nests).
    bound: std::collections::HashMap<&'a [u8], u32>,
    /// Every declaration of every open element, in document order.
    decls: Vec<&'a [u8]>,
    /// Per open element, how many entries of `decls` it pushed.
    open: Vec<usize>,
    work: usize,
    /// [`MAX_XML_NAMESPACES_IN_SCOPE`] in production.
    max_in_scope: usize,
}

impl<'a> NamespaceScope<'a> {
    /// An element opens with `own` declarations: bind them and charge roxmltree's
    /// resolution (each inherited binding copied after a scan of the element's
    /// own range), failing past either cap.
    fn open(&mut self, own: &[&'a [u8]]) -> std::result::Result<(), String> {
        self.open.push(own.len());
        if own.is_empty() {
            return Ok(());
        }
        let inherited = self.bound.len();
        for &prefix in own {
            *self.bound.entry(prefix).or_insert(0) += 1;
            self.decls.push(prefix);
        }
        if self.bound.len() > self.max_in_scope {
            return Err(format!(
                "an XML element has more than the maximum supported number of \
                 namespace bindings in scope ({})",
                self.max_in_scope
            ));
        }
        self.work += (inherited + 1) * (inherited + own.len());
        if self.work > MAX_XML_NAMESPACE_WORK {
            return Err(
                "the XML document declares namespaces on too many elements to \
                 resolve within the supported cost"
                    .to_string(),
            );
        }
        Ok(())
    }

    /// The innermost open element closes: unbind its declarations. (A stray
    /// close with nothing open is a no-op, like the depth count's.)
    fn close(&mut self) {
        let Some(n) = self.open.pop() else {
            return;
        };
        for prefix in self.decls.drain(self.decls.len() - n..) {
            if let Some(count) = self.bound.get_mut(prefix) {
                *count -= 1;
                if *count == 0 {
                    self.bound.remove(prefix);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
