//! The rewrites' own tests: what each makes of the inputs it is for, and
//! what it leaves alone.

use super::*;

fn one(rewrites: Vec<String>) -> String {
    assert_eq!(rewrites.len(), 1, "{rewrites:?}");
    rewrites.into_iter().next().unwrap()
}

#[test]
fn tokens_start_where_properties_can() {
    let input = "a: &x [*y,&z] b&c !<tag:x,2000:y> !t";
    let found: Vec<&str> = tokens(input, &['&', '*', '!'])
        .into_iter()
        .map(|range| &input[range])
        .collect();
    assert_eq!(found, ["&x", "*y", "&z", "!<tag:x,2000:y>", "!t"]);
    // Nor does one in a quoted scalar, a comment or a block scalar.
    let input = "a: 'x &y'\nb: \"!t\" # &z\nc: |\n  *w\nd: &v 1\n";
    let found: Vec<&str> = tokens(input, &['&', '*', '!'])
        .into_iter()
        .map(|range| &input[range])
        .collect();
    assert_eq!(found, ["&v"]);
    assert_eq!(
        wide_anchor_names("a: 'x &\u{e9}'\nb: 1\n"),
        Vec::<String>::new()
    );
    // A sigil inside a token starts none.
    let input = "&a\u{feff}&b x";
    let found: Vec<&str> = tokens(input, &['&'])
        .into_iter()
        .map(|range| &input[range])
        .collect();
    assert_eq!(found, ["&a\u{feff}&b"]);
    // A `:` ends a name before text outside a flow collection only; a `?`
    // ends it before anything but a flow indicator.
    let input = "- &a:b x\n- [*c:d, &e?f g, *h?]\n";
    let found: Vec<&str> = tokens(input, &['&', '*'])
        .into_iter()
        .map(|range| &input[range])
        .collect();
    assert_eq!(found, ["&a", "*c:d", "&e", "*h?"]);
}

/// The quoted-only characters ayaml keeps, in a quoted scalar, go, and so
/// do those past its scanner's error, which it never reads: the one it
/// rejects, which both parsers reject, stays.
#[test]
fn quoted_only_characters_go_from_quoted_scalars_only() {
    assert_eq!(
        quoted_only_characters("a: '\u{7f}'\nb: c\u{ffff}d '\u{80}'\n# \u{80}\n"),
        [
            "a: ''\nb: c\u{ffff}d ''\n# \n",
            "a: 'x'\nb: c\u{ffff}d 'x'\n# x\n",
            "a: ''\nb: c\u{ffff}d '\u{80}'\n# \u{80}\n",
            "a: 'x'\nb: c\u{ffff}d '\u{80}'\n# \u{80}\n"
        ]
    );
    assert_eq!(
        quoted_only_characters("b: c\u{ffff}\n"),
        Vec::<String>::new()
    );
}

/// A tab that leads a block scalar's line before its content is the tab
/// entry's but in a line of tabs alone, which both parsers reject, under a
/// header that is not at the top: ayaml makes the line content past the
/// parent's column or under a top-level header, and otherwise ends the
/// scalar before it and reads it as white space.
#[test]
fn a_leading_line_of_tabs_alone_stays() {
    assert_eq!(tabs("a: |\n\t\n  b\n"), Vec::<String>::new());
    assert_eq!(tabs("a: |\n\t\t\n"), Vec::<String>::new());
    assert_eq!(tabs("a: |\n \t\n  b\n")[0], "a: |\n  \n  b\n");
    assert_eq!(tabs("|\n\t\nb\n")[0], "|\n \nb\n");
    assert_eq!(tabs("b:\n  b: |\n \t\n")[0], "b:\n  b: |\n  \n");
    assert_eq!(tabs("- >+\n\n\t \n")[0], "- >+\n\n  \n");
}

/// Both scanners take `,`, `]`, `}`, `@`, a backquote and `%` for tokens of
/// their own wherever a node may start, and read a flow collection or a
/// quoted scalar that follows a node on its line: no plain scalar then runs
/// on to the next line. A block scalar ends at a line indented less than its
/// content.
#[test]
fn places_follow_the_scanners_tokens() {
    let at = |input: &str, text: &str| places(input)[input.rfind(text).unwrap()];
    for start in [",", "]", "}", "@", "`", "%"] {
        let input = format!("a: {start}1\n\"x\"\n");
        assert!(at(&input, "x").quoted, "{input:?}");
        // A top-level plain scalar would go on at column 1.
        let input = format!(" {start}1\n\"x\"\n");
        assert!(at(&input, "x").quoted, "{input:?}");
    }
    assert_eq!(at("'a'? [1,\nb]\n", "b").depth, 1);
    assert!(at("'a' 'b'\n", "b").quoted);
    assert!(!at("a 'b'\n", "b").quoted);
    let input = "- >-\n  a\n    b\n - [x,\nc]\n";
    assert!(at(input, "b").literal);
    assert_eq!(at(input, "c").depth, 1);
    assert!(!at(input, "c").literal);
}

/// A tab that starts a line in a flow collection becomes one space, and
/// one more than the widest indentation; one in a quoted scalar or after
/// other white space stays, and the tab entry's rewrite leaves it.
#[test]
fn flow_line_tabs_go_where_a_tab_starts_a_flow_line() {
    assert_eq!(flow_line_tabs("a: [x,\n\ty]\n"), ["a: [x,\n y]\n"]);
    assert_eq!(
        flow_line_tabs("a:\n  [x,\n\t y]\n"),
        ["a:\n  [x,\n  y]\n", "a:\n  [x,\n    y]\n"]
    );
    for input in ["a: '\n\tx'\n", "a: [x,\n \ty]\n", "a:\n\tb\n"] {
        assert!(flow_line_tabs(input).is_empty(), "{input:?}");
    }
    assert_eq!(tabs("a: [x,\n\ty]\n"), Vec::<String>::new());
    assert_eq!(tabs("a: [x,\n \ty]\n")[0], "a: [x,\n  y]\n");
}

/// `-0` becomes `+0` where it is a plain scalar of its own only.
#[test]
fn negative_zeros_are_whole_plain_scalars() {
    assert_eq!(
        negative_zeros("a: -0\nb: x -0\nc: '-0'\nd: -00\ne: [-0,-0]\n# -0\nf: |\n  -0\n"),
        ["a: +0\nb: x -0\nc: '-0'\nd: -00\ne: [+0,+0]\n# -0\nf: |\n  -0\n"]
    );
    assert_eq!(negative_zeros("a: -1\n"), Vec::<String>::new());
}

/// A comment line leaves a node that starts on a later line to its owner,
/// and a top-level plain scalar goes on at any indentation: the tab after
/// the next entry's `-`, or on a line of white space after the scalar, is
/// white space, not text.
#[test]
fn a_comment_line_or_a_top_level_scalar_moves_no_owner() {
    assert_eq!(
        tabs("a:\n  -\n # c\n    x\n  -\ty\n"),
        ["a:\n  -\n # c\n    x\n  - y\n"]
    );
    let input = "x\n\"y\n\t\n";
    assert!(places(input)[2].plain);
    assert_ne!(tabs(input), Vec::<String>::new());
    // A marker ends the top-level scalar, and a key makes it no root.
    assert!(!places("x\n---\n\"y\n")[6].plain);
    assert!(!places("a:\n  x\n\"y\n")[7].plain);
}

/// A rewrite changes its construct only: not a sigil or a tab in a plain
/// scalar's text, a tab in a quoted scalar or in a block scalar's
/// content, and a deletion does not start a line or the stream anew.
#[test]
fn rewrites_leave_what_is_not_their_construct() {
    for input in [
        "run: make && test\n",
        "a: x !!int y\n",
        "a: hello !\n",
        "a: see !<tag:x,2000:y> here\n",
        "- [a &b, c !t]\n",
        "a: x\n  &y *z !t\n",
    ] {
        assert_eq!(wide_anchor_names(input), Vec::<String>::new(), "{input:?}");
        assert_eq!(core_tags(input), Vec::<String>::new(), "{input:?}");
        assert_eq!(global_tags(input), Vec::<String>::new(), "{input:?}");
        assert_eq!(non_specific_tags(input), Vec::<String>::new(), "{input:?}");
        assert_eq!(redefined_anchors(input), Vec::<String>::new(), "{input:?}");
    }
    for input in [
        "a: x 18446744073709551616 y\n",
        "[x 18446744073709551616]\n",
    ] {
        assert_eq!(wide_integers(input), Vec::<String>::new(), "{input:?}");
    }
    // A tab in a quoted scalar, a plain scalar's text, a comment, or a
    // block scalar's content past its indentation is content.
    assert_eq!(one(tabs("- \"a\tb\"\n-\tc\n")), "- \"a\tb\"\n- c\n");
    for input in [
        "a: x\ty\n",
        "[a\tb]\n",
        "a: 'x\n\t  y'\n",
        "# a\tb\n",
        "a: |\n  x\ty\n  \tz\n",
    ] {
        assert_eq!(tabs(input), Vec::<String>::new(), "{input:?}");
    }
    // White space around a plain scalar is not: before it, after it,
    // before a comment or an indicator, and leading on its next line;
    // nor a block scalar's indentation, before its first line set it or
    // short of it after.
    for (input, rewritten) in [
        ("a:\tx\n", "a: x\n"),
        ("a: x\t# c\n", "a: x # c\n"),
        ("[a\t, b]\n", "[a , b]\n"),
        ("k:\n  a: x\n  \ty\n", "k:\n  a: x\n   y\n"),
        ("a: |\n  \tx\n", "a: |\n   x\n"),
        ("a: |\n  x\n \ty\n", "a: |\n  x\n  y\n"),
    ] {
        assert_eq!(tabs(input)[0], rewritten, "{input:?}");
    }
    // Deleting a tag first on its line does not make a directive, a
    // document marker or a leading byte-order mark of what follows it.
    assert_eq!(
        core_tags("!!int \u{feff}%TAG !e! x:\n<<: y\n"),
        [
            " \u{feff}%TAG !e! x:\n<<: y\n",
            "!xint \u{feff}%TAG !e! x:\n<<: y\n"
        ]
    );
    assert_eq!(core_tags("- !!str x\n!!str ---\n")[0], "- x\n ---\n");
    assert_eq!(global_tags("!<t:x> %a\n")[0], " %a\n");
    assert_eq!(global_tags("!<t:x> ... #\n")[0], " ... #\n");
}

#[test]
fn an_edit_that_overlaps_the_one_before_is_left_out() {
    let edits = [(0..2, "x".to_string()), (1..3, "y".to_string())];
    assert_eq!(apply("abcd", &edits), ["xcd"]);
}

#[test]
fn places_follow_flow_collections_quotes_and_block_scalars() {
    let depths =
        |input: &str| -> Vec<u32> { places(input).into_iter().map(|place| place.depth).collect() };
    // Outside a flow collection, a `[` or `{` inside a plain scalar is text.
    assert!(
        depths("a b [c\nTRUE{<<: \n:\n- -a? [x\n")
            .iter()
            .all(|depth| *depth == 0)
    );
    let input = "a: [b, '[', {c: \"}\"}] # [\nd: !t {e: [\n  f]}\n";
    let found = depths(input);
    assert_eq!(found[input.find("b,").unwrap()], 1);
    assert_eq!(found[input.find("c:").unwrap()], 2);
    assert_eq!(found[input.find("# [").unwrap()], 0);
    assert_eq!(found[input.find("f]").unwrap()], 2);
    assert_eq!(found[input.len()], 0);
    let found = places("a: 'x\n: y'\nb: |\n  [z\n: w\n");
    assert!(found[6].literal && found[16].literal);
    assert_eq!(found[16].depth, 0);
    assert!(!found[21].literal);
    // A plain scalar's next line, indented past its key, continues it.
    let input = "a: .nan\n ? ['q,\n  b]\n: \n";
    assert!(depths(input).iter().all(|depth| *depth == 0));
    assert!(places(input).iter().all(|place| !place.literal));
    // A block scalar's content is indented past its key, not its line.
    let input = "  - id: |-\n    : r2\n";
    assert!(!places(input)[input.find(": r2").unwrap()].literal);
    // A node on the line after `- ` belongs to the `-`, so the line after
    // it continues a plain scalar there.
    let input = "- \n  ~,\n  'q]]\n:\n";
    assert!(!places(input)[input.find("\n:").unwrap() + 1].literal);
    // In a flow collection, a quote starts a scalar only where a node
    // starts: not inside a plain scalar, even on its next line, or
    // after a `:` that a non-space follows.
    for input in [
        "[a 'b, c]\n",
        "[a\n 'b, c]\n",
        "[:'b, c]\n",
        "{a:'b, c: d}\n",
    ] {
        assert!(
            places(input).iter().all(|place| !place.literal),
            "{input:?}"
        );
    }
    for input in [
        "['b, c']\n",
        "[&a 'b, c']\n",
        "{\"a\":'b, c'}\n",
        "{a: 'b, c'}\n",
    ] {
        assert!(
            places(input)[input.find("c'").unwrap()].literal,
            "{input:?}"
        );
    }
}

#[test]
fn line_breaks_and_byte_order_marks_go() {
    assert_eq!(
        line_breaks("a: x\u{85}y\n"),
        ["a: xy\n", "a: x\n    y\n", "a: x\ny\n", "a: xxy\n"]
    );
    // Each break's next line starts at the break's column.
    assert_eq!(
        line_breaks("- 'a\u{85}b\u{2028}c'\nd\n")[1],
        "- 'a\n    b\n     c'\nd\n"
    );
    assert_eq!(line_breaks("a\u{2028}: \u{2029}\n")[3], "ay: z\n");
    assert_eq!(line_breaks("a\n"), Vec::<String>::new());
    assert_eq!(
        line_start_byte_order_marks("\u{feff}a\n\u{feff}\u{feff}b c\u{feff}\n"),
        [
            "a\nb c\u{feff}\n",
            " a\n  b c\u{feff}\n",
            "a\n  b c\u{feff}\n"
        ]
    );
    assert_eq!(line_start_byte_order_marks("a:\n\u{feff}b\n").len(), 2);
    assert_eq!(
        line_start_byte_order_marks("a\u{feff}\n"),
        Vec::<String>::new()
    );
    assert_eq!(one(tabs("-\ta\n")), "- a\n");
    assert_eq!(
        tabs("a:\t|\n \t\n  b\n"),
        ["a: |\n  \n  b\n", "a: |\n \n  b\n", "a: |\n x\n  b\n"]
    );
}

#[test]
fn empty_keys_get_a_null_key() {
    assert_eq!(one(empty_keys(": v\n- :\nb: c\n")), "~: v\n- ~:\nb: c\n");
    assert_eq!(empty_keys("a: :b\n"), Vec::<String>::new());
    assert_eq!(
        one(empty_keys("? b\n  # c\n: {}\n: {}\n- ? x\n  : y\n")),
        "? b\n  # c\n: {}\n~: {}\n- ? x\n  : y\n"
    );
    assert_eq!(empty_keys("? a\n:\n"), Vec::<String>::new());
    assert_eq!(empty_keys("  ? [1,\n  2]\n  : x\n"), Vec::<String>::new());
    assert_eq!(one(empty_keys("TRUE{<<: \n\n:\n")), "TRUE{<<: \n\n~:\n");
    assert_eq!(
        empty_keys("a: 'x\n: y'\nb: |\n  : z\n"),
        Vec::<String>::new()
    );
    assert_eq!(one(empty_keys("? b\r\n: : {}\r\n")), "? b\r\n: ~: {}\r\n");
}

#[test]
fn a_leading_document_end_goes() {
    assert_eq!(one(leading_document_end("# c\n...\na: 1\n")), "# c\na: 1\n");
    assert_eq!(one(leading_document_end("... # c\n")), "");
    // Each of them.
    assert_eq!(
        one(leading_document_end("...\n... #c\n\n...\r\na: 1\n")),
        "\n\na: 1\n"
    );
    assert_eq!(leading_document_end("a\n...\n"), Vec::<String>::new());
    assert_eq!(leading_document_end("...a\n"), Vec::<String>::new());
}

#[test]
fn wide_names_narrow_with_their_aliases() {
    assert_eq!(
        wide_anchor_names("- &a.b x\n- *a.b\n"),
        ["- &a_b x\n- *a_b\n", "- &a .b x\n- *a .b\n"]
    );
    // A `:` before a character other than white space ends a name.
    assert_eq!(
        wide_anchor_names("&a!<t:x> y\n"),
        ["&a__t:x> y\n", "&a !<t:x> y\n"]
    );
    assert_eq!(one(wide_anchor_names("&é x")), "&_ x");
    assert_eq!(wide_anchor_names("&a1: x\n")[1], "&a1 : x\n");
    // `*a:` is the alias `a` and a `:`, which stays after a name no anchor
    // gave too.
    assert_eq!(
        wide_anchor_names("- &a k\n- {*a: v}\n"),
        Vec::<String>::new()
    );
    assert_eq!(wide_anchor_names("*b<<: x\n")[0], "*b__: x\n");
    assert_eq!(wide_anchor_names("a: {*b<<: ~,}\n")[0], "a: {*b__: ~,}\n");
    // In a flow collection, a `:` before text is part of a name.
    assert_eq!(
        wide_anchor_names("- &l x\n- [*l:a]\n")[1],
        "- &l x\n- [*l : a]\n"
    );
    assert_eq!(wide_anchor_names("&a-1_b x\n*a-1_b"), Vec::<String>::new());
}

#[test]
fn redefined_anchors_get_names_of_their_own() {
    assert_eq!(
        redefined_anchors("- &a 1\n- &a 2\n- *a\n"),
        ["- &a 1\n- &a-2r 2\n- *a-2r\n", "- &a 1\n- &a-2r 2\n- *a\n"]
    );
    assert_eq!(redefined_anchors("- &a 1\n- *a\n"), Vec::<String>::new());
}

#[test]
fn tags_go_by_kind() {
    assert_eq!(
        core_tags("a: !!map 1\nb: !!binary x\n"),
        ["a:  1\nb: !!binary x\n", "a: !xmap 1\nb: !!binary x\n"]
    );
    assert_eq!(
        core_tags("a: !!int '1'\nb: !!str \"x:y\"\n"),
        [
            "a:  '1'\nb:  \"x:y\"\n",
            "a:  1\nb:  \"x:y\"\n",
            "a: !xint '1'\nb: !xstr \"x:y\"\n"
        ]
    );
    assert_eq!(
        one(root_block_scalar_content("--- !t |\nx\n\n  y\n...\nz\n")),
        "--- !t |\n x\n\n   y\n...\nz\n"
    );
    assert_eq!(one(root_block_scalar_content(">\n...x\n")), ">\n ...x\n");
    assert_eq!(root_block_scalar_content("a: |\nx\n"), Vec::<String>::new());
    assert_eq!(
        core_tags("%TAG !! tag:x,2000:\n---\na: !!int 1\n"),
        Vec::<String>::new()
    );
    assert_eq!(
        global_tags("a: !<tag:x,2000:y> 1\nb: !!binary x\nc: !!str y\n"),
        [
            "a:  1\nb:  x\nc: !!str y\n",
            "a: !t 1\nb: !t x\nc: !!str y\n",
            "a: !!str 1\nb: !!str x\nc: !!str y\n"
        ]
    );
    // A verbatim tag that names a local tag is that local tag.
    assert_eq!(
        global_tags("a: !<!t> 1\nb: !<tag:x,2000:y> 1\n")[0],
        "a: !<!t> 1\nb:  1\n"
    );
    // `!!str` goes on a scalar only, after any anchor.
    assert_eq!(
        global_tags("a: !<t:x> [1]\nb: !<t:x> &a 2\nc: !<t:x>\n")[2],
        "a:  [1]\nb: !!str &a 2\nc: \n"
    );
    assert_eq!(
        global_tags("%TAG !! tag:x,2000:\n---\na: !!int 1\n").len(),
        2
    );
    assert_eq!(
        global_tags("%TAG !! tag:x,2000:\n---\na: !!int 1\n")[0],
        "%TAG !! tag:x,2000:\n---\na:  1\n"
    );
    assert_eq!(
        global_tags("%TAG !e! tag:x,2000:\n---\n[!e!z a, !f!z b, !t c]\n")[0],
        "%TAG !e! tag:x,2000:\n---\n[ a, !f!z b, !t c]\n"
    );
    assert_eq!(
        global_tags("%TAG ! tag:x,2000:\n---\n!a b\n")[0],
        "%TAG ! tag:x,2000:\n---\nb\n"
    );
    assert_eq!(
        global_tags("a: !<!> x\nb: !<> y\nc: !t z\n"),
        Vec::<String>::new()
    );
    assert_eq!(
        global_tags("a:\n  b: 1\n!<tag:x,2000:y>   c: 2\n- !!binary\tx\n")[..2],
        ["a:\n  b: 1\nc: 2\n- x\n", "a:\n  b: 1\n   c: 2\n- \tx\n"]
    );
    assert_eq!(core_tags("!!map : x\n")[2], "~ : x\n");
    assert_eq!(
        non_specific_tags("[1,!  ,]\n"),
        ["[1,  ,]\n", "[1,''  ,]\n"]
    );
    assert_eq!(
        one(non_specific_tags("a: ! 1\nb: !t 2\n")),
        "a:  1\nb: !t 2\n"
    );
}

#[test]
fn quoted_only_characters_and_long_runs_go() {
    assert_eq!(
        quoted_only_characters("a: '\u{7f}\u{85}\u{ffff}'\n"),
        [
            "a: '\u{85}'\n",
            "a: 'x\u{85}x'\n",
            "a: '\u{85}\u{ffff}'\n",
            "a: 'x\u{85}\u{ffff}'\n"
        ]
    );
    let key = "k".repeat(1_025);
    let cut = one(long_runs(&format!("{{{key}: v}}")));
    assert_eq!(cut, format!("{{{}: v}}", "k".repeat(1_000)));
    assert_eq!(
        long_runs(&format!("{{{}: v}}", "k".repeat(1_024))),
        Vec::<String>::new()
    );
    // Only a key is cut: not a value, a quoted scalar, a comment or a
    // block scalar, nor a run that a `:` in one of them ends.
    for input in [
        format!("a: {key}\n"),
        format!("- \"{key}\"\n"),
        format!("- '{key}: v'\n"),
        format!("# {key}: v\n"),
        format!("a: |\n  {key}: v\n"),
        format!("a: {key}:v\n"),
    ] {
        assert_eq!(long_runs(&input), Vec::<String>::new(), "{input}");
    }
    let wide = "\u{e9}".repeat(513);
    assert_eq!(
        one(wide_runs(&format!("{wide}: v\n- '{wide}'\n"))),
        format!("{}: v\n- '{wide}'\n", "\u{e9}".repeat(500))
    );
    // A `:` inside a key is part of it; keys that differ stay apart, and
    // equal keys stay equal.
    let other = "k".repeat(1_030);
    assert_eq!(
        one(long_runs(&format!("{{{key}:a{key}: v}}"))),
        format!("{{{}: v}}", "k".repeat(1_000))
    );
    assert_eq!(
        one(long_runs(&format!("{{{key}: 1, {other}: 2, {key}: 3}}"))),
        format!(
            "{{{}: 1, {}: 2, {}: 3}}",
            "k".repeat(1_000),
            "k".repeat(999),
            "k".repeat(1_000)
        )
    );
    // A cut key does not become another key of the input.
    let short = "k".repeat(1_000);
    assert_eq!(
        one(long_runs(&format!("{{{other}: 1, {short}: 2}}"))),
        format!("{{{}: 1, {short}: 2}}", "k".repeat(999))
    );
}

#[test]
fn wide_integers_become_one() {
    // As 1, then as a number of each value's own.
    assert_eq!(
        wide_integers("a: 18446744073709551616\n-9223372036854775809: [+18446744073709551616]\n"),
        ["a: 1\n-1: [+1]\n", "a: 1\n-2: [+1]\n"]
    );
    // Each base, with a sign, and in a flow collection; the numbers of
    // their own skip what the rest of the input holds.
    assert_eq!(
        wide_integers(
            "- 0x10000000000000000\n- -0o2000000000000000000001\n\
             - [+0b1_0, 0b10000000000000000000000000000000000000000000000000000000000000000]\n"
        ),
        ["- 1\n- -1\n- [+0b1_0, 1]\n", "- 2\n- -3\n- [+0b1_0, 2]\n"]
    );
    assert_eq!(
        wide_integers("? 1\n18446744073709551616: x\n"),
        ["? 1\n1: x\n", "? 1\n2: x\n"]
    );
    // Not a 64-bit integer, a scalar wider than 128 bits, a quoted
    // scalar, a comment, digits inside a plain scalar, a leading zero,
    // or a flow indicator in a block scalar's place, which ends nothing.
    for input in [
        "a: 18446744073709551615\nb: -9223372036854775808\nc: 0xFFFFFFFFFFFFFFFF\n",
        "a: 340282366920938463463374607431768211456\n",
        "a: -170141183460469231731687303715884105729\n",
        "a: '18446744073709551616'\n# 18446744073709551616\n",
        "a: x18446744073709551616\nb: 18446744073709551616x\n",
        "a: 018446744073709551616\nb: 0x\nc: 0X10000000000000000\n",
        "a: 18446744073709551616]\nb: x,18446744073709551616\n",
    ] {
        assert_eq!(wide_integers(input), Vec::<String>::new(), "{input:?}");
    }
}

#[test]
fn a_node_below_its_properties_is_not_empty() {
    // The properties' node is the collection below them.
    for input in [
        "!!map \n- a\n",
        "--- !!map\n- a\n",
        "k: !!seq\n- a\n",
        "k: !!map # c\n\n  a: 1\n",
        "- !!map\n  a: 1\n",
        "[!!str\n x]\n",
    ] {
        let at = input.find(['!']).unwrap() + 5;
        assert!(!empty_node_after(input, at), "{input:?}");
    }
    // The next line is another entry, or nothing follows.
    for input in [
        "- !!null\n- a\n",
        "k: !!null\nj: a\n",
        "a:\n  k: !!null\nj: a\n",
        "k: !!null\n",
        "[!!null\n , x]\n",
    ] {
        let at = input.find(['!']).unwrap() + 6;
        assert!(empty_node_after(input, at), "{input:?}");
    }
    // A document marker below is no node.
    for input in ["!!map \n\n...\n", "!!map\n--- a\n"] {
        let at = input.find(['!']).unwrap() + 5;
        assert!(empty_node_after(input, at), "{input:?}");
    }
}

#[test]
fn flow_scalars_that_start_with_a_colon_are_quoted() {
    assert_eq!(
        one(flow_colon_scalars("[!!str :a/**, &x :b c, !t :d'e]\n")),
        "[!!str ':a/**', &x ':b c', !t ':d''e']\n"
    );
    // Not without properties, outside a flow collection, in a quoted
    // scalar, after properties inside a plain scalar, or before a space.
    for input in [
        "[:a]\n",
        "a: !!str :b\n",
        "['!!str :a']\n",
        "[a !!str :b]\n",
        "[!!str : a]\n",
    ] {
        assert_eq!(flow_colon_scalars(input), Vec::<String>::new(), "{input:?}");
    }
}

#[test]
fn empty_flow_keys_become_null() {
    assert_eq!(
        one(empty_flow_keys("[? ]\n{? , a: b}\n[a, ?\n ]\n")),
        "[? ~ ]\n{? ~ , a: b}\n[a, ? ~\n ]\n"
    );
    // A key, a `?` that opens no entry, in a block mapping, a quoted
    // scalar or a comment, or with no space after it.
    for input in [
        "[? a]\n",
        "[a ? ]\n",
        "? ]\n",
        "['? ]']\n",
        "[ # ? ]\n]\n",
        "[?]\n",
    ] {
        assert_eq!(empty_flow_keys(input), Vec::<String>::new(), "{input:?}");
    }
}

/// A tab ayaml rejects as a flow line's only indentation, where its scanner
/// opened a flow collection that YAML 1.2 reads as a plain scalar's text, is
/// a flow-line tab, and the tab entry leaves it.
#[test]
fn a_tab_ayaml_rejects_on_a_flow_line_is_a_flow_line_tab() {
    let input = "  true,\n[{a: é},\n\t  é]\n# end\n";
    assert_eq!(
        flow_line_tabs(input),
        [
            "  true,\n[{a: é},\n   é]\n# end\n",
            "  true,\n[{a: é},\n     é]\n# end\n"
        ]
    );
    assert_eq!(tabs(input), Vec::<String>::new());
}

/// A variant of the tab entry's leaves out a tab that starts a line, past
/// spaces, with the white space after it: ayaml reads the line's
/// indentation from the spaces, which a space for the tab would add to. Not
/// where the content would then start the line as a marker.
#[test]
fn a_variant_leaves_out_a_leading_tab_with_the_white_space_after_it() {
    assert_eq!(
        tabs("-\t# c\n- |\n   x\n\t  'q'\n")
            .last()
            .map(String::as_str),
        Some("- # c\n- |\n   x\n'q'\n")
    );
    assert_eq!(tabs("a\n\t---\n").len(), 3);
}

/// An empty implicit key of a flow entry becomes `~`, with a space after
/// its `:` where a flow indicator follows; a `:` that starts a plain scalar,
/// one after a key, and one in a quoted scalar or a comment stay.
#[test]
fn empty_flow_implicit_keys_become_null() {
    assert_eq!(
        one(empty_flow_implicit_keys("a: [: x, {:}, {b: 1, :\n c}]\n")),
        "a: [~: x, {~: }, {b: 1, ~:\n c}]\n"
    );
    for input in [
        "a: [:a]\n",
        "a: [b: c]\n",
        "a: [': x']\n",
        "a: [x] # [: x]\n",
        ": x\n",
    ] {
        assert!(empty_flow_implicit_keys(input).is_empty(), "{input:?}");
    }
}

/// `!<!>` is named where it is a node's tag, or where ayaml rejects it:
/// after a quoted scalar, and, in a malformed document, in what YAML 1.2
/// reads as a plain scalar's second line. Both parsers then read what
/// follows it alike.
#[test]
fn verbatim_non_specific_tags_become_named() {
    for (input, rewrite) in [
        (
            "a: !<!> x\nb: [!<!> y]\n- !<!>  z\n",
            "a: !<!t> x\nb: [!<!t> y]\n- !<!t>  z\n",
        ),
        ("a: !<!>x\n", "a: !<!t>x\n"),
        ("a: [!<!>, !<!>]\n", "a: [!<!t>, !<!t>]\n"),
        ("'q''s'!<!> x\n", "'q''s'!<!t> x\n"),
        (" ---\n!<!> {a: a b,}\n", " ---\n!<!t> {a: a b,}\n"),
    ] {
        assert_eq!(one(verbatim_non_specific_tags(input)), rewrite);
    }
    for input in [
        "a: x!<!> y\n",
        "a: '!<!> y'\n",
        "# !<!> x\n",
        "a: b !<!> y\n",
        " a\n!<!> x\n",
    ] {
        assert!(verbatim_non_specific_tags(input).is_empty(), "{input:?}");
    }
}

/// One where ayaml reports an error about something else is `x` in every
/// rewrite: removed, it would leave its line empty, where ayaml no longer
/// sees the line that is not indented enough. One ayaml rejects stays.
#[test]
fn quoted_only_characters_keep_the_place_of_ayamls_error() {
    assert_eq!(quoted_only_characters("a: 'x\n\u{8a}'\n"), ["a: 'x\nx'\n"]);
    assert_eq!(
        quoted_only_characters("a: 'x\n\u{8a}\u{7f}'\n"),
        ["a: 'x\nx'\n", "a: 'x\nxx'\n", "a: 'x\nx\u{7f}'\n"]
    );
    assert_eq!(
        quoted_only_characters("k: !'\u{7f}'e x\n"),
        Vec::<String>::new()
    );
}

/// The characters only a quoted scalar may hold are found where ayaml reads
/// them, but the one its scanner rejects: after `:a"`, YAML 1.2 goes on
/// with a plain scalar, and ayaml reads a quoted one, which keeps U+FFFF.
/// One after the scanner's error, which ayaml never reads, goes too.
#[test]
fn quoted_only_characters_are_found_where_ayaml_reads_them() {
    let input = ":a\"\r\n'\u{ffff}'b c, ? {: ''\u{ffff}'\n!!int \n\nTRUE- 007\n\n  ";
    assert!(!places(input)[6].quoted);
    assert_eq!(
        quoted_only_characters(input)[0],
        ":a\"\r\n''b c, ? {: ''\u{ffff}'\n!!int \n\nTRUE- 007\n\n  "
    );
    assert_eq!(quoted_only_characters("|-\"\u{9f}\"a\n")[0], "|-\"\"a\n");
}
