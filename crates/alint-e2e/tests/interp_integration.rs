//! End-to-end checks for `{{env.X}}` variable interpolation
//! (`docs/design/v0.11/variable_interpolation.md`): a config value
//! interpolated at load by `alint-dsl` must flow correctly into a
//! built rule, including the schema-validation-after-interpolation
//! contract for a numeric field.
//!
//! Tests are hermetic: they use the `| default('...')` fallback so no
//! environment variable is set (Rust 2024 marks `set_var` unsafe and
//! the workspace forbids unsafe code).

use std::io::Write;

use crate::support;

fn write_config(body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(".alint.yml");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(body.as_bytes()).unwrap();
    (tmp, path)
}

#[test]
fn interpolated_integer_field_validates_and_builds() {
    // `subject_max_length` is a numeric option on `git_commit_message`.
    // The interpolated `"{{env.MAX | default('72')}}"` re-types to the
    // number 72 at load, so the rule's typed `Options` deserialization
    // (u64) validates clean — proving schema validation runs AFTER
    // interpolation, not before (raw `{{...}}` text would fail u64).
    let (_tmp, path) = write_config(
        "version: 1\n\
         rules:\n  \
         - id: subject-len\n    \
           kind: git_commit_message\n    \
           subject_max_length: \"{{env.ALINT_TEST_MAX | default('72')}}\"\n    \
           level: error\n",
    );
    let config = support::load(&path).expect("config with interpolated integer should load");
    let registry = alint_rules::builtin_registry();
    for spec in &config.rules {
        registry
            .build(spec)
            .expect("rule with interpolated numeric option should build");
    }
}

#[test]
fn interpolated_string_field_flows_into_rule() {
    let (_tmp, path) = write_config(
        "version: 1\n\
         rules:\n  \
         - id: header\n    \
           kind: file_content_matches\n    \
           paths: [\"{{env.ALINT_TEST_DIR | default('src')}}/lib.rs\"]\n    \
           pattern: \"^// SPDX\"\n    \
           level: warning\n",
    );
    let config = support::load(&path).expect("load");
    // `paths:` is interpolated; `id:` is not.
    assert_eq!(config.rules[0].id, "header");
    let paths = format!("{:?}", config.rules[0].paths);
    assert!(
        paths.contains("src/lib.rs"),
        "paths not interpolated: {paths}"
    );
    let registry = alint_rules::builtin_registry();
    registry.build(&config.rules[0]).expect("build");
}

#[test]
fn undefined_env_without_default_fails_load() {
    let (_tmp, path) = write_config(
        "version: 1\n\
         rules:\n  \
         - id: r\n    \
           kind: file_exists\n    \
           paths: \"{{env.ALINT_TEST_DEFINITELY_UNSET}}\"\n    \
           level: error\n",
    );
    let err = support::load(&path).expect_err("undefined env without default must fail load");
    let msg = err.to_string();
    assert!(msg.contains("interpolation error"), "{msg}");
    assert!(msg.contains("ALINT_TEST_DEFINITELY_UNSET"), "{msg}");
}

#[test]
fn local_extends_target_is_interpolated() {
    // A local `extends:` target is the user's trusted source, so its
    // `{{env.X}}` resolves like the top-level config does.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("base.yml"),
        "version: 1\n\
         rules:\n  \
         - id: spdx\n    \
           kind: file_exists\n    \
           paths: \"{{env.ALINT_TEST_DIR | default('base-src')}}/X\"\n    \
           level: error\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        "version: 1\nextends: [./base.yml]\n",
    )
    .unwrap();
    let config = support::load(&tmp.path().join(".alint.yml")).expect("load");
    let interpolated = config
        .rules
        .iter()
        .any(|r| format!("{:?}", r.paths).contains("base-src/X"));
    assert!(interpolated, "extends target was not interpolated");
}

#[test]
fn nested_config_template_vars_expand_while_env_interpolation_is_rejected() {
    // Nested configs are untrusted and must NOT read the host environment
    // (`{{env.*}}`, tested in `nested_config_rejects_env_interpolation`),
    // while non-env template variable expansion (`extends_template:` +
    // `{{vars.*}}`) still works cleanly inside a nested config.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        "version: 1\n\
         nested_configs: true\n\
         templates:\n  \
         - id: dir-file\n    \
           kind: file_exists\n    \
           paths: \"{{vars.nested_dir}}/Y\"\n    \
           level: error\n\
         rules: []\n",
    )
    .unwrap();
    let sub = tmp.path().join("pkg");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(
        sub.join(".alint.yml"),
        "version: 1\n\
         rules:\n  \
         - id: nested-spdx\n    \
           extends_template: dir-file\n    \
           vars:\n      \
             nested_dir: nested-src\n",
    )
    .unwrap();
    let config = support::load(&tmp.path().join(".alint.yml")).expect("load");
    let expanded = config
        .rules
        .iter()
        .any(|r| format!("{:?}", r.paths).contains("pkg/nested-src/Y"));
    assert!(
        expanded,
        "nested config template `{{vars.nested_dir}}` was not expanded and re-rooted: {:?}",
        config.rules
    );
}

#[test]
fn nested_config_rejects_env_interpolation() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        "version: 1\nnested_configs: true\nrules: []\n",
    )
    .unwrap();
    let sub = tmp.path().join("pkg");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(
        sub.join(".alint.yml"),
        "version: 1\n\
         rules:\n  \
         - id: nested-env\n    \
           kind: file_exists\n    \
           paths: \"{{env.ALINT_TEST_DIR | default('nested-src')}}/Y\"\n    \
           level: error\n",
    )
    .unwrap();
    let err = support::load(&tmp.path().join(".alint.yml"))
        .expect_err("nested config must reject {{env.*}}")
        .to_string();
    assert!(
        err.contains("environment interpolation") || err.contains("{{env.*}}"),
        "unexpected error: {err}"
    );
}

#[test]
fn drop_in_config_is_interpolated() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join(".alint.yml"), "version: 1\nrules: []\n").unwrap();
    std::fs::create_dir_all(tmp.path().join(".alint.d")).unwrap();
    std::fs::write(
        tmp.path().join(".alint.d/00-extra.yml"),
        "version: 1\n\
         rules:\n  \
         - id: dropin-spdx\n    \
           kind: file_exists\n    \
           paths: \"{{env.ALINT_TEST_DIR | default('dropin-src')}}/Z\"\n    \
           level: error\n",
    )
    .unwrap();
    let config = support::load(&tmp.path().join(".alint.yml")).expect("load");
    let interpolated = config
        .rules
        .iter()
        .any(|r| format!("{:?}", r.paths).contains("dropin-src"));
    assert!(interpolated, "drop-in `{{env.X}}` was not interpolated");
}

#[test]
fn when_env_clause_loads_and_builds() {
    let (_tmp, path) = write_config(
        "version: 1\n\
         rules:\n  \
         - id: ci-only\n    \
           kind: file_exists\n    \
           paths: [coverage.xml]\n    \
           when: env.CI == \"true\"\n    \
           level: warning\n",
    );
    let config = support::load(&path).expect("config with `when: env.X` should load");
    let registry = alint_rules::builtin_registry();
    registry
        .build(&config.rules[0])
        .expect("rule with `when: env.X` should build");
}

#[test]
fn foreign_go_template_in_command_survives_load_and_build() {
    // Regression for the `{{...}}`-is-shared lesson: a Go template in
    // a `command:` arg must reach the rule untouched.
    let (_tmp, path) = write_config(
        "version: 1\n\
         rules:\n  \
         - id: lint-workflows\n    \
           kind: command\n    \
           paths: [\".github/workflows/*.yml\"]\n    \
           command: [\"true\", \"-format\", \"{{json .}}\"]\n    \
           level: warning\n",
    );
    let config = support::load(&path).expect("config with Go template should load");
    let rendered = format!("{:?}", config.rules[0]);
    assert!(
        rendered.contains("{{json .}}"),
        "Go template was mangled: {rendered}"
    );
    let registry = alint_rules::builtin_registry();
    registry.build(&config.rules[0]).expect("build");
}
