//! Dependency-free, span-native Java `.properties` parser for
//! [`crate::structured_format::Format::Properties`].
//!
//! Faithful to `java.util.Properties.load(Reader)` over UTF-8:
//! - Leading UTF-8 BOMs (`\u{FEFF}`) are stripped while preserving original byte
//!   offsets in every emitted span.
//! - Physical line terminators are `\r\n`, bare `\r`, and `\n`.
//! - Blank lines and full-line `#` / `!` comments (after optional `' '`, `'\t'`,
//!   or `'\x0C'` whitespace) are skipped; comments never continue across lines.
//! - Logical lines join across an unescaped trailing `\` followed by a line
//!   terminator, consuming leading `' '`, `'\t'`, or `'\x0C'` on each
//!   continuation line. A lone trailing continuation `\` at EOF is stripped and
//!   the entry is retained (matching `OpenJDK` `Properties.LineReader`).
//! - Keys terminate at the first unescaped `'='`, `':'`, `' '`, `'\t'`, or
//!   `'\x0C'`. Dotted keys (`a.b.c`) stay one opaque key.
//! - Escapes: `\t`, `\r`, `\n`, `\f`, `\uXXXX` (including UTF-16 surrogate pairs
//!   `\uD800..\uDBFF` + `\uDC00..\uDFFF`), and `\<other>` -> `<other>`.
//!   Malformed `\u` escapes and unpaired surrogates are parse errors.
//! - Leading value whitespace is stripped; trailing value whitespace is kept.
//! - Duplicate keys follow last-wins semantics in [`parse`], while
//!   [`parse_entries`] retains every entry with exact source byte spans for
//!   extraction and structured fixes.

use std::ops::Range;

use serde_json::{Map, Value};

/// One parsed logical entry from a `.properties` document with exact byte spans
/// in the original input (including any stripped leading UTF-8 BOMs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PropertyEntry {
    pub(crate) key: String,
    pub(crate) value: String,
    /// Full logical line span, from the start of its first physical line through
    /// the trailing line terminator of its final physical line.
    pub(crate) full_span: Range<usize>,
    /// Raw source span of the key token.
    pub(crate) key_span: Range<usize>,
    /// Raw source span of the value token (after separator and leading value
    /// whitespace, through the end of the final physical line's content).
    pub(crate) value_span: Range<usize>,
    /// Whether an explicit (`=` / `:`) or whitespace separator appeared after
    /// the key. False only for a bare separator-less key (`flag\n`), where
    /// splicing a value at `value_span` would fuse into the key token.
    pub(crate) has_separator: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PropertiesError {
    Parse(String),
    Limit { limit: usize },
}

impl std::fmt::Display for PropertiesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(msg) => f.write_str(msg),
            Self::Limit { limit } => {
                write!(
                    f,
                    "properties document exceeds maximum entry limit ({limit})"
                )
            }
        }
    }
}

const fn is_prop_ws(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | 0x0c)
}

fn physical_line(bytes: &[u8], start: usize) -> (usize, usize) {
    let mut end = start;
    while end < bytes.len() && !matches!(bytes[end], b'\r' | b'\n') {
        end += 1;
    }
    let next = if bytes.get(end) == Some(&b'\r') && bytes.get(end + 1) == Some(&b'\n') {
        end + 2
    } else if end < bytes.len() {
        end + 1
    } else {
        end
    };
    (end, next)
}

struct LogicalCursor<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    end: usize,
    line: usize,
}

impl<'a> LogicalCursor<'a> {
    fn new(src: &'a str, start: usize, end: usize, line: usize) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            pos: start,
            end,
            line,
        }
    }

    /// Consume any backslash line continuations (and a lone trailing
    /// continuation backslash at the end of the logical line) at `self.pos`.
    fn skip_continuations(&mut self) {
        while self.pos < self.end && self.bytes[self.pos] == b'\\' {
            if self.pos + 1 == self.end {
                // Lone trailing continuation backslash at end of logical line
                // (EOF or before final EOF newline): strip it per OpenJDK.
                self.pos = self.end;
                break;
            }
            match self.bytes[self.pos + 1] {
                b'\r' => {
                    self.pos += 2;
                    if self.pos < self.end && self.bytes[self.pos] == b'\n' {
                        self.pos += 1;
                    }
                    self.line += 1;
                    while self.pos < self.end && is_prop_ws(self.bytes[self.pos]) {
                        self.pos += 1;
                    }
                }
                b'\n' => {
                    self.pos += 2;
                    self.line += 1;
                    while self.pos < self.end && is_prop_ws(self.bytes[self.pos]) {
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
    }

    fn next_logical_byte(&mut self) -> Option<u8> {
        self.skip_continuations();
        if self.pos >= self.end {
            return None;
        }
        let b = self.bytes[self.pos];
        self.pos += 1;
        Some(b)
    }

    fn next_char(&mut self) -> Option<char> {
        if self.pos >= self.end {
            return None;
        }
        let ch = self.src[self.pos..self.end].chars().next()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn parse_hex4(&mut self, escape_line: usize) -> Result<u16, String> {
        let mut value = 0u16;
        for _ in 0..4 {
            let Some(byte) = self.next_logical_byte() else {
                return Err(format!(
                    "truncated \\uXXXX unicode escape at line {escape_line}"
                ));
            };
            let digit = match byte {
                b'0'..=b'9' => u16::from(byte - b'0'),
                b'a'..=b'f' => u16::from(byte - b'a' + 10),
                b'A'..=b'F' => u16::from(byte - b'A' + 10),
                _ => {
                    return Err(format!(
                        "invalid \\uXXXX unicode escape at line {escape_line}"
                    ));
                }
            };
            value = (value << 4) | digit;
        }
        Ok(value)
    }

    /// Decode an escape sequence starting at `self.pos` (caller has already
    /// called [`Self::skip_continuations`] and verified `bytes[pos] == b'\\'`).
    fn decode_escape(&mut self) -> Result<char, String> {
        let escape_line = self.line;
        self.pos += 1; // consume '\'
        let ch = self
            .next_char()
            .ok_or_else(|| format!("incomplete escape sequence at line {escape_line}"))?;
        match ch {
            't' => Ok('\t'),
            'r' => Ok('\r'),
            'n' => Ok('\n'),
            'f' => Ok('\x0c'),
            'u' => {
                let first = self.parse_hex4(escape_line)?;
                if (0xD800..=0xDBFF).contains(&first) {
                    // High surrogate: must be immediately followed by a low
                    // surrogate `\uDC00..\uDFFF` (possibly across a continuation).
                    self.skip_continuations();
                    if self.pos + 1 < self.end
                        && self.bytes[self.pos] == b'\\'
                        && self.bytes[self.pos + 1] == b'u'
                    {
                        self.pos += 2;
                        let second = self.parse_hex4(escape_line)?;
                        if (0xDC00..=0xDFFF).contains(&second) {
                            let scalar = 0x10000
                                + ((u32::from(first - 0xD800) << 10) | u32::from(second - 0xDC00));
                            return char::from_u32(scalar).ok_or_else(|| {
                                format!(
                                    "invalid UTF-16 surrogate pair \\u{first:04X}\\u{second:04X} at line {escape_line}"
                                )
                            });
                        }
                        return Err(format!(
                            "unpaired UTF-16 high surrogate \\u{first:04X} followed by \\u{second:04X} at line {escape_line}"
                        ));
                    }
                    return Err(format!(
                        "unpaired UTF-16 high surrogate \\u{first:04X} at line {escape_line}"
                    ));
                }
                if (0xDC00..=0xDFFF).contains(&first) {
                    return Err(format!(
                        "unpaired UTF-16 low surrogate \\u{first:04X} at line {escape_line}"
                    ));
                }
                char::from_u32(u32::from(first)).ok_or_else(|| {
                    format!("invalid unicode scalar \\u{first:04X} at line {escape_line}")
                })
            }
            other => Ok(other),
        }
    }
}

fn parse_logical_entry(
    body: &str,
    base: usize,
    line_start: usize,
    first: usize,
    content_end: usize,
    next_at: usize,
    entry_line: usize,
) -> Result<Option<PropertyEntry>, PropertiesError> {
    let bytes = body.as_bytes();
    let mut cursor = LogicalCursor::new(body, first, content_end, entry_line);
    cursor.skip_continuations();
    let key_start = cursor.pos;
    let mut key_end = key_start;
    let mut key = String::new();
    let mut saw_key_char = false;

    loop {
        cursor.skip_continuations();
        if cursor.pos >= cursor.end {
            break;
        }
        match bytes[cursor.pos] {
            b'=' | b':' | b' ' | b'\t' | 0x0c => break,
            b'\\' => {
                let ch = cursor.decode_escape().map_err(PropertiesError::Parse)?;
                key.push(ch);
                key_end = cursor.pos;
                saw_key_char = true;
            }
            _ => {
                if let Some(ch) = cursor.next_char() {
                    key.push(ch);
                    key_end = cursor.pos;
                    saw_key_char = true;
                }
            }
        }
    }

    let mut has_separator_ws = false;
    loop {
        cursor.skip_continuations();
        if cursor.pos < cursor.end && is_prop_ws(bytes[cursor.pos]) {
            has_separator_ws = true;
            cursor.pos += 1;
        } else {
            break;
        }
    }

    let mut has_explicit_sep = false;
    cursor.skip_continuations();
    if cursor.pos < cursor.end && matches!(bytes[cursor.pos], b'=' | b':') {
        has_explicit_sep = true;
        cursor.pos += 1;
        loop {
            cursor.skip_continuations();
            if cursor.pos < cursor.end && is_prop_ws(bytes[cursor.pos]) {
                cursor.pos += 1;
            } else {
                break;
            }
        }
    }

    // A line containing only whitespace and a lone continuation backslash
    // produces no logical key or separator -> skip as blank.
    if !saw_key_char && !has_explicit_sep {
        return Ok(None);
    }

    cursor.skip_continuations();
    let value_start = cursor.pos;
    let mut value = String::new();
    loop {
        cursor.skip_continuations();
        if cursor.pos >= cursor.end {
            break;
        }
        if bytes[cursor.pos] == b'\\' {
            let ch = cursor.decode_escape().map_err(PropertiesError::Parse)?;
            value.push(ch);
        } else if let Some(ch) = cursor.next_char() {
            value.push(ch);
        }
    }

    let value_end = content_end.max(value_start);
    Ok(Some(PropertyEntry {
        key,
        value,
        full_span: (base + line_start)..(base + next_at),
        key_span: (base + key_start)..(base + key_end),
        value_span: (base + value_start)..(base + value_end),
        has_separator: has_explicit_sep || has_separator_ws,
    }))
}

/// Parse Java `.properties` entries with exact source byte spans and an
/// optional maximum entry count.
pub(crate) fn parse_entries(
    text: &str,
    max_entries: Option<usize>,
) -> Result<Vec<PropertyEntry>, PropertiesError> {
    if text.len() > crate::structured_format::MAX_STRUCTURED_BYTES {
        return Err(PropertiesError::Parse(format!(
            "input exceeds the maximum supported size for a structured document ({} bytes)",
            crate::structured_format::MAX_STRUCTURED_BYTES
        )));
    }
    let body = text.trim_start_matches('\u{feff}');
    let base = text.len() - body.len();
    let bytes = body.as_bytes();
    let mut entries = Vec::new();
    let mut at = 0usize;
    let mut line_no = 1usize;

    while at < bytes.len() {
        let (phys_end, phys_next) = physical_line(bytes, at);
        let mut first = at;
        while first < phys_end && is_prop_ws(bytes[first]) {
            first += 1;
        }
        // Blank lines and full-line `#` / `!` comments are always one physical
        // line: a trailing backslash inside a comment never continues.
        if first == phys_end || matches!(bytes[first], b'#' | b'!') {
            line_no += 1;
            at = phys_next;
            continue;
        }

        let line_start = at;
        let entry_line = line_no;
        let mut cur_start = at;
        let (content_end, next_at) = loop {
            let (pe, pn) = physical_line(bytes, cur_start);
            let trailing_backslashes = bytes[cur_start..pe]
                .iter()
                .rev()
                .take_while(|&&b| b == b'\\')
                .count();
            line_no += 1;
            if trailing_backslashes % 2 == 0 || pn == bytes.len() {
                break (pe, pn);
            }
            cur_start = pn;
        };

        if let Some(entry) = parse_logical_entry(
            body,
            base,
            line_start,
            first,
            content_end,
            next_at,
            entry_line,
        )? {
            if let Some(limit) = max_entries
                && entries.len() >= limit
            {
                return Err(PropertiesError::Limit { limit });
            }
            entries.push(entry);
        }
        at = next_at;
    }

    Ok(entries)
}

/// Parse Java `.properties` `text` into a flat `Value::Object` of literal
/// strings (last-wins on duplicate keys).
pub(crate) fn parse(text: &str) -> Result<Value, String> {
    let entries = parse_entries(text, None).map_err(|e| e.to_string())?;
    let mut map = Map::with_capacity(entries.len());
    for entry in entries {
        map.insert(entry.key, Value::String(entry.value));
    }
    Ok(Value::Object(map))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_separators_escapes_utf8_and_bom() {
        let src = "\u{feff}\u{feff}# cmt\n! bang\n\
                   a.b=one\n\
                   c.d : two\n\
                   e.f   three with spaces  \n\
                   esc\\:key\\=1 = a\\tb\\nc\\rd\\fe\\\\g\\z\n\
                   bare\n\
                   =empty_key\n\
                   utf8=café 日本語\n";
        let v = parse(src).unwrap();
        assert_eq!(v["a.b"], json!("one"));
        assert_eq!(v["c.d"], json!("two"));
        assert_eq!(v["e.f"], json!("three with spaces  "));
        assert_eq!(v["esc:key=1"], json!("a\tb\nc\rd\x0ce\\gz"));
        assert_eq!(v["bare"], json!(""));
        assert_eq!(v[""], json!("empty_key"));
        assert_eq!(v["utf8"], json!("café 日本語"));
    }

    #[test]
    fn decodes_bmp_and_utf16_surrogate_pair_unicode_escapes() {
        let src = "bmp=caf\\u00e9\n\
                   emoji=\\uD83D\\uDE00\n\
                   split_emoji=\\uD83D\\\n  \\uDE00\n\
                   literal_bs=\\\\u0041\n";
        let v = parse(src).unwrap();
        assert_eq!(v["bmp"], json!("café"));
        assert_eq!(v["emoji"], json!("😀"));
        assert_eq!(v["split_emoji"], json!("😀"));
        assert_eq!(v["literal_bs"], json!("\\u0041"));
    }

    #[test]
    fn rejects_invalid_unicode_escapes_and_unpaired_surrogates() {
        for bad in [
            "k=\\uZZZZ\n",
            "k=\\u123\n",
            "k=\\uD83D\n",
            "k=\\uD83Dplain\n",
            "k=\\uD83D\\u0041\n",
            "k=\\uDE00\n",
            "\\uD83D=bad_key\n",
        ] {
            assert!(parse(bad).is_err(), "must reject {bad:?}");
        }
    }

    #[test]
    fn retains_entries_ending_with_trailing_eof_backslash() {
        assert_eq!(
            parse("kept=ok\neof=value\\").unwrap()["eof"],
            json!("value")
        );
        assert_eq!(
            parse("kept=ok\neof=first\\\n  second\\").unwrap()["eof"],
            json!("firstsecond")
        );
        assert_eq!(
            parse("kept=ok\nesc_bs=value\\\\").unwrap()["esc_bs"],
            json!("value\\")
        );
        assert_eq!(
            parse("kept=ok\nthree_bs=value\\\\\\").unwrap()["three_bs"],
            json!("value\\")
        );
        assert_eq!(
            parse("kept=ok\neof_nl=value\\\n").unwrap()["eof_nl"],
            json!("value")
        );
        // A line containing only whitespace and a trailing continuation backslash is blank.
        assert_eq!(parse("   \\\n").unwrap(), json!({}));
    }

    #[test]
    fn continuations_preserve_hash_and_bang_and_comments_never_continue() {
        let src = concat!(
            "# comment \\\n",
            "after_comment=ok\n",
            "cont=a\\\n",
            "  #not_a_comment\\\r\n",
            "  !also_value\n",
        );
        let entries = parse_entries(src, None).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "after_comment");
        assert_eq!(entries[0].value, "ok");
        assert_eq!(entries[1].key, "cont");
        assert_eq!(entries[1].value, "a#not_a_comment!also_value");
        assert_eq!(
            &src[entries[1].full_span.clone()],
            "cont=a\\\n  #not_a_comment\\\r\n  !also_value\n"
        );
        assert_eq!(&src[entries[1].key_span.clone()], "cont");
        assert_eq!(
            &src[entries[1].value_span.clone()],
            "a\\\n  #not_a_comment\\\r\n  !also_value"
        );
    }

    #[test]
    fn spans_stay_aligned_with_bom_cr_crlf_and_limit() {
        let src = "\u{feff}a=1\rb=2\r\nc\\:d = val\\\n  2\n";
        let entries = parse_entries(src, None).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(&src[entries[0].full_span.clone()], "a=1\r");
        assert_eq!(&src[entries[0].value_span.clone()], "1");
        assert_eq!(&src[entries[1].full_span.clone()], "b=2\r\n");
        assert_eq!(&src[entries[1].value_span.clone()], "2");
        assert_eq!(entries[2].key, "c:d");
        assert_eq!(entries[2].value, "val2");
        assert_eq!(&src[entries[2].key_span.clone()], "c\\:d");
        assert_eq!(&src[entries[2].value_span.clone()], "val\\\n  2");
        assert_eq!(
            parse_entries(src, Some(2)).unwrap_err(),
            PropertiesError::Limit { limit: 2 }
        );
    }
}
