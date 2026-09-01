//! Structured-document parsing shared by the structured-query rule
//! family (`{json,yaml,toml,xml}_path_*`) and core-side predicates
//! that need to read a config / manifest into a `serde_json::Value`
//! tree.
//!
//! [`Format`] parses JSON / YAML / TOML / XML into one uniform
//! `serde_json::Value` shape (YAML and TOML coerce through serde; XML
//! maps via the xmltodict-style convention in `xml_to_value` — `@attr`
//! / `#text` / repeated-element→array, leaf elements collapse to their
//! text string, namespaces flatten to local names, every leaf is a
//! string), so a single `JSONPath` engine only ever has to reason
//! about one tree shape. XML design + open-question resolutions:
//! `docs/design/v0.10/xml_path.md`.

use serde_json::Value;

/// Which YAML-flavoured parser to use on the target file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Yaml,
    Toml,
    Xml,
}

impl Format {
    /// The canonical inventory of config formats and the single source of truth
    /// for the config-format axis. Format-specific surfaces (`extract`, the
    /// structured-query kinds, `json_schema_passes`, the `did_you_mean` hints)
    /// should cover every variant; a per-surface parity test asserts that as each
    /// surface is leveled (today `extract_spec_covers_every_format`; the rest land
    /// with the format-coverage rollout). `format_all_is_complete` guards this
    /// list itself. See `docs/design/format-coverage.md`.
    pub const ALL: &'static [Format] = &[Format::Json, Format::Yaml, Format::Toml, Format::Xml];

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
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Yaml => "YAML",
            Self::Toml => "TOML",
            Self::Xml => "XML",
        }
    }

    /// Detect the format from a path's extension. Returns `None`
    /// for unknown extensions; callers decide how to fall back
    /// (require an explicit `format:` override, default to JSON,
    /// emit a per-file violation, etc).
    pub fn detect_from_path(path: &std::path::Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "json" => Some(Self::Json),
            "yaml" | "yml" => Some(Self::Yaml),
            "toml" => Some(Self::Toml),
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
    fn format_all_is_complete() {
        // Every parity gate iterates `Format::ALL`, so a variant missing from ALL
        // silently bypasses them. The real guard here is the exhaustive `match`
        // below: a NEW `Format` variant is a COMPILE error until a reviewer adds an
        // arm, so no variant reaches the codebase without a human editing this
        // test with `Format::ALL` in view; the asserts then require ALL and
        // `variants` to list the same set. Fully deriving ALL from the enum would
        // need a proc-macro (`strum`), which this crate deliberately avoids -- so
        // the compile error, not the assert, is what stops a half-wired variant.
        let variants = [Format::Json, Format::Yaml, Format::Toml, Format::Xml];
        for f in variants {
            // Distinct arms (clippy-clean) keep the match exhaustive, so a new
            // Format variant is a compile error here until it is added.
            let _label = match f {
                Format::Json => "json",
                Format::Yaml => "yaml",
                Format::Toml => "toml",
                Format::Xml => "xml",
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
