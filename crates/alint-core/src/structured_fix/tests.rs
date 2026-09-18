//! Tests for the structured-fix span resolvers + serializers.
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
        resolve_removal_span(Format::Hcl, src.as_bytes(), &[key("tags"), key("Team")],).is_none()
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
    let span = resolve_removal_span(Format::Xml, src.as_bytes(), &[key("c"), key("drop")]).unwrap();
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
    assert!(resolve_value_span(Format::Dotenv, src.as_bytes(), &[key("A"), key("B")]).is_none());
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
    let removal = resolve_removal_span(Format::Dotenv, src.as_bytes(), &[key("DBURL")]).unwrap();
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
    let span = resolve_removal_span(Format::Ini, src.as_bytes(), &[key("s"), key("drop")]).unwrap();
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
    assert!(resolve_value_span(Format::Ini, src.as_bytes(), &[key("tox"), key("deps")]).is_none());
    let span =
        resolve_removal_span(Format::Ini, src.as_bytes(), &[key("tox"), key("deps")]).unwrap();
    assert_eq!(&src[span], "deps =\n    pytest\n    mock\n");
    // a single-line sibling in the same section still resolves for set.
    assert!(resolve_value_span(Format::Ini, src.as_bytes(), &[key("tox"), key("other")]).is_some());
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
    assert!(resolve_removal_span(Format::Ini, same.as_bytes(), &[key("s"), key("k")]).is_none());
    let split = "[s]\nk = 1\n[o]\nx = 0\n[s]\nk = 2\n";
    assert!(resolve_removal_span(Format::Ini, split.as_bytes(), &[key("s"), key("k")]).is_none());
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
    assert!(document_remove(Format::Toml, b"a = [1, 2]\n", &[vec![key("a"), idx(0)]]).is_none());
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

#[test]
fn toml_finalize_preserves_crlf_bom_and_trailing_newline() {
    // toml_edit's reserialize normalizes CRLF->LF, strips a leading BOM, and
    // appends a trailing newline; `finalize` restores all three so a
    // whole-document rewrite stays byte-surgical (only the edit differs).
    let set = |src: &str, p: &[PathSeg], v: &serde_json::Value| {
        String::from_utf8(document_set(Format::Toml, src.as_bytes(), p, v).unwrap()).unwrap()
    };
    // CRLF preserved (set + remove).
    assert_eq!(
        set(
            "[s]\r\nport = 8080\r\n",
            &[key("s"), key("port")],
            &json!(9090)
        ),
        "[s]\r\nport = 9090\r\n"
    );
    assert_eq!(
        String::from_utf8(
            document_remove(
                Format::Toml,
                "[s]\r\nkeep = 1\r\ndrop = 2\r\n".as_bytes(),
                &[vec![key("s"), key("drop")]]
            )
            .unwrap()
        )
        .unwrap(),
        "[s]\r\nkeep = 1\r\n"
    );
    // BOM preserved.
    assert_eq!(
        set("\u{feff}port = 8080\n", &[key("port")], &json!(9090)),
        "\u{feff}port = 9090\n"
    );
    // A missing trailing newline stays missing.
    assert_eq!(
        set("port = 8080", &[key("port")], &json!(9090)),
        "port = 9090"
    );
    // A pure-LF file is untouched (no spurious CRLF).
    assert_eq!(
        set("port = 8080\n", &[key("port")], &json!(9090)),
        "port = 9090\n"
    );
}
