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
        let document: ayaml::Value = match alint_core::yaml::from_str(&text) {
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
            condition_environment: &prepared.condition_environment,
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

/// The spawn and keyring uses a rule makes, as the capability check names
/// them.
fn capability_uses(rule: &str) -> (Option<String>, Option<String>) {
    let rule: RuleSpec = alint_core::yaml::from_str(rule).unwrap();
    let registry = alint_rules::builtin_registry();
    let (mut spawn_use, mut keyring_use) = (None, None);
    inspect_rule_capabilities(&rule, registry.catalog(), &mut spawn_use, &mut keyring_use);
    (spawn_use, keyring_use)
}

/// Nested rules have no `id` or `level`, and are still read for the kinds and
/// fix operations they use, at any `require:` depth.
#[test]
fn nested_rules_report_their_capabilities_at_any_depth() {
    let parent = "id: outer\nkind: for_each_dir\nselect: pkg\nlevel: error\n";
    let spawn = |usage: &str| (Some(format!("rule \"outer\" {usage}")), None);
    let cases = [
        ("require: [{kind: file_exists, paths: x}]\n", (None, None)),
        (
            "require: [{kind: file_exists, paths: x}, {kind: command, command: [a]}]\n",
            spawn("require[1] (command)"),
        ),
        (
            "require: [{kind: for_each_file, select: x, require: [{kind: command}]}]\n",
            spawn("require[0] require[0] (command)"),
        ),
        (
            "require: [{kind: file_exists, paths: x, fix: {command: {run: [a]}}}]\n",
            spawn("require[0] fix.command"),
        ),
        (
            "require: [{kind: file_exists, paths: x, fix: {file_create: {content: a}}}]\n",
            (None, None),
        ),
        // Only `require:` holds nested rules: a mapping with a `kind` in any
        // other option is data, as the configuration trust gate reads it.
        ("options: {inner: {kind: command}}\n", (None, None)),
        ("equals: [{kind: git_commit_gpg_signed}]\n", (None, None)),
        (
            "require: [{kind: file_exists, paths: x, options: {kind: command}}]\n",
            (None, None),
        ),
        // The first use is the one named.
        (
            "require: [{kind: command}, {kind: command_idempotent}]\n",
            spawn("require[0] (command)"),
        ),
        (
            "require: [{kind: git_commit_gpg_signed}, {kind: git_commit_gpg_signed}]\n",
            (
                None,
                Some("rule \"outer\" require[0] (git_commit_gpg_signed)".to_string()),
            ),
        ),
    ];
    for (options, expected) in cases {
        assert_eq!(
            capability_uses(&format!("{parent}{options}")),
            expected,
            "{options}"
        );
    }
    assert_eq!(
        capability_uses("id: top\nkind: command\ncommand: [a]\nlevel: error\n"),
        (Some("rule \"top\" (command)".to_string()), None)
    );
    assert_eq!(
        capability_uses(
            "id: top\nkind: file_exists\npaths: x\nlevel: error\nfix: {command: {run: [a]}}\n"
        ),
        (Some("rule \"top\" fix.command".to_string()), None)
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

static MARKER_META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::SingleShot,
    paths: alint_core::PathsSupport::Forbidden,
    scope_filter: alint_core::ScopeFilterSupport::Rejected {
        reason: std::borrow::Cow::Borrowed("not file-scoped"),
    },
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::PathOnly,
    formats: std::borrow::Cow::Borrowed(&[]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[]),
    expect_matches: false,
    trace: alint_core::TraceMeta::ASSERTION,
    nested_rules: false,
};

/// A kind no built-in registry has: it always reports one finding.
#[derive(Debug)]
struct MarkerRule {
    id: String,
}

impl alint_core::Rule for MarkerRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn level(&self) -> Level {
        Level::Error
    }

    fn evaluate(
        &self,
        _ctx: &alint_core::Context<'_>,
    ) -> alint_core::Result<Vec<alint_core::Violation>> {
        Ok(vec![alint_core::Violation::new("custom kind ran")])
    }
}

// The function must match the public `RuleBuilder` signature.
#[allow(clippy::unnecessary_wraps)]
fn build_marker(spec: &alint_core::RuleSpec) -> alint_core::Result<Box<dyn alint_core::Rule>> {
    Ok(Box::new(MarkerRule {
        id: spec.id.clone(),
    }))
}

#[test]
fn runner_with_registry_runs_a_custom_kind() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("marker.yml");
    let policy = PolicyTest::from_yaml(
        "test_version: 1\nname: marker\ngiven:\n  tree: {}\n  config: |\n    version: 1\n    \
rules:\n      - id: marker\n        kind: custom_marker\n        level: error\nwhen: [check]\n\
expect:\n  - exit: policy_failure\n    violations:\n      - {rule: marker, message: {regex: \
\"^custom kind ran$\"}}\n",
    )
    .unwrap();
    std::fs::write(&source, "").unwrap();

    let mut registry = alint_rules::builtin_registry();
    registry.register("custom_marker", &MARKER_META, build_marker);
    let injected = Runner::default()
        .with_registry(registry)
        .run(&policy, &source)
        .expect("the runner runs");
    assert_eq!(
        injected.status,
        TestStatus::Passed,
        "{:?}",
        injected.mismatches
    );

    let builtin = Runner::default()
        .run(&policy, &source)
        .expect("the runner runs");
    assert_eq!(
        builtin.status,
        TestStatus::Failed,
        "without the kind the config fails to load"
    );
}

/// The runner's own Git commands get a sandboxed home, no system or global
/// configuration, no prompts and the C locale, and the host's `PATH`.
#[test]
fn runner_git_environment_is_fixed_and_sandboxed() {
    let sandbox = tempfile::tempdir().unwrap();
    let environment = runner_git_environment(sandbox.path(), false).unwrap();
    let home = sandbox.path().join("home");
    assert!(home.is_dir());
    assert_eq!(environment["HOME"], home.to_string_lossy());
    assert_eq!(
        environment["GNUPGHOME"],
        home.join("gnupg").to_string_lossy()
    );
    assert_eq!(environment["GIT_CONFIG_NOSYSTEM"], "1");
    let global = if cfg!(windows) { "NUL" } else { "/dev/null" };
    assert_eq!(environment["GIT_CONFIG_GLOBAL"], global);
    assert_eq!(environment["GIT_TERMINAL_PROMPT"], "0");
    assert_eq!(environment["GIT_OPTIONAL_LOCKS"], "0");
    assert_eq!(environment["LC_ALL"], "C");
    assert_eq!(
        environment.get("PATH").map(String::as_str),
        std::env::var("PATH").ok().as_deref()
    );
    for name in environment.keys() {
        assert!(RUNNER_GIT_VARIABLES.contains(&name.as_str()), "{name}");
    }

    // With the host keyring granted, the home is the host's.
    let sandbox = tempfile::tempdir().unwrap();
    let environment = runner_git_environment(sandbox.path(), true).unwrap();
    assert_eq!(
        environment.get("HOME").map(String::as_str),
        std::env::var("HOME").ok().as_deref()
    );
    assert_eq!(environment["GIT_CONFIG_NOSYSTEM"], "1");
}

/// A granted command gets `given.env`, with the runner's variables taking
/// precedence over a `given.env` entry of the same name.
#[test]
fn granted_process_environment_is_given_env_under_the_runners_variables() {
    let test = PolicyTest::from_yaml(
        r#"test_version: 1
name: demo
given:
  tree: { a.txt: "x" }
  env: { CUSTOM: from-test, HOME: /test-home, LC_ALL: en_US.UTF-8 }
  config: |
    version: 1
    rules: []
when: [check]
expect:
  - exit: success
    violations: []
"#,
    )
    .unwrap();
    let runner = BTreeMap::from([
        ("HOME".to_string(), "/sandbox/home".to_string()),
        ("LC_ALL".to_string(), "C".to_string()),
        ("PATH".to_string(), "/bin".to_string()),
    ]);
    let environment = granted_process_environment(&test, &runner);
    assert_eq!(
        environment,
        BTreeMap::from([
            ("CUSTOM".to_string(), "from-test".to_string()),
            ("HOME".to_string(), "/sandbox/home".to_string()),
            ("LC_ALL".to_string(), "C".to_string()),
            ("PATH".to_string(), "/bin".to_string()),
        ])
    );
}

/// `setup_git` passes Git only the runner's own variables, whatever else its
/// caller hands it (`xtask docs-export` hands it a test's `given.env`).
#[cfg(unix)]
#[test]
fn setup_git_drops_every_variable_but_the_runners() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.txt"), "x\n").unwrap();
    std::fs::write(root.path().join(".gitattributes"), "* filter=probe\n").unwrap();
    let canary_dir = tempfile::tempdir().unwrap();
    let canary = canary_dir.path().display().to_string();
    let sandbox = tempfile::tempdir().unwrap();
    let mut environment = runner_git_environment(sandbox.path(), false).unwrap();
    for (name, value) in [
        ("GIT_TRACE", format!("{canary}/trace.txt")),
        ("GIT_CONFIG_COUNT", "2".to_string()),
        ("GIT_CONFIG_KEY_0", "core.fsmonitor".to_string()),
        (
            "GIT_CONFIG_VALUE_0",
            format!("printf ran > '{canary}/fsmonitor.txt'; true"),
        ),
        ("GIT_CONFIG_KEY_1", "filter.probe.clean".to_string()),
        (
            "GIT_CONFIG_VALUE_1",
            format!("printf ran > '{canary}/filter.txt'; cat"),
        ),
    ] {
        environment.insert(name.to_string(), value);
    }
    let spec = PolicyGit {
        init: true,
        add: vec!["a.txt".to_string(), ".gitattributes".to_string()],
        add_force: Vec::new(),
        commit: true,
        commits: Vec::new(),
    };

    setup_git(root.path(), &spec, &environment).unwrap();
    let written: Vec<_> = std::fs::read_dir(canary_dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(written, Vec::<std::ffi::OsString>::new());
}

/// SEC-15: a fixture file with hard links is copied once per link, so a
/// package of one 1.1 MB file and 30 links filled 31 MB of the sandbox. A
/// test's copies stop at the limit with an error naming the test and the
/// limit (here scaled down to 4 KiB, so the fourth 1100-byte copy passes it).
#[cfg(unix)]
#[test]
fn fixture_copies_stop_at_the_limit_whatever_the_hard_links() {
    let package = tempfile::tempdir().unwrap();
    let fixture = package.path().join("fx");
    std::fs::create_dir(&fixture).unwrap();
    std::fs::write(fixture.join("blob"), vec![b'x'; 1100]).unwrap();
    for link in 0..30 {
        std::fs::hard_link(fixture.join("blob"), fixture.join(format!("link{link}"))).unwrap();
    }
    let source = package.path().join("t.yml");
    let yaml = "test_version: 1\nname: hard links\ngiven:\n  fixture: fx\n  config: |\n    \
                version: 1\n    rules: []\nwhen: [check]\nexpect:\n  - exit: success\n";
    std::fs::write(&source, yaml).unwrap();
    let test = PolicyTest::from_yaml(yaml).unwrap();
    let package_root = PackageRoot::for_policy_test(&source).unwrap();

    let sandbox = tempfile::tempdir().unwrap();
    let error = materialize_repository_within(&test, &package_root, sandbox.path(), 4096)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("test \"hard links\": its fixture copies more than 4096 bytes"),
        "{error}"
    );
    let copied = std::fs::read_dir(sandbox.path()).unwrap().count();
    assert_eq!(copied, 3, "the copy stops at the limit");

    // A fixture within the limit copies whole.
    let sandbox = tempfile::tempdir().unwrap();
    materialize_repository_within(&test, &package_root, sandbox.path(), 31 * 1100).unwrap();
    assert_eq!(std::fs::read_dir(sandbox.path()).unwrap().count(), 31);
}
