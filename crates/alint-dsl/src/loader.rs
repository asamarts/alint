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

/// Mutable state shared by one `load_with` call's whole extends resolution.
#[derive(Default)]
pub(crate) struct LoadState {
    /// Ancestors on the current DFS path (cycle detection + depth bound).
    pub(crate) visiting: std::collections::HashSet<PathBuf>,
    /// Completed non-top-level local loads, reused across a diamond chain: two
    /// entries that both extend the same file at every level would otherwise
    /// reload it once per path through the DAG (2^depth loads, an effective
    /// hang from a hostile repo). `is_top` loads read their own trust and
    /// confinement settings and are never memoized.
    memo: std::collections::HashMap<MemoKey, RawConfig>,
}

#[derive(PartialEq, Eq, Hash)]
struct MemoKey {
    path: PathBuf,
    confine: Option<PathBuf>,
    trusted: Vec<String>,
}

impl LoadState {
    fn cached(&self, key: Option<&MemoKey>) -> Option<RawConfig> {
        key.and_then(|k| self.memo.get(k)).cloned()
    }
}

impl MemoKey {
    fn new(path: &Path, confine: Option<&Path>, trusted: &[String]) -> Self {
        Self {
            path: path.to_path_buf(),
            confine: confine.map(Path::to_path_buf),
            trusted: trusted.to_vec(),
        }
    }
}

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
    // Name the file in a YAML/typed error: in an `extends:` chain or a
    // `.alint.d/` drop-in, a bare "at line 2 column 1" reads as if the
    // top-level config were at fault.
    parse_config_interpolated_inner(contents, source).map_err(|e| match e {
        Error::Yaml(e) => Error::Other(format!("{}: YAML parse error: {e}", display_path(source))),
        other => other,
    })
}

/// `path` for a user-facing message, without the `\\?\` verbatim prefix that
/// `canonicalize` adds on Windows (`\\?\UNC\server\share` becomes
/// `\\server\share`). The prefix never occurs on Unix, so this is a no-op there.
fn display_path(path: &Path) -> String {
    let shown = path.display().to_string();
    if let Some(unc) = shown.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else if let Some(local) = shown.strip_prefix(r"\\?\") {
        local.to_owned()
    } else {
        shown
    }
}

fn parse_config_interpolated_inner(contents: &str, source: &Path) -> Result<RawConfig> {
    // Reject a deeply-nested-flow config before `serde_yaml_ng` (libyaml) chews
    // on it super-linearly — a DoS reachable through an `extends:`'d ruleset.
    if !alint_core::yaml_depth::flow_depth_within_limit(contents) {
        return Err(Error::Other(format!(
            "{}: YAML flow nesting exceeds the maximum supported depth ({})",
            source.display(),
            alint_core::yaml_depth::MAX_YAML_FLOW_DEPTH
        )));
    }
    // Reject an alias-expansion bomb (a single anchor referenced many times) the
    // same way -- `serde_yaml_ng`'s own limits don't catch it.
    if !alint_core::yaml_depth::expansion_within_limit(contents) {
        return Err(Error::Other(format!(
            "{}: YAML alias expansion exceeds the maximum supported size",
            source.display(),
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
    state: &mut LoadState,
    opts: &LoadOptions,
    confine: Option<&Path>,
    is_top: bool,
    // The top-level `trusted_extends:` allowlist (remote URLs whose content fixers
    // stay auto-applying), threaded down the LOCAL nesting so a remote extended at
    // any depth is checked against the user's list. A non-top-level config may not
    // grant trust, so nested calls receive the top's list; `is_top` calls read
    // their OWN `trusted_extends:` and ignore this argument. (W2, auto-fix.md 5.5.)
    trusted: &[String],
) -> Result<RawConfig> {
    let canonical = path.canonicalize().map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    // Diamond chains reload a shared file once per DAG path (2^depth); a
    // non-top-level load is a pure function of its `MemoKey`, so reuse it.
    let memo_key = (!is_top).then(|| MemoKey::new(&canonical, confine, trusted));
    if let Some(cached) = state.cached(memo_key.as_ref()) {
        return Ok(cached);
    }
    let visiting = &mut state.visiting;
    if !visiting.insert(canonical.clone()) {
        return Err(Error::Other(format!(
            "cycle in `extends` chain at {}",
            display_path(&canonical)
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
            display_path(&canonical),
        )));
    }

    let contents = fs::read_to_string(&canonical).map_err(|source| Error::Io {
        path: canonical.clone(),
        source,
    })?;
    let mut config = parse_config_interpolated(&contents, &canonical)?;

    // W2 (auto-fix.md 5.5): `trusted_extends:` is a top-level authority. An is_top
    // config (the user's own `.alint.yml` or a `.alint.d/` drop-in) owns the
    // allowlist and uses it for its own extends chain; a non-top-level config's
    // list is rejected per-source below, so nested loads use the list threaded from
    // the top. Taken (consumed) at is_top -- it gates the demotion here and is not
    // carried onto `Config`.
    let top_trusted: Vec<String>;
    let trusted: &[String] = if is_top {
        top_trusted = std::mem::take(&mut config.trusted_extends);
        &top_trusted
    } else {
        trusted
    };

    let extends = std::mem::take(&mut config.extends);
    if extends.is_empty() {
        visiting.remove(&canonical);
        if let Some(key) = memo_key {
            state.memo.insert(key, config.clone());
        }
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
            let remote = load_remote(url, opts, &mut state.visiting)?;
            crate::reject_env_expansion_in(&remote.rules, &remote.templates, url)?;
            remote
        } else if let Some(spec) = url.strip_prefix("alint://bundled/") {
            load_bundled(spec)?
        } else {
            let target = resolve_relative(&source_dir, url);
            confine_extends_target(&target, url, confine)?;
            load_recursive(&target, state, opts, confine, false, trusted)?
        };
        gate_extended_source(&parent, url)?;
        parent.drop_top_level_settings(url);
        parent.rules = apply_rule_filter(parent.rules, entry)?;
        // W2 content-fixer trust (auto-fix.md 5.5): a REMOTE `extends:` the user has
        // NOT listed in `trusted_extends:` may PROPOSE a content edit but never
        // auto-write one -- demote its content-injecting fixers to `suggestion`
        // before the merge. Local / nested targets (the user's own tree) and bundled
        // (first-party) sources are honored at their declared tier. A remote is a
        // leaf (no nested `extends:`), so this caps exactly that source's own rules;
        // the URL matches with or without its `#sha256-` integrity fragment.
        //
        // BOTH `rules` AND `templates` are demoted: a template's `fix:` block is
        // spliced into its referencing rule at `finalize` (after this per-source
        // gate), so a remote content fixer smuggled through a `templates:` entry
        // would otherwise escape the cap (the template analogue of
        // `reject_fix_promotion_templates_in`).
        //
        // The raw mappings also receive a monotonic provenance marker. It survives
        // id-based field merges and template expansion, then `finalize` demotes the
        // EFFECTIVE fixer. That closes both mixed-source directions: an untrusted
        // rule instantiating a trusted template, and a trusted rule instantiating a
        // template partly defined by an untrusted source.
        if url.starts_with("https://") {
            let base = url.split('#').next().unwrap_or(url);
            let trusted_remote = trusted.iter().any(|t| t == base || t == url);
            if !trusted_remote {
                crate::demote_content_fixers_in(&mut parent.rules);
                crate::demote_content_fixers_in(&mut parent.templates);
                crate::mark_untrusted_fix_sources_in(&mut parent.rules);
                crate::mark_untrusted_fix_sources_in(&mut parent.templates);
            }
        }
        merged = merge(merged, parent);
    }
    merged = merge(merged, config);
    state.visiting.remove(&canonical);
    if let Some(key) = memo_key {
        state.memo.insert(key, merged.clone());
    }
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
    if !alint_core::yaml_depth::expansion_within_limit(body_str) {
        return Err(Error::Other(format!(
            "remote config at {url}: YAML alias expansion exceeds the maximum supported size"
        )));
    }
    let config: RawConfig = serde_yaml_ng::from_str(body_str)
        .map_err(|e| Error::Other(format!("remote config at {url}: YAML parse error: {e}")))?;
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

/// Every per-source trust refusal for one `extends:`'d config. Runs before the
/// config merges, so `url` names the offending source in each error.
fn gate_extended_source(parent: &RawConfig, url: &str) -> Result<()> {
    // Extended configs cannot introduce `custom:` facts or
    // `kind: command` rules — both spawn arbitrary processes
    // on behalf of a ruleset whose code the user didn't
    // write. Same trust model on both sides.
    alint_core::facts::reject_custom_facts_in(&parent.facts, url)?;
    reject_command_rules_in(&parent.rules, url)?;
    // A *spawning* fix op (`git_untrack`) is the RCE analogue of a spawning
    // rule kind: refuse it from any extended source, at every `require:` depth
    // (auto-fix.md 5.5). The kind gate above misses it because a spawning
    // FIXER can hang off a non-spawning kind (`git_untrack` on `file_absent`).
    crate::reject_spawning_fix_ops_in(&parent.rules, url)?;
    crate::reject_fix_promotion_in(&parent.rules, url)?;
    reject_spawning_templates_in(&parent.templates, url)?;
    // ...and the template analogue: a spawning fix in a `templates:` block
    // would splice into its referencing rule at finalize, past the gate above.
    crate::reject_spawning_fix_op_templates_in(&parent.templates, url)?;
    // ...and the same promotion refusal for a `templates:` block, which a
    // template instance would otherwise smuggle a `fix.<op>.applicability:
    // safe` past the rule-level gate above (it expands at finalize time).
    crate::reject_fix_promotion_templates_in(&parent.templates, url)?;
    reject_allow_out_of_root_in(&parent.allow_out_of_root, url)?;
    reject_baseline_in(&parent.baseline, url)?;
    // A ruleset may not allowlist ITSELF into auto-applying content fixers;
    // only the user's top-level config grants that via `trusted_extends:`.
    crate::reject_trusted_extends_in(&parent.trusted_extends, url)?;
    Ok(())
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
    // these guards are defense-in-depth, not an attack surface -- but keeping the
    // checks uniform with the remote/local paths means no `serde_yaml_ng::from_str`
    // in the loader is ever unguarded. A bundled ruleset tripping one is an alint bug.
    if !alint_core::yaml_depth::flow_depth_within_limit(body)
        || !alint_core::yaml_depth::expansion_within_limit(body)
    {
        return Err(Error::internal(format!(
            "built-in ruleset '{spec}' exceeds a maximum supported YAML complexity limit"
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
    fn display_path_strips_the_windows_verbatim_prefix() {
        // `canonicalize` on Windows yields `\\?\C:\...`; error messages must
        // show the path the way the user would write it.
        assert_eq!(
            display_path(Path::new(r"\\?\C:\repo\.alint.yml")),
            r"C:\repo\.alint.yml"
        );
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\server\share\.alint.yml")),
            r"\\server\share\.alint.yml"
        );
        assert_eq!(
            display_path(Path::new("/repo/.alint.yml")),
            "/repo/.alint.yml"
        );
    }

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

    #[test]
    fn diamond_extends_chain_loads_each_file_once() {
        // Audit 2026-10: `cN` extends `[./cN+1.yml, ./cN+1.yml]` at every level,
        // so without memoization the bottom file loads 2^depth times (depth 18
        // measured at 18s; depth 20 is minutes). Each level also contributes a
        // rule so the composed result is checked, not just the runtime.
        let tmp = tempfile::tempdir().unwrap();
        let depth = 20;
        for i in 0..=depth {
            let extends = if i < depth {
                format!("extends: [./c{n}.yml, ./c{n}.yml]\n", n = i + 1)
            } else {
                String::new()
            };
            let body = format!(
                "version: 1\n{extends}rules:\n  - id: r{i}\n    kind: file_exists\n    \
                 paths: README.md\n    level: warning\n"
            );
            std::fs::write(tmp.path().join(format!("c{i}.yml")), body).unwrap();
        }
        let started = std::time::Instant::now();
        let cfg = crate::load(&tmp.path().join("c0.yml")).unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "diamond chain took {:?}",
            started.elapsed()
        );
        assert_eq!(cfg.rules.len(), depth + 1);
    }
}
