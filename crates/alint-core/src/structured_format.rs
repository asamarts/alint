//! Structured-document parsing shared by the structured-query rule
//! family (`{json,yaml,toml,xml,dotenv,properties,ini,hcl}_path_*`) and core-side predicates
//! that need to read a config / manifest into a `serde_json::Value`
//! tree.
//!
//! [`Format`] parses JSON / YAML / TOML / XML / dotenv / properties / INI / HCL into one
//! uniform `serde_json::Value` shape (YAML and TOML coerce through serde; XML
//! maps via the xmltodict-style convention in `xml_to_value` — `@attr`
//! / `#text` / repeated-element→array, leaf elements collapse to their
//! text string, namespaces flatten to local names, every leaf is a
//! string; dotenv and Java `.properties` are flat maps of literal-string
//! values, and INI is a 2-level `{ section: { key: value } }` map of the
//! same, pre-section keys hoisted to the top level; HCL maps via `hcl-rs`,
//! JSON-native — blocks nest by type + labels, values keep their type, and
//! an unevaluated expression is an opaque string), so a single `JSONPath`
//! engine only ever has to reason about one tree shape. XML design +
//! open-question resolutions: `docs/design/v0.10/xml_path.md`.

use serde_json::Value;

/// Which config format the target file is parsed as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Yaml,
    Toml,
    Xml,
    Dotenv,
    Properties,
    Ini,
    Hcl,
}

impl Format {
    /// The canonical inventory of config formats and the single source of truth
    /// for the config-format axis. Format-specific surfaces (`extract`, the
    /// structured-query kinds, `json_schema_passes`, the `did_you_mean` hints)
    /// should cover every variant; a per-surface parity test asserts that as each
    /// surface is leveled (today `extract_spec_covers_every_format`; the rest land
    /// with the format-coverage rollout). `format_all_is_complete` guards this
    /// list itself. See `docs/design/format-coverage.md`.
    pub const ALL: &'static [Format] = &[
        Format::Json,
        Format::Yaml,
        Format::Toml,
        Format::Xml,
        Format::Dotenv,
        Format::Properties,
        Format::Ini,
        Format::Hcl,
    ];

    pub fn parse(self, text: &str) -> std::result::Result<Value, String> {
        match self {
            // Try strict JSON first (the common, fast path — plain
            // JSON is byte-for-byte unchanged). Only on failure retry
            // tolerating JSONC: `//` + `/* */` comments and trailing
            // commas, which the JS/TS ecosystem uses pervasively in
            // `.json` files (tsconfig.json, `.vscode/*.json`). If the
            // tolerant retry also fails, surface the *original* strict
            // error so genuinely-broken JSON reports accurately.
            Self::Json => serde_json::from_str(text).or_else(|strict_err| {
                serde_json::from_str(&strip_jsonc(text)).map_err(|_| strict_err.to_string())
            }),
            Self::Yaml => {
                // Bound flow-nesting before libyaml parses it super-linearly (a
                // crafted `[[[…` file otherwise hangs the run) — the YAML analogue
                // of the `xml_depth_within_limit` guard below. JSON (serde_json,
                // 128-deep) and TOML (toml, 80-deep) carry their own limits.
                if !crate::yaml_depth::flow_depth_within_limit(text) {
                    return Err(format!(
                        "YAML flow nesting exceeds the maximum supported depth ({})",
                        crate::yaml_depth::MAX_YAML_FLOW_DEPTH
                    ));
                }
                serde_yaml_ng::from_str(text).map_err(|e| e.to_string())
            }
            Self::Toml => toml::from_str(text).map_err(|e| e.to_string()),
            Self::Xml => xml_to_value(text),
            Self::Dotenv => crate::dotenv::parse(text),
            Self::Properties => properties_to_value(text),
            Self::Ini => crate::ini::parse(text),
            Self::Hcl => hcl_to_value(text),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Yaml => "YAML",
            Self::Toml => "TOML",
            Self::Xml => "XML",
            Self::Dotenv => "dotenv",
            Self::Properties => "properties",
            Self::Ini => "INI",
            Self::Hcl => "HCL",
        }
    }

    /// Detect the format from a path's extension. Returns `None`
    /// for unknown extensions; callers decide how to fall back
    /// (require an explicit `format:` override, default to JSON,
    /// emit a per-file violation, etc).
    pub fn detect_from_path(path: &std::path::Path) -> Option<Self> {
        // dotenv is filename-based, not extension-based: a bare `.env` has no
        // extension, and `.env.local` / `.env.production` carry the environment
        // where an extension would be. Match the `.env` family by name first.
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name == ".env" || name.starts_with(".env.") {
                return Some(Self::Dotenv);
            }
        }
        match path.extension()?.to_str()? {
            "json" => Some(Self::Json),
            "yaml" | "yml" => Some(Self::Yaml),
            "toml" => Some(Self::Toml),
            "properties" => Some(Self::Properties),
            "ini" | "cfg" => Some(Self::Ini),
            "hcl" | "tf" | "tfvars" | "nomad" => Some(Self::Hcl),
            "xml" | "csproj" | "props" | "targets" | "vbproj" | "fsproj" | "nuspec" => {
                Some(Self::Xml)
            }
            _ => None,
        }
    }
}

/// Make a JSONC document parseable as strict JSON: drop `//` and
/// `/* … */` comments and trailing commas (a `,` immediately before a
/// `]` / `}`). String-aware — markers inside `"…"` (with `\` escapes)
/// are preserved, so a `"https://…"` URL or a `","` literal is
/// untouched. Only invoked when strict parsing already failed, so
/// plain JSON never pays for it.
fn strip_jsonc(src: &str) -> String {
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

// ---------------------------------------------------------------
// XML → serde_json::Value
//
// xmltodict-style convention so the JSONPath a user writes reads
// like the XML they see. Full rationale + false-positive surface:
// `docs/design/v0.10/xml_path.md`.
// ---------------------------------------------------------------

/// Maximum XML element-nesting depth `xml_to_value` will
/// descend. Real config/manifest XML (`.csproj`, `pom.xml`, …)
/// is a handful of levels deep; 256 is far beyond any real
/// manifest yet far below the recursion depth that would
/// overflow the stack. A document nested deeper is rejected as a
/// parse error (one per-file violation via the existing
/// parse-error path) rather than recursed into — a crafted or
/// accidental deeply-nested file must never abort the run. The
/// other formats' parsers carry their own internal recursion
/// limits; this is the XML arm's equivalent.
pub const MAX_XML_DEPTH: usize = 256;

/// Conservatively bound the raw XML's element-nesting depth BEFORE
/// `roxmltree::Document::parse` sees it. `Document::parse` descends recursively
/// per element and overflows the stack — **aborting the whole process** — on
/// deeply-nested input (tens of thousands of levels); the `element_to_value`
/// [`MAX_XML_DEPTH`] guard is post-parse, so it only catches depths the parser
/// already survived. A cheap linear pre-scan rejects an over-deep document here
/// (as one ordinary per-file parse-error violation) so a crafted or accidental
/// `<a><a>…` file can never abort the run. Comment / CDATA / PI / declaration
/// regions are skipped so their contents don't count toward depth.
fn xml_depth_within_limit(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut pos = 0usize;
    let mut depth = 0usize;
    while pos < bytes.len() {
        if bytes[pos] != b'<' {
            pos += 1;
            continue;
        }
        let rest = &text[pos..];
        if rest.starts_with("</") {
            depth = depth.saturating_sub(1);
            pos += 2;
        } else if rest.starts_with("<!--") {
            pos += rest.find("-->").map_or(rest.len(), |p| p + 3);
        } else if rest.starts_with("<![CDATA[") {
            pos += rest.find("]]>").map_or(rest.len(), |p| p + 3);
        } else if rest.starts_with("<!") || rest.starts_with("<?") {
            // DOCTYPE / PI / other declaration: skip to its terminating `>`.
            pos += rest.find('>').map_or(rest.len(), |p| p + 1);
        } else {
            // `<tag …>` or `<tag/>`: find the closing `>` respecting quoted
            // attribute values (a `>` inside `"…"`/`'…'` isn't the tag end).
            let tag = rest.as_bytes();
            let mut end = 1usize;
            let mut quote: Option<u8> = None;
            while end < tag.len() {
                let ch = tag[end];
                if let Some(q) = quote {
                    if ch == q {
                        quote = None;
                    }
                } else if ch == b'"' || ch == b'\'' {
                    quote = Some(ch);
                } else if ch == b'>' {
                    break;
                }
                end += 1;
            }
            // Self-closing `<tag/>` opens and closes, so it adds no depth.
            let self_closing = end >= 2 && tag[end - 1] == b'/';
            if !self_closing {
                depth += 1;
                if depth > MAX_XML_DEPTH {
                    return false;
                }
            }
            pos += end + 1;
        }
    }
    true
}

/// Parse XML into the same `serde_json::Value` tree the rest of
/// the family queries. The document maps to
/// `{ <root-element-name>: <root value> }` so the root element is
/// the first `JSONPath` segment (`$.Project…`, `$.project…`).
fn xml_to_value(text: &str) -> std::result::Result<Value, String> {
    // Reject over-deep XML before `Document::parse` can overflow the stack.
    if !xml_depth_within_limit(text) {
        return Err(format!(
            "XML nesting exceeds the maximum supported depth ({MAX_XML_DEPTH})"
        ));
    }
    let doc = roxmltree::Document::parse(text).map_err(|e| {
        let msg = e.to_string();
        // roxmltree runs with DTD processing disabled (a billion-laughs / XXE
        // safety limit). Make the wholesale rejection actionable instead of the
        // terse upstream "XML with DTD detected".
        if msg.contains("DTD") {
            format!(
                "{msg}: alint parses XML with DTDs disabled for safety, so \
                 DTD-bearing files (Checkstyle, Spring, Ant, log4j) cannot be linted"
            )
        } else {
            msg
        }
    })?;
    let root = doc.root_element();
    let mut obj = serde_json::Map::new();
    obj.insert(
        root.tag_name().name().to_owned(),
        element_to_value(root, 0)?,
    );
    Ok(Value::Object(obj))
}

/// One element → its `Value`. Attributes become `@name` keys;
/// repeated child elements of the same (local) name become a JSON
/// array in document order; loose text becomes `#text` when the
/// element also has attributes/children, or *is* the value when
/// the element is a pure leaf. Empty element → `null`. Namespaces
/// are flattened to the local name (Open question 1 in the design
/// doc). `depth` bounds recursion at `MAX_XML_DEPTH`: past the
/// bound it returns `Err` (surfaced as one parse-error violation
/// via the caller) instead of recursing into a stack abort.
fn element_to_value(node: roxmltree::Node, depth: usize) -> std::result::Result<Value, String> {
    if depth >= MAX_XML_DEPTH {
        return Err(format!(
            "XML nesting exceeds the maximum supported depth ({MAX_XML_DEPTH})"
        ));
    }
    let mut obj = serde_json::Map::new();
    for attr in node.attributes() {
        obj.insert(
            format!("@{}", attr.name()),
            Value::String(attr.value().to_owned()),
        );
    }
    let mut has_child_elem = false;
    for child in node.children().filter(roxmltree::Node::is_element) {
        has_child_elem = true;
        let name = child.tag_name().name().to_owned();
        let val = element_to_value(child, depth + 1)?;
        match obj.get_mut(&name) {
            Some(Value::Array(arr)) => arr.push(val),
            Some(slot) => {
                let prev = slot.take();
                *slot = Value::Array(vec![prev, val]);
            }
            None => {
                obj.insert(name, val);
            }
        }
    }
    let text: String = node
        .children()
        .filter(roxmltree::Node::is_text)
        .filter_map(|n| n.text())
        .collect();
    let text = text.trim();
    if obj.is_empty() && !has_child_elem {
        return Ok(if text.is_empty() {
            Value::Null
        } else {
            Value::String(text.to_owned())
        });
    }
    if !text.is_empty() {
        obj.insert("#text".to_owned(), Value::String(text.to_owned()));
    }
    Ok(Value::Object(obj))
}

/// Java `.properties` -> a flat `{ key: "value" }` object of literal strings
/// (via `java-properties`, decoded as UTF-8; it handles `=`/`:`/space separators,
/// `#`/`!` comments, backslash line-continuations, and `\uXXXX` escapes). Dotted
/// keys (`a.b.c`) stay ONE opaque key, faithful to Java -- query with `$['a.b.c']`.
/// Values are literal: `${...}` placeholders are resolved by the application,
/// not the file, so they are kept verbatim. Duplicate keys: last wins. Two
/// upstream quirks vs Java's `Properties`: a `\uXXXX` surrogate PAIR (an emoji,
/// say) is rejected as a parse error, and a trailing backslash at EOF drops that
/// line.
fn properties_to_value(text: &str) -> std::result::Result<Value, String> {
    // Decode as UTF-8: alint holds the file as UTF-8 (the caller lossy-decodes its
    // bytes), but `java_properties::read` defaults to WINDOWS_1252, which would
    // mojibake any non-ASCII value (`café` -> `cafÃ©`). A leading BOM is stripped
    // so it cannot land on the first key.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut map = serde_json::Map::new();
    java_properties::PropertiesIter::new_with_encoding(text.as_bytes(), encoding_rs::UTF_8)
        .read_into(|k, v| {
            map.insert(k, Value::String(v));
        })
        .map_err(|e| e.to_string())?;
    Ok(Value::Object(map))
}

/// Maximum HCL structural (`{` / `[`) nesting depth `hcl_to_value` will parse.
/// `hcl-rs` is a recursive-descent parser with very large per-level stack frames
/// (tens of KB for a block / object / tuple), so deeply-nested structure OVERFLOWS
/// THE STACK and ABORTS THE PROCESS (SIGABRT) -- the same hazard as
/// [`MAX_XML_DEPTH`], but far shallower per level (the default 1-2 MB thread stack
/// overflows below ~30 levels). 256 is far beyond any real HCL yet well below the
/// parse-thread ceiling. A deeper document is one per-file parse-error violation.
pub const MAX_HCL_DEPTH: usize = 256;

/// Maximum HCL input size `hcl_to_value` will parse. The [`MAX_HCL_DEPTH`] guard
/// bounds `{` / `[` nesting, but `hcl-rs` ALSO recurses on EXPRESSION nesting that
/// carries no delimiter to count -- long operator chains (`1 + 1 + …`,
/// `a == a == …`), nested ternaries, and deeply-nested parentheses / function
/// calls (in code AND inside `${…}` string / heredoc interpolation). A lexical
/// scan cannot bound those, and they too overflow the stack (SIGABRT). They need
/// tens of thousands of levels, though, and every level costs input bytes, so a
/// size cap bounds them: a file this small cannot hold enough expression tokens to
/// exceed the parse-thread stack. Real HCL is far smaller (Terraform best practice
/// splits into modules); an oversize file is one per-file parse-error violation.
/// Kept in step with the parse-thread `stack_size` in `hcl_to_value`.
pub const MAX_HCL_BYTES: usize = 256 * 1024;

/// Conservatively bound the HCL constructs `hcl-rs` recurses on, BEFORE
/// `hcl::from_str` overflows the stack. Every one of these aborts the process on
/// deep input, and none is caught by a naive `{`/`[` scan:
/// - `{` / `[` / `(` nesting -- blocks, objects, tuples, index, **and
///   parentheses / function-call args** (parens have a big frame: ~5 K levels
///   abort). Counted EVERYWHERE, including inside strings / comments / heredocs,
///   because `${…}` interpolation embeds real (parsed, recursing) sub-expressions
///   there -- skipping those spans was a stack-abort BYPASS.
/// - A run of prefix unary `-` / `!` (`----1`, `!!!!true`) -- no delimiter to
///   count; whitespace does not break the run, any other byte does.
///
/// Binary-operator / ternary chains (`1 + 1 + …`, `1 ? … : …`) recurse too but
/// cost >= 2 bytes per level, so they are bounded by [`MAX_HCL_BYTES`] instead of
/// here. Over-counting (a literal brace / paren in a string or a long `-` run in a
/// comment) can at worst turn a pathological value into a parse-error violation --
/// never a crash. Returns false for an over-deep document.
fn hcl_depth_within_limit(text: &str) -> bool {
    let mut depth = 0usize;
    let mut unary_run = 0usize;
    for &c in text.as_bytes() {
        match c {
            b'{' | b'[' | b'(' => {
                depth += 1;
                if depth > MAX_HCL_DEPTH {
                    return false;
                }
                unary_run = 0;
            }
            b'}' | b']' | b')' => {
                depth = depth.saturating_sub(1);
                unary_run = 0;
            }
            b'-' | b'!' => {
                unary_run += 1;
                if unary_run > MAX_HCL_DEPTH {
                    return false;
                }
            }
            b' ' | b'\t' | b'\r' | b'\n' => {}
            _ => unary_run = 0,
        }
    }
    true
}

/// Parse HCL (Terraform / Nomad / Packer) into the same `serde_json::Value` tree
/// the family queries. `hcl::Value` is JSON-native, so `hcl::from_str` maps
/// directly: a block is nested by its type then labels (`resource "t" "n" {…}` ->
/// `$.resource.t.n`), values keep their HCL type (a number stays a number, unlike
/// the stringly-typed dotenv/INI), and an unevaluated expression (`var.x`,
/// `${…}`, a function call) arrives as an opaque string. A block type appearing
/// once is an object but REPEATED is an array (the XML cardinality footgun);
/// duplicate attributes and malformed input are parse errors.
///
/// Three layers keep `hcl-rs`'s unbounded recursion from ABORTING the process on a
/// crafted file: the [`MAX_HCL_BYTES`] size cap (bounds delimiter-less expression
/// nesting), the [`hcl_depth_within_limit`] `{` / `[` depth guard (bounds
/// big-frame structural nesting), and parsing on a large explicit-stack thread so
/// a file at those limits still has headroom on every platform (the 1 MB Windows /
/// ~2 MB rayon-worker default would crash on a tens-deep file). The returned
/// `Value` is safe to drop on the caller's ordinary stack: expressions flatten to
/// strings and structural nesting is bounded by the depth guard.
fn hcl_to_value(text: &str) -> std::result::Result<Value, String> {
    if text.len() > MAX_HCL_BYTES {
        return Err(format!(
            "HCL input exceeds the maximum supported size ({MAX_HCL_BYTES} bytes)"
        ));
    }
    if !hcl_depth_within_limit(text) {
        return Err(format!(
            "HCL nesting exceeds the maximum supported depth ({MAX_HCL_DEPTH})"
        ));
    }
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(512 * 1024 * 1024)
            .spawn_scoped(scope, || {
                hcl::from_str::<Value>(text).map_err(|e| e.to_string())
            })
            .expect("spawn HCL parse thread")
            .join()
            .unwrap_or_else(|_| Err("HCL parser panicked".to_string()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ─── JSONC tolerance ──────────────────────────────────────

    #[test]
    fn json_parse_tolerates_jsonc() {
        // tsconfig.json-style: `//` + `/* */` comments and trailing
        // commas. Strict parse fails, the tolerant retry succeeds.
        let jsonc = "{\n  // line comment\n  \"a\": 1, /* block */\n  \"b\": [1, 2,],\n}\n";
        let v = Format::Json.parse(jsonc).expect("JSONC should parse");
        assert_eq!(v["a"], serde_json::json!(1));
        assert_eq!(v["b"], serde_json::json!([1, 2]));
    }

    #[test]
    fn json_parse_preserves_comment_markers_inside_strings() {
        // `//` and `,` inside string values must NOT be stripped.
        let s = "{ \"url\": \"https://x/y\", \"note\": \"a,b\" }";
        let v = Format::Json.parse(s).expect("plain JSON");
        assert_eq!(v["url"], serde_json::json!("https://x/y"));
        assert_eq!(v["note"], serde_json::json!("a,b"));
    }

    #[test]
    fn broken_json_keeps_the_strict_error() {
        // A genuinely-malformed document (not JSONC) must still fail,
        // and report the *strict* parser's message.
        let err = Format::Json.parse("{ \"x\": 1, \"y\" }").unwrap_err();
        assert!(err.contains("expected"), "strict error preserved: {err}");
    }

    #[test]
    fn xml_depth_scan_does_not_count_comments_cdata_or_self_closing() {
        // The pre-scan must not over-count: comment/CDATA contents and
        // self-closing tags don't add nesting, so valid shallow docs pass.
        assert!(xml_depth_within_limit(
            "<r><!-- <a><a><a> --><c/><![CDATA[ <b><b> ]]><d attr=\"x>y\"/></r>"
        ));
        // A genuinely deep run is rejected.
        let deep = format!("{}{}", "<a>".repeat(300), "</a>".repeat(300));
        assert!(!xml_depth_within_limit(&deep));
        // Real manifest depth is fine.
        assert!(xml_depth_within_limit(
            "<Project><PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup></Project>"
        ));
    }

    #[test]
    fn xml_dtd_is_rejected_with_an_actionable_message() {
        // roxmltree runs with DTD processing disabled (billion-laughs / XXE
        // safety); the wholesale rejection must say WHY, not just the terse
        // upstream "XML with DTD detected".
        let err = Format::Xml
            .parse("<!DOCTYPE note SYSTEM \"note.dtd\"><note/>")
            .unwrap_err();
        assert!(err.contains("DTD"), "names the cause: {err}");
        assert!(
            err.contains("disabled for safety"),
            "explains the limit: {err}"
        );
    }

    #[test]
    fn xml_to_value_maps_attributes_text_arrays_and_empty() {
        // Pins the xmltodict-style contract (module docs): the root element wraps
        // the tree, attributes are `@name`, a leaf collapses to its text, repeated
        // siblings become an array, loose text alongside attributes/children is
        // `#text`, and an empty element is null. A silent drift here changes what
        // EVERY XML-consuming surface sees (extract, json_schema_passes, xml_path_*).
        let xml = r#"
            <Project Sdk="Microsoft.NET.Sdk">
              <PropertyGroup>
                <TargetFramework>net8.0</TargetFramework>
              </PropertyGroup>
              <ItemGroup>
                <Ref Include="a" />
                <Ref Include="b" />
              </ItemGroup>
              <Empty/>
              <Mixed Attr="x">hello</Mixed>
            </Project>
        "#;
        let v = Format::Xml.parse(xml).expect("parse xml");
        // Root element wraps the tree; attributes become `@name` keys.
        assert_eq!(v["Project"]["@Sdk"], json!("Microsoft.NET.Sdk"));
        // A leaf element collapses to its text string.
        assert_eq!(
            v["Project"]["PropertyGroup"]["TargetFramework"],
            json!("net8.0")
        );
        // Repeated siblings become a document-order array of objects.
        assert_eq!(
            v["Project"]["ItemGroup"]["Ref"],
            json!([{"@Include": "a"}, {"@Include": "b"}])
        );
        // An empty element is null (NOT an empty object/string).
        assert_eq!(v["Project"]["Empty"], Value::Null);
        // Text alongside an attribute is `#text` -- the element is an object, not
        // the bare string "hello" (the `Condition=`-attribute reshaping footgun).
        assert_eq!(
            v["Project"]["Mixed"],
            json!({"@Attr": "x", "#text": "hello"})
        );
    }

    #[test]
    fn dotenv_files_detect_by_filename() {
        use std::path::Path;
        // dotenv detection is filename-based (a bare `.env` has no extension), the
        // one piece of new control flow in the dotenv work. Positive cases:
        for p in [".env", ".env.local", ".env.production", ".env.example"] {
            assert_eq!(
                Format::detect_from_path(Path::new(p)),
                Some(Format::Dotenv),
                "{p} should detect as dotenv"
            );
        }
        // Near-misses must NOT match (the `.env.` boundary matters: a regression to
        // `contains(\".env\")` would wrongly claim `.environment` / `foo.env`).
        for p in [".environment", "env.local", "foo.env", "envrc", ".envision"] {
            assert_ne!(
                Format::detect_from_path(Path::new(p)),
                Some(Format::Dotenv),
                "{p} must NOT detect as dotenv"
            );
        }
    }

    #[test]
    fn properties_is_a_flat_literal_map() {
        // `.properties` -> a flat object of literal strings: all three Java
        // separators (`=` / `:` / space), dotted keys stay ONE key, and `${...}`
        // placeholders are kept verbatim (resolved by the app, not the file).
        let text = "# comment\n\
                    db.host = localhost\n\
                    db.port : 5432\n\
                    app.name value with spaces\n\
                    url=${NOT_EXPANDED}/x\n";
        let v = Format::Properties.parse(text).expect("parse properties");
        assert_eq!(
            v["db.host"],
            json!("localhost"),
            "`=` separator, dotted key flat"
        );
        assert_eq!(v["db.port"], json!("5432"), "`:` separator");
        assert_eq!(v["app.name"], json!("value with spaces"), "space separator");
        assert_eq!(
            v["url"],
            json!("${NOT_EXPANDED}/x"),
            "placeholders are literal"
        );
    }

    #[test]
    fn properties_non_ascii_is_utf8_not_latin1_with_bom() {
        // Encoding regression guard: UTF-8 values must round-trip, not mojibake
        // through Windows-1252 (`café` -> `cafÃ©`), and a leading BOM is stripped.
        let v = Format::Properties
            .parse("\u{feff}name=café\ngreeting=\u{65e5}\u{672c}\u{8a9e}\n")
            .expect("parse utf-8 properties");
        assert_eq!(
            v["name"],
            json!("café"),
            "UTF-8 value + BOM stripped from key"
        );
        assert_eq!(v["greeting"], json!("日本語"), "CJK round-trips");
    }

    #[test]
    fn properties_escapes_continuations_dups_and_comments() {
        let v = Format::Properties
            .parse(
                "# comment\n\
                 ! also a comment\n\
                 unicode=caf\\u00e9\n\
                 cont=line1\\\n  line2\n\
                 dup=first\n\
                 dup=second\n",
            )
            .expect("parse");
        assert_eq!(v["unicode"], json!("café"), "\\uXXXX escape decoded");
        assert_eq!(v["cont"], json!("line1line2"), "line-continuation joins");
        assert_eq!(v["dup"], json!("second"), "duplicate keys: last wins");
        let obj = v.as_object().unwrap();
        assert!(!obj.contains_key("# comment"), "`#` comment excluded");
        assert!(
            !obj.contains_key("! also a comment"),
            "`!` comment excluded"
        );
        assert_eq!(obj.len(), 3, "only unicode / cont / dup");
    }

    #[test]
    fn properties_malformed_unicode_escape_is_an_error() {
        // The parse-error path: java-properties rejects a bad `\u` escape.
        assert!(Format::Properties.parse("k=\\uZZZZ\n").is_err());
    }

    #[test]
    fn properties_and_props_extensions_are_distinct() {
        use std::path::Path;
        // `.properties` (Java config) detects as Properties; `.props` (an MSBuild
        // XML file) detects as XML -- distinct extensions kept separate by the
        // match arms, so neither is misparsed as the other.
        assert_eq!(
            Format::detect_from_path(Path::new("application.properties")),
            Some(Format::Properties)
        );
        assert_eq!(
            Format::detect_from_path(Path::new("Directory.Build.props")),
            Some(Format::Xml)
        );
    }

    #[test]
    fn ini_dispatches_to_a_two_level_section_map() {
        // The `Format::Ini` arm reaches the hand-rolled parser: pre-section keys
        // hoist to the top level, a `[section]` is a nested object, and both
        // `=`/`:` separate. Full parser coverage lives in `crate::ini`.
        let v = Format::Ini
            .parse("root = true\n[server]\nhost = localhost\nport : 8080\n")
            .expect("parse ini");
        assert_eq!(v["root"], json!("true"), "global key hoisted to top level");
        assert_eq!(v["server"]["host"], json!("localhost"));
        assert_eq!(v["server"]["port"], json!("8080"), "`:` separator");
    }

    #[test]
    fn ini_and_cfg_extensions_detect_as_ini() {
        use std::path::Path;
        for p in ["tox.ini", "pytest.ini", "setup.cfg", "app.cfg"] {
            assert_eq!(
                Format::detect_from_path(Path::new(p)),
                Some(Format::Ini),
                "{p} should detect as INI"
            );
        }
    }

    #[test]
    fn hcl_dispatches_to_a_json_native_value() {
        // hcl-rs maps HCL directly: a block nests by type + labels, values keep their
        // HCL type (a number stays a number, unlike stringly-typed dotenv/INI), and an
        // unevaluated expression is an opaque string.
        let v = Format::Hcl
            .parse(
                "region = \"us-east-1\"\n\
                 resource \"aws_instance\" \"web\" {\n  ami = \"ami-1\"\n  count = 2\n}\n\
                 locals {\n  name = var.env\n}\n",
            )
            .expect("parse hcl");
        assert_eq!(v["region"], json!("us-east-1"));
        assert_eq!(
            v["resource"]["aws_instance"]["web"]["ami"],
            json!("ami-1"),
            "block nests by type -> label1 -> label2"
        );
        assert_eq!(
            v["resource"]["aws_instance"]["web"]["count"],
            json!(2),
            "a number stays a JSON number"
        );
        assert_eq!(
            v["locals"]["name"],
            json!("${var.env}"),
            "an unevaluated expression is an opaque string"
        );
    }

    #[test]
    fn hcl_repeated_block_is_an_array_single_is_an_object() {
        // The cardinality footgun (same as XML repeated elements): a block type
        // appearing once is an object, repeated is a document-order array.
        assert_eq!(
            Format::Hcl.parse("s {\n  a = 1\n}\n").expect("one")["s"],
            json!({"a": 1})
        );
        assert_eq!(
            Format::Hcl
                .parse("s {\n  a = 1\n}\ns {\n  a = 2\n}\n")
                .expect("many")["s"],
            json!([{"a": 1}, {"a": 2}])
        );
    }

    #[test]
    fn hcl_extensions_detect_as_hcl() {
        use std::path::Path;
        for p in ["main.tf", "variables.tfvars", "config.hcl", "job.nomad"] {
            assert_eq!(
                Format::detect_from_path(Path::new(p)),
                Some(Format::Hcl),
                "{p} should detect as HCL"
            );
        }
    }

    #[test]
    fn hcl_over_deep_nesting_is_rejected_not_a_stack_abort() {
        // hcl-rs overflows the stack (process ABORT) on deep input; the guard must
        // reject an over-limit file as an error, while a realistically-deep one still
        // parses (on the large-stack thread). If this ever aborts instead of failing,
        // the guard or the parse-thread stack regressed.
        let over = format!(
            "{}x=1\n{}",
            "a {\n".repeat(MAX_HCL_DEPTH + 5),
            "}\n".repeat(MAX_HCL_DEPTH + 5)
        );
        assert!(
            Format::Hcl.parse(&over).is_err(),
            "an over-deep HCL file must be a parse error, not a crash"
        );
        let ok = format!("{}x=1\n{}", "a {\n".repeat(40), "}\n".repeat(40));
        assert!(Format::Hcl.parse(&ok).is_ok(), "a 40-deep file parses");
    }

    #[test]
    fn hcl_depth_guard_counts_delimiters_everywhere_and_unary_runs() {
        // The guard counts `{`/`[`/`(` WHEREVER they appear -- including inside a
        // string / heredoc, since `${...}` interpolation embeds real recursing
        // sub-expressions there (skipping those spans was a stack-abort bypass) --
        // and bounds a prefix `-`/`!` run.
        let over = MAX_HCL_DEPTH + 1;
        assert!(!hcl_depth_within_limit(&"(".repeat(over)), "parens count");
        assert!(
            !hcl_depth_within_limit(&format!("x = \"{}\"\n", "[".repeat(over))),
            "delimiters inside a string count"
        );
        assert!(
            !hcl_depth_within_limit(&format!("x = <<EOT\n{}\nEOT\n", "{".repeat(over))),
            "delimiters inside a heredoc count"
        );
        assert!(
            !hcl_depth_within_limit(&"-".repeat(over)),
            "a `-` run is bounded"
        );
        assert!(
            !hcl_depth_within_limit(&"! ".repeat(over)),
            "whitespace does not reset a `!` run"
        );
        // A realistically shallow file passes (balanced parens, a unary `-`, a
        // normal heredoc with a small JSON body).
        assert!(hcl_depth_within_limit(
            "x = max(1, -2)\npolicy = <<EOT\n{ \"a\": [1, 2] }\nEOT\n"
        ));
    }

    #[test]
    fn hcl_expression_bombs_are_rejected_not_a_stack_abort() {
        // THE safety regression. Every construct hcl-rs recurses on -- deep parens /
        // function calls / unary chains / interpolation-embedded nesting (caught by
        // the depth guard), and oversize operator chains (caught by the size cap) --
        // must yield an `Err`, NEVER a SIGABRT that takes down the whole run. Each
        // input below aborts an unguarded hcl-rs; if this test ABORTS instead of
        // failing, the guard / size cap / parse-thread stack regressed.
        let d = 60_000; // past every measured crash threshold
        let bombs = [
            format!("x = {}1{}", "(".repeat(d), ")".repeat(d)), // parens
            format!("x = {}1{}", "f(".repeat(d), ")".repeat(d)), // function calls
            format!("x = {}1", "-".repeat(d)),                  // unary minus
            format!("x = {}true", "!".repeat(d)),               // unary not
            format!("x = \"${{{}1{}}}\"", "(".repeat(d), ")".repeat(d)), // interp parens
            format!("x = \"${{{}1{}}}\"", "[".repeat(d), "]".repeat(d)), // interp tuple
            format!("x = <<EOT\n${{{}1{}}}\nEOT\n", "[".repeat(d), "]".repeat(d)), // heredoc interp
        ];
        for b in &bombs {
            assert!(
                Format::Hcl.parse(b).is_err(),
                "an expression bomb must be a parse error, not a crash"
            );
        }
        // A bounded operator chain still parses (the size cap does not over-reject a
        // normal file); an oversize input is size-rejected before it can recurse.
        assert!(
            Format::Hcl
                .parse(&format!("x = {}1", "1 + ".repeat(5_000)))
                .is_ok(),
            "a bounded binary chain parses"
        );
        assert!(
            Format::Hcl
                .parse(&format!("x = {}1", "1 + ".repeat(MAX_HCL_BYTES)))
                .is_err(),
            "an oversize input is rejected by the byte cap"
        );
    }

    #[test]
    fn hcl_malformed_and_duplicate_attr_are_parse_errors() {
        assert!(
            Format::Hcl.parse("resource \"x\" {\n  a =\n").is_err(),
            "an incomplete block is a parse error"
        );
        assert!(
            Format::Hcl.parse("a = 1\na = 2\n").is_err(),
            "HCL rejects a duplicate attribute"
        );
    }

    #[test]
    fn format_all_is_complete() {
        // Every parity gate iterates `Format::ALL`, so a variant missing from ALL
        // silently bypasses them. The real guard here is the exhaustive `match`
        // below: a NEW `Format` variant is a COMPILE error until a reviewer adds an
        // arm, so no variant reaches the codebase without a human editing this
        // test with `Format::ALL` in view; the asserts then require ALL and
        // `variants` to list the same set. Fully deriving ALL from the enum would
        // need a proc-macro (`strum`), which this crate deliberately avoids -- so
        // the compile error, not the assert, is what stops a half-wired variant.
        let variants = [
            Format::Json,
            Format::Yaml,
            Format::Toml,
            Format::Xml,
            Format::Dotenv,
            Format::Properties,
            Format::Ini,
            Format::Hcl,
        ];
        for f in variants {
            // Distinct arms (clippy-clean) keep the match exhaustive, so a new
            // Format variant is a compile error here until it is added.
            let _label = match f {
                Format::Json => "json",
                Format::Yaml => "yaml",
                Format::Toml => "toml",
                Format::Xml => "xml",
                Format::Dotenv => "dotenv",
                Format::Properties => "properties",
                Format::Ini => "ini",
                Format::Hcl => "hcl",
            };
        }
        assert_eq!(
            Format::ALL.len(),
            variants.len(),
            "Format::ALL is out of sync with the Format variants; update both"
        );
        for f in variants {
            assert!(
                Format::ALL.contains(&f),
                "Format::{f:?} is missing from Format::ALL"
            );
        }
    }
}
