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
    let mut current = Some(start);
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
    let mut visiting = std::collections::HashSet::new();
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
    let mut raw =
        loader::load_recursive(path, &mut visiting, opts, Some(&confine_root), true, &[])?;

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
            &mut visiting,
            opts,
            Some(&confine_root),
            true,
            // is_top: a drop-in reads its own `trusted_extends:`, so seed empty.
            &[],
        )?;
        raw = merge(raw, drop_in);
    }

    // Nested `.alint.yml` discovery (opt-in via `nested_configs:
    // true` on the root config). Walks from the root config's
    // directory, finds any sub-directory configs, scopes their
    // rules to their directory, and appends them to the root's
    // rule list.
    if raw.nested_configs {
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
    #[serde(default)]
    version: u32,
    #[serde(default)]
    extends: Vec<alint_core::ExtendsEntry>,
    #[serde(default)]
    ignore: Vec<String>,
    #[serde(default = "default_respect_gitignore")]
    respect_gitignore: bool,
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
    #[serde(default = "default_fix_size_limit")]
    fix_size_limit: Option<u64>,
    #[serde(default)]
    nested_configs: bool,
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
}

fn default_respect_gitignore() -> bool {
    true
}

#[allow(clippy::unnecessary_wraps)]
fn default_fix_size_limit() -> Option<u64> {
    Some(1 << 20)
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
            let kind = t.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            if SPAWNING_RULE_KINDS.contains(&kind) {
                let id = t.get("id").and_then(|v| v.as_str()).unwrap_or("(unknown)");
                return Err(Error::Other(format!(
                    "template {id:?}: `kind: {kind}` spawns a process and is not allowed \
                     in a `templates:` block - a template is expanded after the spawn \
                     gate, so this would let a ruleset run arbitrary code. Declare the \
                     command rule directly in your top-level `rules:`."
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
                let id = t.get("id").and_then(|v| v.as_str()).unwrap_or("(unknown)");
                return Err(Error::Other(format!(
                    "template {id:?}: `fix.{op}` spawns a process and is not allowed \
                     in a `templates:` block - a template is expanded after the spawn \
                     gate, so this would let a ruleset run arbitrary code. Declare the \
                     fix directly on a rule in your top-level `rules:`."
                )));
            }
        }
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
            let expanded = expand_template(m, &templates_by_id)?;
            let spec: alint_core::RuleSpec = serde_yaml_ng::from_value(
                serde_yaml_ng::Value::Mapping(expanded),
            )
            .map_err(|e| {
                Error::rule_config(&id_hint, format!("could not deserialize merged rule: {e}"))
            })?;
            rules.push(spec);
        }
        Ok(Config {
            version: self.version,
            extends: Vec::new(),
            ignore: self.ignore,
            respect_gitignore: self.respect_gitignore,
            vars: self.vars,
            facts: self.facts,
            rules,
            fix_size_limit: self.fix_size_limit,
            nested_configs: self.nested_configs,
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

    let vars: std::collections::HashMap<String, String> = rule
        .get("vars")
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
        .unwrap_or_default();

    let mut expanded = (*template).clone();
    expanded = substitute_template_vars(expanded, &vars);
    expanded.remove("id");

    for (k, v) in rule {
        let key = k.as_str().unwrap_or_default();
        if matches!(key, "extends_template" | "vars") {
            continue;
        }
        expanded.insert(k.clone(), v.clone());
    }
    Ok(expanded)
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
        Value::String(s) => {
            let rendered = alint_core::template::render_message(&s, |ns, key| {
                if ns == "vars" {
                    vars.get(key).cloned()
                } else {
                    None
                }
            });
            Value::String(rendered)
        }
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
        return Err(Error::Other(format!(
            "YAML alias expansion exceeds the maximum supported node count ({})",
            alint_core::yaml_depth::MAX_YAML_EXPANSION_NODES
        )));
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
/// Rule kinds that spawn an arbitrary user-supplied process.
/// Every one is trust-gated identically by
/// [`reject_command_rules_in`]: it may only be declared in the
/// user's own top-level config, never introduced via `extends:`.
/// Keep in sync with the rule implementations in `alint-rules`
/// that shell out — `command` (per-file CLI),
/// `generated_file_fresh` (runs a generator), `command_idempotent`
/// (runs a checker). Adding a spawn-capable rule kind without
/// adding it here is a code-execution gap.
pub const SPAWNING_RULE_KINDS: &[&str] = &["command", "generated_file_fresh", "command_idempotent"];

/// Fix ops that shell out, trust-gated identically to
/// [`SPAWNING_RULE_KINDS`]: a spawning fix may be declared **only** in the
/// user's own top-level config, never introduced via `extends:` / a nested
/// `.alint.yml` / a `templates:` block / bundled (auto-fix.md 5.5). Enforced by
/// [`reject_spawning_fix_ops_in`] (rules, at every `require:` depth),
/// [`reject_spawning_fix_op_templates_in`] (inherited templates), and a
/// `finalize` backstop that refuses one in ANY source's templates. Adding a
/// spawn-capable op without listing it here is a code-execution gap, exactly as
/// for rule kinds; `spawning_fix_ops_are_gated` asserts every entry is a real op.
/// `git_untrack` (`git rm --cached`) and `command` (a user-supplied `run:` fix
/// command on the `command` rule) are the two spawning fix ops.
pub const SPAWNING_FIX_OPS: &[&str] = &["git_untrack", "command"];

/// Reject any process-spawning rule kind (see
/// [`SPAWNING_RULE_KINDS`]) in the given mapping list. Used by the
/// `extends:` resolver to enforce that only the user's own
/// top-level config can declare a rule that shells out. Same trust
/// model as `alint_core::facts::reject_custom_facts_in` —
/// extending a ruleset must never gain you arbitrary code
/// execution. `source` is shown in the error to help the user
/// identify which extended config introduced the violation.
///
/// (Kept its original name for API stability; it now gates the
/// whole spawning-kind set, not only `kind: command`.)
pub fn reject_command_rules_in(rules: &[Mapping], source: &str) -> Result<()> {
    for rule in rules {
        reject_spawning_in_rule(rule, source)?;
    }
    Ok(())
}

/// Reject a per-rule fix-tier PROMOTION (`fix: { <op>: { applicability: safe } }`)
/// declared in an inherited config. `file_remove` defaults to `Unsafe` -- a bare
/// `alint fix` will not delete a file irreversibly -- and a user may promote it
/// back to `Safe` on a specific rule, but that is ONLY the user's own top-level
/// config's call (auto-fix.md 5.5: an inherited fixer may be *demoted*, never
/// *promoted*). An extended ruleset promoting `file_remove` to `Safe` would
/// silently opt a repo into auto-deletion, so it is refused here. Same trust
/// model as [`reject_command_rules_in`]. Scans nested `require:` blocks too.
pub fn reject_fix_promotion_in(rules: &[Mapping], source: &str) -> Result<()> {
    for rule in rules {
        reject_fix_promotion_in_rule(rule, source)?;
    }
    Ok(())
}

fn reject_fix_promotion_in_rule(rule: &Mapping, source: &str) -> Result<()> {
    if let Some(fix) = rule.get("fix").and_then(|v| v.as_mapping()) {
        for (op, args) in fix {
            let promotes = args
                .as_mapping()
                .and_then(|m| m.get("applicability"))
                .and_then(|v| v.as_str())
                .is_some_and(|a| a.eq_ignore_ascii_case("safe"));
            if promotes {
                let id = rule
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(unknown)");
                let op = op.as_str().unwrap_or("<fix>");
                return Err(Error::Other(format!(
                    "rule {id:?}: `fix.{op}.applicability: safe` promotes a fix to auto-apply and \
                     is only allowed in your own top-level config; an extended config ({source}) \
                     may not opt this repo into auto-applying a destructive fix (it may still \
                     demote to `suggestion`/`never`). Declare the promotion in your top-level \
                     `rules:`."
                )));
            }
        }
    }
    if let Some(require) = rule.get("require").and_then(|v| v.as_sequence()) {
        for nested in require {
            if let Some(nested_map) = nested.as_mapping() {
                reject_fix_promotion_in_rule(nested_map, source)?;
            }
        }
    }
    Ok(())
}

/// The `fix:` ops a remote `extends:` could abuse to mutate YOUR files, so they
/// demote to `suggestion` from an untrusted (non-`trusted_extends:`) remote
/// (auto-fix.md 5.5). Two kinds: ops that write ruleset-authored BYTES (a regex
/// `replace`ment template, the inline `content:` / `content_from:` of a create /
/// prepend / append, a `set_value`), AND ops the ruleset can AIM at a file to
/// change its content or meaning even though the bytes are the victim's own
/// (`sync_from` picks which file overwrites which; `sort` reorders / dedups a
/// file the rule's `paths:` selects). The genuinely fixed-behavior ops (the
/// hygiene normalizers, `file_remove`, `file_rename`, `chmod`, `remove_value`,
/// `dir_create`, `relocate`, `git_untrack`) are honored at their own tier from
/// any source; a destructive one like `file_remove` is already gated by its
/// Unsafe tier, independent of source.
pub(crate) const CONTENT_INJECTING_FIX_OPS: &[&str] = &[
    "replace",
    "file_create",
    "file_prepend",
    "file_append",
    // `set_value` writes the host rule's `equals:` value into the file -- ruleset-
    // authored bytes, so a remote must PROPOSE it, never auto-write (auto-fix.md
    // 5.5). `remove_value` is a DELETION (no ruleset bytes) -> fixed-behavior,
    // gated by its Unsafe tier like `file_remove`, so it is deliberately absent.
    "set_value",
    // `sync_from` overwrites a target with another file's bytes wholesale, and the
    // host `cross_file` rule's `source:` chooses which file overwrites which -- so
    // an untrusted remote could aim it at your files. It PROPOSES, never
    // auto-writes, from an untrusted remote (auto-fix.md 5.5).
    "sync_from",
    // `create_and_register` appends a ruleset-chosen member value into a manifest
    // list (and, in a follow-up, creates a file from ruleset-authored content), so
    // an untrusted remote must PROPOSE it, never auto-write (auto-fix.md 5.5).
    "create_and_register",
    // `sort` writes no ruleset bytes -- it reorders (and, with `unique`, dedups)
    // the victim's OWN lines -- but the host `ordered_block` rule's `paths:` +
    // `comparator` let an untrusted remote AIM a reorder at an order-SIGNIFICANT
    // file (`.gitignore` negation order, `CODEOWNERS` last-match precedence) to
    // change its meaning at the Safe tier. Same "aim it at your files" risk as
    // `sync_from`, so it PROPOSES, never auto-writes, from an untrusted remote.
    "sort",
    // `indent_style` rewrites the victim's OWN leading whitespace (tabs -> spaces),
    // writing no ruleset bytes -- but a remote's `paths:` can AIM the reindent at an
    // indent-SIGNIFICANT file (a `Makefile` recipe needs a literal tab; converting
    // it to spaces is a HARD build break) at the Safe tier. Same aim risk as
    // `sort`, so it PROPOSES, never auto-writes, from an untrusted remote.
    "indent_style",
];

/// Demote every content-injecting fixer in `rules` to `applicability: suggestion`
/// (unless it already declares the stricter `suggestion` / `never`), so a remote
/// `extends:` the user has NOT listed in `trusted_extends:` can PROPOSE a content
/// edit but never auto-write it. Rewrites the raw `Mapping` in place, before the
/// merge, so the built fixer carries the capped tier. Scans nested `require:`
/// blocks too (a `for_each_dir` etc. can bury a content fixer). Mirrors the
/// read-only [`reject_fix_promotion_in`] navigation.
pub(crate) fn demote_content_fixers_in(rules: &mut [Mapping]) {
    for rule in rules.iter_mut() {
        demote_content_fixers_in_rule(rule);
    }
}

fn demote_content_fixers_in_rule(rule: &mut Mapping) {
    if let Some(fix) = rule
        .get_mut("fix")
        .and_then(serde_yaml_ng::Value::as_mapping_mut)
    {
        for (op, args) in fix.iter_mut() {
            let is_content = op
                .as_str()
                .is_some_and(|o| CONTENT_INJECTING_FIX_OPS.contains(&o));
            if !is_content {
                continue;
            }
            let Some(args_map) = args.as_mapping_mut() else {
                continue;
            };
            // Cap to `suggestion`, but never PROMOTE a stricter declared tier: a
            // ruleset that opted its own content fix out entirely (`never`) or down
            // to `suggestion` already stays there. A demote is one-directional.
            let already_stricter = args_map
                .get("applicability")
                .and_then(serde_yaml_ng::Value::as_str)
                .is_some_and(|a| {
                    a.eq_ignore_ascii_case("suggestion") || a.eq_ignore_ascii_case("never")
                });
            if !already_stricter {
                args_map.insert(
                    serde_yaml_ng::Value::from("applicability"),
                    serde_yaml_ng::Value::from("suggestion"),
                );
            }
        }
    }
    if let Some(require) = rule
        .get_mut("require")
        .and_then(serde_yaml_ng::Value::as_sequence_mut)
    {
        for nested in require {
            if let Some(nested_map) = nested.as_mapping_mut() {
                demote_content_fixers_in_rule(nested_map);
            }
        }
    }
}

/// Reject a spawning `kind` in `rule` OR in any of its nested `require:`
/// specs, recursively. `for_each_dir` / `for_each_file` /
/// `every_matching_has` carry a `require:` block of nested rules
/// (`Vec<NestedRuleSpec>`) whose `kind`/`command` flatten into the parent
/// rule's options — a third spawn vector the top-level `kind` check (and a
/// post-`finalize` scan) would miss, since the nested spec is buried in the
/// parent's options and never becomes a top-level `RuleSpec`. We must scan
/// the raw mappings here, before instantiation, at every depth. (If a new
/// rule kind ever adds another `Vec<NestedRuleSpec>` option field, gate it
/// here too.)
fn reject_spawning_in_rule(rule: &Mapping, source: &str) -> Result<()> {
    let kind = rule.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    if SPAWNING_RULE_KINDS.contains(&kind) {
        let id = rule
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown)");
        return Err(Error::Other(format!(
            "rule {id:?}: `kind: {kind}` spawns a process and is only allowed in the \
             user's top-level config; declaring one in an extended config ({source}) - \
             including inside a `require:` block - is refused because it would let a \
             ruleset run arbitrary code"
        )));
    }
    if let Some(require) = rule.get("require").and_then(|v| v.as_sequence()) {
        for nested in require {
            if let Some(nested_map) = nested.as_mapping() {
                reject_spawning_in_rule(nested_map, source)?;
            }
        }
    }
    Ok(())
}

/// Reject any *spawning* fix op (see [`SPAWNING_FIX_OPS`]) declared in the given
/// mapping list. The fix-op analogue of [`reject_command_rules_in`]: a fixer that
/// shells out (today `git_untrack`, `git rm --cached`) is a code-execution
/// surface, so it may be declared ONLY in the user's own top-level config, never
/// introduced via `extends:` / a nested `.alint.yml` / bundled (auto-fix.md 5.5).
/// The existing spawn gate keys on the rule *kind*, so a spawning FIXER attached
/// to a non-spawning kind (a `git_untrack` fix on `file_absent`) slips past it --
/// this gate closes that gap. `source` names the offending config in the error.
/// Scans nested `require:` blocks at every depth, exactly like the kind gate.
pub fn reject_spawning_fix_ops_in(rules: &[Mapping], source: &str) -> Result<()> {
    for rule in rules {
        reject_spawning_fix_op_in_rule(rule, source)?;
    }
    Ok(())
}

fn reject_spawning_fix_op_in_rule(rule: &Mapping, source: &str) -> Result<()> {
    if let Some(op) = find_spawning_fix_op(rule) {
        let id = rule
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown)");
        return Err(Error::Other(format!(
            "rule {id:?}: `fix.{op}` spawns a process and is only allowed in the \
             user's top-level config; declaring one in an extended config ({source}) - \
             including inside a `require:` block or a `templates:` entry - is refused \
             because it would let a ruleset run arbitrary code on a bare `alint fix`"
        )));
    }
    Ok(())
}

/// The name of a spawning fix op (see [`SPAWNING_FIX_OPS`]) declared in `rule`'s
/// `fix:` block or any nested `require:` (recursively), if any. Shared by the
/// per-source refusal ([`reject_spawning_fix_op_in_rule`]) and the `finalize`
/// template backstop so both scan to the SAME depth: a spawning fix must be
/// refused whether it sits at a rule/template's top level OR inside its `require:`.
fn find_spawning_fix_op(rule: &Mapping) -> Option<&str> {
    if let Some(fix) = rule.get("fix").and_then(|v| v.as_mapping()) {
        for (op, _args) in fix {
            if let Some(op) = op.as_str()
                && SPAWNING_FIX_OPS.contains(&op)
            {
                return Some(op);
            }
        }
    }
    if let Some(require) = rule.get("require").and_then(|v| v.as_sequence()) {
        for nested in require {
            if let Some(nested_map) = nested.as_mapping()
                && let Some(op) = find_spawning_fix_op(nested_map)
            {
                return Some(op);
            }
        }
    }
    None
}

/// Reject a spawning fix op declared inside a `templates:` block of an inherited
/// ruleset. A template's `fix:` block splices into its referencing rule at
/// `finalize` (after the per-rule gate above), so a spawning fixer smuggled
/// through a `templates:` entry would otherwise expand into a spawning fix past
/// the gate -- the fix-op analogue of [`reject_spawning_templates_in`]. A
/// template is shaped like a rule (`fix:` + optional `require:`), so the same
/// per-rule scan applies. `finalize` enforces the same invariant for every
/// source; this earlier per-source check names the offending ruleset.
pub fn reject_spawning_fix_op_templates_in(templates: &[Mapping], source: &str) -> Result<()> {
    for template in templates {
        reject_spawning_fix_op_in_rule(template, source)?;
    }
    Ok(())
}

/// Reject any process-spawning rule kind (see [`SPAWNING_RULE_KINDS`])
/// declared inside a `templates:` block of an inherited ruleset. A
/// template instance (`extends_template:`) carries no `kind` of its own,
/// so a spawning template would slip past [`reject_command_rules_in`]
/// (which inspects `rules[].kind`) and expand into a `command` rule at
/// `finalize` time — the C1 code-execution bypass. Spawning kinds are
/// confined to the user's own top-level `rules:`, never a template.
/// `finalize` enforces the same invariant for every source; this earlier
/// per-source check names the offending ruleset (`source`) in the error.
pub fn reject_spawning_templates_in(templates: &[Mapping], source: &str) -> Result<()> {
    for template in templates {
        let kind = template.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        if SPAWNING_RULE_KINDS.contains(&kind) {
            let id = template
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("(unknown)");
            return Err(Error::Other(format!(
                "template {id:?}: `kind: {kind}` spawns a process and is not allowed in \
                 an inherited ruleset ({source}); a spawning kind may only appear in a \
                 top-level `rules:` entry, never in a `templates:` block, because a \
                 template instance expands after the spawn gate and would let the \
                 ruleset run arbitrary code"
            )));
        }
    }
    Ok(())
}

/// Reject a fix-tier PROMOTION (`fix: { <op>: { applicability: safe } }`)
/// declared inside a `templates:` block of an inherited ruleset -- the template
/// analogue of [`reject_fix_promotion_in`]. A template instance
/// (`extends_template:`) carries the template's `fix:` block, which is spliced
/// into the referencing rule at `finalize` time -- AFTER the rule-level
/// promotion gate has run -- so a promotion smuggled through a template would
/// silently opt a repo into auto-applying a destructive fix (a bare `alint fix`
/// deleting files), defeating the whole point of that gate (5.5: inherited
/// fixers may be demoted, never promoted). Same trust model as
/// [`reject_spawning_templates_in`]; each template is rule-shaped, so it is
/// scanned exactly like a rule (its `fix` block plus any nested `require:`).
///
/// A `finalize` backstop cannot substitute here: unlike a spawning kind (never
/// legal in any template), a promotion in the user's OWN top-level template is
/// legitimate, so the refusal must be source-aware -- which only this
/// extends-time, per-source check is.
pub fn reject_fix_promotion_templates_in(templates: &[Mapping], source: &str) -> Result<()> {
    for template in templates {
        reject_fix_promotion_in_rule(template, source)?;
    }
    Ok(())
}

/// Reject a non-default `allow_out_of_root:` in an inherited ruleset.
/// Like [`reject_command_rules_in`], the path-confinement escape hatch
/// may only be opened by the user's own top-level config — an
/// `extends:`'d ruleset granting itself reads outside the repo root is
/// the exact threat confinement exists to stop. `source` names the
/// offending config. See `docs/design/v0.12/allow_out_of_root.md`.
pub fn reject_allow_out_of_root_in(allow: &alint_core::AllowOutOfRoot, source: &str) -> Result<()> {
    if !allow.is_confined() {
        return Err(Error::Other(format!(
            "`allow_out_of_root:` is only allowed in the user's top-level config; \
             declaring it in an extended config ({source}) is refused because it would \
             let a ruleset grant itself reads outside the repo root"
        )));
    }
    Ok(())
}

/// Reject a `baseline:` in an inherited ruleset. Like
/// [`reject_allow_out_of_root_in`], the baseline path is a trusted top-level
/// input: an `extends:`'d ruleset that pointed the gate at its own baseline
/// could silently suppress findings the user never reviewed. `source` names
/// the offending config. See `docs/design/baseline.md` §2.3.
pub fn reject_baseline_in(baseline: &Option<std::path::PathBuf>, source: &str) -> Result<()> {
    if baseline.is_some() {
        return Err(Error::Other(format!(
            "`baseline:` is only allowed in the user's top-level config; \
             declaring it in an extended config ({source}) is refused because it would \
             let a ruleset choose which findings the gate suppresses"
        )));
    }
    Ok(())
}

/// Reject a `trusted_extends:` in an inherited ruleset. The allowlist grants a
/// remote's content-injecting fixers auto-apply rights, so a remote (or any
/// non-top-level config) that could set it would allowlist ITSELF, defeating the
/// gate. Only the user's own top-level config (and its `.alint.d/` drop-ins) may
/// grant trust. `source` names the offending config. See auto-fix.md 5.5.
pub fn reject_trusted_extends_in(trusted_extends: &[String], source: &str) -> Result<()> {
    if !trusted_extends.is_empty() {
        return Err(Error::Other(format!(
            "`trusted_extends:` is only allowed in the user's top-level config; \
             declaring it in an extended config ({source}) is refused because it would \
             let a ruleset allowlist itself into auto-applying its own content fixers"
        )));
    }
    Ok(())
}

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
/// - `respect_gitignore` takes `b`'s value (its default hides
///   "unset"; known v0.2 limitation).
/// - `version` takes `b`'s value.
/// - `fix_size_limit` takes `b`'s value (same "default hides
///   unset" caveat as `respect_gitignore`).
/// - `extends` is always left empty on the merged result;
///   resolved already.
pub(crate) fn merge(a: RawConfig, b: RawConfig) -> RawConfig {
    let version = b.version;
    let respect_gitignore = b.respect_gitignore;
    let fix_size_limit = b.fix_size_limit;
    let nested_configs = b.nested_configs;
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

    let mut vars = a.vars;
    vars.extend(b.vars);

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
            for (k, v) in m {
                existing.insert(k, v);
            }
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
            for (k, v) in m {
                existing.insert(k, v);
            }
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
