//! The documented differences between ayaml and the oracle. A difference no
//! entry explains fails the harness.
//!
//! [`DEVIATIONS`] covers the differences in what the parsers accept and the
//! values they read: each names the inputs it covers, both behaviors and the
//! reason. [`LOCATIONS`] covers the inputs both reject at different places:
//! each names the ayaml errors it covers and where each parser reports them,
//! so an ayaml error not listed there fails until its place is documented.
//!
//! An entry explains a case only if its trigger causes the difference. A
//! [`Trigger::Narrow`] entry reads a parser's error, often with the input at
//! the place the error points to, and that error names the cause. A
//! [`Trigger::Input`] entry looks for a construct anywhere in the input, so
//! the construct's presence proves nothing: the entry explains a case only
//! when the input without the construct no longer differs (see
//! [`super::explanation`]).

use ayaml::ErrorKind;

use super::neutralize;
use super::predicates::{
    ORACLE_READER_ERROR, VERBATIM_NEEDS_A_NAME, ayaml_points_at_a_flow_value_after_a_property,
    ayaml_points_at_a_lone_flow_indicator,
    ayaml_points_at_a_yaml_directive_with_an_adjacent_number_sign, ayaml_rejects,
    ayaml_rejects_a_nameless_verbatim_tag, ayaml_takes_a_document_marker_for_block_scalar_content,
    errors_about_one_character, errors_around_an_empty_node, errors_in_an_empty_verbatim_tag,
    has_tab_line_after_document_end, oracle_points_at_a_block_scalar_under_an_anchor,
    oracle_points_at_a_flow_form, oracle_points_into_a_tag_at_a_number_sign, oracle_rejects,
    oracle_rejects_in_a_quote_after_a_flow_colon, rejects, root_is_block_scalar,
};
use super::{Case, Class, Entry};

/// One documented difference in what the parsers accept or read.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "documentation, which the tests and the long run's report read"
    )
)]
pub(crate) struct Deviation {
    /// A short name, used in the report.
    pub(crate) id: &'static str,
    /// The classes of result it explains.
    pub(crate) classes: &'static [Class],
    /// How it tells the cases it explains.
    pub(crate) trigger: Trigger,
    /// What ayaml does.
    pub(crate) ayaml: &'static str,
    /// What the oracle does.
    pub(crate) oracle: &'static str,
    /// Why alint accepts the difference.
    pub(crate) reason: &'static str,
    /// An input it explains on its own, at the entry point it is about: the
    /// value entry point, [`Entry::Config`] for an entry about typed decoding,
    /// or [`Entry::Rule`] for one about decoding rules.
    pub(crate) example: &'static str,
}

/// How a [`Deviation`] tells the cases it explains.
pub(crate) enum Trigger {
    /// A check of the results that names the cause itself.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "documentation, which the tests and the long run's report read"
        )
    )]
    Narrow {
        covers: fn(&Case<'_>) -> bool,
        /// Why the check names the cause, and so needs no counterfactual.
        because: &'static str,
    },
    /// A construct anywhere in the input.
    Input {
        /// A check of the results; whether the input holds the construct is
        /// up to `neutralize`.
        covers: fn(&Case<'_>) -> bool,
        /// The input without the construct, as rewrites to try in order, or
        /// none when it holds none.
        neutralize: fn(&str) -> Vec<String>,
    },
}

impl Deviation {
    /// Whether the entry's check holds for `case`.
    pub(crate) fn covers(&self, case: &Case<'_>) -> bool {
        self.classes.contains(&case.class)
            && match self.trigger {
                Trigger::Narrow { covers, .. } | Trigger::Input { covers, .. } => covers(case),
            }
    }

    /// The ayaml errors that its trigger is documented to hide: removing the
    /// trigger may make ayaml reject what it read, with one of them, where
    /// the oracle rejected the case.
    pub(crate) fn hides(&self) -> Hidden {
        match self.id {
            // asamarts/ayaml#137.
            "byte-order-mark-starting-a-line" => Hidden::Messages(&[
                "quoted scalar continuation must be indented more than its parent",
            ]),
            // asamarts/ayaml#139.
            "anchor-name-before-a-colon-and-a-flow-indicator" => Hidden::AfterTheTrigger,
            // Each is a scanner error.
            "flow-line-tab-indentation" | "verbatim-non-specific-tag" => Hidden::ParserErrors,
            _ => Hidden::Nothing,
        }
    }

    /// The entry point its example is about.
    #[cfg(test)]
    pub(crate) fn example_entry(&self) -> Entry {
        match self.id {
            "typed-decode-core-tags" | "negative-zero-in-an-unsigned-field" => Entry::Config,
            "rule-keys-that-are-not-strings" | "rule-that-is-null" => Entry::Rule,
            _ => Entry::Value,
        }
    }
}

/// The ayaml errors a [`Deviation`]'s trigger hides.
pub(crate) enum Hidden {
    Nothing,
    /// The errors with one of these messages.
    Messages(&'static [&'static str]),
    /// Any error on a line after the trigger's: the trigger makes ayaml read
    /// the lines after it as part of a scalar, whatever they hold.
    AfterTheTrigger,
    /// A parser error, when the trigger is a scanner error: ayaml reads the
    /// whole stream's tokens before it parses them, so a scanner error hides
    /// a parser error before it, which a rewrite that leaves the tokens as
    /// they were, only moved along their line, did not make.
    ParserErrors,
}

impl Hidden {
    /// Whether `next`, `case` with the trigger removed, shows an error the
    /// trigger hid from ayaml.
    pub(crate) fn shown(&self, case: &Case<'_>, next: &Case<'_>) -> bool {
        let Err(error) = &next.ayaml else {
            return false;
        };
        match self {
            Self::Nothing | Self::ParserErrors => false,
            Self::Messages(messages) => messages
                .iter()
                .any(|message| error.message.contains(message)),
            Self::AfterTheTrigger => {
                let changed = case
                    .input
                    .bytes()
                    .zip(next.input.bytes())
                    .position(|(before, after)| before != after)
                    .unwrap_or(case.input.len().min(next.input.len()));
                let trigger_line = case.input[..changed].matches('\n').count() + 1;
                error.location.is_some_and(|(line, _)| line > trigger_line)
            }
        }
    }

    /// Whether ayaml's error on `next`, `case` with the trigger removed, is
    /// one that the case's error, the trigger's, hid wherever it was.
    pub(crate) fn revealed(&self, case: &Case<'_>, next: &Case<'_>) -> bool {
        let kind = |side: &super::Side| side.as_ref().err().and_then(|rejection| rejection.kind);
        matches!(self, Self::ParserErrors)
            && kind(&case.ayaml) == Some(ErrorKind::Scanner)
            && kind(&next.ayaml) == Some(ErrorKind::Parser)
    }
}

const ALL_DIFFERENCES: &[Class] = &[
    Class::DifferentValue,
    Class::OnlyAyamlRejects,
    Class::OnlyOracleRejects,
];

fn any(_: &Case<'_>) -> bool {
    true
}

fn value_tree(case: &Case<'_>) -> bool {
    matches!(case.entry, Entry::Value | Entry::Json)
}

fn typed(case: &Case<'_>) -> bool {
    matches!(case.entry, Entry::Config | Entry::Rule)
}

/// A message of the oracle's that its scanner and parser give inside a flow
/// collection only.
fn oracle_rejects_in_a_flow_collection(case: &Case<'_>) -> bool {
    oracle_rejects(
        case,
        &[
            "while parsing a flow",
            "found unexpected ':'",
            "did not find expected ',' or",
        ],
    )
}

/// `{`, a key of 1,025 characters, and `: v}`.
const LONG_FLOW_KEY: &str = {
    const BYTES: [u8; 1_031] = {
        let mut bytes = [b'k'; 1_031];
        bytes[0] = b'{';
        let tail = b": v}\n";
        let mut at = 0;
        while at < tail.len() {
            bytes[1_026 + at] = tail[at];
            at += 1;
        }
        bytes
    };
    match std::str::from_utf8(&BYTES) {
        Ok(text) => text,
        Err(_) => panic!("the example is ASCII"),
    }
};

/// A key of 513 `é`, 1,026 bytes, and `: 1`.
const WIDE_KEY: &str = {
    const BYTES: [u8; 1_030] = {
        let mut bytes = [0; 1_030];
        let mut at = 0;
        while at < 1_026 {
            bytes[at] = 0xC3;
            bytes[at + 1] = 0xA9;
            at += 2;
        }
        bytes[1_026] = b':';
        bytes[1_027] = b' ';
        bytes[1_028] = b'1';
        bytes[1_029] = b'\n';
        bytes
    };
    match std::str::from_utf8(&BYTES) {
        Ok(text) => text,
        Err(_) => panic!("the example is UTF-8"),
    }
};

pub(crate) const DEVIATIONS: &[Deviation] = &[
    Deviation {
        id: "yaml-1.2-line-breaks",
        classes: ALL_DIFFERENCES,
        trigger: Trigger::Input {
            covers: any,
            neutralize: neutralize::line_breaks,
        },
        ayaml: "reads U+0085, U+2028 and U+2029 as content characters: a plain scalar keeps \
                them, and where they stand between tokens the document is malformed",
        oracle: "reads them as line breaks, as YAML 1.1 did",
        reason: "YAML 1.2 (section 5.4) made them content; documented in the 0.23 CHANGELOG",
        example: "a: x\u{85}y\n",
    },
    Deviation {
        id: "byte-order-mark-starting-a-line",
        classes: ALL_DIFFERENCES,
        trigger: Trigger::Input {
            covers: any,
            neutralize: neutralize::line_start_byte_order_marks,
        },
        ayaml: "skips a byte-order mark that starts the stream without counting it as a \
                column, and counts one that starts another line as a column, as the oracle \
                does; after the one that starts the stream, it does not check that a quoted \
                scalar's continuation line under a nested parent is indented past it",
        oracle: "counts the one that starts the stream as a column too, so a structure \
                 that lines up with the first line is misread or rejected. Both read a \
                 second mark right after it as content, which YAML 1.2 does not allow \
                 (asamarts/ayaml#140), so it shifts the first line by one column for the \
                 oracle only",
        reason: "a libyaml defect; ayaml reads such files as their author meant. The missed \
                 indentation check is an ayaml defect in a malformed document \
                 (asamarts/ayaml#137), which the oracle accepts with or without the mark",
        example: "\u{feff}- a\n- b\n",
    },
    Deviation {
        id: "quoted-only-characters",
        classes: &[Class::OnlyOracleRejects],
        trigger: Trigger::Input {
            covers: |case| oracle_rejects(case, &[ORACLE_READER_ERROR]),
            neutralize: neutralize::quoted_only_characters,
        },
        ayaml: "keeps U+007F, U+0080 to U+009F other than U+0085, U+FFFE and U+FFFF in a \
                single- or double-quoted scalar, and rejects them elsewhere",
        oracle: "rejects them anywhere, as \"control characters are not allowed\"",
        reason: "YAML 1.2 allows any JSON character in a quoted scalar (`nb-json`, rule 2) \
                 and printable characters only elsewhere; documented in the 0.23 CHANGELOG",
        example: "a: '\u{7f}'\n",
    },
    Deviation {
        id: "tab-as-white-space",
        classes: &[Class::OnlyOracleRejects],
        trigger: Trigger::Input {
            covers: |case| {
                oracle_rejects(
                    case,
                    &[
                        "found character that cannot start any token",
                        "found a tab character",
                    ],
                ) || (has_tab_line_after_document_end(case.input)
                    && oracle_rejects(case, &["more than one document"]))
            },
            neutralize: neutralize::tabs,
        },
        ayaml: "reads a tab as separating white space where YAML 1.2 allows one, such as \
                after `-`, before a top-level scalar, or on a blank line after `...`, and as \
                content after the indentation of a block scalar's first line, as in `|` and \
                then a line of two spaces and a tab; and so before a closing `]` or `}` or a \
                comment on a flow collection's line that tabs alone indent, as `a: [x` then \
                `\\t]`",
        oracle: "rejects a tab where a token could start or before a block scalar's \
                 indentation is known, or that starts a flow collection's line after a plain \
                 scalar's under a block collection, and reads a line of white space with a \
                 tab after `...` as a second document",
        reason: "YAML 1.2 (section 6.2, rule 78 and section 8.1.1.1) allows tabs as \
                 separation outside indentation and on blank and comment lines, and sets a \
                 block scalar's indentation by the spaces before its first line's content; \
                 ayaml exempts a flow collection's line that holds only a closing bracket or \
                 a comment from the indentation of its other lines (its 0.1.2 CHANGELOG), as \
                 both parsers read such a line that no space indents",
        example: "- a\n-\tb\n",
    },
    Deviation {
        id: "empty-implicit-key",
        classes: &[Class::OnlyOracleRejects],
        trigger: Trigger::Input {
            covers: |case| oracle_rejects(case, &["did not find expected key"]),
            neutralize: neutralize::empty_keys,
        },
        ayaml: "reads `: value` as an entry whose key is null",
        oracle: "rejects an implicit key with no content",
        reason: "YAML 1.2 (rule 194) allows an empty implicit key",
        example: ": v\n",
    },
    Deviation {
        id: "empty-flow-explicit-key",
        classes: ALL_DIFFERENCES,
        trigger: Trigger::Input {
            covers: |case| !neutralize::empty_flow_keys(case.input).is_empty(),
            neutralize: neutralize::empty_flow_keys,
        },
        ayaml: "reads a `?` with nothing after it in a flow collection, as in `[? ]` or \
                `{? , a: b}`, as an entry whose key and value are null",
        oracle: "takes the `,`, `]` or `}` after the `?` for the end of the key and drops \
                 it, so that it rejects the document further on, where the collection \
                 fails to end, or reads another value, as `[? , a]` for `[? ,, a]`",
        reason: "YAML 1.2 (rules 142, 143 and 150) allows an explicit key with no key or \
                 value in a flow collection",
        example: "[? ]\n",
    },
    Deviation {
        id: "document-end-marker-alone",
        classes: &[Class::OnlyOracleRejects],
        trigger: Trigger::Input {
            covers: any,
            neutralize: neutralize::leading_document_end,
        },
        ayaml: "reads a stream that starts with `...`, with or without a document after it, \
                as if the `...` were not there; with no document, the value is null",
        oracle: "rejects it",
        reason: "YAML 1.2 (rule 211) allows a document end marker with no document before it",
        example: "...\na: 1\n",
    },
    Deviation {
        id: "integers-beyond-64-bits",
        classes: ALL_DIFFERENCES,
        trigger: Trigger::Input {
            covers: |case| {
                case.entry == Entry::Value && !neutralize::wide_integers(case.input).is_empty()
            },
            neutralize: neutralize::wide_integers,
        },
        ayaml: "keeps an integer outside the 64-bit range as a 128-bit integer",
        oracle: "rejects the document, since its value has no 128-bit integer",
        reason: "a wider integer is not an error; the JSON entry point rejects both alike",
        example: "a: 18446744073709551616\n",
    },
    Deviation {
        id: "alias-repetition-limit",
        classes: &[Class::OnlyOracleRejects],
        trigger: Trigger::Narrow {
            covers: |case| oracle_rejects(case, &["repetition limit exceeded"]),
            because: "the message is the oracle's limit on alias uses",
        },
        ayaml: "expands aliases within its byte-weighted alias-expansion allowance, so \
                aliases that fan out, as ten aliases to a node of ten aliases to a node of \
                ten, are read",
        oracle: "stops after 100 alias uses per event of the document, with \"repetition \
                 limit exceeded\"",
        reason: "the allowance bounds what a document can expand to, which is what a limit \
                 is for; a count of uses refuses small valid documents. Documented in the \
                 0.23 CHANGELOG",
        example: "- &a [x]\n- &b [*a, *a, *a, *a, *a, *a, *a, *a, *a, *a]\n\
                  - &c [*b, *b, *b, *b, *b, *b, *b, *b, *b, *b]\n\
                  - &d [*c, *c, *c, *c, *c, *c, *c, *c, *c, *c]\n\
                  - [*d, *d, *d, *d, *d, *d, *d, *d, *d, *d]\n",
    },
    Deviation {
        id: "anchor-name-characters",
        classes: ALL_DIFFERENCES,
        trigger: Trigger::Input {
            covers: any,
            neutralize: neutralize::wide_anchor_names,
        },
        ayaml: "reads any character but white space and the flow indicators as part of an \
                anchor or alias name, except, as libyaml does, a `?` before a character other \
                than a flow indicator and, outside a flow collection, a `:` before a character \
                other than white space; so `&a: x` anchors `x` as `a:`, `&a!t x` as `a!t`, \
                and `[*a:b]` is an alias to `a:b`",
        oracle: "ends the name at the first character other than an ASCII letter, a digit, \
                 `-` or `_`, so `&a: x` is a mapping, `[*a:b]` the mapping `{*a: b}`, and \
                 `&é x` an error",
        reason: "YAML 1.2 (rule 102) allows the wider set",
        example: "&a\"1 x",
    },
    Deviation {
        id: "anchor-name-before-a-colon-and-a-flow-indicator",
        classes: ALL_DIFFERENCES,
        trigger: Trigger::Input {
            covers: any,
            neutralize: neutralize::names_ended_at_a_colon_before_a_flow_indicator,
        },
        ayaml: "ends an anchor name at a `:` that a flow indicator follows, outside a flow \
                collection, so `&a:[: x` is the anchor `a` on the key `:[`, as the oracle \
                reads it; but its line reader, which decides indentation, reads that line \
                as the anchor `a:` and an unclosed `[`, so the value on it continues onto \
                lines at any indentation",
        oracle: "ends the value at a line indented no more than the key",
        reason: "an ayaml defect in a malformed document: YAML 1.2 does not allow a flow \
                 indicator right after an anchor name, and ayaml's own reader rejects the \
                 line (asamarts/ayaml#139)",
        example: "&a:[: x\n- y\n",
    },
    Deviation {
        id: "redefined-anchor",
        classes: &[
            Class::DifferentValue,
            Class::OnlyAyamlRejects,
            Class::OnlyOracleRejects,
        ],
        trigger: Trigger::Input {
            covers: any,
            neutralize: neutralize::redefined_anchors,
        },
        ayaml: "resolves an alias to the latest node before it with that anchor, and \
                rejects the alias when that node holds it",
        oracle: "resolves an alias to an anchor defined more than once to the wrong node, \
                 such as an earlier one, or one that holds the alias, and then fails with \
                 \"recursion limit exceeded\"",
        reason: "a `yaml_serde` defect; YAML 1.2 (section 3.2.2.2) says an alias refers to \
                 the most recent preceding node with the anchor",
        example: "- &a 1\n- &a 2\n- *a\n- &b 3\n",
    },
    Deviation {
        id: "long-flow-mapping-key",
        classes: &[Class::OnlyOracleRejects],
        trigger: Trigger::Input {
            covers: |case| oracle_rejects(case, &["did not find expected ',' or"]),
            neutralize: neutralize::long_runs,
        },
        ayaml: "reads an implicit key of more than 1,024 characters in a flow mapping, as \
                `{` and a key of 1,025 characters before `: v}`",
        oracle: "allows an implicit key of 1,024 bytes at most, and rejects a longer one",
        reason: "YAML 1.2 limits an implicit key to 1,024 characters in a block mapping and in \
                 the single pair of a flow sequence entry (`ns-s-implicit-yaml-key`, \
                 `c-s-implicit-json-key`), where ayaml rejects a longer one as the oracle \
                 does, but not in a flow mapping; documented in the 0.23 CHANGELOG",
        example: LONG_FLOW_KEY,
    },
    Deviation {
        id: "implicit-key-length-in-characters",
        classes: &[Class::OnlyOracleRejects],
        trigger: Trigger::Input {
            covers: |case| {
                oracle_rejects(
                    case,
                    &[
                        "mapping values are not allowed",
                        "could not find expected ':'",
                        "did not find expected ',' or",
                    ],
                )
            },
            neutralize: neutralize::wide_runs,
        },
        ayaml: "counts the 1,024-character limit on an implicit key in characters",
        oracle: "counts it in bytes, so a key of 513 two-byte characters is too long",
        reason: "YAML 1.2 (`ns-s-implicit-yaml-key`, `c-s-implicit-json-key`) limits an \
                 implicit key to 1,024 characters; documented in the 0.23 CHANGELOG",
        example: WIDE_KEY,
    },
    // A form of `yaml-1.2-flow-syntax` that a rewrite removes, so that the
    // search can read past it where another error follows.
    Deviation {
        id: "empty-flow-implicit-key",
        classes: &[Class::OnlyOracleRejects, Class::RejectedAtDifferentLocation],
        trigger: Trigger::Input {
            covers: |case| !neutralize::empty_flow_implicit_keys(case.input).is_empty(),
            neutralize: neutralize::empty_flow_implicit_keys,
        },
        ayaml: "reads an entry of a flow collection with an empty implicit key, as `[: x]` \
                and `{:}`, with a null key",
        oracle: "rejects it, and reads `~:` there",
        reason: "YAML 1.2 (`c-ns-flow-map-empty-key-entry`) allows it",
        example: "[: x]\n",
    },
    Deviation {
        id: "yaml-1.2-flow-syntax",
        classes: &[Class::OnlyOracleRejects],
        trigger: Trigger::Narrow {
            covers: |case| {
                (oracle_rejects_in_a_flow_collection(case) && oracle_points_at_a_flow_form(case))
                    || (oracle_rejects(case, &["while scanning a quoted scalar"])
                        && oracle_rejects_in_a_quote_after_a_flow_colon(case))
            },
            because: "the oracle's message is one it gives inside a flow collection only, and \
                      its error points at one of these forms outside a quoted scalar and a \
                      comment: a `:` that opens an entry or a value or that a flow indicator \
                      or `?` follows, the `:` after a key that a line feed or carriage return breaks, \
                      or the message is about a quoted scalar that runs from a quote directly after a `:` that opens a flow entry to \
                      the error, and ayaml read the collection",
        },
        ayaml: "reads YAML 1.2's flow forms: a plain scalar that starts with or holds `:` \
                before a non-space (`[:a]`, `[:'q]`, `{a:b: c}`), an entry with an empty key \
                (`[: x]`, \
                `{:}`), an implicit key with an empty value (`[a:]`), and a flow \
                mapping key that spans lines",
        oracle: "rejects them, as YAML 1.1 did",
        reason: "YAML 1.2 (rules 130, 144, 147 and 153) allows them",
        example: "[:a]\n",
    },
    Deviation {
        id: "flow-colon-scalar-after-properties",
        classes: ALL_DIFFERENCES,
        trigger: Trigger::Input {
            covers: |case| !neutralize::flow_colon_scalars(case.input).is_empty(),
            neutralize: neutralize::flow_colon_scalars,
        },
        ayaml: "reads a plain scalar that starts with `:` before a non-space, after \
                properties that start a node in a flow collection, as that scalar: \
                `[!!str :a]` holds the string `:a`",
        oracle: "reads the properties as an empty key's and the rest as its value, so \
                 `[!!str :a]` holds the mapping `{\"\": a}`, and rejects the entry in a flow \
                 mapping",
        reason: "YAML 1.2 lets a plain scalar start with `:` before a safe character (rule \
                 126), and a flow node's content come after its properties; documented in \
                 the 0.23 CHANGELOG",
        example: "[!!str :a]\n",
    },
    Deviation {
        id: "flow-indicator-needs-white-space",
        classes: &[Class::OnlyAyamlRejects],
        trigger: Trigger::Narrow {
            covers: |case| {
                ayaml_rejects(case, &["a flow collection's `?` must be separated"])
                    || (ayaml_rejects(
                        case,
                        &["did not find expected node content, while parsing a flow"],
                    ) && ayaml_points_at_a_lone_flow_indicator(case))
                    || (ayaml_rejects(case, &["did not find expected ',' or"])
                        && ayaml_points_at_a_flow_value_after_a_property(case))
            },
            because: "ayaml's message is about a flow collection, and names the `?` or points \
                      at the lone indicator or the collection after the `:`",
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
        trigger: Trigger::Narrow {
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
            because: "ayaml's message names the handle, which holds a character a named \
                      handle cannot",
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
        trigger: Trigger::Narrow {
            covers: |case| {
                oracle_rejects(
                    case,
                    &[
                        "did not find expected whitespace or line break",
                        // A verbatim tag.
                        "did not find the expected '>'",
                        // A `%TAG` prefix that starts with `#`.
                        "did not find expected tag URI",
                    ],
                ) && oracle_points_into_a_tag_at_a_number_sign(case)
            },
            because: "the oracle's error points at a `#` inside a tag or a `%TAG` prefix",
        },
        ayaml: "reads `#` as part of a tag, a verbatim one too, or a `%TAG` prefix, as in \
                `!t#` or `!<!a#b>`",
        oracle: "rejects `#` there",
        reason: "YAML 1.2 (rules 39 and 40) allows `#` in a tag and a tag prefix",
        example: "- !t# x\n",
    },
    Deviation {
        id: "reserved-directives",
        classes: &[Class::OnlyOracleRejects],
        trigger: Trigger::Narrow {
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
            because: "the oracle's messages are those of its directive scanner, about a \
                      directive's name",
        },
        ayaml: "ignores a directive other than `%YAML` and `%TAG`",
        oracle: "rejects it",
        reason: "YAML 1.2 (section 6.8) reserves such directives and says to ignore them",
        example: "%FOO bar\n---\na\n",
    },
    Deviation {
        id: "other-yaml-minor-version",
        classes: &[Class::OnlyOracleRejects],
        trigger: Trigger::Narrow {
            covers: |case| {
                oracle_rejects(
                    case,
                    &[
                        "found incompatible YAML document",
                        "found extremely long version number",
                    ],
                ) && case.input.split(['\n', '\r']).any(|line| {
                    line.strip_prefix("%YAML")
                        .is_some_and(|version| version.trim_start().starts_with("1."))
                })
            },
            because: "the oracle's messages are those it gives for a `%YAML` directive's \
                      version, and the input has a `%YAML 1.x` directive",
        },
        ayaml: "reads a document under `%YAML 1.x` for any minor version, earlier or later \
                than 2, as 1.2",
        oracle: "rejects a minor version other than 1 and 2, or one of more than nine digits",
        reason: "YAML 1.2 (section 6.8.1) says to read a later minor version as 1.2, with a \
                 warning, and says nothing of an earlier one, such as 1.0, which ayaml reads \
                 the same way",
        example: "%YAML 1.3\n---\na\n",
    },
    Deviation {
        id: "top-level-block-scalar-content",
        classes: &[Class::OnlyOracleRejects],
        trigger: Trigger::Input {
            covers: |case| root_is_block_scalar(case.input),
            neutralize: neutralize::root_block_scalar_content,
        },
        ayaml: "reads the lines after a top-level block scalar header, at any indentation, \
                as its content",
        oracle: "ends the scalar at the first line not indented past column 1, and reads \
                 what follows as a second document, which it rejects",
        reason: "YAML 1.2 (section 8.1.1.1) sets a top-level scalar's content indentation \
                 from its first line, which may be column 1",
        example: "|\nx\n",
    },
    Deviation {
        id: "core-tag-on-another-kind",
        classes: ALL_DIFFERENCES,
        trigger: Trigger::Input {
            covers: value_tree,
            neutralize: neutralize::core_tags,
        },
        ayaml: "in a value, ignores a core tag that names another kind of node, such as \
                `!!map` on a scalar, and resolves the node as if untagged; an empty node \
                tagged `!!null` is null; and a scalar its core tag does not fit, as \
                `!!int x`, is an error, also as a mapping key",
        oracle: "reads such a scalar as a string, rejects an empty `!!null` node, and in the \
                 JSON tree reads a mapping key by its text whatever its tag",
        reason: "the document is malformed either way, and real files do not hold it; an \
                 empty node is null in the core schema (YAML 1.2, section 10.3.2)",
        example: "a: !!map 1\n",
    },
    Deviation {
        id: "typed-decode-core-tags",
        classes: ALL_DIFFERENCES,
        trigger: Trigger::Input {
            covers: typed,
            neutralize: neutralize::core_tags,
        },
        ayaml: "decodes a scalar into a typed field by its core tag: `version: !!str 1` is \
                the string `\"1\"`, which a number field refuses, and `version: !!int '1'` \
                is the number 1",
        oracle: "decoded the scalar's text into the field's type whatever its tag, so it \
                 read `!!str 1` and `!!float 1` as the number 1 and `!!str true` as `true`, \
                 and it refused an `!!int` tag on a quoted scalar",
        reason: "the tag says what the node is (YAML 1.2, section 10.3.2), which the value \
                 entry points already read alike; configurations do not tag their values. \
                 Documented in the 0.23 CHANGELOG",
        example: "version: !!str 1\n",
    },
    Deviation {
        id: "negative-zero-in-an-unsigned-field",
        classes: &[Class::OnlyOracleRejects],
        trigger: Trigger::Input {
            covers: typed,
            neutralize: neutralize::negative_zeros,
        },
        ayaml: "decodes a plain `-0` into an unsigned field as 0",
        oracle: "refuses it there as a negative integer: `invalid type: integer `0`, \
                 expected u64`; it reads `+0` and `0` as 0",
        reason: "`-0` is the integer 0 in the core schema (YAML 1.2, section 10.3.2), which \
                 the value entry points read alike",
        example: "version: 1\nfix_size_limit: -0\n",
    },
    Deviation {
        id: "rule-keys-that-are-not-strings",
        classes: &[Class::OnlyOracleRejects, Class::RejectedAtDifferentLocation],
        trigger: Trigger::Narrow {
            covers: |case| {
                case.entry == Entry::Rule && oracle_rejects(case, &["expected field identifier"])
            },
            because: "the oracle's message says it refused a mapping key as a field name, which \
                      it does as it gathers the rule's entries, before it reads any field; the \
                      rule is a case of its own",
        },
        ayaml: "decodes a rule whose mapping has a key that is not a string, as `1: y` or \
                `~: y`, into `RuleSpec`, with the key among its flattened options, or refuses \
                the rule for one of its fields",
        oracle: "refuses the key as a field name, as \"invalid type: integer `1`, expected \
                 field identifier\", before it reads a field",
        reason: "alint's loader rejects a rule with a key that is not a string before it \
                 decodes the rule (\"`rules[0]` has a non-string key\"), with either parser, \
                 so a configuration loads the same way",
        example: "version: 1\nrules:\n  - {id: a, kind: file_exists, level: error, paths: x, \
                  1: y}\n",
    },
    Deviation {
        id: "rule-that-is-null",
        classes: &[Class::RejectedAtDifferentLocation],
        trigger: Trigger::Narrow {
            covers: |case| {
                case.entry == Entry::Rule
                    && ayaml_rejects(
                        case,
                        &["invalid type: unit value, expected struct RuleSpec"],
                    )
                    && oracle_rejects(case, &["missing field `id`"])
            },
            because: "ayaml's message says it refused a null where the rule goes, and the \
                      oracle's that it missed the first field a rule needs, which is all it \
                      finds in the empty mapping it reads a null as",
        },
        ayaml: "refuses a rule that is null, as a `-` alone in `rules:`, when it decodes the \
                rule from its value, where a null is not an empty mapping: \"invalid type: \
                unit value, expected struct RuleSpec\"",
        oracle: "read the null as an empty mapping and refused it for the first field it \
                 lacks, \"missing field `id`\"",
        reason: "both refuse the rule, and so the configuration; only the message differs",
        example: "version: 1\nrules:\n  -\n",
    },
    Deviation {
        id: "global-tags",
        classes: ALL_DIFFERENCES,
        trigger: Trigger::Input {
            covers: any,
            neutralize: neutralize::global_tags,
        },
        ayaml: "keeps a global tag on its node: a verbatim tag such as `!<tag:x,2000:y>`, a \
                tag from a `%TAG` handle, or `!!` with a name outside the core schema, such as \
                `!!binary`, `!!timestamp`, `!!merge` or `!!value`; where tags are dropped, it \
                reads the node as if untagged",
        oracle: "drops the tag and reads a tagged scalar as a string",
        reason: "a global tag names an application type; neither reading is wrong, and \
                 alint's configurations and queries use local tags or none",
        example: "a: !<tag:x,2000:y> 1\n",
    },
    Deviation {
        id: "verbatim-non-specific-tag",
        classes: &[Class::OnlyAyamlRejects, Class::RejectedAtDifferentLocation],
        trigger: Trigger::Input {
            covers: ayaml_rejects_a_nameless_verbatim_tag,
            neutralize: neutralize::verbatim_non_specific_tags,
        },
        ayaml: "rejects `!<!>`, a verbatim tag that names no tag, before it reports \
                another error about the tag, as a character after it with no white space \
                between, or about the lines before",
        oracle: "reads it as the non-specific tag `!`",
        reason: "YAML 1.2 (example 6.25) gives `!<!>` as an invalid verbatim tag, which must \
                 be a local tag or a URI; documented in the 0.23 CHANGELOG",
        example: "a: !<!> x\n",
    },
    Deviation {
        id: "ayaml-error-without-a-place",
        classes: &[Class::RejectedAtDifferentLocation],
        trigger: Trigger::Narrow {
            covers: |case| {
                case.input.contains('\u{feff}')
                    && case.ayaml.as_ref().err().is_some_and(|rejection| {
                        rejection.location.is_none()
                            && rejection
                                .message
                                .contains("found character that cannot start any token")
                    })
            },
            because: "ayaml gives that message without a place only for a reserved indicator \
                      after a byte-order mark that starts a line (its compatibility mode's \
                      mapping of that error)",
        },
        ayaml: "reports a character that cannot start a token after a byte-order mark \
                without a line and column",
        oracle: "reports the same error with its place",
        reason: "an ayaml defect in a malformed document (asamarts/ayaml#133); the message \
                 still names the problem. Documented in the 0.23 CHANGELOG",
        example: "\u{feff}%YAML 1.2\n",
    },
    Deviation {
        id: "non-specific-tag",
        classes: &[Class::DifferentValue],
        trigger: Trigger::Input {
            covers: any,
            neutralize: neutralize::non_specific_tags,
        },
        ayaml: "reads a node tagged `!` as a string",
        oracle: "keeps the tag as `!!` and resolves the scalar as if untagged",
        reason: "YAML 1.2 (section 6.9.1) resolves a `!`-tagged scalar as a string",
        example: "a: ! 1\n",
    },
    Deviation {
        id: "comment-needs-white-space",
        classes: &[Class::OnlyAyamlRejects],
        trigger: Trigger::Narrow {
            covers: |case| {
                ayaml_rejects(
                    case,
                    &[
                        "a comment must be separated from the preceding token",
                        "a block scalar header comment requires preceding whitespace",
                    ],
                ) || (ayaml_rejects(case, &["invalid %YAML directive"])
                    && ayaml_points_at_a_yaml_directive_with_an_adjacent_number_sign(case))
            },
            because: "ayaml's messages name the comment, or point at a `%YAML` line with a `#` \
                      directly after a token",
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
        trigger: Trigger::Narrow {
            covers: |case| {
                ayaml_rejects(
                    case,
                    &[
                        "quoted scalar continuation must be indented more than its parent",
                        "tabs cannot be the only quoted-scalar indentation",
                    ],
                )
            },
            because: "ayaml's messages name the quoted scalar's continuation line",
        },
        ayaml: "rejects a quoted scalar's continuation line that is not indented past its \
                parent",
        oracle: "accepts it",
        reason: "YAML 1.2 (rule 125) requires the indentation",
        example: "a: 'x\ny'\n",
    },
    Deviation {
        id: "flow-line-tab-indentation",
        classes: &[Class::OnlyAyamlRejects, Class::RejectedAtDifferentLocation],
        trigger: Trigger::Input {
            covers: |case| {
                ayaml_rejects(
                    case,
                    &["tabs cannot be the only flow-collection indentation"],
                )
            },
            neutralize: neutralize::flow_line_tabs,
        },
        ayaml: "rejects a flow collection's continuation line under a block collection that \
                only tabs indent, as `\\t  b]` after `a: [x,`, before an error the line \
                comes after, since it reads the stream's tokens first",
        oracle: "accepts it, and reports the error before it",
        reason: "YAML 1.2 (`s-flow-line-prefix`, rule 69, and section 6.1) indents such a line \
                 with spaces; documented in the 0.23 CHANGELOG",
        example: "a: [x,\n\t  b]\n",
    },
    Deviation {
        id: "block-scalar-under-an-anchor",
        classes: &[Class::OnlyOracleRejects, Class::RejectedAtDifferentLocation],
        trigger: Trigger::Input {
            covers: oracle_points_at_a_block_scalar_under_an_anchor,
            neutralize: neutralize::block_scalar_headers,
        },
        ayaml: "accepts a block scalar header on the line after properties that include an \
                anchor, as `b: &x` then `>`, after properties alone on their line, as `!t` \
                then `>`, or after a key with an anchor and no value, as `&x b:` then `>`, \
                even when it is not indented past its parent",
        oracle: "rejects it, or reads it as a second document where it is the first line \
                 indented less than the document's root",
        reason: "ayaml defects in a malformed document (asamarts/ayaml#132, with the \
                 anchor on a key in a comment there, and #147 for properties alone on their \
                 line): with a tag on the line of its key or `-`, ayaml rejects the same \
                 layout, as YAML 1.2 (section 8.1) does",
        example: "a:\n  b: &x\n>\n",
    },
    Deviation {
        id: "block-scalar-indentation",
        classes: &[Class::OnlyAyamlRejects, Class::RejectedAtDifferentLocation],
        trigger: Trigger::Narrow {
            covers: |case| {
                ayaml_rejects(
                    case,
                    &[
                        "a block scalar is not indented past its parent",
                        "leading empty block-scalar lines are more indented than the content",
                    ],
                ) && !ayaml_takes_a_document_marker_for_block_scalar_content(case)
                    && (case.class != Class::RejectedAtDifferentLocation
                        || super::first_error(case).is_some_and(|(_, by_ayaml)| by_ayaml))
            },
            because: "ayaml's messages name the block scalar's indentation; where both \
                      reject, ayaml's error comes first, and the oracle read its line",
        },
        ayaml: "rejects a block scalar whose header or content is not indented past its \
                parent, such as `-` followed by `|2` at the same column, or whose leading \
                empty lines are more indented than its first line of content",
        oracle: "accepts it, and for leading empty lines takes the content's indentation \
                 from the most indented of them, which ends the scalar before its first \
                 line of content; it may then reject what follows, as a second document \
                 or at a later place",
        reason: "YAML 1.2 (section 8.1.1.1) requires the indentation; documented in the 0.23 \
                 CHANGELOG",
        example: "-\n|2\n  x\n",
    },
    Deviation {
        id: "document-marker-after-indented-empty-lines",
        classes: &[Class::OnlyAyamlRejects, Class::RejectedAtDifferentLocation],
        trigger: Trigger::Narrow {
            covers: ayaml_takes_a_document_marker_for_block_scalar_content,
            because: "ayaml's message is about the leading empty lines, and the first line \
                      after them with more than spaces is a document marker",
        },
        ayaml: "rejects a top-level block scalar whose only lines before a document marker \
                (`---` or `...` at column 1) are empty, one of them indented, as `|`, a line \
                of two spaces and `...`: it takes the marker for content indented less than \
                they are",
        oracle: "ends the scalar, empty, at the marker, and reads on, so that `---` and what \
                 follows it make more than one document",
        reason: "an ayaml defect in a valid document (asamarts/ayaml#146): a document marker ends \
                 any node (YAML 1.2, rule 206, `c-forbidden`); a scalar that is not top-level \
                 ends there in ayaml too",
        example: "|\n  \n...\n",
    },
];

/// The ayaml errors whose place may differ from the oracle's, and why.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "documentation, which the tests and the long run's report read"
    )
)]
pub(crate) struct Location {
    /// A short name, used in the report.
    pub(crate) id: &'static str,
    /// The error kinds it covers.
    pub(crate) kinds: &'static [ErrorKind],
    /// The messages it covers, as text the message contains; none means every
    /// message of those kinds.
    pub(crate) messages: &'static [&'static str],
    /// A check of both places, for a class that covers only some places of
    /// its messages.
    pub(crate) places: Option<fn(&Case<'_>) -> bool>,
    /// Where ayaml reports them.
    pub(crate) ayaml: &'static str,
    /// Where the oracle reports the error it finds instead.
    pub(crate) oracle: &'static str,
    /// An input it covers, and the entry point it is about.
    pub(crate) example: (&'static str, Entry),
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
        places: None,
        ayaml: "the first token that cannot continue the document once its root node is \
                complete",
        oracle: ORACLE,
        example: ("[]\n]", Entry::Value),
    },
    Location {
        id: "where-a-construct-opened",
        kinds: &[ErrorKind::Parser, ErrorKind::Scanner],
        messages: &[
            "unterminated",
            "did not find expected node content, while parsing a flow",
            "did not find expected ',' or",
            "flow collection entries must be indented",
            "quoted scalar continuation must be indented",
            "tabs cannot be the only quoted-scalar indentation",
            "document marker inside quoted scalar",
            "expected ',' or a closing bracket",
            "expected a flow item",
            "unexpected empty flow",
        ],
        places: None,
        ayaml: "where an unterminated quoted scalar, escape or verbatim tag opened, or the \
                token where a flow collection, which its message names, cannot go on",
        oracle: ORACLE,
        example: ("'", Entry::Value),
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
        places: None,
        ayaml: "the line whose indentation, checked against its parent's as it is read, \
                cannot continue the structure",
        oracle: ORACLE,
        example: ("?a\n- : ", Entry::Value),
    },
    Location {
        id: "at-the-token",
        kinds: &[ErrorKind::Parser, ErrorKind::Scanner],
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
            "a verbatim tag may only hold",
            "a verbatim tag must name",
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
        places: None,
        ayaml: "the start of the token that breaks the grammar",
        oracle: ORACLE,
        example: ("a: & x\n", Entry::Value),
    },
    Location {
        id: "verbatim-tag-that-names-nothing",
        kinds: &[ErrorKind::Scanner],
        messages: &[VERBATIM_NEEDS_A_NAME],
        places: Some(errors_in_an_empty_verbatim_tag),
        ayaml: "the start of a verbatim tag that names nothing, `!<>`",
        oracle: "its `>`, where it looks for the name",
        example: ("a: !<> x\n", Entry::Value),
    },
    Location {
        id: "non-printable-character",
        kinds: &[ErrorKind::Scanner],
        messages: &[
            "non-printable character",
            "a tag must be followed by whitespace or a line break",
            "a verbatim tag may only hold URI characters",
            "in block scalar header",
        ],
        places: Some(errors_about_one_character),
        ayaml: "the character YAML does not allow, or, where it ends a tag or stands in a \
                verbatim tag, its error about the tag, and where it follows a block scalar's \
                indicators, the header's, which names it",
        oracle: "line 1, column 1, with the character's byte offset in its message, since \
                 its reader checks each character before the scanner reads it",
        example: ("a: x\u{1}y\n", Entry::Value),
    },
    Location {
        id: "empty-node",
        kinds: &[
            ErrorKind::Serde,
            ErrorKind::Resolution,
            ErrorKind::Composition,
        ],
        messages: &[],
        places: Some(errors_around_an_empty_node),
        ayaml: "a value error about an empty node, such as `version:` with nothing after it, \
                at the start of what follows the node: the next line's content or the end of \
                the input",
        oracle: "the place right after the key or indicator, where the node would be",
        example: ("version: \n", Entry::Config),
    },
    Location {
        id: "values-after-syntax",
        kinds: &[
            ErrorKind::DuplicateKey,
            ErrorKind::Resolution,
            ErrorKind::Composition,
            ErrorKind::Serde,
            ErrorKind::Limit,
        ],
        messages: &[],
        places: None,
        ayaml: "the node a value error is about; ayaml checks the whole stream's syntax \
                before it builds values, so a later syntax error wins over an earlier \
                duplicate key, unknown anchor, recursive alias or type error",
        oracle: "the first error in the input, as it builds values while it reads, at its \
                 own position for a value error (often the start of the enclosing \
                 collection)",
        example: ("&a [*a]\n", Entry::Value),
    },
];

/// The location class that covers an ayaml error of `kind` with `message`
/// by its kind and message, which a class that also checks places narrows.
#[cfg(test)]
pub(crate) fn locate(kind: ErrorKind, message: &str) -> Option<&'static Location> {
    LOCATIONS
        .iter()
        .find(|location| covers_message(location, kind, message))
}

fn covers_message(location: &Location, kind: ErrorKind, message: &str) -> bool {
    location.kinds.contains(&kind)
        && (location.messages.is_empty()
            || location
                .messages
                .iter()
                .any(|covered| message.contains(covered)))
}

/// The first location class that covers `case`, whose ayaml error is of
/// `kind` with `message`. The oracle's error about a character it does not
/// allow is about that character, not the syntax: its reader decodes the
/// whole of a short input before its scanner reads a token. Only a class
/// that checks both places takes it, as `non-printable-character` does.
pub(crate) fn locate_case(
    case: &Case<'_>,
    kind: ErrorKind,
    message: &str,
) -> Option<&'static Location> {
    let reader = rejects(&case.oracle, ORACLE_READER_ERROR);
    LOCATIONS.iter().find(|location| {
        covers_message(location, kind, message)
            && location.places.map_or(!reader, |places| places(case))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
