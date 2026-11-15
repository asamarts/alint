use super::*;

/// The byte prefilters must never change a verdict: compare them with the
/// lexer walks they short-circuit, over every string up to length 4 on an
/// alphabet of YAML's structural characters.
#[test]
fn prefilters_agree_with_the_lexer() {
    fn flow_reference(text: &str, max: usize) -> bool {
        let mut within = true;
        lex(text, |tok| match tok {
            Tok::FlowOpen(depth) if depth > max => {
                within = false;
                false
            }
            _ => true,
        });
        within
    }
    fn alias_reference(text: &str) -> bool {
        let mut found = false;
        lex(text, |tok| {
            if matches!(tok, Tok::Alias) {
                found = true;
            }
            !found
        });
        found
    }
    const ALPHABET: [char; 14] = [
        'a', ' ', '\n', ':', '-', '[', ']', '{', '}', '*', '&', '#', '\'', '"',
    ];
    let mut idx = [0usize; 4];
    let mut checked = 0usize;
    for len in 0..=4 {
        idx.fill(0);
        loop {
            let text: String = idx[..len].iter().map(|&i| ALPHABET[i]).collect();
            for max in [0, 1] {
                assert_eq!(
                    flow_depth_within_limit_with(&text, max),
                    flow_reference(&text, max),
                    "{text:?} max={max}"
                );
            }
            assert_eq!(contains_alias(&text), alias_reference(&text), "{text:?}");
            let scan = scan_guards_with(&text, 1);
            assert_eq!(
                scan.flow_within_limit(),
                flow_reference(&text, 1),
                "{text:?}"
            );
            if scan.flow_within_limit() {
                assert_eq!(scan.has_alias, alias_reference(&text), "{text:?}");
            }
            checked += 1;
            let mut pos = 0;
            while pos < len {
                idx[pos] += 1;
                if idx[pos] < ALPHABET.len() {
                    break;
                }
                idx[pos] = 0;
                pos += 1;
            }
            if pos == len {
                break;
            }
        }
    }
    assert!(checked > 40_000, "{checked}");
}

#[test]
fn shallow_and_realistic_yaml_passes() {
    assert!(flow_depth_within_limit(
        "a: [1, 2, [3, {b: 4}]]\nc:\n  - x\n  - y\n"
    ));
    // Plain scalars containing brackets must NOT be counted as flow.
    assert!(flow_depth_within_limit(&"k: value[0][1]\n".repeat(5000)));
    // Brackets inside quoted scalars don't count.
    assert!(flow_depth_within_limit(&"k: \"[[[[[[[[[[\"\n".repeat(5000)));
}

#[test]
fn deep_flow_nesting_is_rejected() {
    let bomb = format!("x: {}{}", "[".repeat(5000), "]".repeat(5000));
    assert!(!flow_depth_within_limit(&bomb));
    // With content between the opens (`[1,[1,[1,…`) it still nests.
    let bomb2 = format!("x: {}1{}", "[1,".repeat(2000), "]".repeat(2000));
    assert!(!flow_depth_within_limit(&bomb2));
    // Curly flow maps too.
    let bomb3 = format!("x: {}1{}", "{a: ".repeat(2000), "}".repeat(2000));
    assert!(!flow_depth_within_limit(&bomb3));
}

#[test]
fn flow_depth_limit_matches_the_parsers_depth_limit() {
    // The ceiling sits exactly at the parser's depth limit: the deepest
    // flow document it can deserialize passes the guard and parses to the
    // same value as without the guard, and one level deeper is a document
    // the parser could never deserialize (so the guard adds no new false
    // reject). A regression to a looser ceiling (1024 before) fails the
    // second half.
    let at = MAX_YAML_FLOW_DEPTH;
    let ok = bomb(at);
    assert!(flow_depth_within_limit(&ok));
    let parsed = crate::yaml::ayaml_from_str::<serde_json::Value>(&ok).unwrap();
    assert_eq!(crate::Format::Yaml.parse(&ok).unwrap(), parsed);
    let over = bomb(at + 1);
    assert!(crate::yaml::ayaml_from_str::<serde_json::Value>(&over).is_err());
    assert!(crate::yaml::ayaml_from_str::<ayaml::Value>(&over).is_err());
    assert!(!flow_depth_within_limit(&over));
    // Curly flow maps count the same way.
    let maps = |n: usize| format!("{}1{}", "{a: ".repeat(n), "}".repeat(n));
    assert!(crate::yaml::ayaml_from_str::<serde_json::Value>(&maps(at)).is_ok());
    assert!(flow_depth_within_limit(&maps(at)));
    assert!(crate::yaml::ayaml_from_str::<serde_json::Value>(&maps(at + 1)).is_err());
    assert!(!flow_depth_within_limit(&maps(at + 1)));
}

#[test]
fn contains_alias_detects_aliases_not_globs() {
    assert!(contains_alias("x: *anchor\n"));
    assert!(contains_alias("  - <<: *base\n"));
    assert!(contains_alias("k:\n  - *a\n  - *a\n"));
    // A quoted glob is not an alias.
    assert!(!contains_alias("paths:\n  - \"**/*.rs\"\n"));
    assert!(!contains_alias("p: '*.rs'\n"));
    // Multiplication (space after `*`) and comments are not aliases.
    assert!(!contains_alias("expr: 2 * 3\n"));
    assert!(!contains_alias("# a comment mentioning *stars\n"));
    assert!(!contains_alias("plain: value\nother: 42\n"));
}

#[test]
fn alias_hidden_behind_plain_scalar_quote_is_still_detected() {
    // A plain scalar containing a quote is valid, extremely common YAML. The
    // alias detector must not treat that quote as a string delimiter and skip
    // past a real `*alias` after it -- doing so would bypass the budget pass and
    // re-open the alias-bomb DoS. Both quote flavors, both "unclosed to EOF" and
    // "a later quote pairs across the alias" shapes.
    let cases = [
        "desc: it's fine\nanchor: &a [1, 2, 3]\nuse: *a\n",
        "size: 12\" wide\nanchor: &a [1, 2, 3]\nuse: *a\n",
        "a: it's\nb: &x [1]\nc: *x\nd: 'closed'\n",
        "a: 12\" x\nb: &x [1]\nc: *x\ntail: \"y\"\n",
    ];
    for case in cases {
        // Sanity: each case is genuinely parseable YAML (so the bomb is real).
        let parsed: Result<ayaml::Value, _> = crate::yaml::ayaml_from_str(case);
        assert!(parsed.is_ok(), "case must be valid YAML: {case:?}");
        assert!(
            contains_alias(case),
            "alias must not be hidden behind a plain-scalar quote: {case:?}"
        );
    }
    // A GENUINE quoted scalar containing `*` is correctly skipped (no alias).
    assert!(!contains_alias("pattern: \"*.rs\"\nother: '*.md'\n"));
    assert!(!contains_alias("- \"a*b\"\n- '*'\n"));
    // A genuine quoted scalar AFTER an anchor/tag is still recognized (skipped).
    assert!(!contains_alias("k: &a \"*.rs\"\n"));
    assert!(!contains_alias("k: !!str '*.md'\n"));
    // But a real alias that references an anchored node is still detected.
    assert!(contains_alias("base: &b [1]\nuse: *b\n"));
    // End-to-end: a real bomb hidden behind an innocuous apostrophe line must be
    // rejected (previously slipped through the short-circuit).
    let anchor: String = (0..200).map(|_| "0,".to_string()).collect();
    let refs: String = (0..500).map(|_| "  - *a\n".to_string()).collect();
    let hidden = format!("desc: it's fine\nanchor: &a [{anchor}]\nrefs:\n{refs}");
    assert!(
        !expansion_within_limit_with(&hidden, 10_000 * YAML_NODE_COST),
        "alias bomb hidden behind a plain-scalar quote must still be rejected"
    );
}

#[test]
fn flow_bomb_hidden_behind_plain_scalar_quote_is_still_rejected() {
    // A mid-plain-scalar quote must not let a deep flow collection on a later
    // line escape the depth count. `lead: it's` opens a bogus quote region in a
    // naive scanner that would swallow the `deep: [[[[...` bomb below it.
    let bomb = format!(
        "lead: it's\ndeep: {}1{}\n",
        "[".repeat(4000),
        "]".repeat(4000)
    );
    assert!(
        !flow_depth_within_limit(&bomb),
        "flow bomb after a mid-scalar quote must still be rejected"
    );
    let bomb2 = format!(
        "lead: 12\" x\ndeep: {}1{}\n",
        "[".repeat(4000),
        "]".repeat(4000)
    );
    assert!(!flow_depth_within_limit(&bomb2));
    // A genuine quoted scalar full of brackets must still NOT false-reject.
    assert!(flow_depth_within_limit(
        &"re: \"[[[[[[[[[[[[[[[[[[[[\"\n".repeat(3000)
    ));
    assert!(flow_depth_within_limit(&"re: '[[[[[[[[[[' \n".repeat(3000)));
    // Anchored / tagged quoted scalars full of brackets must NOT false-reject
    // (the quote still opens at a node position after `&a` / `!!str`).
    assert!(flow_depth_within_limit(
        &"k: &a \"[[[[[[[[[[\"\n".repeat(2000)
    ));
    assert!(flow_depth_within_limit(
        &"k: !!str \"[[[[[[[[[[\"\n".repeat(2000)
    ));
    // But an anchored FLOW bomb is still caught.
    let anchored = format!("k: &a {}1{}\n", "[".repeat(4000), "]".repeat(4000));
    assert!(!flow_depth_within_limit(&anchored));
}

/// `true` when the REAL parser sees nesting past its 128-level depth limit
/// -- i.e. the fixture's bracket run is genuine structure, not text hidden
/// inside a scalar or comment.
fn parser_sees_deep_nesting(doc: &str) -> bool {
    crate::yaml::ayaml_from_str::<ayaml::Value>(doc)
        .err()
        .is_some_and(|e| e.to_string().contains("recursion limit"))
}

#[test]
fn self_referencing_alias_is_not_mistaken_for_a_flow_bomb() {
    // Proptest counterexample (windows CI, with the parser v0.22 used): `&a`
    // anchored a mapping holding `*a`, which that parser reported as
    // "recursion limit" at any bracket depth, though the brackets sat in a
    // double-quoted scalar the scanner rightly skips. ayaml names the cycle
    // instead, so the depth oracle cannot blame the brackets for it.
    let cycle = crate::yaml::ayaml_from_str::<ayaml::Value>("&a [*a]\n")
        .unwrap_err()
        .to_string();
    assert!(
        cycle.contains("an alias cannot refer to a collection that contains it"),
        "{cycle}"
    );
    assert!(!parser_sees_deep_nesting("&a [*a]\n"));
    // The reported shape: ayaml stops at the quoted scalar's continuation
    // line, whatever the bracket depth after it.
    let doc = |b: &str| format!("&a \n*a : \"\n{b}\nc: \"\"\n");
    let error = |b: &str| {
        crate::yaml::ayaml_from_str::<ayaml::Value>(&doc(b))
            .unwrap_err()
            .to_string()
    };
    assert_eq!(
        error(&bomb(300)),
        "quoted scalar continuation must be indented more than its parent at line 3 column 1"
    );
    assert_eq!(error(&bomb(300)), error(&bomb(1)));
    assert!(flow_depth_within_limit_with(&doc(&bomb(300)), 200));
}

/// `true` when the REAL parser sees a genuine alias token (an undefined
/// `*undefined_zz` reference is reported as an unknown anchor).
fn parser_sees_alias(doc: &str) -> bool {
    crate::yaml::ayaml_from_str::<ayaml::Value>(doc)
        .err()
        .is_some_and(|e| {
            let e = e.into_parser();
            let message = e.to_string();
            message.contains("unknown anchor")
                || (matches!(e.kind(), ayaml::ErrorKind::Limit) && message.contains("alias"))
        })
}

fn bomb(n: usize) -> String {
    format!("{}1{}", "[".repeat(n), "]".repeat(n))
}

#[test]
fn flow_bomb_hidden_by_a_lexer_desync_is_still_rejected() {
    // GATE for the scanner's lexer parity with the parser. Each fixture hides
    // a deep flow collection behind a construct a naive scanner mis-lexes
    // into a quoted scalar or comment running past the bomb. Each is probed
    // against the real parser so the bomb is proven to be structure it nests.
    let n = MAX_YAML_FLOW_DEPTH + 76;
    let b = bomb(n);
    let fixtures = [
        // `#` comment inside a flow collection (a `"` in it is comment text).
        ("flow-comment", format!("b: [ #\"\n{b}\n ]\n")),
        // `#` right at a flow token start is a comment too.
        ("flow-comment-no-space", format!("b: [#\"\n{b}\n ]\n")),
        // A block scalar line starting with `"` is content, not a string.
        ("block-scalar", format!("s: |\n  \"\nb: {b}\nc: \"\"\n")),
        ("folded-chomp", format!("s: >-\n  \"\nb: {b}\nc: \"\"\n")),
        ("explicit-indent", format!("s: |2\n  \"\nb: {b}\nc: \"\"\n")),
        (
            "block-in-seq-map",
            format!("- k: |\n    \"\n  b: {b}\n  c: \"\"\n"),
        ),
        // A block scalar with no content ends at a line its key's column
        // indents, a sibling key here.
        (
            "block-scalar-then-a-sibling-key",
            format!("a:\n  k: |\n  b: {b}\n"),
        ),
        // A plain scalar CONTINUATION line starting with `"` is content.
        (
            "plain-continuation",
            format!("k: some text\n  \"more\nb: {b}\nz: \"x\"\n"),
        ),
        // A `"` mid-plain-scalar inside a flow collection is content.
        ("flow-mid-plain", format!("a: [x\"y, {b}, z\"w]\n")),
        // `-"` / `?"` / `:"` with no space are plain scalars, not indicators.
        ("dash-quote", format!("a: -\"\nb: {b}\nc: \"\"\n")),
        ("colon-quote", format!("a: x:\"\nb: {b}\nc: \"\"\n")),
        // Outside a flow collection, so does `?` before anything but a
        // blank, and a `#` in that scalar is content.
        ("question-mark-plain-scalar", format!("?#k: {b}\n")),
        // In a flow collection too, a `:` before a quote is content
        // (YAML 1.2), so the quote opens no scalar.
        ("flow-colon-quote", format!("a: [x:\"y,\n{b}\n]\nc: \"\"\n")),
        // ...and so is a `:` that starts a node there.
        (
            "flow-colon-quote-at-a-node",
            format!("a: [:\"y,\n{b}\n]\nc: \"\"\n"),
        ),
        // A flow `?` is the key indicator whatever follows, so a `#` right
        // after it starts a comment that hides the `]`.
        ("flow-key-comment", format!("[?#]\\\n{b}\nc: \"\"\n")),
        // ...and ayaml ends that comment at a NEL, where every other
        // comment runs on to the line feed.
        ("flow-key-comment-nel", format!("[?#\u{85} {b}]\n")),
        // What follows a flow `?` and a blank is the key.
        ("flow-key-indicator", format!("[? {b}]\n")),
        // NEL is content (YAML 1.2), not a line break before a quote.
        (
            "nel-is-content",
            format!("a: [x,\u{85}\",\n{b}\n]\nc: \"\"\n"),
        ),
        // An anchor name holds any character but a blank and the flow
        // indicators, so a quote, `#`, `|` or `.` in it opens nothing.
        ("anchor-name-quote", format!("k: &a\" {b}\nc: \"\"\n")),
        ("anchor-name-single-quote", format!("k: &a' {b}\nc: ''\n")),
        ("anchor-name-hash", format!("k: &a# {b}\n")),
        ("anchor-name-block", format!("k: &a|\n  {b}\n")),
        ("anchor-name-dot", format!("k: &a. {b}\n")),
        // A name may start with `:`, and a `:` before a NEL, LS or PS
        // does not end it.
        ("anchor-name-colon-first", format!("&:x\n{b}\n")),
        ("anchor-colon-nel", format!("&a:\u{85} {b}\n")),
        ("anchor-colon-ls", format!("&a:\u{2028} {b}\n")),
        ("anchor-colon-ps", format!("&a:\u{2029} {b}\n")),
        // A tag holds a `#`, as a verbatim tag does, so no comment starts
        // there.
        ("tag-name-hash", format!("k: !a# {b}\n")),
        ("verbatim-tag-hash", format!("k: !<!a#b> {b}\n")),
        // A key candidate expires at the end of its line, so the `:` below
        // `? a` has no key: the plain scalar after it goes on at column 1.
        (
            "key-candidate-ends-with-its-line",
            format!("? a\n: b\n \"\nc: {b}\nd: \"\"\n"),
        ),
        // An alias name gives back a final `:` only, not a quote...
        ("alias-name-quote", format!("k: [*a', {b}]\n")),
        // ...and the `:` it gives back is a value indicator, so the block
        // scalar ends at the sibling key.
        (
            "alias-key-block-scalar",
            format!("x: &a 1\nk:\n  *a: |\n  b: {b}\n"),
        ),
        // ayaml's scanner ends the name `a` at a `:` before a flow
        // indicator, where its line reader reads on: the value then
        // continues past a column-0 quote.
        (
            "anchor-colon-flow-indicator",
            format!("&a:[: x\n\"\nb: {b}\nc: \"\"\n"),
        ),
        ("anchor-colon-comma", format!("&a:,: x\n'\nb: {b}\nc: ''\n")),
        // ayaml gives the byte order mark that starts the text no width and
        // reads a second one as content, the key here, at column 0, so the
        // `|2` scalar's content indent is 2.
        (
            "second-byte-order-mark",
            format!("\u{feff}\u{feff}  : |2\n  \"\nb: {b}\nc: \"\"\n"),
        ),
        // A `#` WITHOUT preceding space is plain-scalar content (not a comment).
        ("hash-in-plain", format!("a: b#c\nb: {b}\n")),
        // Single-quoted `''` escape does not end the scalar early.
        ("single-quote-escape", format!("a: 'it''s'\nb: {b}\n")),
    ];
    for (name, doc) in &fixtures {
        assert!(
            parser_sees_deep_nesting(doc),
            "{name}: fixture must nest for real in the parser"
        );
        assert!(
            !flow_depth_within_limit(doc),
            "{name}: the scanner must see the hidden flow bomb"
        );
    }
}

#[test]
fn scalar_and_comment_contents_never_false_reject() {
    // The other half of parity: brackets / quotes inside genuine scalars and
    // comments are not structure, so ordinary YAML must still pass.
    let deep = "[".repeat(MAX_YAML_FLOW_DEPTH + 50);
    let ok = [
        format!("s: |\n  {deep}\n  \"\nt: 1\n"),
        format!("s: >+\n  {deep}\n\nt: 1\n"),
        format!("- k: |\n    {deep}\n  j: 2\n"),
        format!("a: '{deep}'\nb: 'it''s {deep}'\n"),
        format!("a: \"\\\"{deep}\"\n"),
        format!("# {deep}\nk: v # {deep}\n"),
        format!("k: !!str \"{deep}\"\nj: &a \"{deep}\"\n"),
        format!("k: value{deep}\n"),
        format!("k: [a, \"{deep}\", '{deep}']\n"),
        // Names that end at a `:` before other text, or hold one, are read
        // one way only.
        format!("a: &x: '{deep}'\nb: *x:\nc: &y '{deep}'\nd: *y\n"),
        format!("\u{feff}a:\n\u{feff}  b: '{deep}'\n"),
        // An anchor's name keeps a final `:`, so the anchored scalar goes
        // on to the next line.
        format!("&a: x\n{deep}\n"),
        // `?` and `:` before anything but a blank start a plain scalar,
        // as does the `?#` after a name that a `?` ends.
        format!("?{deep}\n"),
        format!(":{deep}\n"),
        format!("&a?#\n{deep}\n"),
        // A key in a complex key holds a block scalar indented past it.
        format!("? k: |\n   {deep}\n"),
        // A comment after the byte order mark that starts the text.
        format!("\u{feff}# a: {deep}\n"),
        // Outside a flow collection a `:` after a quoted scalar is not a
        // value indicator before a `,`: saphyr's reading of what ayaml
        // reads as one scalar after a second byte order mark.
        format!("\u{feff}\u{feff}'':, {deep}\n"),
    ];
    for doc in &ok {
        assert!(
            crate::yaml::ayaml_from_str::<ayaml::Value>(doc).is_ok(),
            "fixture must be valid YAML: {doc:?}"
        );
        assert!(flow_depth_within_limit(doc), "false reject: {doc:?}");
    }
}

/// The known false refusals that `MAX_YAML_FLOW_DEPTH` documents: valid
/// documents where the scanner counts brackets ayaml reads as text.
#[test]
fn the_documented_false_refusals_still_refuse() {
    let deep = "[".repeat(MAX_YAML_FLOW_DEPTH + 72);
    for doc in [
        // After a name ayaml reads two ways, every later bracket counts.
        format!("- &a:, x\n- '{deep}'\n"),
        // A `...` after the leading byte order mark ends a document in the
        // reading that gives the mark no width; to ayaml it is text here.
        format!("\u{feff}... {deep}\n"),
    ] {
        assert!(
            crate::yaml::from_str::<ayaml::Value>(&doc).is_ok(),
            "{doc:?}"
        );
        assert!(!flow_depth_within_limit(&doc), "{doc:?}");
    }
}

#[test]
fn alias_hidden_by_a_lexer_desync_is_still_detected() {
    // The alias short-circuit must never miss a real `*alias`: a missed one
    // skips the expansion budget and re-opens the alias bomb (a block-scalar
    // line starting with `"` hid 50 000 `*a` refs -> 1.6 GB, 29 s).
    let fixtures = [
        "s: |\n  \"\nb: [*undefined_zz]\nc: \"\"\n",
        "b: [ #\"\n*undefined_zz ]\nc: \"\"\n",
        "k: some text\n  \"more\nb: *undefined_zz\nz: \"x\"\n",
        "a: [x\"y, *undefined_zz, z\"w]\n",
        "a: -\"\nb: *undefined_zz\nc: \"\"\n",
        "- k: >\n    \"\n  b: *undefined_zz\n  c: \"\"\n",
        "k: &x\" v\nb: *undefined_zz\nc: \"\"\n",
        "k: &x|\n  [*undefined_zz]\n",
        "k: !t# [*undefined_zz]\n",
        "? a\n: b\n \"\nc: *undefined_zz\nd: \"\"\n",
    ];
    for doc in fixtures {
        assert!(
            parser_sees_alias(doc),
            "fixture must hold a real alias: {doc:?}"
        );
        assert!(contains_alias(doc), "alias must be detected: {doc:?}");
    }
    // End to end: the reported billion-laughs shape is rejected.
    let anchor = "1,".repeat(1000);
    let refs = "*a,".repeat(50);
    let doc = format!("a: &a [{anchor}]\ns: |\n  \"\nb: [{refs}]\nc: \"\"\n");
    assert!(!expansion_within_limit_with(&doc, 10_000 * YAML_NODE_COST));
}

#[test]
fn leading_bom_does_not_desync_the_line_one_column() {
    // Regression (with the parser v0.22 used): the lexer stripped a leading
    // BOM without advancing the column, but libyaml-rs skipped it as a
    // column-0 character, so `\u{feff}---` was NOT a document marker to it.
    // The off-by-one hid everything after it inside a phantom single-quoted
    // scalar: a 60 000-deep flow bomb passed the guard (7.7 s to parse),
    // and an alias bomb skipped the expansion budget (3.5 GB). ayaml takes
    // the same column, and saphyr does not, so the lexer reads it both ways.
    // Reachable through config / `extends:` / `suggest` bodies, which are
    // guarded on the raw text (`Format::parse` strips BOMs first).
    let b = bomb(MAX_YAML_FLOW_DEPTH + 76);
    for boms in ["\u{feff}", "\u{feff}\u{feff}"] {
        let flow = format!("{boms}--- 'x: {b}\n'\n");
        assert!(
            crate::yaml::ayaml_from_str::<ayaml::Value>(&flow).is_err(),
            "{boms:?}: malformed BOM document is rejected"
        );
        assert!(
            !flow_depth_within_limit(&flow),
            "{boms:?}: hidden flow bomb"
        );
        let alias = format!("{boms}--- 'x: *undefined_zz\n'\n");
        assert!(
            crate::yaml::ayaml_from_str::<ayaml::Value>(&alias).is_err(),
            "{boms:?}: malformed BOM document is rejected"
        );
        assert!(contains_alias(&alias), "{boms:?}: hidden alias");
    }
    // The reported alias-bomb shape, end to end.
    let anchor = "1,".repeat(1000);
    let refs = "*a,".repeat(50);
    let doc =
        format!("\u{feff}--- 'k: {{a: &a [{anchor}], b: [{refs}], c: \"{{{{env.HOME}}}}\"}}\n'\n");
    // ayaml checks the whole stream's syntax before it replays an alias,
    // so the quote that `---` leaves open on line 2 stops it first.
    assert_eq!(
        crate::yaml::ayaml_from_str::<ayaml::Value>(&doc)
            .unwrap_err()
            .to_string(),
        "unterminated single-quoted scalar at line 2 column 1"
    );
    assert!(contains_alias(&doc));
    // A BOM before an ordinary document still scans like the document.
    assert!(flow_depth_within_limit("\u{feff}---\na: [1, [2]]\n"));
    assert!(!contains_alias("\u{feff}---\na: '*x'\n"));
}

/// The body of the differential parity property (see
/// [`scanner_never_misses_what_the_parser_sees`]), shared by its BOM variant.
fn check_parity(prefix: &str, sep: &str) -> Result<(), proptest::test_runner::TestCaseError> {
    let deep = format!("{prefix}{sep}{}\nc: \"\"\n", bomb(300));
    let shallow = format!("{prefix}{sep}{}\nc: \"\"\n", bomb(1));
    if parser_sees_deep_nesting(&deep) && !parser_sees_deep_nesting(&shallow) {
        proptest::prop_assert!(
            !flow_depth_within_limit_with(&deep, 200),
            "scanner missed a real flow bomb: {deep:?}"
        );
    }
    let alias = format!("{prefix}{sep}*undefined_zz\nc: \"\"\n");
    if parser_sees_alias(&alias) {
        proptest::prop_assert!(contains_alias(&alias), "missed alias: {alias:?}");
    }
    Ok(())
}

/// After a name that ayaml's scanner and line reader end in different
/// places, the scanner counts every later bracket as structure and every
/// `*` as an alias, quoted or not, and stops scanning.
#[test]
fn a_name_ayaml_reads_two_ways_makes_the_scanner_assume_the_worst() {
    // The `[` of the key `:[` and the three quoted ones.
    let quoted = format!("&a:[: x\nb: '{}'\n", "[".repeat(3));
    assert!(!flow_depth_within_limit_with(&quoted, 3));
    assert!(flow_depth_within_limit_with(&quoted, 4));
    assert!(contains_alias("&a:{: x\nb: '*'\n"));
    // A `:` before other text is no such name.
    assert!(flow_depth_within_limit_with(
        &format!("&a:b: x\nb: '{}'\n", "[".repeat(3)),
        0
    ));
    assert!(!contains_alias("&a:b: x\nb: '*'\n"));
}

/// Where an alias name ends: before a final `:`, the value indicator of
/// `*a: x`, unless the name is `:` alone or an anchor before it has the
/// whole name. An anchor's name keeps a final `:`.
#[test]
fn an_alias_name_gives_back_a_final_colon() {
    let end = |text: &str, alias: bool, anchors: &[&'static [u8]]| {
        let mut lexer = Lexer::new(text, Bom::Column);
        lexer.colon_anchors.extend(anchors);
        lexer.scan_anchor(alias);
        (lexer.i, lexer.col, lexer.colon_anchors.len())
    };
    assert_eq!(end("*a: x\n", true, &[]), (2, 2, 0));
    assert_eq!(end("*: x\n", true, &[]), (2, 2, 0));
    assert_eq!(end("*a: x\n", true, &[b"a:"]), (3, 3, 1));
    assert_eq!(end("&a: x\n", false, &[]), (3, 3, 1));
}

/// The deepest flow nesting in ayaml's event stream for `doc`, which reads
/// every document of a stream.
fn stream_flow_depth(doc: &str) -> usize {
    use ayaml::Event;
    let events = crate::yaml::stream_events(doc).unwrap();
    // Whether each open collection is a flow one.
    let mut open = Vec::new();
    let mut deepest = 0;
    for event in &events {
        match event {
            Event::SequenceStart { style, .. } | Event::MappingStart { style, .. } => {
                open.push(*style == ayaml::CollectionStyle::Flow);
                deepest = deepest.max(open.iter().filter(|&&flow| flow).count());
            }
            Event::SequenceEnd { .. } | Event::MappingEnd { .. } => {
                open.pop();
            }
            _ => {}
        }
    }
    deepest
}

/// ayaml forgets the anchor names that end in `:` at `---` and `...`, so
/// an alias in a later document gives its final `:` back unless an anchor
/// of its own document has the whole name: here `*a:` is the key `*a`, and
/// its empty block scalar ends at the sibling key below it.
///
/// No alint entry point reads past the first document with ayaml (a decode
/// refuses a second one at its marker), so ayaml's event stream, which reads
/// them all, is the oracle; the scanner keeps step with ayaml's scanner for
/// a reader that goes on.
#[test]
fn colon_anchor_names_end_with_their_document() {
    for marker in ["---", "..."] {
        let doc = format!("&a: 1\n{marker}\nx: &a 1\nk:\n  *a: |\n  b: [[1]]\n");
        let error = crate::yaml::from_str::<ayaml::Value>(&doc)
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("more than one document in stream"),
            "{error}"
        );
        assert_eq!(stream_flow_depth(&doc), 2, "{marker}");
        assert!(!flow_depth_within_limit_with(&doc, 1), "{marker}");
    }
}

/// ayaml counts an implicit key in characters, up to 1024 from the key to
/// its `:`, and refuses a longer one. A key of 1024 multi-byte characters
/// (3,072 bytes) is still a key to the scanner, so its empty block scalar
/// ends at the sibling key below it; one of 1025 ayaml refuses before it
/// reads on.
#[test]
fn an_implicit_key_of_1024_characters_is_still_a_key() {
    let key = |n: usize| "\u{20ac}".repeat(n);
    let b = bomb(MAX_YAML_FLOW_DEPTH + 76);
    let flow = |n: usize| format!("{}: |\n  \"\nb: {b}\nc: \"\"\n", key(n));
    let alias = |n: usize| format!("{}: |\n  \"\nb: *undefined_zz\nc: \"\"\n", key(n));
    assert!(parser_sees_deep_nesting(&flow(1024)));
    assert!(!flow_depth_within_limit(&flow(1024)));
    assert!(parser_sees_alias(&alias(1024)));
    assert!(contains_alias(&alias(1024)));
    for doc in [flow(1025), alias(1025)] {
        let error = crate::yaml::from_str::<ayaml::Value>(&doc)
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("an implicit mapping key may be at most 1024 characters long"),
            "{error}"
        );
    }
}

/// ayaml stops at a character that cannot start a token, such as the
/// reserved `@` and `` ` ``. The scanner steps over it and counts on, so
/// nothing after one is hidden from it.
#[test]
fn the_scanner_counts_past_a_character_that_cannot_start_a_token() {
    for doc in ["@[[1]]\n", "`[[1]]\n", "k: @[[1]]\n"] {
        assert!(!flow_depth_within_limit_with(doc, 1), "{doc:?}");
    }
}

/// `lex` reads a text once, and a text with a byte order mark once more for
/// each other way a parser reads one, and stops for good when `emit`
/// returns `false`.
#[test]
fn lex_reads_a_text_in_each_way_it_needs_and_stops_when_told() {
    let count = |text: &str, stop_after: usize| {
        let mut tokens = 0;
        lex(text, |_| {
            tokens += 1;
            tokens < stop_after
        });
        tokens
    };
    assert_eq!(count("[a]\n", usize::MAX), 1);
    assert_eq!(count("#\n\u{feff}[a]\n", usize::MAX), 2);
    assert_eq!(count("\u{feff}[a]\n", usize::MAX), 3);
    assert_eq!(count("\u{feff}[a]\n", 1), 1);
}

/// The reading of byte order marks as ayaml reads them nests what ayaml
/// nests: the mark that starts the text has no width, so a `...` that ends
/// its line after it ends a document; a second one right after it is content
/// (asamarts/ayaml#140); and one that starts a later line takes a column,
/// so a `...` after it is text. The union of the readings hides these
/// from the guards (see `Bom`), so the test reads this one alone.
#[test]
fn the_ayaml_reading_of_a_byte_order_mark_nests_what_ayaml_nests() {
    fn nesting(value: &ayaml::Value) -> usize {
        match value {
            ayaml::Value::Sequence(items) => 1 + items.iter().map(nesting).max().unwrap_or(0),
            _ => 0,
        }
    }
    for (doc, depth) in [
        ("\u{feff}[[1]]\n", 2),
        ("\u{feff}...\n[[1]]\n", 2),
        ("\u{feff}\u{feff}[[1]]\n", 0),
        ("\u{feff}#\n\u{feff}...\n[[1]]\n", 0),
    ] {
        let value = crate::yaml::ayaml_from_str::<ayaml::Value>(doc).unwrap();
        assert_eq!(nesting(&value), depth, "ayaml: {doc:?}");
        let mut seen = 0;
        Lexer::new(doc, Bom::Ayaml).run(|tok| {
            if let Tok::FlowOpen(level) = tok {
                seen = seen.max(level);
            }
            true
        });
        assert_eq!(seen, depth, "scanner: {doc:?}");
    }
}

/// A flow `:` before `?` is content to ayaml, so the scanner reads a plain
/// scalar there and moves on. It once started a plain scalar at that `:`
/// that ended before it, again and again: these inputs hung the parity
/// properties, two of them as an alias whose name gave back its `:`.
#[test]
fn the_scanner_moves_past_a_flow_colon_before_a_question_mark() {
    for (prefix, sep) in [
        ("[:?", "\n"),
        ("{a:?b", ", "),
        ("[*a:?", " "),
        ("{\\*a:?x!t \"\n", "\nb: "),
        (" {- \t\n\u{2028}|2\\*a:? &a .", "\n"),
    ] {
        check_parity(prefix, sep).unwrap();
    }
    for doc in ["[:?]\n", "[a:?b]\n", "{:?: c}\n", "{*a:?x}\n"] {
        assert!(flow_depth_within_limit_with(doc, 1), "{doc:?}");
        assert!(
            !flow_depth_within_limit_with(&format!("{doc}{}\n", bomb(3)), 2),
            "{doc:?}"
        );
    }
    assert!(!flow_depth_within_limit_with(
        &format!("[a:?b, {}]\n", bomb(2)),
        2
    ));
}

const PARITY_ALPHABET: &[&str] = &[
    "\"",
    "'",
    "''",
    "#",
    " #",
    " ",
    "\n",
    "  ",
    "|",
    ">",
    "|2",
    ">-",
    "-",
    "- ",
    "?",
    "? ",
    ":",
    ": ",
    "k: ",
    "x",
    "[",
    "]",
    "{",
    "}",
    ",",
    "&a ",
    "!t ",
    "*a ",
    "&a",
    "*a",
    "&a:",
    "*a:",
    "!t#",
    "!<!t#>",
    ".",
    "\\",
    "\t",
    "\u{85}",
    "\u{2028}",
    "\u{2029}",
    "\r\n",
    "---\n",
    "a#b",
    "k: |\n  ",
    "- k: |\n    ",
];
const PARITY_SEPS: &[&str] = &["\n", " ", "\n  ", ", ", "\nb: "];

proptest::proptest! {
    /// Differential parity against the real parser: whatever adversarial
    /// prefix precedes a deep flow bomb, if the parser nests it the scanner
    /// must see it; and whatever precedes an alias, if the parser resolves it
    /// as an alias token the short-circuit must report one.
    #[test]
    fn scanner_never_misses_what_the_parser_sees(
        prefix in proptest::collection::vec(
            proptest::sample::select(PARITY_ALPHABET), 0..14,
        ),
        sep in proptest::sample::select(PARITY_SEPS),
    ) {
        check_parity(&prefix.concat(), sep)?;
    }

    /// The same parity behind one or two leading BOMs, with the document
    /// markers and quotes that a line-1 column desync turns into hiding
    /// places (a BOM can also follow a line break, so the alphabet has it).
    #[test]
    fn scanner_never_misses_what_the_parser_sees_with_bom(
        boms in 1usize..3,
        prefix in proptest::collection::vec(
            proptest::sample::select(
                [PARITY_ALPHABET, &["---", "--- ", "--- '", "...", "\u{feff}", "%"]].concat()
            ),
            0..14,
        ),
        sep in proptest::sample::select(PARITY_SEPS),
    ) {
        check_parity(&format!("{}{}", "\u{feff}".repeat(boms), prefix.concat()), sep)?;
    }
}

#[test]
fn alias_expansion_is_bounded() {
    // Uses a SMALL budget so the counting/bail logic is exercised fast (the
    // production ceiling is validated by the compile-time upper-bound assert).
    // Single-level bomb: one 200-element anchor referenced 500 times -> 100k
    // nodes, well over a 10k budget -> rejected.
    let anchor: String = (0..200).map(|_| "0,".to_string()).collect();
    let refs: String = (0..500).map(|_| "  - *a\n".to_string()).collect();
    let bomb = format!("anchor: &a [{anchor}]\nrefs:\n{refs}");
    assert!(
        !expansion_within_limit_with(&bomb, 10_000 * YAML_NODE_COST),
        "single-level alias bomb must be rejected"
    );
    // Merge-key (`<<: *a`) is the same materialization path -> also rejected.
    let merge: String = (0..500).map(|_| "  - <<: *a\n".to_string()).collect();
    let merge_bomb = format!(
        "anchor: &a {{a: 0, b: 0, c: 0, d: 0, e: 0, f: 0, g: 0, h: 0, i: 0, j: 0}}\nrefs:\n{merge}"
    );
    assert!(
        !expansion_within_limit_with(&merge_bomb, 1_000 * YAML_NODE_COST),
        "merge-key alias bomb must be rejected"
    );
    // Legit DRY alias use stays well under budget.
    let mut dry = String::from("d: &d {a: 1, b: 2, c: 3}\nitems:\n");
    for _ in 0..50 {
        dry.push_str("  - <<: *d\n    n: 1\n");
    }
    assert!(
        expansion_within_limit_with(&dry, 10_000 * YAML_NODE_COST),
        "legit DRY alias bundle must pass"
    );
    // Alias-free text short-circuits (never even parses here) -> always within.
    assert!(expansion_within_limit_with(&"k: v\n".repeat(100_000), 10));
    // A quoted glob is not an alias -> short-circuits, passes.
    assert!(expansion_within_limit_with("paths:\n  - \"**/*.rs\"\n", 5));
}

#[test]
fn alias_expansion_budget_is_charged_in_bytes() {
    // Regression: the budget counted NODES, so one big anchored scalar
    // replayed N times cost N, not N x its size. A 1 MB scalar x 4000 refs
    // (4000 nodes) passed and the real parse copied 4 GB (3.8 GB peak RSS) on
    // both the Value config path and the structured-query path. Production
    // budget, the reported shape (scaled to 3000 refs, still 3 GB of copies).
    let big = format!(
        "a: &a \"{}\"\nb: [{}]\n",
        "x".repeat(1_000_000),
        "*a,".repeat(3000)
    );
    assert!(!expansion_within_limit(&big));
    assert!(!scan_guards(&big).expansion_within_limit(&big));
    // The same scalar replayed a handful of times is fine.
    let few = format!("a: &a \"{}\"\nb: [*a, *a, *a]\n", "x".repeat(1_000_000));
    assert!(expansion_within_limit(&few));
    assert!(scan_guards(&few).expansion_within_limit(&few));
    // Mechanism, small budget: keys and tag names are charged too.
    let s = "y".repeat(1000);
    let key = format!("a: &a {{\"{s}\": 1}}\nb: [{}]\n", "*a,".repeat(200));
    assert!(!expansion_within_limit_with(&key, 300_000));
    let tag = format!("a: &a !{s} 1\nb: [{}]\n", "*a,".repeat(200));
    assert!(!expansion_within_limit_with(&tag, 300_000));
}

/// Only ayaml's alias-expansion limit means the alias budget is spent. A
/// document past the input cap that uses one alias passes the budget pass,
/// so its caller does not report an alias expansion, and the parse refuses
/// it for its size.
#[test]
fn a_document_over_the_input_cap_is_not_an_alias_bomb() {
    // One allocation just past the cap, padded with a comment.
    let head = b"a: &a 1\nb: *a\n# ";
    let mut bytes = vec![b'x'; crate::structured_format::MAX_STRUCTURED_BYTES + 1];
    bytes[..head.len()].copy_from_slice(head);
    let doc = String::from_utf8(bytes).unwrap();
    let error = crate::yaml::from_str::<ayaml::Value>(&doc)
        .unwrap_err()
        .into_parser();
    assert_eq!(
        error.limit().map(ayaml::LimitExceeded::kind),
        Some(ayaml::LimitKind::InputBytes),
        "{error}"
    );
    assert!(expansion_within_limit(&doc));
    assert!(scan_guards(&doc).expansion_within_limit(&doc));
}

#[test]
fn map_heavy_alias_bomb_is_rejected_at_the_production_budget() {
    // Regression: 8M nodes was too loose for map-heavy content (~235 B/node):
    // an 18.5 KB `[{k: v} x 500]` anchor x 5000 refs passed and parsed for
    // 4.9 s / 1.76 GB. The byte budget cuts it off at ~1M nodes.
    let doc = format!(
        "a: &a [{}]\nb: [{}]\n",
        "{k: v},".repeat(500),
        "*a,".repeat(5000)
    );
    assert!(!expansion_within_limit(&doc));
    let scan = scan_guards(&doc);
    assert!(scan.flow_within_limit());
    assert!(!scan.expansion_within_limit(&doc));
}

#[test]
fn realistic_anchor_heavy_yaml_passes_with_a_wide_margin() {
    // The other half: real alias use must keep a big margin under the budget.
    // Shaped after the heaviest real file measured (GitLab's
    // `rules.gitlab-ci.yml`: ~1100 aliases into rule lists, ~4 MB by the
    // budget's estimate) plus docker-compose `x-` anchors with `<<:` merges:
    // 65K expanded nodes + 1.3 MB of scalar copies (~18 MB estimated, 12 MB
    // real peak RSS), ~4.5x that file -- and it must pass a budget 10x
    // TIGHTER than production.
    let mut doc = String::from("x-common: &common\n  restart: unless-stopped\n");
    doc.push_str("  environment: &env\n");
    doc.extend((0..40).map(|i| format!("    VAR_{i}: \"value-{i}-with-a-realistic-length\"\n")));
    doc.push_str("  logging: {driver: json-file, options: {max-size: 10m}}\n");
    doc.push_str(".patterns:\n  code: &code\n");
    doc.extend((0..5).map(|p| format!("    - \"{{app,lib,ee/app,ee/lib}}/**/*-{p}.rb\"\n")));
    doc.push_str(".rules:\n");
    for r in 0..300 {
        doc.push_str("  rules-");
        doc.push_str(&r.to_string());
        doc.push_str(": &rules-");
        doc.push_str(&r.to_string());
        doc.push('\n');
        doc.extend((0..3).map(|k| {
            format!(
                "    - if: '$CI_MERGE_REQUEST_LABELS =~ /pipeline:run-{r}-{k}/ && \
                 $CI_PIPELINE_SOURCE == \"merge_request_event\"'\n      \
                 changes: *code\n      when: on_success\n"
            )
        }));
    }
    doc.push_str("services:\n");
    doc.extend((0..60).map(|s| {
        format!(
            "  svc-{s}:\n    <<: *common\n    image: \"registry.example.com/svc-{s}:1.2.3\"\n    \
             environment:\n      <<: *env\n      SVC: \"{s}\"\n"
        )
    }));
    doc.push_str("jobs:\n");
    doc.extend((0..1000).map(|j| {
        format!(
            "  job-{j}:\n    script: [\"make test-{j}\"]\n    rules: *rules-{}\n",
            j % 300
        )
    }));
    assert!(contains_alias(&doc));
    assert!(crate::yaml::ayaml_from_str::<ayaml::Value>(&doc).is_ok());
    assert!(expansion_within_limit_with(
        &doc,
        MAX_YAML_EXPANSION_BYTES / 10
    ));
    assert!(expansion_within_limit(&doc));
}
