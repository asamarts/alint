//! Core config loading, template expansion, drop-ins, and source composition.

use super::*;

#[test]
fn transition_spawning_rule_list_equals_catalog_metadata() {
    use std::collections::BTreeSet;

    let catalog = test_catalog();
    let derived_rule_kinds: BTreeSet<&str> = catalog
        .kinds()
        .filter(|(_kind, meta)| {
            meta.capabilities
                .iter()
                .any(|entry| matches!(entry.capability, alint_core::Capability::SpawnUserCommand))
        })
        .map(|(kind, _meta)| kind)
        .collect();
    let hand_rule_kinds: BTreeSet<&str> = SPAWNING_RULE_KINDS.iter().copied().collect();
    assert_eq!(derived_rule_kinds, hand_rule_kinds);
}

#[test]
fn collect_drop_ins_handles_missing_dir() {
    // Missing `.alint.d/` is the common case (drop-ins
    // are opt-in by mkdir); should be silent.
    let dir = std::path::Path::new("/nonexistent/.alint.d");
    assert_eq!(
        collect_drop_ins(dir, &FilesystemConfigSource).unwrap(),
        Vec::<PathBuf>::new()
    );
}

#[test]
fn collect_drop_ins_yaml_files_only_alphabetical() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("99-late.yaml"), "version: 1\n").unwrap();
    std::fs::write(tmp.path().join("00-early.yml"), "version: 1\n").unwrap();
    std::fs::write(tmp.path().join("50-mid.yml"), "version: 1\n").unwrap();
    // Non-yaml files in the same dir should be skipped.
    std::fs::write(tmp.path().join("README.md"), "ignored\n").unwrap();
    std::fs::write(tmp.path().join(".gitkeep"), "").unwrap();
    let entries = collect_drop_ins(tmp.path(), &FilesystemConfigSource).unwrap();
    let names: Vec<&str> = entries
        .iter()
        .map(|p| p.file_name().unwrap().to_str().unwrap())
        .collect();
    assert_eq!(names, ["00-early.yml", "50-mid.yml", "99-late.yaml"]);
}

#[test]
fn template_expands_into_concrete_rule() {
    let yaml = r"
version: 1
templates:
  - id: dir-has-readme
    kind: pair
    primary: '{{vars.dir}}/**/*'
    partner: '{{vars.dir}}/README.md'
    level: warning
    message: 'every {{vars.dir}}/* should have a README'
rules:
  - extends_template: dir-has-readme
    id: pkgs-have-readme
    vars:
      dir: packages
";
    let cfg: RawConfig = ayaml::compat::from_str(yaml).unwrap();
    let final_cfg = cfg.finalize(Some(test_catalog())).unwrap();
    assert_eq!(final_cfg.rules.len(), 1);
    let r = &final_cfg.rules[0];
    assert_eq!(r.id, "pkgs-have-readme");
    assert_eq!(r.kind, "pair");
}

#[test]
fn template_supports_multiple_instances() {
    let yaml = r"
version: 1
templates:
  - id: dir-has-readme
    kind: pair
    primary: '{{vars.dir}}/**/*'
    partner: '{{vars.dir}}/README.md'
    level: warning
rules:
  - extends_template: dir-has-readme
    id: pkgs-have-readme
    vars: { dir: packages }
  - extends_template: dir-has-readme
    id: services-have-readme
    vars: { dir: services }
  - extends_template: dir-has-readme
    id: apps-have-readme
    vars: { dir: apps }
";
    let cfg: RawConfig = ayaml::compat::from_str(yaml).unwrap();
    let final_cfg = cfg.finalize(Some(test_catalog())).unwrap();
    let ids: Vec<&str> = final_cfg.rules.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "pkgs-have-readme",
            "services-have-readme",
            "apps-have-readme"
        ]
    );
}

#[test]
fn template_instance_can_override_field() {
    let yaml = r"
version: 1
templates:
  - id: dir-has-readme
    kind: pair
    primary: '{{vars.dir}}/**/*'
    partner: '{{vars.dir}}/README.md'
    level: warning
rules:
  - extends_template: dir-has-readme
    id: critical-readme
    level: error
    vars: { dir: services }
";
    let cfg: RawConfig = ayaml::compat::from_str(yaml).unwrap();
    let final_cfg = cfg.finalize(Some(test_catalog())).unwrap();
    assert_eq!(final_cfg.rules[0].level, alint_core::Level::Error);
}

#[test]
fn configured_field_error_names_the_template_that_supplied_it() {
    let yaml = r"
version: 1
templates:
  - id: contains-template
    kind: dir_contains
    paths: packages/*
    select: packages/*
    require: README.md
    level: error
rules:
  - extends_template: contains-template
    id: packages-have-readmes
";
    let raw: RawConfig = ayaml::compat::from_str(yaml).unwrap();
    let error = raw.finalize(Some(test_catalog())).unwrap_err().to_string();
    assert!(
        error.contains("`paths` is not supported by `dir_contains`"),
        "{error}"
    );
    assert!(error.contains("template `contains-template`"), "{error}");
}
#[test]
fn nested_configured_field_error_names_the_template_that_supplied_require() {
    let yaml = r"
version: 1
templates:
  - id: nested-template
    kind: for_each_dir
    select: packages/*
    require:
      - kind: dir_contains
        paths: packages/*
        select: packages/*
        require: README.md
    level: error
rules:
  - extends_template: nested-template
    id: packages-have-readmes
";
    let raw: RawConfig = ayaml::compat::from_str(yaml).unwrap();
    let error = raw.finalize(Some(test_catalog())).unwrap_err().to_string();
    assert!(error.contains("nested rule #0 (`dir_contains`)"), "{error}");
    assert!(error.contains("`paths` is not supported"), "{error}");
    assert!(error.contains("template `nested-template`"), "{error}");
}

#[test]
fn nested_rule_rejects_synthesized_parent_fields() {
    for field in ["id: ignored", "level: warning"] {
        let yaml = format!(
            "version: 1\nrules:\n  - id: packages\n    kind: for_each_dir\n    \
             select: packages/*\n    require:\n      - kind: file_exists\n        \
             paths: \"{{path}}/README.md\"\n        {field}\n    level: error\n"
        );
        let raw: RawConfig = ayaml::compat::from_str(&yaml).unwrap();
        let error = raw.finalize(Some(test_catalog())).unwrap_err().to_string();
        assert!(
            error.contains("not supported on a nested rule"),
            "{field}: {error}"
        );
    }
}

#[test]
fn configured_field_error_names_the_extends_source_that_supplied_it() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
        &base,
        "version: 1\nrules:\n  - id: packages-have-readmes\n    kind: dir_contains\n    \
         paths: packages/*\n    select: packages/*\n    require: README.md\n    level: error\n",
    )
    .unwrap();
    std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();

    let error = load(&child).unwrap_err().to_string();
    assert!(
        error.contains("`paths` is not supported by `dir_contains`"),
        "{error}"
    );
    assert!(error.contains("./base.yml"), "{error}");
}

#[test]
fn local_field_override_is_not_blamed_on_the_extends_source() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
        &base,
        "version: 1\nrules:\n  - id: packages-have-readmes\n    kind: dir_contains\n    \
         select: packages/*\n    require: README.md\n    level: error\n",
    )
    .unwrap();
    std::fs::write(
        &child,
        "version: 1\nextends: [./base.yml]\nrules:\n  - id: packages-have-readmes\n    \
         paths: packages/*\n",
    )
    .unwrap();

    let error = load(&child).unwrap_err().to_string();
    assert!(
        error.contains("`paths` is not supported by `dir_contains`"),
        "{error}"
    );
    assert!(!error.contains("supplied by ./base.yml"), "{error}");
}

#[test]
fn configured_false_expect_matches_is_rejected_when_the_kind_does_not_expose_it() {
    let yaml = r"
version: 1
rules:
  - id: readme
    kind: file_exists
    paths: README.md
    expect_matches: false
    level: error
";
    let raw: RawConfig = ayaml::compat::from_str(yaml).unwrap();
    let error = raw.finalize(Some(test_catalog())).unwrap_err().to_string();
    assert!(
        error.contains("`expect_matches` is not supported by `file_exists`"),
        "{error}"
    );
}

#[test]
fn configured_null_fix_is_rejected_when_the_kind_has_no_fix_operations() {
    let yaml = r"
version: 1
rules:
  - id: format
    kind: command_idempotent
    command: [cargo, fmt, --check]
    fix: null
    level: error
";
    let raw: RawConfig = ayaml::compat::from_str(yaml).unwrap();
    let error = raw.finalize(Some(test_catalog())).unwrap_err().to_string();
    assert!(
        error.contains("`fix` is not supported by `command_idempotent`"),
        "{error}"
    );
}

#[test]
fn configured_null_fix_is_rejected_when_the_kind_supports_fixes() {
    let yaml = r"
version: 1
rules:
  - id: readme
    kind: file_exists
    paths: README.md
    fix: null
    level: error
";
    let raw: RawConfig = ayaml::compat::from_str(yaml).unwrap();
    let error = raw.finalize(Some(test_catalog())).unwrap_err().to_string();
    assert!(
        error.contains("`fix` for `file_exists` must select exactly one operation"),
        "{error}"
    );
}

#[test]
fn configured_null_scope_filter_is_rejected_for_supported_kinds() {
    for (kind, options) in [("file_content_matches", "pattern: x"), ("dir_absent", "")] {
        let yaml = format!(
            "version: 1\n\
             rules:\n\
             \x20 - id: scoped\n\
             \x20   kind: {kind}\n\
             \x20   paths: x\n\
             \x20   scope_filter: null\n\
             \x20   level: error\n\
             \x20   {options}\n"
        );
        let raw: RawConfig = ayaml::compat::from_str(&yaml).unwrap();
        let error = raw.finalize(Some(test_catalog())).unwrap_err().to_string();
        assert!(
            error.contains("`scope_filter` must be a mapping with at least one predicate"),
            "{kind}: {error}"
        );
    }
}

#[test]
fn configured_null_scope_filter_names_unsupported_kind() {
    let yaml = r"
version: 1
rules:
  - id: readme
    kind: file_exists
    paths: README.md
    scope_filter: null
    level: error
";
    let raw: RawConfig = ayaml::compat::from_str(yaml).unwrap();
    let error = raw.finalize(Some(test_catalog())).unwrap_err().to_string();
    assert!(
        error.contains("`scope_filter` is not supported by `file_exists`"),
        "{error}"
    );
}

#[test]
fn template_unknown_id_errors_clearly() {
    let yaml = r"
version: 1
templates:
  - id: real-template
    kind: file_exists
    paths: [X]
rules:
  - extends_template: typo-template
    id: my-rule
";
    let cfg: RawConfig = ayaml::compat::from_str(yaml).unwrap();
    let err = cfg.finalize(Some(test_catalog())).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("typo-template"));
    assert!(msg.contains("unknown template"));
}

#[test]
fn template_cannot_extend_another_template() {
    let yaml = r"
version: 1
templates:
  - id: outer
    extends_template: inner
    kind: file_exists
    paths: [X]
  - id: inner
    kind: file_exists
    paths: [Y]
rules:
  - extends_template: outer
    id: my-rule
";
    let cfg: RawConfig = ayaml::compat::from_str(yaml).unwrap();
    let err = cfg.finalize(Some(test_catalog())).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("leaf-only"));
}

#[test]
fn template_substitutes_inside_lists_and_nested_mappings() {
    let yaml = r"
version: 1
templates:
  - id: list-and-nested
    kind: file_exists
    level: warning
    paths:
      - '{{vars.dir}}/README.md'
      - '{{vars.dir}}/LICENSE'
    fix:
      file_create:
        content: 'Hello, {{vars.dir}}!'
        path: '{{vars.dir}}/README.md'
rules:
  - extends_template: list-and-nested
    id: my-rule
    vars: { dir: pkg }
";
    let cfg: RawConfig = ayaml::compat::from_str(yaml).unwrap();
    let final_cfg = cfg.finalize(Some(test_catalog())).unwrap();
    let r = &final_cfg.rules[0];
    let paths = r.paths.as_ref().unwrap();
    let paths_str = format!("{paths:?}");
    assert!(paths_str.contains("pkg/README.md"));
    assert!(paths_str.contains("pkg/LICENSE"));
    assert!(matches!(
        r.fix,
        Some(alint_core::FixSpec::FileCreate { .. })
    ));
}

#[test]
fn drop_ins_merge_into_main_config_with_field_level_override() {
    // End-to-end: a `.alint.yml` next to a `.alint.d/`
    // dir; the drop-in's `id: main-rule` field-overrides
    // the main config's level. Mirrors the `/etc/*.d/`
    // mental model: drop-ins win on conflict.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        "version: 1\nrules:\n  - {id: main-rule, kind: file_exists, paths: [X], level: error}\n",
    )
    .unwrap();
    std::fs::create_dir_all(tmp.path().join(".alint.d")).unwrap();
    std::fs::write(
        tmp.path().join(".alint.d/00-base.yml"),
        "version: 1\nrules:\n  - {id: extra-rule, kind: file_exists, paths: [Y], level: warning}\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join(".alint.d/50-override.yml"),
        "version: 1\nrules:\n  - {id: main-rule, level: warning}\n",
    )
    .unwrap();
    let cfg = load(&tmp.path().join(".alint.yml")).unwrap();
    let by_id: std::collections::HashMap<&str, alint_core::Level> =
        cfg.rules.iter().map(|r| (r.id.as_str(), r.level)).collect();
    assert_eq!(
        by_id.get("main-rule").copied(),
        Some(alint_core::Level::Warning)
    );
    assert_eq!(
        by_id.get("extra-rule").copied(),
        Some(alint_core::Level::Warning)
    );
    assert_eq!(cfg.rules.len(), 2);
}

#[test]
fn extends_with_allow_out_of_root_is_rejected() {
    // Security: an inherited ruleset may not open the
    // path-confinement escape hatch — only the user's own top-level
    // config can (the same trust model as command/custom kinds).
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("base.yml"),
        "version: 1\nallow_out_of_root: true\nrules: []\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        "version: 1\nextends: [./base.yml]\nrules: []\n",
    )
    .unwrap();
    let err = load(&tmp.path().join(".alint.yml")).unwrap_err();
    assert!(err.to_string().contains("allow_out_of_root"), "{err}");
}

#[test]
fn top_level_allow_out_of_root_is_honored() {
    // The same key in the user's own top-level config is accepted
    // and resolves onto `Config`.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        "version: 1\nallow_out_of_root:\n  kinds: [pair_hash]\nrules: []\n",
    )
    .unwrap();
    let cfg = load(&tmp.path().join(".alint.yml")).unwrap();
    assert!(cfg.allow_out_of_root.allows("any", "pair_hash"));
    assert!(!cfg.allow_out_of_root.allows("any", "json_schema_passes"));
}

#[test]
fn local_extends_outside_lint_root_is_rejected() {
    // M2 (security): a local `extends:` target may not escape the
    // top-level config's directory to read arbitrary files off the host.
    // Layout: tmp/secret.yml (out of the config's tree) + tmp/repo/.alint.yml
    // that climbs to it with `../secret.yml`.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("secret.yml"), "version: 1\nrules: []\n").unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nextends: [../secret.yml]\nrules: []\n",
    )
    .unwrap();
    let err = load(&repo.join(".alint.yml")).unwrap_err().to_string();
    assert!(err.contains("outside the lint root"), "{err}");
    assert!(err.contains("secret.yml"), "{err}");
}

#[test]
fn extended_config_allow_out_of_root_does_not_lift_confinement_before_reject() {
    // Trust bypass: an inherited (untrusted) ruleset that sets
    // `allow_out_of_root: true` used to lift local-extends confinement for ITS
    // OWN `extends:` chain, so an out-of-root target was READ before the
    // parent's `reject_allow_out_of_root_in` fired one level too late. The flag
    // must only lift confinement for the user's TOP-LEVEL config. The out-of-root
    // target here is INVALID YAML: if it were read, the error would mention
    // parsing; the fix rejects it as "outside the lint root" BEFORE any read.
    let base = tempfile::tempdir().unwrap();
    std::fs::write(base.path().join("outside.yml"), "{{{ not valid yaml :::").unwrap();
    let repo = base.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    std::fs::write(
        repo.join("evil.yml"),
        "version: 1\nallow_out_of_root: true\nextends: [\"../outside.yml\"]\nrules: []\n",
    )
    .unwrap();
    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nextends: [./evil.yml]\nrules: []\n",
    )
    .unwrap();
    let err = load(&repo.join(".alint.yml")).unwrap_err().to_string();
    assert!(
        err.contains("outside the lint root"),
        "an extends'd allow_out_of_root must not lift confinement (target read blocked \
         BEFORE any read); got: {err}"
    );
}

#[test]
fn deeply_nested_flow_config_is_rejected_before_libyaml_parses_it() {
    // W2 wiring regression: a config whose YAML nests flow collections far too
    // deep must be rejected by the pre-parse `flow_depth_within_limit` guard
    // BEFORE it reaches yaml_serde/libyaml-rs (which is super-linear on flow
    // nesting and would hang the run). The `yaml_depth` unit tests only cover
    // the scanner in isolation; this pins that the config loader actually calls
    // it, so a deleted guard here would fail rather than silently reopen the DoS.
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join(".alint.yml");
    let bomb = format!(
        "version: 1\nrules: []\nx: {}1{}\n",
        "[".repeat(5000),
        "]".repeat(5000)
    );
    std::fs::write(&cfg, bomb).unwrap();
    let err = load(&cfg).unwrap_err().to_string();
    assert!(
        err.contains("flow nesting") && err.contains("depth"),
        "a flow-depth bomb config must be rejected pre-parse; got: {err}"
    );
}

#[test]
fn bom_prefixed_config_bombs_are_rejected_before_libyaml_parses_them() {
    // Regression: the guards scan the RAW config text, and a leading BOM put
    // their lexer one column behind libyaml on line 1, so `\u{feff}--- '...`
    // hid a flow bomb (7.7 s) or an alias bomb (3.5 GB) inside a phantom
    // single-quoted scalar. Both reported repros, through the real loader.
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join(".alint.yml");
    let n = 60_000;
    let flow = format!("\u{feff}--- 'x: {}1{}\n'\n", "[".repeat(n), "]".repeat(n));
    std::fs::write(&cfg, flow).unwrap();
    let err = load(&cfg).unwrap_err().to_string();
    assert!(err.contains("flow nesting"), "BOM flow bomb; got: {err}");
    let alias = format!(
        "\u{feff}--- 'k: {{a: &a [{}], b: [{}], c: \"{{{{env.HOME}}}}\"}}\n'\n",
        "1,".repeat(1000),
        "*a,".repeat(50_000)
    );
    std::fs::write(&cfg, &alias).unwrap();
    let err = load(&cfg).unwrap_err().to_string();
    assert!(
        err.contains("alias expansion"),
        "BOM alias bomb; got: {err}"
    );
    let err = crate::parse(&alias).unwrap_err().to_string();
    assert!(
        err.contains("alias expansion"),
        "BOM alias bomb; got: {err}"
    );
}

#[test]
fn big_scalar_alias_bomb_config_is_rejected_before_libyaml_parses_it() {
    // Regression: the alias budget counted nodes, so a 1 MB anchored scalar
    // replayed 4000 times (4000 nodes, 4 GB of string copies into the
    // `ayaml::Value` the loader builds) passed the guard.
    let body = format!(
        "x: &a \"{}\"\ny: [{}]\n",
        "x".repeat(1_000_000),
        "*a,".repeat(4000)
    );
    let err = crate::parse(&body).unwrap_err().to_string();
    assert!(err.contains("alias expansion"), "got: {err}");
}

#[test]
fn local_extends_out_of_root_allowed_with_top_level_flag() {
    // M2: the same blanket `allow_out_of_root: true` that lifts per-rule
    // read confinement also lifts the local-extends boundary — for users
    // who deliberately keep a shared ruleset beside (not inside) the tree.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
            tmp.path().join("shared.yml"),
            "version: 1\nrules:\n  - id: inherited\n    kind: file_exists\n    paths: INHERITED.md\n    level: warning\n",
        )
        .unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nallow_out_of_root: true\nextends: [../shared.yml]\nrules: []\n",
    )
    .unwrap();
    let cfg = load(&repo.join(".alint.yml")).unwrap();
    assert!(
        cfg.rules.iter().any(|r| r.id == "inherited"),
        "extends resolved"
    );
}

#[test]
fn top_level_baseline_is_honored() {
    // The `baseline:` key in the user's own top-level config resolves
    // onto `Config` (the CLI then suppresses against it).
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        "version: 1\nbaseline: .alint-baseline.json\nrules: []\n",
    )
    .unwrap();
    let cfg = load(&tmp.path().join(".alint.yml")).unwrap();
    assert_eq!(
        cfg.baseline.as_deref(),
        Some(std::path::Path::new(".alint-baseline.json"))
    );
}

#[test]
fn extends_with_baseline_is_rejected() {
    // Security: an inherited ruleset must not choose which findings the
    // gate suppresses — only the user's own top-level config sets it.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("base.yml"),
        "version: 1\nbaseline: sneaky.json\nrules: []\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        "version: 1\nextends: [./base.yml]\nrules: []\n",
    )
    .unwrap();
    let err = load(&tmp.path().join(".alint.yml")).unwrap_err();
    assert!(err.to_string().contains("baseline"), "{err}");
}

#[test]
fn load_interpolates_env_default_through_real_path() {
    // End-to-end through `load()`: the value field uses an
    // unset env var with a default, so it resolves hermetically
    // (no env var set — Rust 2024 marks `set_var` unsafe). Proves
    // the YAML-value → interpolate → RawConfig wiring in the
    // loader fires and that `vars.`/`id:` are left intact.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        "version: 1\nrules:\n  - id: spdx\n    kind: file_exists\n    \
             paths: \"{{env.ALINT_TEST_UNSET_DIR | default('src')}}/X\"\n    level: error\n",
    )
    .unwrap();
    let cfg = load(&tmp.path().join(".alint.yml")).unwrap();
    assert_eq!(cfg.rules.len(), 1);
    assert_eq!(cfg.rules[0].id, "spdx");
    // `id:` is in SKIP_KEYS, never interpolated; `paths:` is.
    let paths = format!("{:?}", cfg.rules[0].paths);
    assert!(paths.contains("src/X"), "paths not interpolated: {paths}");
}

#[test]
fn load_errors_on_undefined_env_without_default() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        "version: 1\nrules:\n  - id: r\n    kind: file_exists\n    \
             paths: \"{{env.ALINT_TEST_DEFINITELY_UNSET}}\"\n    level: error\n",
    )
    .unwrap();
    let err = load(&tmp.path().join(".alint.yml")).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("interpolation error"), "{msg}");
    assert!(msg.contains("ALINT_TEST_DEFINITELY_UNSET"), "{msg}");
}

#[test]
fn parses_minimal_config() {
    let yaml = r"
version: 1
rules:
  - id: readme
    kind: file_exists
    level: error
    paths: README.md
";
    let cfg = parse(yaml).unwrap();
    assert_eq!(cfg.version, 1);
    assert_eq!(cfg.rules.len(), 1);
    assert_eq!(cfg.rules[0].id, "readme");
    assert_eq!(cfg.rules[0].kind, "file_exists");
}

#[test]
fn rejects_wrong_version() {
    let yaml = "version: 99\nrules: []\n";
    assert!(parse(yaml).is_err());
}

#[test]
fn file_loader_rejects_null_boolean_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(".alint.yml");
    for setting in ["respect_gitignore", "nested_configs"] {
        std::fs::write(&path, format!("version: 1\n{setting}: null\nrules: []\n")).unwrap();
        let err = load(&path).unwrap_err();
        assert!(err.to_string().contains("invalid type"), "{setting}: {err}");
    }
}

#[test]
fn parse_rejects_config_with_extends() {
    // `parse(yaml)` can't resolve a path-relative `extends:` —
    // load_recursive needs a base path. Error rather than
    // silently ignore.
    let yaml = "version: 1\nextends: [base.yml]\nrules: []\n";
    let err = parse(yaml).unwrap_err();
    assert!(err.to_string().contains("extends"));
}

#[test]
fn parse_rejects_fix_block_with_two_ops() {
    // R-TWOOP end to end: a `fix:` block carrying two op keys is rejected by
    // the real loader, not silently first-wins (the untagged-FixSpec trap).
    // Fires during deserialization, before any kind-compatibility build step.
    let yaml = "\
version: 1
rules:
  - id: r
    kind: file_exists
    level: error
    paths: README.md
    fix:
      file_trim_trailing_whitespace: {}
      file_append_final_newline: {}
";
    let err = parse(yaml).unwrap_err();
    assert!(
        err.to_string().contains("exactly one op key"),
        "expected a two-op rejection, got: {err}"
    );
}

#[test]
fn load_resolves_local_extends_and_merges_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
        &base,
        r"version: 1
rules:
  - id: base-readme
    kind: file_exists
    paths: README.md
    level: error
  - id: shared
    kind: file_exists
    paths: X
    level: warning
",
    )
    .unwrap();
    std::fs::write(
        &child,
        r"version: 1
extends: [./base.yml]
rules:
  - id: shared
    kind: file_exists
    paths: X
    level: error   # child override wins
  - id: child-only
    kind: file_exists
    paths: Y
    level: warning
",
    )
    .unwrap();

    let cfg = load(&child).unwrap();
    let ids: Vec<&str> = cfg.rules.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, vec!["base-readme", "shared", "child-only"]);
    let shared = cfg.rules.iter().find(|r| r.id == "shared").unwrap();
    assert_eq!(shared.level, alint_core::Level::Error);
}

#[test]
fn load_merges_vars_and_appends_ignore() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
        &base,
        r"version: 1
ignore: [target]
vars:
  from_base: base
  shared: base
rules: []
",
    )
    .unwrap();
    std::fs::write(
        &child,
        r"version: 1
extends: [./base.yml]
ignore: [node_modules]
vars:
  from_child: child
  shared: child
rules: []
",
    )
    .unwrap();

    let cfg = load(&child).unwrap();
    assert_eq!(
        cfg.ignore,
        vec!["target".to_string(), "node_modules".to_string()]
    );
    assert_eq!(cfg.vars.get("from_base"), Some(&"base".to_string()));
    assert_eq!(cfg.vars.get("from_child"), Some(&"child".to_string()));
    assert_eq!(cfg.vars.get("shared"), Some(&"child".to_string()));
}

#[test]
fn load_detects_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a.yml");
    let b = tmp.path().join("b.yml");
    std::fs::write(&a, "version: 1\nextends: [./b.yml]\nrules: []\n").unwrap();
    std::fs::write(&b, "version: 1\nextends: [./a.yml]\nrules: []\n").unwrap();
    let err = load(&a).unwrap_err().to_string();
    assert!(err.contains("cycle"), "{err}");
}

#[test]
fn extends_only_keeps_listed_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
        &base,
        "version: 1
rules:
  - id: a
    kind: file_exists
    paths: A
    level: error
  - id: b
    kind: file_exists
    paths: B
    level: error
  - id: c
    kind: file_exists
    paths: C
    level: error
",
    )
    .unwrap();
    std::fs::write(
        &child,
        "version: 1
extends:
  - url: ./base.yml
    only: [b]
rules: []
",
    )
    .unwrap();
    let cfg = load(&child).unwrap();
    let ids: Vec<&str> = cfg.rules.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, vec!["b"]);
}

#[test]
fn extends_except_drops_listed_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
        &base,
        "version: 1
rules:
  - id: a
    kind: file_exists
    paths: A
    level: error
  - id: b
    kind: file_exists
    paths: B
    level: error
",
    )
    .unwrap();
    std::fs::write(
        &child,
        "version: 1
extends:
  - url: ./base.yml
    except: [a]
rules: []
",
    )
    .unwrap();
    let cfg = load(&child).unwrap();
    let ids: Vec<&str> = cfg.rules.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, vec!["b"]);
}

#[test]
fn extends_rejects_only_and_except_together() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
        &base,
        "version: 1
rules:
  - id: a
    kind: file_exists
    paths: A
    level: error
",
    )
    .unwrap();
    std::fs::write(
        &child,
        "version: 1
extends:
  - url: ./base.yml
    only: [a]
    except: [a]
rules: []
",
    )
    .unwrap();
    let err = load(&child).unwrap_err().to_string();
    assert!(err.contains("mutually exclusive"), "{err}");
}

#[test]
fn extends_rejects_unknown_rule_id_in_filter() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
        &base,
        "version: 1
rules:
  - id: a
    kind: file_exists
    paths: A
    level: error
",
    )
    .unwrap();
    std::fs::write(
        &child,
        "version: 1
extends:
  - url: ./base.yml
    only: [does-not-exist]
rules: []
",
    )
    .unwrap();
    let err = load(&child).unwrap_err().to_string();
    assert!(err.contains("does-not-exist"), "{err}");
    assert!(err.contains("unknown rule id"), "{err}");
}

#[test]
fn extends_rejects_empty_filter_list() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
        &base,
        "version: 1
rules:
  - id: a
    kind: file_exists
    paths: A
    level: error
",
    )
    .unwrap();
    std::fs::write(
        &child,
        "version: 1
extends:
  - url: ./base.yml
    only: []
rules: []
",
    )
    .unwrap();
    let err = load(&child).unwrap_err().to_string();
    assert!(err.contains("empty"), "{err}");
}

#[test]
fn load_rejects_remote_extends_without_sri() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(".alint.yml");
    std::fs::write(
        &path,
        "version: 1\nextends: [\"https://example.com/base.yml\"]\nrules: []\n",
    )
    .unwrap();
    let opts = LoadOptions::with_cache(extends::Cache::at(tmp.path().join("cache")));
    let err = load_with(&path, &opts).unwrap_err().to_string();
    assert!(err.contains("integrity hash"), "{err}");
    assert!(err.contains("https://example.com"), "{err}");
}

#[test]
fn load_resolves_https_extends_via_cache_hit() {
    use sha2::{Digest, Sha256};

    // The remote body; could be anything valid.
    let remote_body = b"version: 1\nrules:\n  - id: inherited\n    kind: file_exists\n    paths: INHERITED.md\n    level: warning\n";

    // Pre-compute the SRI so the scenario is hermetic and the
    // integrity check on read succeeds.
    let mut hasher = Sha256::new();
    hasher.update(remote_body);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for b in &digest {
        use std::fmt::Write as _;
        write!(hex, "{b:02x}").unwrap();
    }
    let sri_str = format!("sha256-{hex}");

    let tmp = tempfile::tempdir().unwrap();
    let cache = extends::Cache::at(tmp.path().join("cache"));
    let sri = extends::Sri::parse(&sri_str).unwrap();

    // Seed the cache so the loader hits it instead of the network.
    cache.put(&sri, remote_body).unwrap();

    // Local .alint.yml references the remote config + adds one
    // local rule of its own.
    let url = format!("https://example.invalid/base.yml#{sri_str}");
    let config_path = tmp.path().join(".alint.yml");
    std::fs::write(
            &config_path,
            format!(
                "version: 1\nextends: [\"{url}\"]\nrules:\n  - id: local\n    kind: file_exists\n    paths: LOCAL.md\n    level: error\n"
            ),
        )
        .unwrap();

    let opts = LoadOptions::with_cache(cache);
    let cfg = load_with(&config_path, &opts).unwrap();
    let ids: Vec<&str> = cfg.rules.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, vec!["inherited", "local"]);
}
