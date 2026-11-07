//! YAML front-end for alint. Reads a `.alint.yml` and returns a
//! [`alint_core::Config`] that the engine can instantiate.
//!
//! ## Composition model
//!
//! `extends:` resolution happens at the YAML-`Value` layer, not
//! the typed-`Config` layer. Each `.alint.yml` (local, HTTPS,
//! bundled) is parsed into a private `RawConfig` that keeps each
//! rule as a `yaml_serde::Mapping` rather than an
//! [`alint_core::RuleSpec`]. This lets children in the extends
//! chain specify only the fields they want to override — e.g.,
//!
//! ```yaml
//! extends: [./base.yml]
//! rules:
//!   - id: inherited-rule   # only id + level; kind/paths/etc
//!     level: off           # inherit from base.yml
//! ```
//!
//! Merge semantics for rules: group by `id` (insertion-preserving
//! across sources), merge the mapping fields last-wins. After all
//! extends resolve, each merged mapping is deserialized once into
//! an [`alint_core::RuleSpec`] — validation (`kind` required,
//! `level` required, kind-specific fields valid) fires there, so
//! a rule that never gets a `kind` assigned anywhere in its chain
//! is a clean error.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

pub mod bundled;
pub mod extends;
mod interp;
mod loader;
mod nested;
mod trust;

pub(crate) use trust::{
    ENV_VARS_MARKER, FIELD_SOURCES_MARKER, PROVENANCE_MARKER, Provenance, SENSITIVE_FIELDS_MARKER,
    SourceClass, UNTRUSTED_FIX_SOURCE_MARKER, UntrustedWhenVarRead, configured_field_source,
    demote_content_fixers_in, demote_content_fixers_in_rule, field_reads_environment,
    find_spawning_fix_op, find_spawning_kind, find_template_placeholder_in_guarded_field,
    has_untrusted_fix_source, mark_field_sources_in, mark_provenance_in, mark_sensitive_conditions,
    mark_template_sensitive_fields, mark_unattributed_field_sources, mark_untrusted_fix_sources_in,
    masked_rule_mappings, merge_field_sources, merge_mapping_fields, merge_sensitive_fields,
    reject_ambiguous_yaml_in, reject_env_expansion_in, reject_env_reads_in_when,
    reject_remote_assembled_env_refs, reject_untrusted_assembled_when,
    reject_untrusted_env_instance_vars, reject_untrusted_env_var_reads,
    strip_fix_promotions_in_rule, take_sensitive_fields, take_untrusted_fix_source,
    union_provenance,
};
pub use trust::{
    SPAWNING_FIX_OPS, SPAWNING_RULE_KINDS, reject_allow_out_of_root_in, reject_baseline_in,
    reject_command_rules_in, reject_fix_promotion_in, reject_fix_promotion_templates_in,
    reject_spawning_fix_op_templates_in, reject_spawning_fix_ops_in, reject_spawning_templates_in,
    reject_trusted_extends_in, reject_untrusted_ignore_in,
};

use alint_core::{
    Config, ConfigCompatibility, ConfigRequirements, Error, FactSpec, Result, RunningVersion,
};
use serde::Deserialize;
use yaml_serde::Mapping;

/// The canonical JSON Schema (draft 2020-12) for `.alint.yml` configuration
/// files. Embedded at build time from the in-crate copy at
/// `crates/alint-dsl/schemas/v1/config.json`, which is kept byte-identical
/// with the root `schemas/v1/config.json` (the public URL source) by the
/// `in_crate_schema_matches_root` test below.
///
/// The schema's primary consumer is the YAML language server for editor
/// autocomplete; tests round-trip representative configs through it to
/// keep the schema and the actual DSL in sync.
pub const CONFIG_SCHEMA_V1: &str = include_str!("../schemas/v1/config.json");

pub(crate) const DEFAULT_CONFIG_NAMES: &[&str] =
    &[".alint.yml", ".alint.yaml", "alint.yml", "alint.yaml"];

/// Locate a config file starting at `start` and walking upward until one is
/// found or the filesystem root is hit.
pub fn discover(start: &Path) -> Option<PathBuf> {
    // Relative paths such as `.` have a lexical parent of the empty path, so
    // walking them directly stops after one directory. Make the start absolute
    // first so discovery from a nested working directory reaches every real
    // ancestor. `absolute` is lexical (it need not resolve symlinks or require
    // the path to exist), which keeps discovery predictable and inexpensive.
    let start = std::path::absolute(start).ok()?;
    let mut current = Some(start.as_path());
    while let Some(dir) = current {
        for name in DEFAULT_CONFIG_NAMES {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        current = dir.parent();
    }
    None
}

/// Load without an injected catalog.
///
/// This remains available to embedders that only parse a trusted,
/// self-contained config without templates. Trust-sensitive templates,
/// inherited rules, and nested rules fail closed when their behavior cannot be
/// classified without a catalog.
pub fn load(path: &Path) -> Result<Config> {
    load_with(path, &LoadOptions::default())
}

/// Load with explicit cache, fetcher, and kind-catalog options.
///
/// An error is scrubbed of the values the environment supplied to the
/// configs' top-level fields (see [`Config::environment_values`]); the loaded
/// config carries them so an embedder can scrub a later error the same way.
pub fn load_with(path: &Path, opts: &LoadOptions<'_>) -> Result<Config> {
    let raw = load_top_level_raw(path, opts)?;
    let mut environment_values = raw.environment_values.clone();
    load_resolved(path, opts, raw, &mut environment_values)
        .map_err(|error| alint_core::redact_environment_values_in_error(error, &environment_values))
}

/// Discover the nested configs of the resolved top-level `raw`, then finalise
/// and validate the whole. The nested configs' environment values are added to
/// `environment_values` too, so a later error is scrubbed of them.
fn load_resolved(
    path: &Path,
    opts: &LoadOptions<'_>,
    mut raw: RawConfig,
    environment_values: &mut Vec<String>,
) -> Result<Config> {
    // Nested `.alint.yml` discovery (opt-in via `nested_configs:
    // true` on the root config). Walks from the root config's
    // directory, finds any sub-directory configs, scopes their
    // rules to their directory, and appends them to the root's
    // rule list.
    if raw.nested_configs == Some(true) {
        let root_dir = opts.repository_root.as_deref().map_or_else(
            || {
                path.parent()
                    .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
            },
            Path::to_path_buf,
        );
        let canonical_root_cfg =
            opts.config_source
                .canonicalize(path)
                .map_err(|source| Error::Io {
                    path: path.to_path_buf(),
                    source,
                })?;
        let (discovered_rules, discovered_compatibility, discovered_environment_values) =
            nested::discover_nested(
                &root_dir,
                &canonical_root_cfg,
                &raw,
                opts.kind_catalog,
                RunningVersion::current(),
                opts.environment.as_ref(),
            )?;
        raw.rules.extend(discovered_rules);
        extend_compatibility(&mut raw.compatibility, discovered_compatibility);
        loader::add_environment_values(environment_values, &discovered_environment_values);
        loader::add_environment_values(&mut raw.environment_values, &discovered_environment_values);
    }

    let merged = raw.finalize(opts.kind_catalog)?;
    validate(&merged)?;
    Ok(merged)
}

/// Whether the config at `path` (with its `extends:` chain and
/// `.alint.d/` drop-ins, exactly as [`load`] resolves them) turns on
/// `nested_configs:`. Skips the nested-config walk itself, so it is cheap
/// enough for an embedder (the LSP) to ask which ancestor config governs
/// a file.
pub fn nested_configs_enabled(path: &Path) -> Result<bool> {
    nested_configs_enabled_with(path, &LoadOptions::default())
}

/// [`nested_configs_enabled`] with explicit loader options.
pub fn nested_configs_enabled_with(path: &Path, opts: &LoadOptions<'_>) -> Result<bool> {
    Ok(load_top_level_raw(path, opts)?.nested_configs == Some(true))
}

/// The top-level config plus its `extends:` chain and `.alint.d/`
/// drop-ins, merged but not yet finalised (and without nested configs). An
/// error is scrubbed of the environment values the configs read so far.
fn load_top_level_raw(path: &Path, opts: &LoadOptions<'_>) -> Result<RawConfig> {
    let mut state = loader::LoadState::default();
    resolve_top_level(path, opts, &mut state).map_err(|error| {
        alint_core::redact_environment_values_in_error(error, &state.environment_values)
    })
}

fn resolve_top_level(
    path: &Path,
    opts: &LoadOptions<'_>,
    state: &mut loader::LoadState,
) -> Result<RawConfig> {
    // Confinement boundary for local `extends:` targets — the top-level
    // config's directory. A local extends chain (e.g. a shared ruleset
    // committed to the repo) may not escape this tree to read arbitrary
    // local files; `allow_out_of_root: true` on the top-level config lifts
    // it. The top-level config itself is trusted and unchecked (it may sit
    // anywhere, e.g. a `-c` outside the linted tree); only the *targets* it
    // pulls in are confined.
    let confine_root = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    // `is_top: true` — the user's own top-level config may open the
    // `allow_out_of_root` escape hatch; an `extends:`'d ruleset may not (that
    // recursion passes `false`), so an untrusted ruleset can't lift confinement.
    // `&[]` trusted: the top-level config reads its OWN `trusted_extends:` inside
    // `load_recursive` (is_top), so the seed list is empty.
    let mut raw = loader::load_recursive(
        path,
        state,
        opts,
        Some(&confine_root),
        true,
        &[],
        loader::SourceContext::new(alint_core::ConfigSourceClass::Root, None),
    )?;

    // `.alint.d/*.yml` drop-ins — auto-discovered next to the
    // top-level config and merged in alphabetical order. The
    // last drop-in alphabetically wins on field-level
    // overrides, mirroring the `/etc/*.d/` convention: stage
    // ops conventions as `00-base.yml`, team policies as
    // `50-team.yml`, developer-local tweaks as `99-local.yml`.
    //
    // Trust-equivalent to the main config — drop-ins live in
    // the same workspace under the user's control, so they
    // can declare `custom:` facts and `kind: command` rules
    // without the trust-gate that protects HTTPS / bundled
    // extends. Sub-extended configs (chains rooted via
    // `extends:`) do NOT get their own `.alint.d/` discovery —
    // only the top-level config does, to keep the loading
    // surface comprehensible.
    let drop_in_dir = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".alint.d");
    for drop_in_path in collect_drop_ins(&drop_in_dir, opts.config_source.as_ref())? {
        // Drop-ins are trust-equivalent to the top-level config (local,
        // user-controlled), so `is_top: true` — they may open the escape hatch.
        let drop_in = loader::load_recursive(
            &drop_in_path,
            state,
            opts,
            Some(&confine_root),
            true,
            // is_top: a drop-in reads its own `trusted_extends:`, so seed empty.
            &[],
            loader::SourceContext::new(alint_core::ConfigSourceClass::DropIn, raw.version),
        )?;
        raw = merge(raw, drop_in);
    }
    // With the forms the loader printed an `extends:` entry read from the
    // environment in, which a later error can quote too (`load_extended_source`).
    loader::add_environment_values(&mut raw.environment_values, &state.environment_values);
    Ok(raw)
}

/// List `.alint.d/*.{yml,yaml}` files alphabetically through the injected
/// config source provider.
fn collect_drop_ins(
    dir: &Path,
    source_provider: &dyn ConfigSourceProvider,
) -> Result<Vec<PathBuf>> {
    source_provider.yaml_files(dir).map_err(|source| Error::Io {
        path: dir.to_path_buf(),
        source,
    })
}

/// Intermediate form used during `extends:` resolution. Identical
/// to [`Config`] except that rules are kept as raw
/// `yaml_serde::Mapping`s so overrides can merge per-field
/// instead of per-rule. See the module-level docs for the full
/// composition model.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawConfig {
    // The four top-level settings below are `Option` so `merge` can tell "set"
    // from "unset": a `.alint.d/` drop-in that omits one must not reset the
    // main config's explicit value to the serde default. `finalize` applies
    // the defaults once, after every source has merged.
    #[serde(default)]
    version: Option<u32>,
    /// Source-local minimum binary requirement. Validated before composition.
    #[serde(default, deserialize_with = "deserialize_explicit")]
    requires: Option<ConfigRequirements>,
    #[serde(default)]
    extends: Vec<alint_core::ExtendsEntry>,
    #[serde(default)]
    ignore: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_explicit")]
    respect_gitignore: Option<bool>,
    #[serde(default)]
    vars: std::collections::HashMap<String, String>,
    #[serde(default)]
    facts: Vec<FactSpec>,
    /// Reusable rule shapes referenced by `extends_template:` in
    /// `rules:` entries. Each template has its own `id:` and any
    /// other rule-spec fields; placeholders `{{vars.<name>}}` in
    /// those fields are substituted from the instance's `vars:`
    /// map at expansion time. Templates are kept as raw
    /// `Mapping`s here so the expansion step has the same
    /// field-level granularity as rule overrides.
    #[serde(default)]
    templates: Vec<Mapping>,
    #[serde(default)]
    rules: Vec<Mapping>,
    /// Outer `None` = unset; `Some(None)` = an explicit `fix_size_limit: null`
    /// (no cap), which must survive a later source that omits the key.
    #[serde(default, deserialize_with = "deserialize_explicit")]
    #[allow(clippy::option_option)] // unset vs explicit `null` vs a limit
    fix_size_limit: Option<Option<u64>>,
    #[serde(default, deserialize_with = "deserialize_explicit")]
    nested_configs: Option<bool>,
    /// `allow_out_of_root:` — the top-level escape hatch for path
    /// confinement. Parsed here (the YAML-facing form); the loader
    /// rejects a non-default value from any `extends:`'d ruleset, and
    /// `finalize()` carries the surviving (top-level) value onto
    /// `Config`. See `docs/design/v0.12/allow_out_of_root.md`.
    #[serde(default)]
    allow_out_of_root: alint_core::AllowOutOfRoot,
    /// `baseline:` — path to a committed baseline file `check` suppresses
    /// against (the YAML-facing form). Top-level-only: the loader rejects a
    /// value from any `extends:`'d/nested config, and `finalize()` carries the
    /// surviving (top-level) value onto `Config`. See
    /// `docs/design/baseline.md` §2.3.
    #[serde(default)]
    baseline: Option<std::path::PathBuf>,
    /// `trusted_extends:` -- remote `extends:` URLs whose CONTENT-injecting fixers
    /// (`replace` / `file_create` / `file_prepend` / `file_append`) are honored at
    /// their declared tier instead of demoted to a suggestion. Top-level-only (the
    /// loader rejects a value from any `extends:`'d / nested config): a remote must
    /// never be able to allowlist itself. Consumed at load (it gates the demotion
    /// there), so it is not carried onto `Config`. See auto-fix.md 5.5.
    #[serde(default)]
    trusted_extends: Vec<String>,
    /// The top-level `vars:` whose value was interpolated from the environment,
    /// mapped to the raw pre-interpolation text (`{{env.NPM_TOKEN}}`). Filled by
    /// the local-config parser and kept in step with `vars` by [`merge`] (a
    /// later literal value for the name clears the mark, a later env-derived one
    /// sets it). Not part of the schema.
    #[serde(skip)]
    env_vars: std::collections::HashMap<String, String>,
    /// Every `vars.<NAME>` read in a `when:` / `when_iter:` of an untrusted
    /// remote, recorded per source before the merge. Whether a read is refused
    /// depends on [`Self::env_vars`], which is only final once every local
    /// config has merged, so `finalize` judges them. Not part of the schema.
    #[serde(skip)]
    untrusted_when_var_reads: Vec<UntrustedWhenVarRead>,
    /// Validated per-source compatibility records retained through composition.
    #[serde(skip)]
    compatibility: Vec<ConfigCompatibility>,
    /// Values the environment supplied to this config's top-level fields other
    /// than `rules:`, `templates:` and `vars:`, recorded by the local-config
    /// parser and carried to [`Config::environment_values`]. Not part of the
    /// schema.
    #[serde(skip)]
    environment_values: Vec<String>,
    /// Whether each of [`Self::extends`] read the environment, recorded by the
    /// local-config parser before interpolation erases it. Not part of the
    /// schema.
    #[serde(skip)]
    extends_environment: Vec<bool>,
}

const DEFAULT_FIX_SIZE_LIMIT: Option<u64> = Some(1 << 20);

/// Deserialize a present key (including an explicit `null`) as `Some(value)`,
/// leaving a missing key to `#[serde(default)]`'s `None`.
fn deserialize_explicit<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

impl RawConfig {
    /// Drop the top-level settings an `extends:`'d config declared. They were
    /// never honored from an extended source (the extending config's own
    /// value -- or its default -- always replaced them), and an inherited
    /// ruleset lifting `fix_size_limit` or `respect_gitignore` for the
    /// consumer is not its call; warn instead of silently ignoring.
    pub(crate) fn drop_top_level_settings(&mut self, source: &str) {
        // `version:` describes the extended file itself, not the consumer.
        self.version = None;
        self.requires = None;
        let declared: Vec<&str> = [
            ("respect_gitignore", self.respect_gitignore.take().is_some()),
            ("fix_size_limit", self.fix_size_limit.take().is_some()),
            ("nested_configs", self.nested_configs.take().is_some()),
        ]
        .into_iter()
        .filter_map(|(name, was_set)| was_set.then_some(name))
        .collect();
        if !declared.is_empty() {
            tracing::warn!(
                "extended config {source} sets {}; top-level settings are only read from \
                 your own config and its `.alint.d/` drop-ins, so the value is ignored",
                declared.join(", ")
            );
        }
    }
}

impl RawConfig {
    /// Reject a template that could smuggle a process past the spawn gate.
    fn reject_unsafe_templates(&self, catalog: Option<alint_core::KindCatalog<'_>>) -> Result<()> {
        // A process-spawning kind must never hide inside a `templates:`
        // block. Templates are expanded in `finalize`, *after* the extends/nested
        // spawn gate (`reject_command_rules_in`) has run — and an
        // `extends_template:` instance carries no `kind` of its own — so a
        // spawning template would smuggle code execution straight past the
        // gate (the original C1 RCE bypass). Spawning kinds are confined to
        // a top-level `rules:` entry: declare the command rule directly,
        // never via a template. Checked for every source (top-level too) so
        // the invariant holds regardless of where the template came from;
        // the extends/nested loaders also reject spawning templates earlier
        // with the offending source named.
        for t in &self.templates {
            let id = t.get("id").and_then(|v| v.as_str()).unwrap_or("(unknown)");
            // Trust-relevant fields must be literal in a template. `{{vars.*}}` is
            // substituted at expansion -- after every gate below has inspected the
            // raw text -- so `kind: "{{vars.k}}"` + `vars: {k: command}` (or an
            // `applicability` placeholder resolving to `safe`) would otherwise be
            // judged by its placeholder and executed as its substitution.
            if let Some(field) = find_template_placeholder_in_guarded_field(t) {
                return Err(Error::Other(format!(
                    "template {id:?}: `{field}` must be a literal value; `{{{{vars.*}}}}` \
                     placeholders are not allowed in a rule kind or a fix applicability \
                     because they are substituted after the trust gates have run"
                )));
            }
            // Recurse `require:` at every depth: a spawning kind nested inside a
            // template's `require:` block expands into its instance just like a
            // top-level one (audit 2026-10).
            if let Some(kind) = find_spawning_kind(t, catalog) {
                return Err(Error::Other(format!(
                    "template {id:?}: `kind: {kind}` spawns a process or is not present in the \
                     injected kind catalog, and is not allowed in a `templates:` block \
                     (including inside a `require:` block). Post-gate template expansion could \
                     execute arbitrary code; declare a known spawning rule directly in your \
                     top-level `rules:`."
                )));
            }
            // The same metadata-derived backstop for spawning or unknown fix
            // operations at every `require:` depth. A template's fix is spliced
            // into its referencing rule after the extends/nested gate, so it
            // must be classified before expansion.
            if let Some(op) = find_spawning_fix_op(t, catalog) {
                return Err(Error::Other(format!(
                    "template {id:?}: `fix.{op}` spawns a process or is not present in the \
                     injected fix-op catalog, and is not allowed in a `templates:` block. \
                     Post-gate template expansion could execute arbitrary code; declare a \
                     known spawning fix directly on a rule in your top-level `rules:`."
                )));
            }
        }
        Ok(())
    }

    /// Deserialize each rule mapping into a [`RuleSpec`]. This is
    /// where kind-specific validation fires: a rule that never
    /// received a `kind` anywhere in its extends chain produces a
    /// serde error here, referencing the offending rule's id.
    /// Also where `extends_template:` instances expand against
    /// the `templates:` block: the template body is cloned, its
    /// `{{vars.<name>}}` placeholders substituted from the
    /// instance's `vars:` map, and the instance's own
    /// non-template fields field-merge on top.
    fn finalize(self, catalog: Option<alint_core::KindCatalog<'_>>) -> Result<Config> {
        self.reject_unsafe_templates(catalog)?;
        // An untrusted remote's `when` may read ordinary vars, but not one the
        // user's config interpolated from the environment; that set is final only
        // now, after every local config has merged.
        reject_untrusted_env_var_reads(&self.untrusted_when_var_reads, &self.env_vars)?;
        let templates_by_id: std::collections::HashMap<String, &Mapping> = self
            .templates
            .iter()
            .filter_map(|t| {
                t.get("id")
                    .and_then(|v| v.as_str())
                    .map(|id| (id.to_string(), t))
            })
            .collect();

        let mut rules = Vec::with_capacity(self.rules.len());
        for m in &self.rules {
            let id_hint = m
                .get("id")
                .and_then(|v| v.as_str())
                .map_or_else(|| "<anonymous>".to_string(), str::to_string);
            reject_remote_assembled_env_refs(m, &templates_by_id, &id_hint)?;
            reject_untrusted_assembled_when(m, &templates_by_id, &id_hint, &self.env_vars)?;
            reject_untrusted_env_instance_vars(m, &templates_by_id, &id_hint)?;
            // An unknown template's error quotes `extends_template:`, which may
            // be read from the environment.
            let masked_instance = masked_rule_mappings(m);
            let mut expanded = expand_template(m, &templates_by_id).map_err(|error| {
                redact_rule_error(error, masked_instance.as_deref(), &id_hint, |masked| {
                    expand_template(masked, &templates_by_id).map(drop)
                })
            })?;
            mark_sensitive_conditions(&mut expanded, &self.env_vars);
            expanded.remove(ENV_VARS_MARKER);
            // A nested-config rule is confined to its subtree here, on the effective
            // rule, so fields and the kind a root template contributes are re-rooted
            // and kind-checked too (not only the instance's own fields).
            // Scoping quotes a field it rejects (an absolute `paths:`), so its
            // error is redacted like the later steps' when that field may be secret.
            let masked = masked_rule_mappings(&expanded);
            nested::apply_nested_scope(&mut expanded, catalog).map_err(|error| {
                redact_rule_error(error, masked.as_deref(), &id_hint, |masked| {
                    nested::apply_nested_scope(&mut masked.clone(), catalog)
                })
            })?;
            // A source-local cap is not enough: an untrusted rule can instantiate
            // a fixer-bearing trusted template, and a trusted rule can instantiate
            // a template partly defined by an untrusted source. Carry one bit of
            // provenance through merge + expansion, then cap the EFFECTIVE fixer.
            // Remove the private marker before RuleSpec deserialization so it never
            // leaks into the public DSL or runtime model.
            let untrusted_fix_source = take_untrusted_fix_source(&mut expanded);
            let provenance = Provenance::read(&expanded);
            expanded.remove(PROVENANCE_MARKER);
            // A spawning rule runs whatever its fields say, so ALL of them must
            // come from the user's own config. An extended source cannot declare
            // a spawning kind or fix op (the per-source gates), but it could share
            // the id of the user's spawning rule (or define the template it
            // instantiates) and field-merge a `workdir:` / `paths:` / `command:` /
            // `require:` into it -- the kind-less contribution passes every
            // per-source gate. Refuse any such contribution to a rule whose
            // EFFECTIVE kind or fix spawns (audit R2).
            if let Some(source) = &provenance.extended {
                let spawns = find_spawning_kind(&expanded, catalog)
                    .map(|k| format!("`kind: {k}`"))
                    .or_else(|| {
                        find_spawning_fix_op(&expanded, catalog).map(|op| format!("`fix.{op}`"))
                    });
                if let Some(what) = spawns {
                    return Err(Error::rule_config(
                        &id_hint,
                        format!(
                            "{what} spawns a process, but an extended config ({source}) \
                             contributes fields to this rule (it shares the rule's id, or \
                             defines the template the rule instantiates). A spawning rule \
                             must be declared entirely in your own top-level config; give \
                             it an id no extended config uses."
                        ),
                    ));
                }
            }
            if untrusted_fix_source {
                demote_content_fixers_in_rule(&mut expanded, catalog);
                // An untrusted rule may instantiate a TRUSTED template that promotes
                // a destructive fixer (`file_remove: {applicability: safe}`) and aim
                // it with its own `paths:`. The promotion belongs to the template's
                // author, not to this rule, so the effective fixer falls back to its
                // op's own default tier.
                strip_fix_promotions_in_rule(&mut expanded);
            }
            rules.push(deserialize_effective_rule(expanded, catalog, &id_hint)?);
        }
        Ok(Config::new(self.version.unwrap_or(0), rules)
            .with_requires(effective_requirements(&self.compatibility))
            .with_ignore(self.ignore)
            .with_respect_gitignore(self.respect_gitignore.unwrap_or(true))
            .with_vars(self.vars)
            .with_facts(self.facts)
            .with_fix_size_limit(self.fix_size_limit.unwrap_or(DEFAULT_FIX_SIZE_LIMIT))
            .with_nested_configs(self.nested_configs.unwrap_or(false))
            .with_allow_out_of_root(self.allow_out_of_root)
            .with_baseline(self.baseline)
            .with_compatibility(self.compatibility)
            .with_environment_values(self.environment_values))
    }
}

fn deserialize_effective_rule(
    mut expanded: Mapping,
    catalog: Option<alint_core::KindCatalog<'_>>,
    id_hint: &str,
) -> Result<alint_core::RuleSpec> {
    let masked = masked_rule_mappings(&expanded);
    let validate =
        |mapping: &Mapping| validate_effective_rule_fields(mapping, catalog, id_hint, None, None);
    validate(&expanded)
        .map_err(|error| redact_rule_error(error, masked.as_deref(), id_hint, validate))?;
    let deserialize = |mut mapping: Mapping| {
        mapping.remove(FIELD_SOURCES_MARKER);
        let sensitive_fields = take_sensitive_fields(&mut mapping);
        yaml_serde::from_value::<alint_core::RuleSpec>(yaml_serde::Value::Mapping(mapping))
            .map(|spec| (spec, sensitive_fields))
            .map_err(|error| {
                Error::rule_config(
                    id_hint,
                    format!("could not deserialize merged rule: {error}"),
                )
            })
    };
    let (mut spec, sensitive_fields) =
        deserialize(std::mem::take(&mut expanded)).map_err(|error| {
            redact_rule_error(error, masked.as_deref(), id_hint, |masked| {
                deserialize(masked.clone()).map(drop)
            })
        })?;
    spec.set_sensitive_fields(sensitive_fields);
    Ok(spec)
}

/// Keep a configuration error of a rule with possibly secret values readable
/// only when `rerun` on a masked copy ([`masked_rule_mappings`]) fails with the
/// identical message, which therefore does not quote one of them; otherwise
/// redact it. A rule without such values (`masked` is `None`) keeps every
/// error.
fn redact_rule_error(
    error: Error,
    masked: Option<&[Mapping]>,
    id_hint: &str,
    mut rerun: impl FnMut(&Mapping) -> Result<()>,
) -> Error {
    let Some(masked) = masked else {
        return error;
    };
    let message = error.to_string();
    if masked.iter().any(|masked| {
        rerun(masked)
            .err()
            .is_some_and(|masked_error| masked_error.to_string() == message)
    }) {
        return error;
    }
    if error.is_internal() {
        Error::internal(alint_core::REDACTED_CONFIGURATION_ERROR)
    } else {
        Error::rule_config(id_hint, alint_core::REDACTED_CONFIGURATION_ERROR)
    }
}

fn effective_requirements(compatibility: &[ConfigCompatibility]) -> Option<ConfigRequirements> {
    compatibility
        .iter()
        .filter_map(|record| record.requires_alint)
        .max()
        .map(|alint| ConfigRequirements { alint })
}
/// Expand a rule mapping that references `extends_template:`,
/// or pass it through unchanged if it doesn't. The expansion
/// looks up the named template, rejects unknown ids and
/// chained templates, substitutes `{{vars.<name>}}` placeholders
/// throughout the cloned body, drops the template-only fields
/// (`id`, `extends_template`, `vars`), and field-merges the
/// instance's remaining keys on top.
fn expand_template(
    rule: &Mapping,
    templates_by_id: &std::collections::HashMap<String, &Mapping>,
) -> Result<Mapping> {
    let Some(template_id) = rule
        .get("extends_template")
        .and_then(|v| v.as_str())
        .map(str::to_string)
    else {
        return Ok(rule.clone());
    };

    let id_hint = rule
        .get("id")
        .and_then(|v| v.as_str())
        .map_or_else(|| "<anonymous>".to_string(), str::to_string);

    let template = templates_by_id.get(&template_id).ok_or_else(|| {
        Error::rule_config(
            &id_hint,
            format!("`extends_template: {template_id}` references an unknown template"),
        )
    })?;

    if template.contains_key("extends_template") {
        return Err(Error::rule_config(
            &id_hint,
            format!(
                "template `{template_id}` itself references `extends_template:` - \
                 templates are leaf-only (mirrors the bundled-rulesets restriction)"
            ),
        ));
    }

    let vars = instance_vars(rule);
    let env_vars = rule
        .get(ENV_VARS_MARKER)
        .and_then(|value| value.as_mapping());

    let untrusted_fix_source = has_untrusted_fix_source(template) || has_untrusted_fix_source(rule);
    let mut expanded = (*template).clone();
    mark_template_sensitive_fields(&mut expanded, template, env_vars);
    // A template id read from the environment stays out of the attribution
    // an error about a field the template supplies quotes. (`sensitive: true`
    // leaves it shown: it names the rule's template, part of its identity.)
    let template_source = if field_reads_environment(rule, "extends_template") {
        "a template".to_string()
    } else {
        format!("template `{template_id}`")
    };
    mark_unattributed_field_sources(&mut expanded, &template_source);
    let template_field_sources = expanded.remove(FIELD_SOURCES_MARKER);
    let template_provenance = expanded.remove(PROVENANCE_MARKER);
    expanded = substitute_template_vars(expanded, &vars);
    expanded.remove("id");
    if let Some(field_sources) = template_field_sources {
        expanded.insert(FIELD_SOURCES_MARKER.into(), field_sources);
    }

    merge_field_sources(&mut expanded, rule);
    merge_sensitive_fields(&mut expanded, rule);
    for (k, v) in rule {
        let key = k.as_str().unwrap_or_default();
        if matches!(
            key,
            "extends_template"
                | "vars"
                | UNTRUSTED_FIX_SOURCE_MARKER
                | PROVENANCE_MARKER
                | FIELD_SOURCES_MARKER
                | SENSITIVE_FIELDS_MARKER
        ) {
            continue;
        }
        expanded.insert(k.clone(), v.clone());
    }
    // The effective rule was shaped by both the instance and the template.
    union_provenance(&mut expanded, rule);
    if let Some(marker) = template_provenance {
        let mut from_template = Mapping::new();
        from_template.insert(PROVENANCE_MARKER.into(), marker);
        union_provenance(&mut expanded, &from_template);
    }
    if untrusted_fix_source {
        expanded.insert(
            yaml_serde::Value::from(UNTRUSTED_FIX_SOURCE_MARKER),
            yaml_serde::Value::Bool(true),
        );
    }
    Ok(expanded)
}

fn validate_effective_rule_fields(
    mapping: &Mapping,
    catalog: Option<alint_core::KindCatalog<'_>>,
    parent_id: &str,
    inherited_source: Option<&str>,
    nested_context: Option<(usize, &str)>,
) -> Result<()> {
    let kind = mapping.get("kind").and_then(yaml_serde::Value::as_str);

    if nested_context.is_some() {
        for field in [
            "id",
            "level",
            "fix",
            "git_tracked_only",
            "respect_gitignore",
        ] {
            if mapping.contains_key(field) {
                return Err(configured_field_error(
                    parent_id,
                    nested_context,
                    field,
                    format!(
                        "`{field}` is not supported on a nested rule and would be ignored; \
                         declare a top-level rule instead"
                    ),
                    configured_source(mapping, field, inherited_source),
                ));
            }
        }
    }

    let meta = kind.and_then(|kind| catalog.and_then(|catalog| catalog.meta(kind)));
    if let (Some(kind), Some(meta)) = (kind, meta) {
        let fields = configured_fields(mapping);
        if let Err(violation) = alint_core::validate_configured_fields(&fields, kind, meta) {
            let source = configured_source(mapping, &violation.field, inherited_source);
            return Err(configured_field_error(
                parent_id,
                nested_context,
                &violation.field,
                violation.message,
                source,
            ));
        }
    }

    if !meta.is_some_and(|meta| meta.nested_rules) {
        return Ok(());
    }

    let nested_source = configured_source(mapping, "require", inherited_source);
    if let Some(require) = mapping
        .get("require")
        .and_then(yaml_serde::Value::as_sequence)
    {
        for (idx, value) in require.iter().enumerate() {
            let Some(nested) = value.as_mapping() else {
                continue;
            };
            let nested_kind = nested
                .get("kind")
                .and_then(yaml_serde::Value::as_str)
                .unwrap_or("<unknown>");
            validate_effective_rule_fields(
                nested,
                catalog,
                parent_id,
                nested_source,
                Some((idx, nested_kind)),
            )?;
        }
    }
    Ok(())
}

fn configured_fields(mapping: &Mapping) -> alint_core::ConfiguredFields<'_> {
    let predicates = mapping
        .get("scope_filter")
        .and_then(yaml_serde::Value::as_mapping);
    let fix_op = mapping
        .get("fix")
        .and_then(yaml_serde::Value::as_mapping)
        .and_then(|fix| {
            fix.iter()
                .find_map(|(op, options)| options.as_mapping().and_then(|_| op.as_str()))
        });
    alint_core::ConfiguredFields::new(fix_op)
        .with_null_fix(mapping.get("fix").is_some_and(yaml_serde::Value::is_null))
        .with_null_scope_filter(
            mapping
                .get("scope_filter")
                .is_some_and(yaml_serde::Value::is_null),
        )
        .with(
            alint_core::ConfiguredField::Paths,
            mapping.contains_key("paths"),
        )
        .with(
            alint_core::ConfiguredField::ScopeFilter,
            mapping.contains_key("scope_filter"),
        )
        .with(
            alint_core::ConfiguredField::Fix,
            mapping.contains_key("fix"),
        )
        .with(
            alint_core::ConfiguredField::ExpectMatches,
            mapping.contains_key("expect_matches"),
        )
        .with(
            alint_core::ConfiguredField::GitTrackedOnly,
            mapping.contains_key("git_tracked_only"),
        )
        .with(
            alint_core::ConfiguredField::RespectGitignore,
            mapping.contains_key("respect_gitignore"),
        )
        .with(
            alint_core::ConfiguredField::RootOnly,
            mapping.contains_key("root_only"),
        )
        .with(
            alint_core::ConfiguredField::Since,
            mapping.contains_key("since"),
        )
        .with_scope_filter(
            alint_core::ConfiguredScopeFilterField::ChangedSince,
            predicates.is_some_and(|m| m.contains_key("changed_since")),
        )
        .with_scope_filter(
            alint_core::ConfiguredScopeFilterField::HasAncestor,
            predicates.is_some_and(|m| m.contains_key("has_ancestor")),
        )
        .with_scope_filter(
            alint_core::ConfiguredScopeFilterField::IncludeManifestPaths,
            predicates.is_some_and(|m| m.contains_key("include_manifest_paths")),
        )
        .with_scope_filter(
            alint_core::ConfiguredScopeFilterField::ExcludeManifestPaths,
            predicates.is_some_and(|m| m.contains_key("exclude_manifest_paths")),
        )
}

fn configured_source<'a>(
    mapping: &'a Mapping,
    field: &str,
    inherited_source: Option<&'a str>,
) -> Option<&'a str> {
    let top_level = field.split('.').next().unwrap_or(field);
    mapping
        .contains_key(top_level)
        .then(|| configured_field_source(mapping, field).or(inherited_source))?
}

fn configured_field_error(
    rule_id: &str,
    nested_context: Option<(usize, &str)>,
    field: &str,
    mut message: String,
    source: Option<&str>,
) -> Error {
    if let Some(source) = source {
        write!(message, "; `{field}` was supplied by {source}")
            .expect("writing to a String cannot fail");
    }
    if let Some((idx, kind)) = nested_context {
        message = format!("nested rule #{idx} (`{kind}`): {message}");
    }
    Error::rule_config(rule_id, message)
}

/// A template instance's `vars:` map, as strings (numbers and bools stringify).
fn instance_vars(rule: &Mapping) -> std::collections::HashMap<String, String> {
    rule.get("vars")
        .and_then(|v| v.as_mapping())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| match (k.as_str(), v) {
                    (Some(key), yaml_serde::Value::String(s)) => Some((key.to_string(), s.clone())),
                    (Some(key), yaml_serde::Value::Number(n)) => {
                        Some((key.to_string(), n.to_string()))
                    }
                    (Some(key), yaml_serde::Value::Bool(b)) => {
                        Some((key.to_string(), b.to_string()))
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Render the `{{vars.<name>}}` placeholders of one string (other namespaces
/// and unknown names stay literal), exactly as template expansion does.
fn render_template_vars(s: &str, vars: &std::collections::HashMap<String, String>) -> String {
    alint_core::template::render_message(s, |ns, key| {
        if ns == "vars" {
            vars.get(key).cloned()
        } else {
            None
        }
    })
}

/// Recursively walk a YAML mapping and substitute
/// `{{vars.<name>}}` placeholders in every string value with
/// the corresponding entry from `vars`. Unknown placeholders
/// are preserved literally so a typo surfaces in the rule's
/// error / output rather than silently blanking a field.
fn substitute_template_vars(
    m: Mapping,
    vars: &std::collections::HashMap<String, String>,
) -> Mapping {
    let mut out = Mapping::with_capacity(m.len());
    for (k, v) in m {
        out.insert(k, substitute_template_vars_value(v, vars));
    }
    out
}

fn substitute_template_vars_value(
    value: yaml_serde::Value,
    vars: &std::collections::HashMap<String, String>,
) -> yaml_serde::Value {
    use yaml_serde::Value;
    match value {
        Value::String(s) => Value::String(render_template_vars(&s, vars)),
        Value::Sequence(seq) => Value::Sequence(
            seq.into_iter()
                .map(|v| substitute_template_vars_value(v, vars))
                .collect(),
        ),
        Value::Mapping(inner) => Value::Mapping(substitute_template_vars(inner, vars)),
        other => other,
    }
}

/// Lookup used by config interpolation.
///
/// Production loads use [`HostEnvironment`]. Embedders that need reproducible
/// evaluation can inject a closed map instead; unknown names must return
/// `None`.
pub trait EnvironmentProvider: std::fmt::Debug + Send + Sync {
    fn get(&self, name: &str) -> Option<String>;
}

/// Environment provider backed by the current process.
#[derive(Debug, Default)]
pub struct HostEnvironment;

impl EnvironmentProvider for HostEnvironment {
    fn get(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

/// Read-only source abstraction used for root configs, local `extends`, and
/// top-level `.alint.d` files.
///
/// Nested repository configs intentionally remain repository reads: callers
/// can provide a separate [`LoadOptions::repository_root`] while keeping the
/// policy package and repository roots distinct.
pub trait ConfigSourceProvider: std::fmt::Debug + Send + Sync {
    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf>;
    fn read_to_string(&self, path: &Path) -> std::io::Result<String>;
    /// Return direct regular YAML files in `dir`. A missing directory is an
    /// empty list; all other I/O failures are errors.
    fn yaml_files(&self, dir: &Path) -> std::io::Result<Vec<PathBuf>>;
}

/// Normal filesystem-backed config source provider.
#[derive(Debug, Default)]
pub struct FilesystemConfigSource;

impl ConfigSourceProvider for FilesystemConfigSource {
    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        path.canonicalize()
    }

    fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
        fs::read_to_string(path)
    }

    fn yaml_files(&self, dir: &Path) -> std::io::Result<Vec<PathBuf>> {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut paths = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path.is_file()
                && matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("yml" | "yaml")
                )
            {
                paths.push(path);
            }
        }
        paths.sort();
        Ok(paths)
    }
}

/// Whether HTTPS `extends:` resolution is permitted for this load.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RemotePolicy {
    #[default]
    Allow,
    Deny,
}

/// Configuration for [`load_with`].
///
/// Defaults retain normal production behavior. Policy runners can inject
/// deterministic environment and package-source providers, deny remote
/// resolution before cache access, and point nested-config discovery at a
/// separate repository root.
#[derive(Debug, Clone)]
pub struct LoadOptions<'catalog> {
    /// Rule-kind and fix-operation capability inventory. Production callers
    /// should always inject the registry they will use to build the config.
    pub kind_catalog: Option<alint_core::KindCatalog<'catalog>>,
    /// Explicit cache. When `None`, a platform-default cache is resolved lazily
    /// on first HTTPS entry.
    pub cache: Option<extends::Cache>,
    /// Explicit fetcher. When `None`, `Fetcher::default()` is used.
    pub fetcher: Option<extends::Fetcher>,
    /// Environment consulted by `{{env.NAME}}` interpolation.
    pub environment: std::sync::Arc<dyn EnvironmentProvider>,
    /// Provider for the root config, local extends, and drop-ins.
    pub config_source: std::sync::Arc<dyn ConfigSourceProvider>,
    /// HTTPS policy checked before cache construction or fetch.
    pub remote_policy: RemotePolicy,
    /// Repository root used by nested-config discovery. When absent, the
    /// top-level config directory remains the repository root.
    pub repository_root: Option<PathBuf>,
}

impl Default for LoadOptions<'_> {
    fn default() -> Self {
        Self {
            kind_catalog: None,
            cache: None,
            fetcher: None,
            environment: std::sync::Arc::new(HostEnvironment),
            config_source: std::sync::Arc::new(FilesystemConfigSource),
            remote_policy: RemotePolicy::Allow,
            repository_root: None,
        }
    }
}

impl<'catalog> LoadOptions<'catalog> {
    /// Start a load using `catalog` as the capability source of truth.
    #[must_use]
    pub fn with_catalog(catalog: alint_core::KindCatalog<'catalog>) -> Self {
        Self {
            kind_catalog: Some(catalog),
            ..Self::default()
        }
    }

    /// Pin HTTPS resolution to an explicit cache path.
    #[must_use]
    pub fn with_cache(cache: extends::Cache) -> Self {
        Self {
            cache: Some(cache),
            ..Self::default()
        }
    }

    /// Add an explicit cache path while retaining the other options.
    #[must_use]
    pub fn cache(mut self, cache: extends::Cache) -> Self {
        self.cache = Some(cache);
        self
    }
}

pub fn parse(yaml: &str) -> Result<Config> {
    // Guard the deep-flow bomb before `yaml_serde`/libyaml-rs processes it
    // super-linearly, same as `load()` / the remote+bundled `extends:` bodies. This
    // is a public entry point, so an external caller's untrusted YAML must be as
    // safe here as through the file loader.
    let yaml_guards = alint_core::yaml_depth::scan_guards(yaml);
    if !yaml_guards.flow_within_limit() {
        return Err(Error::Other(format!(
            "YAML flow nesting exceeds the maximum supported depth ({})",
            alint_core::yaml_depth::MAX_YAML_FLOW_DEPTH
        )));
    }
    if !yaml_guards.expansion_within_limit(yaml) {
        return Err(Error::Other(
            "YAML alias expansion exceeds the maximum supported size".to_string(),
        ));
    }
    let mut config: Config = match yaml_serde::from_str(yaml) {
        Ok(config) => config,
        Err(parse_error) => {
            if let Err(error @ Error::Other(_)) =
                loader::reject_nonliteral_requirement(yaml, "<inline>")
            {
                return Err(error);
            }
            return Err(Error::Yaml(parse_error));
        }
    };
    if config.requires.is_some() {
        loader::reject_nonliteral_decoded_requirement(yaml, "<inline>")?;
    }
    if !config.extends.is_empty() {
        return Err(Error::Other(
            "`extends:` is only resolved when loading from a file; \
             use alint_dsl::load(path) rather than parse(yaml)"
                .into(),
        ));
    }
    if !Config::supports_version(config.version) {
        return Err(Error::Other(format!(
            "<inline>: unsupported config version {} (this build supports {} through {})",
            config.version,
            Config::MIN_SUPPORTED_VERSION,
            Config::CURRENT_VERSION
        )));
    }
    let running_version = RunningVersion::current();
    if let Some(requirements) = config.requires
        && !running_version.satisfies(requirements.alint)
    {
        return Err(Error::Other(format!(
            "<inline>: requires alint {} or newer (running {running_version}; config version {})",
            requirements.alint, config.version
        )));
    }
    config.compatibility.push(ConfigCompatibility {
        source: "<inline>".to_string(),
        source_class: alint_core::ConfigSourceClass::Inline,
        config_version: config.version,
        requires_alint: config.requires.map(|requirements| requirements.alint),
        consumer_version: None,
    });
    validate(&config)?;
    Ok(config)
}

/// Apply an `extends:` entry's `only:` / `except:` filters to the
/// fully-resolved rule set of the extended config. Validates that
/// the two filters are mutually exclusive, that the filter list is
/// non-empty, and that every listed id actually exists in the
/// ruleset (unknown ids are almost always typos worth catching at
/// load time).
pub(crate) fn apply_rule_filter(
    rules: Vec<yaml_serde::Mapping>,
    entry: &alint_core::ExtendsEntry,
    source_display: &str,
) -> Result<Vec<yaml_serde::Mapping>> {
    if entry.only().is_some() && entry.except().is_some() {
        return Err(Error::Other(format!(
            "`extends:` entry {source_display:?}: `only:` and `except:` are mutually exclusive"
        )));
    }
    let Some((filter_ids, mode)) = entry
        .only()
        .map(|ids| (ids, FilterMode::Only))
        .or_else(|| entry.except().map(|ids| (ids, FilterMode::Except)))
    else {
        return Ok(rules);
    };
    if filter_ids.is_empty() {
        return Err(Error::Other(format!(
            "`extends:` entry {source_display:?}: `{}:` is empty; list at least one rule id",
            mode.field_name()
        )));
    }

    let available: std::collections::HashSet<String> = rules
        .iter()
        .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(str::to_string))
        .collect();
    let unknown: Vec<&String> = filter_ids
        .iter()
        .filter(|id| !available.contains(*id))
        .collect();
    if !unknown.is_empty() {
        let mut known: Vec<&String> = available.iter().collect();
        known.sort();
        return Err(Error::Other(format!(
            "`extends:` entry {source_display:?}: {} references unknown rule id(s) {:?}; ruleset ships: {:?}",
            mode.field_name(),
            unknown,
            known,
        )));
    }

    let keep: std::collections::HashSet<&str> = filter_ids.iter().map(String::as_str).collect();
    Ok(rules
        .into_iter()
        .filter(|m| {
            let Some(id) = m.get("id").and_then(|v| v.as_str()) else {
                // No id yet — leave it; downstream deserialize
                // will flag the missing id with a clear error.
                return true;
            };
            match mode {
                FilterMode::Only => keep.contains(id),
                FilterMode::Except => !keep.contains(id),
            }
        })
        .collect())
}

#[derive(Clone, Copy)]
enum FilterMode {
    Only,
    Except,
}

impl FilterMode {
    fn field_name(self) -> &'static str {
        match self {
            Self::Only => "only",
            Self::Except => "except",
        }
    }
}

/// Merge `b` into `a`, with `b` winning on conflicts.
///
/// Semantics:
/// - `rules` dedupe by id; rule mappings are **field-merged**,
///   not replaced — `b`'s keys override `a`'s keys individually.
///   So a child that specifies `{id: X, level: off}` over a
///   parent `{id: X, kind: file_exists, paths: README.md, level:
///   error}` yields a merged rule with kind + paths still set
///   and level overridden. Ordering: `a`'s entries first (in
///   order they first appear), then `b`'s new entries.
/// - `facts` dedupe by id; `b`'s entry replaces `a`'s wholesale
///   (fact kinds are a discriminated union — field-merging
///   `any_file_exists` with `all_files_exist` would produce an
///   invalid fact).
/// - `vars` merged as a map; `b`'s values override.
/// - `ignore` concatenated `a` then `b`.
/// - `version`, `respect_gitignore`, `fix_size_limit` and
///   `nested_configs` take `b`'s value when `b` sets it, else keep
///   `a`'s (defaults are applied once, in `finalize`). An
///   `extends:`'d config's values are dropped before the merge
///   ([`RawConfig::drop_top_level_settings`]).
/// - `extends` is always left empty on the merged result;
///   resolved already.
fn extend_compatibility(
    target: &mut Vec<ConfigCompatibility>,
    incoming: impl IntoIterator<Item = ConfigCompatibility>,
) {
    for record in incoming {
        if !target.contains(&record) {
            target.push(record);
        }
    }
}

fn merged_compatibility(
    mut current: Vec<ConfigCompatibility>,
    incoming: Vec<ConfigCompatibility>,
) -> Vec<ConfigCompatibility> {
    extend_compatibility(&mut current, incoming);
    current
}

/// Facts merged by id: a later fact replaces an earlier one with its id, in
/// the earlier one's place.
fn merge_facts(a: Vec<FactSpec>, b: Vec<FactSpec>) -> Vec<FactSpec> {
    let mut facts_by_id: std::collections::BTreeMap<String, FactSpec> =
        std::collections::BTreeMap::new();
    let mut fact_order: Vec<String> = Vec::new();
    for f in a.into_iter().chain(b) {
        if !facts_by_id.contains_key(&f.id) {
            fact_order.push(f.id.clone());
        }
        facts_by_id.insert(f.id.clone(), f);
    }
    fact_order
        .into_iter()
        .map(|id| facts_by_id.remove(&id).unwrap())
        .collect()
}

pub(crate) fn merge(a: RawConfig, b: RawConfig) -> RawConfig {
    let version = b.version.or(a.version);
    let respect_gitignore = b.respect_gitignore.or(a.respect_gitignore);
    let fix_size_limit = b.fix_size_limit.or(a.fix_size_limit);
    let nested_configs = b.nested_configs.or(a.nested_configs);
    // `allow_out_of_root` is top-level-only: `b` (the later / child
    // config) wins when it sets a non-default value; an inherited
    // (`a`-side) value only survives if the child is silent. Extended
    // rulesets are rejected upstream (`reject_allow_out_of_root_in`),
    // so in practice only the user's top-level config carries a
    // non-default value here.
    let allow_out_of_root = if b.allow_out_of_root.is_confined() {
        a.allow_out_of_root
    } else {
        b.allow_out_of_root
    };
    // `baseline:` is top-level-only (an `extends:`'d value is rejected upstream
    // by `reject_baseline_in`); the later (child) config wins when it sets one.
    let baseline = b.baseline.or(a.baseline);

    let mut ignore = a.ignore;
    ignore.extend(b.ignore);

    // The env-derived mark follows the value that wins: `b` overriding a name
    // with a literal clears it, with an interpolated value sets it.
    let mut env_vars = a.env_vars;
    for name in b.vars.keys() {
        match b.env_vars.get(name) {
            Some(raw) => env_vars.insert(name.clone(), raw.clone()),
            None => env_vars.remove(name),
        };
    }
    let mut vars = a.vars;
    vars.extend(b.vars);
    let mut untrusted_when_var_reads = a.untrusted_when_var_reads;
    untrusted_when_var_reads.extend(b.untrusted_when_var_reads);
    let compatibility = merged_compatibility(a.compatibility, b.compatibility);
    let mut environment_values = a.environment_values;
    loader::add_environment_values(&mut environment_values, &b.environment_values);
    let facts = merge_facts(a.facts, b.facts);

    // Templates merge by id, same shape as rules — later wins
    // on field-level conflict. A child config can replace an
    // upstream template's body wholesale by re-defining the id.
    let mut templates_by_id: std::collections::BTreeMap<String, Mapping> =
        std::collections::BTreeMap::new();
    let mut template_order: Vec<String> = Vec::new();
    let mut template_orphans: Vec<Mapping> = Vec::new();
    for m in a.templates.into_iter().chain(b.templates) {
        let Some(id) = m.get("id").and_then(|v| v.as_str()).map(str::to_string) else {
            template_orphans.push(m);
            continue;
        };
        if let Some(existing) = templates_by_id.get_mut(&id) {
            merge_mapping_fields(existing, m);
        } else {
            template_order.push(id.clone());
            templates_by_id.insert(id, m);
        }
    }
    let mut templates: Vec<Mapping> = template_order
        .into_iter()
        .map(|id| templates_by_id.remove(&id).unwrap())
        .collect();
    templates.extend(template_orphans);

    // Rules: field-merge mappings by id. Rules without an id key
    // can't participate in merge and are passed through unchanged
    // (the final `finalize` step will reject them — RuleSpec
    // requires `id`).
    let mut rules_by_id: std::collections::BTreeMap<String, Mapping> =
        std::collections::BTreeMap::new();
    let mut rule_order: Vec<String> = Vec::new();
    let mut orphans: Vec<Mapping> = Vec::new();
    for m in a.rules.into_iter().chain(b.rules) {
        let Some(id) = m.get("id").and_then(|v| v.as_str()).map(str::to_string) else {
            orphans.push(m);
            continue;
        };
        if let Some(existing) = rules_by_id.get_mut(&id) {
            // Field-merge: b's keys overwrite a's at the top
            // level of the rule mapping. Nested structures (e.g.
            // a `fix:` block or `paths:` include/exclude pair)
            // are replaced wholesale, which matches user
            // expectation — overriding `fix.file_create.content`
            // alone would be too surprising.
            merge_mapping_fields(existing, m);
        } else {
            rule_order.push(id.clone());
            rules_by_id.insert(id, m);
        }
    }
    let mut rules: Vec<Mapping> = rule_order
        .into_iter()
        .map(|id| rules_by_id.remove(&id).unwrap())
        .collect();
    rules.extend(orphans);

    // `trusted_extends:` is top-level-only -- consumed at is_top before any merge,
    // and rejected from an extended / nested source. Concatenate defensively so a
    // stray non-empty value still survives to the per-source rejection upstream.
    let mut trusted_extends = a.trusted_extends;
    trusted_extends.extend(b.trusted_extends);

    RawConfig {
        version,
        requires: None,
        extends: Vec::new(),
        ignore,
        respect_gitignore,
        vars,
        facts,
        templates,
        rules,
        fix_size_limit,
        nested_configs,
        allow_out_of_root,
        baseline,
        trusted_extends,
        env_vars,
        untrusted_when_var_reads,
        compatibility,
        environment_values,
        extends_environment: Vec::new(),
    }
}

fn validate(config: &Config) -> Result<()> {
    if !Config::supports_version(config.version) {
        return Err(Error::Other(format!(
            "unsupported config version {} (this build supports {} through {})",
            config.version,
            Config::MIN_SUPPORTED_VERSION,
            Config::CURRENT_VERSION,
        )));
    }
    let mut seen = std::collections::HashSet::new();
    for rule in &config.rules {
        if !seen.insert(&rule.id) {
            return Err(Error::rule_config(&rule.id, "duplicate rule id in config"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
