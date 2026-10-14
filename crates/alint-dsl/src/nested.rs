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

/// Kinds whose `select:` is a line-selector REGEX, not a path glob: re-rooting it
/// would prefix the regex with the directory (`sub/TODO`) so it never matches.
/// Their file scope is `paths:`, which is still re-rooted.
const REGEX_SELECT_KINDS: &[&str] = &["for_each_match", "ordered_block"];

/// Kinds whose real selectors are not `paths` / `select` / `primary` (commit
/// ranges, diff globs, `source` / `targets`, graph nodes), so re-rooting the
/// scope fields cannot confine them to the nested directory: a dummy `paths:`
/// would let them run repo-wide from a subtree config. Refused in nested configs;
/// declare them in the root config. (v0.19's `KindMeta` derives this from each
/// kind's typed selectors.)
const UNCONFINABLE_KINDS: &[&str] = &[
    "changeset_requires_path",
    "cross_file",
    "cross_file_value_equals",
    "file_graph",
    "git_commit_author_allowlist",
    "git_commit_gpg_signed",
    "git_commit_message",
    "git_commit_no_fixup",
    "git_commit_signed_off",
    "git_commit_subject_matches",
    "git_no_denied_paths",
    "pair_changed_together",
    "pair_hash",
    "registry_paths_resolve",
];

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
        respect_gitignore: root.respect_gitignore.unwrap_or(true),
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
            if let Some(id) = rule.get("id").and_then(|v| v.as_str())
                && !seen_ids.insert(id.to_string())
            {
                return Err(Error::rule_config(
                    id,
                    format!(
                        "nested config {} redefines rule id {id:?} - \
                             per-subtree overrides aren't supported yet; \
                             pick a unique id or disable the root's rule \
                             and define it per-subtree",
                        abs.display()
                    ),
                ));
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
    // drop-ins. Its errors already name the file.
    let mut config: RawConfig = crate::loader::parse_config_interpolated(&contents, abs_path)?;

    // MVP: reject nested configs that try to set anything that
    // could affect the whole repo. Only `version:` and `rules:`
    // are meaningful; everything else is suspicious enough to
    // require an explicit error.
    let source = abs_path.display().to_string();
    if !config.extends.is_empty() {
        return Err(Error::Other(format!(
            "nested config {source} declares `extends:` - nested configs \
             are flat in this release; extend only from the root config"
        )));
    }
    if !config.facts.is_empty() {
        return Err(Error::Other(format!(
            "nested config {source} declares `facts:` - facts are a \
             root-only concept; move the fact to the root config"
        )));
    }
    if !config.vars.is_empty() {
        return Err(Error::Other(format!(
            "nested config {source} declares `vars:` - vars are a \
             root-only concept; move them to the root config"
        )));
    }
    if !config.ignore.is_empty() || config.nested_configs == Some(true) {
        return Err(Error::Other(format!(
            "nested config {source} declares `ignore:` or `nested_configs:` - \
             both are root-only in this release"
        )));
    }
    if config.baseline.is_some() {
        return Err(Error::Other(format!(
            "nested config {source} declares `baseline:` - the baseline is a \
             trusted, root-only input; declare it in the top-level config"
        )));
    }
    if !config.trusted_extends.is_empty() {
        return Err(Error::Other(format!(
            "nested config {source} declares `trusted_extends:` - it is a trusted, \
             root-only grant (a subtree config must not allowlist a remote ruleset's \
             content fixers); declare it in the top-level config"
        )));
    }
    if !config.allow_out_of_root.is_confined() {
        return Err(Error::Other(format!(
            "nested config {source} declares `allow_out_of_root:` - the out-of-root \
             escape hatch is a trusted, root-only grant; declare it in the top-level \
             config. (It is ignored here, never silently honored.)"
        )));
    }
    if !config.templates.is_empty() {
        return Err(Error::Other(format!(
            "nested config {source} declares `templates:` - templates are a root-only \
             concept (a nested template would be silently dropped, and could otherwise \
             smuggle a spawning rule into the subtree); declare them in the top-level \
             config"
        )));
    }
    // A nested config is untrusted in exactly the way an `extends:`'d
    // ruleset is — a subtree `.alint.yml` may be authored by anyone who
    // can open a PR against the monorepo — so it may not introduce a
    // process-spawning rule. `kind: command` / `command_idempotent` /
    // `generated_file_fresh` are confined to the user's own top-level
    // config; without this gate a nested `.alint.yml` declaring
    // `kind: command` achieved arbitrary code execution on `alint check`
    // (the C2 RCE bypass). Mirrors the `extends:` gate; spawning templates
    // are refused wholesale just above, and `finalize` re-checks the
    // expanded rule set as a backstop.
    crate::reject_command_rules_in(&config.rules, &source)?;
    // ...and no *spawning fix op* (`git_untrack`, `git rm --cached`) either: a
    // subtree `.alint.yml` is as untrusted as an `extends:`'d ruleset, so a
    // spawning fixer it declares would shell out on `alint fix`. (Templates are
    // already refused wholesale above, so only `rules:` needs this gate here.)
    crate::reject_spawning_fix_ops_in(&config.rules, &source)?;
    // ...and no inherited rule may promote a destructive fix to auto-apply.
    crate::reject_fix_promotion_in(&config.rules, &source)?;
    // ...and every CONTENT-INJECTING fixer is demoted to a suggestion, EXACTLY as
    // for an `extends:`'d remote (loader.rs). Without this a subtree `.alint.yml`
    // -- untrusted, "anyone who can open a PR" -- could auto-apply a content fixer
    // on a bare `alint fix`; and because a fix op's own explicit `path` is NOT
    // re-scoped to the subtree (only `paths`/`select`/`primary` are), a nested
    // `file_create` could write a repo-ROOT file (e.g. a `.github/workflows/` CI
    // job -> code execution once pushed). Demoting to a suggestion closes the
    // SILENT auto-apply, matching this module's stated "as untrusted as an
    // `extends:`'d ruleset" trust model (audit: nested-config HIGH).
    crate::demote_content_fixers_in(&mut config.rules);
    // Preserve that source trust through root-template expansion. Without this
    // marker a nested rule with `extends_template:` could acquire a root template's
    // content fixer only after the source-local demotion above had already run.
    crate::mark_untrusted_fix_sources_in(&mut config.rules);

    // Glob patterns are platform-agnostic (always `/`); on
    // Windows `rel_dir.to_string_lossy()` would emit `\` and we'd
    // end up with mixed-separator globs like `packages\foo/foo.txt`
    // that don't compile against globset cleanly.
    let dir_prefix = rel_dir.to_string_lossy().replace('\\', "/");
    let mut out = Vec::with_capacity(config.rules.len());
    for mut rule in config.rules {
        // Re-rooting is deferred to `finalize`, AFTER template expansion: a nested
        // rule may `extends_template:` a root template, and the template's kind and
        // path fields must be confined (and kind-checked) too. Re-rooting the raw
        // instance here missed both (v0.18.3 review). The marker carries the prefix.
        let mut marker = Mapping::new();
        marker.insert("prefix".into(), dir_prefix.clone().into());
        marker.insert("source".into(), source.clone().into());
        rule.insert(
            NESTED_SCOPE_MARKER.into(),
            serde_yaml_ng::Value::Mapping(marker),
        );
        out.push(rule);
    }
    Ok(out)
}

/// Internal raw-mapping marker on a rule contributed by a nested config: the
/// subtree prefix and the config's path. [`apply_nested_scope`] consumes it in
/// `finalize`, after template expansion, so it never reaches the typed `RuleSpec`.
pub(crate) const NESTED_SCOPE_MARKER: &str = "__alint_internal_nested_scope";

/// Confine an expanded rule to the nested subtree it came from, if it came from a
/// nested config. Called on the EFFECTIVE (template-expanded) rule.
pub(crate) fn apply_nested_scope(rule: &mut Mapping) -> Result<()> {
    let Some(serde_yaml_ng::Value::Mapping(marker)) = rule.remove(NESTED_SCOPE_MARKER) else {
        return Ok(());
    };
    let field = |name: &str| {
        marker
            .get(name)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    scope_rule(rule, &field("prefix"), &field("source"))
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

    let kind = rule
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let any_scoped = scope_kind_fields(rule, &kind, prefix, &id_hint, source, false)?;

    // A manifest predicate's `source:` is a path relative to this config's
    // directory (like `paths:`), so rebase it under the nested prefix — else a
    // nested rule would read the repo-root manifest, not its own package's, and
    // silently mis-scope. `has_ancestor:` / `changed_since:` need no rebasing (a
    // bare filename walked up the tree / a git ref). Not counted toward
    // `any_scoped`: a per-file rule still needs its own `paths:` base glob.
    if let Some(sf) = rule
        .get_mut("scope_filter")
        .and_then(serde_yaml_ng::Value::as_mapping_mut)
    {
        for key in ["include_manifest_paths", "exclude_manifest_paths"] {
            if let Some(src) = sf
                .get_mut(key)
                .and_then(serde_yaml_ng::Value::as_mapping_mut)
                .and_then(|mp| mp.get_mut("source"))
                && let serde_yaml_ng::Value::String(s) = src
            {
                *s = scope_glob(s, prefix).map_err(|e| {
                    Error::rule_config(
                        &id_hint,
                        format!("scoping `scope_filter.{key}.source` in {source}: {e}"),
                    )
                })?;
            }
        }
    }

    if !any_scoped {
        return Err(Error::rule_config(
            &id_hint,
            format!(
                "rule in nested config {source} has no path-like scope \
                 field ({}) - nested configs can only contribute rules \
                 whose scope can be confined to the nested directory",
                NESTED_SCOPE_FIELDS.join(", "),
            ),
        ));
    }
    Ok(())
}

/// Nested `require:` rules of these kinds are themselves rules with scope fields.
const NESTED_RULE_KINDS: &[&str] = &["for_each_dir", "for_each_file", "every_matching_has"];

/// Re-root the path-like fields of one rule (or, with `in_require`, of one nested
/// `require:` entry) of kind `kind`, recursing into nested `require:` blocks.
/// Returns whether any primary scope field (`paths`, `select`, `primary`) was
/// present. Inside a `require:` block, a value that starts with a template token
/// (`{path}/...`) is already anchored to the iterated entry, which is itself
/// re-rooted, so it is left alone; any other value is relative to the repository
/// root and is prefixed, or it would escape the subtree (v0.18.3 review).
fn scope_kind_fields(
    rule: &mut Mapping,
    kind: &str,
    prefix: &str,
    id_hint: &str,
    source: &str,
    in_require: bool,
) -> Result<bool> {
    if UNCONFINABLE_KINDS.contains(&kind) {
        return Err(Error::rule_config(
            id_hint,
            format!(
                "rule in nested config {source} uses kind `{kind}`{}, whose selectors \
                 cannot be confined to the nested directory (it would run over the whole \
                 repository); declare it in the root config",
                if in_require {
                    " in a `require:` block"
                } else {
                    ""
                }
            ),
        ));
    }
    let scope_err = |field: &str, e: Error| {
        Error::rule_config(id_hint, format!("scoping `{field}` in {source}: {e}"))
    };

    let mut any_scoped = false;
    for field in NESTED_SCOPE_FIELDS {
        if *field == "select" && REGEX_SELECT_KINDS.contains(&kind) {
            continue;
        }
        if let Some(value) = rule.get_mut(*field) {
            scope_paths_value_in(value, prefix, in_require).map_err(|e| scope_err(field, e))?;
            any_scoped = true;
        }
    }

    // Kind-specific fields that are repository-relative paths or globs.
    match kind {
        "import_gate" => {
            if let Some(value) = rule.get_mut("allow") {
                scope_paths_value_in(value, prefix, in_require)
                    .map_err(|e| scope_err("allow", e))?;
            }
        }
        "json_schema_passes" => {
            if let Some(serde_yaml_ng::Value::String(s)) = rule.get_mut("schema_path") {
                *s = scope_value_in(s, prefix, in_require)
                    .map_err(|e| scope_err("schema_path", e))?;
            }
        }
        "markdown_links_resolve" => {
            if let Some(serde_yaml_ng::Value::String(s)) = rule
                .get_mut("root")
                .and_then(serde_yaml_ng::Value::as_mapping_mut)
                .and_then(|m| m.get_mut("dir"))
            {
                *s = if s == "." {
                    prefix.to_string()
                } else {
                    scope_value_in(s, prefix, in_require).map_err(|e| scope_err("root.dir", e))?
                };
            }
        }
        "pair" => {
            if let Some(serde_yaml_ng::Value::String(s)) = rule.get_mut("partner") {
                // A partner template built from the (re-rooted) primary's tokens
                // stays in the subtree; a literal partner is prefixed.
                if !starts_with_token(s) {
                    *s = scope_glob(s, prefix).map_err(|e| scope_err("partner", e))?;
                }
            }
        }
        _ => {}
    }

    if NESTED_RULE_KINDS.contains(&kind)
        && let Some(serde_yaml_ng::Value::Sequence(items)) = rule.get_mut("require")
    {
        for item in items {
            let Some(child) = item.as_mapping_mut() else {
                continue;
            };
            let child_kind = child
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            scope_kind_fields(child, &child_kind, prefix, id_hint, source, true)?;
        }
    }
    Ok(any_scoped)
}

/// Whether a path value (after an optional `!`) starts with a template token.
fn starts_with_token(value: &str) -> bool {
    value.strip_prefix('!').unwrap_or(value).starts_with('{')
}

/// Re-root one path or glob, leaving a token-anchored value inside a `require:`
/// block untouched (see [`scope_kind_fields`]).
fn scope_value_in(value: &str, prefix: &str, in_require: bool) -> Result<String> {
    if in_require && starts_with_token(value) {
        Ok(value.to_string())
    } else {
        scope_glob(value, prefix)
    }
}

/// [`scope_paths_value`] with the `require:`-block token rule applied to every
/// string in the value.
fn scope_paths_value_in(
    value: &mut serde_yaml_ng::Value,
    prefix: &str,
    in_require: bool,
) -> Result<()> {
    if !in_require {
        return scope_paths_value(value, prefix);
    }
    match value {
        serde_yaml_ng::Value::String(s) => *s = scope_value_in(s, prefix, true)?,
        serde_yaml_ng::Value::Sequence(seq) => {
            for item in seq {
                if let serde_yaml_ng::Value::String(s) = item {
                    *s = scope_value_in(s, prefix, true)?;
                }
            }
        }
        serde_yaml_ng::Value::Mapping(m) => {
            for key in &["include", "exclude"] {
                if let Some(inner) = m.get_mut(*key) {
                    scope_paths_value_in(inner, prefix, true)?;
                }
            }
        }
        _ => {}
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
            "absolute path {glob:?} can't be used in a nested config - \
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
