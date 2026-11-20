//! `strip_jsonc` reads JSONC in one pass into one copy of the text, where
//! it once read it in two, holding the text four times over as characters
//! between them. What it gives must not change, since every check of a
//! JSONC file parses what it gives: these tests hold it to the two passes
//! it replaced, kept here as [`two_passes`], over JSONC that exercises each
//! rule (comments of both kinds, markers inside strings, escapes, trailing
//! commas before comments and line breaks, a BOM, CRLF, and whitespace
//! outside ASCII), and over many texts drawn from those pieces.

use super::*;

/// The `strip_jsonc` alint shipped through v0.22, unchanged.
fn two_passes(src: &str) -> String {
    // Pass 1: remove comments.
    let mut decommented = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            decommented.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    decommented.push(n);
                }
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                decommented.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        decommented.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
            }
            _ => decommented.push(c),
        }
    }
    // Pass 2: drop trailing commas (`,` then whitespace then `]`/`}`).
    let cs: Vec<char> = decommented.chars().collect();
    let mut out = String::with_capacity(cs.len());
    let mut in_string = false;
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if in_string {
            out.push(c);
            if c == '\\' {
                i += 1;
                if i < cs.len() {
                    out.push(cs[i]);
                }
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == ',' {
            let mut j = i + 1;
            while j < cs.len() && cs[j].is_whitespace() {
                j += 1;
            }
            if j < cs.len() && (cs[j] == ']' || cs[j] == '}') {
                // Drop the comma; keep the intervening whitespace.
                out.extend(&cs[i + 1..j]);
                i = j;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Texts that exercise each rule of the stripping, one at a time.
const EDGES: &[&str] = &[
    "",
    "{}",
    "{\"a\": 1,}",
    "[1, 2,]",
    "[1, 2 , ]",
    "[1, // c\n]",
    "[1, // c\r\n]",
    "[1, /* c */ ]",
    "[1, /* c */ 2]",
    "[1,/**/]",
    "[1,,]",
    "[1, , ]",
    "[1,\u{a0}]",
    "[1,\u{2028}]",
    "[1,\u{3000}\t\n ]",
    "[1,\u{feff}]",
    "[1,\r\n]",
    "{\"url\": \"https://x/y\", \"note\": \"a,b\", \"c\": \"/* not */\",}",
    "{\"a\": \"\\\"// still a string\"}",
    "{\"a\": \"\\\\\"} // a comment",
    "{\"a\": \"\\\\\\\"\",}",
    "{\"a\": \"x,\"}",
    "{\"a\": \",]\"}",
    "/*/ x */1",
    "/* a */ /* b */ [/* c */1/* d */,/* e */]/* f */",
    "/**/",
    "/* unterminated [1,]",
    "// only a comment",
    "[1, // eof",
    "\"unterminated, ]",
    "\"escape at the end\\",
    "[1/2]",
    "[1,/]",
    "[1,/",
    "\u{feff}{\"a\": 1,}",
    "{\"a\": [1,],}",
    "[\"é\",]",
    "[\"𝄞\", /* 𝄞 */ ]",
    "{\n  // line comment\n  \"a\": 1, /* block */\n  \"b\": [1, 2,],\n}\n",
    "{\r\n  \"a\": 1, // c\r\n  \"b\": 2,\r\n}\r\n",
];

/// Pieces of JSONC, each a rule's marker or what sits around one.
const PIECES: &[&str] = &[
    "\"", "\\", "/", "*", "//", "/*", "*/", "\n", "\r\n", "\r", ",", "]", "}", "[", "{", " ", "\t",
    "a", "1", ":", "\u{a0}", "\u{2028}", "\u{feff}", "é", "𝄞", "\\\"", "\"a\"", ",]", ", }",
    "// c\n", "/* c */",
];

#[test]
fn one_pass_strips_jsonc_as_the_two_passes_did() {
    for text in EDGES {
        assert_eq!(strip_jsonc(text), two_passes(text), "{text:?}");
    }
    // Texts of up to 24 pieces, drawn by a fixed xorshift sequence, so
    // every run reads the same ones.
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        usize::try_from(state % 1_000_003).expect("small")
    };
    let mut text = String::new();
    for _ in 0..300_000 {
        text.clear();
        for _ in 0..next() % 25 {
            text.push_str(PIECES[next() % PIECES.len()]);
        }
        assert_eq!(strip_jsonc(&text), two_passes(&text), "{text:?}");
    }
}

/// JSONC files parse to what they parsed to before, comments and
/// trailing commas dropped, and a text that is not JSONC still fails
/// with the strict parser's error.
#[test]
fn jsonc_parses_as_before() {
    for text in EDGES {
        let before =
            serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).or_else(|strict| {
                serde_json::from_str::<Value>(&two_passes(text.trim_start_matches('\u{feff}')))
                    .map_err(|_| strict.to_string())
            });
        let empty = text
            .trim_matches(|c: char| c == '\u{feff}' || c.is_whitespace())
            .is_empty();
        if !empty {
            assert_eq!(Format::Json.parse(text), before, "{text:?}");
        }
    }
}
