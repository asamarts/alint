//! Literal `.env` parsing for [`crate::structured_format::Format::Dotenv`].
//!
//! Deliberately hand-rolled and dependency-free. A linter needs the value
//! exactly as written, but `dotenvy` expands `${VAR}` / `$VAR` unconditionally
//! (there is no toggle; `apply_substitution` is always called), which is
//! environment-dependent and wrong for static analysis. So this reads the raw
//! pairs into a flat `{ KEY: "value" }` object of strings, with no expansion.
//!
//! Kept self-contained (no alint-specific types) so it can be lifted into its
//! own crate later, and/or the no-substitution behavior contributed upstream to
//! `dotenvy`. See docs/design/format-coverage.md, section 5.3.
//!
//! ## Supported subset
//! - `KEY=VALUE`, one per line; the value is stringly-typed (never coerced).
//! - `#` full-line comments (after optional leading whitespace) and ` #` inline
//!   comments on *unquoted* values (a `#` with no leading space is part of the
//!   value).
//! - An optional `export ` prefix.
//! - Single-quoted values are fully literal. Double-quoted values honor the
//!   `\n \r \t \\ \"` escapes; every other backslash pair is kept verbatim.
//!   Neither quote style expands variables.
//! - Duplicate keys: last wins.
//!
//! Out of scope (a linter surfaces a parse error, one violation, like the other
//! formats): a line with no `=`, a key containing whitespace, and an
//! unterminated quote (multiline values are not supported).

use serde_json::{Map, Value};

/// Parse `.env` `text` into a flat `Value::Object` of string values, with no
/// variable expansion. Returns `Err(message)` on a malformed line.
pub(crate) fn parse(text: &str) -> Result<Value, String> {
    let mut map = Map::new();
    for (i, raw) in text.lines().enumerate() {
        let lineno = i + 1;
        let line = raw.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Optional `export ` prefix (bash-style); `export=x` (key named
        // `export`) is untouched because there is no following space.
        let line = line.strip_prefix("export ").map_or(line, str::trim_start);

        let Some(eq) = line.find('=') else {
            return Err(format!(
                "line {lineno}: expected `KEY=VALUE`, found `{}`",
                line.trim_end()
            ));
        };
        let key = line[..eq].trim_end();
        if key.is_empty() {
            return Err(format!("line {lineno}: empty key before `=`"));
        }
        if key.contains(char::is_whitespace) {
            return Err(format!("line {lineno}: key `{key}` contains whitespace"));
        }
        let value = parse_value(&line[eq + 1..], lineno)?;
        map.insert(key.to_string(), Value::String(value));
    }
    Ok(Value::Object(map))
}

fn parse_value(raw: &str, lineno: usize) -> Result<String, String> {
    let v = raw.trim_start();
    if let Some(rest) = v.strip_prefix('\'') {
        // Single-quoted: literal up to the next single quote. Anything after the
        // closing quote (whitespace / a comment) is ignored.
        rest.find('\'')
            .map(|end| rest[..end].to_string())
            .ok_or_else(|| format!("line {lineno}: unterminated single-quoted value"))
    } else if let Some(rest) = v.strip_prefix('"') {
        // Double-quoted: honor the common escapes, up to the next unescaped `"`.
        let mut out = String::new();
        let mut chars = rest.chars();
        loop {
            match chars.next() {
                None => return Err(format!("line {lineno}: unterminated double-quoted value")),
                Some('"') => return Ok(out),
                Some('\\') => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some('\\') => out.push('\\'),
                    Some('"') => out.push('"'),
                    // Unknown escape: keep both chars verbatim (no expansion).
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => return Err(format!("line {lineno}: trailing backslash in value")),
                },
                Some(c) => out.push(c),
            }
        }
    } else {
        // Unquoted: value runs to an inline ` #` comment (a `#` with no leading
        // space is part of the value), then trailing whitespace is trimmed.
        let end = v.find(" #").unwrap_or(v.len());
        Ok(v[..end].trim_end().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(text: &str) -> Value {
        parse(text).expect("parse dotenv")
    }

    #[test]
    fn basic_pairs_export_and_comments() {
        let v = obj("# a comment\n\
             NODE_ENV=production\n\
             export PORT=8080\n\
             \n\
             EMPTY=\n\
             INLINE=value # trailing comment\n\
             HASH=a#b\n");
        assert_eq!(v["NODE_ENV"], json!("production"));
        assert_eq!(v["PORT"], json!("8080"), "`export ` prefix is stripped");
        assert_eq!(v["EMPTY"], json!(""), "empty value is the empty string");
        assert_eq!(v["INLINE"], json!("value"), "` #` starts an inline comment");
        assert_eq!(
            v["HASH"],
            json!("a#b"),
            "`#` with no leading space is literal"
        );
    }

    #[test]
    fn no_variable_expansion() {
        // The whole reason for the hand-rolled parser: values stay literal.
        let v = obj("HOME_REF=${HOME}/bin\n\
             DOLLAR=$USER\n\
             DQ=\"${HOME}/x\"\n\
             SQ='${HOME}/y'\n");
        assert_eq!(v["HOME_REF"], json!("${HOME}/bin"));
        assert_eq!(v["DOLLAR"], json!("$USER"));
        assert_eq!(
            v["DQ"],
            json!("${HOME}/x"),
            "no expansion inside double quotes"
        );
        assert_eq!(
            v["SQ"],
            json!("${HOME}/y"),
            "no expansion inside single quotes"
        );
    }

    #[test]
    fn quotes_and_escapes() {
        let v = obj("SINGLE='literal # not a comment'\n\
             DOUBLE=\"a\\nb\\t\\\"c\\\"\"\n\
             SPACES=\"  padded  \"\n");
        assert_eq!(v["SINGLE"], json!("literal # not a comment"));
        assert_eq!(
            v["DOUBLE"],
            json!("a\nb\t\"c\""),
            "escapes honored in double quotes"
        );
        assert_eq!(
            v["SPACES"],
            json!("  padded  "),
            "quotes preserve inner whitespace"
        );
    }

    #[test]
    fn duplicate_keys_last_wins() {
        assert_eq!(obj("K=first\nK=second\n")["K"], json!("second"));
    }

    #[test]
    fn crlf_line_endings() {
        assert_eq!(obj("A=1\r\nB=2\r\n")["B"], json!("2"));
    }

    #[test]
    fn malformed_lines_are_errors() {
        assert!(
            parse("NOEQUALS\n").is_err(),
            "a line without `=` is a parse error"
        );
        assert!(
            parse("=novalue\n").is_err(),
            "an empty key is a parse error"
        );
        assert!(
            parse("A B=1\n").is_err(),
            "whitespace in a key is a parse error"
        );
        assert!(
            parse("Q=\"unterminated\n").is_err(),
            "an unterminated quote is a parse error (no multiline)"
        );
    }
}
