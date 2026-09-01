//! Shared structured / line / regex extraction for the
//! manifest-driven cross-file rules (`registry_paths_resolve`,
//! `cross_file`, `file_graph`) and core-side predicates. One place so
//! the one-of decode (`serde_yaml` can't decode an externally-tagged
//! enum from a `{ key: value }` map; an untagged enum can't tell the
//! structured `JSONPath` string variants apart) and the non-literal skip
//! can't drift between consumers.

use regex::Regex;
use serde::Deserialize;
use serde_json_path::JsonPath;

use crate::structured_format::Format;

/// Runtime extraction mode, resolved from [`ExtractSpec`].
#[derive(Debug, Clone)]
pub enum Extract {
    /// Structured-query (RFC 9535 `JSONPath` over the parsed tree) against any
    /// [`Format`]; dispatch runs `Format::parse` then applies the query, so a
    /// new format reaches every structured consumer without a new variant here.
    Structured(Format, String),
    /// One path per non-blank, non-comment line.
    Lines(LinesOpts),
    /// Capture group 1 of each match is the value.
    Regex(String),
    /// The whole file content as a single value (e.g. for a
    /// `cross_file` `equals` + `normalize` whole-file compare).
    WholeFile,
}

/// Exactly one of: toml/json/yaml/xml/dotenv/properties (RFC 9535 `JSONPath` string), lines (object;
/// optional `comment` prefix, default `#`), regex (string; capture group 1 is
/// the value), `whole_file` (object `{}`; the entire file content as one value,
/// for byte-level `cross_file` comparison; the non-literal skip does not apply).
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "extract_spec", extend("minProperties" = 1, "maxProperties" = 1))]
pub struct ExtractSpec {
    #[serde(default)]
    toml: Option<String>,
    #[serde(default)]
    json: Option<String>,
    #[serde(default)]
    yaml: Option<String>,
    #[serde(default)]
    xml: Option<String>,
    #[serde(default)]
    dotenv: Option<String>,
    #[serde(default)]
    properties: Option<String>,
    #[serde(default)]
    lines: Option<LinesOpts>,
    #[serde(default)]
    regex: Option<String>,
    #[serde(default)]
    whole_file: Option<WholeFileOpts>,
}

impl ExtractSpec {
    pub fn resolve(self) -> std::result::Result<Extract, String> {
        let set: Vec<&str> = [
            ("toml", self.toml.is_some()),
            ("json", self.json.is_some()),
            ("yaml", self.yaml.is_some()),
            ("xml", self.xml.is_some()),
            ("dotenv", self.dotenv.is_some()),
            ("properties", self.properties.is_some()),
            ("lines", self.lines.is_some()),
            ("regex", self.regex.is_some()),
            ("whole_file", self.whole_file.is_some()),
        ]
        .into_iter()
        .filter_map(|(n, on)| on.then_some(n))
        .collect();
        match set.as_slice() {
            [] => Err(
                "`extract` must set exactly one of toml/json/yaml/xml/dotenv/properties/lines/regex/whole_file (none set)"
                    .to_string(),
            ),
            [_] => Ok(if let Some(q) = self.toml {
                Extract::Structured(Format::Toml, q)
            } else if let Some(q) = self.json {
                Extract::Structured(Format::Json, q)
            } else if let Some(q) = self.yaml {
                Extract::Structured(Format::Yaml, q)
            } else if let Some(q) = self.xml {
                Extract::Structured(Format::Xml, q)
            } else if let Some(q) = self.dotenv {
                Extract::Structured(Format::Dotenv, q)
            } else if let Some(q) = self.properties {
                Extract::Structured(Format::Properties, q)
            } else if let Some(o) = self.lines {
                Extract::Lines(o)
            } else if let Some(q) = self.regex {
                Extract::Regex(q)
            } else {
                Extract::WholeFile
            }),
            many => Err(format!(
                "`extract` must set exactly one of toml/json/yaml/xml/dotenv/properties/lines/regex/whole_file (got {})",
                many.join(", ")
            )),
        }
    }
}

impl From<Extract> for ExtractSpec {
    fn from(e: Extract) -> Self {
        let mut s = ExtractSpec::default();
        match e {
            Extract::Structured(Format::Toml, q) => s.toml = Some(q),
            Extract::Structured(Format::Json, q) => s.json = Some(q),
            Extract::Structured(Format::Yaml, q) => s.yaml = Some(q),
            Extract::Structured(Format::Xml, q) => s.xml = Some(q),
            Extract::Structured(Format::Dotenv, q) => s.dotenv = Some(q),
            Extract::Structured(Format::Properties, q) => s.properties = Some(q),
            Extract::Lines(o) => s.lines = Some(o),
            Extract::Regex(q) => s.regex = Some(q),
            Extract::WholeFile => s.whole_file = Some(WholeFileOpts::default()),
        }
        s
    }
}

/// `whole_file:` carries no options today (an empty `{}` map, like a
/// marker); kept as a struct so options can be added without a
/// breaking change.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WholeFileOpts {}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LinesOpts {
    /// Lines starting with this (after trim) are skipped.
    #[serde(default = "default_comment")]
    pub(crate) comment: String,
}

fn default_comment() -> String {
    "#".to_string()
}

// `#[serde(default = "default_comment")]` only fires on the
// deserialize path; `LinesOpts::default()` (used by the
// `Lines(#[serde(default)] …)` variant and tests) needs the
// same `#` default, so derive can't be used here.
impl Default for LinesOpts {
    fn default() -> Self {
        Self {
            comment: default_comment(),
        }
    }
}

/// True when `entry` is a *computed* value (interpolation /
/// concatenation), which the caller skips rather than checks.
/// Genuine markers only: shell/Nix `${var}` and `$(cmd)`,
/// mustache/jinja `{{ … }}`, string concatenation `"a" + b`.
/// A bare `$`, backtick, or `(.` is legal in a real filename, so
/// it is **not** treated as non-literal — over-matching those
/// silently dropped real literal paths (a false negative; v0.10
/// post-audit P2). The skip never fails the rule and is
/// intentionally silent; visibly surfacing skipped entries is a
/// tracked v0.11 item (`alint check` has no `--explain` /
/// informational-finding channel).
pub fn is_non_literal(entry: &str) -> bool {
    entry.contains("${") || entry.contains("$(") || entry.contains("{{") || entry.contains("+ ")
}

/// Every string match for `extract` over `text`, raw (the caller
/// applies [`is_non_literal`] filtering as it needs). Structured
/// modes yield string-valued `JSONPath` matches; `lines` yields
/// trimmed non-comment lines; `regex` yields capture group 1.
pub fn extract_values(extract: &Extract, text: &str) -> std::result::Result<Vec<String>, String> {
    Ok(match extract {
        Extract::Structured(fmt, q) => structured(*fmt, q, text)?,
        Extract::Lines(opts) => text
            .lines()
            .map(str::trim)
            .filter(|l| {
                if l.is_empty() {
                    return false;
                }
                if opts.comment.is_empty() {
                    return true;
                }
                !l.starts_with(opts.comment.as_str())
            })
            .map(ToString::to_string)
            .collect(),
        Extract::Regex(pat) => {
            let re = Regex::new(pat).map_err(|e| format!("bad regex: {e}"))?;
            re.captures_iter(text)
                .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
                .collect()
        }
        Extract::WholeFile => vec![text.to_string()],
    })
}

/// Run a structured-query (`Format::parse` + RFC 9535 `JSONPath`),
/// returning every string-valued match. Non-string nodes are
/// dropped (a value the manifest expresses as a table/array is
/// skipped, not failed).
fn structured(fmt: Format, query: &str, text: &str) -> std::result::Result<Vec<String>, String> {
    let value = fmt.parse(text)?;
    let path = JsonPath::parse(query).map_err(|e| format!("bad JSONPath {query:?}: {e}"))?;
    Ok(path
        .query(&value)
        .iter()
        .filter_map(|v| v.as_str().map(ToString::to_string))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::is_non_literal;

    #[test]
    fn genuine_interpolation_is_non_literal() {
        for e in [
            "${pkgs.foo}/bin",
            "$(date +%s)/x",
            "{{ pkg }}/lib",
            "crates/a + crates/b",
        ] {
            assert!(is_non_literal(e), "{e:?} must be non-literal");
        }
    }

    #[test]
    fn bare_dollar_backtick_dotparen_are_literal() {
        // v0.10 post-audit P2 regression: all legal in real
        // filenames — must be CHECKED, not silently skipped.
        for e in [
            "foo$bar.rs",
            "weird`name`.txt",
            "a/b (.c)/d",
            "./relative/path",
            "pkg-1.0",
            "crates/serde_json",
        ] {
            assert!(!is_non_literal(e), "{e:?} must be literal");
        }
    }

    #[test]
    fn whole_file_resolves_and_yields_full_text() {
        let spec: super::ExtractSpec =
            serde_yaml_ng::from_str("whole_file: {}").expect("parse whole_file spec");
        let extract = spec.resolve().expect("resolve whole_file");
        assert!(matches!(extract, super::Extract::WholeFile));
        let text = "line one\nline two\n";
        let got = super::extract_values(&extract, text).expect("extract whole file");
        assert_eq!(got, vec![text.to_string()]);
    }

    #[test]
    fn whole_file_conflicts_with_another_source() {
        let spec: super::ExtractSpec =
            serde_yaml_ng::from_str("whole_file: {}\nregex: '(x)'").expect("parse spec");
        assert!(spec.resolve().is_err(), "two sources must be rejected");
    }

    #[test]
    fn resolve_and_extract_over_xml() {
        // The `xml:` extract mode reuses `Format::Xml.parse` -- an XML leaf comes
        // back as its string value, exactly like the other structured modes.
        let spec: super::ExtractSpec =
            serde_yaml_ng::from_str("xml: \"$.Project.PropertyGroup.Version\"")
                .expect("parse xml spec");
        let extract = spec.resolve().expect("resolve xml");
        assert!(matches!(
            extract,
            super::Extract::Structured(crate::structured_format::Format::Xml, _)
        ));
        let xml = "<Project><PropertyGroup><Version>1.2.3</Version></PropertyGroup></Project>";
        assert_eq!(
            super::extract_values(&extract, xml).expect("extract xml"),
            vec!["1.2.3".to_string()]
        );
    }

    #[test]
    fn extract_spec_covers_every_format() {
        // Parity gate: every `Format::ALL` variant must have a structured extract
        // field, so a format wired into the rule family / schema can't be forgotten
        // here. The field names are lowercase; `label()` is uppercase, so case-fold.
        let schema = serde_json::to_value(schemars::schema_for!(super::ExtractSpec))
            .expect("ExtractSpec schema serializes");
        // schemars may inline the root or emit a $ref to $defs/extract_spec; read
        // properties from whichever carries them, so a schemars upgrade doesn't
        // silently turn this into a false RED.
        let props = schema
            .get("properties")
            .or_else(|| schema.pointer("/$defs/extract_spec/properties"))
            .and_then(|v| v.as_object())
            .expect("ExtractSpec schema exposes a properties object");
        for fmt in crate::structured_format::Format::ALL {
            let name = fmt.label().to_lowercase();
            assert!(
                props.contains_key(&name),
                "ExtractSpec is missing a `{name}` field for Format::{fmt:?} (format parity)"
            );
        }
    }

    #[test]
    fn xml_and_toml_both_set_is_rejected() {
        // The one-of guard still holds with the new `xml` field in the mix.
        let spec: super::ExtractSpec =
            serde_yaml_ng::from_str("xml: \"$.a\"\ntoml: \"$.b\"").expect("parse two-set spec");
        assert!(
            spec.resolve().is_err(),
            "two structured modes must be rejected"
        );
    }

    #[test]
    fn xml_extract_drops_non_string_nodes() {
        // A path landing on a subtree (object) is not a string, so it is dropped --
        // the same string-only filter the other structured modes apply.
        let extract = super::Extract::Structured(
            crate::structured_format::Format::Xml,
            "$.Project.PropertyGroup".into(),
        );
        let xml = "<Project><PropertyGroup><Version>1.0</Version></PropertyGroup></Project>";
        assert!(
            super::extract_values(&extract, xml)
                .expect("extract")
                .is_empty(),
            "an object node yields no string value"
        );
    }

    #[test]
    fn xml_extract_malformed_propagates_error() {
        // A malformed XML target surfaces as an extract error (the consumer turns
        // it into a per-file violation), same as the other structured modes.
        let extract =
            super::Extract::Structured(crate::structured_format::Format::Xml, "$.a".into());
        assert!(
            super::extract_values(&extract, "<a><unclosed>").is_err(),
            "malformed XML must surface as an extract error"
        );
    }

    #[test]
    fn xml_element_cardinality_is_data_dependent() {
        // XML has no array type: a single <Ref> is an object, but repeated <Ref>
        // become an array. So a `[*]` wildcard does NOT normalize cardinality --
        // the same query yields different results for one vs many, a documented
        // footgun (docs/rules.md #xml-mapping). Recursive descent (`$..`) is the
        // cardinality-independent idiom a rule should use when it must handle both.
        use crate::structured_format::Format;
        let xv = |q: &str, xml: &str| {
            super::extract_values(&super::Extract::Structured(Format::Xml, q.into()), xml).unwrap()
        };
        let one = r#"<Project><ItemGroup><Ref Include="x.csproj"/></ItemGroup></Project>"#;
        let many =
            r#"<Project><ItemGroup><Ref Include="a"/><Ref Include="b"/></ItemGroup></Project>"#;

        // The `[*]['@Include']` idiom (as used in the csproj scenarios) silently
        // extracts NOTHING for a single element -- the false-negative footgun ...
        assert!(
            xv("$.Project.ItemGroup.Ref[*]['@Include']", one).is_empty(),
            "single element + [*] is the documented silent-miss footgun"
        );
        // ... but works for two or more.
        assert_eq!(
            xv("$.Project.ItemGroup.Ref[*]['@Include']", many),
            vec!["a", "b"]
        );

        // Recursive descent from the element handles BOTH cardinalities -- the
        // idiom the docs recommend for cardinality-independent XML extraction.
        assert_eq!(
            xv("$.Project.ItemGroup.Ref..['@Include']", one),
            vec!["x.csproj"]
        );
        assert_eq!(
            xv("$.Project.ItemGroup.Ref..['@Include']", many),
            vec!["a", "b"]
        );
    }
}
