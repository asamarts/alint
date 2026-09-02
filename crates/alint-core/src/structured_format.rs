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

/// Maximum INPUT size any structured format will parse into a value tree. The read
/// cap [`crate::walker::MAX_ANALYZE_BYTES`] (256 MiB) bounds bytes, but a parsed
/// `serde_json::Value` tree measures ~16-19x the input (XML worst; dotenv ~10x), so
/// a near-read-cap structured file would peak at multiple GB of RSS -- and the
/// per-file rule dispatch is parallel, so several parse at once. Capping the
/// structured-parse INPUT well below the read cap bounds a single file's tree to a
/// few hundred MB. 64 MiB is far beyond any real config / manifest (they are KB-MB;
/// even a large `OpenAPI` spec or SBOM is well under) yet 4x tighter than the read
/// cap. An oversize structured file is one per-file parse-error violation. HCL keeps
/// its own tighter [`MAX_HCL_BYTES`] (64 KiB); this is the ceiling for the rest.
/// (Total concurrent parse memory -- this cap times the `par_iter` fan-out -- is a
/// separate, coarser bound tracked in `docs/design/format-coverage.md`.)
pub const MAX_STRUCTURED_BYTES: usize = 64 * 1024 * 1024;

/// The structured-parse cap must stay meaningfully below the read cap, or it does
/// nothing (a file that passes the read cap would also pass this and still balloon).
const _: () = assert!((MAX_STRUCTURED_BYTES as u64) < crate::walker::MAX_ANALYZE_BYTES / 2);

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
        // Strip any leading UTF-8 BOM(s) uniformly. Some libraries (serde_json,
        // hcl-rs) reject a `\u{feff}` prefix as a syntax error, so a BOM-prefixed
        // `package.json` / `config.hcl` (common from Windows editors) would
        // false-fire the structured rules; the hand-rolled dotenv/ini/properties
        // parsers already strip it. `trim_start_matches` (not `strip_prefix`) so
        // consecutive leading BOMs are all removed. Flagging a BOM is the `no_bom`
        // rule's job.
        let text = text.trim_start_matches('\u{feff}');
        // An empty file (nothing but BOMs and whitespace) is "no config", not a
        // broken document: return an empty OBJECT (an empty document) so
        // `*_path_absent` is satisfied, `if_present` stays silent, AND
        // `json_schema_passes` validates it as `{}` -- which satisfies
        // `{"type":"object"}`, since an empty config file IS a valid empty table.
        // (Returning `Value::Null` here made `json_schema_passes` false-fire "null
        // is not of type object" on an empty `.toml`/`.ini`/`.env`/`.properties`/
        // `.hcl`, while a comment-only file of the same format parsed to `{}` and
        // passed -- two "no config" files disagreeing.) Otherwise the JSON / XML
        // libraries reject empty input as a syntax error -- a false positive for the
        // absent family. The emptiness test also strips a stray BOM interleaved with
        // whitespace (e.g. `\u{feff} \u{feff}`), which `trim_start_matches` alone
        // leaves and which YAML/properties/INI would otherwise mis-read as a
        // one-character scalar. Flagging an empty file is the `no_empty_files` rule's
        // job. (TOML / dotenv / INI / properties already parse empty to `{}`; YAML's
        // native empty is `null`, but one uniform empty document is less surprising
        // than per-format null-vs-object.)
        if text
            .trim_matches(|c: char| c == '\u{feff}' || c.is_whitespace())
            .is_empty()
        {
            return Ok(Value::Object(serde_json::Map::new()));
        }
        // Bound the parsed-tree memory: a structured tree is ~16-19x the input, so a
        // large file balloons to multiple GB. Reject before building it. (HCL's own
        // 64 KiB cap in `hcl_to_value` is tighter and fires first for HCL.)
        if text.len() > MAX_STRUCTURED_BYTES {
            return Err(format!(
                "input exceeds the maximum supported size for a structured document ({MAX_STRUCTURED_BYTES} bytes)"
            ));
        }
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
                // of the `xml_within_parse_limits` guard below. JSON (serde_json,
                // 128-deep) and TOML (toml, 80-deep) carry their own limits.
                if !crate::yaml_depth::flow_depth_within_limit(text) {
                    return Err(format!(
                        "YAML flow nesting exceeds the maximum supported depth ({})",
                        crate::yaml_depth::MAX_YAML_FLOW_DEPTH
                    ));
                }
                // Bound ALIAS expansion: `serde_yaml_ng`'s own limits miss a single
                // anchor referenced many times (`*a` x N -> N x anchor-size nodes),
                // which balloons a small file into millions of nodes. Cheap
                // discard-only pre-count; alias-free text short-circuits for free.
                if !crate::yaml_depth::expansion_within_limit(text) {
                    return Err(format!(
                        "YAML alias expansion exceeds the maximum supported node count ({})",
                        crate::yaml_depth::MAX_YAML_EXPANSION_NODES
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
        // Extension detection first, case-INSENSITIVELY: `.JSON` / `.CSPROJ` /
        // `.XML` (common on Windows and case-insensitive filesystems, or from Java
        // tooling) are valid and must not yield a false "could not detect format".
        // A KNOWN format extension also wins over the `.env` family below, so a
        // `.env.json` / `.env.yaml` is parsed by its real format, not shadowed as
        // dotenv.
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            match ext.to_ascii_lowercase().as_str() {
                "json" => return Some(Self::Json),
                "yaml" | "yml" => return Some(Self::Yaml),
                "toml" => return Some(Self::Toml),
                "properties" => return Some(Self::Properties),
                "ini" | "cfg" => return Some(Self::Ini),
                "hcl" | "tf" | "tfvars" | "nomad" => return Some(Self::Hcl),
                "xml" | "csproj" | "props" | "targets" | "vbproj" | "fsproj" | "nuspec" => {
                    return Some(Self::Xml);
                }
                _ => {}
            }
        }
        // Well-known config files identified by NAME, not extension: extension-less
        // (`.editorconfig`, `Pipfile`) or a `.config` that is specifically .NET XML
        // (a bare `.config` extension is not universally XML, so match exact names).
        // Only UNAMBIGUOUS names are mapped -- `.eslintrc`/`.prettierrc` are skipped
        // because they may be JSON OR YAML, so they still need an explicit `format:`.
        // dotenv is likewise filename-based: a bare `.env` has no extension, and
        // `.env.local` / `.env.production` carry the environment where an extension
        // would be. The `.env` family matches by name but only for a suffix that
        // ISN'T a known format extension (handled above), so `.env.json` is JSON.
        // All case-folded (`.ENV`, `WEB.CONFIG`, ...).
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            let lname = name.to_ascii_lowercase();
            match lname.as_str() {
                ".editorconfig" => return Some(Self::Ini),
                "pipfile" => return Some(Self::Toml),
                "web.config" | "app.config" => return Some(Self::Xml),
                _ => {}
            }
            if lname == ".env" || lname.starts_with(".env.") {
                return Some(Self::Dotenv);
            }
        }
        None
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
/// is a handful of levels deep; 128 is far beyond any real
/// manifest yet far below the recursion depth that would
/// overflow the stack. A document nested deeper is rejected as a
/// parse error (one per-file violation via the existing
/// parse-error path) rather than recursed into — a crafted or
/// accidental deeply-nested file must never abort the run. Unlike
/// HCL, XML parses on the CALLING thread (a rayon worker, ~2 MB
/// stack by Rust's std-thread default). roxmltree recurses ~1
/// frame per element; measured overflow is around depth ~350 on a
/// 2 MB debug stack (the realistic worker) and ~175 on a
/// constrained 1 MB stack, with far deeper limits in release
/// (~3100 on 2 MB). 128 keeps a ~2.7x margin on the 2 MB worker
/// (still ~1.4x even on a 1 MB stack), matching the JSON recursion
/// limit and the `when`-parser's calibration. The other formats'
/// parsers carry their own internal recursion limits; this is the
/// XML arm's equivalent.
pub const MAX_XML_DEPTH: usize = 128;

/// The analytically-safe ceiling for [`MAX_XML_DEPTH`], enforced at compile time
/// below. roxmltree recurses ~1 frame per element; the smallest stack alint
/// realistically parses XML on is a ~2 MiB rayon worker, which overflows (debug)
/// around ~350 elements deep, so ~half of that keeps a >=2x margin. This ceiling is
/// SCOPED to that 2 MiB production worker; the shipping `MAX_XML_DEPTH = 128` also
/// survives a constrained 1 MiB stack (~1.4x), but a raise all the way to this
/// ceiling would erode that non-production 1 MiB margin to ~1.1x (fine on 2 MiB) --
/// so treat a bump toward 160 as 2-MiB-only. Raising `MAX_XML_DEPTH` PAST this risks
/// a stack-overflow process-abort even on the 2 MiB worker -- and the depth tests
/// only exercise the REJECTION path (they run on a >=2 MiB harness stack where even
/// 300-deep survives), so a careless re-widening would pass every runtime test. This
/// static bound is the real guard.
const SAFE_MAX_XML_DEPTH: usize = 160;
const _: () = assert!(
    MAX_XML_DEPTH <= SAFE_MAX_XML_DEPTH,
    "MAX_XML_DEPTH exceeds its analytically-safe ceiling -- a deeply-nested XML \
     file could overflow the rayon-worker stack inside roxmltree parsing (SIGABRT)."
);

/// Maximum attributes on a single XML element that `xml_to_value` will accept.
/// roxmltree 0.20 validates per-element attribute UNIQUENESS in O(n^2) -- each new
/// attribute is compared against every prior attribute on the same element -- so a
/// single element bearing tens of thousands of distinct attributes turns a tiny
/// file into MINUTES of parse time (`<r a0=".." a1=".." …/>`: ~64 K attrs ≈ 96 s,
/// clean quadratic), an algorithmic-complexity `DoS` that NO nesting guard catches
/// (all the depth guards bound height, not width). Bounding attributes per element
/// makes total parse cost linear in the input: the aggregate work is
/// `sum(k_i^2) <= cap * sum(k_i) = cap * total_attrs`, and `total_attrs` is bounded
/// by the `MAX_ANALYZE_BYTES` (256 MiB) read cap, so the whole document is O(bytes).
/// The cap ALSO sets the constant: at 256 a crafted attribute-dense file parses at
/// roughly benign-XML speed (measured ~1.5x a same-size ordinary file, vs ~5x at
/// 1024), so it no longer costs meaningfully more than any other file of its size --
/// unlike HCL, XML has no format-specific byte cap (real XML data files can be large
/// and must not false-error), so the per-element cap is the sole width bound and is
/// kept tight. 256 is still ~5x beyond even an attribute-heavy real element (an
/// `MSBuild` `<Csc>`/`<Vbc>` task, the widest common case, exposes ~40; SVG/`.csproj`
/// nodes have far fewer) -- XML expresses repetition with child ELEMENTS, not
/// hundreds of attributes on one tag. roxmltree can't be bumped to fix this (0.21
/// stack-overflows on nesting; pinned at 0.20). An over-cap element is rejected as
/// one ordinary per-file parse-error violation.
const MAX_XML_ATTRS_PER_ELEMENT: usize = 256;

/// Conservatively bound the raw XML's element-nesting DEPTH and per-element
/// attribute WIDTH BEFORE `roxmltree::Document::parse` sees it, in one linear scan.
/// `Document::parse` descends recursively per element and overflows the stack —
/// **aborting the whole process** — on deeply-nested input (tens of thousands of
/// levels); the `element_to_value` [`MAX_XML_DEPTH`] guard is post-parse, so it
/// only catches depths the parser already survived. It also validates attribute
/// uniqueness in O(n^2) per element (see [`MAX_XML_ATTRS_PER_ELEMENT`]), a separate
/// wall-clock `DoS`. A cheap linear pre-scan rejects an over-deep OR over-wide
/// document here (as one ordinary per-file parse-error violation) so a crafted or
/// accidental `<a><a>…` / `<r a0.. a1..>` file can never abort or hang the run.
/// Comment / CDATA / PI / declaration regions are skipped so their contents don't
/// count toward depth or attributes. `Ok(())` when within both limits.
fn xml_within_parse_limits(text: &str) -> std::result::Result<(), String> {
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
            // Count attributes by the `=` signs OUTSIDE quotes: XML requires
            // quoted values, so each attribute contributes exactly one unquoted
            // `=`, and a `=` inside a value is skipped with the quote run.
            let tag = rest.as_bytes();
            let mut end = 1usize;
            let mut quote: Option<u8> = None;
            let mut attrs = 0usize;
            while end < tag.len() {
                let ch = tag[end];
                if let Some(q) = quote {
                    if ch == q {
                        quote = None;
                    }
                } else if ch == b'"' || ch == b'\'' {
                    quote = Some(ch);
                } else if ch == b'=' {
                    attrs += 1;
                    // Bail the instant the cap is exceeded, so a pathological
                    // single tag (up to `MAX_ANALYZE_BYTES`) can't even make the
                    // pre-scan read to its end -- work stays bounded by the cap,
                    // not the tag size.
                    if attrs > MAX_XML_ATTRS_PER_ELEMENT {
                        break;
                    }
                } else if ch == b'>' {
                    break;
                }
                end += 1;
            }
            if attrs > MAX_XML_ATTRS_PER_ELEMENT {
                return Err(format!(
                    "an XML element has more than the maximum supported number of \
                     attributes ({MAX_XML_ATTRS_PER_ELEMENT})"
                ));
            }
            // Self-closing `<tag/>` opens and closes, so it adds no depth.
            let self_closing = end >= 2 && tag[end - 1] == b'/';
            if !self_closing {
                depth += 1;
                if depth > MAX_XML_DEPTH {
                    return Err(format!(
                        "XML nesting exceeds the maximum supported depth ({MAX_XML_DEPTH})"
                    ));
                }
            }
            pos += end + 1;
        }
    }
    Ok(())
}

/// Parse XML into the same `serde_json::Value` tree the rest of
/// the family queries. The document maps to
/// `{ <root-element-name>: <root value> }` so the root element is
/// the first `JSONPath` segment (`$.Project…`, `$.project…`).
fn xml_to_value(text: &str) -> std::result::Result<Value, String> {
    // Reject over-deep OR over-wide XML before `Document::parse` can overflow the
    // stack (depth) or hang in O(n^2) attribute validation (width).
    xml_within_parse_limits(text)?;
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
/// bounds `{` / `[` / `(` and unary nesting, but `hcl-rs` ALSO recurses on
/// EXPRESSION-OPERATOR nesting that carries no such delimiter -- long binary /
/// ternary chains (`1 + 1 + …`, `a == a == …`, `1 ? … : …`), including inside
/// `${…}` string / heredoc interpolation. A lexical scan cannot cleanly bound
/// those (operators interleave with operands and span lines), and they too
/// overflow the stack (SIGABRT). The DENSEST such chain is ~2 bytes per recursion
/// level (`+1`), so the byte cap bounds them: on the 512 MB parse thread an
/// operator chain overflows around ~75 000 levels (~150 KB), so 64 KiB admits at
/// most ~32 000 levels -- a >2x stack margin that tolerates per-level frame drift
/// across `hcl-rs` versions. Real HCL is far smaller (Terraform best practice
/// splits into modules); an oversize file is one per-file parse-error violation.
/// MUST be kept below (parse-thread `stack_size` / operator frame / 2 bytes).
pub const MAX_HCL_BYTES: usize = 64 * 1024;

/// The analytically-derived ceiling for [`MAX_HCL_BYTES`], enforced at compile
/// time below. The 512 MB parse thread overflows a densest (~2 bytes/level)
/// operator chain around ~75 000 levels (~150 KiB); half of that (a >=2x stack
/// margin) is the largest cap that stays safe against per-level frame drift.
/// Widening `MAX_HCL_BYTES` past this re-opens the operator-chain stack-overflow
/// `DoS` that the 256 KiB cap let through (PR #223) -- and the bomb regression test
/// only fails by CRASHING, which is fragile, so pin the bound statically too.
const SAFE_MAX_HCL_BYTES: usize = 75 * 1024;
const _: () = assert!(
    MAX_HCL_BYTES <= SAFE_MAX_HCL_BYTES,
    "MAX_HCL_BYTES exceeds its analytically-safe ceiling -- a crafted operator \
     chain could again overflow the HCL parse thread (SIGABRT). See PR #223."
);

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
        match std::thread::Builder::new()
            .stack_size(512 * 1024 * 1024)
            .spawn_scoped(scope, || {
                hcl::from_str::<Value>(text).map_err(|e| e.to_string())
            }) {
            Ok(handle) => handle
                .join()
                .unwrap_or_else(|_| Err("HCL parser panicked".to_string())),
            // A linter must never abort the run -- not on untrusted input, and not
            // on resource pressure. If the OS refuses the large-stack parse thread
            // (an `RLIMIT_AS` / `RLIMIT_NPROC` / `vm.max_map_count` ceiling, common
            // in hardened CI / containers, against which the 512 MB reservation
            // counts), surface it as one ordinary per-file parse-error violation
            // rather than `.expect`-panicking with the "alint crashed" banner.
            Err(e) => Err(format!("could not allocate HCL parse thread: {e}")),
        }
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
        assert!(
            xml_within_parse_limits(
                "<r><!-- <a><a><a> --><c/><![CDATA[ <b><b> ]]><d attr=\"x>y\"/></r>"
            )
            .is_ok()
        );
        // A genuinely deep run is rejected with a depth message.
        let deep = format!("{}{}", "<a>".repeat(300), "</a>".repeat(300));
        let err = xml_within_parse_limits(&deep).unwrap_err();
        assert!(err.contains("depth"), "depth rejection: {err}");
        // Real manifest depth is fine.
        assert!(xml_within_parse_limits(
            "<Project><PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup></Project>"
        )
        .is_ok());
    }

    #[test]
    fn xml_scan_bounds_attribute_width_to_prevent_quadratic_parse() {
        // roxmltree 0.20 validates per-element attribute uniqueness in O(n^2), so
        // one element with tens of thousands of attributes is a wall-clock DoS the
        // depth guard can't see. The pre-scan must reject an over-wide element as
        // an ordinary parse error -- NOT hand it to roxmltree to grind on.
        use std::fmt::Write as _;
        let mut wide = String::from("<r");
        for i in 0..(MAX_XML_ATTRS_PER_ELEMENT + 10) {
            write!(wide, " a{i}=\"v\"").unwrap();
        }
        wide.push_str("/>");
        let err = xml_within_parse_limits(&wide).unwrap_err();
        assert!(
            err.contains("attributes"),
            "over-wide element rejected with an attribute message: {err}"
        );
        // And the full parse surfaces it as one parse-error (no hang), not a panic.
        assert!(Format::Xml.parse(&wide).is_err());
        // A normal, even generously-attributed, element is unaffected.
        let mut ok = String::from("<Reference");
        for i in 0..64 {
            write!(ok, " p{i}=\"v\"").unwrap();
        }
        ok.push_str("/>");
        assert!(xml_within_parse_limits(&ok).is_ok());
        // `=` inside a quoted value is NOT an attribute and must not be counted:
        // a single attribute whose value is packed with `=` stays one attribute.
        let equals_in_value = format!("<r a=\"{}\"/>", "=".repeat(5000));
        assert!(xml_within_parse_limits(&equals_in_value).is_ok());
        // Exact boundary: `MAX_XML_ATTRS_PER_ELEMENT` is accepted, one more is not.
        let at_cap: String = format!(
            "<r{}/>",
            (0..MAX_XML_ATTRS_PER_ELEMENT).fold(String::new(), |mut s, i| {
                write!(s, " a{i}=\"v\"").unwrap();
                s
            })
        );
        assert!(
            xml_within_parse_limits(&at_cap).is_ok(),
            "exactly the cap is accepted"
        );
        let over_cap = at_cap.replace("/>", " extra=\"v\"/>");
        assert!(
            xml_within_parse_limits(&over_cap).is_err(),
            "one over the cap is rejected"
        );
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
        // A KNOWN format extension after `.env.` wins over the dotenv family: a
        // `.env.json` is JSON-format env config, NOT a flat dotenv map (parsing it
        // with the dotenv parser would silently produce a wrong tree).
        for (p, want) in [
            (".env.json", Format::Json),
            (".env.yaml", Format::Yaml),
            (".env.toml", Format::Toml),
        ] {
            assert_eq!(
                Format::detect_from_path(Path::new(p)),
                Some(want),
                "{p} should detect by its real extension, not be shadowed as dotenv"
            );
        }
    }

    #[test]
    fn format_detection_is_case_insensitive() {
        use std::path::Path;
        // An uppercase extension (Windows / case-insensitive FS / Java tooling) is
        // valid and must detect, not yield a false "could not detect format".
        for (p, want) in [
            ("Config.JSON", Format::Json),
            ("data.YAML", Format::Yaml),
            ("app.YML", Format::Yaml),
            ("Cargo.TOML", Format::Toml),
            ("App.CSPROJ", Format::Xml),
            ("build.XML", Format::Xml),
            ("main.TF", Format::Hcl),
            ("settings.INI", Format::Ini),
            ("db.PROPERTIES", Format::Properties),
        ] {
            assert_eq!(
                Format::detect_from_path(Path::new(p)),
                Some(want),
                "{p} should detect case-insensitively"
            );
        }
        // The `.env` family is case-folded too.
        assert_eq!(
            Format::detect_from_path(Path::new(".ENV")),
            Some(Format::Dotenv),
            ".ENV should detect as dotenv"
        );
    }

    #[test]
    fn well_known_config_files_detect_by_name() {
        use std::path::Path;
        // Extension-less / name-identified config files auto-detect by filename.
        for (p, want) in [
            (".editorconfig", Format::Ini),
            ("Pipfile", Format::Toml),
            ("web.config", Format::Xml),
            ("app.config", Format::Xml),
            ("WEB.CONFIG", Format::Xml), // case-folded
        ] {
            assert_eq!(
                Format::detect_from_path(Path::new(p)),
                Some(want),
                "{p} should detect by name"
            );
        }
        // `Pipfile.lock` is JSON, NOT TOML -- the exact-name match must not catch it
        // (and we don't auto-detect it, so it stays None rather than mis-detecting).
        assert_eq!(Format::detect_from_path(Path::new("Pipfile.lock")), None);
        // Ambiguous dotfiles (JSON or YAML) are deliberately NOT auto-detected.
        assert_eq!(Format::detect_from_path(Path::new(".eslintrc")), None);
        assert_eq!(Format::detect_from_path(Path::new(".prettierrc")), None);
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
            // Delimiter / unary nesting -- caught by the depth guard (small inputs).
            format!("x = {}1{}", "(".repeat(d), ")".repeat(d)), // parens
            format!("x = {}1{}", "f(".repeat(d), ")".repeat(d)), // function calls
            format!("x = {}1", "-".repeat(d)),                  // unary minus
            format!("x = {}true", "!".repeat(d)),               // unary not
            format!("x = \"${{{}1{}}}\"", "(".repeat(d), ")".repeat(d)), // interp parens
            format!("x = \"${{{}1{}}}\"", "[".repeat(d), "]".repeat(d)), // interp tuple
            format!("x = <<EOT\n${{{}1{}}}\nEOT\n", "[".repeat(d), "]".repeat(d)), // heredoc interp
            // Delimiter-less OPERATOR chains -- these carry no `{`/`[`/`(` and no
            // unary run, so ONLY the byte cap stops them. Each is in the ~150 KB+
            // overflow band that a naive cap (256 KB) let crash (audit finding);
            // 64 KB rejects them. Regression guards against re-widening the cap.
            format!("x = 1{}", "+1".repeat(80_000)), // binary chain (~160 KB)
            format!("x = {}1", "1?1:".repeat(65_000)), // ternary chain
            format!("x = \"${{1{}}}\"", "+1".repeat(131_000)), // binary inside interpolation
            format!("x = <<EOT\n${{1{}}}\nEOT\n", "+1".repeat(131_000)), // binary inside heredoc interp
        ];
        for b in &bombs {
            assert!(
                Format::Hcl.parse(b).is_err(),
                "an expression bomb must be a parse error, not a crash"
            );
        }
        // A bounded operator chain UNDER the cap still parses (no over-reject of a
        // normal file), at a depth well inside the parse-thread stack ceiling.
        assert!(
            Format::Hcl
                .parse(&format!("x = 1{}", "+1".repeat(10_000)))
                .is_ok(),
            "a bounded binary chain parses"
        );
    }

    #[test]
    fn hcl_at_cap_operator_chain_parses_on_the_stack_thread() {
        // CAP-RELATIVE companion to the bomb battery: a densest (~2 B/level)
        // operator chain sized right up to `MAX_HCL_BYTES` is NOT size-rejected, so
        // it reaches the parser and MUST parse without overflowing the 512 MB parse
        // thread. This ties the margin to the CONSTANT itself: if `MAX_HCL_BYTES`
        // is ever widened past the empirical overflow point, this builds a crashing
        // chain and ABORTS the test binary loudly -- catching an unsafe re-widening
        // even if the compile-time `SAFE_MAX_HCL_BYTES` ceiling were miscalibrated
        // by hcl-rs frame-size drift. (The fixed-size bombs above all sit ABOVE the
        // cap, so they can't catch a re-widening on their own -- the audit gap.)
        let n = (MAX_HCL_BYTES - "x = 1".len()) / 2; // "+1" is 2 bytes per level
        let at_cap = format!("x = 1{}", "+1".repeat(n));
        assert!(
            at_cap.len() <= MAX_HCL_BYTES,
            "the chain must be within the cap so it reaches the parser"
        );
        assert!(
            Format::Hcl.parse(&at_cap).is_ok(),
            "a cap-sized operator chain must parse, not overflow the stack"
        );
    }

    #[test]
    fn xml_at_max_depth_parses_on_a_worker_sized_stack() {
        // The depth REJECTION path is tested elsewhere; this gates the other half
        // -- that a document AT `MAX_XML_DEPTH` actually PARSES without overflowing
        // on the stack alint really parses XML on (a ~2 MiB rayon worker), not just
        // on the test harness's larger stack. roxmltree recurses ~1 frame/element.
        // NOTE this proves at-limit safety for the CURRENT value; it does NOT by
        // itself catch a re-widening: a 2 MiB debug stack survives to ~350 deep, so
        // a regression to e.g. 256 would still PASS here. The compile-time
        // `SAFE_MAX_XML_DEPTH = 160` assert is what blocks such a raise (`256 > 160`
        // = build error); this runtime test is the empirical at-limit backstop that
        // the ceiling isn't fantasy.
        let deep = format!(
            "{}x{}",
            "<a>".repeat(MAX_XML_DEPTH),
            "</a>".repeat(MAX_XML_DEPTH)
        );
        let ok = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || Format::Xml.parse(&deep).is_ok())
            .expect("spawn worker-sized thread")
            .join()
            .expect("the parse thread must return, not abort the process");
        assert!(
            ok,
            "an at-limit XML document must parse on a 2 MiB worker stack"
        );
    }

    #[test]
    fn every_format_tolerates_bom_and_empty_input() {
        // GATE for the uniform BOM-strip + empty->{} handling at the top of
        // `Format::parse`. A BOM-prefixed file must parse IDENTICALLY to its
        // un-BOM'd form (JSON / HCL previously rejected a BOM as a syntax error),
        // and an empty / whitespace-only file must parse to an empty OBJECT for
        // EVERY format -- so `*_path_absent` is satisfied and `json_schema_passes`
        // validates it as `{}` (which passes `{"type":"object"}`), NOT the
        // false-firing `null` the first cut returned. Iterates `Format::ALL`, so a
        // newly-added format cannot silently skip this contract.
        let empty_doc = Value::Object(serde_json::Map::new());
        for &f in Format::ALL {
            assert_eq!(
                f.parse(""),
                Ok(empty_doc.clone()),
                "{} empty input must be an empty object",
                f.label()
            );
            assert_eq!(
                f.parse("   \n\t  "),
                Ok(empty_doc.clone()),
                "{} whitespace-only input must be an empty object",
                f.label()
            );
            // A BOM interleaved with whitespace (a stray BOM that `trim_start_matches`
            // alone leaves behind) on otherwise-empty input must ALSO be an empty
            // object, not a one-char scalar (which YAML/properties/INI would produce).
            assert_eq!(
                f.parse("\u{feff} \u{feff}\t"),
                Ok(empty_doc.clone()),
                "{} BOM-and-whitespace-only input must be an empty object",
                f.label()
            );
        }
        // A minimal valid document per format; a single OR double BOM prefix must
        // not change the parse. The sample table MUST cover every `Format::ALL`
        // variant (asserted), so a new format forces a BOM sample here too.
        let samples: &[(Format, &str)] = &[
            (Format::Json, "{\"k\":\"v\"}"),
            (Format::Yaml, "k: v"),
            (Format::Toml, "k = \"v\""),
            (Format::Xml, "<r><k>v</k></r>"),
            (Format::Dotenv, "k=v"),
            (Format::Properties, "k=v"),
            (Format::Ini, "[s]\nk=v"),
            (Format::Hcl, "k = \"v\""),
        ];
        let covered: std::collections::BTreeSet<&str> =
            samples.iter().map(|(f, _)| f.label()).collect();
        let all: std::collections::BTreeSet<&str> = Format::ALL.iter().map(|f| f.label()).collect();
        assert_eq!(
            covered, all,
            "the BOM sample table must cover every Format::ALL variant"
        );
        for &(f, doc) in samples {
            let plain = f.parse(doc);
            assert!(plain.is_ok(), "{} sample must parse: {plain:?}", f.label());
            assert_eq!(
                f.parse(&format!("\u{feff}{doc}")),
                plain,
                "{} single-BOM must parse identically",
                f.label()
            );
            assert_eq!(
                f.parse(&format!("\u{feff}\u{feff}{doc}")),
                plain,
                "{} double-BOM must parse identically (no residual mis-parse)",
                f.label()
            );
        }
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
    fn duplicate_keys_diverge_by_format_as_documented() {
        // GATE for the "Duplicate keys differ" caveat in docs/rules.md: a key
        // repeated within one scope resolves three incompatible ways, and a rule
        // author needs to know which. Pinning it here keeps the doc honest and
        // catches a parser swap that changes the behavior (e.g. a serde_yaml that
        // starts rejecting dup keys would move YAML from keep-last to reject).
        //
        // Keep-LAST (earlier value invisible):
        assert_eq!(
            Format::Json.parse("{\"a\":1,\"a\":2}").unwrap()["a"],
            serde_json::json!(2),
            "JSON keeps the last duplicate key"
        );
        assert_eq!(
            Format::Yaml.parse("a: 1\na: 2\n").unwrap()["a"],
            serde_json::json!(2),
            "YAML (serde_yaml_ng) keeps the last duplicate key"
        );
        assert_eq!(
            Format::Dotenv.parse("a=1\na=2\n").unwrap()["a"],
            serde_json::json!("2"),
            "dotenv keeps the last duplicate key"
        );
        assert_eq!(
            Format::Properties.parse("a=1\na=2\n").unwrap()["a"],
            serde_json::json!("2"),
            "properties keeps the last duplicate key"
        );
        // REJECT (parse error):
        assert!(
            Format::Toml.parse("a = 1\na = 2\n").is_err(),
            "TOML rejects a duplicate key"
        );
        assert!(
            Format::Hcl.parse("a = 1\na = 2\n").is_err(),
            "HCL rejects a duplicate attribute"
        );
        // Document-order ARRAY (both values queryable):
        assert_eq!(
            Format::Xml.parse("<r><a>1</a><a>2</a></r>").unwrap()["r"]["a"],
            serde_json::json!(["1", "2"]),
            "XML collects repeated elements into a document-order array"
        );
        assert_eq!(
            Format::Ini.parse("[s]\na=1\na=2\n").unwrap()["s"]["a"],
            serde_json::json!(["1", "2"]),
            "INI collects a repeated key into a document-order array"
        );
    }

    #[test]
    fn value_mapping_caveats_are_accurate() {
        // GATE for the "Value-mapping caveats" block in docs/rules.md. That block
        // has drifted from the real parser twice (a linter that documents a footgun
        // must document the RIGHT footgun), so pin every claim here: a doc/parser
        // mismatch now fails CI. (Duplicate-key divergence has its own test above.)
        use serde_json::json;

        // Stringly-typed formats store even a numeric literal as a STRING -- the root
        // cause of "numeric/boolean filter comparators no-op on XML/dotenv/properties/
        // INI"; a typed format keeps the number.
        assert_eq!(
            Format::Dotenv.parse("PORT=8080").unwrap()["PORT"],
            json!("8080")
        );
        assert_eq!(
            Format::Json.parse("{\"PORT\":8080}").unwrap()["PORT"],
            json!(8080)
        );

        // inf/nan: TOML (`inf`/`nan`) and YAML (`.inf`/`.nan`) -> null; HCL -> opaque
        // expression string (NOT an error); JSON has no such literal. NB YAML's bare
        // `nan` (no dot) is a plain string -- only the dotted form is a float.
        assert_eq!(Format::Toml.parse("a = inf").unwrap()["a"], json!(null));
        assert_eq!(Format::Toml.parse("a = nan").unwrap()["a"], json!(null));
        assert_eq!(Format::Yaml.parse("a: .inf").unwrap()["a"], json!(null));
        assert_eq!(Format::Yaml.parse("a: .nan").unwrap()["a"], json!(null));
        assert_eq!(Format::Yaml.parse("a: nan").unwrap()["a"], json!("nan"));
        assert_eq!(Format::Hcl.parse("a = inf").unwrap()["a"], json!("${inf}"));
        assert_eq!(Format::Hcl.parse("a = nan").unwrap()["a"], json!("${nan}"));

        // Empty value: null in XML/YAML, "" in json/toml/dotenv/ini/properties.
        assert_eq!(
            Format::Xml.parse("<r><a/></r>").unwrap()["r"]["a"],
            json!(null)
        );
        assert_eq!(Format::Yaml.parse("a:").unwrap()["a"], json!(null));
        assert_eq!(Format::Json.parse("{\"a\":\"\"}").unwrap()["a"], json!(""));
        assert_eq!(Format::Toml.parse("a = \"\"").unwrap()["a"], json!(""));
        assert_eq!(Format::Dotenv.parse("a=").unwrap()["a"], json!(""));
        assert_eq!(Format::Ini.parse("[s]\nk=").unwrap()["s"]["k"], json!(""));
        assert_eq!(Format::Properties.parse("k=").unwrap()["k"], json!(""));

        // A TOML date/time is a magic-key OBJECT, not a string; YAML's is a string.
        assert_eq!(
            Format::Toml.parse("a = 1979-05-27").unwrap()["a"],
            json!({ "$__toml_private_datetime": "1979-05-27" })
        );
        assert_eq!(
            Format::Yaml.parse("a: 2002-12-14").unwrap()["a"],
            json!("2002-12-14")
        );

        // Based / underscored numbers: TOML parses hex/octal/binary + underscores;
        // YAML parses the base prefixes but keeps an underscored decimal as a string;
        // HCL REJECTS non-decimal literals; the stringly formats keep them as strings.
        assert_eq!(Format::Toml.parse("a = 0x1F").unwrap()["a"], json!(31));
        assert_eq!(Format::Toml.parse("a = 0o17").unwrap()["a"], json!(15));
        assert_eq!(Format::Toml.parse("a = 0b101").unwrap()["a"], json!(5));
        assert_eq!(Format::Toml.parse("a = 1_000").unwrap()["a"], json!(1000));
        assert_eq!(Format::Yaml.parse("a: 0x1F").unwrap()["a"], json!(31));
        assert_eq!(Format::Yaml.parse("a: 0o17").unwrap()["a"], json!(15));
        assert_eq!(Format::Yaml.parse("a: 0b101").unwrap()["a"], json!(5));
        assert_eq!(Format::Yaml.parse("a: 1_000").unwrap()["a"], json!("1_000"));
        assert!(Format::Hcl.parse("a = 0x1F").is_err());
        assert!(Format::Hcl.parse("a = 1_000").is_err());
        assert_eq!(
            Format::Ini.parse("[s]\nport=8080").unwrap()["s"]["port"],
            json!("8080")
        );
        assert_eq!(
            Format::Xml.parse("<r><n>42</n></r>").unwrap()["r"]["n"],
            json!("42")
        );

        // Big integer beyond i64: lossy float in JSON, parse error in YAML/TOML/HCL.
        assert!(Format::Json.parse("{\"a\":100000000000000000000}").unwrap()["a"].is_f64());
        assert!(Format::Yaml.parse("a: 100000000000000000000").is_err());
        assert!(Format::Toml.parse("a = 100000000000000000000").is_err());
        assert!(Format::Hcl.parse("a = 100000000000000000000").is_err());
    }

    #[test]
    fn oversize_structured_input_is_rejected_before_building_a_tree() {
        // A structured tree is ~16-19x the input, so an input over MAX_STRUCTURED_BYTES
        // is rejected at the size check (no tree built, no multi-GB balloon). The
        // rejection is on `.len()` alone, so no parse runs for the oversize input.
        let over = "x".repeat(MAX_STRUCTURED_BYTES + 1);
        for f in [Format::Json, Format::Yaml, Format::Xml, Format::Toml] {
            let err = f.parse(&over).unwrap_err();
            assert!(
                err.contains("maximum supported size"),
                "{} oversize input rejected: {err}",
                f.label()
            );
        }
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
