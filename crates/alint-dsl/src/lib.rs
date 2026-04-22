//! YAML front-end for alint. Reads a `.alint.yml` and returns a
//! [`alint_core::Config`] that the engine can instantiate.

use std::fs;
use std::path::{Path, PathBuf};

pub mod bundled;
pub mod extends;

use alint_core::{Config, Error, FactSpec, Result, RuleSpec};

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
    let merged = load_recursive(path, &mut visiting, opts)?;
    validate(&merged)?;
    Ok(merged)
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
/// it extends.
fn load_recursive(
    path: &Path,
    visiting: &mut std::collections::HashSet<PathBuf>,
    opts: &LoadOptions,
) -> Result<Config> {
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
    let mut config: Config = serde_yaml_ng::from_str(&contents)?;

    let extends = std::mem::take(&mut config.extends);
    if extends.is_empty() {
        visiting.remove(&canonical);
        return Ok(config);
    }

    let source_dir = canonical
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);

    let mut merged = Config {
        version: config.version,
        ..Config::default()
    };
    for entry in &extends {
        let parent = if entry.starts_with("http://") {
            return Err(Error::Other(format!(
                "plain http:// is not allowed in `extends:` (entry {entry:?}); \
                 use https:// with an SRI hash instead"
            )));
        } else if entry.starts_with("https://") {
            load_remote(entry, opts, visiting)?
        } else if let Some(spec) = entry.strip_prefix("alint://bundled/") {
            load_bundled(spec)?
        } else {
            let target = resolve_relative(&source_dir, entry);
            load_recursive(&target, visiting, opts)?
        };
        // Extended configs cannot introduce `custom:` facts —
        // those would spawn arbitrary processes on behalf of a
        // ruleset whose code the user didn't write.
        alint_core::facts::reject_custom_facts(&parent, entry)?;
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
) -> Result<Config> {
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
    let config: Config = serde_yaml_ng::from_str(
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
fn load_bundled(spec: &str) -> Result<Config> {
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

    let config: Config = serde_yaml_ng::from_str(body).map_err(|e| {
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

/// Merge `b` into `a`, with `b` winning on conflicts.
///
/// Semantics:
/// - `rules` and `facts` dedupe by id; `b`'s entry wins for any
///   duplicate. Ordering: `a`'s entries first (in order), then
///   `b`'s entries, minus duplicates.
/// - `vars` merged as a map; `b`'s values override.
/// - `ignore` concatenated `a` then `b`.
/// - `respect_gitignore` takes `b`'s value (it has a default, so
///   there's no way to tell "unset" from "false"; document this).
/// - `version` takes `b`'s value.
/// - `extends` is always left empty on the merged result; resolved
///   already.
fn merge(a: Config, b: Config) -> Config {
    let version = b.version;
    let respect_gitignore = b.respect_gitignore;
    // fix_size_limit: child wins, same as respect_gitignore.
    // Can't distinguish "unset" from the default here either.
    let fix_size_limit = b.fix_size_limit;

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

    let mut rules_by_id: std::collections::BTreeMap<String, RuleSpec> =
        std::collections::BTreeMap::new();
    let mut rule_order: Vec<String> = Vec::new();
    for r in a.rules.into_iter().chain(b.rules) {
        if !rules_by_id.contains_key(&r.id) {
            rule_order.push(r.id.clone());
        }
        rules_by_id.insert(r.id.clone(), r);
    }
    let rules: Vec<RuleSpec> = rule_order
        .into_iter()
        .map(|id| rules_by_id.remove(&id).unwrap())
        .collect();

    Config {
        version,
        extends: Vec::new(),
        ignore,
        respect_gitignore,
        vars,
        facts,
        rules,
        fix_size_limit,
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
}
