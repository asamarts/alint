//! Nested `.alint.yml` discovery — find sub-directory configs
//! under a root config's tree and lift their rules into the root's
//! rule list, scoped to the sub-directory they live in.
//!
//! Triggered from `lib::load_with` when the root config sets
//! `nested_configs: true`. Each sub-config contributes rules
//! whose path-like scope fields (`paths`, `select`, `primary`)
//! get prefixed with the sub-config's directory so the rule's
//! effective scope can't escape its subtree.

use std::fs;
use std::path::Path;

use alint_core::{Error, Result};
use serde_yaml_ng::Mapping;

use crate::{DEFAULT_CONFIG_NAMES, RawConfig};

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
pub(crate) fn discover_nested(
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
    // Route through the shared parse path so nested configs get the
    // same `{{env.X}}` interpolation as the top-level config and
    // drop-ins. A YAML/typed error keeps the "parsing nested config"
    // context; an interpolation error already carries the path.
    let config: RawConfig = match crate::loader::parse_config_interpolated(&contents, abs_path) {
        Ok(c) => c,
        Err(Error::Yaml(e)) => {
            return Err(Error::Other(format!(
                "parsing nested config {}: {e}",
                abs_path.display()
            )));
        }
        Err(other) => return Err(other),
    };

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
    if config.baseline.is_some() {
        return Err(Error::Other(format!(
            "nested config {source} declares `baseline:` — the baseline is a \
             trusted, root-only input; declare it in the top-level config"
        )));
    }

    // Glob patterns are platform-agnostic (always `/`); on
    // Windows `rel_dir.to_string_lossy()` would emit `\` and we'd
    // end up with mixed-separator globs like `packages\foo/foo.txt`
    // that don't compile against globset cleanly.
    let dir_prefix = rel_dir.to_string_lossy().replace('\\', "/");
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
pub(crate) fn scope_glob(glob: &str, prefix: &str) -> Result<String> {
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
