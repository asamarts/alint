//! Constitution invariant 2: under `--changed`, a rule that requires the full
//! index receives the full index whenever it runs. A path scope it declares
//! (for `constraint`, its trigger scope) decides only whether it runs, never
//! what it sees (ADR-0034 decision 3, ADR-0037).
//!
//! The test takes every kind the built-in registry marks full-index (a
//! `FullIndex` dispatch, or a per-file dispatch with `full_index`), with no
//! list of its own, and every scenario of the policy-test corpus (the e2e
//! scenarios and the bundled rulesets' tests) that configures one inline,
//! over an inline tree, in a config that loads. It wraps each such rule in a
//! [`Probe`] that records what the engine hands it and evaluates nothing (so
//! no rule spawns a process here), adds an unchanged file to the tree, and runs
//! the engine once in full and then under `--changed` with each file of the
//! tree changed alone, and with a deleted path. A scenario with a rule that has
//! a trigger scope (`constraint`) also runs each file changed alone under
//! `fix`, each directory changed alone (as Git lists a submodule), and each
//! file deleted and renamed for real, against a full run over the tree so
//! changed. Whenever the rule runs under `--changed`, what it received must
//! equal what it received in the full run:
//! the same index, for a per-file rule the same files, and for a rule that
//! reads relations the same rows, cell by cell. Every full-index kind must run
//! under `--changed` in at least one scenario, so a new kind without a
//! scenario fails here instead of passing unchecked.

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
/// relations, each relation as it evaluates: its fields, every row's cells
/// and where each came from, the resources it read and its notes, or its
/// failures.
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
                    Ok(rows) => format!("{id:?}: {rows:?}"),
                    Err(failures) => format!("{id:?}: {failures:?}"),
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

/// How a run goes: `check`, or `fix` (which writes nothing here, the probes
/// reporting no finding).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verb {
    Check,
    Fix,
}

/// One run of the scenario's full-index rules over `root`, with `changed`
/// as the changed set, or in full: the ids of the rules that ran, and what
/// each received.
fn run(
    root: &Path,
    rules: &[(String, Arc<dyn Rule>)],
    changed: Option<&[PathBuf]>,
    verb: Verb,
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
    let index = walk(root, &support::walk_options()).unwrap();
    if let Some(changed) = changed {
        // A directory changes as Git lists a submodule or an untracked
        // repository: by its path, marked as a directory.
        let directories = changed
            .iter()
            .filter(|path| index.dirs().any(|dir| *dir.path == **path))
            .cloned()
            .collect();
        engine = engine.with_changed_set(alint_core::git::ChangedSet::new(
            changed.iter().cloned().collect::<HashSet<_>>(),
            directories,
        ));
    }
    let ran = match verb {
        Verb::Check => engine
            .run(root, &index)
            .unwrap()
            .results
            .iter()
            .map(|result| result.rule_id.to_string())
            .collect(),
        // A fix report leaves out a rule with nothing to fix: one ran when
        // its probe was handed an index.
        Verb::Fix => {
            engine
                .fix(
                    root,
                    &index,
                    &support::walk_options(),
                    false,
                    alint_core::Applicability::Safe,
                )
                .unwrap();
            rules
                .iter()
                .zip(&seen)
                .filter(|(_, seen)| !seen.lock().unwrap().indexes.is_empty())
                .map(|((id, _), _)| id.clone())
                .collect()
        }
    };
    let seen = rules
        .iter()
        .zip(seen)
        .map(|((id, _), seen)| (id.clone(), seen.lock().unwrap().clone()))
        .collect();
    (ran, seen)
}

/// The scenario's full-index rules, with each rule's kind.
struct Scenario<'s> {
    path: &'s Path,
    root: &'s Path,
    rules: &'s [(String, Arc<dyn Rule>)],
    kind_of: &'s BTreeMap<&'s str, &'s str>,
}

impl Scenario<'_> {
    /// Run `verb` with `changed` as the changed set over the tree as it is,
    /// and compare what each rule that ran received with what it received
    /// in `full`, a full run of `verb` over the same tree. Returns the ids
    /// of the rules that ran.
    fn compare(
        &self,
        changed: &[PathBuf],
        verb: Verb,
        full: &BTreeMap<String, Seen>,
        shape: &str,
        failures: &mut Vec<String>,
    ) -> BTreeSet<String> {
        let (ran, seen) = run(self.root, self.rules, Some(changed), verb);
        for id in &ran {
            if seen[id] != full[id] {
                failures.push(format!(
                    "{}: rule {id} ({}) under {verb:?} with {changed:?} {shape} received \
                     {:?}, where a full run received {:?}",
                    self.path.display(),
                    self.kind_of[id.as_str()],
                    seen[id],
                    full[id]
                ));
            }
        }
        ran
    }

    /// The shapes of change besides one file changed under `check`: each
    /// of `files` changed alone under `fix`, each directory of `index`
    /// changed alone under `check`, and each of `files` deleted, and renamed
    /// out of its name (to `<name>.moved`, a change to both its paths). A
    /// change no rule runs on needs no full run to compare with.
    fn other_shapes(
        &self,
        index: &alint_core::FileIndex,
        files: &[PathBuf],
        full: &BTreeMap<String, Seen>,
        failures: &mut Vec<String>,
    ) {
        let (_, full_fix) = run(self.root, self.rules, None, Verb::Fix);
        for file in files {
            self.compare(
                std::slice::from_ref(file),
                Verb::Fix,
                &full_fix,
                "changed",
                failures,
            );
        }
        for dir in index.dirs() {
            let dir = [dir.path.to_path_buf()];
            self.compare(&dir, Verb::Check, full, "changed", failures);
        }
        let aside = tempfile::Builder::new()
            .prefix("alint-full-index-aside-")
            .tempdir()
            .unwrap();
        let kept = aside.path().join("file");
        for file in files {
            std::fs::rename(self.root.join(file), &kept).unwrap();
            self.compare_with_full_run(std::slice::from_ref(file), "deleted", failures);
            std::fs::rename(&kept, self.root.join(file)).unwrap();

            let mut moved = file.clone().into_os_string();
            moved.push(".moved");
            let moved = PathBuf::from(moved);
            self.rename(file, &moved);
            self.compare_with_full_run(&[file.clone(), moved.clone()], "renamed", failures);
            self.rename(&moved, file);
        }
    }

    /// [`Self::compare`] under `check` with a full run over the tree as it
    /// is now, made only when a rule ran.
    fn compare_with_full_run(&self, changed: &[PathBuf], shape: &str, failures: &mut Vec<String>) {
        let (ran, seen) = run(self.root, self.rules, Some(changed), Verb::Check);
        if ran.is_empty() {
            return;
        }
        let (_, full) = run(self.root, self.rules, None, Verb::Check);
        for id in &ran {
            if seen[id] != full[id] {
                failures.push(format!(
                    "{}: rule {id} ({}) with {changed:?} {shape} received {:?}, where a full \
                     run received {:?}",
                    self.path.display(),
                    self.kind_of[id.as_str()],
                    seen[id],
                    full[id]
                ));
            }
        }
    }

    /// Move `from` to `to` (both root-relative), creating `to`'s directory.
    fn rename(&self, from: &Path, to: &Path) {
        let to = self.root.join(to);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::rename(self.root.join(from), to).unwrap();
    }
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
        let scenario = Scenario {
            path: &path,
            root,
            rules: &rules,
            kind_of: &kind_of,
        };
        let (_, full) = run(root, &rules, None, Verb::Check);
        let index = walk(root, &support::walk_options()).unwrap();
        let files: Vec<PathBuf> = index
            .files()
            .map(|entry| entry.path.to_path_buf())
            .filter(|path| path != Path::new(SENTINEL) && path != Path::new(".alint.yml"))
            .collect();
        // Each file changed alone, and a path no tree holds, deleted.
        for changed in files.iter().cloned().chain([PathBuf::from(DELETED)]) {
            let ran = scenario.compare(&[changed], Verb::Check, &full, "changed", &mut failures);
            for id in &ran {
                covered
                    .entry(kind_of[id.as_str()].to_string())
                    .or_default()
                    .insert(path.display().to_string());
            }
        }
        // The shapes of change a trigger scope judges (`constraint`), for a
        // scenario with one: each file changed alone under `fix`, each
        // directory changed alone (as Git lists a submodule or an untracked
        // repository), and each file deleted and renamed for real, compared
        // with a full run over the tree so changed.
        if rules.iter().any(|(_, rule)| rule.trigger_scope().is_some()) {
            scenario.other_shapes(&index, &files, &full, &mut failures);
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
