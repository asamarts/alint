//! The narrow checks of the allowlist: each reads a parser's error, often
//! with the input at the place the error points to, so that it names the
//! cause of the difference itself.

use super::neutralize::{Place, places};
use super::{Case, Rejection, Side};

pub(super) fn rejects(side: &Side, message: &str) -> bool {
    side.as_ref()
        .err()
        .is_some_and(|rejection| rejection.message.contains(message))
}

pub(super) fn oracle_rejects(case: &Case<'_>, messages: &[&str]) -> bool {
    messages
        .iter()
        .any(|message| rejects(&case.oracle, message))
}

pub(super) fn ayaml_rejects(case: &Case<'_>, messages: &[&str]) -> bool {
    messages.iter().any(|message| rejects(&case.ayaml, message))
}

/// Which parser counts a place: ayaml breaks lines at LF, CR and CRLF, as
/// YAML 1.2 does, and does not count a byte-order mark that starts the input
/// as a column; the oracle, which follows YAML 1.1, also breaks them at NEL,
/// LS and PS, and counts that mark.
#[derive(Clone, Copy)]
pub(super) enum Counter {
    Ayaml,
    Oracle,
}

impl Counter {
    /// The parser that gave `rejection`: ayaml's errors have a kind.
    fn of(rejection: &Rejection) -> Self {
        if rejection.kind.is_some() {
            Self::Ayaml
        } else {
            Self::Oracle
        }
    }

    /// The byte offsets where the lines of `input` start.
    pub(super) fn line_starts(self, input: &str) -> Vec<usize> {
        let mut starts = vec![0];
        for (at, ch) in input.char_indices() {
            let next = at + ch.len_utf8();
            let breaks = match ch {
                '\r' => !input[next..].starts_with('\n'),
                '\n' => true,
                '\u{85}' | '\u{2028}' | '\u{2029}' => matches!(self, Self::Oracle),
                _ => false,
            };
            if breaks {
                starts.push(next);
            }
        }
        starts
    }

    /// The lines of `input`, without their line breaks.
    pub(super) fn lines(self, input: &str) -> Vec<&str> {
        let starts = self.line_starts(input);
        starts
            .iter()
            .enumerate()
            .map(|(index, start)| {
                let end = starts.get(index + 1).map_or(input.len(), |next| {
                    next - input[..*next].chars().next_back().map_or(0, char::len_utf8)
                });
                input[*start..end].trim_end_matches('\r')
            })
            .collect()
    }
}

/// The byte offset of the one-based `line` and `column` (in characters) of
/// `input`, as `counter` counts them, when the place is in it or at its end.
pub(super) fn offset(input: &str, line: usize, column: usize, counter: Counter) -> Option<usize> {
    let mut start = *counter.line_starts(input).get(line.checked_sub(1)?)?;
    if line == 1 && matches!(counter, Counter::Ayaml) {
        start = input.len() - input.trim_start_matches('\u{feff}').len();
    }
    let text = &input[start..];
    text.char_indices()
        .map(|(at, _)| at)
        .chain([text.len()])
        .nth(column.checked_sub(1)?)
        .map(|at| start + at)
}

/// The byte offset where `side` reports its error, when it has a place.
pub(super) fn error_offset(input: &str, side: &Side) -> Option<usize> {
    match side {
        Err(
            rejection @ Rejection {
                location: Some((line, column)),
                ..
            },
        ) => offset(input, *line, *column, Counter::of(rejection)),
        _ => None,
    }
}

/// Whether ayaml parses `input`'s event stream with at most one document.
pub(super) fn ayaml_accepts_single_document(input: &str) -> bool {
    crate::yaml::stream_events(input).is_ok_and(|events| {
        events
            .iter()
            .filter(|event| matches!(event, ayaml::Event::DocumentStart { .. }))
            .count()
            <= 1
    })
}

/// The byte offset of ayaml's parser error on `input`'s event stream, if any.
pub(super) fn preexisting_ayaml_parser_offset(input: &str) -> Option<usize> {
    let error = crate::yaml::stream_events(input).err()?;
    (error.kind() == ayaml::ErrorKind::Parser)
        .then(|| error_offset(input, &Err(super::ayaml_rejection(&error))))
        .flatten()
}

/// The line of `input` where `side` reports its error, as its parser counts
/// lines, and the line's one-based number.
fn error_line<'a>(input: &'a str, side: &Side) -> Option<(usize, &'a str)> {
    match side {
        Err(
            rejection @ Rejection {
                location: Some((line, _)),
                ..
            },
        ) => {
            let text = Counter::of(rejection)
                .lines(input)
                .get(line.checked_sub(1)?)
                .copied()?;
            Some((*line, text))
        }
        _ => None,
    }
}

/// Whether the oracle's error points at a `#` inside a tag or a `%TAG`
/// prefix: the text before it on its line, back to white space or a flow
/// indicator, starts with `!`, or the `#` starts or is inside the third field
/// of a `%TAG` line.
pub(super) fn oracle_points_into_a_tag_at_a_number_sign(case: &Case<'_>) -> bool {
    let Err(Rejection {
        location: Some((_, column)),
        ..
    }) = &case.oracle
    else {
        return false;
    };
    let Some((_, text)) = error_line(case.input, &case.oracle) else {
        return false;
    };
    let before: String = text.chars().take(column.saturating_sub(1)).collect();
    let fields = before.split_whitespace().count();
    let prefix = text.starts_with("%TAG")
        && if before.ends_with(char::is_whitespace) {
            fields == 2
        } else {
            fields == 3
        };
    text.chars().nth(column.saturating_sub(1)) == Some('#')
        && (prefix
            || before
                .rsplit(|ch: char| ch.is_whitespace() || ",[]{}".contains(ch))
                .next()
                .is_some_and(|token| token.starts_with('!')))
}

/// White space as YAML reads it, line breaks included: not NEL, LS or PS.
fn yaml_white(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r')
}

/// The last character of `input` before byte `at` that is neither white
/// space nor in a quoted scalar, a comment or a block scalar, with its byte
/// offset.
fn structure_before(input: &str, places: &[Place], at: usize) -> Option<(usize, char)> {
    input[..at]
        .char_indices()
        .rev()
        .find(|&(offset, ch)| !yaml_white(ch) && !places[offset].literal)
}

/// Whether a node of a flow collection starts at byte `at`: `[`, `{`, `,`,
/// `?` or a `:` comes before it, or a property that comes after one of them,
/// as in `[&a :b]`.
pub(super) fn starts_a_flow_node(input: &str, places: &[Place], at: usize) -> bool {
    let Some((before, ch)) = structure_before(input, places, at) else {
        return false;
    };
    if "[{,?:".contains(ch) {
        return true;
    }
    let token = input[..before + ch.len_utf8()]
        .rfind(|ch: char| yaml_white(ch) || "[{,".contains(ch))
        .map_or(0, |offset| offset + 1);
    token < at && input[token..].starts_with(['&', '!']) && starts_a_flow_node(input, places, token)
}

/// Whether a `:` before byte `at`, in the flow entry `at` is in, starts a
/// node and a non-space follows it: YAML 1.2 reads `:a` there as a plain
/// scalar, and libyaml reads the `:` as a value indicator and rejects the
/// entry at that `:` or later, as at the second `:` of `{&a :b: c}`.
fn colon_scalar_before(input: &str, places: &[Place], at: usize) -> bool {
    input[..at].char_indices().any(|(colon, ch)| {
        let depth = places[colon].depth;
        ch == ':'
            && !places[colon].literal
            && depth > 0
            && input[colon + 1..]
                .chars()
                .next()
                .is_some_and(|next| !yaml_white(next) && !",[]{}".contains(next))
            && starts_a_flow_node(input, places, colon)
            && places[at].depth >= depth
            && !input[colon..at].char_indices().any(|(offset, ch)| {
                let place = places[colon + offset];
                ",]}".contains(ch) && !place.literal && place.depth == depth
            })
    })
}

/// Whether the oracle's error points at a YAML 1.2 flow form that libyaml
/// rejects, outside a quoted scalar, a comment and a block scalar: a `:` in
/// a flow collection that starts a node (`[:a]`, `[: x]`, `{a: :b}`,
/// `[&a :b]`), that a flow indicator or `?` follows (`[a:]`, `{a:? b}`),
/// which libyaml reads as the end of a plain scalar, or that ends a flow
/// mapping key that spans lines (`{a\nb: c}`); a later place in the entry of
/// a `:` that starts a node before a non-space (the second `:` of
/// `{&a :b: c}`). Only a line feed or a carriage return breaks a line here,
/// as for ayaml: a key that NEL, LS or PS breaks, for the oracle, is not
/// this form. A lone `?` (`[? ]`) is `empty-flow-explicit-key`'s.
pub(super) fn oracle_points_at_a_flow_form(case: &Case<'_>) -> bool {
    error_offset(case.input, &case.oracle).is_some_and(|at| flow_form_at(case.input, at))
}

/// Whether byte `at` of `input` is where libyaml rejects a YAML 1.2 flow
/// form (see [`oracle_points_at_a_flow_form`]).
pub(super) fn flow_form_at(input: &str, at: usize) -> bool {
    let places = places(input);
    let (Some(place), Some(rest)) = (places.get(at), input.get(at..)) else {
        return false;
    };
    if rest.starts_with(':') && !place.literal && place.depth > 0 {
        let closes = input[at + 1..].starts_with([',', '[', ']', '{', '}', '?']);
        // The key runs from the last `,`, `[` or `{` of the collection the
        // `:` is in.
        let start = input[..at]
            .char_indices()
            .rev()
            .find(|&(offset, ch)| {
                matches!(ch, ',' | '[' | '{')
                    && !places[offset].literal
                    && places[offset + 1].depth == place.depth
            })
            .map_or(0, |(offset, _)| offset + 1);
        let spans = input[start..at]
            .char_indices()
            .skip_while(|&(offset, ch)| yaml_white(ch) || places[start + offset].literal)
            .any(|(_, ch)| matches!(ch, '\n' | '\r'));
        if starts_a_flow_node(input, &places, at) || closes || spans {
            return true;
        }
    }
    colon_scalar_before(input, &places, at)
}

/// Whether `input` has a `:` that starts a node of a flow collection (see
/// [`starts_a_flow_node`]) directly before a quote, whose quoted scalar, as
/// libyaml reads it, reaches byte `end`: YAML 1.2 reads `[:'q]` as the plain
/// scalar `:'q`, and libyaml as an empty key, a `:` and a quoted scalar
/// that runs to the end. A `:` in a quoted scalar, a comment or a block
/// scalar is not one.
pub(super) fn flow_colon_before_a_quote(input: &str, end: usize) -> bool {
    let places = places(input);
    let end = end.min(input.len());
    input[..end].char_indices().any(|(at, ch)| {
        ch == ':'
            && !places[at].literal
            && places[at].depth > 0
            && input[at + 1..].starts_with(['\'', '"'])
            && starts_a_flow_node(input, &places, at)
            && quoted_scalar_reaches(input, at + 1, end)
    })
}

/// Whether the quoted scalar that the quote at byte `open` starts holds
/// every byte up to `end`: no closing quote comes before it.
fn quoted_scalar_reaches(input: &str, open: usize, end: usize) -> bool {
    let Some(quote) = input[open..].chars().next() else {
        return false;
    };
    let mut chars = input.get(open + 1..end).unwrap_or("").chars().peekable();
    while let Some(ch) = chars.next() {
        if quote == '"' && ch == '\\' {
            chars.next();
        } else if ch == quote {
            if quote == '\'' && chars.peek() == Some(&'\'') {
                chars.next();
            } else {
                return false;
            }
        }
    }
    true
}

/// Whether the oracle rejects inside a quoted scalar that a `:` opening a
/// flow entry or value comes directly before (see
/// [`flow_colon_before_a_quote`]).
pub(super) fn oracle_rejects_in_a_quote_after_a_flow_colon(case: &Case<'_>) -> bool {
    error_offset(case.input, &case.oracle)
        .is_some_and(|end| flow_colon_before_a_quote(case.input, end))
}

/// Whether byte `at` of `input` holds a flow entry or value that is a lone
/// `-`, `?` or `:` before a flow indicator, as in `[-]` or `{a: -, b}`.
pub(super) fn lone_flow_indicator_at(input: &str, at: usize) -> bool {
    let chars: Vec<(usize, char)> = input.char_indices().collect();
    let Some(index) = chars.iter().position(|(offset, _)| *offset == at) else {
        return false;
    };
    let ch = chars[index].1;
    let next = chars.get(index + 1).map(|(_, ch)| *ch);
    let lone = "-?:".contains(ch)
        && (index == 0 || {
            let before = chars[index - 1].1;
            before.is_whitespace() || "[{,".contains(before)
        })
        && next.is_some_and(|after| after.is_whitespace() || "]},".contains(after));
    let before = chars[..index]
        .iter()
        .rposition(|(_, ch)| !ch.is_whitespace());
    let after = chars[index + 1..]
        .iter()
        .find(|(_, ch)| !ch.is_whitespace());
    lone && after.is_some_and(|(_, after)| "]},".contains(*after))
        && before.is_some_and(|before| {
            "[{,".contains(chars[before].1)
                || (chars[before].1 == ':' && chars[before + 1].1.is_whitespace())
        })
}

/// Whether ayaml's error points at a lone flow indicator.
pub(super) fn ayaml_points_at_a_lone_flow_indicator(case: &Case<'_>) -> bool {
    error_offset(case.input, &case.ayaml).is_some_and(|at| lone_flow_indicator_at(case.input, at))
}

/// Whether the flow collection at byte `at` follows a `:` that follows a
/// property and white space, as `[b]` in `[&a :[b]]`.
pub(super) fn flow_value_after_a_property_at(input: &str, at: usize) -> bool {
    // The character before `at` may be wider than a byte.
    let Some(before) = input[..at].strip_suffix(':') else {
        return false;
    };
    input[at..].starts_with(['[', '{'])
        && before.ends_with([' ', '\t'])
        && before
            .trim_end()
            .rsplit(|ch: char| ch.is_whitespace() || ",[{".contains(ch))
            .next()
            .is_some_and(|token| token.starts_with(['&', '!']))
}

/// Whether ayaml's error points at a flow collection directly after a `:`
/// that follows a property and white space.
pub(super) fn ayaml_points_at_a_flow_value_after_a_property(case: &Case<'_>) -> bool {
    error_offset(case.input, &case.ayaml)
        .is_some_and(|at| flow_value_after_a_property_at(case.input, at))
}

/// Whether a line of white space with a tab, or with a tab before a
/// comment, follows a `...` line.
pub(super) fn has_tab_line_after_document_end(input: &str) -> bool {
    input
        .split(['\n', '\r'])
        .skip_while(|line| !line.starts_with("..."))
        .any(|line| {
            let rest = line.trim_start();
            line[..line.len() - rest.len()].contains('\t')
                && (rest.is_empty() || rest.starts_with('#'))
        })
}

/// Whether the document's root node is a block scalar: the first line with
/// content, past directives, `---` and the root's properties, starts with `|`
/// or `>`.
pub(super) fn root_is_block_scalar(input: &str) -> bool {
    let lines = input
        .split(['\n', '\r'])
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

/// Whether ayaml's error is on a `%YAML` line where a `#` directly follows
/// a character other than white space.
pub(super) fn ayaml_points_at_a_yaml_directive_with_an_adjacent_number_sign(
    case: &Case<'_>,
) -> bool {
    error_line(case.input, &case.ayaml).is_some_and(|(_, line)| {
        line.starts_with("%YAML")
            && line
                .char_indices()
                .any(|(at, ch)| ch == '#' && !line[..at].ends_with(char::is_whitespace))
    })
}

/// The oracle's error about a character its reader does not allow.
pub(super) const ORACLE_READER_ERROR: &str = "control characters are not allowed";

/// Whether the oracle's error is about the character ayaml rejects: the
/// first its reader does not allow, which it reports at line 1, column 1.
/// Or ayaml's is about the first C0 control, other than a tab or a line
/// break, which its reader rejects anywhere before its scanner rejects a
/// character YAML allows only in a quoted scalar, outside one: the input
/// then shows nothing of how ayaml reads such a character before it, about
/// which the oracle's error may be. Otherwise the oracle's error about an
/// earlier character, which ayaml keeps in a quoted scalar, is another.
pub(super) fn errors_about_one_character(case: &Case<'_>) -> bool {
    let Some(ayaml) = error_offset(case.input, &case.ayaml) else {
        return false;
    };
    let control = |ch: char| ch < ' ' && !matches!(ch, '\t' | '\n' | '\r');
    match super::oracle_error_offset(case.input, &case.oracle) {
        Some(oracle) if oracle == ayaml => true,
        // The oracle's reader refuses that control too, so its error, if
        // it is the reader's, is then before ayaml's.
        Some(_) => {
            rejects(&case.oracle, ORACLE_READER_ERROR) && case.input.find(control) == Some(ayaml)
        }
        None => false,
    }
}

/// Whether both parsers place their errors at different ends of white space
/// and comments alone, the oracle first: where an empty node would be, and
/// at the start of what follows it.
pub(super) fn errors_around_an_empty_node(case: &Case<'_>) -> bool {
    let (Some(oracle), Some(ayaml)) = (
        error_offset(case.input, &case.oracle),
        error_offset(case.input, &case.ayaml),
    ) else {
        return false;
    };
    // The oracle's error about a character is at line 1, column 1, not
    // where an empty node would be (see [`super::allowlist::locate_case`]).
    if rejects(&case.oracle, ORACLE_READER_ERROR) {
        return false;
    }
    oracle < ayaml
        && case.input[oracle..ayaml].split(['\n', '\r']).all(|line| {
            let line = line.trim_start();
            line.is_empty() || line.starts_with('#')
        })
}

/// A verbatim tag that names no tag.
pub(super) const NAMELESS_VERBATIM: &str = "!<!>";

/// ayaml's error about a verbatim tag that names no tag, or nothing.
pub(super) const VERBATIM_NEEDS_A_NAME: &str = "a verbatim tag needs a name";

/// Whether ayaml rejects the input at a verbatim tag that names no tag,
/// `!<!>`, which the oracle reads as the non-specific tag `!`. Not at `!<>`,
/// which names nothing, with the same message: both parsers reject that
/// (see [`errors_in_an_empty_verbatim_tag`]).
pub(super) fn ayaml_rejects_a_nameless_verbatim_tag(case: &Case<'_>) -> bool {
    ayaml_rejects(case, &[VERBATIM_NEEDS_A_NAME])
        && error_offset(case.input, &case.ayaml)
            .is_some_and(|at| case.input[at..].starts_with(NAMELESS_VERBATIM))
}

/// Whether ayaml's error is at a verbatim tag that names nothing, `!<>`,
/// and the oracle's at its `>`, where it looks for the name.
pub(super) fn errors_in_an_empty_verbatim_tag(case: &Case<'_>) -> bool {
    let (Some(ayaml), Some(oracle)) = (
        error_offset(case.input, &case.ayaml),
        error_offset(case.input, &case.oracle),
    ) else {
        return false;
    };
    case.input[ayaml..].starts_with("!<>") && oracle == ayaml + 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml_differential::{Class, Entry};

    #[test]
    fn offsets_count_characters_from_one() {
        for counter in [Counter::Ayaml, Counter::Oracle] {
            assert_eq!(offset("ab\néc\n", 2, 2, counter), Some(5));
            assert_eq!(offset("ab\n", 2, 1, counter), Some(3));
            assert_eq!(offset("ab", 1, 3, counter), Some(2));
            assert_eq!(offset("ab", 3, 1, counter), None);
            assert_eq!(offset("ab", 1, 0, counter), None);
            // A lone CR and a CRLF each end a line.
            assert_eq!(offset("a\rb\r\nc", 3, 1, counter), Some(5));
            assert_eq!(counter.lines("a\rb\r\nc\n"), ["a", "b", "c", ""]);
        }
        // The oracle breaks lines at NEL, LS and PS, and counts a leading
        // byte-order mark as a column.
        assert_eq!(offset("a\u{85}b", 2, 1, Counter::Oracle), Some(3));
        assert_eq!(offset("a\u{85}b", 1, 3, Counter::Ayaml), Some(3));
        assert_eq!(Counter::Oracle.lines("a\u{2028}b"), ["a", "b"]);
        assert_eq!(offset("\u{feff}ab", 1, 2, Counter::Oracle), Some(3));
        assert_eq!(offset("\u{feff}ab", 1, 2, Counter::Ayaml), Some(4));
        assert_eq!(offset("\u{feff}\u{feff}ab", 1, 2, Counter::Oracle), Some(3));
        assert_eq!(offset("\u{feff}\u{feff}ab", 1, 2, Counter::Ayaml), Some(7));
    }

    #[test]
    fn flow_predicates_match_only_their_forms() {
        for input in [
            "[:'q,]\n",
            "{a: 1, :\"x}\n",
            "- {key: :'q,}\n",
            "[:'a''b]\n",
        ] {
            assert!(flow_colon_before_a_quote(input, input.len()), "{input:?}");
        }
        for other in [
            "a: 'q'\n",
            "[a:'q']\n",
            "[: 'q']\n",
            // In a comment or a quoted scalar, or closed before the end.
            "a: 'x'\n# [:'\n",
            "a: '[:'\n",
            "[:'q', 'r\n",
        ] {
            assert!(!flow_colon_before_a_quote(other, other.len()), "{other:?}");
        }
        // Where libyaml rejects each flow form, and other places.
        for (input, at) in [
            ("[:a]\n", 1),
            ("{a: :b}\n", 4),
            ("{a: 1,\n :b}\n", 8),
            ("[a:]\n", 2),
            ("{a:? b}\n", 2),
            ("{a:[b]}\n", 2),
            ("{a\nb: c}\n", 4),
            ("{\"a\n b\": c}\n", 7),
            ("{[a,\n b]: c}\n", 8),
            ("[&a :b]\n", 4),
            ("{&a :b: c}\n", 6),
            ("{!t &a :b c: d}\n", 11),
        ] {
            assert!(flow_form_at(input, at), "{input:?} at {at}");
        }
        for (input, at) in [
            // A long key, and keys that NEL, LS or PS breaks.
            ("{kkk: 1}\n", 4),
            ("[a\u{2028}: b]\n", 5),
            ("{a\u{2028}b: c}\n", 6),
            ("[\u{2028}: b]\n", 4),
            // In a comment, a quoted scalar, or not in a flow collection.
            ("[a, # :b\n c]\n", 7),
            ("['x :a']\n", 4),
            (":a\n", 0),
            ("{a: 1,\n b: c}\n", 9),
            ("[a, ? b]\n", 7),
            // A lone `?`, which another entry rewrites.
            ("[? ]\n", 5),
            ("[? , a]\n", 5),
            // A `:` inside a plain scalar, or in an entry before this one.
            ("{a :b: c}\n", 5),
            ("{:a, b c: d}\n", 8),
        ] {
            assert!(!flow_form_at(input, at), "{input:?} at {at}");
        }
        for (input, at) in [
            ("[-]", 1),
            ("{a: -, b}", 4),
            ("[a, ? ]", 4),
            ("[\n  -\n]", 4),
        ] {
            assert!(lone_flow_indicator_at(input, at), "{input:?}");
        }
        for (input, at) in [("[-1]", 1), ("[a-]", 2), ("{a: -1}", 4), ("{a:-, b}", 3)] {
            assert!(!lone_flow_indicator_at(input, at), "{input:?}");
        }
        assert!(flow_value_after_a_property_at("[&a :[b]]", 5));
        assert!(flow_value_after_a_property_at("{x: 1, !t :{b: c}}", 11));
        for (input, at) in [
            ("{a:[b]}", 3),
            ("[&a : [b]]", 6),
            ("[&a, :[b]]", 6),
            ("a: &x:[b]", 6),
        ] {
            assert!(!flow_value_after_a_property_at(input, at), "{input:?}");
        }
    }

    #[test]
    fn layout_predicates_match_only_their_forms() {
        assert!(has_tab_line_after_document_end("a\n...\n\t\n"));
        assert!(
            !has_tab_line_after_document_end("a\n\t\n...\n")
                && !has_tab_line_after_document_end("a\n...\nb\t\n")
        );
        assert!(root_is_block_scalar("|\nx\n") && root_is_block_scalar("--- !t &a\n>-\n x\n"));
        assert!(root_is_block_scalar("%YAML 1.2\n# c\n--- |\nx\n"));
        assert!(root_is_block_scalar("&b  # c\n>\nx\n"));
        assert!(!root_is_block_scalar("a: |\n x\n") && !root_is_block_scalar("- |\n x\n"));
        assert!(has_tab_line_after_document_end("a: 1\n...\n\t# c\n"));
        assert!(has_tab_line_after_document_end("- a\r\n...\r\t\n"));
        assert!(!has_tab_line_after_document_end("a: 1\n...\n# c\t\n"));
    }

    fn rejected_at(input: &str, oracle: (usize, usize), ayaml: (usize, usize)) -> Case<'_> {
        let rejection = |location| {
            Err(Rejection {
                message: "e".to_string(),
                location: Some(location),
                kind: None,
            })
        };
        Case {
            input,
            entry: Entry::Config,
            rule: 0,
            class: Class::RejectedAtDifferentLocation,
            ayaml: rejection(ayaml),
            oracle: rejection(oracle),
        }
    }

    #[test]
    fn errors_around_an_empty_node_span_only_blanks_and_comments() {
        assert!(errors_around_an_empty_node(&rejected_at(
            "version: \n",
            (1, 9),
            (2, 1)
        )));
        assert!(errors_around_an_empty_node(&rejected_at(
            "a: # c\n\n  # d\nb: 1\n",
            (1, 3),
            (4, 1)
        )));
        assert!(!errors_around_an_empty_node(&rejected_at(
            "a: x\nb: 1\n",
            (1, 3),
            (2, 1)
        )));
        assert!(!errors_around_an_empty_node(&rejected_at(
            "a: \n",
            (2, 1),
            (1, 3)
        )));
        // The oracle's reader error is at line 1, column 1 wherever the
        // character it refuses is, here in a comment.
        let mut reader = rejected_at("# \u{7f}\n", (1, 1), (2, 1));
        if let Err(oracle) = &mut reader.oracle {
            oracle.message = ORACLE_READER_ERROR.to_string();
        }
        assert!(!errors_around_an_empty_node(&reader));
    }

    /// ayaml's error at `!<>` and the oracle's at its `>`, two columns on,
    /// are about one tag; not elsewhere, nor at `!<!>`.
    #[test]
    fn errors_in_an_empty_verbatim_tag_are_two_columns_apart() {
        assert!(errors_in_an_empty_verbatim_tag(&rejected_at(
            "a: !<> x\n",
            (1, 6),
            (1, 4)
        )));
        assert!(!errors_in_an_empty_verbatim_tag(&rejected_at(
            "a: !<> x\n",
            (1, 7),
            (1, 4)
        )));
        assert!(!errors_in_an_empty_verbatim_tag(&rejected_at(
            "a: !<!> x\n",
            (1, 6),
            (1, 4)
        )));
    }

    fn oracle_error_at(input: &str, line: usize, column: usize) -> Case<'_> {
        Case {
            input,
            entry: Entry::Value,
            rule: 0,
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
        assert!(oracle_points_into_a_tag_at_a_number_sign(&oracle_error_at(
            "%TAG !e! tag:x,2000a#b:\n---\n[1]\n",
            1,
            21
        )));
        assert!(oracle_points_into_a_tag_at_a_number_sign(&oracle_error_at(
            "%TAG !e! #tag:x,2000:\n---\n[1]\n",
            1,
            10
        )));
        assert!(!oracle_points_into_a_tag_at_a_number_sign(
            &oracle_error_at("%TAG !e! tag:x,2000: #c\n---\n[1]\n", 1, 22)
        ));
        assert!(!oracle_points_into_a_tag_at_a_number_sign(
            &oracle_error_at("%TAG #e! x\n---\n[1]\n", 1, 6)
        ));
        for (input, line, column) in [
            ("- !t #x\n", 1, 6),
            ("- a# x\n", 1, 4),
            ("- !tx x\n", 1, 5),
            ("!t#\n", 3, 1),
        ] {
            assert!(
                !oracle_points_into_a_tag_at_a_number_sign(&oracle_error_at(input, line, column)),
                "{input:?}"
            );
        }
    }
}
