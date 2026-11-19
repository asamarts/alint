//! Merging configs: each `extends:` source onto what its list merged so far,
//! and each `.alint.d/` drop-in onto the top-level config. Pulled out of
//! `lib.rs` to keep that file to the public surface and the config's shape.

use alint_core::{ConfigCompatibility, Error, FactSpec, Result};
use ayaml::Mapping;

use crate::{RawConfig, loader, merge_mapping_fields};

/// Add each of `incoming` that `target` does not hold yet.
pub(crate) fn extend_compatibility(
    target: &mut Vec<ConfigCompatibility>,
    incoming: impl IntoIterator<Item = ConfigCompatibility>,
) {
    for record in incoming {
        if !target.contains(&record) {
            target.push(record);
        }
    }
}

fn merged_compatibility(
    mut current: Vec<ConfigCompatibility>,
    incoming: Vec<ConfigCompatibility>,
) -> Vec<ConfigCompatibility> {
    extend_compatibility(&mut current, incoming);
    current
}

/// Facts merged by id: a later fact replaces an earlier one with its id, in
/// the earlier one's place.
fn merge_facts(a: Vec<FactSpec>, b: Vec<FactSpec>) -> Vec<FactSpec> {
    let mut facts_by_id: std::collections::BTreeMap<String, FactSpec> =
        std::collections::BTreeMap::new();
    let mut fact_order: Vec<String> = Vec::new();
    for f in a.into_iter().chain(b) {
        if !facts_by_id.contains_key(&f.id) {
            fact_order.push(f.id.clone());
        }
        facts_by_id.insert(f.id.clone(), f);
    }
    fact_order
        .into_iter()
        .map(|id| facts_by_id.remove(&id).unwrap())
        .collect()
}

/// The most entries (rules, templates, facts, vars, `ignore:` patterns,
/// recorded `when` reads, selectors and relations) a config may hold once its
/// `extends:` chain and drop-ins are merged. Real configs hold hundreds; an
/// `extends:` diamond that lists one source twice per level once doubled the
/// lists at each level.
pub(crate) const MAX_MERGED_ENTRIES: usize = 1 << 20;

/// `items` without repeats, each kept where it first appears.
fn dedup_in_order<T: Eq + std::hash::Hash + Clone>(items: Vec<T>) -> Vec<T> {
    let mut seen = std::collections::HashSet::with_capacity(items.len());
    items
        .into_iter()
        .filter(|item| seen.insert(item.clone()))
        .collect()
}

impl RawConfig {
    /// The entries [`MAX_MERGED_ENTRIES`] counts.
    fn entry_count(&self) -> usize {
        self.rules.len()
            + self.templates.len()
            + self.facts.len()
            + self.vars.len()
            + self.ignore.len()
            + self.untrusted_when_var_reads.len()
            + self.v2.object_count()
    }

    /// What an `extends:` merge or load handles of the config
    /// ([`loader::LoadState::charge_extends`]): its entries, and each version
    /// 2 source, which a merge looks for among those it holds.
    pub(crate) fn merge_size(&self) -> usize {
        self.entry_count() + self.v2.sources.len()
    }
}

/// [`merge_unbounded`], refused past [`MAX_MERGED_ENTRIES`].
pub(crate) fn merge(a: RawConfig, b: RawConfig) -> Result<RawConfig> {
    let merged = merge_unbounded(a, b);
    let objects = merged.v2.object_count();
    let entries = merged.entry_count();
    if entries > MAX_MERGED_ENTRIES {
        // A version 1 config's message is unchanged.
        let held = if objects == 0 {
            "rules, templates, facts, vars and `ignore:` patterns"
        } else {
            "rules, templates, facts, vars, `ignore:` patterns, selectors and relations"
        };
        return Err(Error::Other(format!(
            "the merged config holds {entries} entries ({held}), more than the \
             {MAX_MERGED_ENTRIES} supported; flatten the `extends:` chain or split the ruleset"
        )));
    }
    Ok(merged)
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
/// - `ignore` concatenated `a` then `b`, each pattern once.
/// - `version`, `respect_gitignore`, `fix_size_limit` and
///   `nested_configs` take `b`'s value when `b` sets it, else keep
///   `a`'s (defaults are applied once, in `finalize`). An
///   `extends:`'d config's values are dropped before the merge
///   ([`RawConfig::drop_top_level_settings`]).
/// - `extends` is always left empty on the merged result;
///   resolved already.
fn merge_unbounded(a: RawConfig, b: RawConfig) -> RawConfig {
    let version = b.version.or(a.version);
    let respect_gitignore = b.respect_gitignore.or(a.respect_gitignore);
    let fix_size_limit = b.fix_size_limit.or(a.fix_size_limit);
    let nested_configs = b.nested_configs.or(a.nested_configs);
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

    // Each list keeps one copy of an entry two sources share, so a source an
    // `extends:` diamond reaches twice does not double it at every level. An
    // `ignore:` pattern always excludes, so order and repeats do not matter.
    let mut ignore = a.ignore;
    ignore.extend(b.ignore);
    let ignore = dedup_in_order(ignore);

    // The env-derived mark follows the value that wins: `b` overriding a name
    // with a literal clears it, with an interpolated value sets it.
    let mut env_vars = a.env_vars;
    for name in b.vars.keys() {
        match b.env_vars.get(name) {
            Some(raw) => env_vars.insert(name.clone(), raw.clone()),
            None => env_vars.remove(name),
        };
    }
    let mut vars = a.vars;
    vars.extend(b.vars);
    let mut untrusted_when_var_reads = a.untrusted_when_var_reads;
    untrusted_when_var_reads.extend(b.untrusted_when_var_reads);
    let untrusted_when_var_reads = dedup_in_order(untrusted_when_var_reads);
    let compatibility = merged_compatibility(a.compatibility, b.compatibility);
    let mut environment_values = a.environment_values;
    loader::add_environment_values(&mut environment_values, &b.environment_values);
    let facts = merge_facts(a.facts, b.facts);

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
            merge_mapping_fields(existing, m);
        } else {
            template_order.push(id.clone());
            templates_by_id.insert(id, m);
        }
    }
    let mut templates: Vec<Mapping> = template_order
        .into_iter()
        .map(|id| templates_by_id.remove(&id).unwrap())
        .collect();
    templates.extend(dedup_in_order(template_orphans));

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
            merge_mapping_fields(existing, m);
        } else {
            rule_order.push(id.clone());
            rules_by_id.insert(id, m);
        }
    }
    let mut rules: Vec<Mapping> = rule_order
        .into_iter()
        .map(|id| rules_by_id.remove(&id).unwrap())
        .collect();
    rules.extend(dedup_in_order(orphans));

    // `trusted_extends:` is top-level-only -- consumed at is_top before any merge,
    // and rejected from an extended / nested source. Concatenate defensively so a
    // stray non-empty value still survives to the per-source rejection upstream.
    let mut trusted_extends = a.trusted_extends;
    trusted_extends.extend(b.trusted_extends);
    let trusted_extends = dedup_in_order(trusted_extends);

    RawConfig {
        version,
        requires: None,
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
        env_vars,
        untrusted_when_var_reads,
        compatibility,
        environment_values,
        config_files: [a.config_files, b.config_files].concat(),
        field_source_names: either_set(a.field_source_names, b.field_source_names),
        extends_environment: Vec::new(),
        source_size: a.source_size.saturating_add(b.source_size),
        // The language follows the version: the source that sets it decides.
        surface: b.version.map_or(a.surface, |_| b.surface),
        v2: a.v2.merged(b.v2),
    }
}

/// `a`, unless it is empty: for a list set once, on the top-level result
/// after every merge (`RawConfig::field_source_names`).
fn either_set<T>(a: Vec<T>, b: Vec<T>) -> Vec<T> {
    if a.is_empty() { b } else { a }
}
