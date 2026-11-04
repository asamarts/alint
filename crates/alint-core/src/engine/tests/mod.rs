use super::*;
use crate::scope::Scope;
use crate::walker::FileEntry;
use crate::{FixContext, FixOutcome, PerFileRule, level::Level};
use std::path::Path;

#[test]
fn worker_pool_current_thread_fallback_runs_par_iter_ordered() {
    // `with_worker_pool` returns the job's result unchanged across REPEATED calls
    // -- the two dispatch sites hit it in one run, so a per-call pool build (the
    // bug this replaced) would fail at the second site. (Common path: the cached
    // default pool, exercised by every engine test.) Do these first, before the
    // local `use_current_thread` pool below claims this thread.
    assert_eq!(with_worker_pool(|| 40 + 2), 42);
    assert_eq!(with_worker_pool(|| "ok".to_string()), "ok");

    // The fallback pool mechanism: a single-thread `use_current_thread` pool
    // spawns ZERO new threads (so it builds under the pressure that defeats the
    // default pool) and is ORDER-PRESERVING, so a degraded run matches a normal
    // one. It must also install REPEATEDLY (both sites reuse one cached pool).
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .use_current_thread()
        .build()
        .expect("a zero-new-thread pool must always build");
    let out: Vec<usize> = pool.install(|| (0..1000usize).into_par_iter().map(|x| x * 2).collect());
    assert_eq!(out, (0..1000usize).map(|x| x * 2).collect::<Vec<_>>());
    let again: Vec<usize> = pool.install(|| (0..10usize).into_par_iter().map(|x| x + 1).collect());
    assert_eq!(again, (1..=10usize).collect::<Vec<_>>());
    // Building a SECOND `use_current_thread` pool on this thread FAILS -- exactly
    // why `with_worker_pool` must cache one pool (build once, install many) rather
    // than build per call.
    assert!(
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .use_current_thread()
            .build()
            .is_err(),
        "a second use_current_thread build on one thread must fail (the cache requirement)"
    );
}

/// Stub rule: emits one violation per matched file in scope.
/// Configurable to advertise `requires_full_index` for
/// cross-file rule simulation, and a `path_scope` for
/// changed-mode tests.
#[derive(Debug)]
struct StubRule {
    id: String,
    level: Level,
    scope: Scope,
    full_index: bool,
    expose_scope: bool,
}

impl Rule for StubRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        self.level
    }
    fn requires_full_index(&self) -> bool {
        self.full_index
    }
    fn path_scope(&self) -> Option<&Scope> {
        self.expose_scope.then_some(&self.scope)
    }
    fn evaluate(&self, ctx: &Context<'_>) -> crate::error::Result<Vec<Violation>> {
        let mut out = Vec::new();
        for entry in ctx.index.files() {
            if self.scope.matches(&entry.path, ctx.index) {
                out.push(Violation::new("hit").with_path(entry.path.clone()));
            }
        }
        Ok(out)
    }
}

fn stub(id: &str, glob: &str) -> Box<dyn Rule> {
    Box::new(StubRule {
        id: id.into(),
        level: Level::Error,
        scope: Scope::from_patterns(&[glob.to_string()]).unwrap(),
        full_index: false,
        expose_scope: true,
    })
}

fn full_index_stub(id: &str) -> Box<dyn Rule> {
    Box::new(StubRule {
        id: id.into(),
        level: Level::Error,
        scope: Scope::match_all(),
        full_index: true,
        expose_scope: false,
    })
}
fn manifest_scope() -> Scope {
    let spec: crate::RuleSpec = ayaml::compat::from_str(
        "id: a\nkind: x\nlevel: error\npaths: \"**/*.txt\"\n\
             scope_filter:\n  include_manifest_paths:\n    source: package.json\n    \
             extract: { json: \"$.files[*]\" }\n",
    )
    .unwrap();
    Scope::from_spec(&spec).unwrap()
}

fn manifest_stub(id: &str) -> Box<dyn Rule> {
    Box::new(StubRule {
        id: id.into(),
        level: Level::Error,
        scope: manifest_scope(),
        full_index: false,
        expose_scope: true,
    })
}

#[test]
fn malformed_manifest_fails_every_owning_rule_once() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("package.json"), b"{not json").unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"payload").unwrap();
    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
    let report = Engine::new(
        vec![manifest_stub("first"), manifest_stub("second")],
        RuleRegistry::new(),
    )
    // The changed file matches `paths:`. The unavailable include set must
    // not make changed-mode dispatch silently skip either owning rule.
    .with_changed_paths(HashSet::from([PathBuf::from("a.txt")]))
    .run(tmp.path(), &index)
    .unwrap();

    assert_eq!(report.total_violations(), 0);
    assert_eq!(report.total_analysis_failures(), 2);
    assert_eq!(report.results.len(), 2);
    for result in &report.results {
        assert_eq!(result.analysis_failures.len(), 1);
        let failure = &result.analysis_failures[0];
        assert_eq!(failure.kind, AnalysisFailureKind::ResourceParse);
        assert_eq!(failure.path.as_deref(), Some(Path::new("package.json")));
        assert!(failure.message.contains("extract failed"), "{failure:?}");
    }

    // A rule whose ordinary path/filter gates exclude the invocation does
    // not report an unused dependency failure.
    let skipped = Engine::new(vec![manifest_stub("skipped")], RuleRegistry::new())
        .with_changed_paths(HashSet::from([PathBuf::from("README.md")]))
        .run(tmp.path(), &index)
        .unwrap();
    assert_eq!(skipped.total_analysis_failures(), 0);
    assert!(skipped.results.is_empty());
}

#[test]
fn false_when_does_not_resolve_manifest_scope() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("package.json"), b"{not json").unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"payload").unwrap();
    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
    let entry =
        RuleEntry::new(manifest_stub("inactive")).with_when(crate::when::parse("false").unwrap());

    let report = Engine::from_entries(vec![entry], RuleRegistry::new())
        .run(tmp.path(), &index)
        .unwrap();

    assert!(report.results.is_empty());
    assert!(
        index.manifest_failures_map().is_some_and(HashMap::is_empty),
        "an inactive rule must not read or cache its malformed manifest"
    );
}

#[derive(Debug)]
struct NestedDependencyParent {
    id: &'static str,
    nested: Vec<crate::CompiledNestedSpec>,
}

impl Rule for NestedDependencyParent {
    fn id(&self) -> &str {
        self.id
    }

    fn level(&self) -> Level {
        Level::Error
    }

    fn requires_full_index(&self) -> bool {
        true
    }

    fn nested_rule_specs(&self) -> &[crate::CompiledNestedSpec] {
        &self.nested
    }

    fn evaluate(&self, _ctx: &Context<'_>) -> Result<Vec<Violation>> {
        Ok(vec![Violation::new("parent evaluated")])
    }
}

fn nested_leaf_builder(spec: &crate::RuleSpec) -> Result<Box<dyn Rule>> {
    Ok(Box::new(StubRule {
        id: spec.id.clone(),
        level: spec.level,
        scope: Scope::from_spec(spec)?,
        full_index: false,
        expose_scope: true,
    }))
}

static TEST_GIT_CAPABILITIES: &[crate::CapabilityMeta] = &[crate::CapabilityMeta {
    capability: crate::Capability::Git {
        operations: std::borrow::Cow::Borrowed(&[crate::GitOperation::Diff]),
    },
    condition: crate::CapabilityCondition::Always,
}];

static TEST_NESTED_SCOPE_META: crate::KindMeta = crate::KindMeta {
    dispatch: crate::DispatchClass::RuleMajorScoped,
    paths: crate::PathsSupport::Optional,
    scope_filter: crate::ScopeFilterSupport::Full,
    git_tracked_only: crate::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: crate::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: crate::ContentAccess::PathOnly,
    formats: std::borrow::Cow::Borrowed(&[]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[]),
    expect_matches: true,
    nested_rules: false,
    trace: crate::TraceMeta::SELECTION,
};

static TEST_NESTED_GIT_META: crate::KindMeta = crate::KindMeta {
    dispatch: crate::DispatchClass::RuleMajorScoped,
    paths: crate::PathsSupport::Optional,
    scope_filter: crate::ScopeFilterSupport::Full,
    git_tracked_only: crate::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: crate::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(TEST_GIT_CAPABILITIES),
    content: crate::ContentAccess::PathOnly,
    formats: std::borrow::Cow::Borrowed(&[]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[]),
    expect_matches: true,
    nested_rules: false,
    trace: crate::TraceMeta::SELECTION,
};

fn nested_registry() -> RuleRegistry {
    let mut registry = RuleRegistry::new();
    registry.register(
        "nested_scope_fixture",
        &TEST_NESTED_SCOPE_META,
        nested_leaf_builder,
    );
    registry.register(
        "nested_git_fixture",
        &TEST_NESTED_GIT_META,
        nested_leaf_builder,
    );
    registry
}

fn nested_parent(yaml: &str) -> Box<dyn Rule> {
    let spec: crate::NestedRuleSpec = ayaml::compat::from_str(yaml).unwrap();
    let compiled = crate::CompiledNestedSpec::compile(spec, "owner", 0).unwrap();
    Box::new(NestedDependencyParent {
        id: "owner",
        nested: vec![compiled],
    })
}

#[test]
fn inactive_nested_git_and_changed_since_do_not_probe_git() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"payload").unwrap();

    for yaml in [
        "kind: nested_git_fixture\nwhen: false\n",
        "kind: nested_git_fixture\nwhen: iter.basename == \"never\"\n",
        "kind: nested_scope_fixture\nwhen: false\nscope_filter:\n  changed_since: definitely-not-a-ref\n",
        "kind: nested_scope_fixture\nwhen: iter.basename == \"never\"\nscope_filter:\n  changed_since: definitely-not-a-ref\n",
    ] {
        let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
        let report = Engine::new(vec![nested_parent(yaml)], nested_registry())
            .run(tmp.path(), &index)
            .unwrap();
        assert_eq!(report.total_analysis_failures(), 0);
        assert_eq!(report.total_violations(), 1);
        assert_eq!(report.results[0].rule_id.as_ref(), "owner");
    }
}

#[test]
fn active_nested_git_failure_is_owned_once_and_other_rules_continue() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"payload").unwrap();
    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
    let report = Engine::new(
        vec![
            nested_parent("kind: nested_git_fixture\nwhen: iter.basename == \"a.txt\"\n"),
            stub("unrelated", "**/*.txt"),
        ],
        nested_registry(),
    )
    .run(tmp.path(), &index)
    .unwrap();

    assert_eq!(report.total_analysis_failures(), 1);
    assert_eq!(report.results.len(), 2);
    let owner = report
        .results
        .iter()
        .find(|result| result.rule_id.as_ref() == "owner")
        .unwrap();
    assert!(owner.violations.is_empty());
    assert_eq!(owner.analysis_failures.len(), 1);
    assert_eq!(
        owner.analysis_failures[0].kind,
        AnalysisFailureKind::ResourceRead
    );
    let unrelated = report
        .results
        .iter()
        .find(|result| result.rule_id.as_ref() == "unrelated")
        .unwrap();
    assert_eq!(unrelated.violations.len(), 1);
}

#[test]
fn inactive_nested_manifest_is_never_resolved() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"payload").unwrap();
    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
    let report = Engine::new(
        vec![nested_parent(
            "kind: nested_scope_fixture\nwhen: iter.basename == \"never\"\n\
                 paths: \"**/*.txt\"\n\
                 scope_filter:\n  include_manifest_paths:\n    source: missing.json\n    \
                 extract: { json: \"$.files[*]\" }\n",
        )],
        nested_registry(),
    )
    .run(tmp.path(), &index)
    .unwrap();

    assert_eq!(report.total_analysis_failures(), 0);
    assert_eq!(report.total_violations(), 1);
    assert!(
        index.manifest_failures_map().is_some_and(HashMap::is_empty),
        "a false-gated nested rule must not touch its manifest"
    );
}

#[test]
fn active_nested_manifest_failure_is_owned_once_and_other_rules_continue() {
    for (contents, expected_kind) in [
        (None, AnalysisFailureKind::ResourceRead),
        (Some("{not json"), AnalysisFailureKind::ResourceParse),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), b"payload").unwrap();
        if let Some(contents) = contents {
            std::fs::write(tmp.path().join("package.json"), contents).unwrap();
        }
        let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
        let report = Engine::new(
            vec![
                nested_parent(
                    "kind: nested_scope_fixture\nwhen: iter.basename == \"a.txt\"\n\
                         paths: \"**/*.txt\"\n\
                         scope_filter:\n  include_manifest_paths:\n    source: package.json\n    \
                         extract: { json: \"$.files[*]\" }\n  exclude_manifest_paths:\n    \
                         source: package.json\n    extract: { json: \"$.files[*]\" }\n",
                ),
                stub("unrelated", "**/*.txt"),
            ],
            nested_registry(),
        )
        .run(tmp.path(), &index)
        .unwrap();
        assert_eq!(
            index.manifest_failures_map().map(HashMap::len),
            Some(1),
            "shared include/exclude predicates must cache one resolution"
        );

        assert_eq!(report.total_analysis_failures(), 1);
        assert_eq!(report.results.len(), 2);
        let owner = report
            .results
            .iter()
            .find(|result| result.rule_id.as_ref() == "owner")
            .unwrap();
        assert!(owner.violations.is_empty());
        assert_eq!(owner.analysis_failures.len(), 1);
        assert_eq!(owner.analysis_failures[0].kind, expected_kind);
        assert_eq!(
            owner.analysis_failures[0].path.as_deref(),
            Some(Path::new("package.json"))
        );
        let unrelated = report
            .results
            .iter()
            .find(|result| result.rule_id.as_ref() == "unrelated")
            .unwrap();
        assert_eq!(unrelated.violations.len(), 1);
    }
}

#[test]
fn active_nested_manifest_failure_stays_owned_in_fix_mode() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"payload").unwrap();
    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
    let report = Engine::new(
        vec![nested_parent(
            "kind: nested_scope_fixture\npaths: \"**/*.txt\"\n\
                 scope_filter:\n  include_manifest_paths:\n    source: missing.json\n    \
                 extract: { json: \"$.files[*]\" }\n",
        )],
        nested_registry(),
    )
    .fix(
        tmp.path(),
        &index,
        &crate::WalkOptions::default(),
        true,
        Applicability::Safe,
    )
    .unwrap();

    assert_eq!(report.total_analysis_failures(), 1);
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].rule_id.as_ref(), "owner");
    assert_eq!(report.results[0].analysis_failures.len(), 1);
    assert_eq!(
        report.results[0].analysis_failures[0].kind,
        AnalysisFailureKind::ResourceRead
    );
}

#[derive(Debug)]
struct TimeRule;

impl Rule for TimeRule {
    fn id(&self) -> &'static str {
        "time"
    }

    fn level(&self) -> Level {
        Level::Error
    }

    fn wants_analysis_time(&self) -> bool {
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> crate::error::Result<Vec<Violation>> {
        Ok(vec![Violation::new(format!(
            "reference {}",
            ctx.analysis_time()?
        ))])
    }
}

#[derive(Debug)]
struct FailingPerFileRule {
    scope: Scope,
}

#[derive(Debug)]
struct MustNotRunFixer;

static FIXER_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static MUST_NOT_RUN_FIXER: MustNotRunFixer = MustNotRunFixer;

impl Fixer for MustNotRunFixer {
    fn describe(&self) -> String {
        "must not run".to_string()
    }

    fn apply(&self, _violation: &Violation, _ctx: &FixContext<'_>) -> Result<FixOutcome> {
        FIXER_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(FixOutcome::Applied("unexpected".to_string()))
    }
}

impl Rule for FailingPerFileRule {
    fn id(&self) -> &'static str {
        "failing-per-file"
    }

    fn level(&self) -> Level {
        Level::Warning
    }

    fn evaluate(&self, _ctx: &Context<'_>) -> Result<Vec<Violation>> {
        Err(Error::Other("rule-major boom".to_string()))
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        Some(&MUST_NOT_RUN_FIXER)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for FailingPerFileRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        _path: &Path,
        _bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        Err(Error::Other("file boom".to_string()))
    }
}

fn failing_per_file_rule() -> Box<dyn Rule> {
    Box::new(FailingPerFileRule {
        scope: Scope::from_patterns(&["*.rs".to_string()]).unwrap(),
    })
}

#[test]
fn per_file_single_file_and_fix_failures_are_structural_and_path_preserving() {
    FIXER_CALLS.store(0, std::sync::atomic::Ordering::SeqCst);
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.rs"), "fn main() {}\n").unwrap();
    let index = idx(&["a.rs"]);
    let walk_opts = crate::WalkOptions {
        respect_gitignore: true,
        extra_ignores: Vec::new(),
    };

    let engine = Engine::new(vec![failing_per_file_rule()], RuleRegistry::new());
    let report = engine.run(tmp.path(), &index).unwrap();
    assert!(
        report.has_errors(),
        "warning-level failure must count as error"
    );
    assert_eq!(report.total_violations(), 0);
    let failure = &report.results[0].analysis_failures[0];
    assert_eq!(failure.kind, AnalysisFailureKind::Evaluation);
    assert_eq!(failure.path.as_deref(), Some(Path::new("a.rs")));
    assert_eq!(failure.message, "rule evaluation failed: file boom");

    let single = engine
        .run_for_file(tmp.path(), &index, Path::new("a.rs"), b"fn main() {}\n")
        .unwrap();
    assert_eq!(single.len(), 1);
    assert!(single[0].violations.is_empty());
    assert_eq!(
        single[0].analysis_failures[0].path.as_deref(),
        Some(Path::new("a.rs")),
    );

    let fix = engine
        .fix(tmp.path(), &index, &walk_opts, true, Applicability::Safe)
        .unwrap();
    assert!(fix.results.iter().all(|result| result.items.is_empty()));
    assert_eq!(fix.total_analysis_failures(), 1);
    assert_eq!(
        fix.results[0].analysis_failures[0].message,
        "rule evaluation failed: rule-major boom",
    );
    assert!(fix.has_unfixable_errors());
    assert_eq!(FIXER_CALLS.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn analysis_time_resolution_is_lazy_and_explicit_time_works_outside_git() {
    let tmp = tempfile::tempdir().unwrap();
    let ordinary = Engine::new(Vec::new(), RuleRegistry::new());
    assert!(
        ordinary
            .resolve_analysis_time_if_needed(tmp.path())
            .is_none()
    );

    let explicit: crate::AnalysisTime = "2024-02-29".parse().unwrap();
    let engine =
        Engine::new(vec![Box::new(TimeRule)], RuleRegistry::new()).with_analysis_time(explicit);
    assert_eq!(
        engine.resolve_analysis_time_if_needed(tmp.path()),
        Some(crate::AnalysisTimeResolution::Available {
            time: explicit,
            source: crate::AnalysisTimeSource::Explicit,
        })
    );
    let report = engine.run(tmp.path(), &idx(&[])).unwrap();
    assert_eq!(
        report.results[0].violations[0].message,
        "reference 2024-02-29"
    );
}
#[test]
fn head_analysis_time_is_cached_for_the_engine_lifetime() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let run_git = |args: &[&str], date: Option<&str>| {
        let mut command = std::process::Command::new("git");
        command.arg("-C").arg(root).args(args);
        if let Some(date) = date {
            command
                .env("GIT_AUTHOR_DATE", date)
                .env("GIT_COMMITTER_DATE", date);
        }
        assert!(command.status().unwrap().success(), "git {args:?}");
    };
    run_git(&["init", "-q"], None);
    std::fs::write(root.join("tracked"), "one\n").unwrap();
    run_git(&["add", "tracked"], None);
    run_git(
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "one",
        ],
        Some("2024-01-02T03:04:05Z"),
    );

    let engine = Engine::new(vec![Box::new(TimeRule)], RuleRegistry::new());
    let first = engine.resolve_analysis_time_if_needed(root).unwrap();
    assert_eq!(first.time().unwrap().to_string(), "2024-01-02",);

    std::fs::write(root.join("tracked"), "two\n").unwrap();
    run_git(&["add", "tracked"], None);
    run_git(
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "two",
        ],
        Some("2025-06-07T08:09:10Z"),
    );
    assert_eq!(engine.resolve_analysis_time_if_needed(root), Some(first));
    assert_eq!(
        engine.resolve_analysis_time_if_needed(&root.join(".")),
        Some(first),
        "a lexical alias of the same root must reuse the first resolution"
    );

    let other = tempfile::tempdir().unwrap();
    assert_eq!(
        engine.resolve_analysis_time_if_needed(other.path()),
        Some(crate::AnalysisTimeResolution::Unavailable(
            crate::AnalysisTimeUnavailable::EngineRootChanged
        )),
        "one engine must not silently reuse or replace another root's time"
    );
}

#[test]
fn empty_changed_fix_and_diff_do_not_resolve_analysis_time() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::new(vec![Box::new(TimeRule)], RuleRegistry::new())
        .with_changed_paths(HashSet::new());
    let index = idx(&[]);
    let walk_opts = crate::WalkOptions {
        respect_gitignore: true,
        extra_ignores: Vec::new(),
    };

    let report = engine
        .fix(tmp.path(), &index, &walk_opts, false, Applicability::Safe)
        .unwrap();
    assert!(report.results.is_empty());
    assert!(engine.analysis_time_cache.lock().is_none());

    let (report, staged) = engine
        .stage_fixes(tmp.path(), &index, Applicability::Safe)
        .unwrap();
    assert!(report.results.is_empty());
    assert!(staged.is_empty());
    assert!(engine.analysis_time_cache.lock().is_none());
}

#[test]
fn unavailable_head_time_surfaces_as_an_analysis_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::new(vec![Box::new(TimeRule)], RuleRegistry::new());
    let report = engine.run(tmp.path(), &idx(&[])).unwrap();
    let result = &report.results[0];
    assert!(result.violations.is_empty());
    assert_eq!(result.analysis_failures.len(), 1);
    let failure = &result.analysis_failures[0];
    assert_eq!(failure.kind, AnalysisFailureKind::Evaluation);
    assert!(
        failure
            .message
            .starts_with("rule evaluation failed: analysis time unavailable: ")
    );
    assert!(failure.message.contains("pass --as-of YYYY-MM-DD"));
    assert!(report.has_errors());
}

fn idx(paths: &[&str]) -> FileIndex {
    FileIndex::from_entries(
        paths
            .iter()
            .map(|p| FileEntry {
                path: std::path::Path::new(p).into(),
                is_dir: false,
                size: 0,
            })
            .collect(),
    )
}

/// Per-file rule that emits one violation per file based on
/// the byte content prefix. Used to verify the file-major
/// dispatch path actually hands the bytes to the rule and
/// aggregates the violations correctly.
#[derive(Debug)]
struct PerFileStub {
    id: String,
    scope: Scope,
    prefix: Vec<u8>,
}

impl Rule for PerFileStub {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        Level::Error
    }
    fn evaluate(&self, _ctx: &Context<'_>) -> crate::error::Result<Vec<Violation>> {
        // Rule-major fallback: not exercised when
        // `as_per_file` is set + the engine routes to the
        // file-major loop.
        Ok(Vec::new())
    }
    fn as_per_file(&self) -> Option<&dyn crate::PerFileRule> {
        Some(self)
    }
}
impl crate::PerFileRule for PerFileStub {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }
    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &std::path::Path,
        bytes: &[u8],
    ) -> crate::error::Result<Vec<Violation>> {
        if !bytes.starts_with(&self.prefix) {
            return Ok(vec![
                Violation::new("missing prefix")
                    .with_path(std::sync::Arc::<std::path::Path>::from(path)),
            ]);
        }
        Ok(Vec::new())
    }
}
mod fix;
mod run;
mod trace;
