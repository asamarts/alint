//! Differences that long runs left unexplained before the search and the
//! rewrites were fixed for them, with the entries that explain them now.

use super::*;

const FINDINGS: &[(&str, Entry, &[&str])] = &[
    // Both reject the rewrite as a duplicate key, worded differently for a
    // null key.
    (
        "null: -3\r\n!!map null: !!str .nan\r\nb: 's ''q'''\r\n",
        Entry::Value,
        &["core-tag-on-another-kind"],
    ),
    // `!` on an empty flow entry becomes `''`, not nothing.
    ("[1,!  ,]\n", Entry::Value, &["non-specific-tag"]),
    // A wide integer as 1 would meet the key 1, an error the rewrite made.
    (
        "? 1\n18446744073709551616: x\n",
        Entry::Value,
        &["integers-beyond-64-bits"],
    ),
    (
        "true: \n&a1 1: 's ''q'''\n118446744073709551616: []\n",
        Entry::Value,
        &["integers-beyond-64-bits"],
    ),
    // Each `...` before any content goes, not only the first.
    ("...\n... #c", Entry::Value, &["document-end-marker-alone"]),
    (
        "...\n...\n  - ---",
        Entry::Value,
        &["document-end-marker-alone"],
    ),
    // A plain scalar that starts with `:` after a tag, in a flow sequence.
    (
        "- [!!str a/**, !!str :a/**]\n",
        Entry::Value,
        &["flow-colon-scalar-after-properties"],
    ),
    // A `...` below a tag is no node of the tag's.
    (
        "\n\n\n \n\n  \n!!map \n\n...",
        Entry::Value,
        &["core-tag-on-another-kind"],
    ),
    // A flow value that starts with `:` before a quote.
    ("- {key: :'q,}\n", Entry::Value, &["yaml-1.2-flow-syntax"]),
    // A `%TAG` prefix with `#` after a `,`.
    (
        "%TAG !e! tag:x,#2000:\n---\n[-2.5,\n  's', 's']\n",
        Entry::Value,
        &["number-sign-in-a-tag"],
    ),
    // A `%TAG` prefix that starts with `#` (seed 19).
    (
        "%TAG !e! #tag:x,2000:\n---\n[[1],x]\n",
        Entry::Value,
        &["number-sign-in-a-tag"],
    ),
    // The node on the line after `- ` continues there, so `'` opens no
    // quote and the `:` line is an empty key.
    (
        "b:\n- \n  ~,\n  'q]]\n:\n",
        Entry::Value,
        &["empty-implicit-key"],
    ),
    // A block scalar header after a key with an anchor.
    (
        "true:\n  &a2 null:\n>\n",
        Entry::Value,
        &["block-scalar-under-an-anchor"],
    ),
    // ayaml's duplicate key, reported where the mapping starts, and the
    // oracle's empty key, which hides it: without the empty key, both give
    // the duplicate key, at the place where each reports it.
    (
        "key: {key: x}\n: \n? key\n",
        Entry::Value,
        &["empty-implicit-key"],
    ),
    // A tab before a comment, or a tab line after a CR, after `...`.
    ("a: 1\n...\n\t# c\n", Entry::Value, &["tab-as-white-space"]),
    (
        "- 2001-12-14\r\n...\r\t\n",
        Entry::Value,
        &["tab-as-white-space"],
    ),
    // A leading byte-order mark hides ayaml's error on the quoted scalar's
    // continuation line (asamarts/ayaml#137); without it, both reject, at
    // different places, and without the tab, ayaml's error is the
    // continuation line's. The NEL in the block scalar after it matters to
    // neither result.
    (
        "\u{feff}-\n  -\t\"d\\\n  \\u00e9\\t\"\n  - \"d\\_\\u00e9\\t\"\n  -\n    - |\n      \
         line 0\u{85}\n    - \n- []\n# end",
        Entry::Value,
        &[
            "byte-order-mark-starting-a-line",
            "tab-as-white-space",
            "quoted-scalar-continuation-indentation",
        ],
    ),
    // Shapes the parser facade's compare mode found in the depth guard's
    // tests, which the generator had not made. A second leading mark is
    // content for both, and the first moves it to column 1 for the oracle.
    (
        "\u{feff}\u{feff}  k: |2 #\nb: [1]\nc: \"\"\n",
        Entry::Value,
        &["byte-order-mark-starting-a-line"],
    ),
    (
        "\u{feff}\u{feff}|k: |2 #x.&ak: |\n  k: \"\nb: [[[1]]]\nc: \"\"\n",
        Entry::Value,
        &["byte-order-mark-starting-a-line"],
    ),
    // ayaml's line reader takes `a:` for the name and the quote line for
    // the value's continuation (asamarts/ayaml#139).
    (
        "&a:[: x\ny\n",
        Entry::Value,
        &[
            "anchor-name-before-a-colon-and-a-flow-indicator",
            "after-the-root-node",
        ],
    ),
    (
        "&a:[: x\n\"\n",
        Entry::Value,
        &[
            "anchor-name-before-a-colon-and-a-flow-indicator",
            "where-a-construct-opened",
        ],
    ),
    (
        "&a:[: x\n\"\nb: [[[1]]]\nc: \"\"\n",
        Entry::Value,
        &[
            "anchor-name-before-a-colon-and-a-flow-indicator",
            "where-a-construct-opened",
        ],
    ),
    // Deleting `!!int ` first on its line left the byte-order mark at the
    // stream's start, so `%TAG` became a directive (seed 3); the local tag of
    // the same width moves nothing.
    (
        "!!int \u{feff}%TAG !e! tag:x,2000:\n<<: \u{2028}",
        Entry::Json,
        &["core-tag-on-another-kind"],
    ),
    (
        "!!int \u{feff}%TAG !e! tag:x,2000:\n<<: x",
        Entry::Json,
        &["core-tag-on-another-kind"],
    ),
    // A comment line less indented than a sequence entry's plain scalar,
    // which starts on the line after the entry's `-`, read as the owner of
    // that scalar, so the next entry's line seemed to continue it and its
    // tab to be text (seed 7).
    (
        "- 1e3 # c\n- [-2.5,\n  1\u{feff}]\n-\n  -\n # c\n    line 0\n    line 1\n    \
         line 2\n  \n  - !!binary |+2\n    line 0\n    line 1\n  -\t|\n",
        Entry::Value,
        &["tab-as-white-space", "global-tags"],
    ),
    // A top-level plain scalar goes on at column 1, so the `"` there is its
    // text, not a quoted scalar the tab after it would be in (seed 11).
    (
        "2001-12-14'a b'\u{2029}\n\"!!str 1.5|2!<tag:x,2000:y>  #c\n\t\n",
        Entry::Json,
        &["yaml-1.2-line-breaks", "tab-as-white-space"],
    ),
    // The line separator, which both parsers read alike in a quoted scalar,
    // plays no part beside ayaml's comment error (the review of #525).
    (
        "- 'x'#c\n- 'd\u{2029}e'\n",
        Entry::Value,
        &["comment-needs-white-space"],
    ),
    // The oracle's reader rejects the input at the first character it does
    // not allow, DEL in a quoted scalar, which ayaml keeps, and ayaml at
    // U+FFFF in a plain one: two errors. The rewrite of quoted-only
    // characters leaves that one, which both reject, and the oracle's error
    // is then about it (seed 1).
    (
        "'\u{7f}'\n  - \n--- b c---'\u{ffff}'False-",
        Entry::Value,
        &["quoted-only-characters", "non-printable-character"],
    ),
    // A first line of a tab alone, which the oracle rejects, is a prefix
    // that the rest reads as well without (seed 1).
    (
        "\t\n\n\n\n\n\n  - \n\n  \n- \"e\\n\\u0041\":%TAG !e! tag:x,2000:\n\r\n\t? ",
        Entry::Value,
        &["tab-as-white-space", "indentation"],
    ),
    // A tab alone on a line after a block scalar's content, which ayaml
    // reads, and one before another's content, which both reject: only the
    // first is the tab entry's (seed 2).
    (
        "- >\n    line 0\n\t\n  line 1\n-\n  - 2001-12-14\n- &a1 |+2\n\t\n  line 0\u{2028}\n  \
         line 1\u{2028}\n# end\n",
        Entry::Value,
        &["tab-as-white-space"],
    ),
    // `-0`, which the oracle refuses in an unsigned field (seed 7).
    (
        "version: !!int '1'\nfix_size_limit: -0\nnested_configs: false\nrules:\n  - &rule0\n    \
         id: \"r0\"\n    kind: filename_case\n    level: !!str info\n    when: facts.is_rust\n    \
         case: kebab\n",
        Entry::Config,
        &[
            "typed-decode-core-tags",
            "negative-zero-in-an-unsigned-field",
        ],
    ),
    // Leading empty lines more indented than content at column 1, which the
    // oracle ends the scalar before, reading the rest as a second document
    // (seed 1).
    (
        " >\n  \na \"\\x\"\n    \t---:a1_000key12:30TRUE1",
        Entry::Value,
        &["block-scalar-indentation"],
    ),
    // A `:` that starts a line holds the value of the `?` key before it, and
    // a split there would leave it an empty key (seed 2).
    (
        ": !<!> 0.5\n? !<!> [a b,\n  ~]\n: &a1  # c\n",
        Entry::Value,
        &["empty-implicit-key", "verbatim-non-specific-tag"],
    ),
    // ayaml's flow-line tab error, which it gives first since it reads the
    // stream's tokens before it parses them, hides the parser error that both
    // give before it: two errors, not one found at two places (seed 2).
    (
        "- >-\n  line 0\n    line 1\n  line 2\n - [!!str [-2.5,\n  ~,\n  -2.5],\n\t  a b]\n\
         - {a b: {null: ~,\n\t  b: true}}\n",
        Entry::Value,
        &["flow-line-tab-indentation"],
    ),
    (
        "\t  null,\n{a b: [\"d\",\n\t  a b], null: 'q, true: 's'}\n",
        Entry::Value,
        &[
            "tab-as-white-space",
            "flow-line-tab-indentation",
            "after-the-root-node",
        ],
    ),
    (
        "  - ~\n\t\n  line 0\n? [a b,{1: -2.5},\n\t  -2.5]\n:\n  - \ntrue: >2-\n  - |\n\n    \
         line 0\n  \n",
        Entry::Value,
        &[
            "tab-as-white-space",
            "flow-line-tab-indentation",
            "indentation",
        ],
    ),
    (
        "\u{feff}\t'a b'? [1>+%\n- \u{85}<<: \n   \n\t0o17",
        Entry::Value,
        &[
            "byte-order-mark-starting-a-line",
            "tab-as-white-space",
            "flow-line-tab-indentation",
        ],
    ),
    // ayaml keeps DEL in a quoted scalar, and rejects U+009F where its
    // scanner reads a plain scalar that line 7 continues; the oracle's error
    // is about DEL (seed 2).
    (
        "'\u{7f}'\n  - \n\n    \r\n\tNull\n'a b'\"\\U0001F600\"12:30-20o17\ta#b\n  - \"\u{9f}\"",
        Entry::Value,
        &["quoted-only-characters", "non-printable-character"],
    ),
    // ayaml's reader rejects a C0 control before its scanner rejects a
    // quoted-only character outside a quoted scalar, so whether it keeps
    // U+009F here the input does not show (bounded run).
    (
        ", @1?a\r\n\"\u{9f}\"x\u{1}y?: \n",
        Entry::Value,
        &["non-printable-character"],
    ),
    // A leading line of spaces and a tab, not past the parent's column,
    // after which ayaml ends the scalar and reads the line as white space,
    // and the oracle rejects the tab (seed 2).
    (
        "\n- >+ \n\n\t \n\r\n.inf\n    '0x1F\n'a b'*b",
        Entry::Value,
        &["tab-as-white-space"],
    ),
    (
        "--- # c\na b: \nb:\n  b:\t|\n \t\n    line 0\n\n    line 1\n    line 2\n  key: \n",
        Entry::Value,
        &["tab-as-white-space"],
    ),
    (
        "1: &a1 \na b:\n  - 2001-12-14\n  - \n  - |+2\n \t\n    line 0\n    line 1 # c\n",
        Entry::Value,
        &["tab-as-white-space"],
    ),
    // A block scalar header under an anchor, indented less than the root,
    // where the oracle ends the document (seed 2).
    (
        " a: &a1\n|+\n  line 0\n...\n",
        Entry::Value,
        &["block-scalar-under-an-anchor"],
    ),
    // The same past a directive and `---`, and under a tag alone on its
    // line (seeds 7 and 11).
    (
        "%YAML 1.2\n---\n a b: &a1\n|+\n  line 0\n\n",
        Entry::Value,
        &["block-scalar-under-an-anchor"],
    ),
    (
        "\n  - \n  - \r\n\n\n     \n\n    !!map \n>",
        Entry::Value,
        &["block-scalar-under-an-anchor"],
    ),
    // ayaml's scanner opens a flow collection where YAML 1.2 goes on with a
    // plain scalar, and rejects the tab on its next line (seed 2).
    (
        "  true,\n[{a: é},\n\t  é]\n# end\n",
        Entry::Value,
        &["flow-line-tab-indentation", "after-the-root-node"],
    ),
    // The oracle rejects a later flow-line tab in a plain scalar, after an
    // error ayaml's first tab hides: only ayaml's tab is rewritten (seed 1).
    (
        "? [[],\n\t  'q,]\n:\n  a: no\n  a:\n    - !!null 's ''q'''\n    - y z\n    -\t~\n  -\n  \
         - |+\n    line 0\n\n    line 1\n\t\n    line 2\n  - !t \n1: 's ''q''' # c\n",
        Entry::Value,
        &["flow-line-tab-indentation", "where-a-construct-opened"],
    ),
    // A tab ayaml rejects is not white space, and the tab entry leaves it
    // (seeds 3 and 7).
    (
        "a b: &a1 []\n\t\n  - &a2 \"d\\e\\u00e9\\t\"\n  - |+\n1:\n    line 0\n\t\n    line 1\n\
         &a3 b: .nan\n# end\n",
        Entry::Value,
        &["tab-as-white-space", "indentation"],
    ),
    (
        "key: |+\n    line 0\n    line 1\n\t\n  line 2\n\n\nk1: 18446744073709551616\n\t\n  \
         line 0\n&a1 null: !!binary |+2\n  line 1\n",
        Entry::Value,
        &["tab-as-white-space"],
    ),
    // An empty implicit key in a flow mapping, where ayaml rejects a later
    // line the oracle never reads (seed 11).
    (
        "!t -a-0.0 <<: { \t:}\n\n\n    2001-12-14-\n1.5 \nTRUE,",
        Entry::Value,
        &["empty-flow-implicit-key"],
    ),
    // A line break the oracle reads in an LS makes `!<!>` a tag (seed 3).
    (
        "a b:\u{2028} !<!> 's ''q'''\nnull: \n&a1 k1: \"d\u{85}\\r\\u00e9\\t\"\n",
        Entry::Json,
        &["yaml-1.2-line-breaks", "verbatim-non-specific-tag"],
    ),
    // ayaml reads a quote that YAML 1.2 reads in a plain scalar, after
    // `:a"`, and keeps the U+FFFF in it that the oracle refuses; both
    // reject the next (seed 2).
    (
        ":a\"\r\n'\u{ffff}'b c, ? {: ''\u{ffff}'\n!!int \n\nTRUE- 007\n\n  ",
        Entry::Value,
        &["quoted-only-characters", "non-printable-character"],
    ),
    // ayaml reads a tab that leads its line, after a block scalar's
    // content, as white space with no indentation, and the oracle rejects
    // it: the rewrite leaves it out with the spaces after it, which a space
    // would add to (seed 11, shortened).
    (
        "-\t# c\n- |\n   x\n\t  'q,\n  y\n---\n",
        Entry::Value,
        &["tab-as-white-space"],
    ),
    // `!<!>` before a character with no white space between: named, it
    // keeps the error both parsers give there (seed 2).
    (
        "1: &a1 !<!>*b 0.5\r\n",
        Entry::Value,
        &["verbatim-non-specific-tag"],
    ),
    // `!<!>` on an empty key, which the oracle reads: named, the key keeps
    // its tag, where without it the key would be empty (seed 3).
    (
        "- !<!> : 0o7\n- \n- \n",
        Entry::Value,
        &["verbatim-non-specific-tag"],
    ),
    // ayaml reads a tag in what YAML 1.2 reads as a plain scalar's second
    // line, after ` ---`, which is no marker, and rejects it there: its
    // error hides its parser error, which the oracle's matches (seed 1).
    (
        " ---\n!<!> {a: a b, 1: a b,}\n",
        Entry::Value,
        &["verbatim-non-specific-tag", "after-the-root-node"],
    ),
    // `!<>` names nothing, which both parsers reject, with ayaml's message
    // about `!<!>` (seed 7).
    (
        "&a1 a: &a2 !<>!> \nb: &a3 no # c\u{2028}\n",
        Entry::Value,
        &["verbatim-tag-that-names-nothing"],
    ),
    // U+FFFF ends a tag: ayaml's error that white space does not follow
    // the tag is about the character the oracle's reader refuses (seed 2).
    (
        "!'\u{ffff}'!str {}\n",
        Entry::Value,
        &["non-printable-character"],
    ),
    // ayaml's error at a C1 control is about the line it starts, which is
    // not indented enough: the rewrite keeps a character there (seed 2,
    // shortened).
    (
        "a: 'x\n\u{8a}'\n",
        Entry::Value,
        &[
            "quoted-only-characters",
            "quoted-scalar-continuation-indentation",
        ],
    ),
    // `!<!>` named as `!<!t>` is no global tag, whose rewrites would spend
    // the search's comparisons before it reaches the explanation (seed 7).
    (
        "null: &a1 |\n    line 0\n  line 1\n\t\n  line 2\n  \na b:\t\"d\\n\\u00e9\\t\"\n1:\n  -\n  \
         - {k1: \"\\x41\",\n\t  k1: \"\\x41\",\n\t  k1\u{feff}: \"d\",}\n  -\n    k1: \n  -\n    1\
         :\ttrue\n    a:      - !!null \n      - !<!> 0x10\n      - 's ''q'''\n    key:\n      key\
         : 's ''q'''\n      null: 's\u{ffff} ''q'''\n      a: 0o7 # c\n",
        Entry::Value,
        &[
            "quoted-only-characters",
            "flow-line-tab-indentation",
            "verbatim-non-specific-tag",
            "tab-as-white-space",
            "after-the-root-node",
        ],
    ),
    // A `#` in a verbatim tag that names a local tag, which the oracle
    // rejects: no global tag (seed 7).
    ("!<!a#b> {1: 1}\n", Entry::Json, &["number-sign-in-a-tag"]),
    // U+FFFF in a verbatim tag that names a local tag: ayaml's error about
    // the tag is at the character the oracle's reader refuses (seed 7).
    (
        "- &a1 !<!'\u{ffff}'> [a b]\n- \n",
        Entry::Value,
        &["non-printable-character"],
    ),
    // A flow key with an empty value, `:}`, which the oracle rejects, then
    // a directive after the document, which ayaml rejects: with the flow
    // collection empty, the oracle reads on to a second document (seed 4).
    (
        "? {-a.nan\tFalse%'open \n  :}\n%YAML 1.2\n\n.inf--- \n",
        Entry::Value,
        &["yaml-1.2-flow-syntax", "after-the-root-node"],
    ),
    // The same after a root flow sequence (seed 10).
    (
        "[:12:30😀'open,-0.0  ]\n    TRUE\n  - &b \n\n \n0o17- \u{85}*a\n  - [key-2",
        Entry::Value,
        &["yaml-1.2-flow-syntax", "after-the-root-node"],
    ),
    // A block scalar header after a key with an anchor, past which ayaml
    // reads to a quote no line ends: with the header past the key, the
    // oracle does too (seed 4).
    (
        "- \n- &b on: \n|\n  \"unterminated?\t|2",
        Entry::Value,
        &["block-scalar-under-an-anchor", "where-a-construct-opened"],
    ),
    // The same, to a verbatim tag that names nothing (seed 8).
    (
        "k1:\n  - 1e3\n  -\n    &a1 true:\n      - &a2 true: &a3\n>\n  line 0\n  line 1\n\n  \
         line 2\n  \ntrue:\n-\n  -\n    - !<!> null\n    -\t*a2 # c\n  - 's ''q'''\n- \
         [\"d\"]\n-\n  ? a b\n  :\t.nan\t# c\n  b: \n  k1:\n    a: *a3\n    <<: \
         18446744073709551616\n    <<: 2001-12-14\n",
        Entry::Value,
        &[
            "verbatim-non-specific-tag",
            "block-scalar-under-an-anchor",
            "after-the-root-node",
        ],
    ),
    // A tab before the `]` that ends a flow collection's line after a plain
    // scalar's (seeds 4 and 9).
    (
        "b: &a2 [a b,\nb: &a1 0x10\n\t  ]\na: *a1 # c\n",
        Entry::Value,
        &["tab-as-white-space"],
    ),
    (
        "- 0x10\n- [~\u{2028},1\n\t  ]\n# end\n",
        Entry::Value,
        &["yaml-1.2-line-breaks", "tab-as-white-space"],
    ),
    // A byte-order mark after `- ` starts a plain scalar, in which a quote
    // opens nothing, so the `- :` line is an empty key (seed 6).
    (
        "\n- \u{feff}'q''s'\"\"e\\n\\u0041\"!e!z ? [\n- on\t\t,\n- : ",
        Entry::Value,
        &["empty-implicit-key"],
    ),
    // A line of a tab ends a block scalar for ayaml, which then rejects a
    // tab as the leading indentation of a scalar it opens below; with the
    // first a space, it reads the second as white space too (seed 8).
    (
        "<<: >-\n  line 0\n  line 1\n\n\n  null:\n    -\t>+\n      line 0\n\t\n        \
         line 1\n      line 2\n\n    - >-\n\t\n      line 0\n  key: .nan\n",
        Entry::Value,
        &["tab-as-white-space"],
    ),
    // Tabs ayaml reads as white space, then one it rejects, which it still
    // does with them as content (`x`): that variant, which keeps it,
    // explains the case, where the one that frees it too does not (seed 2).
    (
        "- >+\n  line 0\n\t\n  line 1\n # c\n- [a b,\n- |2\n  line 0\n\t\n  \
         line 1\n\t\n    line 2\n",
        Entry::Value,
        &["tab-as-white-space", "indentation"],
    ),
    // Once the core tags go, ayaml reports the second document it reports
    // only after the first one's values (seeds 9 and 12).
    (
        "key: {}\n1:\n  - 's\u{80} ''q'''\n  - &a1 null\n  - !!null |\n  - !!null |\n    \
         line 0\n\n # c\n1: *a1\n---\nb: 2\n",
        Entry::Value,
        &[
            "quoted-only-characters",
            "core-tag-on-another-kind",
            "after-the-root-node",
        ],
    ),
    (
        "!!int :a%~%TAG !e! tag:x,2000:\n--- &a.inf007&a\n    1_000\tb c%%TAG !e! tag:x,2000:\n \
         'a b'\n    \n    \r\n18446744073709551616\n    !t ",
        Entry::Json,
        &["core-tag-on-another-kind", "after-the-root-node"],
    ),
    // A line that starts with `%` continues the root's plain scalar, so the
    // quote after it is the scalar's text, not a quoted scalar that holds
    // the tab after the comment (seed 17).
    ("1\n%\n\" #\n\t", Entry::Value, &["tab-as-white-space"]),
    ("1\n%\n\" #\n\t", Entry::Json, &["tab-as-white-space"]),
    ("1\n%x\n'a #\n\t \n", Entry::Value, &["tab-as-white-space"]),
    ("1\n%x\n'a #\n\t \n", Entry::Json, &["tab-as-white-space"]),
];

#[test]
fn long_run_findings_are_explained() {
    for (input, entry, expected) in FINDINGS {
        let (case, panicked) = compare(input, *entry);
        assert!(!panicked && case.class.is_difference(), "{case:?}");
        assert_eq!(explanation(&case).as_deref(), Some(*expected), "{input:?}");
    }
    // A long flow key cut to 1,000 bytes would meet a key of 1,000, an
    // error the rewrite made.
    let input = format!(
        "{{a: 1,{}: ~,{}: }}\n",
        "k".repeat(1_000),
        "k".repeat(1_042)
    );
    let (case, _) = compare(&input, Entry::Value);
    assert_eq!(case.class, Class::OnlyOracleRejects, "{case:?}");
    assert_eq!(
        explanation(&case),
        Some(vec!["long-flow-mapping-key"]),
        "{case:?}"
    );
}
