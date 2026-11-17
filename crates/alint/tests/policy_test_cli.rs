use std::path::{Path, PathBuf};
use std::process::Output;

fn alint() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn run(root: &Path, args: &[&str]) -> Output {
    crate::hermetic::command(alint())
        .args(args)
        .current_dir(root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn passing_test(name: &str, tags: &str, snapshot: Option<&str>) -> String {
    let snapshot = snapshot.map_or_else(String::new, |path| format!("snapshot: {path}\n"));
    format!(
        "test_version: 1\nname: {name}\ntags: [{tags}]\ngiven:\n  tree:\n    README.md: \
\"ok\\n\"\n  config: |\n    version: 1\n    rules:\n      - id: readme\n        kind: file_exists\n        paths: README.md\n        level: error\nwhen: [check]\nexpect:\n  - exit: success\n    violations: []\n{snapshot}"
    )
}

fn failing_test(name: &str, snapshot: Option<&str>) -> String {
    let snapshot = snapshot.map_or_else(String::new, |path| format!("snapshot: {path}\n"));
    format!(
        "test_version: 1\nname: {name}\ngiven:\n  tree: {{}}\n  config: |\n    version: 1\n    rules:\n      - id: required\n        kind: file_exists\n        paths: required.txt\n        level: error\nwhen: [check]\nexpect:\n  - exit: success\n    violations: []\n{snapshot}"
    )
}

#[test]
fn default_discovery_filters_and_json_are_deterministic() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "alint-tests/z.yaml",
        &passing_test("zeta", "fast", None),
    );
    write(
        root.path(),
        "alint-tests/a.yml",
        &passing_test("alpha", "fast, smoke", None),
    );
    write(root.path(), "alint-tests/ignored.txt", "not a test");

    let first = run(root.path(), &["test", "--format", "json", "--tag", "fast"]);
    assert_eq!(
        first.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let second = run(root.path(), &["test", "--format", "json", "--tag", "fast"]);
    assert_eq!(first.stdout, second.stdout);
    let report: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["kind"], "policy-test-report");
    assert_eq!(report["summary"]["selected"], 2);
    assert_eq!(report["summary"]["passed"], 2);
    assert_eq!(report["tests"][0]["source"], "alint-tests/a.yml");
    assert_eq!(report["tests"][1]["source"], "alint-tests/z.yaml");
    let schema = alint_testkit::policy_test_report_schema();
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    let schema_errors: Vec<_> = validator
        .iter_errors(&report)
        .map(|error| error.to_string())
        .collect();
    assert!(schema_errors.is_empty(), "{schema_errors:#?}");

    let global_format = run(root.path(), &["--format", "json", "test", "--tag", "fast"]);
    assert_eq!(global_format.status.code(), Some(0));
    let global_report: serde_json::Value = serde_json::from_slice(&global_format.stdout).unwrap();
    assert_eq!(global_report["summary"]["passed"], 2);

    let tagged = run(
        root.path(),
        &[
            "test", "--format", "json", "--tag", "fast", "--tag", "smoke",
        ],
    );
    assert_eq!(tagged.status.code(), Some(0));
    let tagged_report: serde_json::Value = serde_json::from_slice(&tagged.stdout).unwrap();
    assert_eq!(tagged_report["summary"]["selected"], 1);
    assert_eq!(tagged_report["tests"][0]["name"], "alpha");

    let filtered = run(
        root.path(),
        &[
            "test", "--format", "json", "--tag", "fast", "--filter", "zeta",
        ],
    );
    assert_eq!(filtered.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&filtered.stdout).unwrap();
    assert_eq!(report["summary"]["selected"], 1);
    assert_eq!(report["tests"][0]["name"], "zeta");

    let path_filtered = run(
        root.path(),
        &["test", "--format", "json", "--filter", "z.yaml"],
    );
    assert_eq!(path_filtered.status.code(), Some(0));
    let path_report: serde_json::Value = serde_json::from_slice(&path_filtered.stdout).unwrap();
    assert_eq!(path_report["tests"][0]["name"], "zeta");

    let deduplicated = run(
        root.path(),
        &[
            "test",
            "alint-tests/a.yml",
            "alint-tests",
            "alint-tests/a.yml",
            "--format",
            "json",
        ],
    );
    assert_eq!(deduplicated.status.code(), Some(0));
    let deduplicated_report: serde_json::Value =
        serde_json::from_slice(&deduplicated.stdout).unwrap();
    assert_eq!(deduplicated_report["summary"]["selected"], 2);

    let none = run(root.path(), &["test", "--tag", "missing"]);
    assert_eq!(none.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&none.stdout).contains("selected no documents"));
}

#[test]
fn default_discovery_ignores_yaml_owned_by_fixtures_and_configs() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "alint-tests/config/base.yml",
        "version: 1\nrules: []\n",
    );
    write(
        root.path(),
        "alint-tests/fixtures/repository/.github/workflows/ci.yml",
        "name: ci\non: [push]\n",
    );
    write(
        root.path(),
        "alint-tests/fixtures/repository/malformed.yml",
        "malformed: [\n",
    );
    write(
        root.path(),
        "alint-tests/case.yml",
        r"test_version: 1
name: directory fixture containing YAML
given:
  fixture: fixtures/repository
  config_file: config/base.yml
when: [check]
expect:
  - exit: success
    violations: []
",
    );

    let output = run(root.path(), &["test", "--format", "json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["summary"]["selected"], 1);
    assert_eq!(report["summary"]["passed"], 1);

    let explicit = run(
        root.path(),
        &["test", "alint-tests/config/base.yml", "--format", "json"],
    );
    assert_eq!(explicit.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&explicit.stdout).unwrap();
    assert_eq!(report["tests"][0]["issues"][0]["code"], "parse_error");
}
#[test]
fn missing_default_directory_is_an_error() {
    let empty = tempfile::tempdir().unwrap();
    let no_tests = run(empty.path(), &["test", "--format", "json"]);
    assert_eq!(no_tests.status.code(), Some(2));
    let no_tests_report: serde_json::Value = serde_json::from_slice(&no_tests.stdout).unwrap();
    assert_eq!(no_tests_report["issues"][0]["code"], "no_tests");
}

#[test]
fn workspace_binary_runs_directory_fixture_local_config_git_and_fix_pipeline() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "fixtures/repository/Cargo.toml",
        "[package]\nname = \"demo\"\n",
    );
    write(
        root.path(),
        "fixtures/repository/src/main.rs",
        "fn main() {}\n",
    );
    write(
        root.path(),
        "policy/base.yml",
        "version: 1\nrules:\n  - id: has-readme\n    kind: file_exists\n    paths: README.md\n    root_only: true\n    level: error\n    message: \"{{env.POLICY_SECRET}}\"\n    fix:\n      file_create:\n        content: \"# demo\\n\"\n",
    );
    write(
        root.path(),
        "policy/root.yml",
        "version: 1\nextends:\n  - base.yml\n",
    );
    write(
        root.path(),
        "downstream.yml",
        r##"test_version: 1
name: downstream package
given:
  fixture: fixtures/repository
  config_file: policy/root.yml
  env:
    POLICY_SECRET: downstream-secret-canary
  git:
    init: true
    add: ['.']
    commit: true
when: [check, fix, check, check_changed]
expect:
  - exit: policy_failure
    violations:
      - {rule: has-readme, level: error}
  - exit: success
    applied: [has-readme]
  - exit: success
    violations: []
  - exit: success
    violations: []
expect_tree:
  Cargo.toml: "[package]\nname = \"demo\"\n"
  README.md: "# demo\n"
  src:
    main.rs: "fn main() {}\n"
"##,
    );

    let json = run(root.path(), &["test", "downstream.yml", "--format", "json"]);
    assert_eq!(
        json.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&json.stdout)
    );
    let report: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(report["summary"]["passed"], 1);
    assert_eq!(report["tests"][0]["steps"].as_array().unwrap().len(), 4);
    assert_eq!(report["tests"][0]["steps"][1]["operation"], "fix");
    assert_eq!(report["tests"][0]["steps"][3]["operation"], "check_changed");
    assert!(!String::from_utf8_lossy(&json.stdout).contains("downstream-secret-canary"));
    assert_ne!(
        report["tests"][0]["steps"][0]["observation"]["violations"][0]["message"],
        "downstream-secret-canary"
    );

    let human = run(root.path(), &["test", "downstream.yml"]);
    assert_eq!(human.status.code(), Some(0));
    let human = String::from_utf8_lossy(&human.stdout);
    assert!(human.contains("PASS downstream.yml - downstream package"));
    assert!(human.contains("PASS step 4: check_changed"));
}

#[test]
fn mismatch_and_invalid_definition_use_distinct_exit_codes() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "cases/fail.yml",
        &failing_test("mismatch", None),
    );
    let mismatch = run(root.path(), &["test", "cases"]);
    assert_eq!(mismatch.status.code(), Some(1));
    let human = String::from_utf8_lossy(&mismatch.stdout);
    assert!(human.contains("FAIL cases/fail.yml - mismatch"));
    assert!(human.contains("mismatch Exit"));
    assert!(human.contains("1 failed"));

    write(
        root.path(),
        "cases/bad.yml",
        "test_version: 99\nname: bad\n",
    );
    let invalid = run(root.path(), &["test", "cases", "--format", "json"]);
    assert_eq!(invalid.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&invalid.stdout).unwrap();
    assert_eq!(report["summary"]["invalid"], 1);
    assert_eq!(report["tests"][0]["status"], "invalid");
    assert_eq!(report["issues"][0]["code"], "validation_aborted");
}

#[test]
fn snapshot_updates_are_deferred_until_every_inline_expectation_passes() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "alint-tests/one.yml",
        &passing_test("one", "snapshot", Some("snapshots/one.json")),
    );
    write(
        root.path(),
        "alint-tests/two.yml",
        &failing_test("two", Some("snapshots/two.json")),
    );

    let failed = run(root.path(), &["test", "--update-snapshots"]);
    assert_eq!(failed.status.code(), Some(1));
    assert!(!root.path().join("alint-tests/snapshots/one.json").exists());
    assert!(!root.path().join("alint-tests/snapshots/two.json").exists());

    write(
        root.path(),
        "alint-tests/two.yml",
        &passing_test("two", "snapshot", Some("snapshots/two.json")),
    );
    let updated = run(root.path(), &["test", "--update-snapshots"]);
    assert_eq!(
        updated.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&updated.stdout)
    );
    let one = std::fs::read_to_string(root.path().join("alint-tests/snapshots/one.json")).unwrap();
    let two = std::fs::read_to_string(root.path().join("alint-tests/snapshots/two.json")).unwrap();
    assert!(one.contains("\"kind\": \"policy-test-snapshot\""));
    assert!(two.contains("\"kind\": \"policy-test-snapshot\""));
}

#[test]
fn invalid_git_fixture_is_a_definition_error() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "alint-tests/git.yml",
        r#"test_version: 1
name: broken git setup
given:
  tree:
    tracked.txt: "content\n"
  config: |
    version: 1
    rules: []
  git:
    commits:
      - message: invalid date
        add: [tracked.txt]
        date: definitely-not-a-git-date
when: [check]
expect:
  - exit: success
    violations: []
"#,
    );

    let output = run(root.path(), &["test", "--format", "json"]);
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["summary"]["invalid"], 1);
    assert_eq!(report["issues"][0]["code"], "validation_aborted");
    assert_eq!(report["tests"][0]["issues"][0]["code"], "git_fixture");
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains(root.path().to_string_lossy().as_ref())
    );
}

#[test]
fn operational_preflight_failure_uses_exit_three_without_marking_the_test_invalid() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "alint-tests/case.yml",
        &passing_test("cannot allocate sandbox", "failure", None),
    );
    let missing_temp = root.path().join("missing-temp");
    let output = crate::hermetic::command(alint())
        .args(["test", "--format", "json"])
        .current_dir(root.path())
        .env("NO_COLOR", "1")
        .env("TMPDIR", &missing_temp)
        .env("TMP", &missing_temp)
        .env("TEMP", &missing_temp)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["summary"]["invalid"], 0);
    assert_eq!(report["issues"][0]["code"], "preflight_aborted");
    assert_eq!(report["issues"][1]["code"], "runner_io");
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains(root.path().to_string_lossy().as_ref())
    );
}

#[test]
fn duplicate_snapshot_targets_and_incompatible_globals_fail_before_execution() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "alint-tests/one.yml",
        &passing_test("one", "snapshot", Some("same.json")),
    );
    write(
        root.path(),
        "alint-tests/two.yml",
        &passing_test("two", "snapshot", Some("same.json")),
    );
    let duplicate = run(root.path(), &["test", "--update-snapshots"]);
    assert_eq!(duplicate.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&duplicate.stdout).contains("duplicate_snapshot"));
    assert!(!root.path().join("alint-tests/same.json").exists());

    let global = run(root.path(), &["--fail-on-warning", "test"]);
    assert_eq!(global.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&global.stderr).contains("does not apply to `alint test`"));
}

#[cfg(unix)]
#[test]
fn spawn_requires_document_declaration_and_cli_grant_and_batch_preflight_is_non_executing() {
    let root = tempfile::tempdir().unwrap();
    let canary = root.path().join("canary");
    let sensitive_id = "ENV_FACT_ID_DO_NOT_REPORT";
    let command_test = format!(
        "test_version: 1\nname: command\ncapabilities:\n  spawn: true\ngiven:\n  env:\n    CANARY: {}\n    FACT_ID: {}\n  tree:\n    input.txt: \
\"x\\n\"\n  config: |\n    version: 1\n    facts:\n      - id: \"{{{{env.FACT_ID}}}}\"\n        custom:\n          argv: [\"sh\", \"-c\", \"printf ok\"]\n    rules:\n      - id: command\n        kind: command\n        paths: input.txt\n        command: [\"sh\", \"-c\", \"touch \\\"$CANARY\\\"\"]\n        level: error\nwhen: [check]\nexpect:\n  - exit: success\n    violations: []\n",
        canary.display(),
        sensitive_id
    );
    write(root.path(), "alint-tests/a-command.yml", &command_test);
    let denied = run(root.path(), &["test", "--format", "json"]);
    assert_eq!(denied.status.code(), Some(2));
    assert!(!canary.exists(), "command ran without the CLI grant");
    let denied_report: serde_json::Value = serde_json::from_slice(&denied.stdout).unwrap();
    assert_eq!(denied_report["tests"][0]["issues"][0]["code"], "capability");
    let denied_stdout = String::from_utf8_lossy(&denied.stdout);
    assert!(
        !denied_stdout.contains(sensitive_id),
        "capability diagnostic leaked an environment-derived fact id"
    );
    assert!(denied_stdout.contains("<redacted:"), "{denied_stdout}");

    write(
        root.path(),
        "alint-tests/z-invalid.yml",
        "test_version: nope\n",
    );

    let output = run(root.path(), &["test", "--allow-spawn"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        !canary.exists(),
        "command ran before batch validation completed"
    );

    std::fs::remove_file(root.path().join("alint-tests/z-invalid.yml")).unwrap();
    let allowed = run(root.path(), &["test", "--allow-spawn"]);
    assert_eq!(
        allowed.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&allowed.stdout)
    );
    assert!(canary.exists(), "granted command did not execute");

    std::fs::remove_file(&canary).unwrap();
    let undeclared = command_test.replace("capabilities:\n  spawn: true\n", "");
    write(root.path(), "alint-tests/a-command.yml", &undeclared);
    let undeclared = run(root.path(), &["test", "--allow-spawn"]);
    assert_eq!(undeclared.status.code(), Some(2));
    assert!(
        !canary.exists(),
        "command ran without its document declaration"
    );
}

#[cfg(unix)]
#[test]
fn explicit_symlink_directory_is_rejected_without_following_it() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "real/case.yml",
        &passing_test("case", "fast", None),
    );
    symlink("real", root.path().join("linked")).unwrap();
    let output = run(root.path(), &["test", "linked", "--format", "json"]);
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["issues"][0]["code"], "symlink_path");
}

/// What a test sees does not depend on the host's Git setup: the user's
/// global excludes file (`$XDG_CONFIG_HOME/git/ignore`) hides nothing.
#[test]
fn the_hosts_global_excludes_file_hides_nothing_from_a_test() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    write(host.path(), "git/ignore", "secret-*.txt\n");
    write(
        root.path(),
        "alint-tests/excludes.yml",
        "test_version: 1\nname: excludes\ngiven:\n  tree:\n    secret-1.txt: \"x\"\n  git: \
{ init: true }\n  config: |\n    version: 1\n    rules:\n      - id: no-secret\n        kind: \
file_absent\n        paths: \"secret-*.txt\"\n        level: error\nwhen: [check]\nexpect:\n  - \
exit: policy_failure\n    violations:\n      - { rule: no-secret, path: secret-1.txt }\n",
    );

    let output = crate::hermetic::command(alint())
        .args(["test"])
        .current_dir(root.path())
        .env("NO_COLOR", "1")
        .env("XDG_CONFIG_HOME", host.path())
        .env("HOME", host.path())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Nor does it hide a nested config: the walk that finds `nested_configs:`
/// files reads no global excludes file either (nor a `~/.gitconfig` that
/// names one), as the test's own walk reads none. It did, so a host whose
/// excludes listed `.alint.yml` dropped the nested rules, and a test of them
/// passed with no finding. `alint check` keeps honouring the file.
#[test]
fn the_hosts_global_excludes_file_hides_no_nested_config_from_a_test() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    write(host.path(), "git/ignore", ".alint.yml\n");
    // A `.gitconfig` in the host's home can name an excludes file too.
    write(host.path(), "excludes", "*.yml\n");
    write(
        host.path(),
        ".gitconfig",
        &format!(
            "[core]\n\texcludesFile = {}\n",
            host.path().join("excludes").display()
        ),
    );
    write(
        root.path(),
        "alint-tests/nested.yml",
        r#"test_version: 1
name: nested
given:
  tree:
    pkg:
      .alint.yml: |
        version: 1
        rules:
          - id: no-secret
            kind: file_absent
            paths: "secret-*.txt"
            level: error
      secret-1.txt: "x"
  config: |
    version: 1
    nested_configs: true
when: [check]
expect:
  - exit: policy_failure
    violations:
      - { rule: no-secret, path: pkg/secret-1.txt }
"#,
    );

    for xdg_config_home in [host.path().to_path_buf(), host.path().join("no-xdg")] {
        let output = crate::hermetic::command(alint())
            .args(["test"])
            .current_dir(root.path())
            .env("NO_COLOR", "1")
            .env("XDG_CONFIG_HOME", &xdg_config_home)
            .env("HOME", host.path())
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "XDG_CONFIG_HOME={}: {}{}",
            xdg_config_home.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // `alint check` still honours the user's excludes file there, as its own
    // walk does: the nested config it lists is not read.
    let tree = tempfile::tempdir().unwrap();
    write(
        tree.path(),
        ".alint.yml",
        "version: 1\nnested_configs: true\n",
    );
    write(
        tree.path(),
        "pkg/.alint.yml",
        "version: 1\nrules:\n  - id: no-secret\n    kind: file_absent\n    \
         paths: \"secret-*.txt\"\n    level: error\n",
    );
    write(tree.path(), "pkg/secret-1.txt", "x");
    let empty = tempfile::tempdir().unwrap();
    for (home, expected) in [(host.path(), 0), (empty.path(), 1)] {
        let output = crate::hermetic::command(alint())
            .args(["check"])
            .current_dir(tree.path())
            .env("NO_COLOR", "1")
            .env("XDG_CONFIG_HOME", home)
            .env("HOME", home)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(expected),
            "HOME={}: {}{}",
            home.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
