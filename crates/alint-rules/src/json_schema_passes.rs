//! `json_schema_passes` — assert that a set of JSON / YAML /
//! TOML files validates against a JSON Schema.
//!
//! Closes the last unshipped structured-query primitive
//! (`json_path_*` shipped in v0.4.4). JSON Schema sees use far
//! beyond JSON itself: many YAML configs (Kubernetes, GitHub
//! Actions workflows, Helm chart `values.schema.json`) carry a
//! schema, and TOML manifests (Cargo, pyproject) increasingly
//! ship one too. This rule lets a project enforce its own
//! schemas alongside upstream-supplied ones.
//!
//! ## Behaviour
//!
//! - **`schema_path`** points at a JSON Schema file relative to
//!   the lint root. Schema is loaded + compiled lazily on the
//!   first `evaluate()` call and cached on the rule struct
//!   (`OnceLock`); a malformed schema produces one
//!   repository-level violation rather than one violation per
//!   target file.
//! - The target's format is auto-detected from its extension
//!   (`.json` / `.yaml` / `.yml` / `.toml`); pass `format:` to
//!   override. YAML and TOML coerce through serde into the
//!   same `serde_json::Value` tree the schema validates against
//!   — same trick `json_path_*` uses.
//! - Each schema-validation error becomes one violation, with
//!   the message including the failing instance path and the
//!   schema's error description. A target that fails to parse
//!   produces one parse-error violation, not a flood of schema
//!   errors against junk.
//!
//! Check-only — fixing schema violations is a "the user knows
//! what value belongs there" problem, not alint's.

use std::path::PathBuf;
use std::sync::OnceLock;

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use jsonschema::Validator;
use serde::Deserialize;
use serde_json::Value;

use crate::structured_path::Format;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Path to a JSON Schema file, relative to the lint root.
    /// JSON only — even when validating YAML / TOML targets,
    /// the schema document itself must be JSON. Most upstream
    /// schemas (Cargo's, GitHub Actions') ship as JSON anyway.
    schema_path: PathBuf,
    /// Override the auto-detected target file format. One of
    /// `json` / `yaml` / `toml`. When omitted, the format is
    /// detected from the target file's extension; targets with
    /// no detectable extension produce a per-file violation.
    #[serde(default)]
    format: Option<String>,
}

#[derive(Debug)]
pub struct JsonSchemaPassesRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    schema_path: PathBuf,
    /// Explicit format, if the user passed `format:`. When
    /// `None`, the format is detected per-file from the
    /// extension.
    format_override: Option<Format>,
    /// Schema is loaded + compiled at most once per `Rule`
    /// instance. The `Result` lets us cache load failures so a
    /// missing or malformed schema produces a single
    /// repository-level violation rather than re-attempting the
    /// load per matched file.
    compiled: OnceLock<std::result::Result<Validator, String>>,
}

impl Rule for JsonSchemaPassesRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        self.level
    }
    fn policy_url(&self) -> Option<&str> {
        self.policy_url.as_deref()
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();

        let schema_abs = ctx.root.join(&self.schema_path);
        let validator_res = self.compiled.get_or_init(|| compile_schema(&schema_abs));
        let validator = match validator_res {
            Ok(v) => v,
            Err(msg) => {
                // Schema unusable → one repository-level
                // violation, then we're done. Per-file
                // validation against a broken schema would
                // dump the same error N times.
                violations.push(Violation::new(msg.clone()));
                return Ok(violations);
            }
        };

        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path, ctx.index) {
                continue;
            }
            let full = ctx.root.join(&entry.path);
            let Ok(text) = std::fs::read_to_string(&full) else {
                // Permission / race — silent skip, like other
                // content rules.
                continue;
            };

            let Some(format) = self
                .format_override
                .or_else(|| Format::detect_from_path(&entry.path))
            else {
                violations.push(
                    Violation::new(
                        "could not detect format from extension; pass `format:` \
                         (`json` / `yaml` / `toml`) on the rule",
                    )
                    .with_path(entry.path.clone()),
                );
                continue;
            };

            let parsed = match format.parse(&text) {
                Ok(v) => v,
                Err(err) => {
                    violations.push(
                        Violation::new(format!("not a valid {} document: {err}", format.label()))
                            .with_path(entry.path.clone()),
                    );
                    continue;
                }
            };

            for error in validator.iter_errors(&parsed) {
                let detail = format!("schema violation at `{}`: {error}", error.instance_path);
                let msg = self.message.clone().unwrap_or(detail);
                violations.push(Violation::new(msg).with_path(entry.path.clone()));
            }
        }
        Ok(violations)
    }
}

fn compile_schema(schema_abs: &std::path::Path) -> std::result::Result<Validator, String> {
    let bytes = std::fs::read(schema_abs)
        .map_err(|e| format!("could not read schema {}: {e}", schema_abs.display()))?;
    let schema_value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("schema {} is not valid JSON: {e}", schema_abs.display()))?;
    jsonschema::validator_for(&schema_value).map_err(|e| {
        format!(
            "schema {} is not a valid JSON Schema: {e}",
            schema_abs.display()
        )
    })
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let _paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(&spec.id, "json_schema_passes requires a `paths` field")
    })?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;

    let format_override = match opts.format.as_deref() {
        None => None,
        Some("json") => Some(Format::Json),
        Some("yaml" | "yml") => Some(Format::Yaml),
        Some("toml") => Some(Format::Toml),
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!("unknown format `{other}`; expected json | yaml | toml"),
            ));
        }
    };

    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "json_schema_passes has no fix op — alint can't synthesize correct content",
        ));
    }

    Ok(Box::new(JsonSchemaPassesRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        schema_path: opts.schema_path,
        format_override,
        compiled: OnceLock::new(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn compile(schema: &Value) -> Validator {
        jsonschema::validator_for(schema).unwrap()
    }

    #[test]
    fn passing_value_produces_no_errors() {
        let v = compile(&json!({
            "type": "object",
            "required": ["name"],
            "properties": { "name": { "type": "string" } }
        }));
        let instance = json!({ "name": "alint" });
        let errors: Vec<_> = v.iter_errors(&instance).collect();
        assert!(errors.is_empty());
    }

    #[test]
    fn missing_required_field_yields_error() {
        let v = compile(&json!({
            "type": "object",
            "required": ["name"],
        }));
        let instance = json!({});
        let errors: Vec<_> = v.iter_errors(&instance).collect();
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn type_mismatch_yields_error() {
        let v = compile(&json!({
            "type": "object",
            "properties": { "n": { "type": "integer" } },
            "required": ["n"]
        }));
        let instance = json!({ "n": "not an integer" });
        let errors: Vec<_> = v.iter_errors(&instance).collect();
        assert!(!errors.is_empty());
    }

    #[test]
    fn yaml_value_round_trips_through_validator() {
        // Same schema as above; instance comes via YAML →
        // serde_json::Value, mirroring how the rule itself
        // hands targets to the validator.
        let v = compile(&json!({
            "type": "object",
            "required": ["name"],
            "properties": { "name": { "type": "string" } }
        }));
        let yaml = "name: from-yaml\n";
        let instance = Format::Yaml.parse(yaml).unwrap();
        let errors: Vec<_> = v.iter_errors(&instance).collect();
        assert!(errors.is_empty());
    }

    #[test]
    fn toml_value_round_trips_through_validator() {
        let v = compile(&json!({
            "type": "object",
            "required": ["name"],
            "properties": { "name": { "type": "string" } }
        }));
        let toml_text = "name = \"from-toml\"\n";
        let instance = Format::Toml.parse(toml_text).unwrap();
        let errors: Vec<_> = v.iter_errors(&instance).collect();
        assert!(errors.is_empty());
    }

    #[test]
    fn compile_fails_loudly_on_missing_file() {
        let bogus = std::path::PathBuf::from("/nonexistent/schema.json");
        let res = compile_schema(&bogus);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("could not read schema"));
    }

    #[test]
    fn compile_fails_loudly_on_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("schema.json");
        std::fs::write(&path, "{ this is not json").unwrap();
        let res = compile_schema(&path);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("not valid JSON"));
    }

    #[test]
    fn compile_fails_loudly_on_invalid_schema() {
        // Valid JSON but not a valid JSON Schema (type must be
        // a string or array of strings, not a number).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("schema.json");
        std::fs::write(&path, r#"{"type": 12345}"#).unwrap();
        let res = compile_schema(&path);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("not a valid JSON Schema"));
    }

    #[test]
    fn detect_from_path_handles_standard_extensions() {
        assert_eq!(
            Format::detect_from_path(std::path::Path::new("a.json")),
            Some(Format::Json)
        );
        assert_eq!(
            Format::detect_from_path(std::path::Path::new("a.yaml")),
            Some(Format::Yaml)
        );
        assert_eq!(
            Format::detect_from_path(std::path::Path::new("a.yml")),
            Some(Format::Yaml)
        );
        assert_eq!(
            Format::detect_from_path(std::path::Path::new("a.toml")),
            Some(Format::Toml)
        );
        assert_eq!(
            Format::detect_from_path(std::path::Path::new("a.txt")),
            None
        );
        assert_eq!(
            Format::detect_from_path(std::path::Path::new("Makefile")),
            None
        );
    }

    #[test]
    fn scope_filter_narrows() {
        use crate::test_support::{ctx, spec_yaml, tempdir_with_files};
        // Two JSON files that fail the schema; only the one
        // inside a directory with `marker.lock` as ancestor
        // should fire.
        let (tmp, idx) = tempdir_with_files(&[
            ("schema.json", br#"{"type":"object","required":["x"]}"#),
            ("pkg/marker.lock", b""),
            ("pkg/bad.json", b"{}"),
            ("other/bad.json", b"{}"),
        ]);
        let spec = spec_yaml(
            "id: t\n\
             kind: json_schema_passes\n\
             paths: \"**/bad.json\"\n\
             schema_path: schema.json\n\
             scope_filter:\n  \
               has_ancestor: marker.lock\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "only in-scope file should fire: {v:?}");
        assert_eq!(
            v[0].path.as_deref(),
            Some(std::path::Path::new("pkg/bad.json"))
        );
    }
}
