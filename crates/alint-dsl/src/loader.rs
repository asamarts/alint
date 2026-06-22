//! `extends:` resolution — recursive loader for the YAML
//! composition chain. Pulled out of `lib.rs` to keep that file
//! focused on the public surface (discover / load / parse) and
//! the typed config shape.

use std::fs;
use std::path::{Path, PathBuf};

use alint_core::{Error, Result};

use crate::extends;
use crate::{
    LoadOptions, RawConfig, apply_rule_filter, merge, reject_allow_out_of_root_in,
    reject_baseline_in, reject_command_rules_in,
};

/// Parse a local config file's `contents` into a [`RawConfig`],
/// resolving `{{env.X}}` interpolation first. Shared by
/// `load_recursive` and nested-config loading so every local config
/// file (top-level, `.alint.d/` drop-ins, local `extends:` targets,
/// and nested configs) gets identical interpolation treatment —
/// bundled and remote `extends:` content is handled elsewhere and
/// deliberately NOT interpolated against the consumer's environment.
///
/// Gated on the presence of any `{{` marker: a config with no
/// interpolation parses straight into `RawConfig`, which keeps the
/// line/column-aware serde error messages (the `Value` round-trip
/// loses span info) AND skips a redundant second parse. An interp
/// failure is reported with the `source` path; a typed/YAML error
/// propagates bare so the existing diagnostics are unchanged.
pub(crate) fn parse_config_interpolated(contents: &str, source: &Path) -> Result<RawConfig> {
    if contents.contains("{{") {
        let mut value: serde_yaml_ng::Value = serde_yaml_ng::from_str(contents)?;
        crate::interp::interpolate_value(&mut value, &|n| std::env::var(n).ok())
            .map_err(|e| Error::Other(format!("{}: interpolation error: {e}", source.display())))?;
        Ok(serde_yaml_ng::from_value(value)?)
    } else {
        Ok(serde_yaml_ng::from_str(contents)?)
    }
}

/// Recursively load `path`, resolving its `extends:` chain
/// left-to-right. Later entries in the chain override earlier
/// ones; the current file's own definitions override everything
/// it extends. Rules are field-merged at the YAML-Mapping layer
/// so children can override individual fields without re-stating
/// the entire rule.
pub(crate) fn load_recursive(
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
    let mut config = parse_config_interpolated(&contents, &canonical)?;

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
        // Extended configs cannot introduce `custom:` facts or
        // `kind: command` rules — both spawn arbitrary processes
        // on behalf of a ruleset whose code the user didn't
        // write. Same trust model on both sides.
        alint_core::facts::reject_custom_facts_in(&parent.facts, url)?;
        reject_command_rules_in(&parent.rules, url)?;
        reject_allow_out_of_root_in(&parent.allow_out_of_root, url)?;
        reject_baseline_in(&parent.baseline, url)?;
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
    let body = crate::bundled::resolve(spec).ok_or_else(|| {
        let shipped: Vec<String> = crate::bundled::catalog()
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
