//! YAML front-end for alint. Reads a `.alint.yml` and returns a
//! [`alint_core::Config`] that the engine can instantiate.

use std::fs;
use std::path::{Path, PathBuf};

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
    let mut visiting = std::collections::HashSet::new();
    let merged = load_recursive(path, &mut visiting)?;
    validate(&merged)?;
    Ok(merged)
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
        if entry.starts_with("http://") || entry.starts_with("https://") {
            return Err(Error::Other(format!(
                "remote `extends` URLs are not supported in this build \
                 (entry {entry:?}); only local paths work. HTTPS + SRI \
                 support is planned — see the ROADMAP."
            )));
        }
        let target = resolve_relative(&source_dir, entry);
        let parent = load_recursive(&target, visiting)?;
        merged = merge(merged, parent);
    }
    merged = merge(merged, config);
    visiting.remove(&canonical);
    Ok(merged)
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
    fn load_rejects_remote_extends() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".alint.yml");
        std::fs::write(
            &path,
            "version: 1\nextends: [\"https://example.com/base.yml\"]\nrules: []\n",
        )
        .unwrap();
        let err = load(&path).unwrap_err().to_string();
        assert!(err.contains("remote"), "{err}");
        assert!(err.contains("https://example.com"), "{err}");
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
