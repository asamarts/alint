//! The merge of a list in one pass ([`Merger`]) against the left fold of the
//! two-config merge it replaced (#580), kept here as it was
//! ([`reference::merge`]).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use alint_core::facts::FactKind;
use alint_core::{AllowOutOfRoot, ConfigRequirements};
use proptest::prelude::*;

use super::*;
use crate::v2;

/// The merge as it was before #580: `b` merged onto `a`, rebuilding `a`.
mod reference {
    use super::*;
    use crate::loader;

    pub(super) fn merge(a: RawConfig, b: RawConfig) -> Result<RawConfig> {
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
    #[allow(clippy::too_many_lines)]
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
        let (baseline, baseline_from_environment) = if b.baseline.is_some() {
            (b.baseline, b.baseline_from_environment)
        } else {
            (a.baseline, a.baseline_from_environment)
        };

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

        let mut limits = alint_core::ConfigLimits::default();
        limits.findings_per_rule = b.limits.findings_per_rule.or(a.limits.findings_per_rule);
        limits.key_path_bytes_per_decode = b
            .limits
            .key_path_bytes_per_decode
            .or(a.limits.key_path_bytes_per_decode);

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
            limits,
            nested_configs,
            allow_out_of_root,
            baseline,
            baseline_from_environment,
            trusted_extends,
            env_vars,
            untrusted_when_var_reads,
            compatibility,
            environment_values,
            config_files: [a.config_files, b.config_files].concat(),
            environment_config_files: [a.environment_config_files, b.environment_config_files]
                .concat(),
            field_source_names: either_set(a.field_source_names, b.field_source_names),
            extends_environment: Vec::new(),
            extends_filter_environment: Vec::new(),
            source_size: a.source_size.saturating_add(b.source_size),
            // The language follows the version: the source that sets it decides.
            surface: b.version.map_or(a.surface, |_| b.surface),
            v2: merged_objects(a.v2, b.v2),
        }
    }

    /// `a`, unless it is empty: for a list set once, on the top-level result
    /// after every merge (`RawConfig::field_source_names`).
    fn either_set<T>(a: Vec<T>, b: Vec<T>) -> Vec<T> {
        if a.is_empty() { b } else { a }
    }

    /// `a`'s version 2 sources, then those of `b` that `a` does not hold yet:
    /// a file an `extends:` diamond reaches twice declares its objects once
    /// (`v2::Objects::merged`).
    fn merged_objects(mut a: v2::Objects, b: v2::Objects) -> v2::Objects {
        if b.sources.is_empty() {
            return a;
        }
        for source in b.sources {
            if !a
                .sources
                .iter()
                .any(|held| held.source.path == source.source.path)
            {
                a.sources.push(source);
            }
        }
        a
    }
}

/// Everything `config` holds, as text: `vars` and `env_vars` in name order,
/// every list in its order, and each mapping's keys in theirs.
fn canonical(config: &RawConfig) -> String {
    // Every field, so a new one is compared too.
    let RawConfig {
        version,
        requires,
        extends,
        ignore,
        respect_gitignore,
        vars,
        facts,
        templates,
        rules,
        fix_size_limit,
        limits,
        nested_configs,
        allow_out_of_root,
        baseline,
        baseline_from_environment,
        trusted_extends,
        env_vars,
        untrusted_when_var_reads,
        compatibility,
        environment_values,
        config_files,
        environment_config_files,
        field_source_names,
        extends_environment,
        extends_filter_environment,
        source_size,
        surface,
        v2,
    } = config;
    let vars: BTreeMap<_, _> = vars.iter().collect();
    let env_vars: BTreeMap<_, _> = env_vars.iter().collect();
    format!(
        "version {version:?}\nrequires {requires:?}\nextends {extends:?}\nignore {ignore:?}\n\
         respect_gitignore {respect_gitignore:?}\nvars {vars:?}\nfacts {facts:?}\n\
         templates {templates:?}\nrules {rules:?}\nfix_size_limit {fix_size_limit:?}\n\
         limits {limits:?}\n\
         nested_configs {nested_configs:?}\nallow_out_of_root {allow_out_of_root:?}\n\
         baseline {baseline:?}\nbaseline_from_environment {baseline_from_environment:?}\n\
         trusted_extends {trusted_extends:?}\nenv_vars {env_vars:?}\n\
         untrusted_when_var_reads {untrusted_when_var_reads:?}\n\
         compatibility {compatibility:?}\nenvironment_values {environment_values:?}\n\
         config_files {config_files:?}\nenvironment_config_files {environment_config_files:?}\n\
         field_source_names {field_source_names:?}\n\
         extends_environment {extends_environment:?}\nextends_filter_environment {extends_filter_environment:?}\nsource_size {source_size}\n\
         surface {surface:?}\nv2 {v2:?}\n"
    )
}

/// `count` of a few values, so a list repeats them and sources share them.
fn few(prefix: &'static str, count: u8) -> impl Strategy<Value = String> {
    (0..count).prop_map(move |index| format!("{prefix}{index}"))
}

fn list<T: std::fmt::Debug>(
    entry: impl Strategy<Value = T>,
    most: usize,
) -> impl Strategy<Value = Vec<T>> {
    prop::collection::vec(entry, 0..=most)
}

/// A field's value, of a few: text, a list, or a nested mapping (a `paths:`
/// include and exclude pair), which a merge replaces wholesale.
fn value(field: &str, value: u8, shape: u8) -> ayaml::Value {
    let text = |at: u8| ayaml::Value::from(format!("{field}{at}"));
    match shape {
        0 | 1 => text(value),
        2 => vec![text(value), text(value + 1)].into(),
        _ => [
            ("include".into(), vec![text(value)].into()),
            ("exclude".into(), text(value + 1)),
        ]
        .into_iter()
        .collect::<Mapping>()
        .into(),
    }
}

/// A rule or template: an id of a few, so ids repeat and override, or none;
/// fields of a few values; the markers a merge carries, in any order, as the
/// loader writes them, or now and then a table a merge would change (empty,
/// or a sensitive field marked `false`); and fields after them, as a merge
/// appends a field.
fn mapping() -> impl Strategy<Value = Mapping> {
    let field = || {
        (
            prop::sample::select(&["kind", "level", "paths", "message", "vars"][..]),
            0..3_u8,
            0..4_u8,
        )
    };
    (
        prop::option::weighted(0.8, few("id", 4)),
        list(field(), 3),
        (
            prop::option::of(0..3_u64),
            prop::option::of(1..8_u8),
            prop::option::of(0..3_usize),
            prop::option::of((0..3_u8, 0..2_u8)),
            any::<bool>(),
            any::<bool>(),
        ),
        Just((0..6).collect::<Vec<u8>>()).prop_shuffle(),
        prop::option::weighted(0.1, 0..4_usize),
        list(field(), 2),
    )
        .prop_map(|(id, fields, markers, order, flawed, late)| {
            let (source, sensitive, origin, provenance, untrusted, env_vars) = markers;
            let mut mapping = Mapping::new();
            if let Some(id) = id {
                mapping.insert("id".into(), id.into());
            }
            for (field, at, shape) in fields {
                mapping.insert(field.into(), value(field, at, shape));
            }
            let marked = std::slice::from_mut(&mut mapping);
            for marker in order {
                match marker {
                    0 => {
                        if let Some(source) = source {
                            crate::mark_field_sources_in(marked, &source.into());
                        }
                    }
                    1 => {
                        // Some of the fields so far, if any.
                        let fields: Mapping = marked[0]
                            .keys()
                            .filter(|key| key.as_str().is_some_and(|key| !key.starts_with("__")))
                            .enumerate()
                            .filter(|(at, _)| sensitive.is_some_and(|bits| bits & (1 << at) != 0))
                            .map(|(_, key)| (key.clone(), true.into()))
                            .collect();
                        if !fields.is_empty() {
                            marked[0].insert(crate::SENSITIVE_FIELDS_MARKER.into(), fields.into());
                        }
                    }
                    2 => {
                        if let Some(origin) = origin {
                            crate::v2::mark_field_origins(marked, origin);
                        }
                    }
                    3 => {
                        if let Some((class, source)) = provenance {
                            let class = match class {
                                0 => crate::SourceClass::Extended,
                                1 => crate::SourceClass::Remote,
                                _ => crate::SourceClass::UntrustedRemote,
                            };
                            crate::mark_provenance_in(
                                marked,
                                class,
                                &ayaml::Value::from(u64::from(source)),
                            );
                        }
                    }
                    4 => {
                        if untrusted {
                            crate::mark_untrusted_fix_sources_in(marked);
                        }
                    }
                    _ => {
                        if env_vars {
                            let mut vars = Mapping::new();
                            vars.insert("v0".into(), "{{env.V0}}".into());
                            marked[0].insert(crate::ENV_VARS_MARKER.into(), vars.into());
                        }
                    }
                }
            }
            if let Some(flawed) = flawed {
                flaw(&mut mapping, flawed);
            }
            for (field, at, shape) in late {
                mapping.insert(field.into(), value(field, at, shape));
            }
            mapping
        })
}

/// The tables of a mapping a merge would change, merging it onto itself: the
/// field sources, the sensitive fields or the field origins empty
/// (`flawed` 0 to 2), or the first of its fields marked sensitive `false`.
fn flaw(mapping: &mut Mapping, flawed: usize) {
    let tables = [
        crate::FIELD_SOURCES_MARKER,
        crate::SENSITIVE_FIELDS_MARKER,
        crate::v2::FIELD_ORIGINS_MARKER,
    ];
    if let Some(table) = tables.get(flawed) {
        mapping.insert((*table).into(), Mapping::new().into());
        return;
    }
    let field = mapping
        .keys()
        .find(|key| key.as_str().is_some_and(|key| !key.starts_with("__")))
        .cloned();
    if let Some(field) = field {
        let marked: Mapping = [(field, false.into())].into_iter().collect();
        mapping.insert(crate::SENSITIVE_FIELDS_MARKER.into(), marked.into());
    }
}

/// The `when` reads of `vars.v<var>` an untrusted source `s<source>` made in
/// its rule `r<rule>`, as the loader records them.
fn when_reads(reads: Vec<(u8, u8, u8)>) -> Vec<UntrustedWhenVarRead> {
    reads
        .into_iter()
        .flat_map(|(rule, var, source)| {
            let mut mapping = Mapping::new();
            mapping.insert("id".into(), format!("r{rule}").into());
            mapping.insert("when".into(), format!("vars.v{var} == 'x'").into());
            crate::reject_env_reads_in_when(&[mapping], &[], &format!("s{source}")).unwrap()
        })
        .collect()
}

/// A version 2 source at `/repo/p<path>.yml` that declares `selectors`
/// selectors, as parsed: the same path in two configs is a diamond.
fn version_two_source(path: u8, selectors: u8) -> RawConfig {
    thread_local! {
        static PARSED: std::cell::RefCell<BTreeMap<(u8, u8), RawConfig>> =
            const { std::cell::RefCell::new(BTreeMap::new()) };
    }
    PARSED.with(|parsed| {
        parsed
            .borrow_mut()
            .entry((path, selectors))
            .or_insert_with(|| {
                let declared = (0..selectors).fold(String::new(), |mut out, index| {
                    writeln!(out, "  s{index}: {{ paths: x{index} }}").unwrap();
                    out
                });
                let text = if declared.is_empty() {
                    "version: 2\n".to_string()
                } else {
                    format!("version: 2\nselectors:\n{declared}")
                };
                crate::loader::parse_config_interpolated_with_environment(
                    &text,
                    Path::new(&format!("/repo/p{path}.yml")),
                    &crate::HostEnvironment,
                    &crate::loader::TEST_ROOT,
                )
                .unwrap()
            })
            .clone()
    })
}

/// The settings a merge takes from the later config when it sets them.
type Settings = (
    Option<u32>,
    bool,
    Option<bool>,
    Option<Option<u64>>,
    Option<bool>,
    u8,
    Option<u8>,
    bool,
);

fn settings() -> impl Strategy<Value = Settings> {
    (
        prop::option::of(1..=2_u32),
        any::<bool>(),
        prop::option::of(any::<bool>()),
        prop::option::of(prop::option::of(0..3_u64)),
        prop::option::of(any::<bool>()),
        0..3_u8,
        prop::option::of(0..2_u8),
        any::<bool>(),
    )
}

/// The lists a merge combines, as generated.
type Lists = (
    (Vec<Mapping>, Vec<Mapping>),
    (Vec<(String, String)>, Vec<(String, bool)>),
    Vec<(String, String)>,
    (Vec<String>, Vec<String>),
    Vec<(u8, u8, u8)>,
    Vec<Record>,
    (Vec<String>, Vec<String>, Vec<String>),
    Vec<(u8, u8)>,
    (usize, Vec<bool>, Vec<String>),
);

fn lists() -> impl Strategy<Value = Lists> {
    (
        (list(mapping(), 5), list(mapping(), 3)),
        (
            list((few("v", 4), few("x", 3)), 3),
            list((few("v", 5), any::<bool>()), 3),
        ),
        list((few("f", 3), few("count", 3)), 3),
        (list(few("i", 5), 4), list(few("u", 3), 2)),
        list((0..3_u8, 0..3_u8, 0..2_u8), 2),
        list(
            (
                few("c", 2),
                any::<bool>(),
                1..=2_u32,
                prop::option::of(Just(StableVersion::new(0, 1, 0))),
                prop::option::of(1..=2_u32),
            ),
            2,
        ),
        (
            list(few("e", 4), 3),
            list(few("file", 3), 2),
            list(few("n", 2), 2),
        ),
        list((0..4_u8, 0..3_u8), 2),
        (0..1_000_usize, list(any::<bool>(), 2), list(few("x", 2), 2)),
    )
}

/// A config of a few of each thing a merge combines.
fn config() -> impl Strategy<Value = RawConfig> {
    (settings(), lists()).prop_map(|(settings, lists)| {
        let mut config = with_settings(settings);
        let (
            (rules, templates),
            (vars, env_vars),
            facts,
            (ignore, trusted_extends),
            reads,
            compatibility,
            (environment_values, config_files, field_source_names),
            v2_sources,
            (source_size, extends_environment, extends),
        ) = lists;
        config.rules = rules;
        config.templates = templates;
        // Marks for some vars, and for names the config does not set, which a
        // merge ignores.
        config.env_vars = env_vars
            .into_iter()
            .filter(|(_, marked)| *marked)
            .map(|(name, _)| (name.clone(), format!("{{{{env.{name}}}}}")))
            .collect();
        config.vars = vars.into_iter().collect();
        config.facts = facts
            .into_iter()
            .map(|(id, count_files)| FactSpec {
                id,
                kind: FactKind::CountFiles { count_files },
            })
            .collect();
        config.ignore = ignore;
        config.trusted_extends = trusted_extends;
        config.untrusted_when_var_reads = when_reads(reads);
        config.compatibility = compatibility.into_iter().map(record).collect();
        config.environment_values = environment_values;
        config.config_files = config_files.into_iter().map(PathBuf::from).collect();
        config.field_source_names = field_source_names;
        for (path, selectors) in v2_sources {
            let parsed = version_two_source(path, selectors);
            config.v2.sources.extend(parsed.v2.sources);
        }
        config.source_size = source_size;
        config.extends_environment = extends_environment;
        config.extends = extends
            .into_iter()
            .map(|url| v2::ExtendsEntry {
                entry: alint_core::ExtendsEntry::Url(url),
                alias: None,
            })
            .collect();
        config
    })
}

/// A config with `settings` and nothing else.
fn with_settings(settings: Settings) -> RawConfig {
    let (
        version,
        v2_surface,
        respect_gitignore,
        fix_size_limit,
        nested_configs,
        allow_out_of_root,
        baseline,
        requires,
    ) = settings;
    RawConfig {
        version,
        requires: requires.then_some(ConfigRequirements {
            alint: StableVersion::new(0, 1, 0),
        }),
        respect_gitignore,
        fix_size_limit,
        nested_configs,
        allow_out_of_root: match allow_out_of_root {
            0 => AllowOutOfRoot::Confined,
            1 => AllowOutOfRoot::All,
            _ => AllowOutOfRoot::Selective {
                kinds: ["file_exists".to_string()].into(),
                rules: std::collections::HashSet::new(),
            },
        },
        baseline: baseline.map(|index| PathBuf::from(format!("b{index}.json"))),
        surface: if v2_surface {
            v2::Surface::V2
        } else {
            v2::Surface::V1
        },
        ..RawConfig::default()
    }
}

/// A compatibility record as generated: of one of a few sources, so two
/// sources' records can be equal, and of a few of each field.
type Record = (String, bool, u32, Option<StableVersion>, Option<u32>);

fn record(
    (source, root, config_version, requires_alint, consumer_version): Record,
) -> ConfigCompatibility {
    ConfigCompatibility {
        source,
        source_class: if root {
            ConfigSourceClass::Root
        } else {
            ConfigSourceClass::LocalExtend
        },
        config_version,
        requires_alint,
        consumer_version,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// A list pushed onto one [`Merger`] merges to what the left fold of the
    /// old merge gave, and counts the entries the fold's result held at every
    /// step, which [`MAX_MERGED_ENTRIES`] bounds. The list names some sources
    /// more than once, as an `extends:` list can: each is merged from where it
    /// is held but the last time, and a rule it merged onto last is not merged
    /// again ([`Remerge`]). A source named once is moved in or merged from
    /// where it is held.
    #[test]
    fn a_list_merged_in_one_pass_equals_the_fold_of_the_old_merge(
        first in config(),
        sources in prop::collection::vec(config(), 1..=4),
        entries in prop::collection::vec((0..4_usize, any::<bool>()), 0..=12),
    ) {
        let entries: Vec<(usize, bool)> = entries
            .into_iter()
            .map(|(source, held)| (source % sources.len(), held))
            .collect();
        let mut folded = first.clone();
        let mut merger = Merger::new(first);
        for (at, &(source, held)) in entries.iter().enumerate() {
            folded = reference::merge(folded, sources[source].clone()).unwrap();
            let named = |entries: &[(usize, bool)]| {
                entries.iter().filter(|(other, _)| *other == source).count()
            };
            let pushed = if named(&entries) == 1 && !held {
                merger.push(sources[source].clone())
            } else if named(&entries[at + 1..]) > 0 || held {
                merger.push_repeated(Cow::Borrowed(&sources[source]), source)
            } else {
                merger.push_repeated(Cow::Owned(sources[source].clone()), source)
            };
            pushed.unwrap();
            prop_assert_eq!(merger.entry_count(), folded.entry_count());
            prop_assert_eq!(merger.version(), folded.version);
        }
        prop_assert_eq!(canonical(&merger.finish()), canonical(&folded));
    }
}

/// A rule a repeated source added as it was, whose tables a merge of it onto
/// itself would change ([`flaw`]), is merged again, not replayed: `[a, c, a]`
/// merges to what the fold gives.
#[test]
fn a_rule_whose_tables_a_merge_would_change_is_merged_again() {
    let config = |id: &str| {
        let mut rule = Mapping::new();
        rule.insert("id".into(), id.into());
        rule.insert("kind".into(), "file_exists".into());
        crate::mark_field_sources_in(std::slice::from_mut(&mut rule), &0_u64.into());
        RawConfig {
            rules: vec![rule],
            ..RawConfig::default()
        }
    };
    for flawed in 0..4 {
        let mut a = config("a");
        flaw(&mut a.rules[0], flawed);
        let c = config("c");
        let mut folded = RawConfig::default();
        for source in [&a, &c, &a] {
            folded = reference::merge(folded, source.clone()).unwrap();
        }
        let mut merger = Merger::new(RawConfig::default());
        merger.push_repeated(Cow::Borrowed(&a), 0).unwrap();
        merger.push(c).unwrap();
        merger.push_repeated(Cow::Owned(a), 0).unwrap();
        assert_eq!(merger.take_merged_onto(), 1, "flaw {flawed}");
        assert_eq!(
            canonical(&merger.finish()),
            canonical(&folded),
            "flaw {flawed}"
        );
    }
}

/// A merge is refused at the step the old fold was, with its message, though
/// repeats it keeps once came before.
#[test]
fn a_merge_past_the_size_is_refused_where_the_fold_was() {
    let patterns =
        |range: std::ops::Range<usize>| -> Vec<String> { range.map(|i| i.to_string()).collect() };
    let configs = [
        RawConfig {
            ignore: patterns(0..MAX_MERGED_ENTRIES - 2),
            ..RawConfig::default()
        },
        RawConfig {
            ignore: patterns(0..1_000),
            ..RawConfig::default()
        },
        RawConfig {
            ignore: patterns(MAX_MERGED_ENTRIES - 2..MAX_MERGED_ENTRIES),
            ..RawConfig::default()
        },
        RawConfig {
            ignore: patterns(MAX_MERGED_ENTRIES..MAX_MERGED_ENTRIES + 1),
            ..RawConfig::default()
        },
    ];
    let mut folded = Ok(configs[0].clone());
    let mut merger = Merger::new(configs[0].clone());
    let mut pushed = Ok(());
    for source in &configs[1..] {
        folded = folded.and_then(|folded| reference::merge(folded, source.clone()));
        pushed = pushed.and_then(|()| merger.push(source.clone()));
        assert_eq!(pushed.is_err(), folded.is_err());
    }
    assert_eq!(
        pushed.unwrap_err().to_string(),
        folded.unwrap_err().to_string()
    );
}

/// A merge handles what each config brings in, wherever it falls in the
/// list: a thousand configs of a hundred rules each touch each rule once,
/// where the old merge rebuilt the rules merged so far at every config.
#[test]
fn a_push_handles_what_it_brings_in() {
    let config = |index: usize| RawConfig {
        rules: (0..100)
            .map(|rule| {
                let mut mapping = Mapping::new();
                mapping.insert("id".into(), format!("r{index}-{rule}").into());
                mapping
            })
            .collect(),
        ..RawConfig::default()
    };
    let before = MERGE_TOUCHED.with(std::cell::Cell::get);
    let mut merger = Merger::new(RawConfig::default());
    for index in 0..1_000 {
        merger.push(config(index)).unwrap();
    }
    let touched = MERGE_TOUCHED.with(std::cell::Cell::get) - before;
    assert_eq!(merger.finish().rules.len(), 100_000);
    assert_eq!(touched, 100_000);
}

/// A rule merged onto keeps its markers' storage: each was copied at every
/// merge onto it (`merge_mapping_fields`), so a thousand one-field overrides
/// of a rule of 20,000 fields copied 20 million marker entries. The address
/// of a marker's first entry stays where it was when the marker is moved.
#[test]
fn a_merge_onto_a_rule_moves_its_markers() {
    let markers = [
        crate::FIELD_SOURCES_MARKER,
        crate::SENSITIVE_FIELDS_MARKER,
        crate::v2::FIELD_ORIGINS_MARKER,
    ];
    let rule = |fields: &[&str]| {
        let mut mapping = Mapping::new();
        mapping.insert("id".into(), "r".into());
        for field in fields {
            mapping.insert((*field).into(), "v".into());
        }
        mapping
    };
    let mut wide = rule(&["kind", "level", "f1", "f2"]);
    crate::mark_field_sources_in(std::slice::from_mut(&mut wide), &0_u64.into());
    for marker in &markers[1..] {
        let mut marked = Mapping::new();
        marked.insert("f1".into(), true.into());
        marked.insert("f2".into(), true.into());
        wide.insert((*marker).into(), marked.into());
    }
    let mut narrow = rule(&[]);
    narrow.insert("level".into(), "off".into());
    crate::mark_field_sources_in(std::slice::from_mut(&mut narrow), &1_u64.into());
    let config = |rule: Mapping| RawConfig {
        rules: vec![rule],
        ..RawConfig::default()
    };
    let mut merger = Merger::new(RawConfig::default());
    merger.push(config(wide)).unwrap();
    let addresses = |merger: &Merger| -> Vec<*const ayaml::Value> {
        let rule = &merger.rules.with_id[0];
        markers
            .iter()
            .map(|marker| {
                let marked = rule
                    .get(*marker)
                    .and_then(ayaml::Value::as_mapping)
                    .unwrap();
                std::ptr::from_ref(marked.get_index(0).unwrap().0)
            })
            .collect()
    };
    let before = addresses(&merger);
    merger.push(config(narrow)).unwrap();
    assert_eq!(addresses(&merger), before);
    assert_eq!(merger.finish().rules[0]["level"], ayaml::Value::from("off"));
}
