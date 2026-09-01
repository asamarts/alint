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
//! - A leading UTF-8 BOM is tolerated (stripped before parsing).
//!
//! ## Notes
//! - Detection is filename-based and best-effort: `.env` and `.env.*` (so
//!   `.env.local` / `.env.production` / `.env.example`, but also `.env.bak` and
//!   other suffixes). Pass an explicit `format: dotenv` for other names like
//!   `app.env`, and keep backup files out via `.gitignore` / an `exclude:` glob.
//! - Values are literal here, but `cross_file` value relations still skip
//!   interpolation-looking values (`${VAR}` / `$(...)`) via the shared
//!   non-literal filter -- use a `dotenv_path_*` rule to compare a literal
//!   `${VAR}` value.
//!
//! Out of scope (a linter surfaces a parse error, one violation, like the other
//! formats): a line with no `=`, a key containing whitespace, and an
//! unterminated quote (multiline values are not supported; lines split on
//! LF / CRLF only, not a lone classic-Mac `\r`).

use serde_json::{Map, Value};

/// Parse `.env` `text` into a flat `Value::Object` of string values, with no
/// variable expansion. Returns `Err(message)` on a malformed line.
pub(crate) fn parse(text: &str) -> Result<Value, String> {
    // Tolerate a leading UTF-8 BOM (some Windows editors add one). Without this
    // the first key parses as `\u{feff}KEY` -- a silent mis-parse, since
    // `str::trim_start` does NOT strip U+FEFF (it is not White_Space).
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
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
        // Unquoted: an inline comment starts at ` #` (space-hash); a `#` with no
        // leading space is part of the value. Detect it on `raw`, NOT the
        // left-trimmed `v`, so `KEY= # c` (an empty value + a comment) reads as
        // "" rather than the value `# c`. Value whitespace is insignificant.
        let end = raw.find(" #").unwrap_or(raw.len());
        Ok(raw[..end].trim().to_string())
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

    #[test]
    fn bom_is_stripped_from_the_first_key() {
        // A leading UTF-8 BOM must not corrupt the first key (a silent mis-parse:
        // the key would be `\u{feff}NODE_ENV`, so `$.NODE_ENV` would miss it).
        let v = obj("\u{feff}NODE_ENV=production\nPORT=8080\n");
        assert_eq!(v["NODE_ENV"], json!("production"), "BOM stripped");
        assert_eq!(v.as_object().unwrap().len(), 2);
    }

    #[test]
    fn equals_inside_a_value_is_kept() {
        // Split on the FIRST `=`, so a connection string with `=` in its query
        // survives verbatim (a very common real-world shape).
        let v = obj("DATABASE_URL=postgres://u:p@h/db?sslmode=require\n");
        assert_eq!(
            v["DATABASE_URL"],
            json!("postgres://u:p@h/db?sslmode=require")
        );
    }

    #[test]
    fn spaces_around_equals() {
        let v = obj("KEY = value\nPADDED =  x \n");
        assert_eq!(v["KEY"], json!("value"));
        assert_eq!(v["PADDED"], json!("x"), "value whitespace is trimmed");
    }

    #[test]
    fn double_quote_cr_backslash_and_unknown_escapes() {
        let v = obj("CR=\"a\\rb\"\nBS=\"a\\\\b\"\nUNK=\"a\\qb\"\n");
        assert_eq!(v["CR"], json!("a\rb"), "\\r escape");
        assert_eq!(v["BS"], json!("a\\b"), "\\\\ escape");
        assert_eq!(v["UNK"], json!("a\\qb"), "unknown escape kept verbatim");
    }

    #[test]
    fn export_is_a_prefix_only_with_a_following_space() {
        // `export=x` is a key literally named `export` (no trailing space).
        assert_eq!(obj("export=x\n")["export"], json!("x"));
    }

    #[test]
    fn unterminated_single_quote_and_trailing_backslash_are_errors() {
        assert!(
            parse("Q='unterminated\n").is_err(),
            "unterminated single quote"
        );
        assert!(
            parse("B=\"trailing\\").is_err(),
            "a trailing backslash in a double-quoted value is an error"
        );
    }

    #[test]
    fn empty_value_with_inline_comment() {
        // `KEY= # c` is an empty value plus a comment, NOT the value `# c`; the
        // ` #` boundary must be found on the raw value, before left-trimming (a
        // "secret must be non-empty" check would otherwise pass on a placeholder).
        let v = obj("SECRET= # TODO fill before prod\nOK=x # keep\n");
        assert_eq!(
            v["SECRET"],
            json!(""),
            "empty value + comment reads as empty"
        );
        assert_eq!(
            v["OK"],
            json!("x"),
            "value + comment still strips the comment"
        );
    }
}
