use super::WhenError;

// ─── Lexer ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(super) enum Tok {
    Bool(bool),
    Null,
    Int(i64),
    Str(String),
    Ident(String),
    Dot,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Eq2,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    KwAnd,
    KwOr,
    KwNot,
    KwIn,
    KwMatches,
}

#[allow(clippy::too_many_lines)]
pub(super) fn lex(src: &str) -> Result<Vec<(Tok, usize)>, WhenError> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        // whitespace
        if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
            i += 1;
            continue;
        }
        let start = i;
        match c {
            b'.' => {
                out.push((Tok::Dot, start));
                i += 1;
            }
            b'(' => {
                out.push((Tok::LParen, start));
                i += 1;
            }
            b')' => {
                out.push((Tok::RParen, start));
                i += 1;
            }
            b'[' => {
                out.push((Tok::LBracket, start));
                i += 1;
            }
            b']' => {
                out.push((Tok::RBracket, start));
                i += 1;
            }
            b',' => {
                out.push((Tok::Comma, start));
                i += 1;
            }
            b'=' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    out.push((Tok::Eq2, start));
                    i += 2;
                } else {
                    return Err(WhenError::Parse {
                        pos: start,
                        message: "expected '==' (bare '=' is not an operator)".into(),
                    });
                }
            }
            b'!' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    out.push((Tok::Ne, start));
                    i += 2;
                } else {
                    return Err(WhenError::Parse {
                        pos: start,
                        message: "expected '!=' (use 'not' for logical negation)".into(),
                    });
                }
            }
            b'<' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    out.push((Tok::Le, start));
                    i += 2;
                } else {
                    out.push((Tok::Lt, start));
                    i += 1;
                }
            }
            b'>' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    out.push((Tok::Ge, start));
                    i += 2;
                } else {
                    out.push((Tok::Gt, start));
                    i += 1;
                }
            }
            b'"' | b'\'' => {
                let quote = c;
                i += 1;
                let mut s = String::new();
                while i < bytes.len() && bytes[i] != quote {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        let esc = bytes[i + 1];
                        let ch = match esc {
                            b'n' => '\n',
                            b't' => '\t',
                            b'r' => '\r',
                            b'\\' => '\\',
                            b'"' => '"',
                            b'\'' => '\'',
                            _ => {
                                return Err(WhenError::Parse {
                                    pos: i,
                                    message: format!(
                                        "unknown escape \\{} in string literal",
                                        esc as char,
                                    ),
                                });
                            }
                        };
                        s.push(ch);
                        i += 2;
                    } else {
                        // Decode the full UTF-8 scalar rather than casting one
                        // byte to `char` (which interprets a multi-byte char as
                        // Latin-1 mojibake, so a non-ASCII `when:` literal like
                        // `== "café"` could never match) (L3). `src` is valid
                        // UTF-8 and `i` sits on a char boundary here (every
                        // branch advances by whole chars / ASCII bytes).
                        let ch = src[i..]
                            .chars()
                            .next()
                            .expect("i < len and on a UTF-8 boundary");
                        s.push(ch);
                        i += ch.len_utf8();
                    }
                }
                if i >= bytes.len() {
                    return Err(WhenError::Parse {
                        pos: start,
                        message: "unterminated string literal".into(),
                    });
                }
                i += 1;
                out.push((Tok::Str(s), start));
            }
            c if c.is_ascii_digit() => {
                let mut j = i;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                let num = std::str::from_utf8(&bytes[i..j])
                    .unwrap()
                    .parse::<i64>()
                    .map_err(|e| WhenError::Parse {
                        pos: start,
                        message: format!("invalid integer: {e}"),
                    })?;
                out.push((Tok::Int(num), start));
                i = j;
            }
            c if is_ident_start(c) => {
                let mut j = i;
                while j < bytes.len() && is_ident_cont(bytes[j]) {
                    j += 1;
                }
                let word = &src[i..j];
                let tok = match word {
                    "true" => Tok::Bool(true),
                    "false" => Tok::Bool(false),
                    "null" => Tok::Null,
                    "and" => Tok::KwAnd,
                    "or" => Tok::KwOr,
                    "not" => Tok::KwNot,
                    "in" => Tok::KwIn,
                    "matches" => Tok::KwMatches,
                    _ => Tok::Ident(word.to_string()),
                };
                out.push((tok, start));
                i = j;
            }
            _ => {
                return Err(WhenError::Parse {
                    pos: start,
                    message: format!("unexpected character {:?}", c as char),
                });
            }
        }
    }
    Ok(out)
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_ident_cont(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Closed list of methods callable on `iter`. Adding new ones is
/// a deliberate API extension — typos in user configs surface as
/// "unknown iter method" rather than silently coercing to false.
pub(super) fn is_known_iter_method(name: &str) -> bool {
    matches!(name, "has_file")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_str(src: &str) -> String {
        lex(src)
            .unwrap()
            .into_iter()
            .find_map(|(t, _)| match t {
                Tok::Str(s) => Some(s),
                _ => None,
            })
            .expect("a string token")
    }

    #[test]
    fn non_ascii_string_literal_lexes_as_utf8() {
        // L3: a multi-byte char must lex as one scalar, not Latin-1 mojibake
        // (the old `byte as char` produced "cafÃ©", so `== "café"` never matched).
        assert_eq!(first_str("\"café\""), "café");
        assert_eq!(first_str("\"naïve Москва\""), "naïve Москва");
    }

    #[test]
    fn emoji_string_literal_lexes_as_utf8() {
        assert_eq!(first_str("\"a🎉b\""), "a🎉b");
    }

    #[test]
    fn escapes_still_work_alongside_unicode() {
        assert_eq!(first_str("\"café\\n\""), "café\n");
    }
}
