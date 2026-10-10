//! Lexer-parity gates for the XML nesting pre-scan (`xml_within_parse_limits`):
//! every opaque-region boundary is probed against the real roxmltree parser.

use super::*;
use crate::structured_format::Format;
use std::fmt::Write as _;

#[test]
fn xml_depth_scan_does_not_count_comments_cdata_or_self_closing() {
    // The pre-scan must not over-count: comment/CDATA contents and
    // self-closing tags don't add nesting, so valid shallow docs pass.
    assert!(
        xml_within_parse_limits(
            "<r><!-- <a><a><a> --><c/><![CDATA[ <b><b> ]]><d attr=\"x>y\"/></r>"
        )
        .is_ok()
    );
    // A genuinely deep run is rejected with a depth message.
    let deep = format!("{}{}", "<a>".repeat(300), "</a>".repeat(300));
    let err = xml_within_parse_limits(&deep).unwrap_err();
    assert!(err.contains("depth"), "depth rejection: {err}");
    // Real manifest depth is fine.
    assert!(xml_within_parse_limits(
        "<Project><PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup></Project>"
    )
    .is_ok());
}

/// Parse `doc` with the REAL roxmltree on a huge stack (so a deep but
/// legal document survives) and return its maximum element depth, or
/// `None` when roxmltree rejects it.
fn roxmltree_max_depth(doc: &str) -> Option<usize> {
    let doc = doc.to_owned();
    std::thread::Builder::new()
        .stack_size(512 * 1024 * 1024)
        .spawn(move || {
            let parsed = roxmltree::Document::parse(&doc).ok()?;
            parsed
                .descendants()
                .filter(roxmltree::Node::is_element)
                .map(|n| n.ancestors().filter(roxmltree::Node::is_element).count())
                .max()
        })
        .expect("spawn big-stack probe thread")
        .join()
        .expect("probe thread")
}

#[test]
fn xml_depth_scan_lexer_agrees_with_roxmltree_at_every_skip_boundary() {
    // GATE for the depth pre-scan's lexer parity: each fixture hides a
    // `</a>` inside a region roxmltree treats as opaque (a PI runs to `?>`,
    // not the first `>`; a comment's `-->` search starts AFTER `<!--`, so
    // `<!--->` does not close it). If the pre-scan's view of where that
    // region ends disagrees with roxmltree's, the hidden `</a>` cancels
    // each real `<a>` and a document far past `MAX_XML_DEPTH` slips through
    // to a recursive parse that ABORTS the process at scale. Each fixture is
    // probed against the real parser to prove it is legal and over-deep.
    let n = MAX_XML_DEPTH + 72;
    let fixtures = [
        ("pi-with-gt", "<a><?p ></a>?>"),
        ("pi-gt-only", "<a><?p >></a>?>"),
        ("comment-dash-gt", "<a><!---></a>-->"),
        ("cdata-gt", "<a><![CDATA[ ]> </a> ]]>"),
        ("comment-gt", "<a><!-- > </a> -->"),
    ];
    for (name, unit) in fixtures {
        let doc = format!("<r>{}{}</r>", unit.repeat(n), "</a>".repeat(n));
        let real = roxmltree_max_depth(&doc)
            .unwrap_or_else(|| panic!("{name}: fixture must be legal XML"));
        assert!(
            real > MAX_XML_DEPTH,
            "{name}: fixture is over-deep ({real})"
        );
        assert!(
            xml_within_parse_limits(&doc).is_err(),
            "{name}: the pre-scan must reject a {real}-deep document"
        );
    }
    // The XML declaration's attribute values may legally hold `?>`; the
    // scan must not end the declaration there (and must still accept it).
    let decl = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><r a=\"?>\"><b/></r>";
    assert!(roxmltree_max_depth(decl).is_some());
    assert!(xml_within_parse_limits(decl).is_ok());
}

#[test]
fn xml_pi_hidden_close_bomb_is_a_parse_error_not_an_abort() {
    // The reported reproducer at full scale: 100 000 `<a>` levels whose
    // closes are each hidden in a `<?p ></a>?>` PI. It must surface as one
    // ordinary parse error, never a stack-overflow abort (exit 134).
    let n = 100_000;
    let doc = format!("<r>{}{}</r>", "<a><?p ></a>?>".repeat(n), "</a>".repeat(n));
    let err = Format::Xml.parse(&doc).unwrap_err();
    assert!(err.contains("depth"), "rejected by the depth guard: {err}");
}

#[test]
fn xml_depth_scan_never_hides_markup_behind_a_runaway_quote() {
    // roxmltree rejects `<` anywhere inside a tag (even in a quoted value),
    // so an unterminated quote must not let the pre-scan skip the elements
    // that follow it: every `<a>` after it stays visible and counted.
    let doc = format!("<r b=\"{}", "<a>".repeat(MAX_XML_DEPTH + 10));
    assert!(xml_within_parse_limits(&doc).is_err());
}

proptest::proptest! {
    /// Lexer parity as a property: for any document roxmltree ACCEPTS, the
    /// pre-scan never sees it as shallower than the real tree -- probing the
    /// scan with a ceiling one below the true depth must reject. Fragments
    /// are biased toward every opaque-region boundary (PI, comment, CDATA,
    /// quotes, the XML declaration) so a boundary mismatch that hides a
    /// close tag surfaces as a shrunk counterexample.
    #[test]
    fn xml_depth_scan_never_undercounts_a_document_roxmltree_accepts(
        frags in proptest::collection::vec(
            proptest::sample::select(vec![
                "<a>", "</a>", "<a/>", "<a b=\"", "\">", "'", "\"", "<?p ", "?>",
                ">", "-", "<!--", "-->", "<![CDATA[", "]]>", "]", "?", "x", " ",
                "</a>", "<a>", "<a c='>'>", "<?xml version=\"1.0\"?>",
            ]),
            0..40,
        )
    ) {
        let doc = format!("<r>{}</r>", frags.concat());
        if let Ok(parsed) = roxmltree::Document::parse(&doc) {
            // roxmltree's RECURSION depth (what overflows): every enclosing
            // element, plus the element itself when it has content (a
            // childless element may be `<a/>`, which does not recurse).
            let real = parsed
                .descendants()
                .filter(roxmltree::Node::is_element)
                .map(|n| {
                    let with_self = n.ancestors().filter(roxmltree::Node::is_element).count();
                    if n.has_children() { with_self } else { with_self - 1 }
                })
                .max()
                .unwrap_or(0);
            if real > 0 {
                proptest::prop_assert!(
                    xml_within_limits(&doc, real - 1, MAX_XML_NAMESPACES_IN_SCOPE).is_err(),
                    "scan under-counts {real}-deep doc: {doc}"
                );
            }
        }
    }
}

/// `prefixes` namespace declarations, `xmlns:{tag}_0="u"` ...
fn xmlns_decls(tag: &str, prefixes: usize) -> String {
    let mut out = String::new();
    for i in 0..prefixes {
        let _ = write!(out, " xmlns:{tag}_{i}=\"u\"");
    }
    out
}

/// `levels` nested `<eN …decls…>` opens around `inner`, closed again.
fn nested_decls(levels: usize, decls: impl Fn(usize) -> String, inner: &str) -> String {
    let mut out = String::new();
    for d in 0..levels {
        let _ = write!(out, "<e{d}{}>", decls(d));
    }
    out.push_str(inner);
    for d in (0..levels).rev() {
        let _ = write!(out, "</e{d}>");
    }
    out
}

#[test]
fn xml_namespace_heavy_document_is_rejected_before_roxmltree_resolves_it() {
    // Regression: roxmltree 0.20 resolves namespaces in O(P^2) per element that
    // declares any `xmlns`, in the P bindings in scope. 100 nested elements each
    // binding 256 prefixes, then ten `<x xmlns:z="u"/>` leaves (448 KB, inside
    // the depth + attribute caps) took 32-40 s to parse.
    let doc = nested_decls(
        100,
        |d| xmlns_decls(&format!("p{d}"), 256),
        &"<x xmlns:z=\"u\"/>".repeat(10),
    );
    let err = Format::Xml.parse(&doc).unwrap_err();
    assert!(err.contains("namespace"), "rejected pre-parse: {err}");
}

#[test]
fn xml_repeated_namespace_declarations_are_bounded_document_wide() {
    // The in-scope cap bounds ONE element; a max-size file can repeat the
    // declaring element ~1.5M times (64 bindings in scope over 1.5M
    // `<x xmlns:z="u"/>` children: 9.4 s). The work budget bounds the total:
    // here 130K such children under 64 bindings (~2.3 MB) are rejected.
    let doc = format!(
        "<r{}>{}</r>",
        xmlns_decls("p", 64),
        "<x xmlns:z=\"u\"/>".repeat(130_000)
    );
    let err = xml_within_parse_limits(&doc).unwrap_err();
    assert!(err.contains("namespaces"), "{err}");
    // Ordinary heavy namespace use passes: a generated SOAP / .NET-serializer
    // style document redeclaring the default namespace (plus xsi/xsd) on every
    // one of 200K elements, and a Word `document.xml`-style root binding 40
    // prefixes over 50K `mc:AlternateContent`-style declaring blocks.
    let soap = format!(
        "<s:Envelope xmlns:s=\"urn:soap\"><s:Body>{}</s:Body></s:Envelope>",
        "<Item xmlns=\"urn:x\" xmlns:xsi=\"urn:xsi\" xmlns:xsd=\"urn:xsd\"><v>1</v></Item>"
            .repeat(200_000)
    );
    assert!(xml_within_parse_limits(&soap).is_ok());
    let docx = format!(
        "<w_0:document{}><w_0:body>{}</w_0:body></w_0:document>",
        xmlns_decls("w", 40),
        "<mc:AlternateContent xmlns:mc=\"urn:mc\"><mc:Choice Requires=\"w_1\"/></mc:AlternateContent>"
            .repeat(50_000)
    );
    assert!(xml_within_parse_limits(&docx).is_ok());
}

#[test]
fn xml_namespace_count_matches_roxmltree_at_the_cap() {
    // The scan counts DISTINCT prefixes, as roxmltree does: a prefix
    // redeclared at every level shadows rather than adds, so 100 levels each
    // redeclaring the same 128 prefixes is exactly 128 in scope -- accepted,
    // and it parses quickly -- while one more distinct prefix is rejected.
    let at = MAX_XML_NAMESPACES_IN_SCOPE;
    let doc = nested_decls(
        100,
        |_| xmlns_decls("p", at),
        &"<x xmlns:p_0=\"v\"/>".repeat(100),
    );
    let parsed = roxmltree::Document::parse(&doc).unwrap();
    let real = parsed
        .descendants()
        .map(|n| n.namespaces().count())
        .max()
        .unwrap();
    assert_eq!(real, at);
    assert!(xml_within_parse_limits(&doc).is_ok());
    let t = std::time::Instant::now();
    assert!(Format::Xml.parse(&doc).is_ok());
    assert!(
        t.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        t.elapsed()
    );
    let over = doc.replacen("<e0", "<e0 xmlns:extra=\"u\"", 1);
    assert!(xml_within_parse_limits(&over).is_err());
    // Default-namespace and blank-around-`=` declarations count too.
    let blanks = format!("<r{} xmlns = \"u\"/>", xmlns_decls("q", at));
    assert!(xml_within_parse_limits(&blanks).is_err());
    let blanks = format!("<r{} xmlns:z\n=\n'u'/>", xmlns_decls("q", at));
    assert!(xml_within_parse_limits(&blanks).is_err());
}

proptest::proptest! {
    /// Namespace-count parity as a property: for any document roxmltree
    /// ACCEPTS, the scan never sees fewer bindings in scope than roxmltree
    /// resolves at some element -- probing with a ceiling one below must
    /// reject. Fragments mix declarations, redeclarations, defaults, blanks
    /// around `=`, and every opaque region that could hide or fake one.
    #[test]
    fn xml_namespace_scan_never_undercounts_a_document_roxmltree_accepts(
        frags in proptest::collection::vec(
            proptest::sample::select(vec![
                "<a>", "</a>", "<a/>", "<a xmlns:p=\"u\">", "<a xmlns:q='u'>",
                "<a xmlns=\"u\">", "<a xmlns:p = \"v\">", "<a\nxmlns:r\t=\n'u'/>",
                "<p:a xmlns:p=\"u\" xmlns:q=\"u\"/>", "<a b=\"xmlns:s='u'\">",
                "<!-- <a xmlns:t=\"u\"> -->", "<?p xmlns:u=\"u\" ?>",
                "<![CDATA[<a xmlns:v=\"u\">]]>", "<a xmlns:p=\"u\" c='>'>",
                "x", " ", "\"", "'", ">", "=", "<a xmlns:w=\"u\" xmlns:x=\"u\">",
            ]),
            0..30,
        )
    ) {
        let doc = format!("<r>{}</r>", frags.concat());
        if let Ok(parsed) = roxmltree::Document::parse(&doc) {
            let real = parsed
                .descendants()
                .map(|n| n.namespaces().count())
                .max()
                .unwrap_or(0);
            if real > 0 {
                proptest::prop_assert!(
                    xml_within_limits(&doc, usize::MAX, real - 1).is_err(),
                    "scan under-counts {real} namespaces: {doc}"
                );
            }
        }
    }
}
