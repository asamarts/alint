//! Shared structured / line / regex extraction for the
//! manifest-driven cross-file rules (`registry_paths_resolve`,
//! `cross_file`, `file_graph`) and core-side predicates. One place so
//! the one-of decode (`serde_yaml` can't decode an externally-tagged
//! enum from a `{ key: value }` map; an untagged enum can't tell the
//! three `JSONPath` string variants apart) and the non-literal skip
//! can't drift between consumers.

use regex::Regex;
use serde::Deserialize;
use serde_json_path::JsonPath;

use crate::structured_format::Format;

/// Runtime extraction mode, resolved from [`ExtractSpec`].
#[derive(Debug, Clone)]
pub enum Extract {
    /// Structured-query (RFC 9535 `JSONPath` over the parsed tree).
    Toml(String),
    Json(String),
    Yaml(String),
    /// One path per non-blank, non-comment line.
    Lines(LinesOpts),
    /// Capture group 1 of each match is the value.
    Regex(String),
    /// The whole file content as a single value (e.g. for a
    /// `cross_file` `equals` + `normalize` whole-file compare).
    WholeFile,
}

/// Exactly one of: toml/json/yaml (RFC 9535 `JSONPath` string), lines (object;
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
            ("lines", self.lines.is_some()),
            ("regex", self.regex.is_some()),
            ("whole_file", self.whole_file.is_some()),
        ]
        .into_iter()
        .filter_map(|(n, on)| on.then_some(n))
        .collect();
        match set.as_slice() {
            [] => Err(
                "`extract` must set exactly one of toml/json/yaml/lines/regex/whole_file (none set)"
                    .to_string(),
            ),
            [_] => Ok(if let Some(q) = self.toml {
                Extract::Toml(q)
            } else if let Some(q) = self.json {
                Extract::Json(q)
            } else if let Some(q) = self.yaml {
                Extract::Yaml(q)
            } else if let Some(o) = self.lines {
                Extract::Lines(o)
            } else if let Some(q) = self.regex {
                Extract::Regex(q)
            } else {
                Extract::WholeFile
            }),
            many => Err(format!(
                "`extract` must set exactly one of toml/json/yaml/lines/regex/whole_file (got {})",
                many.join(", ")
            )),
        }
    }
}

impl From<Extract> for ExtractSpec {
    fn from(e: Extract) -> Self {
        let mut s = ExtractSpec::default();
        match e {
            Extract::Toml(q) => s.toml = Some(q),
            Extract::Json(q) => s.json = Some(q),
            Extract::Yaml(q) => s.yaml = Some(q),
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
        Extract::Toml(q) => structured(Format::Toml, q, text)?,
        Extract::Json(q) => structured(Format::Json, q, text)?,
        Extract::Yaml(q) => structured(Format::Yaml, q, text)?,
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
}
