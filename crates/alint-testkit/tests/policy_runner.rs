use std::path::{Path, PathBuf};

#[cfg(unix)]
use alint_testkit::Error;
use alint_testkit::{PolicyTest, Runner, RunnerOptions};

fn policy_file(package: &Path, name: &str, yaml: &str) -> (PathBuf, PolicyTest) {
    let source = package.join(name);
    std::fs::write(&source, yaml).expect("write policy test");
    let test = PolicyTest::from_yaml(yaml).expect("parse policy test");
    (source, test)
}

#[test]
fn remote_extends_is_an_assertable_configuration_error_without_fetching() {
    let package = tempfile::tempdir().unwrap();
    let (source, test) = policy_file(
        package.path(),
        "remote.yml",
        r#"test_version: 1
name: remote resolution is denied
given:
  tree: {}
  config: |
    version: 1
    extends:
      - https://user:secret@example.invalid/rules.yml#sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
    rules: []
when: [check]
expect:
  - exit: configuration_error
    configuration_error:
      message: { regex: "remote `extends` is disabled" }
"#,
    );

    let run = Runner::default().run(&test, &source).unwrap();
    assert!(run.passed(), "{:#?}", run.mismatches);
    let diagnostic = run.observations[0].configuration_error.as_deref().unwrap();
    assert!(diagnostic.contains("example.invalid"));
    assert!(!diagnostic.contains("user:secret"));
}

#[test]
fn local_extends_uses_the_package_root_not_the_repository_root() {
    let package = tempfile::tempdir().unwrap();
    std::fs::create_dir(package.path().join("support")).unwrap();
    std::fs::write(
        package.path().join("support/base.yaml"),
        "version: 1\nrules:\n  - id: inherited\n    kind: file_exists\n    paths: REQUIRED.md\n    level: error\n",
    )
    .unwrap();
    let (source, test) = policy_file(
        package.path(),
        "roots.yml",
        r#"test_version: 1
name: source and repository roots stay separate
given:
  tree:
    support/base.yaml: "this repository file must not be loaded as config"
  config: |
    version: 1
    extends: [./support/base.yaml]
    rules: []
when: [check]
expect:
  - exit: policy_failure
    violations:
      - { rule: inherited, level: error }
"#,
    );

    let run = Runner::default().run(&test, &source).unwrap();
    assert!(run.passed(), "{:#?}", run.mismatches);
}

#[cfg(unix)]
#[test]
fn spawning_fix_needs_both_capability_keys_and_runs_once_with_diff() {
    let package = tempfile::tempdir().unwrap();
    let yaml = r#"test_version: 1
name: spawning fixes are explicit and single execution
capabilities:
  spawn: true
given:
  tree:
    src/file.txt: "unfixed\n"
  config: |
    version: 1
    rules:
      - id: command-fix
        kind: command
        paths: src/file.txt
        command: ["sh", "-c", "grep -q FIXED \"$1\"", "_", "{path}"]
        level: error
        fix:
          command:
            run: ["sh", "-c", "printf 'x\\n' >> count.txt; printf FIXED > \"$1\"", "_", "{path}"]
when: [fix_unsafe, check]
expect:
  - exit: success
    applied: [command-fix]
    diff: "placeholder"
  - exit: success
    violations: []
expect_tree_mode: contains
expect_tree:
  count.txt: "x\n"
  src/file.txt: "FIXED"
"#;
    let (source, mut test) = policy_file(package.path(), "spawn.yml", yaml);

    let denied = Runner::default().run(&test, &source).unwrap_err();
    assert!(matches!(denied, Error::Capability(_)), "{denied:?}");

    let runner = Runner::new(RunnerOptions {
        allow_spawn: true,
        allow_host_keyring: false,
        update_snapshots: false,
    });
    let first = runner.run(&test, &source).unwrap();
    assert_eq!(
        std::fs::read_to_string(first.root.join("count.txt")).unwrap(),
        "x\n"
    );
    let diff = first.observations[0]
        .diff
        .clone()
        .expect("diff observation");
    assert!(diff.contains("count.txt"), "{diff}");
    assert!(diff.contains("src/file.txt"), "{diff}");

    test.expect[0].diff = Some(diff);
    let second = runner.run(&test, &source).unwrap();
    assert!(second.passed(), "{:#?}", second.mismatches);
    assert_eq!(
        std::fs::read_to_string(second.root.join("count.txt")).unwrap(),
        "x\n"
    );

    test.capabilities.spawn = false;
    let undeclared = runner.run(&test, &source).unwrap_err();
    assert!(matches!(undeclared, Error::Capability(_)), "{undeclared:?}");
}

#[test]
fn environment_is_injected_and_snapshots_round_trip_without_absolute_paths() {
    let package = tempfile::tempdir().unwrap();
    let (source, test) = policy_file(
        package.path(),
        "snapshot.yml",
        r#"test_version: 1
name: deterministic environment and snapshots
given:
  tree:
    selected/README.md: "ok\n"
  env:
    POLICY_PATH: selected/README.md
    POLICY_SECRET: never-render-this-value
  config: |
    version: 1
    rules:
      - id: selected-file
        kind: file_exists
        paths: "{{env.POLICY_PATH}}"
        level: error
      - id: sensitive-warning
        kind: file_exists
        paths: missing.txt
        level: warning
        message: "{{env.POLICY_SECRET}}"
when: [check]
expect:
  - exit: success
    violations:
      - { rule: sensitive-warning, level: warning }
snapshot: snapshots/result.json
"#,
    );

    let updater = Runner::new(RunnerOptions {
        allow_spawn: false,
        allow_host_keyring: false,
        update_snapshots: true,
    });
    let mut deferred = updater
        .run_deferred_snapshot_update(&test, &source)
        .unwrap();
    assert!(deferred.passed(), "{:#?}", deferred.mismatches);
    let snapshot_path = package.path().join("snapshots/result.json");
    assert!(!snapshot_path.exists());
    assert!(deferred.commit_snapshot_update().unwrap());
    assert!(!deferred.commit_snapshot_update().unwrap());
    let snapshot = std::fs::read_to_string(&snapshot_path).unwrap();
    assert!(snapshot.contains("policy-test-snapshot"));
    assert!(!snapshot.contains("never-render-this-value"));
    assert!(snapshot.contains("<redacted:"), "{snapshot}");
    assert!(!snapshot.contains(&package.path().to_string_lossy().to_string()));

    std::fs::remove_file(&snapshot_path).unwrap();
    let immediate = updater.run(&test, &source).unwrap();
    assert!(immediate.passed(), "{:#?}", immediate.mismatches);
    assert!(snapshot_path.exists());

    let checked = Runner::default().run(&test, &source).unwrap();
    assert!(checked.passed(), "{:#?}", checked.mismatches);
}

#[cfg(unix)]
#[test]
fn deferred_snapshot_commit_revalidates_a_replaced_parent() {
    use std::os::unix::fs::symlink;

    let package = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let (source, test) = policy_file(
        package.path(),
        "snapshot-race.yml",
        r"test_version: 1
name: snapshot parent race
given:
  tree: {}
  config: |
    version: 1
    rules: []
when: [check]
expect:
  - exit: success
    violations: []
snapshot: snapshots/result.json
",
    );
    let runner = Runner::new(RunnerOptions {
        allow_spawn: false,
        allow_host_keyring: false,
        update_snapshots: true,
    });
    let mut run = runner.run_deferred_snapshot_update(&test, &source).unwrap();
    symlink(outside.path(), package.path().join("snapshots")).unwrap();

    let error = run.commit_snapshot_update().unwrap_err();
    assert!(error.to_string().contains("snapshot parent"), "{error:?}");
    assert!(!outside.path().join("result.json").exists());
    std::fs::remove_file(package.path().join("snapshots")).unwrap();
    std::fs::create_dir(package.path().join("snapshots")).unwrap();
    assert!(run.commit_snapshot_update().unwrap());
    assert!(!run.commit_snapshot_update().unwrap());
    assert!(package.path().join("snapshots/result.json").exists());
}

/// A spawning rule nested in another rule's `require:` needs the spawn
/// capability too, from the document and the host, before anything runs.
#[cfg(unix)]
#[test]
fn nested_spawning_rule_needs_both_capability_keys() {
    let package = tempfile::tempdir().unwrap();
    // The canary is outside the sandbox, which is deleted when a run ends, so
    // a check that refused the test only after the command ran still fails.
    let canary_dir = tempfile::tempdir().unwrap();
    let canary = canary_dir.path().join("ran.txt");
    let yaml = format!(
        r#"test_version: 1
name: a nested command rule spawns
capabilities:
  spawn: true
given:
  tree:
    pkg/file.txt: "x\n"
  config: |
    version: 1
    rules:
      - id: per-dir
        kind: for_each_dir
        select: pkg
        level: error
        require:
          - kind: command
            paths: "{{path}}/file.txt"
            command: ["sh", "-c", "printf ran > '{canary}'"]
when: [check]
expect:
  - exit: success
    violations: []
"#,
        canary = canary.display()
    );
    let (source, mut test) = policy_file(package.path(), "nested.yml", &yaml);

    let denied = Runner::default().run(&test, &source).unwrap_err();
    assert!(matches!(denied, Error::Capability(_)), "{denied:?}");
    assert!(
        denied
            .to_string()
            .contains(r#"rule "per-dir" require[0] (command)"#),
        "{denied}"
    );
    let preflight = Runner::default().preflight(&test, &source).unwrap_err();
    assert!(matches!(preflight, Error::Capability(_)), "{preflight:?}");
    assert!(!canary.exists(), "a refused test ran its command");

    let runner = Runner::new(RunnerOptions {
        allow_spawn: true,
        allow_host_keyring: false,
        update_snapshots: false,
    });
    let run = runner.run(&test, &source).unwrap();
    assert!(run.passed(), "{:#?}", run.mismatches);
    assert!(canary.exists());

    std::fs::remove_file(&canary).unwrap();
    test.capabilities.spawn = false;
    let undeclared = runner.run(&test, &source).unwrap_err();
    assert!(matches!(undeclared, Error::Capability(_)), "{undeclared:?}");
    assert!(!canary.exists(), "an undeclared test ran its command");
}

/// `given.env` reaches interpolation and granted commands only. The runner's
/// own Git commands (the fixture setup and alint-core's Git reads) run with a
/// fixed environment, so a test cannot make Git run a command or write a file
/// without the `spawn` capability.
#[cfg(unix)]
#[test]
fn given_env_does_not_reach_the_runners_git() {
    let package = tempfile::tempdir().unwrap();
    let canary_dir = tempfile::tempdir().unwrap();
    let canary = canary_dir.path().display().to_string();
    let yaml = format!(
        r#"test_version: 1
name: git ignores the test's environment
given:
  env:
    GIT_TRACE: "{canary}/trace.txt"
    GIT_CONFIG_COUNT: "2"
    GIT_CONFIG_KEY_0: core.fsmonitor
    GIT_CONFIG_VALUE_0: "printf ran > '{canary}/fsmonitor.txt'; true"
    GIT_CONFIG_KEY_1: filter.probe.clean
    GIT_CONFIG_VALUE_1: "printf ran > '{canary}/filter.txt'; cat"
  git:
    init: true
    add: [a.txt, .gitattributes]
  tree:
    a.txt: "x\n"
    .gitattributes: "* filter=probe\n"
  config: |
    version: 1
    rules:
      - id: tracked
        kind: file_exists
        paths: a.txt
        git_tracked_only: true
        level: error
when: [check]
expect:
  - exit: success
    violations: []
"#
    );
    let (source, test) = policy_file(package.path(), "git-env.yml", &yaml);

    Runner::default().preflight(&test, &source).unwrap();
    let run = Runner::default().run(&test, &source).unwrap();
    assert!(run.passed(), "{:#?}", run.mismatches);
    let written: Vec<_> = std::fs::read_dir(canary_dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(written, Vec::<std::ffi::OsString>::new());
}
