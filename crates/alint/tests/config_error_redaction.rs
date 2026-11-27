//! A configuration error never prints a value read from the environment or
//! set in a `sensitive: true` rule, on any surface: `check` in every format,
//! `validate-config`, `list`, `explain`, `fix`, `alint test` and the language
//! server. The unit tests in `alint-core` and `alint-dsl` pin each step; these
//! drive the real binary, so a call site that skips the redaction fails here.

use std::path::Path;
use std::process::Output;

/// A value interpolation re-types as a number: the error quotes it as one.
const ACCOUNT_ID: &str = "123456789012";

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn run(root: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    crate::hermetic::command(env!("CARGO_BIN_EXE_alint"))
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
fn assert_no_secret(surface: &str, output: &Output, secrets: &[&str]) {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for secret in secrets {
        assert!(!text.contains(secret), "{surface} leaked {secret}:\n{text}");
    }
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
        assert_no_secret(&args.join(" "), &output, &[ACCOUNT_ID]);
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
    assert_no_secret("hex message", &output, &["3735928559", "DEADBEEF"]);
}

#[test]
#[allow(clippy::too_many_lines)]
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

    // Nested configs are untrusted and reject `{{env.*}}` before reading the
    // environment at all, so neither paths nor top-level fields can leak.
    for (case, nested_yaml, value) in [
        (
            "nested absolute path",
            "version: 1\nrules:\n  - id: nested\n    kind: file_exists\n    \
             paths: '{{env.VALUE}}'\n    level: error\n",
            "/canary-secret/x",
        ),
        (
            "nested version",
            "version: '{{env.VALUE}}'\nrules: []\n",
            "987654",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "README.md", "hi\n");
        write(
            root.path(),
            ".alint.yml",
            "version: 1\nnested_configs: true\nrules: []\n",
        );
        write(root.path(), "sub/.alint.yml", nested_yaml);
        let output = run(root.path(), &["check"], &[("VALUE", value)]);
        assert!(!output.status.success(), "{case}: expected failure");
        assert_no_secret(case, &output, &[value, "canary-secret"]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("nested configs are untrusted and may not read the environment"),
            "{case}: unexpected stderr:\n{stderr}"
        );
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
        let status = crate::hermetic::git()
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
        let status = crate::hermetic::git()
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

    // The same holds for a `when_iter:` or `when:` inside a nested rule, whose
    // `require:` is masked as a whole.
    let deeper = |field: &str| {
        format!(
            "version: 1\nrules:\n  - id: outer\n    kind: for_each_dir\n    \
             select: 'crates/*'\n    level: error\n    require:\n      \
             - kind: for_each_file\n        select: '{{path}}/*'\n        \
             {field}: 'iter.basename != \"{{{{env.SKIP}}}}\"'\n        require:\n          \
             - kind: file_exists\n            paths: '{{path}}.lock'\n            colour: red\n"
        )
    };
    for field in ["when_iter", "when"] {
        write(root.path(), ".alint.yml", &deeper(field));
        let output = run(root.path(), &["check"], &[("SKIP", "secret-crate")]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("unknown field `colour`"),
            "{field}: {stderr}"
        );
        assert!(!stderr.contains("secret-crate"), "{field}: {stderr}");
    }
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

/// The error names the number it found (`invalid type: integer ...`), so a
/// sensitive rule's error is redacted whole.
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
             configuration_error:\n      message: {{ regex: '^rule \"scan\": <redacted> ' }}\n"
        ),
    );
    let output = run(root.path(), &["test", "--format", "json"], &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
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
        // Trusted: an untrusted workspace's config reads no environment value.
        server.send(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "rootUri": format!("file://{}", root_path.display()),
                "capabilities": {},
                "initializationOptions": { "trusted": true }
            }
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
        if secret != ACCOUNT_ID {
            assert!(text.contains("<redacted>"), "{text}");
        }
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
        "version: 1\nfacts:\n  - id: token\n    custom:\n      argv: ['printf', '%s', '{{env.VALUE}}']\n\
         rules: []\n",
    );
    for secret in ["canary-secret", r#"canary"sec\ret"#] {
        for args in [&["facts"][..], &["facts", "--format", "json"]] {
            let output = run(root.path(), args, &[("VALUE", secret)]);
            assert!(output.status.success(), "{args:?}: {output:?}");
            assert_left_out(
                &format!("{} ({secret})", args.join(" ")),
                &output,
                &[secret, "canary"],
            );
        }
    }
}

/// An `extends:` entry with an unsupported `<scheme>://` fails to load with
/// the scheme named, and no command prints its userinfo or query.
#[test]
fn an_unsupported_extends_scheme_fails_without_its_credentials() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join(".alint.yml"),
        "version: 1\nextends: [\"ftp://user:pw@host/x.yml?t=1\"]\nrules: []\n",
    )
    .unwrap();
    for args in [
        &["check"][..],
        &["validate-config"],
        &["--format", "json", "check"],
    ] {
        let output = run(root.path(), args, &[]);
        let surface = format!("{args:?}");
        assert!(!output.status.success(), "{surface}");
        assert_no_secret(&surface, &output, &["pw", "t=1"]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            format!("{stdout}{stderr}").contains("unsupported `extends:` scheme `ftp`"),
            "{surface}: {stdout}{stderr}"
        );
    }
}

/// Every surface that loads a config, in each format that differs.
const LOADING_SURFACES: [&[&str]; 8] = [
    &["check"],
    &["check", "--format", "json"],
    &["validate-config"],
    &["validate-config", "--format", "json"],
    &["list"],
    &["list", "--format", "json"],
    &["export-agents-md"],
    &["export-agents-md", "--format", "json"],
];

/// A version 2 relation's `JSONPath` read from the environment. The error's
/// hint quotes the path rewritten (`$['canary-secret']`), which removing the
/// value as written misses, so the error stands only when the config with the
/// value masked fails the same way, and is redacted otherwise.
#[test]
fn a_v2_relation_error_read_from_the_environment_stays_redacted() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "README.md", "hi\n");
    let config = |body: &str| {
        format!(
            "version: 2\nselectors:\n  docs: {{ paths: a.json }}\nrelations:\n  b:\n    from: \
             docs\n{body}rules: []\n"
        )
    };
    let env = [("AT", "$.canary-secret")];
    for body in [
        "    entries: { format: json, at: '{{env.AT | default(\"$.x\")}}' }\n    fields: { k: \
         entry.key }\n",
        "    read: { json: '{{env.AT | default(\"$.x\")}}' }\n    fields: { v: value }\n",
        "    read: { json: $.x }\n    fields: { v: value, n: 'bound: {{env.AT | \
         default(\"$.x\")}}' }\n",
    ] {
        write(root.path(), ".alint.yml", &config(body));
        for args in LOADING_SURFACES {
            let output = run(root.path(), args, &env);
            assert_redacted(&format!("{args:?} {body}"), &output, &["canary-secret"]);
            // `validate-config --format json` reports it on stdout.
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                text.contains(
                    ".alint.yml: `relations.b`: <redacted> (the error could quote a value read \
                     from the environment)"
                ),
                "{args:?} {body}\n{text}"
            );
        }
    }

    // An error the masked config repeats stays readable beside the value.
    write(
        root.path(),
        ".alint.yml",
        &config(
            "    entries: { format: json, at: '{{env.AT | default(\"$.x\")}}' }\n    fields: { \
             k: entry.key }\n    colour: red\n",
        ),
    );
    for args in LOADING_SURFACES {
        let output = run(root.path(), args, &[("AT", "$.canary")]);
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.status.success(), "{args:?}");
        assert!(
            text.contains(".alint.yml: `relations.b`: unknown key `colour` in a relation;"),
            "{args:?}\n{text}"
        );
    }
}

/// A version 2 listing names the file that declares each selector and
/// relation; one an `extends:` entry read from the environment loads is
/// shown redacted, as an error naming it would be.
#[test]
fn a_v2_source_path_read_from_the_environment_stays_out_of_listings() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "README.md", "hi\n");
    write(
        root.path(),
        "canary-secret.yml",
        "version: 2\nselectors:\n  docs: { paths: README.md }\nrules: []\n",
    );
    write(
        root.path(),
        ".alint.yml",
        "version: 2\nextends:\n  - url: '{{env.EXTENDS | default(\"./x.yml\")}}'\n    as: \
         deps\nselectors:\n  own: { paths: README.md }\nrules: []\n",
    );
    let env = [("EXTENDS", "./canary-secret.yml")];
    for args in &LOADING_SURFACES[4..] {
        let output = run(root.path(), args, &env);
        assert!(
            output.status.success(),
            "{args:?}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_left_out(&format!("{args:?}"), &output, &["canary-secret"]);
    }
    let stdout = String::from_utf8(run(root.path(), &["list"], &env).stdout).unwrap();
    assert!(
        stdout.ends_with(
            "\nselector docs  <redacted>\nselector own  .alint.yml\naliases <redacted>  as deps \
             in .alint.yml\n"
        ),
        "{stdout}"
    );
}

/// A diamond `extends:` reaching the same bundled v2 ruleset both literally
/// (`as: pub_ws`) and via `{{env.SECRET_RULESET}}` (`as: env_ws`) must produce
/// order-independent `<redacted>` provenance and never fuse `env_ws` onto
/// `pub_ws`'s alias list in `list`, `list --format json`, or `export-agents-md`.
#[test]
fn diamond_literal_and_env_v2_extends_redacts_order_independently_without_alias_oracle() {
    let env = [("SECRET_RULESET", "alint://bundled/js/workspace-deps@v2")];
    for (label, config) in [
        (
            "literal-first",
            "version: 2\nextends:\n  - url: alint://bundled/js/workspace-deps@v2\n    as: pub_ws\n  \
             - url: '{{env.SECRET_RULESET}}'\n    as: env_ws\nrules: []\n",
        ),
        (
            "env-first",
            "version: 2\nextends:\n  - url: '{{env.SECRET_RULESET}}'\n    as: env_ws\n  \
             - url: alint://bundled/js/workspace-deps@v2\n    as: pub_ws\nrules: []\n",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), ".alint.yml", config);
        for args in [
            &["list"][..],
            &["list", "--format", "json"][..],
            &["export-agents-md"][..],
        ] {
            let output = run(root.path(), args, &env);
            assert!(
                output.status.success(),
                "{label} {args:?}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_left_out(
                &format!("{label} {args:?}"),
                &output,
                &["workspace-deps@v2"],
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                !stdout.contains("env_ws"),
                "{label} {args:?} leaked alias correlation with env_ws:\n{stdout}"
            );
        }
    }
}

/// A source an `extends:` entry read from the environment loads tells where
/// the environment points, and so does every source it extends in turn:
/// `p.yml`'s own `extends: ./q.yml` was listed, and named in errors, as
/// `secret-dir/q.yml`. Each is `<redacted>`.
#[test]
fn a_source_below_an_extends_entry_read_from_the_environment_stays_out_of_output() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "README.md", "hi\n");
    write(
        root.path(),
        "secret-dir/p.yml",
        "version: 2\nextends:\n  - url: ./q.yml\n    as: q\nselectors:\n  own: { paths: \
         README.md }\nrules: []\n",
    );
    write(
        root.path(),
        "secret-dir/q.yml",
        "version: 2\nselectors:\n  docs: { paths: README.md }\nrules: []\n",
    );
    write(
        root.path(),
        ".alint.yml",
        "version: 2\nextends:\n  - url: '{{env.POLICY}}'\n    as: p\nrules: []\n",
    );
    let env = [("POLICY", "./secret-dir/p.yml")];
    let listed = run(root.path(), &["list"], &env);
    assert!(listed.status.success(), "{listed:?}");
    assert_left_out("list", &listed, &["secret-dir"]);
    let stdout = String::from_utf8(listed.stdout).unwrap();
    assert!(
        stdout.ends_with(
            "\nselector docs  <redacted>\nselector own  <redacted>\naliases <redacted>  as q in \
             <redacted>\naliases <redacted>  as p in .alint.yml\n"
        ),
        "{stdout}"
    );
    let listed = run(root.path(), &["list", "--format", "json"], &env);
    assert!(listed.status.success(), "{listed:?}");
    assert_left_out("list --format json", &listed, &["secret-dir"]);

    // An error in q.yml names it so too.
    write(
        root.path(),
        "secret-dir/q.yml",
        "version: 2\nselectors:\n  docs: { paths: README.md, colour: red }\nrules: []\n",
    );
    for args in LOADING_SURFACES {
        let output = run(root.path(), args, &env);
        assert_redacted(&format!("{args:?}"), &output, &["secret-dir"]);
    }
}

/// A source below an `extends:` entry read from the environment whose name is
/// shorter than four characters, which the text scrub leaves, is named
/// `<redacted>` all the same: `q`, which `sd/p.yml` extends as `../q`, was
/// listed, in JSON and Markdown too, and named in errors, as `q`.
#[test]
fn a_short_source_name_below_an_extends_entry_read_from_the_environment_stays_out() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "README.md", "hi\n");
    write(
        root.path(),
        "sd/p.yml",
        "version: 2\nextends:\n  - url: ../q\n    as: q\nselectors:\n  own: { paths: README.md \
         }\nrules: []\n",
    );
    write(
        root.path(),
        "q",
        "version: 2\nselectors:\n  docs: { paths: README.md }\nrules: []\n",
    );
    write(
        root.path(),
        ".alint.yml",
        "version: 2\nextends:\n  - url: '{{env.POLICY}}'\n    as: p\nrules: []\n",
    );
    let env = [("POLICY", "./sd/p.yml")];
    let listed = run(root.path(), &["list"], &env);
    assert!(listed.status.success(), "{listed:?}");
    let stdout = String::from_utf8(listed.stdout).unwrap();
    assert!(
        stdout.ends_with(
            "\nselector docs  <redacted>\nselector own  <redacted>\naliases <redacted>  as q in \
             <redacted>\naliases <redacted>  as p in .alint.yml\n"
        ),
        "{stdout}"
    );
    let listed = run(root.path(), &["list", "--format", "json"], &env);
    assert!(listed.status.success(), "{listed:?}");
    let json: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    let paths: Vec<&str> = json["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|source| source["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["<redacted>", "<redacted>", ".alint.yml"]);
    let exported = run(root.path(), &["export-agents-md"], &env);
    assert!(exported.status.success(), "{exported:?}");
    let markdown = String::from_utf8(exported.stdout).unwrap();
    assert!(
        markdown.contains("- selector `docs`: `<redacted>`") && !markdown.contains("`q`\n"),
        "{markdown}"
    );

    // An error in `q` names it so too.
    write(
        root.path(),
        "q",
        "version: 2\nselectors:\n  docs: { paths: README.md, colour: red }\nrules: []\n",
    );
    for args in LOADING_SURFACES {
        let output = run(root.path(), args, &env);
        assert_redacted(&format!("{args:?}"), &output, &["q: ", "q:`", "\"q\""]);
    }
}

/// Every position of a version 2 selector or relation that takes a value,
/// with `{S}` where the value is read from the environment: what the decode
/// checks, what the plan check compiles (globs, `scope_filter:` names and
/// manifests), and what it quotes from a field (a `bound:` path, a join's
/// copy of one, a literal).
const V2_POSITIONS: &[&str] = &[
    "selectors:\n  s:\n    paths: {S}\n",
    "selectors:\n  s:\n    paths: [{S}]\n",
    "selectors:\n  s:\n    paths: ['ok/**', {S}]\n",
    "selectors:\n  s:\n    paths: { include: {S}, exclude: x }\n",
    "selectors:\n  s:\n    paths: { include: x, exclude: {S} }\n",
    "selectors:\n  s:\n    paths: 'pre/{T}/post'\n",
    "selectors:\n  s:\n    paths: '*.json'\n    scope_filter: { has_ancestor: {S} }\n",
    "selectors:\n  s:\n    paths: '*.json'\n    scope_filter: { has_ancestor: [{S}] }\n",
    "selectors:\n  s:\n    paths: '*.json'\n    scope_filter: { has_ancestor: 'pre/{T}' }\n",
    "selectors:\n  s:\n    paths: {S}\n    scope_filter: { has_ancestor: {S} }\n",
    "selectors:\n  s:\n    paths: '*.json'\n    scope_filter: { include_manifest_paths: { source: \
     {S}, at: $.w } }\n",
    "selectors:\n  s:\n    paths: '*.json'\n    scope_filter: { include_manifest_paths: { source: \
     a.json, at: {S} } }\n",
    "selectors:\n  s:\n    paths: '*.json'\n    scope_filter: { exclude_manifest_paths: { source: \
     {S}, at: $.w } }\n",
    "selectors:\n  s:\n    paths: '*.json'\n    scope_filter: { exclude_manifest_paths: { source: \
     a.json, at: $.w, format: {S} } }\n",
    "{R}    entries: { format: {S}, at: $ }\n    fields: { k: entry.key }\n",
    "{R}    entries: { format: json, at: {S} }\n    fields: { k: entry.key }\n",
    "{R}    entries: { format: json, at: [$.a, {S}] }\n    fields: { k: entry.key }\n",
    "{R}    entries: { format: json, at: '$.{T}' }\n    fields: { k: entry.key }\n",
    "{R}    entries: { format: json, at: $, leaves: {S} }\n    fields: { k: entry.key }\n",
    "{R}    read: { json: {S} }\n    fields: { v: value }\n",
    "{R}    read: { json: '$.a.{T}' }\n    fields: { v: value }\n",
    "{R}    read: { yaml: {S} }\n    fields: { v: value }\n",
    "{R}    read: { toml: {S} }\n    fields: { v: value }\n",
    "{R}    read: { regex: {S} }\n    fields: { v: value }\n",
    "{R}    read: { regex: 'x({T})' }\n    fields: { v: value }\n",
    "{R}    read: { lines: { comment: {S} } }\n    fields: { v: value }\n",
    "{R}    read: { json: $.a }\n    optional: {S}\n    fields: { v: value }\n",
    "{R}    read: { json: $.a }\n    fields: { v: value, b: 'bound: {T}' }\n",
    "{R}    read: { json: $.a }\n    fields: { v: { source: value, sensitive: {S} } }\n",
    "{R}    read: { json: $.a }\n    fields: { v: value }\n    transform: { v: [{S}] }\n",
    "{R}    read: { json: $.a }\n    fields: { v: value }\n    where: { v: { matches: {S} } }\n",
    "{R}    read: { json: $.a }\n    fields: { v: value }\n    where: { v: { not_matches: {S} } \
     }\n",
    "{R}    read: { json: $.a }\n    fields: { v: value }\n    where: { v: { in: [{S}] } }\n",
    "{R}    read: { json: $.a }\n    fields: { v: path }\n    where: { v: { not_in: [{S}] } }\n",
    "{R}    entries: { format: json, at: $ }\n    fields: { v: entry.value }\n    where: { v: { \
     in: [{S}] } }\n",
    "{R}    read: { json: $.a }\n    fields: { v: 'bound: {T}' }\n    transform: { v: \
     [strip-root] }\n",
    "{R}    read: { json: $.a }\n    fields: { v: 'bound: {T}' }\n    transform: { v: \
     [semver-major] }\n",
    "{R}    read: { regex: 'x(.*)' }\n    fields: { v: value, b: 'bound: {T}' }\n",
    "relations:\n  r:\n    from: { paths: {S} }\n    read: { json: $.a }\n    fields: { v: value \
     }\n",
    "relations:\n  r:\n    from: { paths: '*.json', scope_filter: { has_ancestor: {S} } }\n    \
     read: { json: $.a }\n    fields: { v: value }\n",
    "selectors:\n  s:\n    paths: '*.json'\nrelations:\n  u:\n    union:\n      - { from: s, \
     entries: { format: json, at: {S} } }\n      - { from: s, entries: { format: json, at: $.b } \
     }\n    fields: { k: entry.key }\n",
    "selectors:\n  s:\n    paths: '*.json'\nrelations:\n  u:\n    union:\n      - { from: s, read: \
     { regex: {S} } }\n      - { from: s, read: { json: $.b } }\n    fields: { k: value }\n",
    "relations:\n  u:\n    union:\n      - { from: { paths: '*.json', scope_filter: { \
     has_ancestor: {S} } }, read: { json: $.a } }\n      - { from: { paths: {S} }, read: { json: \
     $.b } }\n    fields: { k: value }\n",
    "selectors:\n  s:\n    paths: '*.json'\nrelations:\n  u:\n    union:\n      - { from: s, read: \
     { json: $.a } }\n      - { from: s, read: { regex: 'x(.*)' } }\n    fields: { v: 'bound: \
     {T}' }\n",
    "selectors:\n  s:\n    paths: '*.json'\nrelations:\n  a:\n    from: s\n    read: { json: $.a \
     }\n    fields: { v: value, b: 'bound: {T}' }\n  c:\n    from: s\n    read: { json: $.b }\n    \
     fields: { w: value }\n  j:\n    join: { left: a, right: c, on: { v: w }, unique: right }\n    \
     fields: { x: left.b }\n    transform: { x: [strip-root] }\n",
    "selectors:\n  s:\n    paths: '*.json'\nrelations:\n  a:\n    from: s\n    read: { json: $.a \
     }\n    fields: { v: value }\n  c:\n    from: s\n    read: { json: $.b }\n    fields: { w: \
     value }\n  j:\n    join: { left: a, right: c, on: { v: w }, unique: {S} }\n    fields: { x: \
     left.v }\n",
];

/// Values that break each kind of value a position takes, long and short: a
/// `JSONPath` whose dashed key a hint rewrites, a path whose basename a
/// `has_ancestor:` hint quotes, a glob and a regex that do not compile, and
/// values under the four characters the whole-value scrub removes. Each
/// holds `canary`, or `QZ`, `Q/Z` or `$.Q`, which no output may print.
const V2_CANARIES: &[&str] = &[
    "$.canary-secret",
    "../canary-secret.json",
    "canary-secret/**/[",
    "(?P<canary",
    "[QZ",
    "Q/Z",
    "QZ",
    "$.Q",
];

/// A version 2 value read from the environment reaches no output of any
/// command, whole, cut or rewritten, wherever it is written and however it
/// breaks the config: an error that could quote it is redacted unless the
/// config with it masked fails the same way.
#[test]
fn a_v2_value_read_from_the_environment_reaches_no_output() {
    let root = tempfile::tempdir().unwrap();
    let cases: Vec<(usize, &str, &str)> = V2_POSITIONS
        .iter()
        .enumerate()
        .flat_map(|(index, position)| {
            V2_CANARIES
                .iter()
                .map(move |value| (index, *position, *value))
        })
        .collect();
    for (index, position) in V2_POSITIONS.iter().enumerate() {
        let dir = root.path().join(index.to_string());
        let body = position
            .replace(
                "{R}",
                "selectors:\n  s:\n    paths: '*.json'\nrelations:\n  r:\n    from: s\n",
            )
            .replace("{S}", "'{{env.S | default(\"x\")}}'")
            .replace("{T}", "{{env.S | default(\"x\")}}");
        write(
            &dir,
            ".alint.yml",
            &format!("version: 2\n{body}rules: []\n"),
        );
        write(
            &dir,
            "a.json",
            "{\"a\": \"x\", \"b\": \"y\", \"w\": [\"x\"]}\n",
        );
    }
    let threads = std::thread::available_parallelism().map_or(4, |count| count.get().min(8));
    let leaks: Vec<String> = std::thread::scope(|scope| {
        let workers: Vec<_> = cases
            .chunks(cases.len().div_ceil(threads))
            .map(|chunk| {
                let root = root.path();
                scope.spawn(move || {
                    let root_str = root.to_string_lossy();
                    let mut leaks = Vec::new();
                    for &(index, position, value) in chunk {
                        let dir = root.join(index.to_string());
                        for args in LOADING_SURFACES {
                            let output = run(&dir, args, &[("S", value)]);
                            let text = format!(
                                "{}{}",
                                String::from_utf8_lossy(&output.stdout),
                                String::from_utf8_lossy(&output.stderr)
                            );
                            // Strip the random tempdir root path so a tempdir
                            // suffix like `.tmp0hJIQZ` cannot false-positive on
                            // the `"QZ"` canary.
                            let scrubbed = text.replace(root_str.as_ref(), "<root>");
                            let lower = scrubbed.to_lowercase();
                            let leaked = ["anary", "ecret"].iter().any(|word| lower.contains(word))
                                || ["QZ", "Q/Z", "$.Q"]
                                    .iter()
                                    .any(|word| scrubbed.contains(word));
                            if leaked {
                                leaks.push(format!("{position:?} S={value:?} {args:?}:\n{text}"));
                            }
                        }
                    }
                    leaks
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect()
    });
    assert!(
        leaks.is_empty(),
        "{} leaks:\n{}",
        leaks.len(),
        leaks.join("\n")
    );
}

/// An error about one relation that quotes no value reads as written beside
/// another relation that reads one where a mask is no valid value (a
/// `format:`): the decode is rerun with only the erroring relation's values
/// masked, and a plan-check error only with those of the relations its
/// check reads.
#[test]
fn a_v2_error_beside_an_unmaskable_value_elsewhere_stays_readable() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "a.json", "{\"a\": \"x\"}\n");
    let config = |b: &str| {
        format!(
            "version: 2\nselectors:\n  s: {{ paths: a.json }}\nrelations:\n  a:\n    from: s\n    \
             entries: {{ format: '{{{{env.FORMAT | default(\"x\")}}}}', at: $ }}\n    fields: {{ \
             k: entry.key }}\n  b:\n    from: s\n    read: {{ json: $.a }}\n{b}rules: []\n"
        )
    };
    for (b, message) in [
        (
            "    fieldz: { v: value }\n",
            ".alint.yml: `relations.b`: unknown key `fieldz` in a relation; expected one of \
             `from`, `entries`, `records`, `read`, `optional`, `union`, `join`, `fields`, \
             `transform`, `where`",
        ),
        (
            "    fields: { v: value }\n    transform: { vv: [trim] }\n",
            ".alint.yml: `relations.b.transform.vv`: `vv` is not a field of relation `b`; its \
             fields are `v`",
        ),
    ] {
        write(root.path(), ".alint.yml", &config(b));
        for args in LOADING_SURFACES {
            let output = run(root.path(), args, &[("FORMAT", "json")]);
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(!output.status.success(), "{args:?}");
            assert!(text.contains(message), "{args:?}\n{text}");
        }
    }
}

/// Environment values shorter than four characters (which the text scrubber
/// skips) must stay out of baseline load/write/regeneration/refusal messages
/// and out of extended-rule fix refusal messages on environment-chosen config
/// files.
#[test]
fn short_environment_values_stay_out_of_baseline_and_fix_config_messages() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "a.txt", "bad\n");
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nbaseline: '{{env.B}}.json'\nrules:\n  - id: no-bad\n    \
         kind: file_content_forbidden\n    paths: '*.txt'\n    pattern: bad\n    level: error\n",
    );

    // 1. Missing baseline on `check` with a 2-char env value (`xy`).
    let out = run(root.path(), &["check"], &[("B", "xy")]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(stderr.contains("<redacted>"), "{stderr}");
    assert!(!stderr.contains("xy.json"), "{stderr}");

    // 2. Writing baseline (`alint baseline`) prints `<redacted>`, not `xy.json`.
    let out = run(root.path(), &["baseline"], &[("B", "xy")]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("wrote <redacted>"), "{stderr}");
    assert!(!stderr.contains("xy.json"), "{stderr}");
    assert!(root.path().join("xy.json").is_file());

    // 3. Regeneration guard (`alint baseline` after adding a new violation).
    write(root.path(), "b.txt", "bad\n");
    let out = run(root.path(), &["baseline"], &[("B", "xy")]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(stderr.contains("regenerating <redacted>"), "{stderr}");
    assert!(!stderr.contains("xy.json"), "{stderr}");

    // 4. Out-of-root `..` baseline refusal via `alint baseline`.
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nbaseline: '../{{env.B}}.json'\nrules: []\n",
    );
    let out = run(root.path(), &["baseline"], &[("B", "xy")]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(stderr.contains("baseline <redacted>"), "{stderr}");
    assert!(!stderr.contains("xy.json"), "{stderr}");

    // 5. Extended-rule fix refusing to write an environment-chosen config file
    //    named with a 2-char env value (`xy.yml`) and a file it extends
    //    (`sub.yml`): refusal details structurally name `<redacted>` instead of
    //    `xy.yml` or `sub.yml`.
    let fix_root = tempfile::tempdir().unwrap();
    write(fix_root.path(), "sub.yml", "version: 1\nrules: []\n");
    write(
        fix_root.path(),
        "xy.yml",
        "version: 1\nextends: ['./sub.yml']\nrules:\n  - id: fix-ext\n    \
         kind: file_content_forbidden\n    paths: '**/*.yml'\n    pattern: version\n    \
         level: error\n    fix:\n      replace:\n        replacement: ver\n",
    );
    write(
        fix_root.path(),
        ".alint.yml",
        "version: 1\nextends: ['./{{env.X}}.yml']\nrules: []\n",
    );
    let out = run(
        fix_root.path(),
        &["fix", "--unsafe-fixes", "--format", "json"],
        &[("X", "xy")],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("invalid json ({e}):\nstdout:\n{stdout}\nstderr:\n{stderr}"));
    let details: Vec<&str> = parsed["results"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|r| r["items"].as_array().into_iter().flatten())
        .filter_map(|item| item["detail"].as_str())
        .collect();
    let redacted_count = details
        .iter()
        .filter(|detail| detail.contains("<redacted> is an alint configuration file"))
        .count();
    assert_eq!(
        redacted_count, 2,
        "expected structural `<redacted>` refusal details for both `xy.yml` and `sub.yml`, got {details:?}\nfull:\n{stdout}"
    );
    assert!(
        !stdout.contains("xy.yml")
            && !stdout.contains("sub.yml")
            && !stderr.contains("xy.yml")
            && !stderr.contains("sub.yml"),
        "fix JSON output must not leak env-chosen config paths anywhere:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    for args in [
        &["fix", "--unsafe-fixes"][..],
        &["fix", "--unsafe-fixes", "--format", "markdown"][..],
        &["fix", "--unsafe-fixes", "--dry-run"][..],
    ] {
        let out = run(fix_root.path(), args, &[("X", "xy")]);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stdout.contains("xy.yml")
                && !stdout.contains("sub.yml")
                && !stderr.contains("xy.yml")
                && !stderr.contains("sub.yml"),
            "{args:?} must not leak env-chosen config paths:\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }

    // 6. Short (<4-char) env value in `extends:` `only:` / `except:` rule
    //    filter and out-of-root extends target stay structurally masked.
    write(
        fix_root.path(),
        ".alint.yml",
        "version: 1\nextends:\n  - url: './sub.yml'\n    only: ['{{env.R}}']\nrules: []\n",
    );
    let out = run(fix_root.path(), &["check"], &[("R", "ab")]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(stderr.contains("<redacted>"), "{stderr}");
    assert!(!stderr.contains("\"ab\""), "{stderr}");

    let parent_dir = tempfile::tempdir().unwrap();
    let repo_dir = parent_dir.path().join("repo");
    std::fs::create_dir(&repo_dir).unwrap();
    write(parent_dir.path(), "ab.yml", "version: 1\nrules: []\n");
    write(
        &repo_dir,
        ".alint.yml",
        "version: 1\nextends: ['../{{env.X}}.yml']\nrules: []\n",
    );
    let out = run(&repo_dir, &["check"], &[("X", "ab")]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(
        stderr.contains("resolves to <redacted> which is outside the lint root"),
        "{stderr}"
    );
    assert!(!stderr.contains("ab.yml"), "{stderr}");
}

#[test]
fn overlapping_env_secrets_and_nested_since_literal_and_xdg_cache_offline_hit() {
    // 1. Overlapping env secrets (`abcd` and `cdefgh`) are coalesced cleanly so
    //    no trailing suffix (`efgh`) leaks in config errors (#585).
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        ".alint.yml",
        "version: 1\nignore:\n  - \"{{env.S1}}\"\n  - \"[{{env.S2}}\"\nrules: []\n",
    );
    let out = run(
        dir.path(),
        &["check"],
        &[("S1", "abcd"), ("S2", "abcdefgh")],
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("abcd") && !stderr.contains("efgh"),
        "overlapping secret suffix leaked: {stderr}"
    );

    // 2. Short 2-char env config filename in `alint fix` refusal must neither
    //    leak the 2-char token in `violation.message` nor clobber `item.status`
    //    (Audit F18).
    let short_dir = tempfile::tempdir().unwrap();
    write(
        short_dir.path(),
        "ab.yml",
        r#"version: 1
rules:
  - id: forbid-yml
    kind: file_absent
    paths: "a*.yml"
    level: error
    fix:
      file_remove: {}
"#,
    );
    write(
        short_dir.path(),
        ".alint.yml",
        "version: 1\nextends: ['./{{env.X}}.yml']\nrules: []\n",
    );
    let out = run(
        short_dir.path(),
        &["fix", "--unsafe-fixes", "--format", "json"],
        &[("X", "ab")],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let item = &parsed["results"][0]["items"][0];
    assert!(
        item["detail"]
            .as_str()
            .unwrap()
            .contains("<redacted> is an alint configuration file"),
        "detail clobbered: {item}"
    );
    assert!(
        !stdout.contains("ab.yml"),
        "short env config filename leaked: {stdout}"
    );

    // 3. Offline remote `extends:` cache lookup respects `XDG_CACHE_HOME` (#611).
    let cache_root = tempfile::tempdir().unwrap();
    let xdg_dir = cache_root.path().join("xdg");
    let ruleset_body = "version: 1\nrules: []\n";
    let digest = "sha256-7f2c43106364d8112fcc3060e4ab8b20f5d1a17d9cbf3c2540529663004adff8";
    let xdg_repo = cache_root.path().join("repo");
    write(
        &xdg_repo,
        ".alint.yml",
        &format!("version: 1\nextends: ['https://example.invalid/r.yml#{digest}']\nrules: []\n"),
    );
    // Seed exact cached file at `$XDG_CACHE_HOME/alint/rulesets/<sri>.yml`
    // (on Linux; macOS uses Library/Caches instead of XDG_CACHE_HOME).
    let cache_sub = if cfg!(target_os = "macos") {
        None
    } else if cfg!(unix) {
        Some(xdg_dir.join("alint").join("rulesets"))
    } else {
        None
    };
    if let Some(rulesets_dir) = cache_sub {
        std::fs::create_dir_all(&rulesets_dir).unwrap();
        std::fs::write(rulesets_dir.join(format!("{digest}.yml")), ruleset_body).unwrap();
        let out = crate::hermetic::command(env!("CARGO_BIN_EXE_alint"))
            .current_dir(&xdg_repo)
            .args(["validate-config"])
            .env("XDG_CACHE_HOME", &xdg_dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "expected XDG_CACHE_HOME cache lookup to serve seeded ruleset offline: stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }
}
