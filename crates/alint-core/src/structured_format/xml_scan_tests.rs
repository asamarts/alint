//! Lexer-parity gates for the XML nesting pre-scan (`xml_within_parse_limits`):
//! every opaque-region boundary is probed against the real roxmltree parser.

use super::*;

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
                    xml_within_limits(&doc, real - 1).is_err(),
                    "scan under-counts {real}-deep doc: {doc}"
                );
            }
        }
    }
}
