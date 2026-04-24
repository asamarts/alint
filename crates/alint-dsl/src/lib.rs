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

const DEFAULT_CONFIG_NAMES: &[&str] = &[".alint.yml", ".alint.yaml", "alint.yml", "alint.yaml"];

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
    let mut raw = load_recursive(path, &mut visiting, opts)?;

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
        let discovered = discover_nested(&root_dir, &canonical_root_cfg, &raw)?;
        raw.rules.extend(discovered);
    }

    let merged = raw.finalize()?;
    validate(&merged)?;
    Ok(merged)
}

/// Intermediate form used during `extends:` resolution. Identical
/// to [`Config`] except that rules are kept as raw
/// `serde_yaml_ng::Mapping`s so overrides can merge per-field
/// instead of per-rule. See the module-level docs for the full
/// composition model.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawConfig {
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
    #[serde(default)]
    rules: Vec<Mapping>,
    #[serde(default = "default_fix_size_limit")]
    fix_size_limit: Option<u64>,
    #[serde(default)]
    nested_configs: bool,
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
    fn finalize(self) -> Result<Config> {
        let mut rules = Vec::with_capacity(self.rules.len());
        for m in self.rules {
            // Extract the id first so a deserialization error can
            // name the offending rule.
            let id_hint = m
                .get("id")
                .and_then(|v| v.as_str())
                .map_or_else(|| "<anonymous>".to_string(), str::to_string);
            let spec: alint_core::RuleSpec =
                serde_yaml_ng::from_value(serde_yaml_ng::Value::Mapping(m)).map_err(|e| {
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
        })
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

/// Recursively load `path`, resolving its `extends:` chain
/// left-to-right. Later entries in the chain override earlier
/// ones; the current file's own definitions override everything
/// it extends. Rules are field-merged at the YAML-Mapping layer
/// so children can override individual fields without re-stating
/// the entire rule.
fn load_recursive(
    path: &Path,
    visiting: &mut std::collections::HashSet<PathBuf>,
    opts: &LoadOptions,
) -> Result<RawConfig> {
    let canonical = path.canonicalize().map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !visiting.insert(canonical.clone()) {
        return Err(Error::Other(format!(
            "cycle in `extends` chain at {}",
            canonical.display()
        )));
    }

    let contents = fs::read_to_string(&canonical).map_err(|source| Error::Io {
        path: canonical.clone(),
        source,
    })?;
    let mut config: RawConfig = serde_yaml_ng::from_str(&contents)?;

    let extends = std::mem::take(&mut config.extends);
    if extends.is_empty() {
        visiting.remove(&canonical);
        return Ok(config);
    }

    let source_dir = canonical
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);

    let mut merged = RawConfig {
        version: config.version,
        ..RawConfig::default()
    };
    for entry in &extends {
        let url = entry.url();
        let mut parent = if url.starts_with("http://") {
            return Err(Error::Other(format!(
                "plain http:// is not allowed in `extends:` (entry {url:?}); \
                 use https:// with an SRI hash instead"
            )));
        } else if url.starts_with("https://") {
            load_remote(url, opts, visiting)?
        } else if let Some(spec) = url.strip_prefix("alint://bundled/") {
            load_bundled(spec)?
        } else {
            let target = resolve_relative(&source_dir, url);
            load_recursive(&target, visiting, opts)?
        };
        // Extended configs cannot introduce `custom:` facts —
        // those would spawn arbitrary processes on behalf of a
        // ruleset whose code the user didn't write.
        alint_core::facts::reject_custom_facts_in(&parent.facts, url)?;
        parent.rules = apply_rule_filter(parent.rules, entry)?;
        merged = merge(merged, parent);
    }
    merged = merge(merged, config);
    visiting.remove(&canonical);
    Ok(merged)
}

fn load_remote(
    entry: &str,
    opts: &LoadOptions,
    visiting: &mut std::collections::HashSet<PathBuf>,
) -> Result<RawConfig> {
    let (url, sri) = extends::split_url_and_sri(entry).map_err(|e| Error::Other(e.to_string()))?;
    let Some(sri) = sri else {
        return Err(Error::Other(format!(
            "remote `extends` entry {entry:?} has no integrity hash; \
             HTTPS extends require `#sha256-<hex>` in the URL fragment"
        )));
    };

    let cache = match opts.cache.clone() {
        Some(c) => c,
        None => extends::Cache::user_default()
            .map_err(|e| Error::Other(format!("could not open cache: {e}")))?,
    };
    let fetcher = opts.fetcher.clone().unwrap_or_default();
    let body = extends::resolve_remote(&url, &sri, &fetcher, &cache)
        .map_err(|e| Error::Other(format!("resolving {url}: {e}")))?;

    // Remote entries may themselves extend other things (local
    // paths relative to… what, exactly?). For v0.2 we forbid
    // nested extends in a remote body to dodge that ambiguity.
    // When we lift this restriction, the base for relative
    // resolution needs a deliberate decision.
    let config: RawConfig = serde_yaml_ng::from_str(
        std::str::from_utf8(&body)
            .map_err(|e| Error::Other(format!("remote body from {url} is not UTF-8: {e}")))?,
    )?;
    if !config.extends.is_empty() {
        return Err(Error::Other(format!(
            "remote config at {url} contains its own `extends:`; \
             nested remote extends are not supported in this build"
        )));
    }
    // Cycle guard token for the URL itself so a self-referencing
    // fetched config can't loop.
    let token = std::path::PathBuf::from(format!("remote://{}", sri.encoded()));
    if !visiting.insert(token.clone()) {
        return Err(Error::Other(format!("cycle on remote extends: {url}")));
    }
    visiting.remove(&token);
    Ok(config)
}

/// Load an `alint://bundled/<name>@<rev>` ruleset from the
/// in-binary registry. Bundled rulesets can't themselves extend
/// anything — they're static, leaf-only fragments.
fn load_bundled(spec: &str) -> Result<RawConfig> {
    let body = bundled::resolve(spec).ok_or_else(|| {
        let shipped: Vec<String> = bundled::catalog()
            .map(|(n, r)| format!("alint://bundled/{n}@{r}"))
            .collect();
        Error::Other(format!(
            "unknown bundled ruleset 'alint://bundled/{spec}'; \
             this build ships: [{}]",
            shipped.join(", "),
        ))
    })?;

    let config: RawConfig = serde_yaml_ng::from_str(body).map_err(|e| {
        Error::Other(format!(
            "built-in ruleset '{spec}' failed to parse: {e}; \
             this is a bug in alint — please file an issue"
        ))
    })?;
    if !config.extends.is_empty() {
        return Err(Error::Other(format!(
            "bundled ruleset '{spec}' declares its own `extends:`; \
             this is a bug in alint"
        )));
    }
    Ok(config)
}

fn resolve_relative(source_dir: &Path, entry: &str) -> PathBuf {
    let candidate = Path::new(entry);
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        source_dir.join(candidate)
    }
}

/// Apply an `extends:` entry's `only:` / `except:` filters to the
/// fully-resolved rule set of the extended config. Validates that
/// the two filters are mutually exclusive, that the filter list is
/// non-empty, and that every listed id actually exists in the
/// ruleset (unknown ids are almost always typos worth catching at
/// load time).
fn apply_rule_filter(
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
fn merge(a: RawConfig, b: RawConfig) -> RawConfig {
    let version = b.version;
    let respect_gitignore = b.respect_gitignore;
    let fix_size_limit = b.fix_size_limit;
    let nested_configs = b.nested_configs;

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
        rules,
        fix_size_limit,
        nested_configs,
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

// ---------------------------------------------------------------
// Nested `.alint.yml` discovery
// ---------------------------------------------------------------

/// The subset of [`RuleSpec`] fields that carry a path-like scope
/// — i.e., the keys whose values get re-rooted when a rule is
/// lifted from a nested config into the root's rule list.
const NESTED_SCOPE_FIELDS: &[&str] = &["paths", "select", "primary"];

/// Walk `root_dir` (respecting the root config's gitignore +
/// ignore settings), locate every `.alint.yml` / `.alint.yaml` /
/// `alint.yml` / `alint.yaml` that is not the root config
/// itself, and return the scoped-and-flattened list of rule
/// mappings contributed by those nested configs.
///
/// Each returned mapping is already scoped to the directory the
/// nested config lives in: `paths`, `select`, `primary` all get
/// prefixed. Rule ids are checked against the root's rules
/// immediately so id collisions surface as clear errors instead
/// of sneaking past as silent overrides.
///
/// MVP restrictions enforced here:
/// - Nested configs cannot declare `extends:`, `facts:`,
///   `vars:`, `ignore:`, `respect_gitignore:`, `fix_size_limit:`,
///   or `nested_configs:`. Only `version:` and `rules:` are
///   honored; other fields trip `deny_unknown_fields` or a
///   dedicated check.
/// - Each nested rule must provide at least one scope field
///   (`paths` / `select` / `primary`) — otherwise there's
///   nothing to re-root and the rule's effective scope can't
///   be confined to its directory.
/// - Absolute paths or paths starting with `..` aren't
///   supported in nested configs (would escape the subtree).
fn discover_nested(
    root_dir: &Path,
    canonical_root_cfg: &Path,
    root: &RawConfig,
) -> Result<Vec<Mapping>> {
    let walk_opts = alint_core::WalkOptions {
        respect_gitignore: root.respect_gitignore,
        extra_ignores: root.ignore.clone(),
    };
    let index = alint_core::walk(root_dir, &walk_opts)?;

    // First pass: collect existing rule ids from the root so we
    // can surface collisions early. (The root's rules are still
    // raw mappings at this point, pre-finalize.)
    let mut seen_ids: std::collections::HashSet<String> = root
        .rules
        .iter()
        .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(str::to_string))
        .collect();

    let mut discovered: Vec<Mapping> = Vec::new();
    for entry in &index.entries {
        if entry.is_dir {
            continue;
        }
        let file_name = entry
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if !DEFAULT_CONFIG_NAMES.contains(&file_name) {
            continue;
        }
        let abs = root_dir.join(&entry.path);
        let canon = abs.canonicalize().map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        if canon == canonical_root_cfg {
            // Root config itself; skip.
            continue;
        }
        let rel_dir = entry
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();

        let nested_rules = load_nested_config(&abs, &rel_dir)?;
        for rule in nested_rules {
            if let Some(id) = rule.get("id").and_then(|v| v.as_str()) {
                if !seen_ids.insert(id.to_string()) {
                    return Err(Error::rule_config(
                        id,
                        format!(
                            "nested config {} redefines rule id {id:?} — \
                             per-subtree overrides aren't supported yet; \
                             pick a unique id or disable the root's rule \
                             and define it per-subtree",
                            abs.display()
                        ),
                    ));
                }
            }
            discovered.push(rule);
        }
    }
    Ok(discovered)
}

/// Load a nested config and return its rule mappings, each
/// scoped to `rel_dir` (the path, relative to the root config's
/// directory, where the nested config lives).
fn load_nested_config(abs_path: &Path, rel_dir: &Path) -> Result<Vec<Mapping>> {
    let contents = fs::read_to_string(abs_path).map_err(|source| Error::Io {
        path: abs_path.to_path_buf(),
        source,
    })?;
    let config: RawConfig = serde_yaml_ng::from_str(&contents)
        .map_err(|e| Error::Other(format!("parsing nested config {}: {e}", abs_path.display())))?;

    // MVP: reject nested configs that try to set anything that
    // could affect the whole repo. Only `version:` and `rules:`
    // are meaningful; everything else is suspicious enough to
    // require an explicit error.
    let source = abs_path.display().to_string();
    if !config.extends.is_empty() {
        return Err(Error::Other(format!(
            "nested config {source} declares `extends:` — nested configs \
             are flat in this release; extend only from the root config"
        )));
    }
    if !config.facts.is_empty() {
        return Err(Error::Other(format!(
            "nested config {source} declares `facts:` — facts are a \
             root-only concept; move the fact to the root config"
        )));
    }
    if !config.vars.is_empty() {
        return Err(Error::Other(format!(
            "nested config {source} declares `vars:` — vars are a \
             root-only concept; move them to the root config"
        )));
    }
    if !config.ignore.is_empty() || config.nested_configs {
        return Err(Error::Other(format!(
            "nested config {source} declares `ignore:` or `nested_configs:` — \
             both are root-only in this release"
        )));
    }

    let dir_prefix = rel_dir.to_string_lossy().into_owned();
    let mut out = Vec::with_capacity(config.rules.len());
    for mut rule in config.rules {
        scope_rule(&mut rule, &dir_prefix, &source)?;
        out.push(rule);
    }
    Ok(out)
}

/// Re-root every path-like scope field of a rule mapping in
/// place. Returns an error if the rule has no scope field (we
/// can't confine it to its subtree).
fn scope_rule(rule: &mut Mapping, prefix: &str, source: &str) -> Result<()> {
    let id_hint = rule
        .get("id")
        .and_then(|v| v.as_str())
        .map_or_else(|| "<anonymous>".to_string(), str::to_string);

    // Reject obvious antipatterns before touching anything.
    if rule
        .get("root_only")
        .and_then(serde_yaml_ng::Value::as_bool)
        == Some(true)
    {
        return Err(Error::rule_config(
            &id_hint,
            format!(
                "rule in nested config {source} uses `root_only: true`, \
                 which doesn't make sense in a subdirectory config"
            ),
        ));
    }

    let mut any_scoped = false;
    for field in NESTED_SCOPE_FIELDS {
        if let Some(value) = rule.get_mut(*field) {
            scope_paths_value(value, prefix).map_err(|e| {
                Error::rule_config(&id_hint, format!("scoping `{field}` in {source}: {e}"))
            })?;
            any_scoped = true;
        }
    }

    if !any_scoped {
        return Err(Error::rule_config(
            &id_hint,
            format!(
                "rule in nested config {source} has no path-like scope \
                 field ({}) — nested configs can only contribute rules \
                 whose scope can be confined to the nested directory",
                NESTED_SCOPE_FIELDS.join(", "),
            ),
        ));
    }
    Ok(())
}

/// Re-root a YAML value representing a paths-spec. Accepts:
/// - a single string (plain glob, possibly `!`-negated)
/// - an array of strings
/// - an `{include, exclude}` mapping (each list gets prefixed)
///
/// Absolute paths and `..`-prefixed globs are rejected; they'd
/// escape the subtree the nested config is supposed to confine.
fn scope_paths_value(value: &mut serde_yaml_ng::Value, prefix: &str) -> Result<()> {
    match value {
        serde_yaml_ng::Value::String(s) => {
            *s = scope_glob(s, prefix)?;
        }
        serde_yaml_ng::Value::Sequence(seq) => {
            for item in seq {
                if let Some(s) = item.as_str() {
                    *item = serde_yaml_ng::Value::String(scope_glob(s, prefix)?);
                } else {
                    return Err(Error::Other(
                        "path array contains a non-string entry; nested scoping only \
                         supports strings"
                            .into(),
                    ));
                }
            }
        }
        serde_yaml_ng::Value::Mapping(m) => {
            // include / exclude form — prefix each list in place.
            for key in &["include", "exclude"] {
                if let Some(inner) = m.get_mut(*key) {
                    scope_paths_value(inner, prefix)?;
                }
            }
        }
        _ => {
            return Err(Error::Other(
                "unrecognized paths shape in nested config (expected string, \
                 array, or include/exclude mapping)"
                    .into(),
            ));
        }
    }
    Ok(())
}

/// Join `prefix` to a single glob. Preserves leading `!` for
/// negations. Rejects absolute paths and `..` escapes.
fn scope_glob(glob: &str, prefix: &str) -> Result<String> {
    if prefix.is_empty() {
        return Ok(glob.to_string());
    }
    let (negate, rest) = match glob.strip_prefix('!') {
        Some(r) => (true, r),
        None => (false, glob),
    };
    if rest.starts_with('/') {
        return Err(Error::Other(format!(
            "absolute path {glob:?} can't be used in a nested config — \
             it would escape the subtree"
        )));
    }
    if rest.starts_with("../") || rest == ".." {
        return Err(Error::Other(format!(
            "parent-directory escape {glob:?} isn't allowed in a nested config"
        )));
    }
    let joined = if negate {
        format!("!{prefix}/{rest}")
    } else {
        format!("{prefix}/{rest}")
    };
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // public URL serves. Only runs inside the workspace checkout — the
        // published crate does not ship the root copy, so the test skips.
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/v1/config.json");
        let Ok(canonical) = std::fs::read_to_string(&root) else {
            return;
        };
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
            scope_glob("!src/**/*.test.ts", "packages/foo").unwrap(),
            "!packages/foo/src/**/*.test.ts"
        );
    }
}
