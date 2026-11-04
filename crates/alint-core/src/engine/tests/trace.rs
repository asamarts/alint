use super::*;

static UNRELATED_TRACE_EVALUATIONS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[derive(Debug)]
struct UnrelatedTraceRule;

impl Rule for UnrelatedTraceRule {
    fn id(&self) -> &'static str {
        "unrelated"
    }

    fn level(&self) -> Level {
        Level::Error
    }

    fn evaluate(&self, _ctx: &Context<'_>) -> Result<Vec<Violation>> {
        UNRELATED_TRACE_EVALUATIONS.fetch_add(1, Ordering::SeqCst);
        Ok(vec![Violation::new("unrelated executed")])
    }
}

fn trace_spec(id: &str, level: &str, paths: &str, sensitive: bool) -> Arc<crate::RuleSpec> {
    Arc::new(
        ayaml::compat::from_str(&format!(
            "id: {id}\nkind: nested_scope_fixture\nlevel: {level}\npaths: \"{paths}\"\n\
                 sensitive: {sensitive}\n"
        ))
        .unwrap(),
    )
}

fn traced_entry(spec: Arc<crate::RuleSpec>, rule: Box<dyn Rule>) -> RuleEntry {
    RuleEntry::new(rule).with_spec(spec)
}

#[test]
fn selected_trace_uses_production_dispatch_and_never_runs_unrelated_rules() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"payload").unwrap();
    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
    let selected_spec = trace_spec("selected", "error", "**/*.txt", true);
    let unrelated_spec = trace_spec("unrelated", "error", "**/*", false);
    let engine = Engine::from_entries(
        vec![
            traced_entry(
                selected_spec.clone(),
                nested_leaf_builder(&selected_spec).unwrap(),
            ),
            traced_entry(unrelated_spec, Box::new(UnrelatedTraceRule)),
        ],
        nested_registry(),
    );

    UNRELATED_TRACE_EVALUATIONS.store(0, Ordering::SeqCst);
    let trace = engine.trace_rule("selected", tmp.path(), &index).unwrap();
    assert_eq!(UNRELATED_TRACE_EVALUATIONS.load(Ordering::SeqCst), 0);
    assert_eq!(trace.rule_id, "selected");
    assert_eq!(trace.kind, "nested_scope_fixture");
    assert_eq!(trace.outcome, TraceStatus::Violated);
    assert_eq!(
        trace
            .stages
            .iter()
            .map(|stage| stage.class)
            .collect::<Vec<_>>(),
        [
            TraceStageClass::Gate,
            TraceStageClass::Selection,
            TraceStageClass::Assertion,
            TraceStageClass::Outcome,
        ]
    );
    assert_eq!(
        trace
            .stages
            .iter()
            .map(|stage| stage.ordinal)
            .collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );

    let full = engine.run(tmp.path(), &index).unwrap();
    assert_eq!(UNRELATED_TRACE_EVALUATIONS.load(Ordering::SeqCst), 1);
    let expected = full
        .results
        .iter()
        .find(|result| &*result.rule_id == "selected")
        .unwrap();
    let actual = trace.result.as_ref().unwrap();
    assert_eq!(actual.rule_id, expected.rule_id);
    assert_eq!(actual.level, expected.level);
    assert_eq!(actual.policy_url, expected.policy_url);
    assert_eq!(actual.is_fixable, expected.is_fixable);
    assert_eq!(actual.violations.len(), expected.violations.len());
    assert_eq!(
        actual.analysis_failures.len(),
        expected.analysis_failures.len()
    );
    assert_eq!(actual.notes.len(), expected.notes.len());
    for (actual, expected) in actual.violations.iter().zip(&expected.violations) {
        assert_eq!(actual.path, expected.path);
        assert_eq!(actual.message, expected.message);
        assert_eq!(actual.range, expected.range);
        assert_eq!(actual.related_locations, expected.related_locations);
        assert_eq!(
            actual.related_locations_omitted,
            expected.related_locations_omitted
        );
        assert_eq!(actual.disposition, expected.disposition);
        assert_eq!(actual.sensitive, expected.sensitive);
        assert_eq!(actual.path_sensitive, expected.path_sensitive);
        assert_eq!(actual.is_fixable, expected.is_fixable);
        assert_eq!(actual.not_fixable, expected.not_fixable);
        assert_eq!(actual.proposed_edits, expected.proposed_edits);
    }
}

#[test]
fn per_file_trace_records_exact_dispatch_counts_and_terminal_outcome() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"wrong").unwrap();
    std::fs::write(tmp.path().join("ignore.md"), b"wrong").unwrap();
    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
    let spec = trace_spec("per-file", "error", "**/*.txt", false);
    let rule = Box::new(PerFileStub {
        id: "per-file".into(),
        scope: Scope::from_patterns(&["**/*.txt".to_string()]).unwrap(),
        prefix: b"MAGIC".to_vec(),
    });
    let engine = Engine::from_entries(vec![traced_entry(spec, rule)], nested_registry());

    let trace = engine.trace_rule("per-file", tmp.path(), &index).unwrap();
    let selection = trace
        .stages
        .iter()
        .find(|stage| stage.class == TraceStageClass::Selection)
        .unwrap();
    assert_eq!(selection.counts.considered, 2);
    assert_eq!(selection.counts.matched, 1);
    assert_eq!(trace.outcome, TraceStatus::Violated);
    assert_eq!(trace.result.as_ref().unwrap().level, Level::Error);
}

#[test]
fn trace_represents_common_gate_skips_and_failures() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"payload").unwrap();
    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();

    let off_spec = trace_spec("off", "off", "**/*.txt", false);
    let off = Engine::from_entries(
        vec![traced_entry(
            off_spec.clone(),
            nested_leaf_builder(&off_spec).unwrap(),
        )],
        nested_registry(),
    )
    .trace_rule("off", tmp.path(), &index)
    .unwrap();
    assert_eq!(off.outcome, TraceStatus::Skipped);
    assert!(off.result.is_none());
    assert_eq!(off.stages[0].reason, TraceReason::LevelOff);

    let false_spec = trace_spec("conditional", "error", "**/*.txt", false);
    let false_entry = traced_entry(
        false_spec.clone(),
        nested_leaf_builder(&false_spec).unwrap(),
    )
    .with_when(crate::when::parse("false").unwrap());
    let skipped = Engine::from_entries(vec![false_entry], nested_registry())
        .trace_rule("conditional", tmp.path(), &index)
        .unwrap();
    assert_eq!(skipped.outcome, TraceStatus::Skipped);
    assert!(
        skipped
            .stages
            .iter()
            .any(|stage| stage.reason == TraceReason::WhenFalse)
    );

    let error_spec = trace_spec("broken-condition", "error", "**/*.txt", false);
    let error_entry = traced_entry(
        error_spec.clone(),
        nested_leaf_builder(&error_spec).unwrap(),
    )
    .with_when(crate::when::parse("1 < \"x\"").unwrap());
    let failed = Engine::from_entries(vec![error_entry], nested_registry())
        .trace_rule("broken-condition", tmp.path(), &index)
        .unwrap();
    assert_eq!(failed.outcome, TraceStatus::AnalysisFailure);
    assert_eq!(
        failed
            .result
            .as_ref()
            .unwrap()
            .analysis_failures
            .first()
            .unwrap()
            .kind,
        AnalysisFailureKind::Condition
    );
}

#[test]
fn trace_represents_changed_expect_manifest_git_and_time_gates() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"payload").unwrap();
    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();

    let changed_spec = trace_spec("changed", "error", "**/*.txt", false);
    let changed_entry = || {
        traced_entry(
            changed_spec.clone(),
            nested_leaf_builder(&changed_spec).unwrap(),
        )
    };
    let empty_changed = Engine::from_entries(vec![changed_entry()], nested_registry())
        .with_changed_paths(HashSet::new())
        .trace_rule("changed", tmp.path(), &index)
        .unwrap();
    assert_eq!(empty_changed.outcome, TraceStatus::Skipped);
    assert!(
        empty_changed
            .stages
            .iter()
            .any(|stage| stage.reason == TraceReason::ChangedScopeEmpty)
    );

    let matched_changed = Engine::from_entries(vec![changed_entry()], nested_registry())
        .with_changed_paths(HashSet::from([PathBuf::from("a.txt")]))
        .trace_rule("changed", tmp.path(), &index)
        .unwrap();
    assert!(
        matched_changed
            .stages
            .iter()
            .any(|stage| stage.reason == TraceReason::ChangedScopeMatched)
    );

    let expect_spec: Arc<crate::RuleSpec> = Arc::new(
        ayaml::compat::from_str(
            "id: expect\nkind: nested_scope_fixture\nlevel: error\n\
                 paths: \"**/*.md\"\nexpect_matches: true\n",
        )
        .unwrap(),
    );
    let expected = Engine::from_entries(
        vec![traced_entry(
            expect_spec.clone(),
            nested_leaf_builder(&expect_spec).unwrap(),
        )],
        nested_registry(),
    )
    .with_changed_paths(HashSet::new())
    .trace_rule("expect", tmp.path(), &index)
    .unwrap();
    assert_eq!(expected.outcome, TraceStatus::Violated);
    assert!(
        expected
            .stages
            .iter()
            .any(|stage| stage.reason == TraceReason::ExpectMatchesViolated)
    );
    assert!(
        expected
            .stages
            .iter()
            .any(|stage| stage.reason == TraceReason::ChangedScopeEmpty)
    );

    std::fs::write(tmp.path().join("package.json"), br#"{"files":["a.txt"]}"#).unwrap();
    let manifest_index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
    let manifest_spec = trace_spec("manifest", "error", "**/*.txt", false);
    let manifest = Engine::from_entries(
        vec![traced_entry(
            manifest_spec.clone(),
            manifest_stub("manifest"),
        )],
        nested_registry(),
    )
    .trace_rule("manifest", tmp.path(), &manifest_index)
    .unwrap();
    assert!(
        manifest
            .stages
            .iter()
            .any(|stage| stage.reason == TraceReason::ManifestAvailable)
    );

    let owner_spec = trace_spec("owner", "error", "**/*", false);
    let nested_manifest = Engine::from_entries(
        vec![traced_entry(
            owner_spec,
            nested_parent(
                "kind: nested_scope_fixture\nwhen: iter.basename == \"a.txt\"\n\
                     paths: \"**/*.txt\"\n\
                     scope_filter:\n  include_manifest_paths:\n    source: package.json\n    \
                     extract: { json: \"$.files[*]\" }\n",
            ),
        )],
        nested_registry(),
    )
    .trace_rule("owner", tmp.path(), &manifest_index)
    .unwrap();
    assert!(
        nested_manifest
            .stages
            .iter()
            .any(|stage| stage.reason == TraceReason::ManifestAvailable)
    );

    std::fs::write(tmp.path().join("package.json"), b"{not json").unwrap();
    let invalid_manifest_index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
    let unavailable_manifest = Engine::from_entries(
        vec![traced_entry(manifest_spec, manifest_stub("manifest"))],
        nested_registry(),
    )
    .trace_rule("manifest", tmp.path(), &invalid_manifest_index)
    .unwrap();
    assert_eq!(unavailable_manifest.outcome, TraceStatus::AnalysisFailure);
    assert!(
        unavailable_manifest
            .stages
            .iter()
            .any(|stage| stage.reason == TraceReason::ManifestUnavailable)
    );

    let owner_spec = trace_spec("owner", "error", "**/*", false);
    let nested_git = Engine::from_entries(
        vec![traced_entry(
            owner_spec,
            nested_parent("kind: nested_git_fixture\nwhen: iter.basename == \"a.txt\"\n"),
        )],
        nested_registry(),
    )
    .trace_rule("owner", tmp.path(), &manifest_index)
    .unwrap();
    assert_eq!(nested_git.outcome, TraceStatus::AnalysisFailure);
    assert!(
        nested_git
            .stages
            .iter()
            .any(|stage| stage.reason == TraceReason::GitUnavailable)
    );
    let git_spec: Arc<crate::RuleSpec> = Arc::new(
        ayaml::compat::from_str(
            "id: git\nkind: nested_git_fixture\nlevel: error\npaths: \"**/*.txt\"\n",
        )
        .unwrap(),
    );
    let git = Engine::from_entries(
        vec![traced_entry(
            git_spec.clone(),
            nested_leaf_builder(&git_spec).unwrap(),
        )],
        nested_registry(),
    )
    .trace_rule("git", tmp.path(), &manifest_index)
    .unwrap();
    assert_eq!(git.outcome, TraceStatus::AnalysisFailure);
    assert!(
        git.stages
            .iter()
            .any(|stage| stage.reason == TraceReason::GitUnavailable)
    );

    let time_spec = trace_spec("time", "error", "**/*.txt", false);
    let time = Engine::from_entries(
        vec![traced_entry(time_spec, Box::new(TimeRule))],
        nested_registry(),
    )
    .trace_rule("time", tmp.path(), &manifest_index)
    .unwrap();
    assert_eq!(time.outcome, TraceStatus::AnalysisFailure);
    assert!(
        time.stages
            .iter()
            .any(|stage| stage.reason == TraceReason::AnalysisTimeUnavailable)
    );

    let skipped_time_spec = trace_spec("skipped-time", "error", "**/*.txt", false);
    let skipped_time_entry = traced_entry(skipped_time_spec, Box::new(TimeRule))
        .with_when(crate::when::parse("false").unwrap());
    let skipped_time = Engine::from_entries(vec![skipped_time_entry], nested_registry())
        .trace_rule("time", tmp.path(), &manifest_index)
        .unwrap();
    assert_eq!(skipped_time.outcome, TraceStatus::Skipped);
    assert!(!skipped_time.stages.iter().any(|stage| matches!(
        stage.reason,
        TraceReason::AnalysisTimeAvailable | TraceReason::AnalysisTimeUnavailable
    )));
}
#[test]
fn ordinary_run_never_constructs_a_trace_recorder() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"payload").unwrap();
    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
    let before = crate::trace::recorders_created_on_current_thread();
    Engine::new(vec![stub("ordinary", "**/*.txt")], RuleRegistry::new())
        .run(tmp.path(), &index)
        .unwrap();
    assert_eq!(
        crate::trace::recorders_created_on_current_thread(),
        before,
        "ordinary check must not construct trace state"
    );
}

#[test]
fn rule_producer_cannot_emit_engine_owned_trace_stages() {
    for class in [TraceStageClass::Gate, TraceStageClass::Outcome] {
        let trace = TraceSession::with_options(
            crate::TraceMeta::ASSERTION,
            TraceOptions::counts_only(),
            false,
            false,
        );
        let (name, reason) = match class {
            TraceStageClass::Gate => ("level", TraceReason::LevelEnabled),
            TraceStageClass::Outcome => ("outcome", TraceReason::OutcomeSatisfied),
            _ => unreachable!(),
        };
        <TraceSession as TraceSink>::record_kind(
            &trace,
            TraceStage::new(class, name, TraceStatus::Satisfied, reason),
        );
        let error = trace
            .finish("id".to_string(), "kind".to_string(), None)
            .unwrap_err();
        assert!(error.to_string().contains("engine-owned trace stage"));
    }
}
