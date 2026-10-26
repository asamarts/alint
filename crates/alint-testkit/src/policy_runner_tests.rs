use super::matching::{match_counted, violation_matches};
use super::*;
use crate::policy_test::ViolationMatcher;

#[test]
fn observations_use_public_paths_without_changing_fingerprint_inputs() {
    let repository = tempfile::tempdir().unwrap();
    let secret_path = Path::new("secret-selector.txt");
    std::fs::write(repository.path().join(secret_path), "bad\n").unwrap();
    let mut violation = alint_core::Violation::new("redacted")
        .with_path(secret_path)
        .with_location(2, 3);
    violation.path_sensitive = true;
    let mut note = alint_core::Violation::new("redacted")
        .with_path(secret_path)
        .with_location(4, 5)
        .as_note();
    note.path_sensitive = true;
    let mut failure = alint_core::Violation::new("redacted")
        .with_path(secret_path)
        .with_location(6, 7)
        .as_analysis_failure(alint_core::AnalysisFailureKind::Evaluation);
    failure.path_sensitive = true;
    let report = Report {
        results: vec![alint_core::RuleResult::new(
            "protected".into(),
            Level::Error,
            None,
            vec![violation, note, failure],
            false,
        )],
    };

    let observation = observe_check(report, repository.path());

    assert_eq!(
        observation.violations[0].path.as_deref(),
        Some("<redacted>")
    );
    assert_eq!(observation.violations[0].line, None);
    assert_ne!(observation.violations[0].fingerprint, "");
    assert_eq!(observation.notes[0].path.as_deref(), Some("<redacted>"));
    assert_eq!(observation.notes[0].line, None);
    assert_eq!(
        observation.analysis_failures[0].path.as_deref(),
        Some("<redacted>")
    );
    assert_eq!(observation.analysis_failures[0].line, None);
}

#[test]
fn maximum_matching_handles_overlapping_matchers() {
    let expected = vec![
        ViolationMatcher {
            rule: "r".to_string(),
            level: None,
            path: None,
            line: None,
            column: None,
            end_line: None,
            end_column: None,
            message: None,
            count: 1,
            related: None,
        },
        ViolationMatcher {
            rule: "r".to_string(),
            level: None,
            path: Some("a".to_string()),
            line: None,
            column: None,
            end_line: None,
            end_column: None,
            message: None,
            count: 1,
            related: None,
        },
    ];
    let actual = vec![
        ObservedViolation {
            rule: "r".to_string(),
            level: "error".to_string(),
            path: Some("a".to_string()),
            line: None,
            column: None,
            end_line: None,
            end_column: None,
            message: "first".to_string(),
            is_fixable: false,
            baseline_key: None,
            fingerprint: String::new(),
            related: Vec::new(),
        },
        ObservedViolation {
            rule: "r".to_string(),
            level: "error".to_string(),
            path: Some("b".to_string()),
            line: None,
            column: None,
            end_line: None,
            end_column: None,
            message: "second".to_string(),
            is_fixable: false,
            baseline_key: None,
            fingerprint: String::new(),
            related: Vec::new(),
        },
    ];
    let mut mismatches = Vec::new();
    let regexes = HashMap::new();
    match_counted(
        &expected,
        &actual,
        |matcher| matcher.count,
        |matcher, observation| violation_matches(matcher, observation, &regexes),
        0,
        MismatchChannel::Violations,
        &mut mismatches,
    );
    assert!(mismatches.is_empty(), "{mismatches:?}");
}

#[test]
fn counted_matcher_rejects_impossible_total_without_expansion() {
    let expected = [usize::MAX];
    let actual: [(); 0] = [];
    let mut mismatches = Vec::new();
    match_counted(
        &expected,
        &actual,
        |count| *count,
        |_, ()| true,
        0,
        MismatchChannel::Violations,
        &mut mismatches,
    );
    assert_eq!(mismatches.len(), 1);
    assert!(
        mismatches[0].detail.contains("expected count"),
        "{mismatches:?}"
    );
}

#[test]
fn counted_matcher_rejects_overflowed_total() {
    let expected = [usize::MAX, 1];
    let actual: [(); 0] = [];
    let mut mismatches = Vec::new();
    match_counted(
        &expected,
        &actual,
        |count| *count,
        |_, ()| true,
        0,
        MismatchChannel::Violations,
        &mut mismatches,
    );
    assert_eq!(mismatches.len(), 1);
    assert!(
        mismatches[0].detail.contains("platform limit"),
        "{mismatches:?}"
    );
}

#[cfg(target_os = "linux")]
fn policy_scenario_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("yml" | "yaml")
            ) {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths
}

// Complete trace coverage is a Linux-canonical audit: explicit platform tags skip
// unique pass/fail fixtures on other targets.
#[cfg(target_os = "linux")]
#[test]
#[allow(clippy::too_many_lines)]
fn canonical_pass_fail_corpus_produces_complete_traces() {
    let scenarios = Path::new(env!("CARGO_MANIFEST_DIR")).join("../alint-e2e/scenarios");
    let runner = Runner::new(RunnerOptions {
        allow_spawn: true,
        allow_host_keyring: true,
        update_snapshots: false,
    });
    // The published crate intentionally excludes the workspace's e2e corpus.
    if !scenarios.is_dir() {
        return;
    }
    let registry = alint_rules::builtin_registry();
    let mut outcomes: BTreeMap<String, (bool, bool)> = registry
        .canonical_kinds()
        .map(|kind| (kind.to_string(), (false, false)))
        .collect();

    for source in policy_scenario_paths(&scenarios) {
        if outcomes
            .values()
            .all(|(passing, failing)| *passing && *failing)
        {
            break;
        }
        let text = std::fs::read_to_string(&source).unwrap();
        let document: yaml_serde::Value = match yaml_serde::from_str(&text) {
            Ok(document) => document,
            Err(_) => continue,
        };
        if document.get("test_version").is_none() {
            continue;
        }
        let policy = PolicyTest::from_yaml(&text)
            .unwrap_or_else(|error| panic!("{}: {error}", source.display()));
        if let Err(diagnostics) = policy.validate() {
            panic!("{}: {diagnostics:?}", source.display());
        }
        if platform_skip_reason(&policy.tags).is_some() {
            continue;
        }
        let prepared = runner
            .prepare_policy_test(&policy, &source)
            .unwrap_or_else(|error| panic!("{}: {error}", source.display()));
        let context = ExecutionContext {
            repository: &prepared.repository,
            sandbox: prepared.sandbox.path(),
            config_path: &prepared.config_path,
            source_provider: prepared.source_provider.clone(),
            environment: prepared.environment.clone(),
            process_environment: &prepared.process_environment,
            test: &policy,
            package: &prepared.package,
        };
        let PreparedStep::Ready { engine, index, .. } = runner
            .prepare_step(TestStep::Check, &context)
            .unwrap_or_else(|error| panic!("{}: {error}", source.display()))
        else {
            continue;
        };
        let rules: Vec<(String, String)> = engine
            .trace_rule_inventory()
            .map(|(id, kind)| (id.to_string(), kind.to_string()))
            .collect();
        for (id, kind) in rules {
            let Some((passing, failing)) = outcomes.get(&kind).copied() else {
                panic!("{}: unregistered kind `{kind}`", source.display());
            };
            if passing && failing {
                continue;
            }
            let trace = engine
                .trace_rule(&id, &prepared.repository, &index)
                .unwrap_or_else(|error| {
                    panic!(
                        "{}: trace `{id}` ({kind}) failed: {error}",
                        source.display()
                    )
                });
            assert_eq!(trace.kind, kind, "{}", source.display());
            assert_eq!(
                trace
                    .stages
                    .iter()
                    .filter(|stage| stage.class == alint_core::TraceStageClass::Outcome)
                    .count(),
                1,
                "{}: trace `{id}` must have one terminal outcome",
                source.display()
            );
            assert!(
                trace
                    .stages
                    .iter()
                    .enumerate()
                    .all(|(index, stage)| stage.ordinal == u32::try_from(index).unwrap()),
                "{}: trace `{id}` ordinals are not contiguous",
                source.display()
            );
            let status = outcomes.get_mut(&kind).unwrap();
            match trace.outcome {
                alint_core::TraceStatus::Satisfied => status.0 = true,
                alint_core::TraceStatus::Violated => status.1 = true,
                alint_core::TraceStatus::Skipped
                | alint_core::TraceStatus::AnalysisFailure
                | alint_core::TraceStatus::NotEvaluated => {}
            }
        }
    }

    let missing: Vec<String> = outcomes
        .into_iter()
        .filter_map(|(kind, (passing, failing))| {
            (!passing || !failing).then(|| format!("{kind}: pass={passing}, fail={failing}"))
        })
        .collect();
    assert!(
        missing.is_empty(),
        "canonical kinds without completed pass/fail traces:\n{}",
        missing.join("\n")
    );
}

#[test]
fn runner_with_registry_replaces_the_builtin_registry() {
    let builtin = Runner::default();
    assert!(
        builtin
            .registry
            .canonical_kinds()
            .any(|kind| kind == "file_exists")
    );
    let empty = alint_core::RuleRegistry::default();
    let injected = Runner::default().with_registry(empty);
    assert_eq!(injected.registry.canonical_kinds().count(), 0);
}
