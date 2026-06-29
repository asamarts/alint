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
    let mut raw = loader::load_recursive(path, &mut visiting, opts)?;

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
        let drop_in = loader::load_recursive(&drop_in_path, &mut visiting, opts)?;
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
                     in a `templates:` block — a template is expanded after the spawn \
                     gate, so this would let a ruleset run arbitrary code. Declare the \
                     command rule directly in your top-level `rules:`."
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
                "template `{template_id}` itself references `extends_template:` — \
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
             user's top-level config; declaring one in an extended config ({source}) — \
             including inside a `require:` block — is refused because it would let a \
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
mod tests {
    use super::*;

    #[test]
    fn collect_drop_ins_handles_missing_dir() {
        // Missing `.alint.d/` is the common case (drop-ins
        // are opt-in by mkdir); should be silent.
        let dir = std::path::Path::new("/nonexistent/.alint.d");
        assert_eq!(collect_drop_ins(dir).unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn collect_drop_ins_yaml_files_only_alphabetical() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("99-late.yaml"), "version: 1\n").unwrap();
        std::fs::write(tmp.path().join("00-early.yml"), "version: 1\n").unwrap();
        std::fs::write(tmp.path().join("50-mid.yml"), "version: 1\n").unwrap();
        // Non-yaml files in the same dir should be skipped.
        std::fs::write(tmp.path().join("README.md"), "ignored\n").unwrap();
        std::fs::write(tmp.path().join(".gitkeep"), "").unwrap();
        let entries = collect_drop_ins(tmp.path()).unwrap();
        let names: Vec<&str> = entries
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, ["00-early.yml", "50-mid.yml", "99-late.yaml"]);
    }

    #[test]
    fn template_expands_into_concrete_rule() {
        let yaml = r"
version: 1
templates:
  - id: dir-has-readme
    kind: pair
    primary: '{{vars.dir}}/**/*'
    partner: '{{vars.dir}}/README.md'
    level: warning
    message: 'every {{vars.dir}}/* should have a README'
rules:
  - extends_template: dir-has-readme
    id: pkgs-have-readme
    vars:
      dir: packages
";
        let cfg: RawConfig = serde_yaml_ng::from_str(yaml).unwrap();
        let final_cfg = cfg.finalize().unwrap();
        assert_eq!(final_cfg.rules.len(), 1);
        let r = &final_cfg.rules[0];
        assert_eq!(r.id, "pkgs-have-readme");
        assert_eq!(r.kind, "pair");
    }

    #[test]
    fn template_supports_multiple_instances() {
        let yaml = r"
version: 1
templates:
  - id: dir-has-readme
    kind: pair
    primary: '{{vars.dir}}/**/*'
    partner: '{{vars.dir}}/README.md'
    level: warning
rules:
  - extends_template: dir-has-readme
    id: pkgs-have-readme
    vars: { dir: packages }
  - extends_template: dir-has-readme
    id: services-have-readme
    vars: { dir: services }
  - extends_template: dir-has-readme
    id: apps-have-readme
    vars: { dir: apps }
";
        let cfg: RawConfig = serde_yaml_ng::from_str(yaml).unwrap();
        let final_cfg = cfg.finalize().unwrap();
        let ids: Vec<&str> = final_cfg.rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "pkgs-have-readme",
                "services-have-readme",
                "apps-have-readme"
            ]
        );
    }

    #[test]
    fn template_instance_can_override_field() {
        let yaml = r"
version: 1
templates:
  - id: dir-has-readme
    kind: pair
    primary: '{{vars.dir}}/**/*'
    partner: '{{vars.dir}}/README.md'
    level: warning
rules:
  - extends_template: dir-has-readme
    id: critical-readme
    level: error
    vars: { dir: services }
";
        let cfg: RawConfig = serde_yaml_ng::from_str(yaml).unwrap();
        let final_cfg = cfg.finalize().unwrap();
        assert_eq!(final_cfg.rules[0].level, alint_core::Level::Error);
    }

    #[test]
    fn template_unknown_id_errors_clearly() {
        let yaml = r"
version: 1
templates:
  - id: real-template
    kind: file_exists
    paths: [X]
rules:
  - extends_template: typo-template
    id: my-rule
";
        let cfg: RawConfig = serde_yaml_ng::from_str(yaml).unwrap();
        let err = cfg.finalize().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("typo-template"));
        assert!(msg.contains("unknown template"));
    }

    #[test]
    fn template_cannot_extend_another_template() {
        let yaml = r"
version: 1
templates:
  - id: outer
    extends_template: inner
    kind: file_exists
    paths: [X]
  - id: inner
    kind: file_exists
    paths: [Y]
rules:
  - extends_template: outer
    id: my-rule
";
        let cfg: RawConfig = serde_yaml_ng::from_str(yaml).unwrap();
        let err = cfg.finalize().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("leaf-only"));
    }

    #[test]
    fn template_substitutes_inside_lists_and_nested_mappings() {
        let yaml = r"
version: 1
templates:
  - id: list-and-nested
    kind: file_exists
    level: warning
    paths:
      - '{{vars.dir}}/README.md'
      - '{{vars.dir}}/LICENSE'
    fix:
      file_create:
        content: 'Hello, {{vars.dir}}!'
        path: '{{vars.dir}}/README.md'
rules:
  - extends_template: list-and-nested
    id: my-rule
    vars: { dir: pkg }
";
        let cfg: RawConfig = serde_yaml_ng::from_str(yaml).unwrap();
        let final_cfg = cfg.finalize().unwrap();
        let r = &final_cfg.rules[0];
        let paths = r.paths.as_ref().unwrap();
        let paths_str = format!("{paths:?}");
        assert!(paths_str.contains("pkg/README.md"));
        assert!(paths_str.contains("pkg/LICENSE"));
        assert!(matches!(
            r.fix,
            Some(alint_core::FixSpec::FileCreate { .. })
        ));
    }

    #[test]
    fn drop_ins_merge_into_main_config_with_field_level_override() {
        // End-to-end: a `.alint.yml` next to a `.alint.d/`
        // dir; the drop-in's `id: main-rule` field-overrides
        // the main config's level. Mirrors the `/etc/*.d/`
        // mental model: drop-ins win on conflict.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(".alint.yml"),
            "version: 1\nrules:\n  - {id: main-rule, kind: file_exists, paths: [X], level: error}\n",
        )
        .unwrap();
        std::fs::create_dir_all(tmp.path().join(".alint.d")).unwrap();
        std::fs::write(
            tmp.path().join(".alint.d/00-base.yml"),
            "version: 1\nrules:\n  - {id: extra-rule, kind: file_exists, paths: [Y], level: warning}\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join(".alint.d/50-override.yml"),
            "version: 1\nrules:\n  - {id: main-rule, level: warning}\n",
        )
        .unwrap();
        let cfg = load(&tmp.path().join(".alint.yml")).unwrap();
        let by_id: std::collections::HashMap<&str, alint_core::Level> =
            cfg.rules.iter().map(|r| (r.id.as_str(), r.level)).collect();
        assert_eq!(
            by_id.get("main-rule").copied(),
            Some(alint_core::Level::Warning)
        );
        assert_eq!(
            by_id.get("extra-rule").copied(),
            Some(alint_core::Level::Warning)
        );
        assert_eq!(cfg.rules.len(), 2);
    }

    #[test]
    fn extends_with_allow_out_of_root_is_rejected() {
        // Security: an inherited ruleset may not open the
        // path-confinement escape hatch — only the user's own top-level
        // config can (the same trust model as command/custom kinds).
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("base.yml"),
            "version: 1\nallow_out_of_root: true\nrules: []\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join(".alint.yml"),
            "version: 1\nextends: [./base.yml]\nrules: []\n",
        )
        .unwrap();
        let err = load(&tmp.path().join(".alint.yml")).unwrap_err();
        assert!(err.to_string().contains("allow_out_of_root"), "{err}");
    }

    #[test]
    fn top_level_allow_out_of_root_is_honored() {
        // The same key in the user's own top-level config is accepted
        // and resolves onto `Config`.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(".alint.yml"),
            "version: 1\nallow_out_of_root:\n  kinds: [pair_hash]\nrules: []\n",
        )
        .unwrap();
        let cfg = load(&tmp.path().join(".alint.yml")).unwrap();
        assert!(cfg.allow_out_of_root.allows("any", "pair_hash"));
        assert!(!cfg.allow_out_of_root.allows("any", "json_schema_passes"));
    }

    #[test]
    fn top_level_baseline_is_honored() {
        // The `baseline:` key in the user's own top-level config resolves
        // onto `Config` (the CLI then suppresses against it).
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(".alint.yml"),
            "version: 1\nbaseline: .alint-baseline.json\nrules: []\n",
        )
        .unwrap();
        let cfg = load(&tmp.path().join(".alint.yml")).unwrap();
        assert_eq!(
            cfg.baseline.as_deref(),
            Some(std::path::Path::new(".alint-baseline.json"))
        );
    }

    #[test]
    fn extends_with_baseline_is_rejected() {
        // Security: an inherited ruleset must not choose which findings the
        // gate suppresses — only the user's own top-level config sets it.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("base.yml"),
            "version: 1\nbaseline: sneaky.json\nrules: []\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join(".alint.yml"),
            "version: 1\nextends: [./base.yml]\nrules: []\n",
        )
        .unwrap();
        let err = load(&tmp.path().join(".alint.yml")).unwrap_err();
        assert!(err.to_string().contains("baseline"), "{err}");
    }

    #[test]
    fn load_interpolates_env_default_through_real_path() {
        // End-to-end through `load()`: the value field uses an
        // unset env var with a default, so it resolves hermetically
        // (no env var set — Rust 2024 marks `set_var` unsafe). Proves
        // the YAML-value → interpolate → RawConfig wiring in the
        // loader fires and that `vars.`/`id:` are left intact.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(".alint.yml"),
            "version: 1\nrules:\n  - id: spdx\n    kind: file_exists\n    \
             paths: \"{{env.ALINT_TEST_UNSET_DIR | default('src')}}/X\"\n    level: error\n",
        )
        .unwrap();
        let cfg = load(&tmp.path().join(".alint.yml")).unwrap();
        assert_eq!(cfg.rules.len(), 1);
        assert_eq!(cfg.rules[0].id, "spdx");
        // `id:` is in SKIP_KEYS, never interpolated; `paths:` is.
        let paths = format!("{:?}", cfg.rules[0].paths);
        assert!(paths.contains("src/X"), "paths not interpolated: {paths}");
    }

    #[test]
    fn load_errors_on_undefined_env_without_default() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(".alint.yml"),
            "version: 1\nrules:\n  - id: r\n    kind: file_exists\n    \
             paths: \"{{env.ALINT_TEST_DEFINITELY_UNSET}}\"\n    level: error\n",
        )
        .unwrap();
        let err = load(&tmp.path().join(".alint.yml")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("interpolation error"), "{msg}");
        assert!(msg.contains("ALINT_TEST_DEFINITELY_UNSET"), "{msg}");
    }

    #[test]
    fn parses_minimal_config() {
        let yaml = r"
version: 1
rules:
  - id: readme
    kind: file_exists
    level: error
    paths: README.md
";
        let cfg = parse(yaml).unwrap();
        assert_eq!(cfg.version, 1);
        assert_eq!(cfg.rules.len(), 1);
        assert_eq!(cfg.rules[0].id, "readme");
        assert_eq!(cfg.rules[0].kind, "file_exists");
    }

    #[test]
    fn rejects_wrong_version() {
        let yaml = "version: 99\nrules: []\n";
        assert!(parse(yaml).is_err());
    }

    #[test]
    fn parse_rejects_config_with_extends() {
        // `parse(yaml)` can't resolve a path-relative `extends:` —
        // load_recursive needs a base path. Error rather than
        // silently ignore.
        let yaml = "version: 1\nextends: [base.yml]\nrules: []\n";
        let err = parse(yaml).unwrap_err();
        assert!(err.to_string().contains("extends"));
    }

    #[test]
    fn load_resolves_local_extends_and_merges_rules() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base.yml");
        let child = tmp.path().join(".alint.yml");
        std::fs::write(
            &base,
            r"version: 1
rules:
  - id: base-readme
    kind: file_exists
    paths: README.md
    level: error
  - id: shared
    kind: file_exists
    paths: X
    level: warning
",
        )
        .unwrap();
        std::fs::write(
            &child,
            r"version: 1
extends: [./base.yml]
rules:
  - id: shared
    kind: file_exists
    paths: X
    level: error   # child override wins
  - id: child-only
    kind: file_exists
    paths: Y
    level: warning
",
        )
        .unwrap();

        let cfg = load(&child).unwrap();
        let ids: Vec<&str> = cfg.rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["base-readme", "shared", "child-only"]);
        let shared = cfg.rules.iter().find(|r| r.id == "shared").unwrap();
        assert_eq!(shared.level, alint_core::Level::Error);
    }

    #[test]
    fn load_merges_vars_and_appends_ignore() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base.yml");
        let child = tmp.path().join(".alint.yml");
        std::fs::write(
            &base,
            r"version: 1
ignore: [target]
vars:
  from_base: base
  shared: base
rules: []
",
        )
        .unwrap();
        std::fs::write(
            &child,
            r"version: 1
extends: [./base.yml]
ignore: [node_modules]
vars:
  from_child: child
  shared: child
rules: []
",
        )
        .unwrap();

        let cfg = load(&child).unwrap();
        assert_eq!(
            cfg.ignore,
            vec!["target".to_string(), "node_modules".to_string()]
        );
        assert_eq!(cfg.vars.get("from_base"), Some(&"base".to_string()));
        assert_eq!(cfg.vars.get("from_child"), Some(&"child".to_string()));
        assert_eq!(cfg.vars.get("shared"), Some(&"child".to_string()));
    }

    #[test]
    fn load_detects_cycle() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.yml");
        let b = tmp.path().join("b.yml");
        std::fs::write(&a, "version: 1\nextends: [./b.yml]\nrules: []\n").unwrap();
        std::fs::write(&b, "version: 1\nextends: [./a.yml]\nrules: []\n").unwrap();
        let err = load(&a).unwrap_err().to_string();
        assert!(err.contains("cycle"), "{err}");
    }

    #[test]
    fn extends_only_keeps_listed_rules() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base.yml");
        let child = tmp.path().join(".alint.yml");
        std::fs::write(
            &base,
            "version: 1
rules:
  - id: a
    kind: file_exists
    paths: A
    level: error
  - id: b
    kind: file_exists
    paths: B
    level: error
  - id: c
    kind: file_exists
    paths: C
    level: error
",
        )
        .unwrap();
        std::fs::write(
            &child,
            "version: 1
extends:
  - url: ./base.yml
    only: [b]
rules: []
",
        )
        .unwrap();
        let cfg = load(&child).unwrap();
        let ids: Vec<&str> = cfg.rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["b"]);
    }

    #[test]
    fn extends_except_drops_listed_rules() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base.yml");
        let child = tmp.path().join(".alint.yml");
        std::fs::write(
            &base,
            "version: 1
rules:
  - id: a
    kind: file_exists
    paths: A
    level: error
  - id: b
    kind: file_exists
    paths: B
    level: error
",
        )
        .unwrap();
        std::fs::write(
            &child,
            "version: 1
extends:
  - url: ./base.yml
    except: [a]
rules: []
",
        )
        .unwrap();
        let cfg = load(&child).unwrap();
        let ids: Vec<&str> = cfg.rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["b"]);
    }

    #[test]
    fn extends_rejects_only_and_except_together() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base.yml");
        let child = tmp.path().join(".alint.yml");
        std::fs::write(
            &base,
            "version: 1
rules:
  - id: a
    kind: file_exists
    paths: A
    level: error
",
        )
        .unwrap();
        std::fs::write(
            &child,
            "version: 1
extends:
  - url: ./base.yml
    only: [a]
    except: [a]
rules: []
",
        )
        .unwrap();
        let err = load(&child).unwrap_err().to_string();
        assert!(err.contains("mutually exclusive"), "{err}");
    }

    #[test]
    fn extends_rejects_unknown_rule_id_in_filter() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base.yml");
        let child = tmp.path().join(".alint.yml");
        std::fs::write(
            &base,
            "version: 1
rules:
  - id: a
    kind: file_exists
    paths: A
    level: error
",
        )
        .unwrap();
        std::fs::write(
            &child,
            "version: 1
extends:
  - url: ./base.yml
    only: [does-not-exist]
rules: []
",
        )
        .unwrap();
        let err = load(&child).unwrap_err().to_string();
        assert!(err.contains("does-not-exist"), "{err}");
        assert!(err.contains("unknown rule id"), "{err}");
    }

    #[test]
    fn extends_rejects_empty_filter_list() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base.yml");
        let child = tmp.path().join(".alint.yml");
        std::fs::write(
            &base,
            "version: 1
rules:
  - id: a
    kind: file_exists
    paths: A
    level: error
",
        )
        .unwrap();
        std::fs::write(
            &child,
            "version: 1
extends:
  - url: ./base.yml
    only: []
rules: []
",
        )
        .unwrap();
        let err = load(&child).unwrap_err().to_string();
        assert!(err.contains("empty"), "{err}");
    }

    #[test]
    fn load_rejects_remote_extends_without_sri() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".alint.yml");
        std::fs::write(
            &path,
            "version: 1\nextends: [\"https://example.com/base.yml\"]\nrules: []\n",
        )
        .unwrap();
        let opts = LoadOptions::with_cache(extends::Cache::at(tmp.path().join("cache")));
        let err = load_with(&path, &opts).unwrap_err().to_string();
        assert!(err.contains("integrity hash"), "{err}");
        assert!(err.contains("https://example.com"), "{err}");
    }

    #[test]
    fn load_resolves_https_extends_via_cache_hit() {
        use sha2::{Digest, Sha256};

        // The remote body; could be anything valid.
        let remote_body = b"version: 1\nrules:\n  - id: inherited\n    kind: file_exists\n    paths: INHERITED.md\n    level: warning\n";

        // Pre-compute the SRI so the scenario is hermetic and the
        // integrity check on read succeeds.
        let mut hasher = Sha256::new();
        hasher.update(remote_body);
        let digest = hasher.finalize();
        let mut hex = String::with_capacity(digest.len() * 2);
        for b in &digest {
            use std::fmt::Write as _;
            write!(hex, "{b:02x}").unwrap();
        }
        let sri_str = format!("sha256-{hex}");

        let tmp = tempfile::tempdir().unwrap();
        let cache = extends::Cache::at(tmp.path().join("cache"));
        let sri = extends::Sri::parse(&sri_str).unwrap();

        // Seed the cache so the loader hits it instead of the network.
        cache.put(&sri, remote_body).unwrap();

        // Local .alint.yml references the remote config + adds one
        // local rule of its own.
        let url = format!("https://example.invalid/base.yml#{sri_str}");
        let config_path = tmp.path().join(".alint.yml");
        std::fs::write(
            &config_path,
            format!(
                "version: 1\nextends: [\"{url}\"]\nrules:\n  - id: local\n    kind: file_exists\n    paths: LOCAL.md\n    level: error\n"
            ),
        )
        .unwrap();

        let opts = LoadOptions::with_cache(cache);
        let cfg = load_with(&config_path, &opts).unwrap();
        let ids: Vec<&str> = cfg.rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["inherited", "local"]);
    }

    #[test]
    fn load_rejects_custom_fact_declared_in_local_extends() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base.yml");
        let child = tmp.path().join(".alint.yml");
        std::fs::write(
            &base,
            r#"version: 1
facts:
  - id: from_base
    custom:
      argv: ["/bin/true"]
rules: []
"#,
        )
        .unwrap();
        std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();
        let err = load(&child).unwrap_err().to_string();
        assert!(err.contains("custom"), "{err}");
        assert!(err.contains("base.yml"), "{err}");
    }

    #[test]
    fn load_allows_custom_fact_in_top_level_config() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".alint.yml");
        std::fs::write(
            &path,
            r#"version: 1
facts:
  - id: whoami
    custom:
      argv: ["/bin/true"]
rules: []
"#,
        )
        .unwrap();
        let cfg = load(&path).unwrap();
        assert_eq!(cfg.facts.len(), 1);
        assert_eq!(cfg.facts[0].id, "whoami");
    }

    #[test]
    fn load_rejects_command_rule_declared_in_local_extends() {
        // Mirror of the custom-fact gate. A `kind: command` rule
        // hidden in an extended config must be refused — otherwise
        // adopting a published ruleset would imply granting it
        // arbitrary process execution on the user's machine.
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base.yml");
        let child = tmp.path().join(".alint.yml");
        std::fs::write(
            &base,
            r#"version: 1
rules:
  - id: shellcheck-from-base
    kind: command
    paths: "**/*.sh"
    command: ["shellcheck", "{path}"]
    level: error
"#,
        )
        .unwrap();
        std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();
        let err = load(&child).unwrap_err().to_string();
        assert!(err.contains("command"), "{err}");
        assert!(err.contains("base.yml"), "{err}");
    }

    #[test]
    fn load_rejects_every_spawning_kind_in_extends_not_just_command() {
        // Regression for the closed trust-gate gap:
        // `generated_file_fresh` and `command_idempotent` shell
        // out identically to `command`, so an extended config
        // declaring either must be refused too — otherwise
        // adopting a ruleset implies arbitrary code execution.
        for (kind, body) in [
            (
                "generated_file_fresh",
                "    file: out.txt\n    command: [\"sh\", \"-c\", \"echo pwn\"]\n",
            ),
            (
                "command_idempotent",
                "    command: [\"sh\", \"-c\", \"echo pwn\"]\n",
            ),
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let base = tmp.path().join("base.yml");
            let child = tmp.path().join(".alint.yml");
            std::fs::write(
                &base,
                format!(
                    "version: 1\nrules:\n  - id: sneaky\n    kind: {kind}\n{body}    level: error\n"
                ),
            )
            .unwrap();
            std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();
            let err = load(&child).unwrap_err().to_string();
            assert!(err.contains(kind), "{kind} not gated: {err}");
            assert!(err.contains("arbitrary code"), "{kind}: {err}");
        }
    }

    #[test]
    fn load_rejects_spawning_template_smuggled_via_extends() {
        // C1 (RCE bypass): an extended ruleset can't carry a spawning
        // `kind` directly (caught by `reject_command_rules_in`), but it
        // could hide one in a `templates:` block and reference it from a
        // `kind`-less `extends_template:` rule. The template expands into a
        // `command` rule at finalize, *after* the gate — so without the
        // template gate the consumer gets arbitrary code execution by
        // adding a single SRI-pinned `extends:` line. Body is
        // self-contained, mirroring a real published ruleset.
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base.yml");
        let child = tmp.path().join(".alint.yml");
        std::fs::write(
            &base,
            "version: 1\ntemplates:\n  - id: t\n    kind: command\n    command: [\"sh\", \"-c\", \"echo pwn\"]\n    paths: \"**/*\"\n    level: error\nrules:\n  - id: pwned\n    extends_template: t\n",
        )
        .unwrap();
        std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();
        let err = load(&child).unwrap_err().to_string();
        assert!(err.contains("command"), "kind not named: {err}");
        assert!(err.contains("base.yml"), "source not named: {err}");
        assert!(err.contains("arbitrary code"), "{err}");
    }

    #[test]
    fn finalize_rejects_a_top_level_spawning_template() {
        // The invariant holds with no `extends:` at all: a spawning kind
        // may never live in a `templates:` block (it would be a latent
        // bypass the moment the config is extended or a nested config
        // references it), so even a top-level spawning template is a hard
        // error. `finalize` is the source-agnostic backstop.
        let tmp = tempfile::tempdir().unwrap();
        let cfg = tmp.path().join(".alint.yml");
        std::fs::write(
            &cfg,
            "version: 1\ntemplates:\n  - id: t\n    kind: generated_file_fresh\n    file: out.txt\n    command: [\"sh\", \"-c\", \"echo pwn\"]\n    level: error\nrules:\n  - id: x\n    extends_template: t\n",
        )
        .unwrap();
        let err = load(&cfg).unwrap_err().to_string();
        assert!(err.contains("generated_file_fresh"), "{err}");
        assert!(err.contains("templates"), "{err}");
    }

    #[test]
    fn top_level_command_rule_still_loads() {
        // Guard against over-rejection: a process-spawning rule declared
        // directly in the user's own top-level `rules:` is the allowed case
        // and must keep working.
        let tmp = tempfile::tempdir().unwrap();
        let cfg = tmp.path().join(".alint.yml");
        std::fs::write(
            &cfg,
            "version: 1\nrules:\n  - id: run-true\n    kind: command\n    command: [\"true\"]\n    paths: \"**/*\"\n    level: error\n",
        )
        .unwrap();
        let loaded = load(&cfg).expect("a top-level command rule should still load");
        assert_eq!(loaded.rules.len(), 1);
    }

    #[test]
    fn load_allows_command_rule_in_top_level_config() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".alint.yml");
        std::fs::write(
            &path,
            r#"version: 1
rules:
  - id: shellcheck
    kind: command
    paths: "**/*.sh"
    command: ["shellcheck", "{path}"]
    level: error
"#,
        )
        .unwrap();
        let cfg = load(&path).unwrap();
        assert_eq!(cfg.rules.len(), 1);
        assert_eq!(cfg.rules[0].id, "shellcheck");
    }

    #[test]
    fn load_rejects_remote_extends_with_nested_extends() {
        use sha2::{Digest, Sha256};

        let remote_body = b"version: 1\nextends: [./chained.yml]\nrules: []\n";
        let mut hasher = Sha256::new();
        hasher.update(remote_body);
        let digest = hasher.finalize();
        let mut hex = String::with_capacity(digest.len() * 2);
        for b in &digest {
            use std::fmt::Write as _;
            write!(hex, "{b:02x}").unwrap();
        }
        let sri_str = format!("sha256-{hex}");

        let tmp = tempfile::tempdir().unwrap();
        let cache = extends::Cache::at(tmp.path().join("cache"));
        let sri = extends::Sri::parse(&sri_str).unwrap();
        cache.put(&sri, remote_body).unwrap();

        let url = format!("https://example.invalid/base.yml#{sri_str}");
        let config_path = tmp.path().join(".alint.yml");
        std::fs::write(
            &config_path,
            format!("version: 1\nextends: [\"{url}\"]\nrules: []\n"),
        )
        .unwrap();

        let opts = LoadOptions::with_cache(cache);
        let err = load_with(&config_path, &opts).unwrap_err().to_string();
        assert!(err.contains("nested remote extends"), "{err}");
    }

    #[test]
    fn load_merges_facts_with_id_dedup() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base.yml");
        let child = tmp.path().join(".alint.yml");
        std::fs::write(
            &base,
            r"version: 1
facts:
  - id: is_rust
    any_file_exists: [Cargo.toml]
  - id: only_base
    any_file_exists: [B]
rules: []
",
        )
        .unwrap();
        std::fs::write(
            &child,
            r"version: 1
extends: [./base.yml]
facts:
  - id: is_rust
    any_file_exists: [Cargo.toml, rust-toolchain.toml]
  - id: only_child
    any_file_exists: [C]
rules: []
",
        )
        .unwrap();
        let cfg = load(&child).unwrap();
        let ids: Vec<&str> = cfg.facts.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, vec!["is_rust", "only_base", "only_child"]);
    }

    #[test]
    fn load_resolves_transitive_extends() {
        // a.yml extends b.yml extends c.yml; check that every level's
        // rules flow through, and overrides happen at the leaf.
        let tmp = tempfile::tempdir().unwrap();
        let c = tmp.path().join("c.yml");
        let b = tmp.path().join("b.yml");
        let a = tmp.path().join("a.yml");
        std::fs::write(
            &c,
            r"version: 1
rules:
  - id: from-c
    kind: file_exists
    paths: C
    level: warning
",
        )
        .unwrap();
        std::fs::write(
            &b,
            r"version: 1
extends: [./c.yml]
rules:
  - id: from-b
    kind: file_exists
    paths: B
    level: warning
",
        )
        .unwrap();
        std::fs::write(
            &a,
            r"version: 1
extends: [./b.yml]
rules:
  - id: from-a
    kind: file_exists
    paths: A
    level: warning
",
        )
        .unwrap();
        let cfg = load(&a).unwrap();
        let ids: Vec<&str> = cfg.rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["from-c", "from-b", "from-a"]);
    }

    #[test]
    fn in_crate_schema_matches_root() {
        // Guard against drift between the in-crate copy (embedded by
        // `include_str!`) and the root `schemas/v1/config.json` that the
        // public URL serves.
        //
        // The crate-tarball context (`cargo publish` strips the root
        // schemas/ tree) skips the assertion — but only when we can
        // POSITIVELY identify that we are running from a tarball, not
        // silently every time the file fails to read. Workspace context
        // is detected by a co-located workspace `Cargo.lock`; absence
        // of that lock means we are unpacked outside the workspace and
        // the test correctly bows out. Presence + a missing root schema
        // is a real failure (someone deleted the canonical copy) and is
        // now flagged, not papered over.
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let workspace_lock = manifest_dir.join("../../Cargo.lock");
        if !workspace_lock.is_file() {
            return; // crate-tarball context — workspace Cargo.lock absent.
        }
        let root = manifest_dir.join("../../schemas/v1/config.json");
        let canonical = std::fs::read_to_string(&root).unwrap_or_else(|e| {
            panic!(
                "workspace context detected (../../Cargo.lock exists) but the \
                 canonical schema at {} is unreadable: {e}",
                root.display()
            )
        });
        assert_eq!(
            canonical, CONFIG_SCHEMA_V1,
            "crates/alint-dsl/schemas/v1/config.json has drifted from \
             schemas/v1/config.json — run `cp schemas/v1/config.json \
             crates/alint-dsl/schemas/v1/config.json` to resync",
        );
    }

    #[test]
    fn rejects_duplicate_ids() {
        let yaml = r"
version: 1
rules:
  - id: dupe
    kind: file_exists
    level: error
    paths: A
  - id: dupe
    kind: file_exists
    level: error
    paths: B
";
        assert!(parse(yaml).is_err());
    }

    // -----------------------------------------------------------
    // Nested `.alint.yml` discovery
    // -----------------------------------------------------------

    #[test]
    fn nested_discovery_scopes_rules_to_subtree() {
        let tmp = tempfile::tempdir().unwrap();
        let root_cfg = tmp.path().join(".alint.yml");
        std::fs::write(
            &root_cfg,
            r"version: 1
nested_configs: true
rules: []
",
        )
        .unwrap();

        // Nested config at packages/foo
        let pkg_dir = tmp.path().join("packages/foo");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        let nested_cfg = pkg_dir.join(".alint.yml");
        std::fs::write(
            &nested_cfg,
            r#"version: 1
rules:
  - id: foo-readme
    kind: file_exists
    paths: "README.md"
    level: error
"#,
        )
        .unwrap();

        let cfg = load(&root_cfg).unwrap();
        assert_eq!(cfg.rules.len(), 1);
        let rule = &cfg.rules[0];
        assert_eq!(rule.id, "foo-readme");
        // The path should now be prefixed with the nested dir.
        // PathsSpec doesn't implement Serialize, so Debug is
        // the readable path to its contents in a test.
        let paths_dbg = format!("{:?}", rule.paths);
        assert!(
            paths_dbg.contains("packages/foo/README.md"),
            "expected scoped path, got: {paths_dbg}"
        );
    }

    #[test]
    fn nested_baseline_is_rejected() {
        // A nested config may not declare `baseline:` — it's a trusted,
        // root-only input (a subtree must not pick what the gate suppresses).
        let tmp = tempfile::tempdir().unwrap();
        let root_cfg = tmp.path().join(".alint.yml");
        std::fs::write(&root_cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
        let pkg_dir = tmp.path().join("packages/foo");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join(".alint.yml"),
            "version: 1\nbaseline: sneaky.json\nrules: []\n",
        )
        .unwrap();
        let err = load(&root_cfg).unwrap_err();
        assert!(err.to_string().contains("baseline"), "{err}");
    }

    #[test]
    fn nested_allow_out_of_root_is_rejected() {
        // A nested config may not declare `allow_out_of_root:` — the
        // out-of-root escape hatch is a trusted, root-only grant (a subtree
        // must not grant itself reads outside the repo root). Parallels
        // `nested_baseline_is_rejected`; both close the silent-drop gap where
        // the key parsed into the config but was ignored without feedback,
        // unlike every other root-only key.
        let tmp = tempfile::tempdir().unwrap();
        let root_cfg = tmp.path().join(".alint.yml");
        std::fs::write(&root_cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
        let pkg_dir = tmp.path().join("packages/foo");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join(".alint.yml"),
            "version: 1\nallow_out_of_root: true\nrules: []\n",
        )
        .unwrap();
        let err = load(&root_cfg).unwrap_err();
        assert!(err.to_string().contains("allow_out_of_root"), "{err}");
    }

    #[test]
    fn nested_command_rule_is_rejected() {
        // C2 (RCE bypass): a nested `.alint.yml` is untrusted like an
        // `extends:`'d ruleset (anyone who can open a monorepo PR can add
        // one), so it may not declare a process-spawning rule. Without this
        // gate a subtree config running `kind: command` achieved arbitrary
        // code execution on `alint check`. Parallels the `extends:` gate and
        // the root-only `nested_baseline`/`nested_allow_out_of_root` checks.
        let tmp = tempfile::tempdir().unwrap();
        let root_cfg = tmp.path().join(".alint.yml");
        std::fs::write(&root_cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
        let pkg_dir = tmp.path().join("packages/foo");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join(".alint.yml"),
            "version: 1\nrules:\n  - id: sneaky\n    kind: command\n    command: [\"sh\", \"-c\", \"echo pwn\"]\n    paths: \"**/*\"\n    level: error\n",
        )
        .unwrap();
        let err = load(&root_cfg).unwrap_err().to_string();
        assert!(err.contains("command"), "{err}");
        assert!(err.contains("arbitrary code"), "{err}");
    }

    #[test]
    fn nested_templates_are_rejected() {
        // A nested config may not declare `templates:` — they're root-only
        // (a nested template would be silently dropped), and refusing them
        // closes the nested variant of the spawning-template smuggle.
        let tmp = tempfile::tempdir().unwrap();
        let root_cfg = tmp.path().join(".alint.yml");
        std::fs::write(&root_cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
        let pkg_dir = tmp.path().join("packages/foo");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join(".alint.yml"),
            "version: 1\ntemplates:\n  - id: t\n    kind: file_exists\n    paths: \"README.md\"\n    level: error\nrules: []\n",
        )
        .unwrap();
        let err = load(&root_cfg).unwrap_err().to_string();
        assert!(err.contains("templates"), "{err}");
    }

    #[test]
    fn load_rejects_spawning_kind_nested_in_a_require_block() {
        // Third spawn vector (found in adversarial review): `for_each_dir` /
        // `for_each_file` / `every_matching_has` carry a `require:` block of
        // nested rules whose `kind` flattens into the parent's options. An
        // extends:'d ruleset could hide a `command` there — the top-level
        // `kind` check (and a post-finalize scan) miss it, so the gate must
        // recurse into `require:`.
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base.yml");
        let child = tmp.path().join(".alint.yml");
        std::fs::write(
            &base,
            "version: 1\nrules:\n  - id: pwn\n    kind: for_each_dir\n    select: \"**/\"\n    require:\n      - kind: command\n        command: [\"sh\", \"-c\", \"echo pwn\"]\n        level: error\n    level: error\n",
        )
        .unwrap();
        std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();
        let err = load(&child).unwrap_err().to_string();
        assert!(err.contains("command"), "kind not named: {err}");
        assert!(err.contains("arbitrary code"), "{err}");
    }

    #[test]
    fn nested_config_rejects_spawning_kind_in_a_require_block() {
        // The same `require:` vector via a nested `.alint.yml` (under
        // nested_configs). The spawn gate runs before scoping, so it catches
        // the buried `command`.
        let tmp = tempfile::tempdir().unwrap();
        let root_cfg = tmp.path().join(".alint.yml");
        std::fs::write(&root_cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
        let pkg_dir = tmp.path().join("packages/foo");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join(".alint.yml"),
            "version: 1\nrules:\n  - id: pwn\n    kind: for_each_dir\n    select: \"**/\"\n    require:\n      - kind: command\n        command: [\"sh\", \"-c\", \"echo pwn\"]\n        level: error\n    level: error\n",
        )
        .unwrap();
        let err = load(&root_cfg).unwrap_err().to_string();
        assert!(err.contains("command"), "{err}");
        assert!(err.contains("arbitrary code"), "{err}");
    }

    #[test]
    fn nested_discovery_ignored_when_flag_is_false() {
        let tmp = tempfile::tempdir().unwrap();
        let root_cfg = tmp.path().join(".alint.yml");
        std::fs::write(
            &root_cfg,
            // No nested_configs field → defaults to false.
            r"version: 1
rules: []
",
        )
        .unwrap();
        let pkg_dir = tmp.path().join("packages/foo");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join(".alint.yml"),
            r#"version: 1
rules:
  - id: foo-readme
    kind: file_exists
    paths: "README.md"
    level: error
"#,
        )
        .unwrap();

        let cfg = load(&root_cfg).unwrap();
        assert!(
            cfg.rules.is_empty(),
            "nested rule leaked in without the opt-in: {cfg:?}"
        );
    }

    #[test]
    fn nested_id_collision_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let root_cfg = tmp.path().join(".alint.yml");
        std::fs::write(
            &root_cfg,
            r#"version: 1
nested_configs: true
rules:
  - id: collision
    kind: file_exists
    paths: "root.md"
    level: error
"#,
        )
        .unwrap();
        let pkg_dir = tmp.path().join("packages/foo");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join(".alint.yml"),
            r#"version: 1
rules:
  - id: collision
    kind: file_exists
    paths: "other.md"
    level: warning
"#,
        )
        .unwrap();

        let err = load(&root_cfg).unwrap_err().to_string();
        assert!(
            err.contains("collision"),
            "error should name the rule: {err}"
        );
        assert!(
            err.contains("redefines") || err.contains("nested"),
            "error should explain what happened: {err}"
        );
    }

    #[test]
    fn nested_rule_without_scope_field_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let root_cfg = tmp.path().join(".alint.yml");
        std::fs::write(
            &root_cfg,
            r"version: 1
nested_configs: true
rules: []
",
        )
        .unwrap();
        let pkg_dir = tmp.path().join("packages/foo");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join(".alint.yml"),
            // no_submodules has no path field — can't be scoped.
            r"version: 1
rules:
  - id: no-subs
    kind: no_submodules
    level: error
",
        )
        .unwrap();

        let err = load(&root_cfg).unwrap_err().to_string();
        assert!(
            err.contains("no path-like scope"),
            "error should explain the missing scope field: {err}"
        );
    }

    #[test]
    fn nested_absolute_path_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let root_cfg = tmp.path().join(".alint.yml");
        std::fs::write(
            &root_cfg,
            r"version: 1
nested_configs: true
rules: []
",
        )
        .unwrap();
        let pkg_dir = tmp.path().join("packages/foo");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join(".alint.yml"),
            // Absolute path would escape the subtree.
            r#"version: 1
rules:
  - id: absolute
    kind: file_exists
    paths: "/etc/foo"
    level: error
"#,
        )
        .unwrap();

        let err = load(&root_cfg).unwrap_err().to_string();
        assert!(
            err.contains("absolute") && err.contains("escape"),
            "error should explain path constraint: {err}"
        );
    }

    #[test]
    fn nested_path_negation_is_preserved() {
        // Verifies the scope helper correctly re-prefixes `!pattern`
        // so negated globs still sit inside the nested subtree.
        assert_eq!(
            nested::scope_glob("!src/**/*.test.ts", "packages/foo").unwrap(),
            "!packages/foo/src/**/*.test.ts"
        );
    }

    #[test]
    fn discover_finds_config_in_starting_directory() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".alint.yml"), "version: 1\nrules: []\n").unwrap();
        let found = discover(tmp.path()).expect("config should be found");
        assert_eq!(found.file_name().unwrap(), ".alint.yml");
    }

    #[test]
    fn discover_walks_up_to_find_ancestor_config() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".alint.yml"), "version: 1\nrules: []\n").unwrap();
        let nested = tmp.path().join("a/b/c");
        std::fs::create_dir_all(&nested).unwrap();
        let found = discover(&nested).expect("ancestor config should be found");
        assert_eq!(found, tmp.path().join(".alint.yml"));
    }

    #[test]
    fn discover_returns_none_when_no_config_exists() {
        let tmp = tempfile::tempdir().unwrap();
        // Empty tempdir, no parents have config either.
        let found = discover(tmp.path());
        // The tempdir's parent might have an alint.yml in some
        // CI environments; the strict assertion is that discover
        // either returns Some(path inside or above tempdir's
        // parent chain) or None.
        if let Some(p) = &found {
            assert!(!p.starts_with(tmp.path()), "tempdir has no config: {p:?}");
        }
    }

    #[test]
    fn discover_prefers_nearest_config_over_ancestor() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(".alint.yml"),
            "version: 1\nrules: [{id: outer, kind: file_exists, paths: a, level: error}]\n",
        )
        .unwrap();
        let inner = tmp.path().join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(
            inner.join(".alint.yml"),
            "version: 1\nrules: [{id: inner, kind: file_exists, paths: b, level: error}]\n",
        )
        .unwrap();
        let found = discover(&inner).expect("inner config wins");
        assert_eq!(found, inner.join(".alint.yml"));
    }

    #[test]
    fn discover_recognises_alternate_config_names() {
        // The loader accepts `.alint.yml`, `.alint.yaml`,
        // `alint.yml`, `alint.yaml` — `discover` mirrors that list.
        for name in [".alint.yaml", "alint.yml", "alint.yaml"] {
            let tmp = tempfile::tempdir().unwrap();
            std::fs::write(tmp.path().join(name), "version: 1\nrules: []\n").unwrap();
            let found = discover(tmp.path()).expect("config should be found");
            assert_eq!(
                found.file_name().unwrap().to_str().unwrap(),
                name,
                "expected discover to find {name}",
            );
        }
    }

    #[test]
    fn extends_diamond_inheritance_resolves_without_duplicate_rules() {
        // Diamond shape: root extends B + C, both extend D.
        // D's rule should appear once, not twice.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("d.yml"),
            "version: 1\nrules: [{id: from-d, kind: file_exists, paths: D, level: error}]\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("b.yml"),
            "version: 1\nextends: [\"./d.yml\"]\nrules: []\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("c.yml"),
            "version: 1\nextends: [\"./d.yml\"]\nrules: []\n",
        )
        .unwrap();
        let root = tmp.path().join(".alint.yml");
        std::fs::write(
            &root,
            "version: 1\nextends: [\"./b.yml\", \"./c.yml\"]\nrules: []\n",
        )
        .unwrap();
        let cfg = load(&root).unwrap();
        let from_d_count = cfg.rules.iter().filter(|r| r.id == "from-d").count();
        assert_eq!(
            from_d_count, 1,
            "diamond inheritance should yield one `from-d` rule, got {from_d_count}",
        );
    }
}
