//! Literal `.env` parsing for [`crate::structured_format::Format::Dotenv`].
//!
//! A linter needs the value exactly as written, but `dotenvy` expands
//! `${VAR}` / `$VAR`, which is environment-dependent and wrong for static
//! analysis. The parser is the `adotenv` crate (extracted from alint into
//! asamarts/adotenv): literal values, diagnostics, exact spans, and verified
//! native edits, which `structured_fix` also uses. This module maps its entries
//! into the flat object consumed by structured-query rules.
//!
//! See the `adotenv` docs for the accepted Portable syntax. In short: ASCII
//! keys, optional bare declarations, `KEY=VALUE` assignments, `#` comments
//! (inline only after whitespace on unquoted values), an optional `export`
//! prefix, multiline single or double quotes, standard double-quote escapes,
//! no variable expansion, last-wins duplicates, and a retained leading BOM.
//! Bare declarations map to JSON `null`; every assigned value maps to a string.
//! Error diagnostics reject the document as one parse failure, while portability
//! warnings do not.
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

/// Bound retained source, decoded escape maps and large keys within a line.
/// The generic structured cap is too loose for this lossless representation.
pub(crate) const MAX_DOTENV_BYTES: usize = 4 * 1024 * 1024;

/// Bound lossless items and per-item metadata independently of the input-byte cap.
/// Short repeated declarations can otherwise retain hundreds of bytes per line.
pub(crate) const MAX_PHYSICAL_LINES: usize = 100_000;

/// Portable `.env` dialect with multi-BOM stripping handled natively by `adotenv`.
pub(crate) const DIALECT: adotenv::Dialect =
    adotenv::Dialect::PORTABLE.with_bom(adotenv::BomPolicy::StripAll);

/// Native `adotenv` input limits applied to both read-only and lossless parses.
pub(crate) const PARSE_OPTIONS: adotenv::ParseOptions = adotenv::ParseOptions::new()
    .with_max_input_bytes(MAX_DOTENV_BYTES)
    .with_max_lines(MAX_PHYSICAL_LINES);

/// Native `adotenv` output and verification limits for structured edits.
pub(crate) const EDIT_OPTIONS: adotenv::EditOptions = adotenv::EditOptions::new()
    .with_max_output_bytes(MAX_DOTENV_BYTES)
    .with_max_output_lines(MAX_PHYSICAL_LINES)
    .with_parse_limits(PARSE_OPTIONS);

fn format_parse_error(error: &adotenv::Diagnostic) -> String {
    if let Some(limit) = error.limit() {
        return match limit.kind() {
            adotenv::LimitKind::InputBytes => format!(
                "dotenv input exceeds the maximum supported size ({} bytes)",
                limit.limit()
            ),
            adotenv::LimitKind::Lines => format!(
                "dotenv input exceeds the maximum supported physical line count ({})",
                limit.limit()
            ),
            _ => error.to_string(),
        };
    }
    error.to_string()
}

/// Construct the shared lossless document with native byte/line budgets and
/// multi-BOM stripping; every emitted span uses original `text` byte offsets.
pub(crate) fn parse_document(text: &str) -> Result<adotenv::Document<'_>, String> {
    adotenv::parse_strict_with_options(text, DIALECT, PARSE_OPTIONS)
        .map_err(|error| format_parse_error(&error))
}

/// Parse `.env` `text` into a flat `Value::Object`, with no variable
/// expansion, using `adotenv`'s compact bare-preserving read-only parser.
/// Assigned values are strings and bare declarations are `null`.
/// Returns the first error-severity diagnostic on malformed input; portability
/// warnings remain accepted.
pub(crate) fn parse(text: &str) -> Result<Value, String> {
    let entries = adotenv::parse_entries_compact_with_options(text, DIALECT, PARSE_OPTIONS)
        .map_err(|error| format_parse_error(&error))?;
    let mut map = Map::with_capacity(entries.len());
    for (key, value) in entries.iter() {
        map.insert(
            key.to_owned(),
            value.map_or(Value::Null, |value| Value::String(value.to_owned())),
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
    fn byte_budget_includes_a_single_physical_line() {
        let value = "x".repeat(MAX_DOTENV_BYTES - 2);
        let mut source = format!("A={value}");
        assert_eq!(obj(&source)["A"], json!(value));
        source.push('#');
        assert!(parse(&source).is_err());
    }

    #[test]
    fn multi_bom_and_parse_options_preserve_original_spans() {
        let src = "\u{feff}\u{feff}\u{feff}PORT=8080\nBARE\n";
        let value = obj(src);
        assert_eq!(value["PORT"], json!("8080"));
        assert_eq!(value["BARE"], Value::Null);
        let doc = parse_document(src).expect("parse multi-bom document");
        assert_eq!(doc.bom_span(), Some(0..9));
        let port = doc.unique("PORT").unwrap();
        assert_eq!(&src[port.key_span()], "PORT");
        assert_eq!(&src[port.value_span().unwrap()], "8080");
    }

    #[test]
    fn lossless_document_line_budget_handles_every_line_ending() {
        for ending in ["\n", "\r\n", "\r"] {
            let mut source = format!(
                "{}A=kept",
                format!("#{ending}").repeat(MAX_PHYSICAL_LINES - 1)
            );
            assert_eq!(obj(&source)["A"], json!("kept"));
            source.push_str(ending);
            assert_eq!(obj(&source)["A"], json!("kept"));
            source.push('#');
            assert!(parse(&source).is_err(), "over budget with {ending:?}");
        }
    }

    #[test]
    fn repeated_bare_declarations_cannot_bypass_the_lossless_budget() {
        assert!(parse(&"A\n".repeat(MAX_PHYSICAL_LINES + 1)).is_err());
    }

    #[test]
    fn basic_pairs_export_and_comments() {
        let v = obj("# a comment\n\
             NODE_ENV=production\n\
             export PORT=8080\n\
             export\tHOST=localhost\n\
             \n\
             EMPTY=\n\
             INLINE=value # trailing comment\n\
             TAB_COMMENT=value\t# trailing comment\n\
             HASH=a#b\n\
             UNICODE_WS=\u{a0}x\u{a0}\n");
        assert_eq!(v["NODE_ENV"], json!("production"));
        assert_eq!(v["PORT"], json!("8080"), "`export ` prefix is stripped");
        assert_eq!(
            v["HOST"],
            json!("localhost"),
            "`export\\t` is also a prefix"
        );
        assert_eq!(v["EMPTY"], json!(""), "empty value is the empty string");
        assert_eq!(v["INLINE"], json!("value"), "` #` starts an inline comment");
        assert_eq!(
            v["TAB_COMMENT"],
            json!("value"),
            "`\\t#` also starts an inline comment"
        );
        assert_eq!(
            v["HASH"],
            json!("a#b"),
            "`#` with no leading space is literal"
        );
        assert_eq!(
            v["UNICODE_WS"],
            json!("\u{a0}x\u{a0}"),
            "non-ASCII whitespace is value data"
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
    fn duplicate_keys_last_wins_including_bare_declarations() {
        assert_eq!(obj("K=first\nK=second\n")["K"], json!("second"));
        assert_eq!(obj("K=assigned\nK\n")["K"], Value::Null);
    }

    #[test]
    fn crlf_and_lone_cr_line_endings() {
        assert_eq!(obj("A=1\r\nB=2\r\n")["B"], json!("2"));
        assert_eq!(
            obj("A=1\rB=2\r")["B"],
            json!("2"),
            "lone CR is accepted with a portability warning"
        );
    }

    #[test]
    fn bare_declarations_are_null_and_malformed_lines_are_errors() {
        assert_eq!(obj("NOEQUALS\n")["NOEQUALS"], Value::Null);
        assert!(
            parse("=novalue\n").is_err(),
            "an empty key is a parse error"
        );
        assert!(
            parse("A B=1\n").is_err(),
            "whitespace in a key is a parse error"
        );
        assert!(parse("CAFÉ=1\n").is_err(), "Portable keys are ASCII");
        assert!(
            parse("Q=\"unterminated\n").is_err(),
            "an unmatched quote is a parse error"
        );
        assert!(
            parse("Q=\"value\"tail\n").is_err(),
            "text after a closing quote is a parse error"
        );
    }

    #[test]
    fn multiline_quotes_and_portability_warnings_are_accepted() {
        let value = obj("1KEY=portable-warning\nMULTI=\"one\ntwo\"\n");
        assert_eq!(value["1KEY"], json!("portable-warning"));
        assert_eq!(value["MULTI"], json!("one\ntwo"));
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
    fn double_quote_cr_backslash_dollar_and_unknown_escapes() {
        let v = obj("CR=\"a\\rb\"\nBS=\"a\\\\b\"\nDOLLAR=\"a\\$b\"\n\
             BACKSLASH_DOLLAR=\"a\\\\$b\"\nUNK=\"a\\qb\"\n");
        assert_eq!(v["CR"], json!("a\rb"), "\\r escape");
        assert_eq!(v["BS"], json!("a\\b"), "\\\\ escape");
        assert_eq!(v["DOLLAR"], json!("a$b"), "\\$ decodes to a literal dollar");
        assert_eq!(
            v["BACKSLASH_DOLLAR"],
            json!("a\\$b"),
            "\\\\ retains the backslash before a dollar"
        );
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
