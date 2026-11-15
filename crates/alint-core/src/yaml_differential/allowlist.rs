//! The documented differences between ayaml and the oracle. A difference no
//! entry explains fails the harness.
//!
//! [`DEVIATIONS`] covers the differences in what the parsers accept and the
//! values they read: each names the inputs it covers, both behaviors and the
//! reason. [`LOCATIONS`] covers the inputs both reject at different places:
//! each names the ayaml errors it covers and where each parser reports them,
//! so an ayaml error not listed there fails until its place is documented.

use ayaml::ErrorKind;

use super::{Case, Class, Entry, Rejection, Side};

/// One documented difference in what the parsers accept or read.
pub(crate) struct Deviation {
    /// A short name, used in the report.
    pub(crate) id: &'static str,
    /// The classes of result it explains.
    pub(crate) classes: &'static [Class],
    /// The inputs and results it covers.
    pub(crate) covers: fn(&Case<'_>) -> bool,
    /// What ayaml does.
    pub(crate) ayaml: &'static str,
    /// What the oracle does.
    pub(crate) oracle: &'static str,
    /// Why alint accepts the difference.
    pub(crate) reason: &'static str,
    /// An input it covers, at the value entry point.
    pub(crate) example: &'static str,
}

const ALL_DIFFERENCES: &[Class] = &[
    Class::DifferentValue,
    Class::OnlyAyamlRejects,
    Class::OnlyOracleRejects,
];

fn rejects(side: &Side, message: &str) -> bool {
    side.as_ref()
        .err()
        .is_some_and(|rejection| rejection.message.contains(message))
}

fn oracle_rejects(case: &Case<'_>, messages: &[&str]) -> bool {
    messages
        .iter()
        .any(|message| rejects(&case.oracle, message))
}

fn ayaml_rejects(case: &Case<'_>, messages: &[&str]) -> bool {
    messages.iter().any(|message| rejects(&case.ayaml, message))
}

/// Whether some line's first non-blank characters are `:` followed by white
/// space or the end of the line: an entry with an empty key.
fn has_empty_key(input: &str) -> bool {
    input.lines().any(|line| {
        let rest = line.trim_start_matches([' ', '\t', '-', '?']);
        rest.strip_prefix(':')
            .is_some_and(|after| after.is_empty() || after.starts_with([' ', '\t']))
    })
}

/// Whether an anchor or alias name holds a character other than a letter, a
/// digit, `-` or `_`.
fn has_wide_anchor_name(input: &str) -> bool {
    let mut previous = ' ';
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if (ch == '&' || ch == '*') && (previous.is_whitespace() || ",[{".contains(previous)) {
            let mut wide = false;
            while let Some(&next) = chars.peek() {
                if next.is_whitespace() || ",[]{}".contains(next) {
                    break;
                }
                wide |= !(next.is_alphanumeric() || next == '-' || next == '_');
                chars.next();
            }
            if wide {
                return true;
            }
        }
        previous = ch;
    }
    false
}

/// Whether the oracle's error points at a `#` inside a tag: the text before
/// it on its line, back to white space or a flow indicator, starts with `!`.
fn oracle_points_into_a_tag_at_a_number_sign(case: &Case<'_>) -> bool {
    let Err(Rejection {
        location: Some((line, column)),
        ..
    }) = &case.oracle
    else {
        return false;
    };
    let Some(text) = case.input.lines().nth(line - 1) else {
        return false;
    };
    let before: String = text.chars().take(column - 1).collect();
    text.chars().nth(column - 1) == Some('#')
        && before
            .rsplit(|ch: char| ch.is_whitespace() || ",[]{}".contains(ch))
            .next()
            .is_some_and(|token| token.starts_with('!'))
}

/// Whether an anchor name is defined more than once.
fn has_redefined_anchor(input: &str) -> bool {
    let mut seen = std::collections::HashSet::new();
    input
        .split(|ch: char| ch.is_whitespace() || ",[]{}".contains(ch))
        .filter_map(|token| token.strip_prefix('&'))
        .any(|name| !seen.insert(name))
}

/// Whether a flow entry or value is a lone `-`, `?` or `:` before a flow
/// indicator, as in `[-]` or `{a: -, b}`.
fn has_lone_flow_indicator(input: &str) -> bool {
    let chars: Vec<char> = input.chars().collect();
    (0..chars.len()).any(|at| {
        let lone = "-?:".contains(chars[at])
            && (at == 0 || chars[at - 1].is_whitespace() || "[{,".contains(chars[at - 1]))
            && chars
                .get(at + 1)
                .is_some_and(|after| after.is_whitespace() || "]},".contains(*after));
        let before = chars[..at].iter().rposition(|ch| !ch.is_whitespace());
        let after = chars[at + 1..].iter().find(|ch| !ch.is_whitespace());
        lone && after.is_some_and(|after| "]},".contains(*after))
            && before.is_some_and(|before| {
                "[{,".contains(chars[before])
                    || (chars[before] == ':' && chars[before + 1].is_whitespace())
            })
    })
}

/// Whether a line of white space with a tab follows a `...` line.
fn has_tab_line_after_document_end(input: &str) -> bool {
    input
        .lines()
        .skip_while(|line| !line.starts_with("..."))
        .any(|line| line.contains('\t') && line.trim().is_empty())
}

/// Whether a property, white space, and then a `:` directly before a flow
/// collection appear, as in `[&a :[b]]`.
fn has_property_before_adjacent_flow_value(input: &str) -> bool {
    input.match_indices(':').any(|(at, _)| {
        let before = &input[..at];
        input[at + 1..].starts_with(['[', '{'])
            && before.ends_with([' ', '\t'])
            && before
                .trim_end()
                .rsplit(|ch: char| ch.is_whitespace() || ",[{".contains(ch))
                .next()
                .is_some_and(|token| token.starts_with(['&', '!']))
    })
}

/// Whether a line ends with properties that include an anchor, and the next
/// line, at no greater indentation, starts with a block scalar header.
fn has_block_scalar_under_an_anchor(input: &str) -> bool {
    let indentation = |line: &str| line.len() - line.trim_start_matches(' ').len();
    let lines: Vec<&str> = input.lines().collect();
    lines.windows(2).any(|pair| {
        let tokens: Vec<&str> = pair[0].split_whitespace().rev().collect();
        let properties: Vec<&str> = tokens
            .iter()
            .copied()
            .take_while(|token| token.starts_with(['&', '!']))
            .collect();
        properties.iter().any(|token| token.starts_with('&'))
            && pair[1].trim_start().starts_with(['|', '>'])
            && indentation(pair[1]) <= indentation(pair[0])
    })
}

/// Whether the document's root node is a block scalar: the first line with
/// content, past directives, `---` and the root's properties, starts with `|`
/// or `>`.
fn root_is_block_scalar(input: &str) -> bool {
    let lines = input
        .lines()
        .map(|line| line.trim_start_matches('\u{feff}').trim())
        .filter(|line| !line.is_empty() && !line.starts_with(['#', '%']));
    for line in lines {
        let mut rest = line.strip_prefix("---").unwrap_or(line).trim_start();
        while rest.starts_with(['!', '&']) {
            rest = rest
                .split_once(char::is_whitespace)
                .map_or("", |(_, after)| after.trim_start());
        }
        // A line of properties alone, or with a comment, puts them on the
        // node of the next line.
        if !rest.is_empty() && !rest.starts_with('#') {
            return rest.starts_with(['|', '>']);
        }
    }
    false
}

/// Whether a `!` stands alone as a tag: the non-specific tag.
fn has_non_specific_tag(input: &str) -> bool {
    input.char_indices().any(|(at, ch)| {
        ch == '!'
            && !input[..at].ends_with(|before: char| {
                !before.is_whitespace() && before != '[' && before != '{' && before != ','
            })
            && input[at + 1..]
                .chars()
                .next()
                .is_none_or(|after| after.is_whitespace() || ",[]{}".contains(after))
    })
}

pub(crate) const DEVIATIONS: &[Deviation] = &[
    Deviation {
        id: "yaml-1.2-line-breaks",
        classes: ALL_DIFFERENCES,
        covers: |case| case.input.contains(['\u{85}', '\u{2028}', '\u{2029}']),
        ayaml: "reads U+0085, U+2028 and U+2029 as content characters: a plain scalar keeps \
                them, and where they stand between tokens the document is malformed",
        oracle: "reads them as line breaks, as YAML 1.1 did",
        reason: "YAML 1.2 (section 5.4) made them content; documented in the 0.23 CHANGELOG",
        example: "a: x\u{85}y\n",
    },
    Deviation {
        id: "byte-order-mark-starting-a-line",
        classes: ALL_DIFFERENCES,
        covers: |case| case.input.lines().any(|line| line.starts_with('\u{feff}')),
        ayaml: "skips a byte-order mark that starts the stream or a document without \
                counting it as a column",
        oracle: "skips one at the start of any line but counts it as a column, so a \
                 structure that lines up with that line is misread or rejected",
        reason: "a libyaml defect; ayaml reads such files as their author meant",
        example: "\u{feff}- a\n- b\n",
    },
    Deviation {
        id: "tab-as-white-space",
        classes: &[Class::OnlyOracleRejects, Class::DifferentValue],
        covers: |case| {
            (case.input.contains('\t')
                && oracle_rejects(
                    case,
                    &[
                        "found character that cannot start any token",
                        "found a tab character",
                    ],
                ))
                || (has_tab_line_after_document_end(case.input)
                    && oracle_rejects(case, &["more than one document"]))
        },
        ayaml: "reads a tab as separating white space where YAML 1.2 allows one, such as \
                after `-`, before a top-level scalar, or on a blank line after `...`",
        oracle: "rejects a tab where a token could start, and reads a line of white space \
                 with a tab after `...` as a second document",
        reason: "YAML 1.2 (section 6.2 and rule 78) allows tabs as separation outside \
                 indentation and on blank lines",
        example: "- a\n-\tb\n",
    },
    Deviation {
        id: "empty-implicit-key",
        classes: &[Class::OnlyOracleRejects],
        covers: |case| {
            has_empty_key(case.input) && oracle_rejects(case, &["did not find expected key"])
        },
        ayaml: "reads `: value` as an entry whose key is null",
        oracle: "rejects an implicit key with no content",
        reason: "YAML 1.2 (rule 194) allows an empty implicit key",
        example: ": v\n",
    },
    Deviation {
        id: "document-end-marker-alone",
        classes: &[Class::OnlyOracleRejects],
        covers: |case| {
            case.input
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty() && !line.starts_with('#'))
                .is_some_and(|line| line.starts_with("..."))
        },
        ayaml: "reads a stream that starts with `...` as having no document, which is null",
        oracle: "rejects it",
        reason: "YAML 1.2 (rule 211) allows a document end marker with no document",
        example: "...\n",
    },
    Deviation {
        id: "integers-beyond-64-bits",
        classes: &[Class::OnlyOracleRejects],
        covers: |case| case.entry == Entry::Value && oracle_rejects(case, &["as u128", "as i128"]),
        ayaml: "keeps an integer outside the 64-bit range as a 128-bit integer",
        oracle: "rejects the document, since its value has no 128-bit integer",
        reason: "a wider integer is not an error; the JSON entry point rejects both alike",
        example: "a: 18446744073709551616\n",
    },
    Deviation {
        id: "anchor-name-characters",
        classes: ALL_DIFFERENCES,
        covers: |case| {
            has_wide_anchor_name(case.input)
                || oracle_rejects(case, &["did not find expected alphabetic or numeric"])
        },
        ayaml: "reads any character but white space and the flow indicators as part of an \
                anchor or alias name, so `&a: x` anchors `x` as `a:`",
        oracle: "ends the name at the first character other than a letter, a digit, `-` or \
                 `_`, so `&a: x` is a mapping",
        reason: "YAML 1.2 (rule 102) allows the wider set",
        example: "&a\"1 x",
    },
    Deviation {
        id: "redefined-anchor",
        classes: &[Class::DifferentValue, Class::OnlyOracleRejects],
        covers: |case| has_redefined_anchor(case.input),
        ayaml: "resolves an alias to the latest node before it with that anchor",
        oracle: "resolves an alias to an anchor defined more than once to the wrong node, \
                 which can be one that holds the alias, and then fails with \"recursion \
                 limit exceeded\"",
        reason: "a `yaml_serde` defect; YAML 1.2 (section 3.2.2.2) says an alias refers to \
                 the most recent preceding node with the anchor",
        example: "- &a 1\n- &a 2\n- *a\n- &b 3\n",
    },
    Deviation {
        id: "yaml-1.2-flow-syntax",
        classes: &[Class::OnlyOracleRejects],
        covers: |case| {
            case.input.contains(['[', '{'])
                && oracle_rejects(
                    case,
                    &[
                        "did not find expected node content",
                        "found unexpected ':'",
                        "did not find expected ',' or",
                    ],
                )
        },
        ayaml: "reads YAML 1.2's flow forms: a plain scalar that starts with or holds `:` \
                before a non-space (`[:a]`, `{a:b: c}`), an entry with an empty key (`[: x]`, \
                `{:}`, `[? ]`), an implicit key with an empty value (`[a:]`), and a flow \
                mapping key that spans lines",
        oracle: "rejects them, as YAML 1.1 did",
        reason: "YAML 1.2 (rules 130, 144, 147 and 153) allows them",
        example: "[:a]\n",
    },
    Deviation {
        id: "flow-indicator-needs-white-space",
        classes: &[Class::OnlyAyamlRejects],
        covers: |case| {
            ayaml_rejects(case, &["a flow collection's `?` must be separated"])
                || (ayaml_rejects(
                    case,
                    &["did not find expected node content, while parsing a flow"],
                ) && has_lone_flow_indicator(case.input))
                || (ayaml_rejects(case, &["did not find expected ',' or"])
                    && has_property_before_adjacent_flow_value(case.input))
        },
        ayaml: "rejects a flow entry that is only `-` before a flow indicator, as `[-]`, a \
                `?` not followed by white space, as `{?}`, and a `:` directly before a flow \
                collection after an empty key with properties, as `[&a :[b]]`",
        oracle: "reads `-` as a plain scalar, `?` as an explicit key, and the collection as \
                 the value",
        reason: "YAML 1.2 lets `-` start a plain scalar only before a safe character, which a \
                 flow indicator is not (rule 126); an explicit key's `?` needs separation \
                 after it (rule 142), and so does the `:` of a key that is not JSON-like \
                 (section 7.4)",
        example: "[-]\n",
    },
    Deviation {
        id: "tag-handle-characters",
        classes: &[Class::OnlyAyamlRejects],
        covers: |case| {
            case.ayaml.as_ref().err().is_some_and(|rejection| {
                rejection
                    .message
                    .strip_prefix("undefined tag handle \"")
                    .and_then(|rest| rest.split('"').next())
                    .is_some_and(|handle| {
                        handle
                            .trim_matches('!')
                            .chars()
                            .any(|ch| !(ch.is_ascii_alphanumeric() || ch == '-'))
                    })
            })
        },
        ayaml: "reads a tag such as `!e:a!z` as the handle `!e:a!`, which is undefined",
        oracle: "reads it as a local tag",
        reason: "YAML 1.2 (rules 92 and 99) allows neither reading, since a tag's name \
                 cannot hold a second `!`; the document is malformed",
        example: "a: !e:a!z x\n",
    },
    Deviation {
        id: "number-sign-in-a-tag",
        classes: &[Class::OnlyOracleRejects],
        covers: |case| {
            oracle_rejects(case, &["did not find expected whitespace or line break"])
                && oracle_points_into_a_tag_at_a_number_sign(case)
        },
        ayaml: "reads `#` as part of a tag, as in `!t#`",
        oracle: "rejects `#` in a tag",
        reason: "YAML 1.2 (rules 39 and 40) allows `#` in a tag",
        example: "- !t# x\n",
    },
    Deviation {
        id: "reserved-directives",
        classes: &[Class::OnlyOracleRejects],
        covers: |case| {
            oracle_rejects(
                case,
                &[
                    "found unknown directive name",
                    "could not find expected directive name",
                    "found unexpected non-alphabetical character",
                ],
            )
        },
        ayaml: "ignores a directive other than `%YAML` and `%TAG`",
        oracle: "rejects it",
        reason: "YAML 1.2 (section 6.8) reserves such directives and says to ignore them",
        example: "%FOO bar\n---\na\n",
    },
    Deviation {
        id: "later-yaml-minor-version",
        classes: &[Class::OnlyOracleRejects],
        covers: |case| {
            oracle_rejects(
                case,
                &[
                    "found incompatible YAML document",
                    "found extremely long version number",
                ],
            ) && case.input.lines().any(|line| {
                line.strip_prefix("%YAML")
                    .is_some_and(|version| version.trim_start().starts_with("1."))
            })
        },
        ayaml: "reads a document under `%YAML 1.x` for any minor version",
        oracle: "rejects a minor version other than 1 and 2, or one of more than nine digits",
        reason: "YAML 1.2 (section 6.8.1) says to read a later minor version as 1.2, with a \
                 warning",
        example: "%YAML 1.3\n---\na\n",
    },
    Deviation {
        id: "top-level-block-scalar-content",
        classes: &[Class::OnlyOracleRejects],
        covers: |case| root_is_block_scalar(case.input),
        ayaml: "reads the lines after a top-level block scalar header, at any indentation, \
                as its content",
        oracle: "ends the scalar at the first line not indented past column 1, and then \
                 reports a second document",
        reason: "YAML 1.2 (section 8.1.1.1) sets a top-level scalar's content indentation \
                 from its first line, which may be column 1",
        example: "|\nx\n",
    },
    Deviation {
        id: "core-tag-on-another-kind",
        classes: ALL_DIFFERENCES,
        covers: |case| {
            [
                "!!map", "!!seq", "!!str", "!!int", "!!float", "!!bool", "!!null",
            ]
            .iter()
            .any(|tag| case.input.contains(tag))
        },
        ayaml: "ignores a core tag that names another kind of node, such as `!!map` on a \
                scalar, and resolves the node as if untagged",
        oracle: "reads such a scalar as a string",
        reason: "the document is malformed either way, and real files do not hold it",
        example: "a: !!map 1\n",
    },
    Deviation {
        id: "global-tags",
        classes: ALL_DIFFERENCES,
        covers: |case| {
            ["!<", "%TAG", "!!"]
                .iter()
                .any(|tag| case.input.contains(tag))
        },
        ayaml: "keeps a global tag on its node: a verbatim tag such as `!<tag:x,2000:y>`, a \
                tag from a `%TAG` handle, or `!!` with a name outside the core schema; where \
                tags are dropped, it reads the node as if untagged",
        oracle: "drops the tag and reads a tagged scalar as a string",
        reason: "a global tag names an application type; neither reading is wrong, and \
                 alint's configurations and queries use local tags or none",
        example: "a: !<tag:x,2000:y> 1\n",
    },
    Deviation {
        id: "ayaml-error-without-a-place",
        classes: &[Class::RejectedAtDifferentLocation],
        covers: |case| {
            case.input.contains('\u{feff}')
                && case.ayaml.as_ref().err().is_some_and(|rejection| {
                    rejection.location.is_none()
                        && rejection
                            .message
                            .contains("found character that cannot start any token")
                })
        },
        ayaml: "reports a character that cannot start a token after a byte-order mark \
                without a line and column",
        oracle: "reports the same error with its place",
        reason: "an ayaml defect in a malformed document (asamarts/ayaml#133); the message \
                 still names the problem",
        example: "\u{feff}%YAML 1.2\n",
    },
    Deviation {
        id: "non-specific-tag",
        classes: &[Class::DifferentValue],
        covers: |case| has_non_specific_tag(case.input),
        ayaml: "reads a node tagged `!` as a string",
        oracle: "keeps the tag as `!!` and resolves the scalar as if untagged",
        reason: "YAML 1.2 (section 6.9.1) resolves a `!`-tagged scalar as a string",
        example: "a: ! 1\n",
    },
    Deviation {
        id: "comment-needs-white-space",
        classes: &[Class::OnlyAyamlRejects],
        covers: |case| {
            ayaml_rejects(
                case,
                &[
                    "a comment must be separated from the preceding token",
                    "a block scalar header comment requires preceding whitespace",
                ],
            ) || (ayaml_rejects(case, &["invalid %YAML directive"])
                && case.input.lines().any(|line| {
                    line.starts_with("%YAML")
                        && line
                            .char_indices()
                            .any(|(at, ch)| ch == '#' && !line[..at].ends_with(char::is_whitespace))
                }))
        },
        ayaml: "rejects a `#` that directly follows a token, such as `'x'#c` or \
                `%YAML 1.2#`",
        oracle: "reads it as a comment",
        reason: "YAML 1.2 (rule 77) requires white space before a comment",
        example: "a: 'x'#c\n",
    },
    Deviation {
        id: "quoted-scalar-continuation-indentation",
        classes: &[Class::OnlyAyamlRejects],
        covers: |case| {
            ayaml_rejects(
                case,
                &[
                    "quoted scalar continuation must be indented more than its parent",
                    "tabs cannot be the only quoted-scalar indentation",
                ],
            )
        },
        ayaml: "rejects a quoted scalar's continuation line that is not indented past its \
                parent",
        oracle: "accepts it",
        reason: "YAML 1.2 (rule 125) requires the indentation",
        example: "a: 'x\ny'\n",
    },
    Deviation {
        id: "block-scalar-under-an-anchor",
        classes: &[Class::OnlyOracleRejects, Class::DifferentValue],
        covers: |case| has_block_scalar_under_an_anchor(case.input),
        ayaml: "accepts a block scalar header on the line after properties that include an \
                anchor, as `b: &x` then `>`, even when it is not indented past its parent",
        oracle: "rejects it",
        reason: "an ayaml defect in a malformed document (asamarts/ayaml#132): with a tag \
                 alone, ayaml rejects the same layout, as YAML 1.2 (section 8.1) does",
        example: "a:\n  b: &x\n>\n",
    },
    Deviation {
        id: "block-scalar-indentation",
        classes: &[Class::OnlyAyamlRejects],
        covers: |case| {
            ayaml_rejects(
                case,
                &[
                    "a block scalar is not indented past its parent",
                    "leading empty block-scalar lines are more indented than the content",
                ],
            )
        },
        ayaml: "rejects a block scalar whose header or content is not indented past its \
                parent, such as `-` followed by `|2` at the same column",
        oracle: "accepts it",
        reason: "YAML 1.2 (section 8.1.1.1) requires the indentation; documented in the 0.23 \
                 CHANGELOG",
        example: "-\n|2\n  x\n",
    },
];

/// The first entry that covers `case`.
pub(crate) fn explain(case: &Case<'_>) -> Option<&'static Deviation> {
    DEVIATIONS
        .iter()
        .find(|deviation| deviation.classes.contains(&case.class) && (deviation.covers)(case))
}

/// The ayaml errors whose place may differ from the oracle's, and why.
pub(crate) struct Location {
    /// A short name, used in the report.
    pub(crate) id: &'static str,
    /// The error kinds it covers.
    pub(crate) kinds: &'static [ErrorKind],
    /// The messages it covers, as text the message contains; none means every
    /// message of those kinds.
    pub(crate) messages: &'static [&'static str],
    /// Where ayaml reports them.
    pub(crate) ayaml: &'static str,
    /// Where the oracle reports the error it finds instead.
    pub(crate) oracle: &'static str,
    /// An input it covers, at the value entry point.
    pub(crate) example: &'static str,
}

/// Where the oracle reports, whatever ayaml's error: its first error in the
/// input, or "more than one document" with no place.
const ORACLE: &str = "the first error in the input as it reads, often where its look-ahead \
                      stopped (the end of a simple key, a quoted scalar or the input); once \
                      the first document ends, `yaml_serde` reports more than one document, \
                      with no place, whatever follows";

pub(crate) const LOCATIONS: &[Location] = &[
    Location {
        id: "after-the-root-node",
        kinds: &[ErrorKind::Parser, ErrorKind::Scanner],
        messages: &[
            "unexpected content after document",
            "unexpected token after node",
            "more than one document in stream",
            "content after document end marker",
            "in node position",
            "directive must be followed by a document start",
            "directive without a document",
            "document markers are not allowed inside flow collections",
            "expected a single YAML document",
            "a byte order mark can only start a document",
        ],
        ayaml: "the first token that cannot continue the document once its root node is \
                complete",
        oracle: ORACLE,
        example: "[]\n]",
    },
    Location {
        id: "where-a-construct-opened",
        kinds: &[ErrorKind::Parser, ErrorKind::Scanner, ErrorKind::Tag],
        messages: &[
            "unterminated",
            "did not find expected node content, while parsing a flow",
            "did not find expected ',' or",
            "flow collection entries must be indented",
            "quoted scalar continuation must be indented",
            "tabs cannot be the only",
            "document marker inside quoted scalar",
            "expected ',' or a closing bracket",
            "expected a flow item",
            "malformed flow",
            "unexpected empty flow",
        ],
        ayaml: "where an unterminated quoted scalar, escape or verbatim tag opened, or the \
                token where a flow collection, which its message names, cannot go on",
        oracle: ORACLE,
        example: "'",
    },
    Location {
        id: "indentation",
        kinds: &[ErrorKind::Parser, ErrorKind::Scanner],
        messages: &[
            "indented less than its parent",
            "is not indented past its parent",
            "a mapping line is missing",
            "implicit mapping keys must be on one line",
            "a block collection cannot start on",
            "leading empty block-scalar lines",
            "tab cannot",
            "a tab cannot",
            "properties cannot precede",
            "a document-start property cannot precede",
            "mapping values are not allowed in this context",
            "tabs are not allowed",
            "explicit mapping key must be followed",
            "an implicit mapping key may be at most",
        ],
        ayaml: "the line whose indentation, checked against its parent's as it is read, \
                cannot continue the structure",
        oracle: ORACLE,
        example: "?a\n- : ",
    },
    Location {
        id: "at-the-token",
        kinds: &[ErrorKind::Parser, ErrorKind::Scanner, ErrorKind::Tag],
        messages: &[
            "is a reserved indicator",
            "in block scalar header",
            "a block scalar header comment",
            "a comment must be separated",
            "anchor or alias must be followed",
            "anchor/alias name must not be empty",
            "an anchor name cannot contain",
            "aliases cannot carry",
            "a node cannot have duplicate",
            "a tag must be followed",
            "a tag's handle needs a suffix",
            "a verbatim tag",
            "undefined tag handle",
            "hexadecimal escape",
            "unknown escape",
            "a directive needs a name",
            "a directive cannot contain",
            "%YAML directive",
            "%TAG",
            "must be separated from what follows it",
            "unexpected token in mapping key",
            "found character that cannot start any token",
            "invalid !!",
            "%-escape",
            "block scalar header must end",
            "escape is not a Unicode scalar value",
            "undefined primary tag handle",
            "undefined secondary tag handle",
            "duplicate %YAML",
            "a `%` in a verbatim tag",
            "expected a node",
            "expected a token",
        ],
        ayaml: "the start of the token that breaks the grammar",
        oracle: ORACLE,
        example: "a: 'x'#c\nb",
    },
    Location {
        id: "values-after-syntax",
        kinds: &[
            ErrorKind::Alias,
            ErrorKind::DuplicateKey,
            ErrorKind::Resolution,
            ErrorKind::Composition,
            ErrorKind::Serde,
            ErrorKind::Limit,
        ],
        messages: &[],
        ayaml: "the node a value error is about; ayaml checks the whole stream's syntax \
                before it builds values, so a later syntax error wins over an earlier \
                duplicate key, unknown anchor, recursive alias or type error",
        oracle: "the first error in the input, as it builds values while it reads, at its \
                 own position for a value error (often the start of the enclosing \
                 collection)",
        example: "&a [*a]\n",
    },
];

/// The location class that covers an ayaml error of `kind` with `message`.
pub(crate) fn locate(kind: ErrorKind, message: &str) -> Option<&'static Location> {
    LOCATIONS.iter().find(|location| {
        location.kinds.contains(&kind)
            && (location.messages.is_empty()
                || location
                    .messages
                    .iter()
                    .any(|covered| message.contains(covered)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_predicates_match_only_their_forms() {
        assert!(has_empty_key(": v\n") && has_empty_key("- :\n") && !has_empty_key("a: :b\n"));
        assert!(has_wide_anchor_name("&a: x") && has_wide_anchor_name("[*a.b]"));
        assert!(!has_wide_anchor_name("&a-1_b x\n*a_1") && !has_wide_anchor_name("a&b: c"));
        assert!(has_redefined_anchor("- &a 1\n- &a 2") && !has_redefined_anchor("- &a 1\n- &b 2"));
        assert!(has_lone_flow_indicator("[-]") && has_lone_flow_indicator("{a: -, b}"));
        assert!(has_lone_flow_indicator("[a, ? ]") && has_lone_flow_indicator("[\n  -\n]"));
        for other in ["[-1]", "[a-]", "{a: -1}", "{a:-, b}", "- [a]", "[a, b-]"] {
            assert!(!has_lone_flow_indicator(other), "{other:?}");
        }
        assert!(has_tab_line_after_document_end("a\n...\n\t\n"));
        assert!(
            !has_tab_line_after_document_end("a\n\t\n...\n")
                && !has_tab_line_after_document_end("a\n...\nb\t\n")
        );
        assert!(root_is_block_scalar("|\nx\n") && root_is_block_scalar("--- !t &a\n>-\n x\n"));
        assert!(root_is_block_scalar("%YAML 1.2\n# c\n--- |\nx\n"));
        assert!(!root_is_block_scalar("a: |\n x\n") && !root_is_block_scalar("- |\n x\n"));
        assert!(
            has_non_specific_tag("a: ! 1")
                && has_non_specific_tag("[!, x]")
                && has_non_specific_tag("!")
        );
        assert!(
            !has_non_specific_tag("a: !t 1")
                && !has_non_specific_tag("a: b! 1")
                && !has_non_specific_tag("!!str x")
        );
    }

    #[test]
    fn flow_value_and_block_scalar_predicates_match_only_their_forms() {
        assert!(has_property_before_adjacent_flow_value("[&a :[b]]"));
        assert!(has_property_before_adjacent_flow_value(
            "{x: 1, !t :{b: c}}"
        ));
        for other in [
            "{a:[b]}",
            "{\"a\":[b]}",
            "[&a : [b]]",
            "[&a, :[b]]",
            "a: &x:[b]",
        ] {
            assert!(!has_property_before_adjacent_flow_value(other), "{other:?}");
        }
        assert!(has_block_scalar_under_an_anchor("a:\n  b: &x\n>\n"));
        assert!(has_block_scalar_under_an_anchor("-\n  <<: &a1 !t\n|-"));
        for other in [
            "a:\n  b: !t\n|\n",
            "a: &x\n  |\n  y\n",
            "a: &x |\n x\n",
            "&x\n|\n",
        ] {
            let expected = other == "&x\n|\n";
            assert_eq!(
                has_block_scalar_under_an_anchor(other),
                expected,
                "{other:?}"
            );
        }
        assert!(root_is_block_scalar("&b  # c\n>\nx\n"));
    }

    fn oracle_error_at(input: &str, line: usize, column: usize) -> Case<'_> {
        Case {
            input,
            entry: Entry::Value,
            class: Class::OnlyOracleRejects,
            ayaml: Ok(String::new()),
            oracle: Err(Rejection {
                message: "did not find expected whitespace or line break".to_string(),
                location: Some((line, column)),
                kind: None,
            }),
        }
    }

    #[test]
    fn a_number_sign_counts_only_inside_a_tag() {
        assert!(oracle_points_into_a_tag_at_a_number_sign(&oracle_error_at(
            "- !t# x\n",
            1,
            5
        )));
        assert!(oracle_points_into_a_tag_at_a_number_sign(&oracle_error_at(
            "a\n[!# x]\n",
            2,
            3
        )));
        assert!(!oracle_points_into_a_tag_at_a_number_sign(
            &oracle_error_at("- !t #x\n", 1, 6)
        ));
        assert!(!oracle_points_into_a_tag_at_a_number_sign(
            &oracle_error_at("- a# x\n", 1, 4)
        ));
        assert!(!oracle_points_into_a_tag_at_a_number_sign(
            &oracle_error_at("- !tx x\n", 1, 5)
        ));
        assert!(!oracle_points_into_a_tag_at_a_number_sign(
            &oracle_error_at("!t#\n", 3, 1)
        ));
    }

    #[test]
    fn locate_needs_the_kind_and_a_listed_message() {
        assert_eq!(
            locate(ErrorKind::Parser, "unterminated escape").map(|l| l.id),
            Some("where-a-construct-opened")
        );
        assert_eq!(
            locate(ErrorKind::DuplicateKey, "anything").map(|l| l.id),
            Some("values-after-syntax")
        );
        assert!(locate(ErrorKind::Parser, "a message no class lists").is_none());
        assert!(locate(ErrorKind::Io, "unterminated escape").is_none());
    }
}
