//! YAML front-end for alint. Reads a `.alint.yml` and returns a
//! [`alint_core::Config`] that the engine can instantiate.
//!
//! ## Composition model
//!
//! `extends:` resolution happens at the YAML-`Value` layer, not
//! the typed-`Config` layer. Each `.alint.yml` (local, HTTPS,
//! bundled) is parsed into a private `RawConfig` that keeps each
//! rule as a `serde_yaml_ng::Mapping` rather than an
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

use std::fs;
use std::path::{Path, PathBuf};

pub mod bundled;
pub mod extends;
mod interp;
mod loader;
mod nested;
mod trust;

pub(crate) use trust::{
    ENV_VARS_MARKER, PROVENANCE_MARKER, Provenance, SourceClass, UNTRUSTED_FIX_SOURCE_MARKER,
    UntrustedWhenVarRead, demote_content_fixers_in, demote_content_fixers_in_rule,
    find_spawning_fix_op, find_spawning_kind, find_template_placeholder_in_guarded_field,
    has_untrusted_fix_source, mark_provenance_in, mark_untrusted_fix_sources_in,
    merge_mapping_fields, reject_ambiguous_yaml_in, reject_env_expansion_in,
    reject_env_reads_in_when, reject_remote_assembled_env_refs, reject_untrusted_assembled_when,
    reject_untrusted_env_instance_vars, reject_untrusted_env_var_reads,
    strip_fix_promotions_in_rule, take_untrusted_fix_source, union_provenance,
};
pub use trust::{
    SPAWNING_FIX_OPS, SPAWNING_RULE_KINDS, reject_allow_out_of_root_in, reject_baseline_in,
    reject_command_rules_in, reject_fix_promotion_in, reject_fix_promotion_templates_in,
    reject_spawning_fix_op_templates_in, reject_spawning_fix_ops_in, reject_spawning_templates_in,
    reject_trusted_extends_in, reject_untrusted_ignore_in,
};

use alint_core::{Config, Error, FactSpec, Result};
use serde::Deserialize;
use serde_yaml_ng::Mapping;

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

pub fn load(path: &Path) -> Result<Config> {
    load_with(path, &LoadOptions::default())
}

/// Load with explicit options. Primarily useful for tests that
/// want to point HTTPS `extends:` resolution at a scoped cache
/// directory, and for embeddings that want to plug in a custom
/// fetcher.
pub fn load_with(path: &Path, opts: &LoadOptions) -> Result<Config> {
    let mut raw = load_top_level_raw(path, opts)?;

    // Nested `.alint.yml` discovery (opt-in via `nested_configs:
    // true` on the root config). Walks from the root config's
    // directory, finds any sub-directory configs, scopes their
    // rules to their directory, and appends them to the root's
    // rule list.
    if raw.nested_configs == Some(true) {
        let root_dir = path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let canonical_root_cfg = path.canonicalize().map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let discovered = nested::discover_nested(&root_dir, &canonical_root_cfg, &raw)?;
        raw.rules.extend(discovered);
    }

    let merged = raw.finalize()?;
    validate(&merged)?;
    Ok(merged)
}

/// Whether the config at `path` (with its `extends:` chain and
/// `.alint.d/` drop-ins, exactly as [`load`] resolves them) turns on
/// `nested_configs:`. Skips the nested-config walk itself, so it is cheap
/// enough for an embedder (the LSP) to ask which ancestor config governs
/// a file.
pub fn nested_configs_enabled(path: &Path) -> Result<bool> {
    Ok(load_top_level_raw(path, &LoadOptions::default())?.nested_configs == Some(true))
}

/// The top-level config plus its `extends:` chain and `.alint.d/`
/// drop-ins, merged but not yet finalised (and without nested configs).
fn load_top_level_raw(path: &Path, opts: &LoadOptions) -> Result<RawConfig> {
    let mut state = loader::LoadState::default();
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
    let mut raw = loader::load_recursive(path, &mut state, opts, Some(&confine_root), true, &[])?;

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
    for drop_in_path in collect_drop_ins(&drop_in_dir)? {
        // Drop-ins are trust-equivalent to the top-level config (local,
        // user-controlled), so `is_top: true` — they may open the escape hatch.
        let drop_in = loader::load_recursive(
            &drop_in_path,
            &mut state,
            opts,
            Some(&confine_root),
            true,
            // is_top: a drop-in reads its own `trusted_extends:`, so seed empty.
            &[],
        )?;
        raw = merge(raw, drop_in);
    }
    Ok(raw)
}

/// List `.alint.d/*.{yml,yaml}` files alphabetically. Returns
/// an empty Vec when the directory doesn't exist (drop-ins are
/// purely opt-in by mkdir). Non-YAML files are silently
/// skipped so a stray `.gitkeep` or `README.md` in the dir
/// doesn't break loading. Sort order is fixed (lexicographic
/// over the file name) so the merge result is deterministic
/// across filesystems whose readdir order isn't.
fn collect_drop_ins(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(dir).map_err(|source| Error::Io {
        path: dir.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| Error::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let is_yaml = matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("yml" | "yaml")
        );
        if is_yaml {
            entries.push(path);
        }
    }
    entries.sort();
    Ok(entries)
}

/// Intermediate form used during `extends:` resolution. Identical
/// to [`Config`] except that rules are kept as raw
/// `serde_yaml_ng::Mapping`s so overrides can merge per-field
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
    #[serde(default)]
    extends: Vec<alint_core::ExtendsEntry>,
    #[serde(default)]
    ignore: Vec<String>,
    #[serde(default)]
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
    #[serde(default)]
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
    /// Deserialize each rule mapping into a [`RuleSpec`]. This is
    /// where kind-specific validation fires: a rule that never
    /// received a `kind` anywhere in its extends chain produces a
    /// serde error here, referencing the offending rule's id.
    /// Also where `extends_template:` instances expand against
    /// the `templates:` block: the template body is cloned, its
    /// `{{vars.<name>}}` placeholders substituted from the
    /// instance's `vars:` map, and the instance's own
    /// non-template fields field-merge on top.
    fn finalize(self) -> Result<Config> {
        // A process-spawning kind must never hide inside a `templates:`
        // block. Templates are expanded here, *after* the extends/nested
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
            if let Some(kind) = find_spawning_kind(t) {
                return Err(Error::Other(format!(
                    "template {id:?}: `kind: {kind}` spawns a process and is not allowed \
                     in a `templates:` block (including inside a `require:` block) - a \
                     template is expanded after the spawn gate, so this would let a \
                     ruleset run arbitrary code. Declare the command rule directly in \
                     your top-level `rules:`."
                )));
            }
            // The same backstop for a spawning FIX op (see `SPAWNING_FIX_OPS`), at
            // EVERY `require:` depth (audit A1): a template's `fix:` -- or one buried
            // in its `require:` -- splices into its referencing rule at finalize
            // (below), after the extends/nested fix-op spawn gate, so a spawning
            // fixer in a template would smuggle code execution past it. `find_spawning
            // _fix_op` recurses, matching the per-source gate, so a require-nested
            // spawning fix in a top-level template is refused too. Confined to a
            // top-level `rules:` entry like a spawning kind, for EVERY source.
            if let Some(op) = find_spawning_fix_op(t) {
                return Err(Error::Other(format!(
                    "template {id:?}: `fix.{op}` spawns a process and is not allowed \
                     in a `templates:` block - a template is expanded after the spawn \
                     gate, so this would let a ruleset run arbitrary code. Declare the \
                     fix directly on a rule in your top-level `rules:`."
                )));
            }
        }
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
            let mut expanded = expand_template(m, &templates_by_id)?;
            expanded.remove(ENV_VARS_MARKER);
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
                let spawns = find_spawning_kind(&expanded)
                    .map(|k| format!("`kind: {k}`"))
                    .or_else(|| find_spawning_fix_op(&expanded).map(|op| format!("`fix.{op}`")));
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
                demote_content_fixers_in_rule(&mut expanded);
                // An untrusted rule may instantiate a TRUSTED template that promotes
                // a destructive fixer (`file_remove: {applicability: safe}`) and aim
                // it with its own `paths:`. The promotion belongs to the template's
                // author, not to this rule, so the effective fixer falls back to its
                // op's own default tier.
                strip_fix_promotions_in_rule(&mut expanded);
            }
            let spec: alint_core::RuleSpec = serde_yaml_ng::from_value(
                serde_yaml_ng::Value::Mapping(expanded),
            )
            .map_err(|e| {
                Error::rule_config(&id_hint, format!("could not deserialize merged rule: {e}"))
            })?;
            rules.push(spec);
        }
        Ok(Config {
            version: self.version.unwrap_or(0),
            extends: Vec::new(),
            ignore: self.ignore,
            respect_gitignore: self.respect_gitignore.unwrap_or(true),
            vars: self.vars,
            facts: self.facts,
            rules,
            fix_size_limit: self.fix_size_limit.unwrap_or(DEFAULT_FIX_SIZE_LIMIT),
            nested_configs: self.nested_configs.unwrap_or(false),
            allow_out_of_root: self.allow_out_of_root,
            baseline: self.baseline,
        })
    }
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

    let untrusted_fix_source = has_untrusted_fix_source(template) || has_untrusted_fix_source(rule);
    let mut expanded = (*template).clone();
    let template_provenance = expanded.remove(PROVENANCE_MARKER);
    expanded = substitute_template_vars(expanded, &vars);
    expanded.remove("id");

    for (k, v) in rule {
        let key = k.as_str().unwrap_or_default();
        if matches!(
            key,
            "extends_template" | "vars" | UNTRUSTED_FIX_SOURCE_MARKER | PROVENANCE_MARKER
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
            serde_yaml_ng::Value::from(UNTRUSTED_FIX_SOURCE_MARKER),
            serde_yaml_ng::Value::Bool(true),
        );
    }
    Ok(expanded)
}

/// A template instance's `vars:` map, as strings (numbers and bools stringify).
fn instance_vars(rule: &Mapping) -> std::collections::HashMap<String, String> {
    rule.get("vars")
        .and_then(|v| v.as_mapping())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| match (k.as_str(), v) {
                    (Some(key), serde_yaml_ng::Value::String(s)) => {
                        Some((key.to_string(), s.clone()))
                    }
                    (Some(key), serde_yaml_ng::Value::Number(n)) => {
                        Some((key.to_string(), n.to_string()))
                    }
                    (Some(key), serde_yaml_ng::Value::Bool(b)) => {
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
    value: serde_yaml_ng::Value,
    vars: &std::collections::HashMap<String, String>,
) -> serde_yaml_ng::Value {
    use serde_yaml_ng::Value;
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

/// Configuration for `load_with`.
///
/// Defaults enable HTTPS `extends:` resolution against the
/// platform-default user cache and the default fetcher
/// (30 s timeout, 16 MiB body cap, `rustls` TLS). Tests pin both
/// via [`LoadOptions::with_cache`] to avoid touching the user's
/// real cache dir.
#[derive(Debug, Default, Clone)]
pub struct LoadOptions {
    /// Explicit cache. When `None`, a platform-default cache is
    /// resolved lazily on first HTTPS entry.
    pub cache: Option<extends::Cache>,
    /// Explicit fetcher. When `None`, `Fetcher::default()` is used.
    pub fetcher: Option<extends::Fetcher>,
}

impl LoadOptions {
    /// Convenience: pin HTTPS resolution to an explicit cache
    /// path. Used heavily in tests so scenarios don't share state
    /// with each other or the user's real cache.
    #[must_use]
    pub fn with_cache(cache: extends::Cache) -> Self {
        Self {
            cache: Some(cache),
            ..Self::default()
        }
    }
}

pub fn parse(yaml: &str) -> Result<Config> {
    // Guard the deep-flow bomb before `serde_yaml_ng` (libyaml) processes it
    // super-linearly, same as `load()` / the remote+bundled `extends:` bodies. This
    // is a public entry point, so an external caller's untrusted YAML must be as
    // safe here as through the file loader.
    if !alint_core::yaml_depth::flow_depth_within_limit(yaml) {
        return Err(Error::Other(format!(
            "YAML flow nesting exceeds the maximum supported depth ({})",
            alint_core::yaml_depth::MAX_YAML_FLOW_DEPTH
        )));
    }
    if !alint_core::yaml_depth::expansion_within_limit(yaml) {
        return Err(Error::Other(
            "YAML alias expansion exceeds the maximum supported size".to_string(),
        ));
    }
    let config: Config = serde_yaml_ng::from_str(yaml)?;
    if !config.extends.is_empty() {
        return Err(Error::Other(
            "`extends:` is only resolved when loading from a file; \
             use alint_dsl::load(path) rather than parse(yaml)"
                .into(),
        ));
    }
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
    rules: Vec<serde_yaml_ng::Mapping>,
    entry: &alint_core::ExtendsEntry,
) -> Result<Vec<serde_yaml_ng::Mapping>> {
    let url = entry.url();
    if entry.only().is_some() && entry.except().is_some() {
        return Err(Error::Other(format!(
            "`extends:` entry {url:?}: `only:` and `except:` are mutually exclusive"
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
            "`extends:` entry {url:?}: `{}:` is empty; list at least one rule id",
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
            "`extends:` entry {url:?}: {} references unknown rule id(s) {:?}; ruleset ships: {:?}",
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

    let mut facts_by_id: std::collections::BTreeMap<String, FactSpec> =
        std::collections::BTreeMap::new();
    let mut fact_order: Vec<String> = Vec::new();
    for f in a.facts.into_iter().chain(b.facts) {
        if !facts_by_id.contains_key(&f.id) {
            fact_order.push(f.id.clone());
        }
        facts_by_id.insert(f.id.clone(), f);
    }
    let facts: Vec<FactSpec> = fact_order
        .into_iter()
        .map(|id| facts_by_id.remove(&id).unwrap())
        .collect();

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
    }
}

fn validate(config: &Config) -> Result<()> {
    if config.version != Config::CURRENT_VERSION {
        return Err(Error::Other(format!(
            "unsupported config version {} (this build supports {})",
            config.version,
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
