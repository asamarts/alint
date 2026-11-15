//! Neutralizers: each removes one entry's trigger from an input, so the
//! harness can tell whether that trigger causes a difference (see
//! [`super::allowlist::Trigger::Input`]). Each returns the rewritten inputs
//! to try, in order, and none when the input holds no trigger. A rewrite
//! changes only the trigger, so a difference it leaves has another cause.

use std::collections::HashMap;
use std::ops::Range;

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
/// property or an alias can: not in a directive, a quoted scalar, a comment
/// or a block scalar, as [`places`] reads them. A token runs to white
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
pub(super) struct Place {
    /// The flow collections around it.
    pub(super) depth: u32,
    /// Whether it is inside a quoted scalar, a comment, a directive or a
    /// block scalar's header or content, rather than structure.
    pub(super) literal: bool,
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
    // The column a block scalar's content is indented past, while it lasts.
    let mut header: Option<usize> = None;
    // The column of the key or indicator of a plain scalar that ran to the
    // end of its line.
    let mut continued: Option<usize> = None;
    // The column of the key or indicator that ended the last line, whose
    // node starts on a later one.
    let mut open: Option<usize> = None;
    for line in input.split_inclusive(['\n', '\r']) {
        let text = line.trim_end_matches(['\n', '\r']);
        let indent = text.len() - text.trim_start_matches(' ').len();
        let blank = text.trim().is_empty();
        if let Some(parent) = header {
            if blank || indent > parent {
                places.extend(std::iter::repeat_n(
                    Place {
                        depth,
                        literal: true,
                    },
                    line.len(),
                ));
                continue;
            }
            header = None;
        }
        let block = depth == 0 && quote.is_none();
        let continuation = continued.filter(|owner| {
            block && !blank && indent > *owner && !text.trim_start().starts_with('#')
        });
        let marker = (text.starts_with("---") || text.starts_with("..."))
            && text[3..].chars().next().is_none_or(char::is_whitespace);
        let mut comment = block && text.starts_with('%');
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
            places.extend(std::iter::repeat_n(Place { depth, literal }, ch.len_utf8()));
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
                        }
                        '#' => comment = true,
                        '|' | '>' => {
                            header = Some(owner);
                            comment = true;
                        }
                        _ if ch.is_whitespace() || ch == '\u{feff}' => {}
                        _ => {
                            node.get_or_insert(column);
                            state = match ch {
                                '&' | '!' => Block::Property,
                                '*' => Block::After,
                                '[' | '{' => {
                                    depth = 1;
                                    flow = Flow::Start;
                                    Block::After
                                }
                                '\'' | '"' => {
                                    quote = Some(ch);
                                    Block::After
                                }
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
                        }
                        '#' if before.is_whitespace() => comment = true,
                        _ => {}
                    },
                }
            }
            before = ch;
        }
        if !blank {
            let block = depth == 0 && quote.is_none();
            continued = (block && !comment && state == Block::Scalar).then_some(owner);
            open = (block && state == Block::Start).then_some(owner);
        }
    }
    places.push(Place {
        depth,
        literal: quote.is_some(),
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
/// `!t   c: 2` becomes `c: 2`.
fn deletion(input: &str, range: Range<usize>) -> Range<usize> {
    let line = input[..range.start]
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    let first = input[line..range.start]
        .chars()
        .all(|ch| " \t-?:\u{feff}".contains(ch));
    let rest = &input[range.end..];
    let gap = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    range.start..range.end + if first { gap } else { 0 }
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
/// that two keys stay apart.
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

/// With a tab as a space, wherever it is; then with the tabs that only
/// spaces come before on their line left out, and then as content (`x`),
/// where the first line of a block scalar can hold them; the other tabs as
/// spaces.
pub(super) fn tabs(input: &str) -> Vec<String> {
    if !input.contains('\t') {
        return Vec::new();
    }
    let leading = |with: &str| {
        let mut out = String::with_capacity(input.len());
        let mut line_start = true;
        for ch in input.chars() {
            match ch {
                '\t' if line_start => out.push_str(with),
                '\t' => out.push(' '),
                _ => out.push(ch),
            }
            line_start = matches!(ch, '\n' | '\r') || (line_start && matches!(ch, ' ' | '\t'));
        }
        out
    };
    let mut rewrites = vec![input.replace('\t', " ")];
    for rewrite in [leading(""), leading("x")] {
        if !rewrites.contains(&rewrite) {
            rewrites.push(rewrite);
        }
    }
    rewrites
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
        } else if let Some(stem) = name.strip_suffix(':')
            && let Some(renamed) = anchors.get(stem)
        {
            (renamed != stem).then(|| (name_range.start..name_range.end - 1, renamed.clone()))
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
/// tags on empty nodes replaced so, the others kept.
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

/// Without each global tag: a verbatim tag that names a tag, a tag of a
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
            return !matches!(verbatim, "" | ">" | "!>");
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

/// Without the characters YAML 1.2 allows only in a quoted scalar; then with
/// each as `x`.
pub(super) fn quoted_only_characters(input: &str) -> Vec<String> {
    if !input.contains(quoted_only_character) {
        return Vec::new();
    }
    vec![
        input.replace(quoted_only_character, ""),
        input.replace(quoted_only_character, "x"),
    ]
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

/// With each plain integer that a 128-bit integer holds and a 64-bit one
/// does not, as `18446744073709551616` or `0x10000000000000000`, written as
/// `1` after its sign; then with each such value written as a number of its
/// own that the rest of the input does not hold, so that a rewritten key
/// meets no other key, as `1` would meet the key of `? 1`. Not one in a
/// quoted scalar, a comment or a block scalar. Only in a flow collection do
/// `,`, `[`, `]`, `{` and `}` end a plain scalar.
pub(super) fn wide_integers(input: &str) -> Vec<String> {
    let places = places(input);
    let mut found = Vec::new();
    let mut before = ' ';
    for (at, ch) in input.char_indices() {
        let flow = places[at].depth > 0;
        let starts = before.is_whitespace() || (flow && "[{,".contains(before));
        before = ch;
        if !starts || places[at].literal || !(ch.is_ascii_digit() || ch == '-' || ch == '+') {
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
            && let Some(value) = wide_integer(&input[at..end])
        {
            found.push((at + sign..end, value));
        }
    }
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
        let token = before
            .rfind(|ch: char| ch.is_whitespace() || "[{,".contains(ch))
            .map_or(0, |offset| offset + 1);
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
mod tests {
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
        let input = "&a:b x\n[*c:d, &e?f g, *h?]\n";
        let found: Vec<&str> = tokens(input, &['&', '*'])
            .into_iter()
            .map(|range| &input[range])
            .collect();
        assert_eq!(found, ["&a", "*c:d", "&e", "*h?"]);
    }

    #[test]
    fn an_edit_that_overlaps_the_one_before_is_left_out() {
        let edits = [(0..2, "x".to_string()), (1..3, "y".to_string())];
        assert_eq!(apply("abcd", &edits), ["xcd"]);
    }

    #[test]
    fn places_follow_flow_collections_quotes_and_block_scalars() {
        let depths = |input: &str| -> Vec<u32> {
            places(input).into_iter().map(|place| place.depth).collect()
        };
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
        assert_eq!(
            one(wide_anchor_names("run: make && test\n")),
            "run: make &_ test\n"
        );
        assert_eq!(one(wide_anchor_names("&é x")), "&_ x");
        assert_eq!(wide_anchor_names("&a1: x\n")[1], "&a1 : x\n");
        // `*a:` is the alias `a` and a `:`.
        assert_eq!(
            wide_anchor_names("- &a k\n- {*a: v}\n"),
            Vec::<String>::new()
        );
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
            one(core_tags("a: !!map 1\nb: !!binary x\n")),
            "a:  1\nb: !!binary x\n"
        );
        assert_eq!(
            core_tags("a: !!int '1'\nb: !!str \"x:y\"\n"),
            ["a:  '1'\nb:  \"x:y\"\n", "a:  1\nb:  \"x:y\"\n"]
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
            ["a: '\u{85}'\n", "a: 'x\u{85}x'\n"]
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
            wide_integers(
                "a: 18446744073709551616\n-9223372036854775809: [+18446744073709551616]\n"
            ),
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
}
