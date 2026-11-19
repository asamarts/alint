//! Constitution invariant 2: under `--changed`, a rule that requires the full
//! index receives the full index whenever it runs. A path scope it declares
//! (for `constraint`, its trigger scope) decides only whether it runs, never
//! what it sees (ADR-0034 decision 3, ADR-0037).
//!
//! The test takes every kind the built-in registry marks full-index (a
//! `FullIndex` dispatch, or a per-file dispatch with `full_index`), with no
//! list of its own, and every scenario of the policy-test corpus that
//! configures one inline over an inline tree. It wraps each such rule in a
//! [`Probe`] that records what the engine hands it and evaluates nothing (so
//! no rule spawns a process here), adds an unchanged file to the tree, and runs
//! the engine once in full and then under `--changed` with each file of the
//! tree changed alone, and with a deleted path. Whenever the rule runs under
//! `--changed`, what it received must equal what it received in the full run:
//! the same index, and for a per-file rule the same files. Every full-index
//! kind must run under `--changed` in at least one scenario, so a new kind
//! without a scenario fails here instead of passing unchecked.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use alint_core::{
    Context, DispatchClass, Engine, PerFileRule, Result, Rule, RuleEntry, Violation, walk,
};
use alint_testkit::PolicyTest;

use crate::support;

/// An unchanged file in every tree, so a run that hands a rule only the
/// changed files differs from a full one.
const SENTINEL: &str = "zz-full-index-sentinel.txt";

/// A path no tree holds, changed as a deletion.
const DELETED: &str = "zz-deleted-under-changed.txt";

/// What a rule received in one run: the paths of the index it was handed,
/// for a per-file rule the files it was dispatched, and for a rule that reads
/// relations, each relation's rows and the resources they were read from.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Seen {
    indexes: BTreeSet<Vec<PathBuf>>,
    files: BTreeSet<PathBuf>,
    relations: BTreeSet<String>,
}

impl Seen {
    fn record_index(&mut self, ctx: &Context<'_>) {
        let paths = ctx
            .index
            .entries
            .iter()
            .map(|entry| entry.path.to_path_buf())
            .collect();
        self.indexes.insert(paths);
    }
}

/// A rule that forwards every question the engine asks to the rule it wraps,
/// but evaluates nothing: it records what it was handed and reports no
/// finding.
#[derive(Debug)]
struct Probe {
    inner: Arc<dyn Rule>,
    seen: Arc<Mutex<Seen>>,
    per_file: Option<PerFileProbe>,
}

#[derive(Debug)]
struct PerFileProbe {
    inner: Arc<dyn Rule>,
    seen: Arc<Mutex<Seen>>,
}

impl Probe {
    fn new(inner: Arc<dyn Rule>, seen: Arc<Mutex<Seen>>) -> Self {
        let per_file = inner.as_per_file().is_some().then(|| PerFileProbe {
            inner: Arc::clone(&inner),
            seen: Arc::clone(&seen),
        });
        Self {
            inner,
            seen,
            per_file,
        }
    }
}

impl Rule for Probe {
    fn id(&self) -> &str {
        self.inner.id()
    }
    fn level(&self) -> alint_core::Level {
        self.inner.level()
    }
    fn policy_url(&self) -> Option<&str> {
        self.inner.policy_url()
    }
    fn git_tracked_mode(&self) -> alint_core::GitTrackedMode {
        self.inner.git_tracked_mode()
    }
    fn wants_git_blame(&self) -> bool {
        self.inner.wants_git_blame()
    }
    fn wants_analysis_time(&self) -> bool {
        self.inner.wants_analysis_time()
    }
    fn uses_relations(&self) -> bool {
        self.inner.uses_relations()
    }
    fn relation_inputs(
        &self,
    ) -> Option<(
        &Arc<alint_core::relation::plan::Plan>,
        &[alint_core::relation::plan::RelationId],
    )> {
        self.inner.relation_inputs()
    }
    fn validate_nested(&self, registry: &alint_core::RuleRegistry) -> Result<()> {
        self.inner.validate_nested(registry)
    }
    fn nested_rule_specs(&self) -> &[alint_core::CompiledNestedSpec] {
        self.inner.nested_rule_specs()
    }
    fn requires_full_index(&self) -> bool {
        self.inner.requires_full_index()
    }
    fn changed_dependencies(&self) -> Vec<&Path> {
        self.inner.changed_dependencies()
    }
    fn path_scope(&self) -> Option<&alint_core::Scope> {
        self.inner.path_scope()
    }
    fn trigger_scope(&self) -> Option<&alint_core::relation::TriggerScope> {
        self.inner.trigger_scope()
    }
    fn sees_path_past_walk(&self, path: &Path) -> bool {
        self.inner.sees_path_past_walk(path)
    }
    fn supports_expect_matches(&self) -> bool {
        self.inner.supports_expect_matches()
    }
    fn scope_matches_any(&self, index: &alint_core::FileIndex) -> bool {
        self.inner.scope_matches_any(index)
    }
    fn path_contents_sensitive_by_default(&self) -> bool {
        self.inner.path_contents_sensitive_by_default()
    }
    fn visit_sensitive_paths(
        &self,
        explicit: bool,
        ctx: &Context<'_>,
        visit: &mut dyn FnMut(&Path),
    ) {
        self.inner.visit_sensitive_paths(explicit, ctx, visit);
    }
    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut seen = self.seen.lock().unwrap();
        seen.record_index(ctx);
        if let Some((plan, relations)) = self.inner.relation_inputs() {
            for &id in relations {
                let outcome = alint_core::relation::eval::evaluate(plan, id, ctx);
                seen.relations.insert(match outcome {
                    Ok(rows) => format!(
                        "{id:?}: {} rows from {:?}",
                        rows.rows.len(),
                        rows.resources
                            .iter()
                            .map(|resource| resource.path().to_string())
                            .collect::<Vec<_>>()
                    ),
                    Err(failures) => format!("{id:?}: {} failures", failures.len()),
                });
            }
        }
        Ok(Vec::new())
    }
    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        self.per_file
            .as_ref()
            .map(|probe| probe as &dyn PerFileRule)
    }
}

impl PerFileRule for PerFileProbe {
    fn path_scope(&self) -> &alint_core::Scope {
        self.inner.as_per_file().unwrap().path_scope()
    }
    fn evaluate_file(
        &self,
        ctx: &Context<'_>,
        path: &Path,
        _bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        let mut seen = self.seen.lock().unwrap();
        seen.record_index(ctx);
        seen.files.insert(path.to_path_buf());
        Ok(Vec::new())
    }
    fn max_bytes_needed(&self) -> Option<usize> {
        self.inner.as_per_file().unwrap().max_bytes_needed()
    }
}

/// The kinds the registry marks full-index, by canonical name.
fn full_index_kinds(registry: &alint_core::RuleRegistry) -> BTreeSet<String> {
    registry
        .kind_metadata()
        .filter(|(_, meta)| {
            matches!(
                meta.dispatch,
                DispatchClass::FullIndex { .. } | DispatchClass::PerFile { full_index: true }
            )
        })
        .map(|(kind, _)| registry.canonical_kind(kind).to_string())
        .collect()
}

/// One run of the scenario's full-index rules over `root`: the ids of the
/// rules that ran, and what each received.
fn run(
    root: &Path,
    rules: &[(String, Arc<dyn Rule>)],
    changed: Option<&Path>,
) -> (BTreeSet<String>, BTreeMap<String, Seen>) {
    let seen: Vec<Arc<Mutex<Seen>>> = rules.iter().map(|_| Arc::default()).collect();
    let entries = rules
        .iter()
        .zip(&seen)
        .map(|((_, rule), seen)| {
            RuleEntry::new(Box::new(Probe::new(Arc::clone(rule), Arc::clone(seen))))
        })
        .collect();
    let mut engine = Engine::from_entries(entries, alint_rules::builtin_registry());
    if let Some(changed) = changed {
        engine = engine.with_changed_paths(HashSet::from([changed.to_path_buf()]));
    }
    let index = walk(root, &support::walk_options()).unwrap();
    let report = engine.run(root, &index).unwrap();
    let ran = report
        .results
        .iter()
        .map(|result| result.rule_id.to_string())
        .collect();
    let seen = rules
        .iter()
        .zip(seen)
        .map(|((id, _), seen)| (id.clone(), seen.lock().unwrap().clone()))
        .collect();
    (ran, seen)
}

#[test]
fn every_full_index_kind_receives_the_full_index_under_changed() {
    let registry = alint_rules::builtin_registry();
    let kinds = full_index_kinds(&registry);
    assert!(kinds.contains("constraint"), "{kinds:?}");
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let roots = [
        manifest.join("scenarios"),
        manifest.join("../alint-dsl/ruleset-tests/v1"),
    ];
    // Per kind: the scenarios whose rule ran under `--changed`.
    let mut covered: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut failures = Vec::new();
    for (path, yaml) in roots
        .iter()
        .flat_map(|root| support::policy_test_sources(root))
    {
        let test = PolicyTest::from_yaml(&yaml)
            .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
        let (Some(config), Some(tree)) = (&test.given.config, &test.given.tree) else {
            continue;
        };
        if !kinds.iter().any(|kind| config.contains(kind.as_str())) {
            continue;
        }
        let tmp = tempfile::Builder::new()
            .prefix("alint-full-index-")
            .tempdir()
            .unwrap();
        let root = tmp.path();
        alint_testkit::materialize(tree, root).unwrap();
        std::fs::write(root.join(SENTINEL), "unchanged\n").unwrap();
        let config_path = root.join(".alint.yml");
        std::fs::write(&config_path, config).unwrap();
        // A scenario about a configuration error has no rules to probe.
        let Ok(config) = support::load(&config_path) else {
            continue;
        };
        let rules: Vec<(String, Arc<dyn Rule>)> = config
            .rules
            .iter()
            .filter_map(|spec| {
                let rule: Arc<dyn Rule> = Arc::from(registry.build(spec).ok()?);
                let kind = registry.canonical_kind(&spec.kind);
                // The registry's marks and the rule's own answer agree.
                assert_eq!(
                    rule.requires_full_index(),
                    kinds.contains(kind),
                    "{}: rule {} of kind {kind}",
                    path.display(),
                    spec.id
                );
                rule.requires_full_index().then(|| (spec.id.clone(), rule))
            })
            .collect();
        if rules.is_empty() {
            continue;
        }
        let kind_of: BTreeMap<&str, &str> = config
            .rules
            .iter()
            .map(|spec| (spec.id.as_str(), registry.canonical_kind(&spec.kind)))
            .collect();
        let (_, full) = run(root, &rules, None);
        let index = walk(root, &support::walk_options()).unwrap();
        let changes = index
            .files()
            .map(|entry| entry.path.to_path_buf())
            .filter(|path| path != Path::new(SENTINEL) && path != Path::new(".alint.yml"))
            .chain([PathBuf::from(DELETED)])
            .collect::<Vec<_>>();
        for changed in &changes {
            let (ran, seen) = run(root, &rules, Some(changed));
            for id in &ran {
                let kind = kind_of[id.as_str()];
                covered
                    .entry(kind.to_string())
                    .or_default()
                    .insert(path.display().to_string());
                if seen[id] != full[id] {
                    failures.push(format!(
                        "{}: rule {id} ({kind}) with {} changed received {:?}, \
                         where a full run received {:?}",
                        path.display(),
                        changed.display(),
                        seen[id],
                        full[id]
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let uncovered: Vec<&String> = kinds
        .iter()
        .filter(|kind| !covered.contains_key(*kind))
        .collect();
    assert!(
        uncovered.is_empty(),
        "no scenario runs these full-index kinds under --changed: {uncovered:?}"
    );
}
