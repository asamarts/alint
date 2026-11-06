//! W5 leak gate: secret-bearing findings remain present, but no public report
//! surface may serialize their evidence bytes.

use std::path::Path;
use std::process::{Command, Output};

const DOTENV_SECRET: &str = "DOTENV_TOKEN_VALUE_DO_NOT_REPORT";
const PROPERTIES_SECRET: &str = "PROPERTIES_TOKEN_VALUE_DO_NOT_REPORT";
const EXPLICIT_SECRET: &str = "EXPLICIT_TOKEN_VALUE_DO_NOT_REPORT";
const RELATED_PATH_SECRET: &str = "RELATED_PATH_VALUE_DO_NOT_REPORT";
const ENV_PATH_SECRET: &str = "ENV_SELECTOR_PATH_DO_NOT_REPORT";

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_alint"))
        .args(args)
        .current_dir(root)
        .env("NO_COLOR", "1")
        .env("ENV_PATH_SECRET", ENV_PATH_SECRET)
        .output()
        .unwrap()
}

fn all_output(output: &Output) -> Vec<u8> {
    let mut bytes = output.stdout.clone();
    bytes.extend_from_slice(&output.stderr);
    bytes
}

fn assert_redacted(surface: &str, bytes: &[u8]) {
    let rendered = String::from_utf8_lossy(bytes);
    for secret in [
        DOTENV_SECRET,
        PROPERTIES_SECRET,
        EXPLICIT_SECRET,
        RELATED_PATH_SECRET,
        ENV_PATH_SECRET,
    ] {
        assert!(
            !rendered.contains(secret),
            "{surface} leaked {secret}:\n{rendered}"
        );
    }
    let marker_count =
        rendered.matches("<redacted:").count() + rendered.matches("&lt;redacted:").count();
    assert!(
        marker_count >= 3,
        "{surface} dropped one or more redacted findings ({marker_count}/3 markers):\n{rendered}"
    );
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".env"),
        format!("TOKEN={DOTENV_SECRET}\nPATH={RELATED_PATH_SECRET}\n"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("app.properties"),
        format!("TOKEN={PROPERTIES_SECRET}\n"),
    )
    .unwrap();
    std::fs::write(dir.path().join("plain.txt"), EXPLICIT_SECRET).unwrap();
    std::fs::write(
        dir.path().join(format!("{ENV_PATH_SECRET}.txt")),
        "forbidden \n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".scope.env"),
        format!("FILES={ENV_PATH_SECRET}.txt\n"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        format!(
            r#"version: 1
rules:
  - id: dotenv-secret
    kind: dotenv_path_equals
    paths: .env
    path: $.TOKEN
    equals: expected
    level: error
    fix: {{ set_value: {{}} }}
  - id: properties-secret
    kind: properties_path_equals
    paths: app.properties
    path: $.TOKEN
    equals: expected
    level: error
    fix: {{ set_value: {{}} }}
  - id: sensitive-registry-target
    kind: registry_paths_resolve
    source: .env
    extract: {{ dotenv: $.PATH }}
    base: lint_root
    level: error
  - id: env-selector-path
    kind: file_content_forbidden
    paths: "{{{{env.ENV_PATH_SECRET}}}}.txt"
    pattern: forbidden
    level: error
    fix: {{ replace: {{ replacement: allowed }} }}
  - id: sensitive-manifest-path
    kind: no_trailing_whitespace
    paths: "**/*.txt"
    scope_filter:
      include_manifest_paths:
        source: .scope.env
        extract: {{ dotenv: $.FILES }}
    level: error
  - id: explicit-secret
    kind: file_content_forbidden
    paths: plain.txt
    pattern: {EXPLICIT_SECRET}
    message: "explicit evidence: {EXPLICIT_SECRET}"
    sensitive: true
    level: error
"#,
        ),
    )
    .unwrap();
    dir
}

#[test]
fn every_report_baseline_and_fix_surface_is_secret_free() {
    let dir = fixture();
    for format in [
        "human", "json", "sarif", "github", "gitlab", "markdown", "junit", "agent",
    ] {
        let output = run(dir.path(), &["check", ".", "--format", format]);
        assert_eq!(output.status.code(), Some(1), "{format}: {output:?}");
        assert_redacted(format, &all_output(&output));
    }

    for format in ["markdown", "json"] {
        let output = run(dir.path(), &["export-agents-md", "--format", format]);
        assert!(output.status.success(), "{format}: {output:?}");
        let rendered = String::from_utf8_lossy(&output.stdout);
        assert!(
            !rendered.contains(ENV_PATH_SECRET),
            "export-agents-md {format} leaked the selector path:\n{rendered}"
        );
        assert!(rendered.contains("<redacted>"), "{rendered}");
    }

    let baseline = run(
        dir.path(),
        &["baseline", "--output", ".alint-baseline.json", "."],
    );
    assert!(baseline.status.success(), "{baseline:?}");
    let baseline_bytes = std::fs::read(dir.path().join(".alint-baseline.json")).unwrap();
    assert_redacted("baseline", &baseline_bytes);

    for format in ["human", "json"] {
        let output = run(dir.path(), &["fix", "--dry-run", "--format", format, "."]);
        assert_eq!(output.status.code(), Some(1), "fix {format}: {output:?}");
        assert_redacted(&format!("fix {format}"), &all_output(&output));
    }

    for format in ["human", "json"] {
        let fix_dir = fixture();
        let output = run(
            fix_dir.path(),
            &["fix", "--unsafe-fixes", "--format", format, "."],
        );
        assert_eq!(output.status.code(), Some(1), "fix {format}: {output:?}");
        assert_redacted(&format!("fix {format}"), &all_output(&output));
        assert!(
            !std::fs::read_to_string(fix_dir.path().join(".env"))
                .unwrap()
                .contains(DOTENV_SECRET)
        );
        assert!(
            !std::fs::read_to_string(fix_dir.path().join("app.properties"))
                .unwrap()
                .contains(PROPERTIES_SECRET)
        );
        assert!(
            !std::fs::read_to_string(fix_dir.path().join(format!("{ENV_PATH_SECRET}.txt")))
                .unwrap()
                .contains("forbidden"),
            "path redaction must not change the fix target"
        );
    }
}

#[test]
fn sensitive_fix_diff_omits_secret_bearing_file_edits() {
    let dir = fixture();
    let output = run(dir.path(), &["fix", "--diff", "."]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let rendered = String::from_utf8_lossy(&output.stdout);
    assert!(
        rendered.is_empty(),
        "all previewable edits in this fixture are sensitive and must be omitted:\n{rendered}"
    );
    for secret in [
        DOTENV_SECRET,
        PROPERTIES_SECRET,
        EXPLICIT_SECRET,
        ENV_PATH_SECRET,
    ] {
        assert!(
            !all_output(&output)
                .windows(secret.len())
                .any(|window| window == secret.as_bytes()),
            "fix --diff leaked {secret}: {output:?}"
        );
    }
}

#[test]
fn explain_hides_a_fix_description_built_from_an_environment_value() {
    // Each fixer describes itself from a field other than `fix:`.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        r#"version: 1
rules:
  - id: create-file
    kind: file_exists
    paths: "{{env.ENV_PATH_SECRET}}.txt"
    level: error
    fix:
      file_create: { content: "x\n" }
  - id: create-dir
    kind: dir_exists
    paths: "{{env.ENV_PATH_SECRET}}-dir"
    level: error
    fix:
      dir_create: {}
  - id: replace-pattern
    kind: file_content_forbidden
    paths: "*.md"
    pattern: "{{env.ENV_PATH_SECRET}}"
    level: error
    fix:
      replace: { replacement: "ok" }
  - id: set-path
    kind: json_path_equals
    paths: package.json
    path: "$['{{env.ENV_PATH_SECRET}}']"
    equals: 1
    level: error
    fix:
      set_value: {}
  - id: plain-create
    kind: file_exists
    paths: LICENSE
    level: error
    fix:
      file_create: { content: "MIT\n" }
"#,
    )
    .unwrap();

    for id in ["create-file", "create-dir", "replace-pattern", "set-path"] {
        for format in ["human", "json"] {
            let output = run(dir.path(), &["explain", id, "--format", format]);
            assert!(output.status.success(), "{id} {format}: {output:?}");
            let rendered = String::from_utf8_lossy(&output.stdout);
            assert!(
                !rendered.contains(ENV_PATH_SECRET),
                "explain {id} {format} leaked the value:\n{rendered}"
            );
            assert!(
                rendered.contains("<redacted>"),
                "{id} {format}:\n{rendered}"
            );
        }
    }

    // A rule with nothing from the environment keeps its description.
    let output = run(dir.path(), &["explain", "plain-create"]);
    assert!(output.status.success(), "{output:?}");
    let rendered = String::from_utf8_lossy(&output.stdout);
    assert!(rendered.contains("create LICENSE"), "{rendered}");
}

#[test]
fn fix_diff_omits_a_sensitive_create_outside_the_root() {
    // `allow_out_of_root` lets these creates leave the root, where the editor
    // form `--diff` used to find the hidden files by never reaches.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        r#"version: 1
allow_out_of_root: true
rules:
  - id: env-content
    kind: file_exists
    paths: ../shared/token.txt
    level: error
    fix:
      file_create: { content: "{{env.ENV_PATH_SECRET}}\n" }
  - id: sensitive-content
    kind: file_exists
    sensitive: true
    paths: ../shared/sensitive.txt
    level: error
    fix:
      file_create: { content: "EXPLICIT_TOKEN_VALUE_DO_NOT_REPORT\n" }
  - id: env-path
    kind: file_exists
    paths: "../{{env.ENV_PATH_SECRET}}-out/x.txt"
    level: error
    fix:
      file_create: { content: "x\n", create_parents: true }
  - id: plain
    kind: file_exists
    paths: ../shared/plain.txt
    level: error
    fix:
      file_create: { content: "plain\n" }
"#,
    )
    .unwrap();
    let output = run(&root, &["fix", "--diff"]);
    let rendered = String::from_utf8_lossy(&output.stdout);
    for secret in [
        ENV_PATH_SECRET,
        EXPLICIT_SECRET,
        "token.txt",
        "sensitive.txt",
    ] {
        assert!(
            !String::from_utf8_lossy(&all_output(&output)).contains(secret),
            "fix --diff leaked {secret}:\n{rendered}"
        );
    }
    assert!(rendered.contains("+++ b/../shared/plain.txt"), "{rendered}");
}

#[test]
fn a_dotted_selector_read_from_the_environment_hides_its_paths() {
    // `targets.files` names a path into `targets:`, the field the environment
    // marks.
    let dir = tempfile::tempdir().unwrap();
    for member in ["a", "b"] {
        let path = dir.path().join(ENV_PATH_SECRET).join(member);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("README.md"), member).unwrap();
    }
    std::fs::write(dir.path().join("README.md"), "root").unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        r#"version: 1
rules:
  - id: same-readme
    kind: cross_file
    relation: identical
    source: { file: README.md }
    targets: { files: "{{env.ENV_PATH_SECRET}}/*/README.md" }
    level: error
"#,
    )
    .unwrap();
    for args in [
        &["check", "--format", "human"][..],
        &["check", "--format", "json"],
        &["check", "--format", "sarif"],
        &["fix", "--dry-run", "--format", "json"],
    ] {
        let output = run(dir.path(), args);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        let rendered = String::from_utf8_lossy(&all_output(&output)).into_owned();
        assert!(
            !rendered.contains(ENV_PATH_SECRET),
            "{args:?} leaked the selector path:\n{rendered}"
        );
        assert!(rendered.contains("<redacted"), "{args:?}:\n{rendered}");
    }
}

#[test]
fn a_when_iter_filter_read_from_the_environment_hides_its_paths() {
    // `when_iter` picks which directories a nested rule visits, so a filter
    // read from the environment chooses the paths it reports.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("pkgs").join(ENV_PATH_SECRET)).unwrap();
    std::fs::create_dir_all(dir.path().join("pkgs/other")).unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        r#"version: 1
rules:
  - id: each-readme
    kind: for_each_dir
    select: "pkgs/*"
    when_iter: 'iter.basename == "{{env.ENV_PATH_SECRET}}"'
    require:
      - kind: file_exists
        paths: "{path}/README.md"
    level: error
"#,
    )
    .unwrap();
    for args in [
        &["check", "--format", "human"][..],
        &["check", "--format", "json"],
        &["check", "--format", "sarif"],
        &["explain", "each-readme", "--trace", "--preview"],
        &[
            "explain",
            "each-readme",
            "--trace",
            "--format",
            "json",
            "--preview",
        ],
    ] {
        let output = run(dir.path(), args);
        let rendered = String::from_utf8_lossy(&all_output(&output)).into_owned();
        assert!(
            !rendered.contains(ENV_PATH_SECRET),
            "{args:?} leaked the iterated path:\n{rendered}"
        );
        if args[0] == "check" {
            assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
            assert!(rendered.contains("<redacted"), "{args:?}:\n{rendered}");
        }
    }

    let baseline = dir.path().join(".alint-baseline.json");
    let output = run(
        dir.path(),
        &["baseline", "--output", baseline.to_str().unwrap()],
    );
    assert!(output.status.success(), "{output:?}");
    let rendered = String::from_utf8_lossy(&all_output(&output)).into_owned();
    assert!(!rendered.contains(ENV_PATH_SECRET), "{rendered}");
    let written = std::fs::read_to_string(&baseline).unwrap();
    assert!(!written.contains(ENV_PATH_SECRET), "{written}");
}

/// The `<redacted:…>` markers in `rendered`, in order.
fn markers(rendered: &str) -> Vec<&str> {
    rendered
        .match_indices("<redacted:")
        .map(|(start, _)| {
            let end = rendered[start..].find('>').unwrap();
            &rendered[start..=start + end]
        })
        .collect()
}

/// A dotenv manifest that names `one.env` and `two.env`, scoping a rule that
/// fails on both.
fn manifest_scope_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for name in ["one.env", "two.env"] {
        std::fs::write(dir.path().join(name), "MODE=dev\n").unwrap();
    }
    std::fs::write(dir.path().join("manifest.env"), "A=one.env\nB=two.env\n").unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        r#"version: 1
rules:
  - id: envmode
    kind: dotenv_path_equals
    paths: ["one.env", "two.env"]
    path: "$.MODE"
    equals: "prod"
    scope_filter:
      include_manifest_paths:
        source: manifest.env
        extract: { dotenv: "$.*" }
    level: error
"#,
    )
    .unwrap();
    dir
}

#[test]
fn a_manifest_scope_keeps_its_hidden_paths_out_of_the_rules_own_markers() {
    // The rule writes markers of its own, which hash the file they are in, so
    // the same finding in two hidden files must still read the same.
    let dir = manifest_scope_fixture();
    for args in [
        &["check", "--format", "json"][..],
        &["check", "--format", "sarif"],
        &["fix", "--dry-run", "--format", "json"],
    ] {
        let output = run(dir.path(), args);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        let rendered = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(
            !rendered.contains("one.env") && !rendered.contains("two.env"),
            "{args:?} leaked a scoped path:\n{rendered}"
        );
        let found = markers(&rendered);
        assert!(found.len() >= 2, "{args:?}:\n{rendered}");
        assert!(
            found.iter().all(|marker| *marker == found[0]),
            "{args:?} markers differ by file: {found:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_manifest_scope_keeps_its_hidden_paths_out_of_fix_failure_markers() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = manifest_scope_fixture();
    for name in ["one.env", "two.env"] {
        std::fs::set_permissions(
            dir.path().join(name),
            std::fs::Permissions::from_mode(0o000),
        )
        .unwrap();
    }
    if std::fs::read(dir.path().join("one.env")).is_ok() {
        // Running as root: nothing is unreadable.
        return;
    }
    for args in [
        &["check", "--format", "json"][..],
        &["fix", "--dry-run", "--format", "json"],
    ] {
        let output = run(dir.path(), args);
        let rendered = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(
            !rendered.contains("one.env") && !rendered.contains("two.env"),
            "{args:?} leaked a scoped path:\n{rendered}"
        );
        let found = markers(&rendered);
        assert!(found.len() >= 2, "{args:?}:\n{rendered}");
        assert!(
            found.iter().all(|marker| *marker == found[0]),
            "{args:?} failure markers differ by file: {found:?}"
        );
    }
}

#[test]
fn a_when_iter_that_reads_the_environment_itself_hides_its_paths() {
    // `env.WANTED` is read when the expression runs, not interpolated, and a
    // variable can carry the value in, but either way it picks the paths.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("pkgs").join(ENV_PATH_SECRET)).unwrap();
    std::fs::create_dir_all(dir.path().join("pkgs/other")).unwrap();
    for (vars, when_iter) in [
        ("", "iter.basename == env.ENV_PATH_SECRET"),
        (
            "vars:\n  wanted: \"{{env.ENV_PATH_SECRET}}\"\n",
            "iter.basename == vars.wanted",
        ),
    ] {
        std::fs::write(
            dir.path().join(".alint.yml"),
            format!(
                "version: 1\n{vars}rules:\n  - id: each-readme\n    kind: for_each_dir\n    \
                 select: \"pkgs/*\"\n    when_iter: '{when_iter}'\n    require:\n      \
                 - kind: file_exists\n        paths: \"{{path}}/README.md\"\n    level: error\n"
            ),
        )
        .unwrap();
        for format in ["human", "json", "sarif"] {
            let output = run(dir.path(), &["check", "--format", format]);
            assert_eq!(output.status.code(), Some(1), "{when_iter}: {output:?}");
            let rendered = String::from_utf8_lossy(&all_output(&output)).into_owned();
            assert!(
                !rendered.contains(ENV_PATH_SECRET),
                "{when_iter} {format} leaked the iterated path:\n{rendered}"
            );
            assert!(rendered.contains("<redacted"), "{format}:\n{rendered}");
        }
    }
}

/// A config whose fact reads the files `paths` matches.
fn fact_config(paths: &str) -> String {
    format!(
        "version: 1\nfacts:\n  - id: has_marker\n    file_content_matches:\n      \
         paths: \"{paths}\"\n      pattern: marker\nrules:\n  - id: readme\n    \
         kind: file_exists\n    paths: README.md\n    when: facts.has_marker\n    level: error\n"
    )
}

/// Writes an unreadable file at `path`, or returns false when this user can
/// read it anyway (root).
#[cfg(unix)]
fn unreadable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::write(path, "marker\n").unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
    std::fs::read(path).is_err()
}

#[cfg(unix)]
#[test]
fn a_fact_path_read_from_the_environment_stays_out_of_its_warning() {
    let dir = tempfile::tempdir().unwrap();
    if !unreadable(&dir.path().join(format!("{ENV_PATH_SECRET}.txt"))) {
        return;
    }
    std::fs::write(
        dir.path().join(".alint.yml"),
        fact_config("{{env.ENV_PATH_SECRET}}.txt"),
    )
    .unwrap();
    for args in [&["check"][..], &["facts"]] {
        let output = run(dir.path(), args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains(ENV_PATH_SECRET),
            "{args:?} leaked the fact path:\n{stderr}"
        );
        assert!(
            stderr.contains("unreadable") && stderr.contains("<redacted>"),
            "{args:?} warns with the path left out:\n{stderr}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_warning_escapes_the_control_characters_in_a_path() {
    let dir = tempfile::tempdir().unwrap();
    if !unreadable(&dir.path().join("evil\nWARN forged\x1b[31mred.txt")) {
        return;
    }
    std::fs::write(dir.path().join(".alint.yml"), fact_config("evil*")).unwrap();
    let output = run(dir.path(), &["check"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unreadable") && stderr.contains("evil"),
        "the warning names the file:\n{stderr}"
    );
    assert!(!stderr.contains('\x1b'), "{stderr:?}");
    assert!(
        !stderr.lines().any(|line| line.starts_with("WARN forged")),
        "{stderr:?}"
    );
}
