//! Literal `.env` parsing for [`crate::structured_format::Format::Dotenv`].
//!
//! A linter needs the value exactly as written, but `dotenvy` expands
//! `${VAR}` / `$VAR`, which is environment-dependent and wrong for static
//! analysis. The parser is the `adotenv` crate (extracted from alint into
//! asamarts/adotenv): literal values, byte spans, and in-place edits, which
//! `structured_fix` also uses. This module only maps its entries into the flat
//! `{ KEY: "value" }` object of strings the structured-query rules read.
//!
//! See the `adotenv` docs for the accepted syntax. In short: `KEY=VALUE` per
//! line, `#` comments (inline only after a space on unquoted values), an
//! optional `export ` prefix, single quotes fully literal, double quotes with
//! the `\n \r \t \\ \"` escapes, no variable expansion, last-wins duplicates,
//! and a leading BOM skipped. A line with no `=`, an empty or whitespace-
//! containing key, or an unterminated quote is a parse error (one violation,
//! like the other formats).
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

use serde_json::{Map, Value};

/// Parse `.env` `text` into a flat `Value::Object` of string values, with no
/// variable expansion. Returns `Err(message)` on a malformed line.
pub(crate) fn parse(text: &str) -> Result<Value, String> {
    let doc = adotenv::parse(text).map_err(|e| e.to_string())?;
    let mut map = Map::new();
    for entry in doc.entries() {
        map.insert(
            entry.key().to_string(),
            Value::String(entry.value().to_string()),
        );
    }
    Ok(Value::Object(map))
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
        // The whole reason for a literal parser: values stay literal.
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
