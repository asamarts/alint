//! End-to-end contract for selected-rule `explain --trace` execution.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn alint_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
    std::fs::write(path, contents).unwrap();
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(alint_bin())
        .args(args)
        .current_dir(root)
        .env("NO_COLOR", "1")
        .env("SECRET_CANARY", "trace-canary-value")
        .output()
        .expect("spawn alint")
}

fn json(output: &Output) -> serde_json::Value {
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    );
    serde_json::from_slice(&output.stdout).expect("trace output is JSON")
}

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path().join(".alint.yml"),
        r#"version: 1
templates:
  - id: secret-query
    kind: json_path_equals
    paths: secret.json
    path: "$.token"
    equals: "{{vars.expected}}"
    level: warning
rules:
  - id: present
    kind: file_exists
    paths: README.md
    level: warning
  - id: missing
    kind: file_exists
    paths: MISSING.md
    level: error
  - id: disabled
    kind: file_exists
    paths: MISSING.md
    level: off
  - id: values
    kind: json_path_equals
    paths: "data/*.json"
    path: "$.value"
    equals: ok
    level: warning
  - id: malformed
    kind: json_path_equals
    paths: malformed.json
    path: "$.value"
    equals: ok
    level: warning
  - id: secret
    extends_template: secret-query
    vars: { expected: "{{env.SECRET_CANARY}}" }
    message: "{{env.SECRET_CANARY}}"
  - id: explicit-secret
    kind: json_path_equals
    paths: secret.json
    path: "$.token"
    equals: trace-canary-value
    sensitive: true
    message: trace-canary-value
    level: warning
  - id: implicit-secret
    kind: properties_path_equals
    paths: secret.properties
    path: "$.token"
    equals: expected
    level: warning
  - id: secret-url
    kind: file_exists
    paths: MISSING-URL.md
    policy_url: "https://example.test/{{env.SECRET_CANARY}}"
    level: warning
  - id: controls
    kind: json_path_equals
    paths: controls.json
    path: "$.value"
    equals: "\u001b[2J\u202e"
    level: warning
"#,
    );
    write(root.path().join("README.md"), "# fixture\n");
    std::fs::create_dir(root.path().join("data")).unwrap();
    write(
        root.path().join("data/a.json"),
        br#"{"value":"ok"}
"#,
    );
    write(
        root.path().join("data/z.json"),
        br#"{"value":"ok"}
"#,
    );
    write(root.path().join("malformed.json"), "{\n");
    write(
        root.path().join("secret.json"),
        br#"{"token":"trace-canary-value"}
"#,
    );
    write(
        root.path().join("secret.properties"),
        "token=implicit-canary\n",
    );
    write(
        root.path().join("controls.json"),
        br#"{"value":"\u001b[2J\u202e"}
"#,
    );
    root
}

#[test]
fn trace_reports_pass_violation_skip_and_analysis_failure_as_diagnostic_success() {
    let root = fixture();
    for (id, expected) in [
        ("present", "satisfied"),
        ("missing", "violated"),
        ("disabled", "skipped"),
        ("malformed", "analysis_failure"),
    ] {
        let output = run(root.path(), &["explain", id, "--trace", "--format", "json"]);
        let document = json(&output);
        assert_eq!(document["kind"], "rule-trace");
        assert_eq!(document["stability"], "experimental");
        assert_eq!(document["rule"]["id"], id);
        assert_eq!(document["result"]["status"], expected, "{document:#}");
        assert_eq!(
            document["stages"].as_array().unwrap().last().unwrap()["status"],
            expected
        );
    }
}

#[test]
fn tracing_a_rule_does_not_build_unrelated_disabled_rules() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path().join(".alint.yml"),
        r"version: 1
rules:
  - id: selected
    kind: file_exists
    paths: README.md
    level: warning
  - id: malformed-disabled
    kind: command
    paths: README.md
    level: off
",
    );
    write(root.path().join("README.md"), "# fixture\n");

    let document = json(&run(
        root.path(),
        &["explain", "selected", "--trace", "--format", "json"],
    ));
    assert_eq!(document["result"]["status"], "satisfied");
}

#[test]
fn trace_reports_fixability_without_applying_the_fix() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path().join(".alint.yml"),
        r#"version: 1
rules:
  - id: fixable
    kind: file_exists
    paths: generated.txt
    level: error
    fix: { file_create: { content: "generated\n" } }
"#,
    );

    let document = json(&run(
        root.path(),
        &["explain", "fixable", "--trace", "--format", "json"],
    ));
    assert_eq!(document["result"]["status"], "violated");
    assert_eq!(document["result"]["violations"][0]["fixable"], true);
    assert!(!root.path().join("generated.txt").exists());
}

#[test]
fn key_parity_trace_preview_carries_keys_not_values() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path().join(".alint.yml"),
        r#"version: 1
rules:
  - id: locales
    kind: key_parity
    reference: locales/en.json
    peers: "locales/*.json"
    level: error
"#,
    );
    std::fs::create_dir(root.path().join("locales")).unwrap();
    write(
        root.path().join("locales/en.json"),
        r#"{"nav":{"home":"Home","about":"About"}}"#,
    );
    write(
        root.path().join("locales/fr.json"),
        r#"{"nav":{"home":"Accueil"}}"#,
    );
    let document = json(&run(
        root.path(),
        &[
            "explain",
            "locales",
            "--trace",
            "--preview",
            "--format",
            "json",
        ],
    ));
    let extraction = document["stages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stage| stage["class"] == "extraction")
        .expect("extraction stage");
    let rendered = extraction.to_string();
    assert!(!rendered.contains("Home"), "{rendered}");
    assert!(!rendered.contains("Accueil"), "{rendered}");
    assert!(rendered.contains("nav.home"), "{rendered}");
    assert!(rendered.contains("nav.about"), "{rendered}");
    let preview = &extraction["preview"];
    assert_eq!(preview["total"], 3);
    assert_eq!(preview["redacted"], 0);

    let bounded = json(&run(
        root.path(),
        &[
            "explain",
            "locales",
            "--trace",
            "--preview",
            "--preview-limit",
            "1",
            "--format",
            "json",
        ],
    ));
    let preview = &bounded["stages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stage| stage["class"] == "extraction")
        .unwrap()["preview"];
    assert_eq!(preview["total"], 3);
    assert_eq!(preview["shown"], 1);
    assert_eq!(preview["omitted"], 2);
}

#[test]
fn preview_is_bounded_deterministic_and_schema_valid() {
    let root = fixture();
    let args = [
        "explain",
        "values",
        "--trace",
        "--preview",
        "--preview-limit",
        "1",
        "--format",
        "json",
    ];
    let first = run(root.path(), &args);
    let second = run(root.path(), &args);
    assert_eq!(
        first.stdout, second.stdout,
        "trace JSON must be byte-stable"
    );
    let document = json(&first);
    let extraction = document["stages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stage| stage["class"] == "extraction")
        .unwrap();
    assert_eq!(extraction["preview"]["total"], 2);
    assert_eq!(extraction["preview"]["shown"], 1);
    assert_eq!(extraction["preview"]["omitted"], 1);
    assert_eq!(extraction["preview"]["rows"][0]["path"], "data/a.json");

    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../schemas/experimental/rule-trace.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors: Vec<_> = validator
        .iter_errors(&document)
        .map(|error| error.to_string())
        .collect();
    assert!(errors.is_empty(), "schema errors: {errors:#?}");

    let static_document = json(&run(
        root.path(),
        &["explain", "values", "--format", "json"],
    ));
    assert!(!validator.is_valid(&static_document));
}

#[test]
fn nested_preview_retains_the_first_bounded_child_instances() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path().join(".alint.yml"),
        r#"version: 1
rules:
  - id: packages
    kind: for_each_dir
    select: "pkg-*"
    require:
      - kind: file_exists
        paths: "{path}/marker"
    level: warning
"#,
    );
    for package in ["pkg-a", "pkg-b"] {
        std::fs::create_dir(root.path().join(package)).unwrap();
    }
    write(root.path().join("pkg-a/marker"), "ok\n");

    let document = json(&run(
        root.path(),
        &[
            "explain",
            "packages",
            "--trace",
            "--preview",
            "--preview-limit",
            "1",
            "--format",
            "json",
        ],
    ));
    let nested = document["stages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stage| stage["class"] == "nested")
        .expect("nested trace stage");
    assert_eq!(nested["preview"]["total"], 2);
    assert_eq!(nested["preview"]["shown"], 1);
    assert_eq!(nested["preview"]["omitted"], 1);
    assert_eq!(nested["preview"]["rows"][0]["path"], "pkg-a");
    assert_eq!(nested["preview"]["rows"][0]["display_value"], "satisfied");
}

#[test]
fn environment_and_template_evidence_is_redacted_in_static_and_trace_outputs() {
    let root = fixture();
    for args in [
        vec!["explain", "secret", "--format", "json"],
        vec![
            "explain",
            "secret",
            "--trace",
            "--preview",
            "--format",
            "json",
        ],
        vec!["explain", "secret"],
        vec!["explain", "secret", "--trace", "--preview"],
    ] {
        let output = run(root.path(), &args);
        assert_eq!(output.status.code(), Some(0));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!stdout.contains("trace-canary-value"), "{stdout}");
        assert!(stdout.contains("<redacted>"), "{stdout}");
    }
    for id in ["secret", "explicit-secret"] {
        let output = run(
            root.path(),
            &["explain", id, "--trace", "--preview", "--format", "json"],
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!stdout.contains("trace-canary-value"), "{stdout}");
        assert!(stdout.contains("<redacted>"), "{stdout}");
    }

    let document = json(&run(
        root.path(),
        &[
            "explain",
            "secret",
            "--trace",
            "--preview",
            "--format",
            "json",
        ],
    ));
    assert_eq!(document["rule"]["options"]["equals"], "<redacted>");
    assert_eq!(document["rule"]["message"], "<redacted>");
    let preview = document["stages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stage| stage["class"] == "extraction")
        .unwrap()["preview"]
        .clone();
    assert_eq!(preview["redacted"], 1);
    assert_eq!(preview["rows"][0]["display_value"], "<redacted>");
    assert_eq!(preview["rows"][0]["pointer"], serde_json::Value::Null);
    let implicit = run(
        root.path(),
        &[
            "explain",
            "implicit-secret",
            "--trace",
            "--preview",
            "--format",
            "json",
        ],
    );
    let implicit_stdout = String::from_utf8_lossy(&implicit.stdout);
    assert!(
        !implicit_stdout.contains("implicit-canary"),
        "{implicit_stdout}"
    );
    let implicit = json(&implicit);
    assert_eq!(implicit["result"]["status"], "violated");
    let implicit_preview = implicit["stages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stage| stage["class"] == "extraction")
        .unwrap()["preview"]
        .clone();
    assert_eq!(implicit_preview["redacted"], 1);
    assert_eq!(implicit_preview["rows"][0]["display_value"], "<redacted>");
}

#[test]
fn environment_derived_policy_urls_are_redacted_across_consumers() {
    let root = fixture();
    for args in [
        vec!["explain", "secret-url", "--format", "json"],
        vec!["explain", "secret-url", "--trace", "--format", "json"],
        vec!["check", "--only", "secret-url", "--format", "json"],
    ] {
        let output = run(root.path(), &args);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!stdout.contains("trace-canary-value"), "{args:?}\n{stdout}");
        assert!(stdout.contains("<redacted>"), "{args:?}\n{stdout}");
    }
}

#[test]
fn human_trace_neutralizes_terminal_controls_and_bidi_text() {
    let root = fixture();
    let output = run(
        root.path(),
        &[
            "explain",
            "controls",
            "--trace",
            "--preview",
            "--width",
            "40",
        ],
    );
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains('\u{1b}'), "{stdout:?}");
    assert!(!stdout.contains('\u{202e}'), "{stdout:?}");
    assert!(stdout.contains(r"\x1b[2J"), "{stdout:?}");
    assert!(stdout.contains(r"\u{202e}"), "{stdout:?}");
    let trace = stdout.split("trace:\n").nth(1).expect("trace section");
    assert!(
        trace.lines().all(|line| line.chars().count() <= 40),
        "{trace}"
    );
}

#[test]
fn invalid_trace_flag_combinations_and_limits_exit_two() {
    let root = fixture();
    for args in [
        vec!["explain", "present", "--preview"],
        vec!["explain", "present", "--preview-limit", "1"],
        vec!["explain", "present", "--trace", "--preview-limit", "1"],
        vec![
            "explain",
            "present",
            "--trace",
            "--preview",
            "--preview-limit",
            "101",
        ],
        vec![
            "explain",
            "present",
            "--trace",
            "--preview",
            "--preview-limit",
            "0",
        ],
        vec!["explain", "present", "--changed"],
        vec!["explain", "present", "--base", "HEAD"],
        vec!["explain", "present", "--trace", "--format", "sarif"],
        vec!["explain", "unknown", "--trace"],
    ] {
        let output = run(root.path(), &args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "args={args:?}\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

#[test]
fn base_implies_changed_and_is_recorded() {
    let root = fixture();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(root.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q"]);
    git(&["config", "user.name", "Trace Test"]);
    git(&["config", "user.email", "trace@example.test"]);
    git(&["add", "."]);
    git(&["commit", "-qm", "initial"]);
    write(root.path().join("README.md"), "# changed\n");
    git(&["add", "README.md"]);
    git(&["commit", "-qm", "changed"]);

    let document = json(&run(
        root.path(),
        &[
            "explain", "present", "--trace", "--base", "HEAD~1", "--format", "json",
        ],
    ));
    assert_eq!(document["execution"]["changed"], true);
    assert_eq!(document["execution"]["base"], "HEAD~1");
    assert_eq!(document["result"]["status"], "satisfied");
}

#[cfg(unix)]
#[test]
fn tracing_executes_only_the_selected_rule() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path().join(".alint.yml"),
        r#"version: 1
rules:
  - id: selected
    kind: file_exists
    paths: input.txt
    level: warning
  - id: unrelated
    kind: command
    paths: input.txt
    command: ["sh", "-c", "touch marker"]
    level: warning
  - id: sensitive-command
    kind: command
    paths: input.txt
    command: ["sh", "-c", "printf command-canary >&2; exit 1"]
    sensitive: true
    level: warning
"#,
    );
    write(root.path().join("input.txt"), "input\n");

    let document = json(&run(
        root.path(),
        &["explain", "selected", "--trace", "--format", "json"],
    ));
    assert_eq!(document["result"]["status"], "satisfied");
    assert!(!root.path().join("marker").exists());

    let static_output = run(root.path(), &["explain", "unrelated"]);
    assert!(static_output.status.success());
    assert!(!root.path().join("marker").exists());

    let command_document = json(&run(
        root.path(),
        &["explain", "unrelated", "--trace", "--format", "json"],
    ));
    assert_eq!(command_document["result"]["status"], "satisfied");
    assert!(root.path().join("marker").exists());

    for args in [
        vec!["explain", "sensitive-command", "--trace"],
        vec![
            "explain",
            "sensitive-command",
            "--trace",
            "--format",
            "json",
        ],
    ] {
        let output = run(root.path(), &args);
        assert_eq!(output.status.code(), Some(0));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!stdout.contains("command-canary"), "{stdout}");
        assert!(stdout.contains("<redacted>"), "{stdout}");
    }
}

#[test]
fn environment_derived_selector_paths_are_redacted_from_static_and_trace_explain() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path().join(".alint.yml"),
        r#"version: 1
rules:
  - id: env-path
    kind: json_path_equals
    paths: "{{env.SECRET_CANARY}}.json"
    path: $.token
    equals: expected
    level: warning
  - id: nested-env-path
    kind: for_each_file
    select: "{{env.SECRET_CANARY}}.json"
    require:
      - kind: file_content_forbidden
        paths: "{path}"
        pattern: actual
    level: warning
"#,
    );
    write(
        root.path().join("trace-canary-value.json"),
        r#"{"token":"actual"}"#,
    );

    for args in [
        vec!["explain", "env-path"],
        vec!["explain", "env-path", "--format", "json"],
        vec!["explain", "env-path", "--trace", "--preview"],
        vec![
            "explain",
            "env-path",
            "--trace",
            "--preview",
            "--format",
            "json",
        ],
    ] {
        let output = run(root.path(), &args);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut rendered = output.stdout;
        rendered.extend_from_slice(&output.stderr);
        let rendered = String::from_utf8_lossy(&rendered);
        assert!(
            !rendered.contains("trace-canary-value"),
            "{args:?} leaked the environment-derived path:\n{rendered}"
        );
        assert!(
            rendered.contains("<redacted>"),
            "{args:?} omitted the redaction marker:\n{rendered}"
        );
    }

    for format in ["human", "json"] {
        let output = run(
            root.path(),
            &[
                "explain",
                "nested-env-path",
                "--trace",
                "--preview",
                "--format",
                format,
            ],
        );
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        let rendered = String::from_utf8_lossy(&output.stdout);
        assert!(
            !rendered.contains("trace-canary-value"),
            "nested {format} trace leaked the selector path:\n{rendered}"
        );
        assert!(rendered.contains("<redacted>"), "{rendered}");
    }
}

#[cfg(unix)]
#[test]
fn trace_evaluates_custom_facts_required_by_the_production_check_context() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path().join(".alint.yml"),
        r#"version: 1
facts:
  - id: enabled
    custom:
      argv: ["/bin/sh", "-c", "touch fact-marker; printf 'yes\n'"]
rules:
  - id: selected
    when: facts.enabled == "yes"
    kind: file_exists
    paths: input.txt
    level: warning
"#,
    );
    write(root.path().join("input.txt"), "input\n");

    let static_output = run(root.path(), &["explain", "selected"]);
    assert!(static_output.status.success());
    assert!(!root.path().join("fact-marker").exists());

    let document = json(&run(
        root.path(),
        &["explain", "selected", "--trace", "--format", "json"],
    ));
    assert_eq!(document["result"]["status"], "satisfied");
    assert!(root.path().join("fact-marker").exists());
}
