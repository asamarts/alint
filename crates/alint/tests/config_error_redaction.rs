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
    assert!(
        !output.status.success(),
        "{surface}: expected a configuration error:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_left_out(surface, output, secrets);
}

/// `output` says `<redacted>` and never quotes any of `secrets`.
fn assert_left_out(surface: &str, output: &Output, secrets: &[&str]) {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
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
        // A glob parser trims the pattern before quoting it.
        (
            "ignore with trailing whitespace",
            "ignore: ['{{env.VALUE}}']\n",
            None,
            "!canary-secret[  \n",
        ),
        // A regex error quotes a multi-line pattern one line at a time.
        (
            "multi-line fact pattern",
            "facts:\n  - id: f\n    file_content_matches:\n      paths: README.md\n      \
             pattern: '{{env.VALUE}}'\n",
            None,
            "canary-secret-line\n(canary-secret",
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

    // A parse error in an extended file names it by its normalized path,
    // which does not contain the entry as written.
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "sub/README.md", "hi\n");
    write(root.path(), "canary-secret.yml", "version: 1\nrules: [\n");
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nextends: ['{{env.VALUE}}']\nrules: []\n",
    );
    for entry in ["sub/../canary-secret.yml", ".//canary-secret.yml"] {
        let output = run(root.path(), &["check"], &[("VALUE", entry)]);
        assert_redacted(entry, &output, &["canary-secret"]);
    }

    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        ".alint.yml",
        "version: '{{env.VALUE}}'\nrules: []\n",
    );
    let output = run(root.path(), &["check"], &[("VALUE", "987654")]);
    assert_redacted("version", &output, &["987654"]);

    // A nested config's environment values are its own.
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nnested_configs: true\nrules: []\n",
    );
    write(
        root.path(),
        "sub/.alint.yml",
        "version: '{{env.VALUE}}'\nrules: []\n",
    );
    let output = run(root.path(), &["check"], &[("VALUE", "987654")]);
    assert_redacted("nested version", &output, &["987654"]);
}

#[test]
fn a_value_read_from_the_environment_stays_out_of_warnings_and_run_errors() {
    const README: &str = "rules:\n  - id: readme\n    kind: file_exists\n    \
                          paths: README.md\n    level: error\n";
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "README.md", "hi\n");

    // A pattern hint names the fact.
    write(
        root.path(),
        ".alint.yml",
        &format!(
            "version: 1\nfacts:\n  - id: '{{{{env.VALUE}}}}'\n    file_content_matches:\n      \
             paths: README.md\n      pattern: 'hi$'\n{README}"
        ),
    );
    for args in [
        &["check"][..],
        &["validate-config"],
        &["validate-config", "--format", "json"],
        &["facts"],
        &["facts", "--format", "json"],
    ] {
        let output = run(root.path(), args, &[("VALUE", "canary-fact")]);
        assert!(output.status.success(), "{args:?}");
        assert_left_out(&args.join(" "), &output, &["canary-fact"]);
    }

    // An empty manifest set warns, naming the manifest.
    write(
        root.path(),
        "canary-manifest.toml",
        "[workspace]\nmembers = []\n",
    );
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nrules:\n  - id: scoped\n    kind: file_content_forbidden\n    \
         paths: '**/*'\n    pattern: zzz\n    scope_filter:\n      include_manifest_paths:\n        \
         source: '{{env.VALUE}}'\n        extract: { toml: '$.workspace.members[*]' }\n    \
         level: error\n",
    );
    let output = run(
        root.path(),
        &["check"],
        &[("VALUE", "canary-manifest.toml")],
    );
    assert!(output.status.success());
    assert_left_out("manifest warning", &output, &["canary-manifest"]);

    // An unknown template is quoted by its error.
    write(
        root.path(),
        ".alint.yml",
        "version: 1\ntemplates:\n  - id: has-readme\n    kind: file_exists\n    \
         paths: README.md\n    level: error\nrules:\n  - id: instance\n    \
         extends_template: '{{env.VALUE}}'\n",
    );
    let output = run(root.path(), &["check"], &[("VALUE", "canary-template")]);
    assert_redacted("extends_template", &output, &["canary-template"]);
    // A known template is named as the source of a field it supplies.
    write(
        root.path(),
        ".alint.yml",
        "version: 1\ntemplates:\n  - id: canary-template\n    kind: git_commit_no_fixup\n    \
         paths: README.md\n    level: error\nrules:\n  - id: instance\n    \
         extends_template: '{{env.VALUE}}'\n",
    );
    let output = run(root.path(), &["check"], &[("VALUE", "canary-template")]);
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success()
            && !text.contains("canary-template")
            && text.contains("supplied by a template"),
        "template attribution:\n{text}"
    );

    // Git quotes a ref it cannot resolve.
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(root.path())
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&["add", "README.md"]);
    git(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-qm",
        "one",
    ]);
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nrules:\n  - id: scoped\n    kind: file_content_forbidden\n    \
         paths: '*.md'\n    pattern: zzz\n    scope_filter:\n      \
         changed_since: '{{env.VALUE}}'\n    level: error\n",
    );
    // `a:b` is a path in a tree, and git's error names only `a`.
    for value in ["canary-ref", "canary-ref:part"] {
        let output = run(root.path(), &["check"], &[("VALUE", value)]);
        assert_redacted("changed_since", &output, &["canary-ref"]);
    }
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nrules:\n  - id: message\n    kind: git_commit_message\n    \
         requires_body: true\n    since: '${VALUE}'\n    level: error\n",
    );
    // The reference is shown as written, and git's message left out.
    for value in ["canary-ref", "canary-ref:part"] {
        let output = run(root.path(), &["check"], &[("VALUE", value)]);
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(
            !text.contains("canary-ref")
                && text.contains("`${VALUE}..HEAD`: git's message is left out"),
            "legacy since:\n{text}"
        );
    }
}

#[test]
fn a_nested_rule_value_read_from_the_environment_stays_out_of_warnings_and_run_errors() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "pkg/a/README.md", "hi\n");
    let nested = |rule: &str| {
        format!(
            "version: 1\nrules:\n  - id: each\n    kind: for_each_dir\n    select: 'pkg/*'\n    \
             level: error\n{rule}"
        )
    };

    // An empty manifest set warns, naming the manifest.
    write(
        root.path(),
        "canary-manifest.toml",
        "[workspace]\nmembers = []\n",
    );
    write(
        root.path(),
        ".alint.yml",
        &nested(
            "    require:\n      - kind: file_content_forbidden\n        paths: '{path}/*.md'\n        \
             pattern: zzz\n        scope_filter:\n          include_manifest_paths:\n            \
             source: '{{env.VALUE}}'\n            extract: { toml: '$.workspace.members[*]' }\n",
        ),
    );
    let output = run(
        root.path(),
        &["check"],
        &[("VALUE", "canary-manifest.toml")],
    );
    assert!(output.status.success(), "{output:?}");
    assert_left_out("nested manifest warning", &output, &["canary-manifest"]);

    // Git quotes a ref it cannot resolve.
    for args in [
        &["init", "-q"][..],
        &["add", "pkg"],
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "one",
        ],
    ] {
        let status = Command::new("git")
            .arg("-C")
            .arg(root.path())
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }
    let changed_since = |since: &str, parent: &str| {
        nested(&format!(
            "{parent}    require:\n      - kind: file_content_forbidden\n        \
             paths: '{{path}}/*.md'\n        pattern: zzz\n        scope_filter:\n          \
             changed_since: '{since}'\n"
        ))
    };
    write(
        root.path(),
        ".alint.yml",
        &changed_since("{{env.VALUE}}", ""),
    );
    for value in ["canary-ref", "canary-ref:part"] {
        let output = run(root.path(), &["check"], &[("VALUE", value)]);
        assert_redacted("nested changed_since", &output, &["canary-ref"]);
    }
    // A `sensitive: true` parent's nested ref is secret as written.
    write(
        root.path(),
        ".alint.yml",
        &changed_since("canary-ref", "    sensitive: true\n"),
    );
    let output = run(root.path(), &["check"], &[]);
    assert_redacted("sensitive parent", &output, &["canary-ref"]);
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

    // A number at the edge of its range fails its check when the rerun moves
    // it up, so the rerun also moves it down.
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nrules:\n  - id: commented\n    kind: commented_out_code\n    \
         paths: '**/*.rs'\n    threshold: 1.0\n    sensitive: true\n    level: error\n    \
         scope_filter:\n      colour: red\n",
    );
    let output = run(root.path(), &["check"], &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown field `colour`"), "{stderr}");
    assert!(!stderr.contains("<redacted>"), "{stderr}");

    // A JSONPath read from the environment is masked with one that parses, so
    // the rerun reaches the same later error; a bad one stays redacted.
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nrules:\n  - id: absent\n    kind: json_path_absent\n    \
         paths: 'c.json['\n    path: '{{env.QUERY}}'\n    level: error\n",
    );
    let output = run(root.path(), &["check"], &[("QUERY", "$.secretkey")]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid glob \"c.json[\""), "{stderr}");
    assert!(!stderr.contains("secretkey"), "{stderr}");
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nrules:\n  - id: absent\n    kind: json_path_absent\n    \
         paths: c.json\n    path: '{{env.QUERY}}'\n    level: error\n",
    );
    let output = run(root.path(), &["check"], &[("QUERY", "$.[secret-key")]);
    assert_redacted("bad JSONPath", &output, &["secret-key"]);

    // A `when_iter:` is parsed before the nested rules, so it is masked with
    // an expression that parses; a bad one stays redacted.
    let nested = |when_iter: &str| {
        format!(
            "version: 1\nrules:\n  - id: each\n    kind: for_each_dir\n    \
             select: 'crates/*'\n    when_iter: '{when_iter}'\n    level: error\n    \
             require:\n      - kind: file_exists\n        paths: '{{path}}/Cargo.toml'\n        \
             colour: red\n"
        )
    };
    write(
        root.path(),
        ".alint.yml",
        &nested(r#"iter.basename == "{{env.WANTED}}""#),
    );
    let output = run(root.path(), &["check"], &[("WANTED", "secret-crate")]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown field `colour`"), "{stderr}");
    assert!(!stderr.contains("secret-crate"), "{stderr}");
    write(root.path(), ".alint.yml", &nested("{{env.CONDITION}}"));
    let output = run(
        root.path(),
        &["check"],
        &[("CONDITION", "iter.basename == secret-crate")],
    );
    assert_redacted("bad when_iter", &output, &["secret-crate"]);
}

#[test]
fn a_path_read_from_the_environment_stays_out_of_fix_warnings() {
    const TRIM: &str = "    fix:\n      file_trim_trailing_whitespace: {}\n";
    let env = [("VALUE", "canary-file.rs")];
    let root = tempfile::tempdir().unwrap();
    let stderr = |output: &Output| String::from_utf8_lossy(&output.stderr).into_owned();

    // A file over `fix_size_limit` is skipped with a warning naming it.
    write(root.path(), "canary-file.rs", "fn a() {}   \n");
    write(
        root.path(),
        ".alint.yml",
        &format!(
            "version: 1\nfix_size_limit: 4\nrules:\n  - id: trim\n    \
             kind: no_trailing_whitespace\n    paths: '{{{{env.VALUE}}}}'\n    level: error\n{TRIM}"
        ),
    );
    let output = run(root.path(), &["fix"], &env);
    assert!(
        stderr(&output).contains("<redacted> is 13 bytes; exceeds fix_size_limit"),
        "{}",
        stderr(&output)
    );
    assert_left_out("fix_size_limit", &output, &["canary-file"]);
    // `check` composes the same fixes for the formats that carry them.
    let output = run(root.path(), &["check", "--format", "sarif"], &env);
    assert!(
        stderr(&output).contains("<redacted> is 13 bytes"),
        "{}",
        stderr(&output)
    );
    assert_left_out("fix_size_limit in sarif", &output, &["canary-file"]);

    // A `replace` whose replacement still matches is left unfixed with a
    // warning naming the file.
    write(root.path(), "canary-file.rs", "zzz\n");
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nrules:\n  - id: no-z\n    kind: file_content_forbidden\n    \
         paths: '{{env.VALUE}}'\n    pattern: zzz\n    level: error\n    fix:\n      \
         replace:\n        replacement: zzzz\n        applicability: safe\n",
    );
    let output = run(root.path(), &["fix"], &env);
    assert!(
        stderr(&output).contains("<redacted>: 1 `replace` match(es) left unfixed"),
        "{}",
        stderr(&output)
    );
    assert_left_out("replace", &output, &["canary-file"]);

    // A write that fails names the file, and the I/O error names it again.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        write(root.path(), "canary-file.rs", "fn a() {}   \n");
        std::fs::set_permissions(
            root.path().join("canary-file.rs"),
            std::fs::Permissions::from_mode(0o444),
        )
        .unwrap();
        write(
            root.path(),
            ".alint.yml",
            &format!(
                "version: 1\nrules:\n  - id: trim\n    kind: no_trailing_whitespace\n    \
                 paths: '{{{{env.VALUE}}}}'\n    level: error\n{TRIM}"
            ),
        );
        let output = run(root.path(), &["fix"], &env);
        assert!(
            stderr(&output).contains("could not write <redacted>"),
            "{}",
            stderr(&output)
        );
        assert_left_out("write failure", &output, &["canary-file"]);
    }
}

#[test]
fn a_pattern_that_may_be_secret_raises_no_hint_and_no_quoted_warning() {
    const HINT: &str = "matches only at the very end";
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "README.md", "hi\n");
    let output_text = |output: &Output| {
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    };

    // Whether a hint appears says how the pattern ends. A literal pattern
    // still gets its hint.
    let rule = |pattern: &str, extra: &str| {
        format!(
            "version: 1\nrules:\n  - id: anchored\n    kind: file_content_matches\n    \
             paths: README.md\n    pattern: '{pattern}'\n{extra}    level: error\n"
        )
    };
    let fact = "version: 1\nfacts:\n  - id: anchored\n    file_content_matches:\n      \
                paths: README.md\n      pattern: '{{env.VALUE}}'\nrules: []\n";
    for (config, hinted) in [
        (rule("hi$", ""), true),
        (rule("{{env.VALUE}}", ""), false),
        (rule("hi$", "    sensitive: true\n"), false),
        (fact.to_string(), false),
    ] {
        write(root.path(), ".alint.yml", &config);
        for args in [&["check"][..], &["validate-config"]] {
            let output = run(root.path(), args, &[("VALUE", "canary$")]);
            assert_eq!(
                output_text(&output).contains(HINT),
                hinted,
                "{args:?}\n{config}\n{}",
                output_text(&output)
            );
        }
    }

    // The deprecation warning for a `${VAR}` in `since:` quotes the field.
    for (since, extra) in [
        ("{{env.VALUE}}", ""),
        ("${CANARY_REF}", "    sensitive: true\n"),
    ] {
        write(
            root.path(),
            ".alint.yml",
            &format!(
                "version: 1\nrules:\n  - id: message\n    kind: git_commit_message\n    \
                 requires_body: true\n    since: '{since}'\n{extra}    level: error\n"
            ),
        );
        let output = run(root.path(), &["check"], &[("VALUE", "canary-${X}")]);
        let text = output_text(&output);
        assert!(
            text.contains("`since:` uses the deprecated")
                && !text.to_lowercase().contains("canary"),
            "{since}:\n{text}"
        );
    }
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
    // A valid config whose pattern hint names a fact read from the environment.
    let fact_hint = format!(
        "version: 1\nfacts:\n  - id: '{{{{env.VALUE}}}}'\n    file_content_matches:\n      \
         paths: README.md\n      pattern: 'hi$'\n{README}"
    );
    for (config, env, secret) in [
        (baseline.as_str(), "canary-secret.jsonl", "canary-secret"),
        (ignore.as_str(), "!canary-secret[", "canary-secret"),
        (facts.as_str(), "canary-secret[", "canary-secret"),
        (fact_hint.as_str(), "canary-fact", "canary-fact"),
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

#[test]
fn the_baseline_message_leaves_out_a_path_read_from_the_environment() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nbaseline: '{{env.VALUE}}.jsonl'\nrules:\n  - id: readme\n    \
         kind: file_exists\n    paths: README.md\n    level: error\n",
    );
    let output = run(root.path(), &["baseline"], &[("VALUE", "canary-secret")]);
    assert!(output.status.success(), "{output:?}");
    assert_left_out("baseline", &output, &["canary-secret"]);
    assert!(root.path().join("canary-secret.jsonl").is_file());
}

#[cfg(unix)]
#[test]
fn a_custom_fact_that_prints_an_environment_value_shows_it_redacted() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nfacts:\n  - id: token\n    custom:\n      argv: ['echo', '{{env.VALUE}}']\n\
         rules: []\n",
    );
    for args in [&["facts"][..], &["facts", "--format", "json"]] {
        let output = run(root.path(), args, &[("VALUE", "canary-secret")]);
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert_left_out(&args.join(" "), &output, &["canary-secret"]);
    }
}
