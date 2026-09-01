//! Literal INI / `.cfg` parsing for [`crate::structured_format::Format::Ini`].
//!
//! Deliberately hand-rolled and dependency-free, like the sibling [`crate::dotenv`]
//! parser. The maintained crate (`rust-ini`) is capable, but it drags in a
//! six-crate transitive tree (`ordered-multimap` -> `dlv-list` -> `const-random`
//! -> `tiny-keccak`, the last CC0-1.0 and outside alint's cargo-deny allow-list)
//! purely to preserve key order -- a poor trade for a supply-chain-conscious
//! linter when the subset alint needs is small. See docs/design/format-coverage.md,
//! section 5.4.
//!
//! ## The `Value` shape (2-level section map)
//! An INI file maps to `{ <global-key>: "v", <section>: { <key>: "v" } }`:
//! - **Global (pre-section) keys hoist to the top level**, so `.editorconfig`'s
//!   `root = true` is `$.root` and `php.ini`'s `memory_limit` is `$.memory_limit`.
//! - A `[section]` is an object, so `[server] host = x` is `$.server.host`
//!   (bracket-notation `$['server']['host']` for a section or key with a dot).
//! - **Section names and keys are case-preserving** (the reason to hand-roll
//!   rather than use `configparser`, which lowercases keys).
//!
//! ## Supported subset
//! - `key = value` or `key : value`, one per line; split on the EARLIEST `=`/`:`
//!   (so `url = http://h` keeps `http://h`, and `dsn : k=v` keeps `k=v`).
//! - Values are stringly-typed and **literal**: surrounding quotes, if any, are
//!   part of the value; `${...}` placeholders are kept verbatim; an inline `;`/`#`
//!   is part of the value (only a FULL-LINE `;`/`#`, after optional leading
//!   whitespace, is a comment). Leading/trailing value whitespace is trimmed.
//! - **Duplicate keys within one scope collapse to a document-order array** (like
//!   the XML mapping's repeated-element rule). INI has no single duplicate-key
//!   runtime semantic (configparser errors; Windows `GetPrivateProfileString`
//!   takes the first), so alint surfaces every value rather than silently dropping
//!   one. A single occurrence stays a string.
//! - **A repeated `[section]` header merges** into the existing object.
//! - A leading UTF-8 BOM is tolerated (stripped before parsing).
//!
//! ## Out of scope (a linter surfaces a parse error, one violation, like the
//! other formats)
//! - An unclosed section header (`[server` with no `]`), and a header with
//!   trailing content (`[server] ; note`): a header line must be exactly
//!   `[name]`. Put the comment on its own line.
//! - A non-blank line that is neither a comment, a section header, nor a
//!   `key=value` (no `=`/`:`), and an empty key before the separator.
//! - A `[section]` whose name equals an existing top-level (global) key: the
//!   projection would be ambiguous, so it is rejected rather than silently
//!   overwriting.
//! - Indentation-continued multi-line values (a configparser feature): an
//!   indented line is parsed on its own (leading whitespace is not significant).

use serde_json::{Map, Value};

/// Parse INI `text` into a 2-level `Value::Object` of literal string values.
/// Returns `Err(message)` on a malformed line (surfaced as one per-file
/// parse-error violation, like the other formats).
pub(crate) fn parse(text: &str) -> Result<Value, String> {
    // Tolerate a leading UTF-8 BOM (some Windows editors add one). Without this
    // the first key parses as `\u{feff}key` -- a silent mis-parse, since
    // `str::trim` does NOT strip U+FEFF (it is not White_Space).
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut root = Map::new();
    // The current section name; `None` is the global (pre-section) scope, whose
    // keys live at the top level of `root`.
    let mut section: Option<String> = None;
    for (i, raw) in text.lines().enumerate() {
        let lineno = i + 1;
        let line = raw.trim();
        // Blank lines and full-line `;` / `#` comments (after optional leading
        // whitespace, already removed by the trim above).
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        // Section header: `[name]` on its own line.
        if let Some(rest) = line.strip_prefix('[') {
            let Some(name) = rest.strip_suffix(']') else {
                return Err(format!(
                    "line {lineno}: unclosed section header `{line}` (expected `[name]`)"
                ));
            };
            let name = name.trim().to_string();
            match root.get(&name) {
                // A repeated `[section]` merges into the existing object.
                Some(Value::Object(_)) => {}
                // The name is already a global key (string/array): ambiguous.
                Some(_) => {
                    return Err(format!(
                        "line {lineno}: section `[{name}]` conflicts with a \
                         top-level key of the same name"
                    ));
                }
                None => {
                    root.insert(name.clone(), Value::Object(Map::new()));
                }
            }
            section = Some(name);
            continue;
        }
        // `key = value` / `key : value`: split on the earliest separator so a
        // value containing the other separator (a URL's `:`, a query's `=`)
        // survives verbatim.
        let Some(idx) = line.find(['=', ':']) else {
            return Err(format!(
                "line {lineno}: expected `key = value` (or `key : value`), found `{line}`"
            ));
        };
        let key = line[..idx].trim();
        if key.is_empty() {
            return Err(format!(
                "line {lineno}: empty key before `{}`",
                &line[idx..=idx]
            ));
        }
        let value = line[idx + 1..].trim().to_string();
        // The top level for a global key, else the section object (materialized
        // when its header line was read, so the `expect` cannot fire).
        let scope = match &section {
            None => &mut root,
            Some(s) => root
                .get_mut(s)
                .and_then(Value::as_object_mut)
                .expect("section object is materialized on its header line"),
        };
        insert_dup_aware(scope, key.to_string(), value);
    }
    Ok(Value::Object(root))
}

/// Insert `key` -> `value`, collapsing a key repeated within one scope into a
/// document-order array (the XML mapping's repeated-element rule). A single
/// occurrence stays a bare string.
fn insert_dup_aware(scope: &mut Map<String, Value>, key: String, value: String) {
    match scope.get_mut(&key) {
        Some(Value::Array(arr)) => arr.push(Value::String(value)),
        Some(slot) => {
            let prev = slot.take();
            *slot = Value::Array(vec![prev, Value::String(value)]);
        }
        None => {
            scope.insert(key, Value::String(value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(text: &str) -> Value {
        parse(text).expect("parse ini")
    }

    #[test]
    fn globals_hoist_sections_nest_both_separators_and_comments() {
        let v = obj("; a comment\n\
             # another comment\n\
             root = true\n\
             \n\
             [server]\n\
             host = localhost\n\
             port : 8080\n");
        // Pre-section keys live at the top level.
        assert_eq!(v["root"], json!("true"), "global key hoisted");
        // A section is a nested object; both `=` and `:` separate.
        assert_eq!(v["server"]["host"], json!("localhost"), "`=` separator");
        assert_eq!(v["server"]["port"], json!("8080"), "`:` separator");
        // `;` and `#` full-line comments are excluded.
        assert!(!v.as_object().unwrap().contains_key("; a comment"));
    }

    #[test]
    fn keys_and_sections_are_case_preserving() {
        // The reason to hand-roll rather than use configparser (which lowercases).
        let v = obj("[Server]\nMaxConnections = 100\n");
        assert_eq!(v["Server"]["MaxConnections"], json!("100"));
        assert!(v.get("server").is_none(), "section case preserved");
        assert!(
            v["Server"].get("maxconnections").is_none(),
            "key case preserved"
        );
    }

    #[test]
    fn values_are_literal() {
        // No quote-stripping, no escape processing, no inline-comment stripping,
        // no variable expansion: a linter reports exactly what is in the file.
        let v = obj("[x]\n\
             quoted = \"keep quotes\"\n\
             placeholder = ${HOME}/bin\n\
             inline = value ; not a comment\n\
             hashish = a # b\n\
             backslash = C:\\path\\n\n");
        assert_eq!(v["x"]["quoted"], json!("\"keep quotes\""), "quotes kept");
        assert_eq!(v["x"]["placeholder"], json!("${HOME}/bin"), "no expansion");
        assert_eq!(
            v["x"]["inline"],
            json!("value ; not a comment"),
            "inline `;` is literal"
        );
        assert_eq!(v["x"]["hashish"], json!("a # b"), "inline `#` is literal");
        assert_eq!(
            v["x"]["backslash"],
            json!("C:\\path\\n"),
            "backslashes are literal (no escape processing)"
        );
    }

    #[test]
    fn earliest_separator_wins_so_urls_survive() {
        let v = obj("[net]\n\
             url = http://h:9000/p?a=1\n\
             dsn : driver=pg;host=h\n");
        // `=` at `url =` is earlier than the `:` in `http:` -> value keeps the URL.
        assert_eq!(v["net"]["url"], json!("http://h:9000/p?a=1"));
        // `:` at `dsn :` is earlier than the `=`s -> value keeps the `=`s.
        assert_eq!(v["net"]["dsn"], json!("driver=pg;host=h"));
    }

    #[test]
    fn duplicate_keys_collapse_to_a_document_order_array() {
        // INI has no single duplicate-key semantic, so alint surfaces every value
        // (a duplicated key is often the very misconfiguration a rule looks for).
        let v = obj("[server]\nlisten = 80\nlisten = 443\nlisten = 8080\n");
        assert_eq!(
            v["server"]["listen"],
            json!(["80", "443", "8080"]),
            "three values -> array in file order"
        );
        // A single occurrence stays a bare string.
        let single = obj("[server]\nlisten = 80\n");
        assert_eq!(single["server"]["listen"], json!("80"));
    }

    #[test]
    fn duplicate_global_keys_also_array() {
        let v = obj("k = 1\nk = 2\n");
        assert_eq!(v["k"], json!(["1", "2"]));
    }

    #[test]
    fn repeated_section_headers_merge() {
        // Two `[db]` blocks accumulate into one object (classic INI behavior).
        let v = obj("[db]\nhost = a\n[other]\nx = 1\n[db]\nport = 5432\n");
        assert_eq!(v["db"]["host"], json!("a"));
        assert_eq!(v["db"]["port"], json!("5432"));
        assert_eq!(v["other"]["x"], json!("1"));
    }

    #[test]
    fn empty_section_and_empty_value() {
        // An empty section still exists (so `ini_path_absent $.empty` sees it),
        // and `key =` is the empty string.
        let v = obj("[empty]\n[filled]\nk =\n");
        assert_eq!(v["empty"], json!({}), "empty section is an empty object");
        assert_eq!(
            v["filled"]["k"],
            json!(""),
            "empty value is the empty string"
        );
    }

    #[test]
    fn bom_and_crlf_are_tolerated() {
        let v = obj("\u{feff}root = true\r\n[s]\r\nk = v\r\n");
        assert_eq!(v["root"], json!("true"), "BOM stripped from the first key");
        assert_eq!(v["s"]["k"], json!("v"), "CRLF line endings");
    }

    #[test]
    fn indentation_is_not_significant() {
        // Indented keys under a section are common; leading whitespace is trimmed,
        // so they attach to the section (not treated as continuations).
        let v = obj("[s]\n    a = 1\n\tb = 2\n");
        assert_eq!(v["s"]["a"], json!("1"));
        assert_eq!(v["s"]["b"], json!("2"));
    }

    #[test]
    fn malformed_lines_are_parse_errors() {
        assert!(
            parse("[server\nhost = x\n").is_err(),
            "an unclosed section header is a parse error"
        );
        assert!(
            parse("[server] ; trailing\n").is_err(),
            "trailing content after `]` is a parse error"
        );
        assert!(
            parse("bareword\n").is_err(),
            "a line with no `=`/`:` is a parse error"
        );
        assert!(
            parse("= novalue\n").is_err(),
            "an empty key is a parse error"
        );
    }

    #[test]
    fn section_colliding_with_a_global_key_is_an_error() {
        // `db = 1` then `[db]` would silently clobber the global value; reject it.
        assert!(parse("db = 1\n[db]\nhost = x\n").is_err());
    }
}
