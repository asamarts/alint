//! `json_schema_passes` — assert that a set of JSON / YAML /
//! TOML / XML / dotenv / properties / INI / HCL files validates against a JSON Schema.
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
//!   (`.json` / `.yaml` / `.yml` / `.toml` / `.properties` / `.ini` /
//!   `.cfg` / `.hcl` / `.tf` / `.tfvars` / `.nomad` / `.xml` and the `.csproj` /
//!   `.props` / `.targets` family, or `.env` by filename); pass `format:`
//!   to override. YAML and TOML coerce through serde into the
//!   same `serde_json::Value` tree the schema validates against,
//!   and XML maps in via the xmltodict-style `xml_to_value`
//!   convention — same trick `json_path_*` uses.
//! - **XML / dotenv / properties / INI targets are stringly-typed.**
//!   Every value maps to a JSON string, so type those fields as
//!   `string` (with a `pattern`) — `type: integer` / `boolean` /
//!   `number` always fail against them. For XML, `type: array` /
//!   `object` additionally depend on cardinality (a single vs.
//!   repeated element is an object vs. an array). JSON / YAML /
//!   TOML / HCL keep native types. See the mapping notes in
//!   `docs/rules.md`.
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

use alint_core::{Context, Error, Format, Level, Result, Rule, RuleSpec, Scope, Violation};
use jsonschema::Validator;
use serde::Deserialize;
use serde_json::Value;

/// The `format:` override values for `json_schema_passes`: the same formats as
/// `structured_path::Format` (json / yaml / toml / xml / dotenv / properties / ini / hcl), plus `yml`
/// accepted as a `yaml` alias.
#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
enum TargetFormat {
    Json,
    Yaml,
    Yml,
    Toml,
    Xml,
    Dotenv,
    Properties,
    Ini,
    Hcl,
}

impl TargetFormat {
    fn to_format(self) -> Format {
        match self {
            TargetFormat::Json => Format::Json,
            TargetFormat::Yaml | TargetFormat::Yml => Format::Yaml,
            TargetFormat::Toml => Format::Toml,
            TargetFormat::Xml => Format::Xml,
            TargetFormat::Dotenv => Format::Dotenv,
            TargetFormat::Properties => Format::Properties,
            TargetFormat::Ini => Format::Ini,
            TargetFormat::Hcl => Format::Hcl,
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Path to a JSON Schema file relative to the lint root. The schema must
    /// itself be JSON even when validating YAML / TOML targets.
    schema_path: PathBuf,
    /// Override the auto-detected target format. When omitted, format is inferred
    /// from each target file's extension (.json / .yaml / .yml / .toml / .properties /
    /// .ini / .cfg / .hcl / .tf / .tfvars / .nomad / .xml and the .csproj / .props / .targets XML
    /// family), or by filename
    /// for the `.env` family.
    #[serde(default)]
    format: Option<TargetFormat>,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct JsonSchemaPassesRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    schema_path: PathBuf,
    /// Permit reading a `schema_path:` that escapes the repo root —
    /// set post-build from the top-level `allow_out_of_root:` policy.
    allow_out_of_root: bool,
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
    /// Expose the per-file scope so the engine resolves this rule's
    /// `scope_filter` (manifest sets, `changed_since:`) before dispatch and
    /// can `--changed`-skip it (see `Rule::path_scope`).
    fn path_scope(&self) -> Option<&Scope> {
        Some(&self.scope)
    }

    alint_core::rule_common_impl!();

    fn set_allow_out_of_root(&mut self, allow: bool) {
        self.allow_out_of_root = allow;
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();

        // Confine the (config-author-controlled) schema path before any
        // read: an absolute / `../../` `schema_path:` reads outside the
        // repo root only when the user's top-level config opted this rule
        // into `allow_out_of_root`.
        let schema_rel = match crate::pathsafe::confine_read(
            &self.schema_path,
            ctx.root,
            self.allow_out_of_root,
        ) {
            crate::pathsafe::Confined::In(p) => p,
            crate::pathsafe::Confined::AllowedEscape(p) => {
                violations.push(
                    Violation::new(crate::pathsafe::out_of_root_note(&self.schema_path))
                        .as_note()
                        .with_path(self.schema_path.clone()),
                );
                p
            }
            crate::pathsafe::Confined::Denied => {
                violations.push(
                    Violation::new(format!(
                        "schema path {} escapes the repo root",
                        self.schema_path.display()
                    ))
                    .with_path(self.schema_path.clone()),
                );
                return Ok(violations);
            }
        };
        let schema_abs = ctx.root.join(&schema_rel);
        let validator_res = self.compiled.get_or_init(|| compile_schema(&schema_abs));
        let validator = match validator_res {
            Ok(v) => v,
            Err(msg) => {
                // Schema unusable → one repository-level
                // violation, then we're done. Per-file
                // validation against a broken schema would
                // dump the same error N times.
                // No path (a repo-level schema-load failure) → a fixed key so
                // the fingerprint doesn't fall through to the volatile message.
                violations.push(
                    Violation::new(msg.clone())
                        .with_baseline_key("json-schema-passes-schema-unusable"),
                );
                return Ok(violations);
            }
        };

        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path, ctx.index) {
                continue;
            }
            let full = ctx.root.join(&entry.path);
            let text = match crate::io::read_capped(&full) {
                Ok(b) => String::from_utf8_lossy(&b).into_owned(),
                Err(crate::io::ReadCapError::TooLarge(n)) => {
                    // Over the 256 MiB whole-file cap — surface
                    // a clear violation rather than the previous
                    // silent skip (which masked an OOM-DoS
                    // surface on hostile / accidental multi-GB
                    // candidate files).
                    violations.push(
                        Violation::new(format!(
                            "file is too large to analyze ({})",
                            crate::io::over_cap(n),
                        ))
                        .with_path(entry.path.clone()),
                    );
                    continue;
                }
                Err(crate::io::ReadCapError::Io(_)) => {
                    // Permission / race — silent skip, like other
                    // content rules.
                    continue;
                }
            };

            let Some(format) = self
                .format_override
                .or_else(|| Format::detect_from_path(&entry.path))
            else {
                violations.push(
                    Violation::new(
                        "could not detect format from extension; pass `format:` \
                         (`json` / `yaml` / `toml` / `xml` / `dotenv` / `properties` / `ini` / `hcl`) on the rule",
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
                // jsonschema 0.36+ privatised `instance_path` and exposes
                // it via the `instance_path()` accessor method.
                let detail = format!("schema violation at `{}`: {error}", error.instance_path());
                let msg = self.message.clone().unwrap_or(detail);
                // One document can fail N schema constraints; without a key the
                // N path-only findings collapse to one fingerprint and a new
                // schema violation would be masked. Key on the (data location,
                // schema constraint) pair — stable and reword-proof.
                violations.push(
                    Violation::new(msg)
                        .with_path(entry.path.clone())
                        .with_baseline_key(format!(
                            "schema\u{0}{}\u{0}{}",
                            error.instance_path(),
                            error.schema_path()
                        )),
                );
            }
        }
        Ok(violations)
    }
}

fn compile_schema(schema_abs: &std::path::Path) -> std::result::Result<Validator, String> {
    let bytes = crate::io::read_capped(schema_abs).map_err(|e| match e {
        crate::io::ReadCapError::TooLarge(n) => format!(
            "schema {} is too large to read ({})",
            schema_abs.display(),
            crate::io::over_cap(n),
        ),
        crate::io::ReadCapError::Io(e) => {
            format!("could not read schema {}: {e}", schema_abs.display())
        }
    })?;
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

    let format_override = opts.format.map(TargetFormat::to_format);

    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "json_schema_passes has no fix op - alint can't synthesize correct content",
        ));
    }

    Ok(Box::new(JsonSchemaPassesRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        schema_path: opts.schema_path,
        allow_out_of_root: false,
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
    fn schema_path_escape_fires_without_reading() {
        use crate::test_support::{ctx, tempdir_with_files};
        // Security regression (v0.12 path-confinement): an absolute
        // `schema_path:` must produce an "escapes the repo root"
        // violation, never read/compile an out-of-tree file.
        let r = JsonSchemaPassesRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            scope: Scope::from_patterns(&["**/*.json".to_string()]).unwrap(),
            schema_path: "/etc/hostname".into(),
            allow_out_of_root: false,
            format_override: None,
            compiled: OnceLock::new(),
        };
        let (tmp, idx) = tempdir_with_files(&[("data.json", b"{}")]);
        let v = r.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].message.contains("escapes the repo root"),
            "{}",
            v[0].message
        );
    }

    #[test]
    fn schema_path_out_of_root_read_when_allowed() {
        use crate::test_support::{ctx, tempdir_with_files};
        // With `allow_out_of_root`, an absolute out-of-tree schema is
        // read + compiled; the in-tree file validates and a note records
        // the escape.
        let ext = tempfile::tempdir().unwrap();
        let schema = ext.path().join("schema.json");
        std::fs::write(&schema, r#"{"type":"object"}"#).unwrap();
        let r = JsonSchemaPassesRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            scope: Scope::from_patterns(&["**/*.json".to_string()]).unwrap(),
            schema_path: schema.clone(),
            allow_out_of_root: true,
            format_override: None,
            compiled: OnceLock::new(),
        };
        let (tmp, idx) = tempdir_with_files(&[("data.json", b"{}")]);
        let v = r.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(
            v.iter().all(|x| x.is_note),
            "only an out-of-root note: {v:?}"
        );
        assert!(
            v.iter().any(|x| x.message.contains("allow_out_of_root")),
            "{v:?}"
        );
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
    fn target_format_covers_every_format() {
        // Parity: every Format::ALL variant is requestable as `format: <label>`
        // and maps back to it, so json_schema_passes can't silently lack a format.
        for f in Format::ALL {
            let label = f.label().to_lowercase();
            let tf: TargetFormat = serde_json::from_value(json!(label))
                .unwrap_or_else(|e| panic!("`format: {label}` should deserialize: {e}"));
            assert_eq!(
                tf.to_format(),
                *f,
                "format: {label} must map to Format::{f:?}"
            );
        }
    }

    #[test]
    fn xml_targets_are_auto_detected() {
        // The full .csproj / .props / .targets / .vbproj / .fsproj / .nuspec /
        // .xml family auto-detects as XML -- a formerly-latent path, now
        // documented and tested (docs/design/format-coverage.md, section 7 Q2).
        // Every extension in structured_format's XML arm is covered here.
        for p in [
            "App.csproj",
            "Directory.Build.props",
            "Directory.Build.targets",
            "Legacy.vbproj",
            "Lib.fsproj",
            "Pkg.nuspec",
            "config.xml",
        ] {
            assert_eq!(
                Format::detect_from_path(std::path::Path::new(p)),
                Some(Format::Xml),
                "{p} should detect as XML"
            );
        }
    }

    #[test]
    fn format_yml_is_a_yaml_alias() {
        // `format: yml` is a documented alias for YAML. The parity test iterates
        // Format::ALL, which has no `yml`, so this pins the alias explicitly:
        // `yml` deserializes and maps to Format::Yaml (not, say, a silent error).
        let tf: TargetFormat =
            serde_json::from_value(json!("yml")).expect("`format: yml` should deserialize");
        assert_eq!(tf.to_format(), Format::Yaml, "`yml` must map to YAML");
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

    #[test]
    fn empty_file_satisfies_an_object_schema_not_false_fires() {
        use crate::test_support::{ctx, spec_yaml, tempdir_with_files};
        // Regression (pre-v0.16 audit): an empty / whitespace-only config file must
        // parse to `{}` and PASS `{"type":"object"}` -- an empty file is a valid
        // empty document, not a schema violation. An earlier cut returned `null`
        // for empty input, so this rule false-fired "null is not of type object",
        // while a comment-only file of the same format parsed to `{}` and passed --
        // two semantically-identical "no config" files disagreeing. All three must
        // now be silent.
        let (tmp, idx) = tempdir_with_files(&[
            ("schema.json", br#"{"type":"object"}"#),
            ("empty.toml", b""),
            ("blank.toml", b"   \n\t\n"),
            ("comment.toml", b"# just a comment\n"),
        ]);
        let spec = spec_yaml(
            "id: t\n\
             kind: json_schema_passes\n\
             paths: \"**/*.toml\"\n\
             schema_path: schema.json\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(
            v.is_empty(),
            "empty / whitespace / comment-only files must satisfy {{\"type\":\"object\"}}: {v:?}"
        );
    }
}
