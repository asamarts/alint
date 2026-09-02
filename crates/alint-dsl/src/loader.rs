//! `extends:` resolution — recursive loader for the YAML
//! composition chain. Pulled out of `lib.rs` to keep that file
//! focused on the public surface (discover / load / parse) and
//! the typed config shape.

use std::fs;
use std::path::{Path, PathBuf};

use alint_core::{AllowOutOfRoot, Error, Result};

use crate::extends;
use crate::{
    LoadOptions, RawConfig, apply_rule_filter, merge, reject_allow_out_of_root_in,
    reject_baseline_in, reject_command_rules_in, reject_spawning_templates_in,
};

/// Maximum depth of an `extends:` chain — a recursion-stack guard against a
/// hostile deeply-nested (acyclic) chain. Generous: real compositions are a
/// handful deep. See [`load_recursive`] (L5).
const MAX_EXTENDS_DEPTH: usize = 64;

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
    // Reject a deeply-nested-flow config before `serde_yaml_ng` (libyaml) chews
    // on it super-linearly — a DoS reachable through an `extends:`'d ruleset.
    if !alint_core::yaml_depth::flow_depth_within_limit(contents) {
        return Err(Error::Other(format!(
            "{}: YAML flow nesting exceeds the maximum supported depth ({})",
            source.display(),
            alint_core::yaml_depth::MAX_YAML_FLOW_DEPTH
        )));
    }
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
    confine: Option<&Path>,
    is_top: bool,
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
    // Bound the depth of an *acyclic* chain (the cycle guard above only catches
    // repeats): a hostile repo could otherwise nest thousands of local configs
    // each extending the next and overflow the recursion stack (L5). `visiting`
    // holds exactly the ancestors on the current DFS path (balanced insert /
    // remove), so its length is the current depth. The cap is far above any
    // real composition (root → team → org → bundled is ~4).
    if visiting.len() > MAX_EXTENDS_DEPTH {
        return Err(Error::Other(format!(
            "`extends:` chain exceeds the maximum depth of {MAX_EXTENDS_DEPTH} (at {}); \
             flatten the chain or split the ruleset",
            canonical.display(),
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

    // Local `extends:` targets stay within the lint tree (the confinement
    // boundary the loader was handed — the top-level config's directory),
    // so a shared ruleset committed to the repo cannot smuggle in a local
    // `extends: ../../../../etc/shadow` to read arbitrary files off the
    // host. The top-level `allow_out_of_root: true` lifts it for the whole
    // chain — the same blanket escape that lifts per-rule read confinement.
    // A `Selective` allowlist names rule kinds/ids and has no meaning for an
    // extends *path*, so only the `All` form opens this gate. Only the USER'S
    // TOP-LEVEL config may open it: a sub-config's flag is rejected by
    // `reject_allow_out_of_root_in` at the parent's loop below, but that fires
    // AFTER this sub-config has already resolved ITS OWN `extends:` — so gating
    // on `is_top` here is what actually prevents an extended ruleset from
    // lifting confinement and reading an out-of-root `extends:` target (a FIFO
    // hangs, a big file is slurped, host paths become an existence oracle)
    // before the rejection can fire. Without `is_top` the reject is one level
    // too late.
    let confine = match (&config.allow_out_of_root, is_top) {
        (AllowOutOfRoot::All, true) => None,
        _ => confine,
    };

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
            confine_extends_target(&target, url, confine)?;
            load_recursive(&target, visiting, opts, confine, false)?
        };
        // Extended configs cannot introduce `custom:` facts or
        // `kind: command` rules — both spawn arbitrary processes
        // on behalf of a ruleset whose code the user didn't
        // write. Same trust model on both sides.
        alint_core::facts::reject_custom_facts_in(&parent.facts, url)?;
        reject_command_rules_in(&parent.rules, url)?;
        reject_spawning_templates_in(&parent.templates, url)?;
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
    let body_str = std::str::from_utf8(&body)
        .map_err(|e| Error::Other(format!("remote body from {url} is not UTF-8: {e}")))?;
    // A remote body is untrusted input (SRI pins WHICH bytes, not that they are
    // benign), so it needs the same flow-depth guard as a local config -- otherwise
    // `serde_yaml_ng` (libyaml) chews super-linearly on a deep-flow bomb and hangs
    // the run. The local/interpolated path guards in `parse_config_interpolated`;
    // this direct `from_str` bypassed it (the guard's docs claim `extends:` bodies
    // are covered -- true for local, and now for remote).
    if !alint_core::yaml_depth::flow_depth_within_limit(body_str) {
        return Err(Error::Other(format!(
            "remote config at {url}: YAML flow nesting exceeds the maximum supported depth ({})",
            alint_core::yaml_depth::MAX_YAML_FLOW_DEPTH
        )));
    }
    let config: RawConfig = serde_yaml_ng::from_str(body_str)?;
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

    // Bundled rulesets are compiled-in and trusted (byte-identical every build), so
    // this guard is defense-in-depth, not an attack surface -- but keeping the check
    // uniform with the remote/local paths means no `serde_yaml_ng::from_str` in the
    // loader is ever unguarded. A bundled ruleset tripping it is an alint bug.
    if !alint_core::yaml_depth::flow_depth_within_limit(body) {
        return Err(Error::internal(format!(
            "built-in ruleset '{spec}' exceeds the maximum supported YAML flow depth"
        )));
    }
    let config: RawConfig = serde_yaml_ng::from_str(body).map_err(|e| {
        // A ruleset shipped *inside* the binary failing to parse is an alint
        // bug, not the user's config — Internal → CLI exit 3 (M11).
        Error::internal(format!("built-in ruleset '{spec}' failed to parse: {e}"))
    })?;
    if !config.extends.is_empty() {
        return Err(Error::internal(format!(
            "bundled ruleset '{spec}' declares its own `extends:`"
        )));
    }
    Ok(config)
}

/// Reject a local `extends:` target that resolves outside the confinement
/// root. `confine == None` means confinement is disabled — either a
/// programmatic caller that handed the loader no root, or a top-level config
/// that opted out via `allow_out_of_root: true`.
///
/// Both sides are canonicalized, so `..`, `.`, and symlinks are resolved: a
/// symlink that lives inside the tree but points out is caught too. A
/// canonicalize failure (most often a missing target) is deliberately *not*
/// treated as an escape — there is nothing to read, so it is left for
/// [`load_recursive`] to surface with its existing not-found error.
fn confine_extends_target(target: &Path, entry: &str, confine: Option<&Path>) -> Result<()> {
    let Some(root) = confine else { return Ok(()) };
    let (Ok(canon_root), Ok(canon_target)) = (root.canonicalize(), target.canonicalize()) else {
        return Ok(());
    };
    if !canon_target.starts_with(&canon_root) {
        return Err(Error::Other(format!(
            "`extends:` target {entry:?} resolves to {} which is outside the lint root {}; \
             a local `extends:` chain must stay within the linted tree. Set \
             `allow_out_of_root: true` on the top-level config to override.",
            canon_target.display(),
            canon_root.display(),
        )));
    }
    Ok(())
}

fn resolve_relative(source_dir: &Path, entry: &str) -> PathBuf {
    let candidate = Path::new(entry);
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        source_dir.join(candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confine_none_disables_the_check() {
        // A programmatic caller (or `allow_out_of_root: true`) hands `None`:
        // even a blatant escape target is permitted.
        assert!(confine_extends_target(Path::new("/etc/shadow"), "/etc/shadow", None).is_ok());
    }

    #[test]
    fn confine_missing_target_is_not_an_escape() {
        // A non-existent target can't be canonicalized; it is left for the
        // caller's not-found path, NOT reported as out-of-root (nothing to read).
        let tmp = tempfile::tempdir().unwrap();
        let res = confine_extends_target(
            &tmp.path().join("nope.yml"),
            "../nope.yml",
            Some(tmp.path()),
        );
        assert!(
            res.is_ok(),
            "missing target must defer to the not-found path"
        );
    }

    #[test]
    fn confine_rejects_target_outside_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        let outside = tmp.path().join("outside.yml");
        std::fs::write(&outside, "x").unwrap();
        let err = confine_extends_target(&outside, "../outside.yml", Some(&root))
            .unwrap_err()
            .to_string();
        assert!(err.contains("outside the lint root"), "{err}");
    }

    #[test]
    fn confine_allows_target_inside_root() {
        let tmp = tempfile::tempdir().unwrap();
        let inside = tmp.path().join("base.yml");
        std::fs::write(&inside, "x").unwrap();
        assert!(confine_extends_target(&inside, "./base.yml", Some(tmp.path())).is_ok());
    }

    #[test]
    fn extends_chain_depth_is_capped() {
        // L5: an acyclic chain deeper than the cap is rejected (not a stack
        // overflow). c0 -> c1 -> ... all within one dir (so confinement passes).
        let tmp = tempfile::tempdir().unwrap();
        let n = MAX_EXTENDS_DEPTH + 5;
        for i in 0..n {
            let body = if i + 1 < n {
                format!("version: 1\nextends: [./c{}.yml]\nrules: []\n", i + 1)
            } else {
                "version: 1\nrules: []\n".to_string()
            };
            std::fs::write(tmp.path().join(format!("c{i}.yml")), body).unwrap();
        }
        let err = crate::load(&tmp.path().join("c0.yml"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("maximum depth"), "{err}");
    }

    #[test]
    fn extends_chain_within_depth_cap_loads() {
        // A short chain (well under the cap) still composes fine.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("base.yml"), "version: 1\nrules: []\n").unwrap();
        std::fs::write(
            tmp.path().join(".alint.yml"),
            "version: 1\nextends: [./base.yml]\nrules: []\n",
        )
        .unwrap();
        assert!(crate::load(&tmp.path().join(".alint.yml")).is_ok());
    }
}
