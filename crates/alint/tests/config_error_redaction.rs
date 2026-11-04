//! A configuration error never prints a value read from the environment or
//! set in a `sensitive: true` rule, on any surface: `check` in every format,
//! `validate-config`, `list`, `explain`, `fix`, `alint test` and the language
//! server. The unit tests in `alint-core` and `alint-dsl` pin each step; these
//! drive the real binary, so a call site that skips the redaction fails here.

use std::path::Path;
use std::process::{Command, Output};

/// A value interpolation re-types as a number: the error quotes it as one.
const ACCOUNT_ID: &str = "123456789012";

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn run(root: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_alint"))
        .args(args)
        .current_dir(root)
        .env("NO_COLOR", "1")
        .envs(env.iter().copied())
        .output()
        .unwrap()
}

/// `output` failed with a redacted configuration error that never quotes any
/// of `secrets`.
fn assert_redacted(surface: &str, output: &Output, secrets: &[&str]) {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success(),
        "{surface}: expected a configuration error:\n{text}"
    );
    for secret in secrets {
        assert!(!text.contains(secret), "{surface} leaked {secret}:\n{text}");
    }
    assert!(text.contains("<redacted>"), "{surface}:\n{text}");
}

#[test]
fn a_numeric_environment_value_stays_redacted_on_every_surface() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "README.md", "hi\n");
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nrules:\n  - id: no-account-id\n    kind: file_content_forbidden\n    \
         paths: '**/*'\n    pattern: '{{env.AWS_ACCOUNT_ID}}'\n    level: error\n",
    );
    let env = [("AWS_ACCOUNT_ID", ACCOUNT_ID)];
    for args in [
        &["check"][..],
        &["check", "--format", "json"],
        &["check", "--format", "sarif"],
        &["check", "--format", "agent"],
        &["validate-config"],
        &["validate-config", "--format", "json"],
        &["list"],
        &["explain", "no-account-id"],
        &["fix", "--dry-run"],
    ] {
        let output = run(root.path(), args, &env);
        assert_redacted(&args.join(" "), &output, &[ACCOUNT_ID]);
    }

    // A hex value is re-typed too, and an error prints it in decimal; a
    // message field fails at a different step from a pattern.
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nrules:\n  - id: pin\n    kind: file_exists\n    paths: README.md\n    \
         message: '{{env.PIN}}'\n    level: error\n",
    );
    let output = run(root.path(), &["check"], &[("PIN", "0xDEADBEEF")]);
    assert_redacted("hex message", &output, &["3735928559", "DEADBEEF"]);
}

#[test]
fn a_top_level_value_read_from_the_environment_stays_redacted() {
    for (case, config, nested, env) in [
        (
            "ignore",
            "ignore: ['{{env.VALUE}}']\n",
            None,
            "!canary-secret[",
        ),
        (
            "facts",
            "facts:\n  - id: f\n    any_file_exists: ['{{env.VALUE}}']\n",
            None,
            "canary-secret[",
        ),
        (
            "baseline",
            "baseline: '{{env.VALUE}}'\n",
            None,
            "canary-secret.jsonl",
        ),
        (
            "extends",
            "extends: ['{{env.VALUE}}']\n",
            None,
            "./canary-secret.yml",
        ),
        (
            "nested absolute path",
            "nested_configs: true\n",
            Some(
                "version: 1\nrules:\n  - id: nested\n    kind: file_exists\n    \
                 paths: '{{env.VALUE}}'\n    level: error\n",
            ),
            "/canary-secret/x",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "README.md", "hi\n");
        write(
            root.path(),
            ".alint.yml",
            &format!(
                "version: 1\n{config}rules:\n  - id: readme\n    kind: file_exists\n    \
                 paths: README.md\n    level: error\n"
            ),
        );
        if let Some(nested) = nested {
            write(root.path(), "sub/.alint.yml", nested);
        }
        let output = run(root.path(), &["check"], &[("VALUE", env)]);
        assert_redacted(case, &output, &["canary-secret"]);
    }

    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        ".alint.yml",
        "version: '{{env.VALUE}}'\nrules: []\n",
    );
    let output = run(root.path(), &["check"], &[("VALUE", "987654")]);
    assert_redacted("version", &output, &["987654"]);
}

#[test]
fn a_nested_rule_error_read_from_the_environment_stays_redacted() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "crates/a/x.txt", "x\n");
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nrules:\n  - id: each\n    kind: for_each_dir\n    select: 'crates/*'\n    \
         level: error\n    require:\n      - kind: file_content_forbidden\n        \
         paths: '{path}/*.txt'\n        pattern: '{{env.SECRET}}'\n",
    );
    for args in [&["check"][..], &["validate-config", "--format", "json"]] {
        let output = run(root.path(), args, &[("SECRET", "[builder-canary")]);
        assert_redacted(&args.join(" "), &output, &["builder-canary"]);
    }
}

#[test]
fn a_structural_error_beside_a_secret_stays_readable() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nrules:\n  - id: each\n    kind: for_each_dir\n    select: 'crates/*'\n    \
         level: error\n    sensitive: true\n    require:\n      - kind: file_exists\n        \
         paths: '{path}/Cargo.toml'\n        colour: red\n",
    );
    let output = run(root.path(), &["check"], &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown field `colour`"), "{stderr}");
    assert!(!stderr.contains("<redacted>"), "{stderr}");
}

#[test]
fn a_policy_test_reports_a_sensitive_number_redacted() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "alint-tests/sensitive.yml",
        &format!(
            "test_version: 1\nname: sensitive number\ngiven:\n  tree: {{}}\n  config: |\n    \
             version: 1\n    rules:\n      - id: scan\n        kind: file_content_forbidden\n        \
             paths: '**/*'\n        pattern: {ACCOUNT_ID}\n        sensitive: true\n        \
             level: error\nwhen: [check]\nexpect:\n  - exit: configuration_error\n    \
             configuration_error:\n      message: {{ regex: \"redacted\" }}\n"
        ),
    );
    let output = run(root.path(), &["test", "--format", "json"], &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("<redacted>"), "{stdout}");
    assert!(!stdout.contains(ACCOUNT_ID), "{stdout}");
}

#[cfg(unix)]
#[test]
fn the_language_server_reports_configuration_errors_redacted() {
    use crate::lsp_common::{recv_response, spawn_server_with_env, wait_for_diagnostics};
    use serde_json::json;

    const README: &str = "rules:\n  - id: readme\n    kind: file_exists\n    \
                          paths: README.md\n    level: error\n";
    let baseline = format!("version: 1\nbaseline: '{{{{env.VALUE}}}}'\n{README}");
    let ignore = format!("version: 1\nignore: ['{{{{env.VALUE}}}}']\n{README}");
    let facts = format!(
        "version: 1\nfacts:\n  - id: f\n    any_file_exists: ['{{{{env.VALUE}}}}']\n{README}"
    );
    for (config, env, secret) in [
        (baseline.as_str(), "canary-secret.jsonl", "canary-secret"),
        (ignore.as_str(), "!canary-secret[", "canary-secret"),
        (facts.as_str(), "canary-secret[", "canary-secret"),
        (
            "version: 1\nrules:\n  - id: scan\n    kind: file_content_forbidden\n    \
             paths: '**/*'\n    pattern: '{{env.VALUE}}'\n    level: error\n",
            ACCOUNT_ID,
            ACCOUNT_ID,
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path().canonicalize().unwrap();
        write(&root_path, "README.md", "hi\n");
        write(&root_path, ".alint.yml", config);
        let config_uri = format!("file://{}", root_path.join(".alint.yml").display());
        let mut server = spawn_server_with_env(&root_path, &[("VALUE", env)]);
        server.send(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "rootUri": format!("file://{}", root_path.display()), "capabilities": {} }
        }));
        recv_response(&server.rx, 1);
        server.send(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
        // Opening a document runs the check that builds the session.
        server.send(&json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": { "textDocument": {
                "uri": format!("file://{}", root_path.join("README.md").display()),
                "languageId": "markdown", "version": 1, "text": "hi\n"
            }}
        }));
        let diagnostics = wait_for_diagnostics(&server.rx, &config_uri);
        let text = serde_json::to_string(&diagnostics).unwrap();
        assert!(!text.contains(secret), "the server leaked {secret}: {text}");
        assert!(text.contains("<redacted>"), "{text}");
        server.shutdown();
    }
}
