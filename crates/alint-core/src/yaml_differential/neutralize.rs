//! Neutralizers: each removes one entry's trigger from an input, so the
//! harness can tell whether that trigger causes a difference (see
//! [`super::allowlist::Trigger::Input`]). Each returns the rewritten inputs
//! to try, in order, and none when the input holds no trigger. A rewrite
//! changes only the trigger, so a difference it leaves has another cause.

use std::collections::HashMap;
use std::ops::Range;

use super::predicates::{NAMELESS_VERBATIM, VERBATIM_NEEDS_A_NAME};

/// The core schema's tags, which ayaml resolves (YAML 1.2, section 10.3).
pub(super) const CORE_TAGS: &[&str] = &[
    "!!map", "!!seq", "!!str", "!!int", "!!float", "!!bool", "!!null",
];

/// Whether a property or an alias can start at byte `at` of `input`: at the
/// start, or after white space, a byte-order mark or a flow indicator that
/// opens or separates.
fn starts_token(input: &str, at: usize) -> bool {
    input[..at]
        .chars()
        .next_back()
        .is_none_or(|before| before.is_whitespace() || "[{,\u{feff}".contains(before))
}

/// Whether byte `at` of `input` is on a directive's line.
fn in_directive(input: &str, at: usize) -> bool {
    input[..at]
        .rfind('\n')
        .map_or(input, |newline| &input[newline + 1..])
        .starts_with('%')
}

/// The byte ranges of the tokens that start with one of `sigils` where a
/// property or an alias can: not in a directive, a quoted scalar, a comment,
/// a block scalar or a plain scalar's text, as [`places`] reads them, so that
/// `make && test` holds none. A token runs to white
/// space or a flow indicator; a verbatim tag (`!<...>`) runs to its `>`. An
/// anchor or alias name, as ayaml's compatibility mode reads it, also ends
/// before a `?` that a character other than a flow indicator follows, and,
/// outside a flow collection, before a `:` past its first character that a
/// character other than white space follows. A sigil inside a token, as
/// after a byte-order mark, starts none.
///
/// Each sigil looks back to the start of its line for a directive, so a line
/// of many sigils costs their number times its length: quadratic, which the
/// harness's inputs, a few kilobytes at most, keep cheap.
pub(super) fn tokens(input: &str, sigils: &[char]) -> Vec<Range<usize>> {
    let places = places(input);
    let mut end = 0;
    input
        .char_indices()
        .filter(|(at, ch)| {
            sigils.contains(ch)
                && !places[*at].literal
                && !places[*at].plain
                && starts_token(input, *at)
                && !in_directive(input, *at)
        })
        .filter_map(|(at, sigil)| {
            if at < end {
                return None;
            }
            let rest = &input[at + 1..];
            let len = if sigil == '!' && rest.starts_with('<') {
                rest.find('>').map_or(rest.len(), |close| close + 1)
            } else {
                let name = sigil != '!';
                rest.char_indices()
                    .find(|(offset, ch)| {
                        let after = rest[offset + ch.len_utf8()..].chars().next();
                        ch.is_whitespace()
                            || ",[]{}".contains(*ch)
                            || (name
                                && *ch == '?'
                                && after.is_some_and(|after| !",[]{}".contains(after)))
                            || (name
                                && *offset > 0
                                && *ch == ':'
                                && places[at + 1 + offset].depth == 0
                                && after.is_some_and(|after| !after.is_whitespace()))
                    })
                    .map_or(rest.len(), |(offset, _)| offset)
            };
            end = at + 1 + len;
            Some(at..end)
        })
        .collect()
}

/// Where a byte of an input stands, roughly as a scanner sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a fact of its own about the byte, which the rewrites read alone"
)]
pub(super) struct Place {
    /// The flow collections around it.
    pub(super) depth: u32,
    /// Whether it is inside a quoted scalar, a comment, a directive or a
    /// block scalar's header or content, rather than structure.
    pub(super) literal: bool,
    /// Whether it is inside a quoted scalar, past its opening quote.
    pub(super) quoted: bool,
    /// Whether it is inside a plain scalar, past its first character, which
    /// a property, an alias or a tab there is part of; white space at its
    /// end too, which [`places`] cannot tell apart from what follows.
    pub(super) plain: bool,
    /// Whether it is white space at the start of a block scalar's line, short
    /// of the content's indentation once the header's indentation indicator
    /// or a line set it: libyaml takes a tab there for indentation, and ayaml
    /// for content. Before then, libyaml rejects a tab, and ayaml makes a
    /// line with one the first line of content, with its spaces as the
    /// indentation, when they are past the parent's column, as they always
    /// are under a top-level header, and otherwise ends the scalar before the
    /// line and reads it as white space (YAML 1.2, section 8.1.1.1). A line
    /// of tabs alone, from its start, it rejects, as libyaml does, but under
    /// a top-level header.
    pub(super) indentation: bool,
}

/// What a line outside a flow collection is at, as [`places`] reads it.
#[derive(PartialEq)]
enum Block {
    /// Where a node can start: after indentation, an indicator, a property or
    /// a document marker.
    Start,
    /// Inside a plain scalar, until `: ` makes it a key.
    Scalar,
    /// After a quoted scalar, an alias or a flow collection, until `: `.
    After,
    /// Inside an anchor or a tag.
    Property,
}

/// What a flow collection is at, as [`places`] reads it.
#[derive(PartialEq)]
enum Flow {
    /// Where a node can start: after `[`, `{`, `,`, `?`, a `:` that is a
    /// value indicator, or a property.
    Start,
    /// Inside a plain scalar, which may span lines.
    Plain,
    /// Inside an anchor, a tag or an alias.
    Property,
    /// After a quoted scalar or a flow collection, where a `:` is a value
    /// indicator even before a non-space, as in `{"a":1}`.
    After,
}

/// The [`Place`] of each byte of `input`, and of its end. Outside a flow
/// collection, `[` and `{` open one, and a quote starts a quoted scalar,
/// only where a node starts, so `a [b` is a plain scalar; inside one, a
/// quote starts a scalar only where a node starts too, so `[a 'b]` holds a
/// plain scalar, and so does `[:'b]`, as YAML 1.2 reads it. A block scalar's
/// content, and a plain scalar's next line, are the lines indented past the
/// column of the key or indicator the scalar belongs to.
#[allow(clippy::too_many_lines)]
pub(super) fn places(input: &str) -> Vec<Place> {
    let mut places = Vec::with_capacity(input.len() + 1);
    let mut depth = 0_u32;
    let mut flow = Flow::Start;
    let mut quote: Option<char> = None;
    // The column a block scalar's content is indented past, while it lasts,
    // and its content's indentation, once its header's indentation indicator
    // or a line with content set it.
    let mut header: Option<usize> = None;
    let mut content: Option<usize> = None;
    // Whether that block scalar is the document's root node, whose content
    // may start at column 1.
    let mut root_scalar = false;
    // The column of the key or indicator of a plain scalar that ran to the
    // end of its line, and whether the scalar is the document's root node,
    // whose lines continue it at any indentation.
    let mut continued: Option<usize> = None;
    let mut continued_root = false;
    // Whether the document so far holds a key, an indicator or a flow
    // collection, so that a plain scalar starting now is not its root.
    let mut nested = false;
    // The column of the key or indicator that ended the last line, whose
    // node starts on a later one.
    let mut open: Option<usize> = None;
    for line in input.split_inclusive(['\n', '\r']) {
        let text = line.trim_end_matches(['\n', '\r']);
        let indent = text.len() - text.trim_start_matches(' ').len();
        let blank = text.trim().is_empty();
        let tabs_alone = !text.is_empty() && text.chars().all(|ch| ch == '\t');
        if let Some(parent) = header {
            // A line with content indented less than the content's ends the
            // scalar (YAML 1.2, rule 170), even past the parent.
            if blank || (indent > parent && content.is_none_or(|content| indent >= content)) {
                let mut leading = true;
                for (column, ch) in line.chars().enumerate() {
                    leading &= matches!(ch, ' ' | '\t');
                    let indentation = leading
                        && content.map_or(root_scalar || !tabs_alone, |content| column < content);
                    places.extend(std::iter::repeat_n(
                        Place {
                            depth,
                            literal: true,
                            quoted: false,
                            plain: false,
                            indentation,
                        },
                        ch.len_utf8(),
                    ));
                }
                if !blank {
                    content.get_or_insert(indent);
                }
                continue;
            }
            header = None;
        }
        let block = depth == 0 && quote.is_none();
        let marker = (text.starts_with("---") || text.starts_with("..."))
            && text[3..].chars().next().is_none_or(char::is_whitespace);
        let comment_line = text.trim_start().starts_with('#');
        let continuation = continued.filter(|owner| {
            block && !blank && (indent > *owner || continued_root) && !comment_line && !marker
        });
        if marker {
            nested = false;
        }
        // A `%` at the start of a line is a directive, but on a line that
        // continues the root's plain scalar, where both scanners read it as
        // the scalar's text: `1\n%\n" #` is the scalar `1 % "`.
        let mut comment = block && text.starts_with('%') && continuation.is_none();
        let mut state = if continuation.is_some() {
            Block::Scalar
        } else if quote.is_some() {
            Block::After
        } else {
            Block::Start
        };
        // The column of the line's last key or indicator, and of the node
        // that started last.
        let mut owner = continuation
            .or(open.filter(|owner| indent > *owner))
            .unwrap_or(indent);
        let mut node: Option<usize> = None;
        let mut escaped = false;
        let mut before = ' ';
        for (column, (offset, ch)) in line.char_indices().enumerate() {
            let after = line[offset + ch.len_utf8()..].chars().next();
            let blank_after = after.is_none_or(char::is_whitespace);
            let literal = comment || quote.is_some() || (block && marker && offset < 3);
            let plain = !literal
                && if depth > 0 {
                    flow == Flow::Plain
                } else {
                    state == Block::Scalar
                };
            places.extend(std::iter::repeat_n(
                Place {
                    depth,
                    literal,
                    quoted: quote.is_some(),
                    plain,
                    indentation: false,
                },
                ch.len_utf8(),
            ));
            if literal {
                if let Some(open) = quote {
                    if escaped {
                        escaped = false;
                    } else if (open == '"' && ch == '\\')
                        || (ch == open && open == '\'' && after == Some('\''))
                    {
                        escaped = true;
                    } else if ch == open {
                        quote = None;
                        state = Block::After;
                        flow = Flow::After;
                    }
                }
                before = ch;
                continue;
            }
            if depth > 0 {
                let indicator =
                    after.is_none_or(|next| next.is_whitespace() || ",[]{}".contains(next));
                match ch {
                    '#' if before.is_whitespace() => comment = true,
                    '[' | '{' => {
                        depth += 1;
                        flow = Flow::Start;
                    }
                    ']' | '}' => {
                        depth -= 1;
                        state = Block::After;
                        flow = Flow::After;
                    }
                    ',' => flow = Flow::Start,
                    ':' if indicator || flow == Flow::After => flow = Flow::Start,
                    _ if ch.is_whitespace() => {
                        if flow == Flow::Property {
                            flow = Flow::Start;
                        }
                    }
                    _ if flow != Flow::Start => {}
                    '?' if indicator => {}
                    '\'' | '"' => quote = Some(ch),
                    '&' | '!' | '*' => flow = Flow::Property,
                    _ => flow = Flow::Plain,
                }
            } else {
                match state {
                    Block::Start => match ch {
                        '-' | '?' | ':' if blank_after => {
                            owner = column;
                            node = None;
                            nested = true;
                        }
                        '#' => comment = true,
                        '|' | '>' => {
                            header = Some(owner);
                            // An indentation indicator sets the content's
                            // indentation past the parent's column, which
                            // both parsers take as 0 for the root (YAML 1.2,
                            // section 8.1.1.1): `a: |2` keeps `   x` and then
                            // `  - |` as content, with no second header.
                            content = line[offset + 1..]
                                .chars()
                                .take_while(|next| matches!(next, '+' | '-' | '1'..='9'))
                                .find_map(|next| next.to_digit(10))
                                .map(|digit| owner + digit as usize);
                            root_scalar = !nested;
                            comment = true;
                        }
                        // Both scanners skip a byte-order mark at a line's
                        // start only; elsewhere it starts a plain scalar.
                        _ if ch.is_whitespace() || (ch == '\u{feff}' && column == 0) => {}
                        _ => {
                            node.get_or_insert(column);
                            state = match ch {
                                '&' | '!' => Block::Property,
                                '[' | '{' => {
                                    depth = 1;
                                    flow = Flow::Start;
                                    nested = true;
                                    Block::After
                                }
                                '\'' | '"' => {
                                    quote = Some(ch);
                                    Block::After
                                }
                                // An alias, and what both scanners take
                                // for tokens of their own, or reject,
                                // anywhere: no plain scalar starts with one
                                // (YAML 1.2, rule 126).
                                '*' | ',' | ']' | '}' | '@' | '`' | '%' => Block::After,
                                _ => Block::Scalar,
                            };
                        }
                    },
                    Block::Property if ch.is_whitespace() => state = Block::Start,
                    Block::Property => {}
                    Block::Scalar | Block::After => match ch {
                        ':' if blank_after => {
                            owner = node.take().unwrap_or(owner);
                            state = Block::Start;
                            nested = true;
                        }
                        '#' if before.is_whitespace() => comment = true,
                        // After a node, the scanners read a flow collection
                        // or a quoted scalar that follows on its line as
                        // one, which the parsers then reject.
                        '[' | '{' if state == Block::After => {
                            depth = 1;
                            flow = Flow::Start;
                            nested = true;
                        }
                        '\'' | '"' if state == Block::After => quote = Some(ch),
                        _ => {}
                    },
                }
            }
            before = ch;
        }
        // A comment line ends a plain scalar, and leaves the node that
        // started on a later line to its owner before it.
        if comment_line && !marker {
            continued = None;
        } else if !blank {
            let block = depth == 0 && quote.is_none();
            continued = (block && !comment && state == Block::Scalar).then_some(owner);
            continued_root = continued.is_some() && !nested;
            open = (block && state == Block::Start).then_some(owner);
        }
    }
    places.push(Place {
        depth,
        literal: quote.is_some(),
        quoted: quote.is_some(),
        plain: false,
        indentation: false,
    });
    places
}

/// `input` with each range replaced by its text, or none when there is no
/// edit. The ranges are in order; one that overlaps the edit before it is
/// left out.
fn apply(input: &str, edits: &[(Range<usize>, String)]) -> Vec<String> {
    if edits.is_empty() {
        return Vec::new();
    }
    let mut out = String::with_capacity(input.len());
    let mut from = 0;
    for (range, text) in edits {
        if range.start < from {
            continue;
        }
        out.push_str(&input[from..range.start]);
        out.push_str(text);
        from = range.end;
    }
    out.push_str(&input[from..]);
    vec![out]
}

/// What deleting the token at `range` of `input` removes. A token that comes
/// first on its line, after indentation and indicators, goes with the white
/// space after it, so that the node it was on keeps the token's column:
/// `!t   c: 2` becomes `c: 2`. Not where that would start the line, or the
/// stream, with what both parsers read there as something else (see
/// [`starts_anew`]).
fn deletion(input: &str, range: Range<usize>) -> Range<usize> {
    let line = input[..range.start]
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    let first = input[line..range.start]
        .chars()
        .all(|ch| " \t-?:\u{feff}".contains(ch));
    let rest = &input[range.end..];
    let gap = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    if first && !starts_anew(input, range.start, &rest[gap..]) {
        range.start..range.end + gap
    } else {
        range
    }
}

/// Whether `after`, put at byte `at` of `input` in place of what was there,
/// would be read anew: a byte-order mark at the start of the stream, which
/// both parsers skip, or a directive or a document marker at the start of a
/// line, as `!!int \u{feff}%TAG` or `!t ---` would become.
fn starts_anew(input: &str, at: usize, after: &str) -> bool {
    let before = &input[..at];
    let line_start =
        before.trim_start_matches('\u{feff}').is_empty() || before.ends_with(['\n', '\r']);
    let marker = |text: &str| {
        after
            .strip_prefix(text)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
    };
    (at == 0 && after.starts_with('\u{feff}'))
        || (line_start && (after.starts_with('%') || marker("---") || marker("...")))
}

/// `input` without the tokens of `ranges`: as [`deletion`] removes them, and
/// then without the white space after them, which keeps a `---` or `...`
/// after a token at the start of a line from becoming a document marker.
fn delete(input: &str, ranges: impl IntoIterator<Item = Range<usize>>) -> Vec<String> {
    let ranges: Vec<Range<usize>> = ranges.into_iter().collect();
    let edits = |keep_columns: bool| -> Vec<(Range<usize>, String)> {
        ranges
            .iter()
            .map(|range| {
                let removed = if keep_columns {
                    deletion(input, range.clone())
                } else {
                    range.clone()
                };
                (removed, String::new())
            })
            .collect()
    };
    let mut rewrites = apply(input, &edits(true));
    for rewrite in apply(input, &edits(false)) {
        if !rewrites.contains(&rewrite) {
            rewrites.push(rewrite);
        }
    }
    rewrites
}

/// Without NEL, LS and PS; then with each read as the oracle reads it, as a
/// line break, with the next line at the break's column, as a scalar's
/// continuation line must be indented, and at column 1; then as ayaml reads
/// it, as a content character, a different one for each (`x`, `y`, `z`), so
/// that two keys stay apart. A tab before the character, which stays, then
/// ends its line in the variants with a break: both parsers read it there as
/// they read it before any other line break, as trailing white space or a
/// block scalar's content, and only removing the tab, outside the construct,
/// would avoid it.
pub(super) fn line_breaks(input: &str) -> Vec<String> {
    const BREAKS: [char; 3] = ['\u{85}', '\u{2028}', '\u{2029}'];
    if !input.contains(BREAKS) {
        return Vec::new();
    }
    let mut indented = String::with_capacity(input.len());
    let mut column = 0;
    for ch in input.chars() {
        if BREAKS.contains(&ch) {
            indented.push('\n');
            indented.extend(std::iter::repeat_n(' ', column));
        } else if matches!(ch, '\n' | '\r') {
            indented.push(ch);
            column = 0;
        } else {
            indented.push(ch);
            column += 1;
        }
    }
    let content = input
        .replace('\u{85}', "x")
        .replace('\u{2028}', "y")
        .replace('\u{2029}', "z");
    vec![
        input.replace(BREAKS, ""),
        indented,
        input.replace(BREAKS, "\n"),
        content,
    ]
}

/// Without each byte-order mark that starts a line; then with each as a
/// space, the column the oracle counts it as; then as ayaml reads them,
/// without the one that starts the stream and with the others as spaces;
/// then, where more marks follow the first, as both read those (see
/// [`second_leading_marks_as_content`]).
pub(super) fn line_start_byte_order_marks(input: &str) -> Vec<String> {
    let rewrite = |first: &str, with: &str| {
        let mut out = String::with_capacity(input.len());
        let mut line_start = true;
        for (at, ch) in input.char_indices() {
            if ch == '\u{feff}' && line_start {
                out.push_str(if at == 0 { first } else { with });
                continue;
            }
            line_start = matches!(ch, '\n' | '\r');
            out.push(ch);
        }
        out
    };
    let without = rewrite("", "");
    if without == input {
        return Vec::new();
    }
    let mut rewrites = vec![without];
    for rewrite in [
        rewrite(" ", " "),
        rewrite("", " "),
        second_leading_marks_as_content(input),
    ] {
        if !rewrites.contains(&rewrite) && rewrite != input {
            rewrites.push(rewrite);
        }
    }
    rewrites
}

/// Without the mark that starts the stream, and with each mark right after
/// it as `x`. Both parsers read such a mark as content (asamarts/ayaml#140),
/// so `x` keeps what follows it a scalar's text, where without the first
/// mark the second would start the stream and be skipped.
fn second_leading_marks_as_content(input: &str) -> String {
    let Some(rest) = input.strip_prefix('\u{feff}') else {
        return input.to_string();
    };
    let marks = rest.len() - rest.trim_start_matches('\u{feff}').len();
    let count = marks / '\u{feff}'.len_utf8();
    if count == 0 {
        return input.to_string();
    }
    format!("{}{}", "x".repeat(count), &rest[marks..])
}

/// Whether the tab at byte `at` of `input` is a scalar's content, as
/// [`places`] reads it: in a quoted scalar, a comment, a directive, or a
/// block scalar past its content's indentation, or in a plain scalar between
/// its text on the line and more of it.
pub(super) fn tab_in_content(input: &str, places: &[Place], at: usize) -> bool {
    if places[at].literal {
        return !places[at].indentation;
    }
    let start = input[..at]
        .rfind(['\n', '\r'])
        .map_or(0, |offset| offset + 1);
    let end = input[at..]
        .find(['\n', '\r'])
        .map_or(input.len(), |offset| at + offset);
    let white = |ch: char| matches!(ch, ' ' | '\t');
    let before = input[start..at].trim_end_matches(white);
    let after = input[at..end].trim_start_matches(white);
    let flow = places[at].depth > 0;
    let ends = |text: &str| {
        text.chars()
            .next()
            .is_none_or(|next| next.is_whitespace() || (flow && ",[]{}".contains(next)))
    };
    places[at].plain
        && !before.trim_start_matches(white).is_empty()
        && !after.is_empty()
        && !after.starts_with('#')
        && (!flow || !after.starts_with([',', ']', '}']))
        && !(after.starts_with(':') && ends(&after[1..]))
}

/// With each tab that is white space as a space, wherever it is; then with
/// the tabs that only spaces come before on their line left out, and then as
/// content (`x`), where the first line of a block scalar can hold them; the
/// other tabs as spaces; then with those tabs left out with the white space
/// after them (see [`leading_out`]), which keeps the indentation ayaml reads
/// a line with. A tab in a scalar's content
/// stays (see
/// [`tab_in_content`]): both parsers read it alike, and a rewrite that
/// changed it could hide a difference there. So does a tab that starts a
/// line in a flow collection, [`flow_line_tabs`]' construct, but on a line
/// ayaml exempts (see [`exempt_flow_line`]), and one ayaml rejects (see
/// [`tabs_ayaml_rejects`]), which it does not read as white space, but in
/// a last variant, where it does once the others are spaces.
pub(super) fn tabs(input: &str) -> Vec<String> {
    if !input.contains('\t') {
        return Vec::new();
    }
    let places = places(input);
    let mut rejected = tabs_ayaml_rejects(input);
    let flow_lines = flow_line_tab_offsets(input, &places, &rejected);
    let white_among = |rejected: &[(usize, bool)]| -> Vec<usize> {
        input
            .match_indices('\t')
            .map(|(at, _)| at)
            .filter(|at| {
                !tab_in_content(input, &places, *at)
                    && (!flow_lines.contains(at) || exempt_flow_line(input, *at))
                    && !rejected.iter().any(|(tab, _)| tab == at)
            })
            .collect()
    };
    let white = white_among(&rejected);
    if white.is_empty() {
        return Vec::new();
    }
    // A tab ayaml rejects where a tab before it, as white space, left it,
    // as a line of a tab that ended a block scalar leaves the next one at
    // the leading indentation of a block scalar ayaml then opens, is white
    // space once the tabs before it are spaces: a last variant has those
    // as spaces too.
    let mut freed = white.clone();
    while !rejected.is_empty() {
        rejected = tabs_ayaml_rejects(&spaced(input, &freed, 1));
        let more = white_among(&rejected);
        if more.len() == freed.len() {
            break;
        }
        freed = more;
    }
    let leading = |white: &[usize], with: &str| {
        let mut out = String::with_capacity(input.len());
        let mut line_start = true;
        for (at, ch) in input.char_indices() {
            match ch {
                '\t' if !white.contains(&at) => out.push(ch),
                '\t' if line_start => out.push_str(with),
                '\t' => out.push(' '),
                _ => out.push(ch),
            }
            line_start = matches!(ch, '\n' | '\r') || (line_start && matches!(ch, ' ' | '\t'));
        }
        out
    };
    let mut rewrites = vec![leading(&white, " ")];
    for rewrite in [leading(&white, ""), leading(&white, "x")]
        .into_iter()
        .chain(leading_out(input, &white))
        .chain(Some(spaced(input, &freed, 1)))
    {
        if !rewrites.contains(&rewrite) {
            rewrites.push(rewrite);
        }
    }
    rewrites
}

/// Whether the tab at byte `at` of `input` starts a flow collection's line
/// whose white space runs to a closing `]` or `}`, a comment, or the line's
/// end: ayaml exempts such a line from the indentation it holds a flow
/// line to (its 0.1.2 CHANGELOG), so the tab is white space there, which
/// the oracle rejects after a plain scalar's line, as in `a: [x` then
/// `\t]` (`tab-as-white-space`).
fn exempt_flow_line(input: &str, at: usize) -> bool {
    input[at..]
        .trim_start_matches([' ', '\t'])
        .chars()
        .next()
        .is_none_or(|ch| matches!(ch, ']' | '}' | '#' | '\n' | '\r'))
}

/// `input` with each tab at the byte offsets `tabs` as `width` spaces.
fn spaced(input: &str, tabs: &[usize], width: usize) -> String {
    let mut out = String::with_capacity(input.len() + tabs.len() * width);
    for (at, ch) in input.char_indices() {
        if tabs.contains(&at) {
            out.extend(std::iter::repeat_n(' ', width));
        } else {
            out.push(ch);
        }
    }
    out
}

/// `input` with each of the tabs at the byte offsets `white`, which are
/// white space, that only spaces come before on its line left out, with the
/// white space after it up to the line's content, and the others as spaces:
/// ayaml reads the line's indentation from the spaces before the first tab,
/// and a space for it would add to that. None where the content would then
/// read anew (see [`starts_anew`]).
fn leading_out(input: &str, white: &[usize]) -> Option<String> {
    let mut edits = Vec::new();
    for &at in white {
        let start = input[..at]
            .rfind(['\n', '\r'])
            .map_or(0, |offset| offset + 1);
        if !input[start..at].chars().all(|ch| ch == ' ') {
            edits.push((at..at + 1, " ".to_string()));
            continue;
        }
        let content = input[at..]
            .char_indices()
            .find(|&(offset, ch)| ch != ' ' && !(ch == '\t' && white.contains(&(at + offset))))
            .map_or(input.len(), |(offset, _)| at + offset);
        if starts_anew(input, at, &input[content..]) {
            return None;
        }
        edits.push((at..content, String::new()));
    }
    // A tab in the white space an edit leaves out overlaps it, and goes.
    apply(input, &edits).pop()
}

/// ayaml's error about a tab that alone indents a flow collection's line.
const FLOW_LINE_TAB: &str = "tabs cannot be the only flow-collection indentation";

/// The byte offsets of the tabs ayaml rejects, each with whether ayaml
/// rejects it as a flow line's only indentation: each found by reading the
/// input again once the one it reported before is a space, until it reports
/// an error elsewhere or none. Its scanner reports them before any parser
/// error, in their order, and stops at the first error that is not about a
/// tab.
fn tabs_ayaml_rejects(input: &str) -> Vec<(usize, bool)> {
    let mut tabs = Vec::new();
    let mut spaced = input.to_string();
    for _ in 0..input.matches('\t').count() {
        let (side, _) = super::guarded(|| super::run_ayaml(&spaced, super::Entry::Value, 0));
        let Some(at) = super::predicates::error_offset(&spaced, &side)
            .filter(|&at| side.is_err() && spaced[at..].starts_with('\t'))
        else {
            break;
        };
        let flow = side
            .as_ref()
            .is_err_and(|rejection| rejection.message.contains(FLOW_LINE_TAB));
        tabs.push((at, flow));
        spaced.replace_range(at..=at, " ");
    }
    tabs
}

/// The byte offsets of the tabs that start a line in a flow collection,
/// outside a quoted scalar, as [`places`] reads the input or as ayaml does
/// (see [`tabs_ayaml_rejects`]). In a malformed document ayaml's scanner may
/// open a flow collection where YAML 1.2 continues a plain scalar, and the
/// tab is its own error there.
fn flow_line_tab_offsets(input: &str, places: &[Place], rejected: &[(usize, bool)]) -> Vec<usize> {
    let mut tabs: Vec<usize> = input
        .match_indices('\t')
        .map(|(at, _)| at)
        .filter(|&at| {
            ((at == 0 || input[..at].ends_with(['\n', '\r']))
                && places[at].depth > 0
                && !places[at].literal)
                || rejected.contains(&(at, true))
        })
        .collect();
    tabs.dedup();
    tabs
}

/// With spaces for each tab that starts a line in a flow collection (see
/// [`flow_line_tab_offsets`]): one, and then one more than the widest
/// indentation of the input, past that of any block collection the flow
/// collection is in; then with one space for only those ayaml rejects, as
/// a flow line's only indentation, since the oracle may reject one of the
/// others, in a plain scalar's line, after the error ayaml's hides. The
/// oracle reads the tab as white space, and ayaml rejects it under a block
/// collection as the line's only indentation (`flow-line-tab-indentation`).
/// No variant moves the line's content to its start, where it could read as
/// a marker.
pub(super) fn flow_line_tabs(input: &str) -> Vec<String> {
    if !input.contains('\t') {
        return Vec::new();
    }
    let rejected = tabs_ayaml_rejects(input);
    let tabs = flow_line_tab_offsets(input, &places(input), &rejected);
    if tabs.is_empty() {
        return Vec::new();
    }
    let only_rejected: Vec<usize> = rejected
        .iter()
        .filter(|(_, flow)| *flow)
        .map(|(at, _)| *at)
        .collect();
    let widest = input
        .split(['\n', '\r'])
        .map(|line| line.len() - line.trim_start_matches(' ').len())
        .max()
        .unwrap_or(0);
    [
        spaced(input, &tabs, 1),
        spaced(input, &tabs, widest + 1),
        spaced(input, &only_rejected, 1),
    ]
    .into_iter()
    .fold(Vec::new(), |mut rewrites, rewrite| {
        if !rewrites.contains(&rewrite) && rewrite != input {
            rewrites.push(rewrite);
        }
        rewrites
    })
}

/// With each block scalar header that follows a line with an anchor or
/// properties alone, at no greater indentation (see
/// [`super::predicates::block_scalar_headers_under_an_anchor`]), indented
/// past that line's last character, where YAML 1.2 (section 8.1) and the
/// oracle read it as the node's, as ayaml reads it where it is
/// (`block-scalar-under-an-anchor`). Its content's indentation counts from
/// the node's parent, not the header, so the lines after it read alike.
pub(super) fn block_scalar_headers(input: &str) -> Vec<String> {
    let counter = super::predicates::Counter::Oracle;
    let (starts, lines) = (counter.line_starts(input), counter.lines(input));
    let edits: Vec<(Range<usize>, String)> =
        super::predicates::block_scalar_headers_under_an_anchor(input)
            .into_iter()
            .map(|line| {
                let width = lines[line - 2].trim_end().chars().count();
                let at = starts[line - 1];
                (at..at, " ".repeat(width + 1))
            })
            .collect();
    apply(input, &edits)
}

/// Whether `text` starts with `indicator` followed by white space or nothing.
fn starts_indicator(text: &str, indicator: char) -> bool {
    text.strip_prefix(indicator)
        .is_some_and(|after| after.is_empty() || after.starts_with([' ', '\t', '\r', '\n']))
}

/// With `~` as the key of each line's entry with an empty key, as `~: v` for
/// `: v`. A `:` in the column of an earlier `?` is that explicit key's value,
/// and stays; so do the lines that start inside a flow collection, a quoted
/// scalar or a block scalar.
///
/// Each indicator on a line looks through the open `?` columns, so a line of
/// many `? ` costs their number squared, which the harness's inputs keep
/// cheap.
pub(super) fn empty_keys(input: &str) -> Vec<String> {
    let places = places(input);
    let mut changed = false;
    // The columns of the `?` indicators whose `:` has not come yet.
    let mut explicit: Vec<usize> = Vec::new();
    let mut out = String::with_capacity(input.len() + 8);
    let mut start = 0;
    for line in input.split_inclusive(['\n', '\r']) {
        let place = places[start];
        start += line.len();
        let blank = |at: usize| line[at..].len() - line[at..].trim_start_matches([' ', '\t']).len();
        let mut at = blank(0);
        let text = line[at..].trim_end();
        if place.depth > 0 || place.literal || text.is_empty() || text.starts_with('#') {
            out.push_str(line);
            continue;
        }
        // `- `, `? ` and an explicit key's `: ` start a compact node in a
        // further column, which may be an entry with an empty key.
        let empty_key = loop {
            let rest = &line[at..];
            let value = starts_indicator(rest, ':') && explicit.contains(&at);
            explicit.retain(|column| *column < at);
            if starts_indicator(rest, '?') {
                explicit.push(at);
            } else if !value && !starts_indicator(rest, '-') {
                break starts_indicator(rest, ':');
            }
            at += 1;
            at += blank(at);
        };
        if empty_key {
            changed = true;
            out.push_str(&line[..at]);
            out.push('~');
            out.push_str(&line[at..]);
        } else {
            out.push_str(line);
        }
    }
    if changed { vec![out] } else { Vec::new() }
}

/// Without the `...` lines that come before any content, each one of them.
pub(super) fn leading_document_end(input: &str) -> Vec<String> {
    let mut out = String::with_capacity(input.len());
    let mut changed = false;
    let mut offset = 0;
    for line in input.split_inclusive(['\n', '\r']) {
        let text = line.trim();
        let marker = line.strip_prefix("...").is_some_and(|after| {
            after.is_empty() || after.starts_with([' ', '\t', '\r', '\n', '#'])
        });
        if marker {
            changed = true;
        } else if text.is_empty() || text.starts_with('#') {
            out.push_str(line);
        } else {
            break;
        }
        offset += line.len();
    }
    if !changed {
        return Vec::new();
    }
    out.push_str(&input[offset..]);
    vec![out]
}

fn narrow_name_character(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'
}

/// `name` with each character libyaml does not allow in a name as `_`.
fn narrowed(name: &str) -> String {
    name.chars()
        .map(|ch| if narrow_name_character(ch) { ch } else { '_' })
        .collect()
}

/// With a space after each anchor or alias name that, outside a flow
/// collection, ends at a `:` that a flow indicator follows, as in `&a:[`:
/// there ayaml's scanner ends the name and its line reader does not
/// (asamarts/ayaml#139). Both parsers read the `:` and what follows as
/// before, and ayaml's readers agree.
pub(super) fn names_ended_at_a_colon_before_a_flow_indicator(input: &str) -> Vec<String> {
    let places = places(input);
    let edits: Vec<(Range<usize>, String)> = tokens(input, &['&', '*'])
        .into_iter()
        .filter(|range| {
            let rest = &input[range.end..];
            places[range.start].depth == 0
                && rest.starts_with(':')
                && rest[1..].starts_with(|ch: char| ",[]{}".contains(ch))
        })
        .map(|range| (range.end..range.end, " ".to_string()))
        .collect();
    apply(input, &edits)
}

/// With each anchor or alias name that holds a character other than an
/// ASCII letter, a digit, `-` or `_` renamed to one that does not, the
/// aliases to it alike. An alias whose name is a known anchor's followed by
/// `:` keeps the `:`, which both parsers then read as an indicator.
pub(super) fn wide_anchor_names(input: &str) -> Vec<String> {
    let mut anchors: HashMap<&str, String> = HashMap::new();
    let mut edits = Vec::new();
    // The names renamed, without a `:` that ends an alias.
    let mut wide = Vec::new();
    for range in tokens(input, &['&', '*']) {
        let name_range = range.start + 1..range.end;
        let name = &input[name_range.clone()];
        let edit = if input[range.start..].starts_with('&') {
            let renamed = narrowed(name);
            anchors.insert(name, renamed.clone());
            (renamed != name).then_some((name_range, renamed))
        } else if let Some(renamed) = anchors.get(name) {
            (renamed != name).then(|| (name_range, renamed.clone()))
        } else if let Some(stem) = name.strip_suffix(':') {
            // The `:` that ends an alias is the value indicator after it,
            // which stays, as for `*b<<: x`, whose name no anchor gave.
            let renamed = anchors.get(stem).cloned().unwrap_or_else(|| narrowed(stem));
            (renamed != stem).then(|| (name_range.start..name_range.end - 1, renamed))
        } else {
            (narrowed(name) != name).then(|| (name_range, narrowed(name)))
        };
        if let Some((name_range, renamed)) = edit {
            wide.push(name_range.clone());
            edits.push((name_range, renamed));
        }
    }
    let mut rewrites = apply(input, &edits);
    // Then each such name ended where the oracle ends it, with a space, as
    // `&a !t x` for `&a!t x`: what follows is the oracle's next token. A `:`
    // before text there gets a space after it too, which makes it a value
    // indicator for both, as `[*a : b]` for `[*a:b]`.
    let split: Vec<(Range<usize>, String)> = wide
        .into_iter()
        .filter_map(|range| {
            let name = &input[range.clone()];
            let (at, ch) = name
                .char_indices()
                .find(|(_, ch)| !narrow_name_character(*ch))?;
            Some(if at == 0 {
                (range, narrowed(name))
            } else if ch == ':' && !input[range.start + at + 1..].starts_with(char::is_whitespace) {
                (range.start + at..range.start + at + 1, " : ".to_string())
            } else {
                (range.start + at..range.start + at, " ".to_string())
            })
        })
        .collect();
    for rewrite in apply(input, &split) {
        if !rewrites.contains(&rewrite) {
            rewrites.push(rewrite);
        }
    }
    rewrites
}

/// With each anchor that redefines a name renamed, and each alias renamed to
/// the latest anchor before it, which is the node YAML 1.2 says it refers
/// to; then with the aliases left to the first anchor of their name.
pub(super) fn redefined_anchors(input: &str) -> Vec<String> {
    let mut rewrites = Vec::new();
    for follow in [true, false] {
        let mut current: HashMap<&str, String> = HashMap::new();
        let mut definitions: HashMap<&str, usize> = HashMap::new();
        let mut edits = Vec::new();
        for range in tokens(input, &['&', '*']) {
            let name_range = range.start + 1..range.end;
            let name = &input[name_range.clone()];
            if input[range.start..].starts_with('&') {
                let count = definitions.entry(name).or_default();
                *count += 1;
                let renamed = if *count == 1 {
                    name.to_string()
                } else {
                    format!("{name}-{count}r")
                };
                if renamed != name {
                    edits.push((name_range, renamed.clone()));
                }
                if follow || *count == 1 {
                    current.insert(name, renamed);
                }
            } else if let Some(renamed) = current.get(name) {
                if renamed != name {
                    edits.push((name_range, renamed.clone()));
                }
            } else if let Some(stem) = name.strip_suffix(':')
                && let Some(renamed) = current.get(stem)
                && renamed != stem
            {
                edits.push((name_range.start..name_range.end - 1, renamed.clone()));
            }
        }
        if definitions.values().all(|count| *count < 2) {
            return Vec::new();
        }
        for rewrite in apply(input, &edits) {
            if !rewrites.contains(&rewrite) {
                rewrites.push(rewrite);
            }
        }
    }
    rewrites
}

/// The handles a `%TAG` directive of `input` defines.
fn tag_handles(input: &str) -> Vec<&str> {
    input
        .lines()
        .filter_map(|line| line.strip_prefix("%TAG"))
        .filter_map(|rest| rest.split_whitespace().next())
        .collect()
}

/// Whether the node after a property that ends at byte `at` of `input` is
/// empty: a flow entry or the value ends, or an implicit key's `:` comes,
/// before any content, or the line ends and the node is not on a later line
/// (see [`node_below`]).
fn empty_node_after(input: &str, at: usize) -> bool {
    let after = input[at..].trim_start_matches([' ', '\t']);
    if after.is_empty() || after.starts_with(['\n', '\r', '#']) {
        return !node_below(input, at);
    }
    after.starts_with([',', ']', '}']) || starts_indicator(after, ':')
}

/// Whether the node of the properties that end a line at byte `at` of
/// `input` is on a later line, as `!!map` on a line of its own is the
/// block sequence below it. The next line with content holds it when it is
/// indented more than the properties' line, or as much when the properties
/// start their line or follow a key's `:` that a block sequence follows,
/// and is not a document marker; in a flow collection, the next token holds
/// it unless it ends the entry.
fn node_below(input: &str, at: usize) -> bool {
    let start = input[..at]
        .rfind(['\n', '\r'])
        .map_or(0, |offset| offset + 1);
    let line = input[start..at].trim_start_matches('\u{feff}');
    let indent = line.len() - line.trim_start_matches(' ').len();
    // The line before its properties.
    let mut before = line.trim_end();
    loop {
        let token = before
            .char_indices()
            .rev()
            .find(|&(_, ch)| ch.is_whitespace() || "[{,".contains(ch))
            .map_or(0, |(offset, ch)| offset + ch.len_utf8());
        if token == before.len() || !before[token..].starts_with(['!', '&']) {
            break;
        }
        before = before[..token].trim_end();
    }
    let before = before.trim_start();
    let below = input[at..]
        .split_inclusive(['\n', '\r'])
        .skip(1)
        .find(|next| {
            let text = next.trim();
            !text.is_empty() && !text.starts_with('#')
        });
    if before.ends_with(['[', '{', ',']) {
        return below.is_some_and(|next| !next.trim_start().starts_with([',', ']', '}', ':']));
    }
    let Some(next) = below else {
        return false;
    };
    // A document marker ends the document; it holds no node.
    let marker = ["---", "..."].iter().any(|marker| {
        next.strip_prefix(marker)
            .is_some_and(|after| after.is_empty() || after.starts_with([' ', '\t', '\r', '\n']))
    });
    if marker {
        return false;
    }
    let next_indent = next.len() - next.trim_start_matches(' ').len();
    next_indent > indent
        || (next_indent == indent
            && (matches!(before, "" | "---")
                || (before.ends_with(':') && starts_indicator(next.trim_start(), '-'))))
}

/// Without the core schema's tags, unless a `%TAG` directive redefines `!!`,
/// which makes them global tags; then also with each quoted scalar such a
/// tag was on unquoted, so that `!!int '1'` leaves the number it was read as;
/// then with a tag on an empty node as `~`, the null the node is untagged,
/// so that `!!map : x` does not become an empty key; then with only the
/// tags on empty nodes replaced so, the others kept; then with each as a
/// local tag of its width, as `!xint` for `!!int`, which both parsers read
/// alike at every entry point, and which moves nothing, where a deletion
/// would move a key off its column or make the text after the tag start the
/// stream anew.
pub(super) fn core_tags(input: &str) -> Vec<String> {
    if tag_handles(input).contains(&"!!") {
        return Vec::new();
    }
    let ranges: Vec<Range<usize>> = tokens(input, &['!'])
        .into_iter()
        .filter(|range| CORE_TAGS.contains(&&input[range.clone()]))
        .collect();
    let mut rewrites = delete(input, ranges.clone());
    let nulls: Vec<(Range<usize>, String)> = ranges
        .iter()
        .map(|range| {
            if empty_node_after(input, range.end) {
                (range.clone(), "~".to_string())
            } else {
                (deletion(input, range.clone()), String::new())
            }
        })
        .collect();
    // Only the tags on empty nodes, as `~`: the others keep the kinds they
    // give, as `!!str` does a number's.
    let empty: Vec<(Range<usize>, String)> = ranges
        .iter()
        .filter(|range| empty_node_after(input, range.end))
        .map(|range| (range.clone(), "~".to_string()))
        .collect();
    let local: Vec<(Range<usize>, String)> = ranges
        .iter()
        .map(|range| {
            (
                range.clone(),
                format!("!x{}", &input[range.start + 2..range.end]),
            )
        })
        .collect();
    let mut edits = Vec::new();
    for range in ranges {
        let after = &input[range.end..];
        let gap = after.len() - after.trim_start_matches([' ', '\t']).len();
        let quoted = &after[gap..];
        let close = quoted
            .chars()
            .next()
            .filter(|quote| matches!(quote, '\'' | '"'))
            .and_then(|quote| quoted[1..].find(quote).map(|at| (quote, at + 1)));
        match close {
            Some((quote, close))
                if !quoted[1..close].contains(['\\', '\n', '\'', '"', '#', ':'])
                    && !quoted[1..close].is_empty() =>
            {
                let start = range.end + gap;
                edits.push((deletion(input, range), String::new()));
                edits.push((
                    start..start + close + quote.len_utf8(),
                    quoted[1..close].to_string(),
                ));
            }
            _ => edits.push((deletion(input, range), String::new())),
        }
    }
    for rewrite in apply(input, &edits)
        .into_iter()
        .chain(apply(input, &nulls))
        .chain(apply(input, &empty))
        .chain(apply(input, &local))
    {
        if !rewrites.contains(&rewrite) {
            rewrites.push(rewrite);
        }
    }
    rewrites
}

/// The root block scalar's content indented by one more space: every line
/// after its header, up to a document marker.
pub(super) fn root_block_scalar_content(input: &str) -> Vec<String> {
    let mut out = String::with_capacity(input.len() + 64);
    let mut header_seen = false;
    let mut changed = false;
    let mut ended = false;
    for line in input.split_inclusive(['\n', '\r']) {
        if !header_seen {
            let text = line.trim_start_matches('\u{feff}').trim();
            let mut rest = text.strip_prefix("---").unwrap_or(text).trim_start();
            if !text.is_empty() && !text.starts_with(['#', '%']) {
                while rest.starts_with(['!', '&']) {
                    rest = rest
                        .split_once(char::is_whitespace)
                        .map_or("", |(_, after)| after.trim_start());
                }
                if rest.starts_with(['|', '>']) {
                    header_seen = true;
                } else if !rest.is_empty() && !rest.starts_with('#') {
                    return Vec::new();
                }
            }
        } else if !ended {
            let marker = |text: &str| {
                line.strip_prefix(text).is_some_and(|after| {
                    after.is_empty() || after.starts_with([' ', '\t', '\r', '\n'])
                })
            };
            if marker("---") || marker("...") {
                ended = true;
            } else if !line.trim_end_matches(['\n', '\r']).is_empty() {
                out.push(' ');
                changed = true;
            }
        }
        out.push_str(line);
    }
    if changed { vec![out] } else { Vec::new() }
}

/// Whether `tag` names a handle, as `!e!z`.
fn named_handle(tag: &str) -> Option<&str> {
    let close = tag[1..].find('!')? + 1;
    let handle = &tag[..=close];
    (close > 1 && handle[1..close].chars().all(narrow_name_character)).then_some(handle)
}

/// Without each global tag: a verbatim tag that names a global tag, not a
/// local one, as `!<!t>`, which both parsers read as `!t`, a tag of a
/// handle a `%TAG` directive defines, `!!` with a name outside the core
/// schema, and every `!` tag when a `%TAG` directive redefines `!`. Then,
/// unless `!` is redefined, with each as the local tag `!t`, which both
/// parsers read alike and which keeps a tagged key apart from an untagged
/// one. Then, unless `!!` is redefined, with each on a scalar as `!!str`,
/// which reads the scalar as the oracle reads a globally tagged one, as a
/// string, so that removing the tag does not make a number of it that the
/// oracle cannot hold.
pub(super) fn global_tags(input: &str) -> Vec<String> {
    let handles = tag_handles(input);
    let global = |tag: &str| {
        if let Some(verbatim) = tag.strip_prefix("!<") {
            return !matches!(verbatim, "" | ">") && !verbatim.starts_with('!');
        }
        if let Some(name) = tag.strip_prefix("!!") {
            return !name.is_empty() && (handles.contains(&"!!") || !CORE_TAGS.contains(&tag));
        }
        match named_handle(tag) {
            Some(handle) => handles.contains(&handle),
            None => tag.len() > 1 && handles.contains(&"!"),
        }
    };
    let ranges: Vec<Range<usize>> = tokens(input, &['!'])
        .into_iter()
        .filter(|range| global(&input[range.clone()]))
        .collect();
    let mut rewrites = delete(input, ranges.clone());
    if !handles.contains(&"!") {
        let local: Vec<_> = ranges
            .iter()
            .map(|range| (range.clone(), "!t".to_string()))
            .collect();
        rewrites.extend(apply(input, &local));
    }
    if !handles.contains(&"!!") && ranges.iter().any(|range| tags_a_scalar(input, range.end)) {
        let strings: Vec<_> = ranges
            .into_iter()
            .map(|range| {
                if tags_a_scalar(input, range.end) {
                    (range, "!!str".to_string())
                } else {
                    (deletion(input, range.clone()), String::new())
                }
            })
            .collect();
        rewrites.extend(apply(input, &strings));
    }
    rewrites
}

/// Whether the node after a property that ends at byte `at` of `input` is a
/// scalar written on the same line, after any anchor.
fn tags_a_scalar(input: &str, at: usize) -> bool {
    let mut after = input[at..].trim_start_matches([' ', '\t']);
    if let Some(anchor) = after.strip_prefix('&') {
        after = anchor
            .trim_start_matches(|ch: char| !ch.is_whitespace())
            .trim_start_matches([' ', '\t']);
    }
    !empty_node_after(after, 0) && !after.starts_with(['[', '{', '*', '&', '!', '|', '>'])
}

/// Without each non-specific tag, a `!` alone; then with one on an empty
/// node as `''`, the empty string YAML 1.2 makes of that node, so that
/// `[!, x]` keeps its entry.
pub(super) fn non_specific_tags(input: &str) -> Vec<String> {
    let ranges: Vec<Range<usize>> = tokens(input, &['!'])
        .into_iter()
        .filter(|range| range.len() == 1)
        .collect();
    let mut rewrites = delete(input, ranges.clone());
    if ranges
        .iter()
        .any(|range| empty_node_after(input, range.end))
    {
        let strings: Vec<(Range<usize>, String)> = ranges
            .into_iter()
            .map(|range| {
                if empty_node_after(input, range.end) {
                    (range, "''".to_string())
                } else {
                    (deletion(input, range.clone()), String::new())
                }
            })
            .collect();
        rewrites.extend(apply(input, &strings));
    }
    rewrites
}

/// The characters YAML 1.2 allows in a quoted scalar only (JSON characters
/// outside the printable set): DEL, the C1 controls but NEL, U+FFFE and
/// U+FFFF.
pub(super) fn quoted_only_character(ch: char) -> bool {
    matches!(ch, '\u{7f}'..='\u{84}' | '\u{86}'..='\u{9f}' | '\u{fffe}' | '\u{ffff}')
}

/// Without the characters YAML 1.2 allows only in a quoted scalar, which
/// ayaml keeps or never reads; then with each as `x`; then, of those,
/// without the first and as `x`, where more follow. ayaml keeps the ones its
/// scanner read without an error: all of them when it reads the input or
/// rejects it as it parses or builds values, after its scanner read the
/// whole stream, and those before its scanner's error otherwise, past which
/// it reads none. In a malformed document its scanner may read a quoted
/// scalar where YAML 1.2 reads a plain one. Its reader rejects a C0 control
/// before its scanner reads anything, and then the ones [`places`] reads in
/// a quoted scalar stand for them. One it rejects, which both parsers
/// reject, stays. One where ayaml reports an error about something else,
/// as a quoted scalar's line that is not indented enough, is always `x`,
/// which keeps that error: without it, the line could hold nothing. The
/// oracle's reader rejects them anywhere before its scanner reads a token,
/// and its error names only the first: a rewrite of the first alone shows
/// its error at the next.
pub(super) fn quoted_only_characters(input: &str) -> Vec<String> {
    if !input.contains(quoted_only_character) {
        return Vec::new();
    }
    let places = places(input);
    let (side, _) = super::guarded(|| super::run_ayaml(input, super::Entry::Value, 0));
    let error = super::predicates::error_offset(input, &side);
    // A C0 control but a tab or a line break, which ayaml's reader rejects.
    let control = error
        .and_then(|at| input[at..].chars().next())
        .is_some_and(|ch| matches!(ch, '\0'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}'));
    let scanned = side.is_ok() || (!control && error.is_some());
    let at_error = error.filter(|&at| input[at..].starts_with(quoted_only_character));
    let (rejected, elsewhere) = match at_error {
        Some(at) if rejects_the_character(input, at, &side) => (Some(at), None),
        at => (None, at),
    };
    let quoted: Vec<Range<usize>> = input
        .char_indices()
        .filter(|&(at, ch)| {
            quoted_only_character(ch) && rejected != Some(at) && (scanned || places[at].quoted)
        })
        .map(|(at, ch)| at..at + ch.len_utf8())
        .collect();
    if quoted.is_empty() {
        return Vec::new();
    }
    // The first alone repeats them all where it is the only one, and the
    // rewrites it gives then go below.
    let sets: [&[Range<usize>]; 2] = [&quoted, &quoted[..1]];
    let mut rewrites: Vec<String> = Vec::new();
    let all = sets.iter().flat_map(|set| {
        ["", "x"].iter().flat_map(|with| {
            let edits: Vec<(Range<usize>, String)> = set
                .iter()
                .map(|range| {
                    let with = if elsewhere == Some(range.start) {
                        "x"
                    } else {
                        with
                    };
                    (range.clone(), with.to_string())
                })
                .collect();
            apply(input, &edits)
        })
    });
    for rewrite in all {
        if !rewrites.contains(&rewrite) {
            rewrites.push(rewrite);
        }
    }
    rewrites
}

/// Whether ayaml's error `side`, at byte `at` of `input`, is about the
/// character there: with `x` in its place, ayaml reports another error, or
/// none.
fn rejects_the_character(input: &str, at: usize, side: &super::Side) -> bool {
    let Some(ch) = input[at..].chars().next() else {
        return false;
    };
    let mut other = input.to_string();
    other.replace_range(at..at + ch.len_utf8(), "x");
    let (again, _) = super::guarded(|| super::run_ayaml(&other, super::Entry::Value, 0));
    match (side, &again) {
        (Err(before), Err(after)) => {
            before.message != after.message || before.location != after.location
        }
        _ => true,
    }
}

/// The longest implicit key YAML 1.2 allows, in characters, and the oracle,
/// in bytes.
const KEY_LIMIT: usize = 1024;

/// `input` with each implicit key cut to its first and last characters up to
/// `keep` bytes each, where `long` holds given its characters' byte offsets
/// and its end. A key here is a run that holds no flow indicator or line
/// break, starts outside a quoted scalar, a comment and a block scalar, and
/// ends at a `:` outside them that a space, a line break, a flow indicator or
/// the end follows; a `:` before anything else is part of the run, as in
/// `a:b`. A long scalar elsewhere is left alone, so that a cut cannot hide a
/// difference in it. The `n`th different key keeps `n` fewer bytes at its
/// end, and fewer still while it would be written as another key of the
/// input or as another key's cut, so that keys that differ stay apart and
/// equal keys stay equal.
fn cut_runs(input: &str, long: impl Fn(&[usize], usize) -> bool, keep: usize) -> Vec<String> {
    let places = places(input);
    // Every key as written, and the long ones, by their characters' byte
    // offsets and their end.
    let mut written: Vec<&str> = Vec::new();
    let mut found: Vec<(Vec<usize>, usize)> = Vec::new();
    // The byte offset of each character of the current run.
    let mut run: Vec<usize> = Vec::new();
    for (at, ch) in input.char_indices().chain([(input.len(), '\n')]) {
        let key = ch == ':'
            && !places[at].literal
            && input[at + 1..]
                .chars()
                .next()
                .is_none_or(|next| next.is_whitespace() || ",[]{}".contains(next));
        if key || ",[]{}\n\r".contains(ch) {
            // The key starts after the white space before it.
            let blank = run
                .iter()
                .take_while(|&&offset| input[offset..].starts_with([' ', '\t']))
                .count();
            let key_run = &run[blank..];
            if let Some(&first) = key_run.first()
                && key
                && !places[first].literal
            {
                written.push(input[first..at].trim());
                if long(key_run, at) {
                    found.push((key_run.to_vec(), at));
                }
            }
            run.clear();
        } else {
            run.push(at);
        }
    }
    // Each different key, the bytes it keeps at its end, and its cut.
    let mut keys: Vec<(&str, usize, String)> = Vec::new();
    let mut edits = Vec::new();
    for (key_run, at) in found {
        let first = key_run[0];
        let text = input[first..at].trim();
        let Some(&head) = key_run.iter().rfind(|&&offset| offset - first <= keep) else {
            continue;
        };
        let tail_of = |kept: usize| key_run.iter().find(|&&offset| at - offset <= kept).copied();
        let cut_at = |tail: usize| {
            format!("{}{}", &input[first..head], &input[tail..at])
                .trim()
                .to_string()
        };
        let seen = keys
            .iter()
            .find(|(seen, ..)| *seen == text)
            .map(|(_, kept, _)| *kept);
        let kept = seen.unwrap_or_else(|| {
            let mut kept = keep.saturating_sub(keys.len());
            let mut cut = String::new();
            while let Some(tail) = tail_of(kept) {
                cut = cut_at(tail);
                if !written.contains(&cut.as_str()) && keys.iter().all(|(.., other)| *other != cut)
                {
                    break;
                }
                kept -= 1;
            }
            keys.push((text, kept, cut));
            kept
        });
        if let Some(tail) = tail_of(kept)
            && head < tail
        {
            edits.push((head..tail, String::new()));
        }
    }
    apply(input, &edits)
}

/// The sign and magnitude of `text` when it is an integer that ayaml reads,
/// which a 128-bit integer holds and a 64-bit one does not: a sign, then
/// decimal digits with no leading zero, or `0x`, `0o` or `0b` and digits of
/// that base.
fn wide_integer(text: &str) -> Option<(bool, u128)> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (radix, digits) = [("0x", 16), ("0o", 8), ("0b", 2)]
        .into_iter()
        .find_map(|(prefix, radix)| digits.strip_prefix(prefix).map(|rest| (radix, rest)))
        .unwrap_or((10, digits));
    if digits.is_empty()
        || !digits.chars().all(|ch| ch.is_digit(radix))
        || (radix == 10 && digits.len() > 1 && digits.starts_with('0'))
    {
        return None;
    }
    let magnitude = u128::from_str_radix(digits, radix).ok()?;
    let wide = if negative {
        magnitude > 1 << 63 && magnitude <= 1 << 127
    } else {
        magnitude > u128::from(u64::MAX)
    };
    wide.then_some((negative, magnitude))
}

/// The plain scalars of `input` that are letters and digits after a sign or
/// none, which start with a digit or the sign, as `-0` or `0x10`: each starts
/// a node and ends before white space, a `:` that white space follows or,
/// in a flow collection, a flow indicator. Not one in a quoted scalar, a
/// comment, a block scalar or a plain scalar's text, as `x 0`. Only in a
/// flow collection do `,`, `[`, `]`, `{` and `}` end a plain scalar.
fn plain_numbers(input: &str) -> Vec<Range<usize>> {
    let places = places(input);
    let mut found = Vec::new();
    let mut before = ' ';
    for (at, ch) in input.char_indices() {
        let flow = places[at].depth > 0;
        let starts = before.is_whitespace() || (flow && "[{,".contains(before));
        before = ch;
        if !starts
            || places[at].literal
            || places[at].plain
            || !(ch.is_ascii_digit() || ch == '-' || ch == '+')
        {
            continue;
        }
        let sign = usize::from(!ch.is_ascii_digit());
        let end = at
            + sign
            + input[at + sign..]
                .bytes()
                .take_while(u8::is_ascii_alphanumeric)
                .count();
        let rest = &input[end..];
        let (closing, after_colon) = if flow { (",]}", ",[]{}") } else { ("", "") };
        let ends = |text: &str, indicators: &str| {
            text.chars()
                .next()
                .is_none_or(|next| next.is_whitespace() || indicators.contains(next))
        };
        if end > at + sign
            && (ends(rest, closing) || (rest.starts_with(':') && ends(&rest[1..], after_colon)))
        {
            found.push(at..end);
        }
    }
    found
}

/// With each plain `-0` written as `+0`, the same integer, which both
/// parsers decode into an unsigned field; not one in a longer scalar, as
/// `-00` or `x -0`, or in a quoted or block scalar or a comment.
pub(super) fn negative_zeros(input: &str) -> Vec<String> {
    let mut out = input.to_string();
    let mut changed = false;
    for range in plain_numbers(input) {
        if &input[range.clone()] == "-0" {
            out.replace_range(range.start..=range.start, "+");
            changed = true;
        }
    }
    if changed { vec![out] } else { Vec::new() }
}

/// With each plain integer that a 128-bit integer holds and a 64-bit one
/// does not, as `18446744073709551616` or `0x10000000000000000`, written as
/// `1` after its sign; then with each such value written as a number of its
/// own that the rest of the input does not hold, so that a rewritten key
/// meets no other key, as `1` would meet the key of `? 1`. Not one in a
/// quoted scalar, a comment, a block scalar or a plain scalar's text, as
/// `x 18446744073709551616`. Only in a flow collection do
/// `,`, `[`, `]`, `{` and `}` end a plain scalar.
pub(super) fn wide_integers(input: &str) -> Vec<String> {
    let found: Vec<(Range<usize>, (bool, u128))> = plain_numbers(input)
        .into_iter()
        .filter_map(|range| {
            let sign = usize::from(input[range.clone()].starts_with(['-', '+']));
            let value = wide_integer(&input[range.clone()])?;
            Some((range.start + sign..range.end, value))
        })
        .collect();
    let ones: Vec<(Range<usize>, String)> = found
        .iter()
        .map(|(range, _)| (range.clone(), "1".to_string()))
        .collect();
    // The input without the integers, which their numbers must not be in;
    // one value keeps one number.
    let mut rest = String::with_capacity(input.len());
    let mut from = 0;
    for (range, _) in &found {
        rest.push_str(&input[from..range.start]);
        rest.push(' ');
        from = range.end;
    }
    rest.push_str(&input[from..]);
    let mut numbers: Vec<((bool, u128), String)> = Vec::new();
    let mut next = 1_u32;
    let mut own = Vec::new();
    for (range, value) in found {
        let index = numbers
            .iter()
            .position(|(seen, _)| *seen == value)
            .unwrap_or_else(|| {
                while rest.contains(&next.to_string()) {
                    next += 1;
                }
                numbers.push((value, next.to_string()));
                next += 1;
                numbers.len() - 1
            });
        own.push((range, numbers[index].1.clone()));
    }
    let mut rewrites = apply(input, &ones);
    for rewrite in apply(input, &own) {
        if !rewrites.contains(&rewrite) {
            rewrites.push(rewrite);
        }
    }
    rewrites
}

/// With each plain scalar of a flow collection that starts with `:` before
/// a non-space, after properties that start its node, single-quoted: as
/// `[!!str ':a']` for `[!!str :a]`, which libyaml reads as an empty key with
/// the properties and the value `a`. The scalar ends before a flow
/// indicator, a line break, a `#` after white space, or a `:` that white
/// space or a flow indicator follows.
pub(super) fn flow_colon_scalars(input: &str) -> Vec<String> {
    let places = places(input);
    let plain = |ch: char| !ch.is_whitespace() && !",[]{}".contains(ch);
    let mut edits = Vec::new();
    for (colon, ch) in input.char_indices() {
        if ch != ':'
            || places[colon].literal
            || places[colon].depth == 0
            || !input[colon + 1..].chars().next().is_some_and(plain)
        {
            continue;
        }
        let before = input[..colon].trim_end_matches([' ', '\t']);
        // White space may be wider than a byte, as a line separator.
        let token = before
            .char_indices()
            .rfind(|&(_, ch)| ch.is_whitespace() || "[{,".contains(ch))
            .map_or(0, |(offset, ch)| offset + ch.len_utf8());
        if before.len() == colon
            || !before[token..].starts_with(['!', '&'])
            || !super::predicates::starts_a_flow_node(input, &places, colon)
        {
            continue;
        }
        let rest = &input[colon..];
        let mut end = rest.len();
        let mut previous = ':';
        for (offset, ch) in rest.char_indices().skip(1) {
            let next = rest[offset + ch.len_utf8()..].chars().next();
            if ",[]{}\n\r".contains(ch)
                || (ch == '#' && previous.is_whitespace())
                || (ch == ':' && next.is_none_or(|next| !plain(next)))
            {
                end = offset;
                break;
            }
            previous = ch;
        }
        let scalar = rest[..end].trim_end();
        edits.push((
            colon..colon + scalar.len(),
            format!("'{}'", scalar.replace('\'', "''")),
        ));
    }
    apply(input, &edits)
}

/// With `~` after each `?` in a flow collection that a `,`, `]` or `}`
/// follows, past white space and line breaks: an explicit key with nothing
/// in it, as `[? ~]` for `[? ]`, where libyaml takes the indicator for the
/// key's end and drops it. A `?` in a quoted scalar, a comment or a block
/// scalar is not one, nor is one that does not open an entry.
pub(super) fn empty_flow_keys(input: &str) -> Vec<String> {
    let places = places(input);
    let white = |ch: char| matches!(ch, ' ' | '\t' | '\n' | '\r');
    let mut edits = Vec::new();
    for (at, ch) in input.char_indices() {
        if ch != '?' || places[at].literal || places[at].depth == 0 {
            continue;
        }
        let rest = &input[at + 1..];
        let opens = input[..at]
            .char_indices()
            .rev()
            .find(|&(offset, ch)| !white(ch) && !places[offset].literal)
            .is_some_and(|(_, ch)| "[{,".contains(ch));
        if opens
            && rest.starts_with(white)
            && rest.trim_start_matches(white).starts_with([',', ']', '}'])
        {
            edits.push((at + 1..at + 1, " ~".to_string()));
        }
    }
    apply(input, &edits)
}

/// With `~` as the key of each entry of a flow collection whose implicit key
/// is empty, as `[~: x]` for `[: x]`, and a space after its `:` where a flow
/// indicator follows, as `{~: }` for `{:}`: YAML 1.2 reads the key as null,
/// and libyaml reads `~` and rejects the empty key. A `:` opens such an
/// entry where a `[`, `{` or `,` comes before it, past white space and line
/// breaks, and white space, a line break, a flow indicator or the end comes
/// after it. One in a quoted scalar, a comment or a block scalar is not one.
pub(super) fn empty_flow_implicit_keys(input: &str) -> Vec<String> {
    let places = places(input);
    let white = |ch: char| matches!(ch, ' ' | '\t' | '\n' | '\r');
    let mut edits = Vec::new();
    for (at, ch) in input.char_indices() {
        if ch != ':' || places[at].literal || places[at].depth == 0 {
            continue;
        }
        let opens = input[..at]
            .char_indices()
            .rev()
            .find(|&(offset, ch)| !white(ch) && !places[offset].literal)
            .is_some_and(|(_, ch)| "[{,".contains(ch));
        let rest = &input[at + 1..];
        if opens && (rest.is_empty() || rest.starts_with(white)) {
            edits.push((at..at, "~".to_string()));
        } else if opens && rest.starts_with([',', '[', ']', '{', '}']) {
            edits.push((at..at + 1, "~: ".to_string()));
        }
    }
    apply(input, &edits)
}

/// The byte offsets of the verbatim tags that name no tag which ayaml
/// rejects: each found by reading the input again once the one it reported
/// before is a local tag and a space, of its width, until it reports an
/// error elsewhere or none. Its scanner reports them before any parser
/// error, in their order. In a malformed document it may read one where
/// YAML 1.2 continues a plain scalar, or right after a quoted scalar.
fn nameless_verbatim_tags_ayaml_rejects(input: &str) -> Vec<usize> {
    let mut tags = Vec::new();
    let mut named = input.to_string();
    for _ in 0..input.matches(NAMELESS_VERBATIM).count() {
        let (side, _) = super::guarded(|| super::run_ayaml(&named, super::Entry::Value, 0));
        let Some(at) = super::predicates::error_offset(&named, &side).filter(|&at| {
            named[at..].starts_with(NAMELESS_VERBATIM)
                && side
                    .as_ref()
                    .is_err_and(|rejection| rejection.message.contains(VERBATIM_NEEDS_A_NAME))
        }) else {
            break;
        };
        tags.push(at);
        named.replace_range(at..at + NAMELESS_VERBATIM.len(), "!tt ");
    }
    tags
}

/// Each verbatim tag that names no tag, `!<!>`, which the oracle reads as
/// the non-specific tag `!` and ayaml rejects: where it starts a node, after
/// white space, a line break, the start or `[`, `{` or `,`, outside a scalar
/// and a comment, or where ayaml rejects it (see
/// [`nameless_verbatim_tags_ayaml_rejects`]). One that [`places`] reads in a
/// scalar or a comment counts only when the oracle rejects the input: where
/// it reads one, ayaml reads a tag in its text, which is no form of this.
/// With each as `!<!t>`, which both parsers read alike, as a tag: a node
/// keeps its properties, an alias after it stays an error, and so does a
/// character after it with no white space between, as in `!<!>~`.
pub(super) fn verbatim_non_specific_tags(input: &str) -> Vec<String> {
    if !input.contains(NAMELESS_VERBATIM) {
        return Vec::new();
    }
    let places = places(input);
    let text = |at: usize| places[at].literal || places[at].plain;
    let mut tags: Vec<usize> = input
        .match_indices(NAMELESS_VERBATIM)
        .map(|(at, _)| at)
        .filter(|&at| {
            let starts = input[..at]
                .chars()
                .next_back()
                .is_none_or(|ch| matches!(ch, ' ' | '\t' | '\n' | '\r' | '[' | '{' | ','));
            starts && !text(at)
        })
        .collect();
    let mut oracle_rejects = None;
    for at in nameless_verbatim_tags_ayaml_rejects(input) {
        if tags.contains(&at) {
            continue;
        }
        if text(at)
            && !*oracle_rejects.get_or_insert_with(|| {
                super::guarded(|| super::run_oracle(input, super::Entry::Value, 0))
                    .0
                    .is_err()
            })
        {
            continue;
        }
        tags.push(at);
    }
    tags.sort_unstable();
    let edits: Vec<(Range<usize>, String)> = tags
        .into_iter()
        .map(|at| (at..at + NAMELESS_VERBATIM.len(), "!<!t>".to_string()))
        .collect();
    apply(input, &edits)
}

/// With each implicit key of more than 1,024 characters, as [`cut_runs`]
/// finds keys, cut to its first and last 500 bytes.
pub(super) fn long_runs(input: &str) -> Vec<String> {
    cut_runs(input, |run, _| run.len() > KEY_LIMIT, 500)
}

/// With each such key of 1,024 characters or fewer but more than 1,024
/// bytes cut the same way.
pub(super) fn wide_runs(input: &str) -> Vec<String> {
    cut_runs(
        input,
        |run, end| run.len() <= KEY_LIMIT && end - run[0] > KEY_LIMIT,
        500,
    )
}

#[cfg(test)]
#[path = "neutralize_tests.rs"]
mod tests;
