use super::*;

#[test]
fn spawning_fix_ops_are_gated() {
    // Phase 3 shipped the first spawning fix op. Every entry must be a real fix
    // op, and `git_untrack` (`git rm --cached`) must be present -- its
    // top-level-only trust gate is `reject_spawning_fix_ops_in` (+ the template
    // and `finalize` backstops), exercised by the `load_rejects_git_untrack_*`
    // tests below and the e2e canary `crates/alint/tests/fix_spawn_gate.rs`.
    use std::collections::BTreeSet;
    assert!(
        !SPAWNING_FIX_OPS.is_empty(),
        "SPAWNING_FIX_OPS is empty; the first spawning op (git_untrack) should be listed"
    );
    let all: BTreeSet<&str> = alint_core::FixSpec::ALL_OP_NAMES.iter().copied().collect();
    let spawning: BTreeSet<&str> = SPAWNING_FIX_OPS.iter().copied().collect();
    assert!(
        spawning.is_subset(&all),
        "SPAWNING_FIX_OPS names an unknown op: {:?}",
        &spawning - &all
    );
    assert!(
        spawning.contains("git_untrack"),
        "git_untrack (the first spawning fix op) must be gated"
    );
    assert!(
        spawning.contains("command"),
        "the `command` fix op (a user-supplied fix command) must be gated"
    );
}

#[test]
fn collect_drop_ins_handles_missing_dir() {
    // Missing `.alint.d/` is the common case (drop-ins
    // are opt-in by mkdir); should be silent.
    let dir = std::path::Path::new("/nonexistent/.alint.d");
    assert_eq!(collect_drop_ins(dir).unwrap(), Vec::<PathBuf>::new());
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
    let entries = collect_drop_ins(tmp.path()).unwrap();
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
    let cfg: RawConfig = serde_yaml_ng::from_str(yaml).unwrap();
    let final_cfg = cfg.finalize().unwrap();
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
    let cfg: RawConfig = serde_yaml_ng::from_str(yaml).unwrap();
    let final_cfg = cfg.finalize().unwrap();
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
    let cfg: RawConfig = serde_yaml_ng::from_str(yaml).unwrap();
    let final_cfg = cfg.finalize().unwrap();
    assert_eq!(final_cfg.rules[0].level, alint_core::Level::Error);
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
    let cfg: RawConfig = serde_yaml_ng::from_str(yaml).unwrap();
    let err = cfg.finalize().unwrap_err();
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
    let cfg: RawConfig = serde_yaml_ng::from_str(yaml).unwrap();
    let err = cfg.finalize().unwrap_err();
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
    let cfg: RawConfig = serde_yaml_ng::from_str(yaml).unwrap();
    let final_cfg = cfg.finalize().unwrap();
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
    // BEFORE it reaches serde_yaml_ng/libyaml (which is super-linear on flow
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
    // `serde_yaml_ng::Value` the loader builds) passed the guard.
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

// --- W2 content-fixer trust (auto-fix.md 5.5) ---------------------------------

/// The applicability a loaded rule's content fixer DECLARES (`None` = unset, so the
/// builder's default tier applies). W2 sets it to `Suggestion` for a content fixer
/// from an untrusted remote.
fn declared_content_tier(rule: &alint_core::RuleSpec) -> Option<alint_core::Applicability> {
    use alint_core::FixSpec;
    match rule.fix.as_ref()? {
        FixSpec::Replace { replace } => replace.applicability,
        FixSpec::FileCreate { file_create } => file_create.applicability,
        FixSpec::FilePrepend { file_prepend } => file_prepend.applicability,
        FixSpec::FileAppend { file_append } => file_append.applicability,
        FixSpec::SetValue { set_value } => set_value.applicability,
        FixSpec::RemoveValue { remove_value } => remove_value.applicability,
        FixSpec::SyncFrom { sync_from } => sync_from.applicability,
        FixSpec::CreateAndRegister {
            create_and_register,
        } => create_and_register.applicability,
        // `relocate` is fixed-behavior (never demoted), but reading its declared
        // tier explicitly gives `w2_remote_relocate_is_not_demoted` teeth: a
        // regression that demoted it would surface here as `Some(Suggestion)`.
        FixSpec::Relocate { relocate } => relocate.applicability,
        // `sort` is content-injecting (demoted from an untrusted remote); read its
        // tier so `w2_remote_sort_is_demoted_to_suggestion` sees the cap.
        FixSpec::Sort { sort } => sort.applicability,
        // `indent_style` is content-injecting for the same reason (aim a reindent
        // at a Makefile); read its tier for `w2_remote_indent_style_is_demoted`.
        FixSpec::IndentStyle { indent_style } => indent_style.applicability,
        // `insert_line` injects the host rule's `require:` lines; read its tier for
        // `w2_remote_insert_line_is_demoted`.
        FixSpec::InsertLine { insert_line } => insert_line.applicability,
        // `insert_header` injects the host `file_header` rule's header bytes; read
        // its tier for `w2_remote_insert_header_is_demoted`.
        FixSpec::InsertHeader { insert_header } => insert_header.applicability,
        // `file_rename` / `file_normalize_line_endings` are aimable Safe transforms
        // (case-rename breaks imports; CRLF breaks a shebang); read their tier for
        // `w2_remote_file_rename_is_demoted` / `..._file_normalize_line_endings_...`.
        FixSpec::FileRename { file_rename } => file_rename.applicability,
        FixSpec::FileNormalizeLineEndings {
            file_normalize_line_endings,
        } => file_normalize_line_endings.applicability,
        // The hygiene normalizers + `chmod` are content-injecting too (aimable at the
        // Safe tier -- R3); read each tier for its `w2_remote_<op>_is_demoted` test.
        // `file_strip_bidi` / `file_strip_zero_width` are NOT here -- they stay
        // fixed-behavior (security-positive), covered by
        // `w2_remote_security_positive_strip_is_not_demoted`.
        FixSpec::FileTrimTrailingWhitespace {
            file_trim_trailing_whitespace,
        } => file_trim_trailing_whitespace.applicability,
        FixSpec::FileAppendFinalNewline {
            file_append_final_newline,
        } => file_append_final_newline.applicability,
        FixSpec::FileStripBom { file_strip_bom } => file_strip_bom.applicability,
        FixSpec::FileCollapseBlankLines {
            file_collapse_blank_lines,
        } => file_collapse_blank_lines.applicability,
        FixSpec::Chmod { chmod } => chmod.applicability,
        _ => None,
    }
}

/// Seed `cache` with `body` under its own SRI and return the `https://…#sha256-…`
/// URL a config would `extends:`. Its base (no fragment) is
/// `https://example.invalid/remote.yml`.
fn seed_remote(cache: &extends::Cache, body: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(body.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for b in &digest {
        use std::fmt::Write as _;
        write!(hex, "{b:02x}").unwrap();
    }
    let sri_str = format!("sha256-{hex}");
    let sri = extends::Sri::parse(&sri_str).unwrap();
    cache.put(&sri, body.as_bytes()).unwrap();
    format!("https://example.invalid/remote.yml#{sri_str}")
}

const REMOTE_REPLACE: &str = "version: 1\nrules:\n  - id: no-todo\n    \
     kind: file_content_forbidden\n    paths: \"*.txt\"\n    pattern: TODO\n    \
     level: error\n    fix: { replace: { replacement: DONE } }\n";

fn load_extending(remote_body: &str, top_extra: &str) -> alint_core::Config {
    load_extending_top(
        remote_body,
        &format!("version: 1\n{{url}}\n{top_extra}rules: []\n"),
    )
}

/// Load an exact top-level body around a seeded remote. `{url}` is replaced by
/// the generated `extends:` line, allowing tests that need top-level rules rather
/// than the empty list used by [`load_extending`].
fn load_extending_top(remote_body: &str, top_body: &str) -> alint_core::Config {
    let tmp = tempfile::tempdir().unwrap();
    let cache = extends::Cache::at(tmp.path().join("cache"));
    let url = seed_remote(&cache, remote_body);
    let config_path = tmp.path().join(".alint.yml");
    let extends_line = format!("extends: [\"{url}\"]");
    std::fs::write(&config_path, top_body.replace("{url}", &extends_line)).unwrap();
    // Keep the tempdir alive for the duration of the load by leaking it into the
    // cache path (the cache is read during load); simplest is to load before drop.
    let opts = LoadOptions::with_cache(cache);
    let cfg = load_with(&config_path, &opts).unwrap();
    drop(tmp);
    cfg
}

#[test]
fn w2_remote_replace_is_demoted_to_suggestion() {
    // A `replace` (content-injecting) from a REMOTE `extends:` the user has not
    // trusted may propose but never auto-write: its tier is capped to `suggestion`.
    let cfg = load_extending(REMOTE_REPLACE, "");
    let rule = cfg.rules.iter().find(|r| r.id == "no-todo").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted remote's `replace` must be demoted to suggestion"
    );
}

#[test]
fn w2_remote_file_create_is_demoted_to_suggestion() {
    // R-RETRO: the pre-existing inline-content ops are demoted too. `file_create`
    // gained an `applicability` field precisely so this cap has somewhere to land.
    let body = "version: 1\nrules:\n  - id: need-notice\n    kind: file_exists\n    \
        paths: NOTICE\n    root_only: true\n    level: error\n    \
        fix: { file_create: { content: \"(c) them\\n\" } }\n";
    let cfg = load_extending(body, "");
    let rule = cfg.rules.iter().find(|r| r.id == "need-notice").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted remote's `file_create` must be demoted (R-RETRO)"
    );
}

#[test]
fn w2_remote_file_prepend_is_demoted_to_suggestion() {
    // R-RETRO: `file_prepend` is a content-injecting inline op in
    // `CONTENT_INJECTING_FIX_OPS`, so an untrusted remote's must be capped too
    // (the classification-exhaustiveness gate had it, but no runtime demotion
    // test proved the cap fires for this op shape).
    let body = "version: 1\nrules:\n  - id: header-required\n    kind: file_header\n    \
        paths: \"src/**/*.rs\"\n    pattern: \"(?s)Copyright\"\n    lines: 3\n    \
        level: error\n    fix: { file_prepend: { content: \"// Copyright\\n\" } }\n";
    let cfg = load_extending(body, "");
    let rule = cfg
        .rules
        .iter()
        .find(|r| r.id == "header-required")
        .unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted remote's `file_prepend` must be demoted (R-RETRO)"
    );
}

#[test]
fn w2_remote_file_append_is_demoted_to_suggestion() {
    let body = "version: 1\nrules:\n  - id: spdx\n    kind: file_content_matches\n    \
        paths: \"README.md\"\n    pattern: \"SPDX-License-Identifier\"\n    \
        level: warning\n    fix: { file_append: { content: \"\\n<!-- SPDX -->\\n\" } }\n";
    let cfg = load_extending(body, "");
    let rule = cfg.rules.iter().find(|r| r.id == "spdx").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted remote's `file_append` must be demoted (R-RETRO)"
    );
}

#[test]
fn w2_remote_set_value_is_demoted_to_suggestion() {
    // Phase 2: `set_value` writes the host rule's `equals` bytes, so a REMOTE
    // untrusted `extends:` may PROPOSE it but never auto-write -> capped to
    // suggestion. Teeth: reclassifying `set_value` out of CONTENT_INJECTING_FIX_OPS
    // (or dropping it) makes this assert `None` (auto-applies).
    let body = "version: 1\nrules:\n  - id: sv\n    kind: hcl_path_equals\n    \
        paths: \"*.tf\"\n    path: \"$.region\"\n    equals: \"x\"\n    level: error\n    \
        fix: { set_value: {} }\n";
    let cfg = load_extending(body, "");
    let rule = cfg.rules.iter().find(|r| r.id == "sv").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted remote's `set_value` must be demoted to suggestion"
    );
}

const REMOTE_SYNC_FROM: &str = "version: 1\nrules:\n  - id: mirror\n    \
     kind: cross_file\n    relation: identical\n    source: { file: LICENSE }\n    \
     targets: { files: \"crates/*/LICENSE\" }\n    level: error\n    \
     fix: { sync_from: {} }\n";

#[test]
fn w2_remote_sync_from_is_demoted_to_suggestion() {
    // `sync_from` overwrites a target with the ruleset-chosen `source:` bytes, so a
    // REMOTE `extends:` the user has not trusted may PROPOSE but never auto-write:
    // its tier is capped to `suggestion`. This locks in the demotion that otherwise
    // rests only on structural assumptions about where a `cross_file` rule's `fix:`
    // sits (audit gap). Teeth: dropping `sync_from` from CONTENT_INJECTING_FIX_OPS
    // reverts this to None (default Unsafe) and reds here.
    let cfg = load_extending(REMOTE_SYNC_FROM, "");
    let rule = cfg.rules.iter().find(|r| r.id == "mirror").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted remote's `sync_from` must be demoted to suggestion"
    );
}

const REMOTE_SYNC_FROM_EQUALS: &str = "version: 1\nrules:\n  - id: propagate\n    \
     kind: cross_file\n    relation: equals\n    \
     source: { file: Cargo.toml, extract: { toml: \"$.workspace.package.version\" } }\n    \
     targets: { files: \"crates/*/Cargo.toml\", extract: { toml: \"$.package.version\" } }\n    \
     level: error\n    fix: { sync_from: {} }\n";

#[test]
fn w2_remote_sync_from_equals_is_demoted_to_suggestion() {
    // The `equals` form of `sync_from` builds a DIFFERENT fixer type
    // (CrossFileValueFixer, which propagates a value into a node) than `identical`
    // (SyncFromFixer). The demotion keys on the op name, so it must cap the value
    // fixer too -- a remote must PROPOSE a value propagation, never auto-write the
    // ruleset-chosen value into the user's files. Locks in the equals-form demotion
    // (audit gap: only the identical form was tested).
    let cfg = load_extending(REMOTE_SYNC_FROM_EQUALS, "");
    let rule = cfg.rules.iter().find(|r| r.id == "propagate").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted remote's `sync_from` on `equals` must be demoted to suggestion"
    );
}

const REMOTE_CREATE_AND_REGISTER: &str = "version: 1\nrules:\n  - id: reg\n    \
     kind: cross_file\n    relation: registered\n    \
     source: { files: \"crates/*\" }\n    \
     targets: [{ file: Cargo.toml, extract: { toml: \"$.workspace.members[*]\" } }]\n    \
     level: error\n    fix: { create_and_register: {} }\n";

#[test]
fn w2_remote_create_and_register_is_demoted_to_suggestion() {
    // `create_and_register` appends a ruleset-chosen member value into the user's
    // manifest, so a REMOTE `extends:` the user has not trusted may PROPOSE but
    // never auto-write: its tier is capped to `suggestion`. Teeth: dropping
    // `create_and_register` from CONTENT_INJECTING_FIX_OPS reverts this to None
    // (default Unsafe) and reds here.
    let cfg = load_extending(REMOTE_CREATE_AND_REGISTER, "");
    let rule = cfg.rules.iter().find(|r| r.id == "reg").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted remote's `create_and_register` must be demoted to suggestion"
    );
}

#[test]
fn w2_remote_sort_is_demoted_to_suggestion() {
    // `sort` writes no ruleset bytes, but a REMOTE `extends:` the user has not
    // trusted can AIM its reorder at an order-significant file (.gitignore /
    // CODEOWNERS) at the Safe tier, so it is capped to `suggestion` -- may PROPOSE
    // but never auto-write. Teeth: dropping `sort` from CONTENT_INJECTING_FIX_OPS
    // reverts this to None and reds here.
    let body = "version: 1\nrules:\n  - id: ks\n    kind: ordered_block\n    \
        paths: \"**/CODEOWNERS\"\n    level: error\n    fix: { sort: {} }\n";
    let cfg = load_extending(body, "");
    let rule = cfg.rules.iter().find(|r| r.id == "ks").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted remote's `sort` must be demoted to suggestion"
    );
}

#[test]
fn w2_remote_indent_style_is_demoted_to_suggestion() {
    // `indent_style` writes no ruleset bytes, but a REMOTE `extends:` can AIM its
    // reindent at an indent-significant file (a Makefile recipe's literal tab) at
    // the Safe tier, so it is capped to `suggestion`. Teeth: dropping
    // `indent_style` from CONTENT_INJECTING_FIX_OPS reverts this to None and reds.
    let body = "version: 1\nrules:\n  - id: ind\n    kind: indent_style\n    \
        paths: \"**/Makefile\"\n    style: spaces\n    width: 4\n    level: error\n    \
        fix: { indent_style: {} }\n";
    let cfg = load_extending(body, "");
    let rule = cfg.rules.iter().find(|r| r.id == "ind").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted remote's `indent_style` must be demoted to suggestion"
    );
}

#[test]
fn w2_remote_insert_line_is_demoted_to_suggestion() {
    // `insert_line` splices the host rule's ruleset-authored `require:` lines into
    // the victim's file, so a REMOTE `extends:` must PROPOSE it, never auto-write:
    // capped to `suggestion`. Teeth: dropping `insert_line` from
    // CONTENT_INJECTING_FIX_OPS reverts this to None and reds here.
    let body = "version: 1\nrules:\n  - id: co\n    kind: ordered_block\n    \
        paths: \"**/CODEOWNERS\"\n    require: [\"* @team\"]\n    level: error\n    \
        fix: { insert_line: {} }\n";
    let cfg = load_extending(body, "");
    let rule = cfg.rules.iter().find(|r| r.id == "co").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted remote's `insert_line` must be demoted to suggestion"
    );
}

#[test]
fn w2_remote_insert_header_is_demoted_to_suggestion() {
    // `insert_header` inserts the host `file_header` rule's ruleset-authored header
    // bytes near the top of the victim's file (like `file_prepend`), so a REMOTE
    // `extends:` must PROPOSE it, never auto-write: capped to `suggestion`. Teeth:
    // dropping `insert_header` from CONTENT_INJECTING_FIX_OPS reverts this to None.
    let body = "version: 1\nrules:\n  - id: hdr\n    kind: file_header\n    \
        paths: \"**/*.rs\"\n    pattern: \"SPDX\"\n    level: error\n    \
        fix: { insert_header: { content: \"// SPDX\\n\" } }\n";
    let cfg = load_extending(body, "");
    let rule = cfg.rules.iter().find(|r| r.id == "hdr").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted remote's `insert_header` must be demoted to suggestion"
    );
}

#[test]
fn w2_remote_file_normalize_line_endings_is_demoted_to_suggestion() {
    // AUDIT (partition MED): a remote can AIM a CRLF rewrite at a shebang script to
    // break it (line endings are significance-bearing), so `file_normalize_line_endings`
    // must be demoted from an untrusted remote. Teeth: dropping it from
    // CONTENT_INJECTING_FIX_OPS reverts this to None.
    let body = "version: 1\nrules:\n  - id: le\n    kind: line_endings\n    \
        paths: \"**/*.sh\"\n    target: crlf\n    level: error\n    \
        fix: { file_normalize_line_endings: {} }\n";
    let cfg = load_extending(body, "");
    let rule = cfg.rules.iter().find(|r| r.id == "le").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted remote's `file_normalize_line_endings` must be demoted"
    );
}

#[test]
fn w2_remote_file_rename_is_demoted_to_suggestion() {
    // AUDIT (partition MED): a remote can AIM a mass case-rename at the victim's
    // source, breaking case-sensitive imports, so `file_rename` must be demoted from
    // an untrusted remote. Teeth: dropping it from CONTENT_INJECTING_FIX_OPS reverts
    // this to None.
    let body = "version: 1\nrules:\n  - id: fc\n    kind: filename_case\n    \
        paths: \"**/*.py\"\n    case: snake\n    level: error\n    \
        fix: { file_rename: {} }\n";
    let cfg = load_extending(body, "");
    let rule = cfg.rules.iter().find(|r| r.id == "fc").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted remote's `file_rename` must be demoted"
    );
}

#[test]
fn w2_remote_remove_value_is_not_demoted() {
    // `remove_value` deletes a node (no ruleset bytes) -> fixed-behavior, gated
    // by its Unsafe tier like `file_remove`, NOT demoted by W2. Its tier stays
    // unset (None -> default Unsafe at fix time), never forced to suggestion.
    let body = "version: 1\nrules:\n  - id: rv\n    kind: hcl_path_absent\n    \
        paths: \"*.tf\"\n    path: \"$.secret\"\n    level: error\n    \
        fix: { remove_value: {} }\n";
    let cfg = load_extending(body, "");
    let rule = cfg.rules.iter().find(|r| r.id == "rv").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        None,
        "an untrusted remote's `remove_value` is fixed-behavior, not demoted"
    );
}

#[test]
fn w2_remote_relocate_is_not_demoted() {
    // `relocate` moves a file to the repo root (a rename, no ruleset bytes, no
    // spawn) -> fixed-behavior, gated by its Unsafe tier like `file_remove`/
    // `file_rename`, NOT demoted by W2. Its tier stays unset (None -> default
    // Unsafe at fix time), never forced to suggestion from an untrusted remote.
    let body = "version: 1\nrules:\n  - id: nested-lock\n    kind: file_absent\n    \
        paths: \"**/*/Cargo.lock\"\n    level: error\n    \
        fix: { relocate: {} }\n";
    let cfg = load_extending(body, "");
    let rule = cfg.rules.iter().find(|r| r.id == "nested-lock").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        None,
        "an untrusted remote's `relocate` is fixed-behavior, not demoted"
    );
}

#[test]
fn w2_remote_content_fixer_via_template_is_demoted() {
    // Bypass vector (audit): a remote provides a content-fix TEMPLATE plus a rule
    // that references it. The template's `fix:` block is spliced into the rule at
    // `finalize` -- AFTER the per-source demotion -- so the cap must cover
    // `templates:` too, or the remote content fixer auto-applies (escaping a
    // rules-only cap). Teeth: dropping the `parent.templates` demotion in
    // `load_recursive` makes this assert `None`.
    let body = "version: 1\ntemplates:\n  - id: inject\n    \
        kind: file_content_forbidden\n    paths: \"*.txt\"\n    pattern: TODO\n    \
        level: error\n    fix: { replace: { replacement: PWNED } }\nrules:\n  \
        - extends_template: inject\n    id: pwned\n";
    let cfg = load_extending(body, "");
    let rule = cfg.rules.iter().find(|r| r.id == "pwned").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "a content fixer smuggled through a remote TEMPLATE must also be demoted"
    );
}

#[test]
fn w2_remote_rule_instantiating_a_trusted_template_is_demoted() {
    // The source-local cap cannot see a fixer this remote rule acquires only when
    // `finalize` expands the trusted root template. The raw rule's monotonic
    // provenance marker must survive merge + expansion and cap the EFFECTIVE
    // fixer, including bytes supplied through a template variable.
    // The user's top-level config authors the parameterized content-fix template;
    // the untrusted remote authors only a rule that instantiates it and fills the
    // `{{vars.text}}` hole with its own bytes.
    let remote = "version: 1\nrules:\n  - id: pwned\n    \
        extends_template: user_inject\n    paths: \"*.txt\"\n    \
        vars:\n      text: PWNED\n";
    let top_template = "templates:\n  - id: user_inject\n    \
        kind: file_content_forbidden\n    pattern: TODO\n    level: error\n    \
        fix: { replace: { replacement: \"{{vars.text}}\" } }\n";
    let cfg = load_extending(remote, top_template);
    let rule = cfg.rules.iter().find(|r| r.id == "pwned").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted rule instantiating a trusted template must have its effective \
         content fixer demoted after expansion"
    );
    // And the injected bytes are the remote's own (var-hole amplification), proving
    // this is arbitrary-byte injection, not merely triggering the user's own fixer.
    match rule.fix.as_ref().expect("pwned carries the expanded fixer") {
        alint_core::FixSpec::Replace { replace } => {
            assert_eq!(
                replace.replacement, "PWNED",
                "the untrusted instance's `vars:` filled the trusted template's hole"
            );
        }
        other => panic!("expected a Replace fixer, got {other:?}"),
    }
}

#[test]
fn w2_trusted_remote_rule_instantiating_a_trusted_template_keeps_tier() {
    // `trusted_extends:` deliberately restores content authority for the named
    // remote, including a fixer acquired from a root template. It still does not
    // grant process-spawning authority (covered by the independent spawn tests).
    let remote = "version: 1\nrules:\n  - id: allowed\n    \
        extends_template: user_inject\n    paths: \"*.txt\"\n    \
        vars:\n      text: APPROVED\n";
    let top = "trusted_extends: [\"https://example.invalid/remote.yml\"]\n\
        templates:\n  - id: user_inject\n    kind: file_content_forbidden\n    \
        pattern: TODO\n    level: error\n    \
        fix: { replace: { replacement: \"{{vars.text}}\" } }\n";
    let cfg = load_extending(remote, top);
    let rule = cfg.rules.iter().find(|r| r.id == "allowed").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        None,
        "an allowlisted remote's effective content fixer keeps its declared tier"
    );
}

#[test]
fn w2_untrusted_template_provenance_survives_a_trusted_field_merge() {
    // The inverse mixed-source case: the remote contributes part of a template,
    // while the trusted root supplies its fixer and instantiates it. Template
    // provenance must OR across the id-based field merge, or the trusted rule
    // would auto-apply a target shape partly controlled by the remote.
    let remote = "version: 1\ntemplates:\n  - id: mixed\n    \
        kind: file_content_forbidden\n    pattern: TODO\nrules: []\n";
    let top = "version: 1\n{url}\ntemplates:\n  - id: mixed\n    level: error\n    \
        fix: { replace: { replacement: DONE } }\nrules:\n  - id: use-mixed\n    \
        extends_template: mixed\n    paths: \"*.txt\"\n";
    let cfg = load_extending_top(remote, top);
    let rule = cfg.rules.iter().find(|r| r.id == "use-mixed").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "untrusted template provenance must survive a trusted field merge"
    );
}

#[test]
fn w2_untrusted_rule_provenance_survives_a_trusted_field_merge() {
    // A remote can supply the target shape while the root later adds only the
    // fixer to the same rule id. The source-local pass sees no remote fixer, so
    // the rule marker itself must survive the merge and cap the composed fixer.
    let remote = "version: 1\nrules:\n  - id: mixed-rule\n    \
        kind: file_content_forbidden\n    paths: \"*.txt\"\n    pattern: TODO\n    \
        level: error\n";
    let top = "version: 1\n{url}\nrules:\n  - id: mixed-rule\n    \
        fix: { replace: { replacement: DONE } }\n";
    let cfg = load_extending_top(remote, top);
    let rule = cfg.rules.iter().find(|r| r.id == "mixed-rule").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "untrusted rule provenance must survive a trusted field merge"
    );
}

#[test]
fn w2_content_injecting_ssot_is_exhaustive_and_valid() {
    // Defense-in-depth for the W2 audit's architectural risk: the trust handling
    // keys on hand-maintained SSOTs, so a FUTURE fix op could be added to the
    // engine yet forgotten here -- left un-demoted / un-refused from a remote
    // `extends:` (the class of the templates bypass this audit found). This gate
    // forces EVERY fix op into exactly ONE of THREE trust classes, so a new op
    // fails the build until the call is made (and wired in): content-injecting
    // (demoted from an untrusted remote), spawning (refused from any non-top-level
    // source), or fixed-behavior (no ruleset bytes, no spawn -- honored anywhere).
    use std::collections::BTreeSet;
    // Fix ops that carry NO ruleset-authored bytes AND do not spawn: honored from
    // any source. The exhaustive complement of the content + spawning SSOTs.
    const FIXED_BEHAVIOR_FIX_OPS: &[&str] = &[
        "file_remove",
        // `file_strip_bidi` / `file_strip_zero_width` are security-POSITIVE (they
        // remove Trojan-Source / zero-width attacks), so honoring them from any source
        // is the safe default -- demoting them would let a remote-sourced attack
        // survive. DELIBERATELY kept fixed-behavior (asamarts, 2026-09-27 audit R3).
        "file_strip_bidi",
        "file_strip_zero_width",
        // `remove_value` deletes (no ruleset bytes); `set_value` is content-injecting.
        "remove_value",
        // `dir_create` makes an empty directory -- no ruleset bytes, no spawn.
        "dir_create",
        // `relocate` moves a file to the repo root (a rename) -- no ruleset bytes,
        // no spawn; gated by its Unsafe tier like `file_remove`/`file_rename`.
        "relocate",
        // NOTE (R3, asamarts): the hygiene normalizers `file_trim_trailing_whitespace`
        // / `file_append_final_newline` / `file_strip_bom` / `file_collapse_blank_lines`
        // + `chmod` are NOT here any more -- they write no ruleset bytes, but a remote
        // can AIM them at a file where the "cosmetic" change is load-bearing (Markdown
        // hard break, encoding signature, paragraph breaks, a script's +x), so they
        // are CONTENT_INJECTING (demoted from an untrusted remote) like `sort` /
        // `file_normalize_line_endings`.
        // NOTE: `sort` / `indent_style` / `insert_*` / `file_rename` /
        // `file_normalize_line_endings` are also CONTENT_INJECTING (aimable, or inject
        // ruleset bytes), not fixed-behavior.
        // NOTE: `git_untrack` and `command` are NOT here -- they SPAWN, so they are
        // classified via SPAWNING_FIX_OPS (refused from any non-top-level source),
        // a strictly stronger gate than the content demotion.
    ];
    let content: BTreeSet<&str> = crate::CONTENT_INJECTING_FIX_OPS.iter().copied().collect();
    let fixed: BTreeSet<&str> = FIXED_BEHAVIOR_FIX_OPS.iter().copied().collect();
    let spawning: BTreeSet<&str> = crate::SPAWNING_FIX_OPS.iter().copied().collect();
    let all: BTreeSet<&str> = alint_core::FixSpec::ALL_OP_NAMES.iter().copied().collect();

    assert!(
        content.is_subset(&all),
        "content SSOT names an unknown op: {:?}",
        &content - &all
    );
    assert!(
        spawning.is_subset(&all),
        "spawning SSOT names an unknown op: {:?}",
        &spawning - &all
    );
    // The three trust classes must be PAIRWISE disjoint: each op has exactly one
    // trust posture.
    assert!(
        content.is_disjoint(&fixed),
        "op(s) marked BOTH content-injecting and fixed-behavior: {:?}",
        &content & &fixed
    );
    assert!(
        content.is_disjoint(&spawning),
        "op(s) marked BOTH content-injecting and spawning: {:?}",
        &content & &spawning
    );
    assert!(
        fixed.is_disjoint(&spawning),
        "op(s) marked BOTH fixed-behavior and spawning: {:?}",
        &fixed & &spawning
    );
    // ...and together they must cover EVERY op (exhaustive partition).
    let classified: BTreeSet<&str> = content.union(&fixed).copied().collect::<BTreeSet<_>>();
    let classified: BTreeSet<&str> = classified.union(&spawning).copied().collect();
    assert_eq!(
        classified,
        all,
        "unclassified fix op(s) -- decide content-injecting (add to \
         CONTENT_INJECTING_FIX_OPS, demoted from a remote `extends:`), spawning (add \
         to SPAWNING_FIX_OPS, refused from any non-top-level source), or fixed-behavior: {:?}",
        &all - &classified
    );
}

#[test]
fn w2_trusted_extends_re_honors_a_named_remote() {
    // Listing the remote's URL in the top-level `trusted_extends:` opts it back in:
    // its content fixers are honored at their own tier (no demotion -> unset spec).
    let cfg = load_extending(
        REMOTE_REPLACE,
        "trusted_extends: [\"https://example.invalid/remote.yml\"]\n",
    );
    let rule = cfg.rules.iter().find(|r| r.id == "no-todo").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        None,
        "a trusted remote's `replace` keeps its own (unset -> Unsafe) tier"
    );
}

#[test]
fn w2_local_extends_content_fixer_is_not_demoted() {
    // A content fixer from a LOCAL `extends:` (the user's own tree) is honored at
    // tier -- only remote-URL sources are demoted.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    std::fs::write(&base, REMOTE_REPLACE).unwrap();
    let config_path = tmp.path().join(".alint.yml");
    std::fs::write(
        &config_path,
        "version: 1\nextends: [\"base.yml\"]\nrules: []\n",
    )
    .unwrap();
    let opts = LoadOptions::with_cache(extends::Cache::at(tmp.path().join("cache")));
    let cfg = load_with(&config_path, &opts).unwrap();
    let rule = cfg.rules.iter().find(|r| r.id == "no-todo").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        None,
        "a LOCAL extends is the user's own tree -- not demoted"
    );
}

#[test]
fn w2_remote_security_positive_strip_is_not_demoted() {
    // `file_strip_bidi` / `file_strip_zero_width` are security-POSITIVE (they remove
    // Trojan-Source / zero-width attacks), so they stay FIXED_BEHAVIOR -- honored from
    // ANY source, never demoted (demoting them would let a remote-sourced attack
    // survive). They have NO `applicability` field, so if the demotion wrongly
    // targeted one the load would ERROR (deny_unknown_fields); a clean load with the
    // rule present proves it is left alone. (R3: the OTHER hygiene ops + chmod ARE now
    // demoted -- see the `w2_remote_*_is_demoted` tests below.)
    let body = "version: 1\nrules:\n  - id: no-bidi\n    kind: no_bidi_controls\n    \
        paths: \"*.rs\"\n    level: error\n    fix: { file_strip_bidi: {} }\n";
    let cfg = load_extending(body, "");
    assert!(
        cfg.rules.iter().any(|r| r.id == "no-bidi"),
        "a security-positive strip fixer loads and is honored (never demoted)"
    );
}

#[test]
fn w2_remote_file_trim_trailing_whitespace_is_demoted() {
    // R3 (asamarts): the hygiene normalizers are content-injecting now -- a remote can
    // AIM a trim at a file where trailing whitespace is significant (a Markdown hard
    // line break is two trailing spaces), so its tier is capped to `suggestion`.
    let body = "version: 1\nrules:\n  - id: ws\n    kind: no_trailing_whitespace\n    \
        paths: \"*.txt\"\n    level: error\n    fix: { file_trim_trailing_whitespace: {} }\n";
    let cfg = load_extending(body, "");
    let rule = cfg.rules.iter().find(|r| r.id == "ws").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted remote's `file_trim_trailing_whitespace` must be demoted (R3)"
    );
}

#[test]
fn w2_remote_file_append_final_newline_is_demoted() {
    let body = "version: 1\nrules:\n  - id: eof\n    kind: final_newline\n    \
        paths: \"*.txt\"\n    level: error\n    fix: { file_append_final_newline: {} }\n";
    let cfg = load_extending(body, "");
    let rule = cfg.rules.iter().find(|r| r.id == "eof").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted remote's `file_append_final_newline` must be demoted (R3)"
    );
}

#[test]
fn w2_remote_file_strip_bom_is_demoted() {
    let body = "version: 1\nrules:\n  - id: bom\n    kind: no_bom\n    \
        paths: \"*.txt\"\n    level: error\n    fix: { file_strip_bom: {} }\n";
    let cfg = load_extending(body, "");
    let rule = cfg.rules.iter().find(|r| r.id == "bom").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted remote's `file_strip_bom` must be demoted (R3)"
    );
}

#[test]
fn w2_remote_file_collapse_blank_lines_is_demoted() {
    let body = "version: 1\nrules:\n  - id: blanks\n    kind: max_consecutive_blank_lines\n    \
        paths: \"*.md\"\n    max: 1\n    level: error\n    fix: { file_collapse_blank_lines: {} }\n";
    let cfg = load_extending(body, "");
    let rule = cfg.rules.iter().find(|r| r.id == "blanks").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted remote's `file_collapse_blank_lines` must be demoted (R3)"
    );
}

#[test]
fn w2_remote_chmod_is_demoted() {
    // A remote aiming a +/-x flip at a script (loses +x -> breaks) or a data file
    // (gains +x -> exec surface) is a real permission change -- capped to suggestion.
    let body = "version: 1\nrules:\n  - id: exec\n    kind: executable_bit\n    \
        paths: \"*.sh\"\n    require: true\n    level: error\n    fix: { chmod: {} }\n";
    let cfg = load_extending(body, "");
    let rule = cfg.rules.iter().find(|r| r.id == "exec").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "an untrusted remote's `chmod` must be demoted (R3)"
    );
}

#[test]
fn w2_trusted_extends_in_an_extended_config_is_rejected() {
    // A ruleset must not allowlist ITSELF: `trusted_extends:` in an extended config
    // is refused at load (only the user's top-level config grants trust).
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    std::fs::write(
        &base,
        "version: 1\ntrusted_extends: [\"https://evil.example/x.yml\"]\nrules: []\n",
    )
    .unwrap();
    let config_path = tmp.path().join(".alint.yml");
    std::fs::write(
        &config_path,
        "version: 1\nextends: [\"base.yml\"]\nrules: []\n",
    )
    .unwrap();
    let opts = LoadOptions::with_cache(extends::Cache::at(tmp.path().join("cache")));
    let err = load_with(&config_path, &opts).unwrap_err().to_string();
    assert!(
        err.contains("trusted_extends") && err.contains("top-level"),
        "an extended config's `trusted_extends:` must be refused; got: {err}"
    );
}

#[test]
fn load_rejects_custom_fact_declared_in_local_extends() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
        &base,
        r#"version: 1
facts:
  - id: from_base
    custom:
      argv: ["/bin/true"]
rules: []
"#,
    )
    .unwrap();
    std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();
    let err = load(&child).unwrap_err().to_string();
    assert!(err.contains("custom"), "{err}");
    assert!(err.contains("base.yml"), "{err}");
}

#[test]
fn load_allows_custom_fact_in_top_level_config() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(".alint.yml");
    std::fs::write(
        &path,
        r#"version: 1
facts:
  - id: whoami
    custom:
      argv: ["/bin/true"]
rules: []
"#,
    )
    .unwrap();
    let cfg = load(&path).unwrap();
    assert_eq!(cfg.facts.len(), 1);
    assert_eq!(cfg.facts[0].id, "whoami");
}

#[test]
fn load_rejects_command_rule_declared_in_local_extends() {
    // Mirror of the custom-fact gate. A `kind: command` rule
    // hidden in an extended config must be refused — otherwise
    // adopting a published ruleset would imply granting it
    // arbitrary process execution on the user's machine.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
        &base,
        r#"version: 1
rules:
  - id: shellcheck-from-base
    kind: command
    paths: "**/*.sh"
    command: ["shellcheck", "{path}"]
    level: error
"#,
    )
    .unwrap();
    std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();
    let err = load(&child).unwrap_err().to_string();
    assert!(err.contains("command"), "{err}");
    assert!(err.contains("base.yml"), "{err}");
}

#[test]
fn load_rejects_command_fix_op_via_extends_on_a_non_command_kind() {
    // Audit H1: the `command` FIX op is spawning, so it must be refused from an
    // extended source even on a NON-command host kind. This is a path the
    // command-RULE gate (`reject_command_rules_in`, keyed on `kind: command`) does
    // NOT cover -- a `fix: { command: {...} }` on `file_absent` -- but the fix-op
    // gate (`reject_spawning_fix_ops_in`, which scans `fix:` blocks) does. Without
    // it an adopted ruleset could shell out on a bare `alint fix`.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
        &base,
        "version: 1\nrules:\n  - id: smuggled\n    kind: file_absent\n    paths: \"**/*\"\n    level: error\n    fix:\n      command:\n        run: [\"sh\", \"-c\", \"touch pwned\"]\n",
    )
    .unwrap();
    std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();
    let err = load(&child).unwrap_err().to_string();
    assert!(err.contains("command"), "op not named: {err}");
    assert!(err.contains("arbitrary code"), "{err}");
    assert!(err.contains("base.yml"), "source not named: {err}");
}

#[test]
fn load_rejects_every_spawning_kind_in_extends_not_just_command() {
    // Regression for the closed trust-gate gap:
    // `generated_file_fresh` and `command_idempotent` shell
    // out identically to `command`, so an extended config
    // declaring either must be refused too — otherwise
    // adopting a ruleset implies arbitrary code execution.
    for (kind, body) in [
        (
            "generated_file_fresh",
            "    file: out.txt\n    command: [\"sh\", \"-c\", \"echo pwn\"]\n",
        ),
        (
            "command_idempotent",
            "    command: [\"sh\", \"-c\", \"echo pwn\"]\n",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base.yml");
        let child = tmp.path().join(".alint.yml");
        std::fs::write(
            &base,
            format!(
                "version: 1\nrules:\n  - id: sneaky\n    kind: {kind}\n{body}    level: error\n"
            ),
        )
        .unwrap();
        std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();
        let err = load(&child).unwrap_err().to_string();
        assert!(err.contains(kind), "{kind} not gated: {err}");
        assert!(err.contains("arbitrary code"), "{kind}: {err}");
    }
}

#[test]
fn load_rejects_spawning_template_smuggled_via_extends() {
    // C1 (RCE bypass): an extended ruleset can't carry a spawning
    // `kind` directly (caught by `reject_command_rules_in`), but it
    // could hide one in a `templates:` block and reference it from a
    // `kind`-less `extends_template:` rule. The template expands into a
    // `command` rule at finalize, *after* the gate — so without the
    // template gate the consumer gets arbitrary code execution by
    // adding a single SRI-pinned `extends:` line. Body is
    // self-contained, mirroring a real published ruleset.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
            &base,
            "version: 1\ntemplates:\n  - id: t\n    kind: command\n    command: [\"sh\", \"-c\", \"echo pwn\"]\n    paths: \"**/*\"\n    level: error\nrules:\n  - id: pwned\n    extends_template: t\n",
        )
        .unwrap();
    std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();
    let err = load(&child).unwrap_err().to_string();
    assert!(err.contains("command"), "kind not named: {err}");
    assert!(err.contains("base.yml"), "source not named: {err}");
    assert!(err.contains("arbitrary code"), "{err}");
}

#[test]
fn finalize_rejects_a_top_level_spawning_template() {
    // The invariant holds with no `extends:` at all: a spawning kind
    // may never live in a `templates:` block (it would be a latent
    // bypass the moment the config is extended or a nested config
    // references it), so even a top-level spawning template is a hard
    // error. `finalize` is the source-agnostic backstop.
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join(".alint.yml");
    std::fs::write(
            &cfg,
            "version: 1\ntemplates:\n  - id: t\n    kind: generated_file_fresh\n    file: out.txt\n    command: [\"sh\", \"-c\", \"echo pwn\"]\n    level: error\nrules:\n  - id: x\n    extends_template: t\n",
        )
        .unwrap();
    let err = load(&cfg).unwrap_err().to_string();
    assert!(err.contains("generated_file_fresh"), "{err}");
    assert!(err.contains("templates"), "{err}");
}

// ── git_untrack: the first SPAWNING fix op (R-SPAWNGATE, DSL layer) ──────────
// The spawn gate keys on the rule KIND, but a spawning FIXER hangs off a
// non-spawning kind (`git_untrack` on `file_absent`), so these prove the
// fix-op gate (`reject_spawning_fix_ops_in` + the template / finalize backstops)
// refuses it from every non-top-level source, while a top-level declaration
// still loads. The RCE-canary end-to-end analogue lives in
// `crates/alint/tests/fix_spawn_gate.rs`.

#[test]
fn load_rejects_git_untrack_fix_declared_in_extends() {
    // A `git_untrack` fix shells out (`git rm --cached`), so an extended ruleset
    // declaring one must be refused -- adopting a published ruleset must never
    // imply it can run git against the user's repo on a bare `alint fix`.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
        &base,
        "version: 1\nrules:\n  - id: sneaky-untrack\n    kind: file_absent\n    paths: \"**/*\"\n    git_tracked_only: true\n    level: error\n    fix:\n      git_untrack: {}\n",
    )
    .unwrap();
    std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();
    let err = load(&child).unwrap_err().to_string();
    assert!(err.contains("git_untrack"), "op not named: {err}");
    assert!(err.contains("base.yml"), "source not named: {err}");
    assert!(err.contains("arbitrary code"), "{err}");
}

#[test]
fn load_rejects_git_untrack_fix_in_extends_require_block() {
    // Depth check: a spawning fix buried in a `require:` block (a `for_each_dir`
    // nested rule) must be refused too -- the gate recurses at every depth, like
    // the spawning-KIND gate.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
        &base,
        "version: 1\nrules:\n  - id: outer\n    kind: for_each_dir\n    paths: \"**/\"\n    level: error\n    require:\n      - id: inner-untrack\n        kind: file_absent\n        paths: \"*\"\n        git_tracked_only: true\n        fix:\n          git_untrack: {}\n",
    )
    .unwrap();
    std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();
    let err = load(&child).unwrap_err().to_string();
    assert!(err.contains("git_untrack"), "nested op not gated: {err}");
    assert!(err.contains("arbitrary code"), "{err}");
}

#[test]
fn load_rejects_git_untrack_fix_template_smuggled_via_extends() {
    // Template-bypass analogue of the spawning-kind C1: an extended ruleset hides
    // the `git_untrack` fix in a `templates:` block referenced by a
    // `kind`-less `extends_template:` rule; the template's `fix:` splices in at
    // finalize, after the rule-level gate. The template gate must catch it.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
        &base,
        "version: 1\ntemplates:\n  - id: ut\n    kind: file_absent\n    paths: \"**/*\"\n    git_tracked_only: true\n    fix:\n      git_untrack: {}\nrules:\n  - id: pwned\n    level: error\n    extends_template: ut\n",
    )
    .unwrap();
    std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();
    let err = load(&child).unwrap_err().to_string();
    assert!(err.contains("git_untrack"), "op not named: {err}");
    assert!(err.contains("base.yml"), "source not named: {err}");
    assert!(err.contains("arbitrary code"), "{err}");
}

#[test]
fn finalize_rejects_a_top_level_git_untrack_template() {
    // Source-agnostic backstop: a spawning fix op may never live in a `templates:`
    // block (a latent bypass the moment the config is extended), so even a
    // top-level `git_untrack` template is a hard error -- declare the fix directly
    // on a rule. Mirrors `finalize_rejects_a_top_level_spawning_template`.
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join(".alint.yml");
    std::fs::write(
        &cfg,
        "version: 1\ntemplates:\n  - id: ut\n    kind: file_absent\n    paths: \"**/*\"\n    git_tracked_only: true\n    fix:\n      git_untrack: {}\nrules:\n  - id: x\n    level: error\n    extends_template: ut\n",
    )
    .unwrap();
    let err = load(&cfg).unwrap_err().to_string();
    assert!(err.contains("git_untrack"), "{err}");
    assert!(err.contains("templates"), "{err}");
}

#[test]
fn finalize_rejects_a_require_nested_git_untrack_in_a_top_level_template() {
    // Defense-in-depth (audit A1): the finalize template backstop must recurse
    // `require:`, matching the per-source gate. A spawning fix buried in a
    // top-level template's `require:` block would otherwise LOAD (the backstop
    // scanned only the template's top-level `fix:`), leaving the "no spawning fix
    // in ANY template, EVERY source" invariant enforced non-uniformly.
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join(".alint.yml");
    std::fs::write(
        &cfg,
        "version: 1\ntemplates:\n  - id: t\n    kind: for_each_dir\n    select: \"**/\"\n    require:\n      - id: inner\n        kind: file_absent\n        paths: \"*\"\n        git_tracked_only: true\n        fix:\n          git_untrack: {}\nrules:\n  - id: x\n    level: error\n    extends_template: t\n",
    )
    .unwrap();
    let err = load(&cfg).unwrap_err().to_string();
    assert!(
        err.contains("git_untrack"),
        "require-nested op not gated: {err}"
    );
    assert!(err.contains("templates"), "{err}");
}

#[test]
fn load_allows_git_untrack_in_the_users_top_level_config() {
    // No over-rejection: the whole point of the gate is that a `git_untrack` fix
    // in the USER'S OWN top-level `rules:` is allowed (it is their explicit call
    // to let alint run `git rm --cached`). Only inherited sources are refused.
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join(".alint.yml");
    std::fs::write(
        &cfg,
        "version: 1\nrules:\n  - id: no-tracked-build\n    kind: file_absent\n    paths: \"build/**\"\n    git_tracked_only: true\n    level: error\n    fix:\n      git_untrack: {}\n",
    )
    .unwrap();
    let cfg = load(&cfg).expect("a top-level git_untrack fix must load");
    assert_eq!(cfg.rules.len(), 1, "the git_untrack rule is kept");
}

#[test]
fn load_rejects_fix_promotion_template_smuggled_via_extends() {
    // Round-7 (arbitrary file DELETION bypass): an extended ruleset can't
    // promote `file_remove` to Safe on a `rules:` entry (caught by
    // `reject_fix_promotion_in`), but it could hide the promotion in a
    // `templates:` block referenced by a `kind`-less `extends_template:` rule.
    // The template expands into the rule at finalize, *after* the rule-level
    // gate -- so without the template gate a bare `alint fix` would irreversibly
    // DELETE files the moment the user adds one `extends:` line. Mirrors the
    // spawning-template bypass gate above.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
            &base,
            "version: 1\ntemplates:\n  - id: rm\n    kind: file_absent\n    paths: \"*.log\"\n    fix: { file_remove: { applicability: safe } }\nrules:\n  - id: no-logs\n    level: error\n    extends_template: rm\n",
        )
        .unwrap();
    std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();
    let err = load(&child).unwrap_err().to_string();
    assert!(
        err.contains("applicability: safe"),
        "promotion not named: {err}"
    );
    assert!(err.contains("base.yml"), "source not named: {err}");
    assert!(err.contains("top-level"), "{err}");
}

#[test]
fn load_allows_a_non_promoting_template_via_extends() {
    // No over-rejection: an inherited template with a DEFAULT (Unsafe)
    // `file_remove` -- the common case -- must still load. Only a `safe`
    // promotion is refused.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
            &base,
            "version: 1\ntemplates:\n  - id: rm\n    kind: file_absent\n    paths: \"*.log\"\n    fix: { file_remove: {} }\nrules:\n  - id: no-logs\n    level: error\n    extends_template: rm\n",
        )
        .unwrap();
    std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();
    assert!(
        load(&child).is_ok(),
        "a non-promoting inherited template must load"
    );
}

#[test]
fn top_level_command_rule_still_loads() {
    // Guard against over-rejection: a process-spawning rule declared
    // directly in the user's own top-level `rules:` is the allowed case
    // and must keep working.
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join(".alint.yml");
    std::fs::write(
            &cfg,
            "version: 1\nrules:\n  - id: run-true\n    kind: command\n    command: [\"true\"]\n    paths: \"**/*\"\n    level: error\n",
        )
        .unwrap();
    let loaded = load(&cfg).expect("a top-level command rule should still load");
    assert_eq!(loaded.rules.len(), 1);
}

#[test]
fn load_allows_command_rule_in_top_level_config() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(".alint.yml");
    std::fs::write(
        &path,
        r#"version: 1
rules:
  - id: shellcheck
    kind: command
    paths: "**/*.sh"
    command: ["shellcheck", "{path}"]
    level: error
"#,
    )
    .unwrap();
    let cfg = load(&path).unwrap();
    assert_eq!(cfg.rules.len(), 1);
    assert_eq!(cfg.rules[0].id, "shellcheck");
}

#[test]
fn load_rejects_remote_extends_with_nested_extends() {
    use sha2::{Digest, Sha256};

    let remote_body = b"version: 1\nextends: [./chained.yml]\nrules: []\n";
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
    cache.put(&sri, remote_body).unwrap();

    let url = format!("https://example.invalid/base.yml#{sri_str}");
    let config_path = tmp.path().join(".alint.yml");
    std::fs::write(
        &config_path,
        format!("version: 1\nextends: [\"{url}\"]\nrules: []\n"),
    )
    .unwrap();

    let opts = LoadOptions::with_cache(cache);
    let err = load_with(&config_path, &opts).unwrap_err().to_string();
    assert!(err.contains("nested remote extends"), "{err}");
}

#[test]
fn load_merges_facts_with_id_dedup() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
        &base,
        r"version: 1
facts:
  - id: is_rust
    any_file_exists: [Cargo.toml]
  - id: only_base
    any_file_exists: [B]
rules: []
",
    )
    .unwrap();
    std::fs::write(
        &child,
        r"version: 1
extends: [./base.yml]
facts:
  - id: is_rust
    any_file_exists: [Cargo.toml, rust-toolchain.toml]
  - id: only_child
    any_file_exists: [C]
rules: []
",
    )
    .unwrap();
    let cfg = load(&child).unwrap();
    let ids: Vec<&str> = cfg.facts.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(ids, vec!["is_rust", "only_base", "only_child"]);
}

#[test]
fn load_resolves_transitive_extends() {
    // a.yml extends b.yml extends c.yml; check that every level's
    // rules flow through, and overrides happen at the leaf.
    let tmp = tempfile::tempdir().unwrap();
    let c = tmp.path().join("c.yml");
    let b = tmp.path().join("b.yml");
    let a = tmp.path().join("a.yml");
    std::fs::write(
        &c,
        r"version: 1
rules:
  - id: from-c
    kind: file_exists
    paths: C
    level: warning
",
    )
    .unwrap();
    std::fs::write(
        &b,
        r"version: 1
extends: [./c.yml]
rules:
  - id: from-b
    kind: file_exists
    paths: B
    level: warning
",
    )
    .unwrap();
    std::fs::write(
        &a,
        r"version: 1
extends: [./b.yml]
rules:
  - id: from-a
    kind: file_exists
    paths: A
    level: warning
",
    )
    .unwrap();
    let cfg = load(&a).unwrap();
    let ids: Vec<&str> = cfg.rules.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, vec!["from-c", "from-b", "from-a"]);
}

#[test]
fn in_crate_schema_matches_root() {
    // Guard against drift between the in-crate copy (embedded by
    // `include_str!`) and the root `schemas/v1/config.json` that the
    // public URL serves.
    //
    // The crate-tarball context (`cargo publish` strips the root
    // schemas/ tree) skips the assertion — but only when we can
    // POSITIVELY identify that we are running from a tarball, not
    // silently every time the file fails to read. Workspace context
    // is detected by a co-located workspace `Cargo.lock`; absence
    // of that lock means we are unpacked outside the workspace and
    // the test correctly bows out. Presence + a missing root schema
    // is a real failure (someone deleted the canonical copy) and is
    // now flagged, not papered over.
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_lock = manifest_dir.join("../../Cargo.lock");
    if !workspace_lock.is_file() {
        return; // crate-tarball context — workspace Cargo.lock absent.
    }
    let root = manifest_dir.join("../../schemas/v1/config.json");
    let canonical = std::fs::read_to_string(&root).unwrap_or_else(|e| {
        panic!(
            "workspace context detected (../../Cargo.lock exists) but the \
                 canonical schema at {} is unreadable: {e}",
            root.display()
        )
    });
    assert_eq!(
        canonical, CONFIG_SCHEMA_V1,
        "crates/alint-dsl/schemas/v1/config.json has drifted from \
             schemas/v1/config.json — run `cp schemas/v1/config.json \
             crates/alint-dsl/schemas/v1/config.json` to resync",
    );
}

#[test]
fn rejects_duplicate_ids() {
    let yaml = r"
version: 1
rules:
  - id: dupe
    kind: file_exists
    level: error
    paths: A
  - id: dupe
    kind: file_exists
    level: error
    paths: B
";
    assert!(parse(yaml).is_err());
}

// -----------------------------------------------------------
// Nested `.alint.yml` discovery
// -----------------------------------------------------------

#[test]
fn nested_discovery_scopes_rules_to_subtree() {
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(
        &root_cfg,
        r"version: 1
nested_configs: true
rules: []
",
    )
    .unwrap();

    // Nested config at packages/foo
    let pkg_dir = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let nested_cfg = pkg_dir.join(".alint.yml");
    std::fs::write(
        &nested_cfg,
        r#"version: 1
rules:
  - id: foo-readme
    kind: file_exists
    paths: "README.md"
    level: error
"#,
    )
    .unwrap();

    let cfg = load(&root_cfg).unwrap();
    assert_eq!(cfg.rules.len(), 1);
    let rule = &cfg.rules[0];
    assert_eq!(rule.id, "foo-readme");
    // The path should now be prefixed with the nested dir.
    // PathsSpec doesn't implement Serialize, so Debug is
    // the readable path to its contents in a test.
    let paths_dbg = format!("{:?}", rule.paths);
    assert!(
        paths_dbg.contains("packages/foo/README.md"),
        "expected scoped path, got: {paths_dbg}"
    );
}

#[test]
fn nested_baseline_is_rejected() {
    // A nested config may not declare `baseline:` — it's a trusted,
    // root-only input (a subtree must not pick what the gate suppresses).
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(&root_cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join(".alint.yml"),
        "version: 1\nbaseline: sneaky.json\nrules: []\n",
    )
    .unwrap();
    let err = load(&root_cfg).unwrap_err();
    assert!(err.to_string().contains("baseline"), "{err}");
}

#[test]
fn nested_allow_out_of_root_is_rejected() {
    // A nested config may not declare `allow_out_of_root:` — the
    // out-of-root escape hatch is a trusted, root-only grant (a subtree
    // must not grant itself reads outside the repo root). Parallels
    // `nested_baseline_is_rejected`; both close the silent-drop gap where
    // the key parsed into the config but was ignored without feedback,
    // unlike every other root-only key.
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(&root_cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join(".alint.yml"),
        "version: 1\nallow_out_of_root: true\nrules: []\n",
    )
    .unwrap();
    let err = load(&root_cfg).unwrap_err();
    assert!(err.to_string().contains("allow_out_of_root"), "{err}");
}

#[test]
fn nested_trusted_extends_is_rejected() {
    // A nested config may not declare `trusted_extends:` -- it is a trusted,
    // root-only grant (a subtree must not allowlist a remote ruleset's content
    // fixers). Parallels `nested_baseline_is_rejected`; closes the silent-drop gap
    // where the key parsed but was ignored without feedback (W2 audit).
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(&root_cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join(".alint.yml"),
        "version: 1\ntrusted_extends: [\"https://x.example/r.yml\"]\nrules: []\n",
    )
    .unwrap();
    let err = load(&root_cfg).unwrap_err();
    assert!(err.to_string().contains("trusted_extends"), "{err}");
}

#[test]
fn nested_command_rule_is_rejected() {
    // C2 (RCE bypass): a nested `.alint.yml` is untrusted like an
    // `extends:`'d ruleset (anyone who can open a monorepo PR can add
    // one), so it may not declare a process-spawning rule. Without this
    // gate a subtree config running `kind: command` achieved arbitrary
    // code execution on `alint check`. Parallels the `extends:` gate and
    // the root-only `nested_baseline`/`nested_allow_out_of_root` checks.
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(&root_cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
            pkg_dir.join(".alint.yml"),
            "version: 1\nrules:\n  - id: sneaky\n    kind: command\n    command: [\"sh\", \"-c\", \"echo pwn\"]\n    paths: \"**/*\"\n    level: error\n",
        )
        .unwrap();
    let err = load(&root_cfg).unwrap_err().to_string();
    assert!(err.contains("command"), "{err}");
    assert!(err.contains("arbitrary code"), "{err}");
}

#[test]
fn nested_config_rejects_a_git_untrack_fix() {
    // The nested-config analogue: a subtree `.alint.yml` is untrusted like an
    // `extends:`'d ruleset, so a `git_untrack` fix it declares (which shells out
    // to `git rm --cached`) must be refused -- otherwise a monorepo PR adding one
    // subtree config grants it git access on a bare `alint fix`.
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(&root_cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join(".alint.yml"),
        "version: 1\nrules:\n  - id: sneaky-untrack\n    kind: file_absent\n    paths: \"**/*\"\n    git_tracked_only: true\n    level: error\n    fix:\n      git_untrack: {}\n",
    )
    .unwrap();
    let err = load(&root_cfg).unwrap_err().to_string();
    assert!(err.contains("git_untrack"), "op not gated in nested: {err}");
    assert!(err.contains("arbitrary code"), "{err}");
}

#[test]
fn nested_config_demotes_a_content_fixer_to_suggestion() {
    // AUDIT (nested-config HIGH): a subtree `.alint.yml` is untrusted like an
    // `extends:`'d ruleset, so a CONTENT-INJECTING fixer it declares must be demoted
    // to a suggestion -- NEVER auto-applied on a bare `alint fix`. Without this a
    // nested `file_create` silently creates a file, and because its explicit `path`
    // is not re-scoped to the subtree it can land at the repo ROOT (e.g. a
    // `.github/workflows/` CI job -> code execution once pushed). Teeth: dropping the
    // `demote_content_fixers_in` call in nested.rs reverts this to `None` and reds.
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(&root_cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join(".alint.yml"),
        "version: 1\nrules:\n  - id: sneaky-create\n    kind: file_exists\n    \
         paths: \"README.md\"\n    level: error\n    fix:\n      file_create:\n        \
         path: pwn.txt\n        content: \"x\"\n",
    )
    .unwrap();
    let cfg = load(&root_cfg).unwrap();
    let rule = cfg.rules.iter().find(|r| r.id == "sneaky-create").unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "a nested content fixer must be demoted to suggestion, not auto-applied"
    );
}

#[test]
fn nested_rule_instantiating_a_root_template_is_demoted() {
    // A nested rule has no inline fixer to cap before template expansion. Its
    // provenance marker must survive scoping and cause the root template's
    // effective content fixer to become Suggestion at finalize.
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(
        &root_cfg,
        "version: 1\nnested_configs: true\ntemplates:\n  - id: inject\n    \
         kind: file_content_forbidden\n    pattern: TODO\n    level: error\n    \
         fix: { replace: { replacement: DONE } }\nrules: []\n",
    )
    .unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join(".alint.yml"),
        "version: 1\nrules:\n  - id: nested-template\n    extends_template: inject\n    \
         paths: \"*.txt\"\n",
    )
    .unwrap();

    let cfg = load(&root_cfg).unwrap();
    let rule = cfg
        .rules
        .iter()
        .find(|r| r.id == "nested-template")
        .unwrap();
    assert_eq!(
        declared_content_tier(rule),
        Some(alint_core::Applicability::Suggestion),
        "a nested rule's root-template fixer must be capped after expansion"
    );
}

#[test]
fn nested_templates_are_rejected() {
    // A nested config may not declare `templates:` — they're root-only
    // (a nested template would be silently dropped), and refusing them
    // closes the nested variant of the spawning-template smuggle.
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(&root_cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
            pkg_dir.join(".alint.yml"),
            "version: 1\ntemplates:\n  - id: t\n    kind: file_exists\n    paths: \"README.md\"\n    level: error\nrules: []\n",
        )
        .unwrap();
    let err = load(&root_cfg).unwrap_err().to_string();
    assert!(err.contains("templates"), "{err}");
}

#[test]
fn load_rejects_spawning_kind_nested_in_a_require_block() {
    // Third spawn vector (found in adversarial review): `for_each_dir` /
    // `for_each_file` / `every_matching_has` carry a `require:` block of
    // nested rules whose `kind` flattens into the parent's options. An
    // extends:'d ruleset could hide a `command` there — the top-level
    // `kind` check (and a post-finalize scan) miss it, so the gate must
    // recurse into `require:`.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base.yml");
    let child = tmp.path().join(".alint.yml");
    std::fs::write(
            &base,
            "version: 1\nrules:\n  - id: pwn\n    kind: for_each_dir\n    select: \"**/\"\n    require:\n      - kind: command\n        command: [\"sh\", \"-c\", \"echo pwn\"]\n        level: error\n    level: error\n",
        )
        .unwrap();
    std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();
    let err = load(&child).unwrap_err().to_string();
    assert!(err.contains("command"), "kind not named: {err}");
    assert!(err.contains("arbitrary code"), "{err}");
}

#[test]
fn nested_config_rejects_spawning_kind_in_a_require_block() {
    // The same `require:` vector via a nested `.alint.yml` (under
    // nested_configs). The spawn gate runs before scoping, so it catches
    // the buried `command`.
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(&root_cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
            pkg_dir.join(".alint.yml"),
            "version: 1\nrules:\n  - id: pwn\n    kind: for_each_dir\n    select: \"**/\"\n    require:\n      - kind: command\n        command: [\"sh\", \"-c\", \"echo pwn\"]\n        level: error\n    level: error\n",
        )
        .unwrap();
    let err = load(&root_cfg).unwrap_err().to_string();
    assert!(err.contains("command"), "{err}");
    assert!(err.contains("arbitrary code"), "{err}");
}

#[test]
fn nested_discovery_ignored_when_flag_is_false() {
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(
        &root_cfg,
        // No nested_configs field → defaults to false.
        r"version: 1
rules: []
",
    )
    .unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join(".alint.yml"),
        r#"version: 1
rules:
  - id: foo-readme
    kind: file_exists
    paths: "README.md"
    level: error
"#,
    )
    .unwrap();

    let cfg = load(&root_cfg).unwrap();
    assert!(
        cfg.rules.is_empty(),
        "nested rule leaked in without the opt-in: {cfg:?}"
    );
}

#[test]
fn nested_id_collision_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(
        &root_cfg,
        r#"version: 1
nested_configs: true
rules:
  - id: collision
    kind: file_exists
    paths: "root.md"
    level: error
"#,
    )
    .unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join(".alint.yml"),
        r#"version: 1
rules:
  - id: collision
    kind: file_exists
    paths: "other.md"
    level: warning
"#,
    )
    .unwrap();

    let err = load(&root_cfg).unwrap_err().to_string();
    assert!(
        err.contains("collision"),
        "error should name the rule: {err}"
    );
    assert!(
        err.contains("redefines") || err.contains("nested"),
        "error should explain what happened: {err}"
    );
}

#[test]
fn nested_rule_without_scope_field_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(
        &root_cfg,
        r"version: 1
nested_configs: true
rules: []
",
    )
    .unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join(".alint.yml"),
        // no_submodules has no path field — can't be scoped.
        r"version: 1
rules:
  - id: no-subs
    kind: no_submodules
    level: error
",
    )
    .unwrap();

    let err = load(&root_cfg).unwrap_err().to_string();
    assert!(
        err.contains("no path-like scope"),
        "error should explain the missing scope field: {err}"
    );
}

#[test]
fn nested_absolute_path_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(
        &root_cfg,
        r"version: 1
nested_configs: true
rules: []
",
    )
    .unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join(".alint.yml"),
        // Absolute path would escape the subtree.
        r#"version: 1
rules:
  - id: absolute
    kind: file_exists
    paths: "/etc/foo"
    level: error
"#,
    )
    .unwrap();

    let err = load(&root_cfg).unwrap_err().to_string();
    assert!(
        err.contains("absolute") && err.contains("escape"),
        "error should explain path constraint: {err}"
    );
}

#[test]
fn nested_path_negation_is_preserved() {
    // Verifies the scope helper correctly re-prefixes `!pattern`
    // so negated globs still sit inside the nested subtree.
    assert_eq!(
        nested::scope_glob("!src/**/*.test.ts", "packages/foo").unwrap(),
        "!packages/foo/src/**/*.test.ts"
    );
}

#[test]
fn discover_finds_config_in_starting_directory() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join(".alint.yml"), "version: 1\nrules: []\n").unwrap();
    let found = discover(tmp.path()).expect("config should be found");
    assert_eq!(found.file_name().unwrap(), ".alint.yml");
}

#[test]
fn discover_walks_up_to_find_ancestor_config() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join(".alint.yml"), "version: 1\nrules: []\n").unwrap();
    let nested = tmp.path().join("a/b/c");
    std::fs::create_dir_all(&nested).unwrap();
    let found = discover(&nested).expect("ancestor config should be found");
    assert_eq!(found, tmp.path().join(".alint.yml"));
}

#[test]
fn discover_walks_up_from_a_relative_start() {
    let cwd = std::env::current_dir().unwrap();
    let tmp = tempfile::Builder::new()
        .prefix("alint-discover-")
        .tempdir_in(&cwd)
        .unwrap();
    std::fs::write(tmp.path().join(".alint.yml"), "version: 1\nrules: []\n").unwrap();
    let nested = tmp.path().join("a/b/c");
    std::fs::create_dir_all(&nested).unwrap();
    let relative = nested.strip_prefix(&cwd).unwrap();

    let found = discover(relative).expect("ancestor config should be found");
    assert_eq!(found, tmp.path().join(".alint.yml"));
}

#[test]
fn discover_returns_none_when_no_config_exists() {
    let tmp = tempfile::tempdir().unwrap();
    // Empty tempdir, no parents have config either.
    let found = discover(tmp.path());
    // The tempdir's parent might have an alint.yml in some
    // CI environments; the strict assertion is that discover
    // either returns Some(path inside or above tempdir's
    // parent chain) or None.
    if let Some(p) = &found {
        assert!(!p.starts_with(tmp.path()), "tempdir has no config: {p:?}");
    }
}

#[test]
fn discover_prefers_nearest_config_over_ancestor() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        "version: 1\nrules: [{id: outer, kind: file_exists, paths: a, level: error}]\n",
    )
    .unwrap();
    let inner = tmp.path().join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(
        inner.join(".alint.yml"),
        "version: 1\nrules: [{id: inner, kind: file_exists, paths: b, level: error}]\n",
    )
    .unwrap();
    let found = discover(&inner).expect("inner config wins");
    assert_eq!(found, inner.join(".alint.yml"));
}

#[test]
fn discover_recognises_alternate_config_names() {
    // The loader accepts `.alint.yml`, `.alint.yaml`,
    // `alint.yml`, `alint.yaml` — `discover` mirrors that list.
    for name in [".alint.yaml", "alint.yml", "alint.yaml"] {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(name), "version: 1\nrules: []\n").unwrap();
        let found = discover(tmp.path()).expect("config should be found");
        assert_eq!(
            found.file_name().unwrap().to_str().unwrap(),
            name,
            "expected discover to find {name}",
        );
    }
}

#[test]
fn extends_diamond_inheritance_resolves_without_duplicate_rules() {
    // Diamond shape: root extends B + C, both extend D.
    // D's rule should appear once, not twice.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("d.yml"),
        "version: 1\nrules: [{id: from-d, kind: file_exists, paths: D, level: error}]\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("b.yml"),
        "version: 1\nextends: [\"./d.yml\"]\nrules: []\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("c.yml"),
        "version: 1\nextends: [\"./d.yml\"]\nrules: []\n",
    )
    .unwrap();
    let root = tmp.path().join(".alint.yml");
    std::fs::write(
        &root,
        "version: 1\nextends: [\"./b.yml\", \"./c.yml\"]\nrules: []\n",
    )
    .unwrap();
    let cfg = load(&root).unwrap();
    let from_d_count = cfg.rules.iter().filter(|r| r.id == "from-d").count();
    assert_eq!(
        from_d_count, 1,
        "diamond inheritance should yield one `from-d` rule, got {from_d_count}",
    );
}

#[test]
fn parse_rejects_a_yaml_flow_bomb_without_hanging() {
    // `parse()` is a public entry point, so untrusted YAML must be flow-guarded here
    // exactly like the file loader and the remote/bundled `extends:` bodies -- a
    // deep-flow bomb is a fast error, not a super-linear hang.
    let bomb = format!("x: {}1{}", "[".repeat(200_000), "]".repeat(200_000));
    let err = parse(&bomb).unwrap_err();
    assert!(
        err.to_string().contains("flow nesting"),
        "expected a flow-depth error, got: {err}"
    );
}

#[test]
fn extends_promoting_file_remove_to_safe_is_rejected() {
    use serde_yaml_ng::Mapping;
    let parse_rule = |y: &str| -> Mapping { serde_yaml_ng::from_str(y).unwrap() };

    // An INHERITED rule promoting `file_remove` to Safe is refused (5.5: an
    // inherited fixer may be demoted, never promoted -- an extended ruleset must
    // not silently opt a repo into auto-deleting files).
    let promote = parse_rule(
        "id: no-bak\nkind: file_absent\npaths: '**/*.bak'\nlevel: error\n\
         fix: { file_remove: { applicability: safe } }",
    );
    let err = crate::reject_fix_promotion_in(std::slice::from_ref(&promote), "./base.yml")
        .unwrap_err()
        .to_string();
    assert!(err.contains("applicability: safe"), "{err}");
    assert!(err.contains("top-level"), "{err}");

    // The gate is op-agnostic: the Phase-1 `replace` op (also Unsafe, also
    // promotable) is refused from an inherited config just the same.
    let promote_replace = parse_rule(
        "id: no-console\nkind: file_content_forbidden\npaths: '**/*.js'\nlevel: error\n\
         fix: { replace: { replacement: 'logger.debug', applicability: safe } }",
    );
    assert!(
        crate::reject_fix_promotion_in(std::slice::from_ref(&promote_replace), "./base.yml")
            .is_err(),
        "an inherited `replace` promotion to safe must be refused"
    );

    // The default (no override, so Unsafe) from an inherited config is fine.
    let plain = parse_rule(
        "id: no-bak\nkind: file_absent\npaths: '**/*.bak'\nlevel: error\n\
         fix: { file_remove: {} }",
    );
    assert!(crate::reject_fix_promotion_in(std::slice::from_ref(&plain), "./base.yml").is_ok());

    // A DEMOTE (toward suggestion) from an inherited config is allowed.
    let demote = parse_rule(
        "id: no-bak\nkind: file_absent\npaths: '**/*.bak'\nlevel: error\n\
         fix: { file_remove: { applicability: suggestion } }",
    );
    assert!(crate::reject_fix_promotion_in(std::slice::from_ref(&demote), "./base.yml").is_ok());

    // A promotion buried in a nested `require:` block is caught too.
    let nested = parse_rule(
        "id: parent\nkind: for_each_dir\npaths: '*'\nlevel: error\n\
         require:\n  - id: n\n    kind: file_absent\n    paths: '**/*.bak'\n    \
         fix: { file_remove: { applicability: safe } }",
    );
    assert!(crate::reject_fix_promotion_in(std::slice::from_ref(&nested), "./base.yml").is_err());
}

// ── audit 2026-10: trust gates vs. template substitution and op shapes ───────
// Every gate inspects raw YAML, so each test pins one way the EFFECTIVE rule
// could differ from what the gate saw: a `{{vars.*}}` placeholder resolving to a
// guarded value after the gate ran, a spawning kind nested in a template's
// `require:`, a positional (sequence-shaped) fix op, and a promotion acquired
// from a trusted template by an untrusted rule.

fn load_local_extends(base_body: &str) -> Result<alint_core::Config> {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("base.yml"), base_body).unwrap();
    let child = tmp.path().join(".alint.yml");
    std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();
    load(&child)
}

#[test]
fn template_kind_placeholder_cannot_smuggle_a_spawning_kind() {
    // `kind: "{{vars.k}}"` passed every spawn gate (they saw the placeholder) and
    // expanded into `kind: command` at finalize -- arbitrary code execution from
    // an extended ruleset. The placeholder itself is now refused.
    let err = load_local_extends(
        "version: 1\ntemplates:\n  - id: t\n    kind: \"{{vars.k}}\"\n    \
         paths: \"*.md\"\n    level: error\n    command: [\"sh\", \"-c\", \"echo pwn\"]\n\
         rules:\n  - id: innocuous\n    extends_template: t\n    vars: {k: command}\n",
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("`kind` must be a literal"), "{err}");

    // Source-agnostic: a top-level template may not parameterize `kind` either.
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join(".alint.yml");
    std::fs::write(
        &cfg,
        "version: 1\ntemplates:\n  - id: t\n    kind: \"{{vars.k}}\"\n    paths: \"*.md\"\n    \
         level: error\nrules:\n  - id: x\n    extends_template: t\n    vars: {k: file_exists}\n",
    )
    .unwrap();
    let err = load(&cfg).unwrap_err().to_string();
    assert!(err.contains("`kind` must be a literal"), "{err}");
}

#[test]
fn template_nested_require_kind_placeholder_is_refused() {
    let err = load_local_extends(
        "version: 1\ntemplates:\n  - id: t\n    kind: for_each_dir\n    select: \"pkgs/*\"\n    \
         level: error\n    require:\n      - kind: \"{{vars.k}}\"\n        paths: \"{path}/*\"\n        \
         command: [\"sh\", \"-c\", \"echo pwn\"]\nrules:\n  - id: innocuous\n    \
         extends_template: t\n    vars: {k: command}\n",
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("`kind` must be a literal"), "{err}");
}

#[test]
fn spawning_kind_nested_in_template_require_is_refused() {
    // A literal `kind: command` inside a template's `require:` block expanded into
    // its instance past both template gates, which only checked the template's
    // own top-level `kind`.
    let body = "version: 1\ntemplates:\n  - id: t\n    kind: for_each_dir\n    \
        select: \"pkgs/*\"\n    level: error\n    require:\n      - kind: command\n        \
        paths: \"{path}/*\"\n        command: [\"sh\", \"-c\", \"echo pwn\"]\nrules:\n  \
        - id: innocuous\n    extends_template: t\n";
    let err = load_local_extends(body).unwrap_err().to_string();
    assert!(err.contains("kind: command"), "{err}");
    assert!(err.contains("arbitrary code"), "{err}");

    // The finalize backstop recurses too (top-level template, no extends).
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join(".alint.yml");
    std::fs::write(&cfg, body).unwrap();
    let err = load(&cfg).unwrap_err().to_string();
    assert!(err.contains("templates"), "{err}");
    assert!(err.contains("kind: command"), "{err}");
}

#[test]
fn template_applicability_placeholder_cannot_promote_a_fix() {
    let err = load_local_extends(
        "version: 1\ntemplates:\n  - id: t\n    kind: file_absent\n    paths: \"*.md\"\n    \
         level: error\n    fix: { file_remove: { applicability: \"{{vars.a}}\" } }\n\
         rules:\n  - id: innocuous\n    extends_template: t\n    vars: {a: safe}\n",
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("applicability"), "{err}");
}

#[test]
fn sequence_shaped_fix_op_options_are_refused() {
    // `file_remove: [safe]` deserialized positionally into
    // `FileRemoveFixSpec { applicability: Some(Safe) }`, invisible to the
    // mapping-only promotion gate and demotion pass.
    for op in [
        "file_remove: [safe]",
        "replace: ['foo', 'PWNED']",
        "file_append_final_newline: []",
    ] {
        let err = load_local_extends(&format!(
            "version: 1\nrules:\n  - id: r\n    kind: file_absent\n    paths: \"*.md\"\n    \
             level: error\n    fix: {{ {op} }}\n"
        ))
        .unwrap_err()
        .to_string();
        assert!(err.contains("must be a mapping"), "{op}: {err}");
    }
    // Including in the user's own top-level config: there is no legitimate
    // positional form, so the shape is refused for every source.
    let err = parse(
        "version: 1\nrules:\n  - id: r\n    kind: file_absent\n    paths: \"*.md\"\n    \
         level: error\n    fix: { file_remove: [safe] }\n",
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("must be a mapping"), "{err}");
}

#[test]
fn untrusted_rule_cannot_aim_a_trusted_promoting_template() {
    // The user's own template promotes `file_remove` to safe for its own scope; an
    // untrusted remote instantiates it with its own `paths:`. The promotion must
    // not follow the template into the untrusted rule.
    let remote = "version: 1\nrules:\n  - id: aimed\n    extends_template: user_rm\n    \
        paths: \"**/*\"\n";
    let top_template = "templates:\n  - id: user_rm\n    kind: file_absent\n    \
        paths: \"*.bak\"\n    level: error\n    \
        fix: { file_remove: { applicability: safe } }\n";
    let cfg = load_extending(remote, top_template);
    let rule = cfg.rules.iter().find(|r| r.id == "aimed").unwrap();
    match rule.fix.as_ref().expect("aimed carries the expanded fixer") {
        alint_core::FixSpec::FileRemove { file_remove } => assert_eq!(
            file_remove.applicability, None,
            "an untrusted rule must fall back to file_remove's default (unsafe) tier"
        ),
        other => panic!("expected a FileRemove fixer, got {other:?}"),
    }
}

fn try_load_extending(remote_body: &str, top_extra: &str) -> Result<alint_core::Config> {
    let tmp = tempfile::tempdir().unwrap();
    let cache = extends::Cache::at(tmp.path().join("cache"));
    let url = seed_remote(&cache, remote_body);
    let config_path = tmp.path().join(".alint.yml");
    std::fs::write(
        &config_path,
        format!("version: 1\nextends: [\"{url}\"]\n{top_extra}"),
    )
    .unwrap();
    load_with(&config_path, &LoadOptions::with_cache(cache))
}

#[test]
fn remote_ruleset_cannot_route_an_env_var_into_since() {
    // `since: "${SECRET}"` from a remote was expanded at evaluate time and echoed
    // in the "could not resolve commit range" error -- an env exfiltration channel.
    let direct = "version: 1\nrules:\n  - id: cm\n    kind: git_commit_message\n    \
        since: \"${FAKE_SECRET_TOKEN}\"\n    subject_max_length: 72\n    level: warning\n";
    let err = try_load_extending(direct, "rules: []\n")
        .unwrap_err()
        .to_string();
    assert!(err.contains("environment variable"), "{err}");

    // ...or contributed by field-merge onto the user's own rule (no `kind`).
    let merged = "version: 1\nrules:\n  - id: cm\n    since: \"${FAKE_SECRET_TOKEN}\"\n";
    let top = "rules:\n  - id: cm\n    kind: git_commit_message\n    \
        subject_max_length: 72\n    level: warning\n";
    let err = try_load_extending(merged, top).unwrap_err().to_string();
    assert!(err.contains("since"), "{err}");

    // ...or through a template variable the user's template substitutes.
    let via_vars = "version: 1\nrules:\n  - id: cm\n    extends_template: user_cm\n    \
        vars: {base: \"${FAKE_SECRET_TOKEN}\"}\n";
    let top = "templates:\n  - id: user_cm\n    kind: git_commit_message\n    \
        since: \"{{vars.base}}\"\n    subject_max_length: 72\n    level: warning\nrules: []\n";
    let err = try_load_extending(via_vars, top).unwrap_err().to_string();
    assert!(err.contains("vars.base"), "{err}");

    // A literal ref from a remote is fine.
    let literal = "version: 1\nrules:\n  - id: cm\n    kind: git_commit_message\n    \
        since: origin/main\n    subject_max_length: 72\n    level: warning\n";
    assert!(try_load_extending(literal, "rules: []\n").is_ok());
}

#[test]
fn drop_in_does_not_reset_unset_top_level_settings() {
    // Audit 2026-10: a drop-in that omits `respect_gitignore` / `fix_size_limit`
    // / `nested_configs` (or `version`) used to overwrite the main config's
    // explicit value with the serde default.
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join(".alint.yml");
    std::fs::write(
        &cfg,
        "version: 1\nrespect_gitignore: false\nfix_size_limit: null\nnested_configs: true\nrules: []\n",
    )
    .unwrap();
    std::fs::create_dir(tmp.path().join(".alint.d")).unwrap();
    std::fs::write(tmp.path().join(".alint.d/50-team.yml"), "rules: []\n").unwrap();
    let c = load(&cfg).unwrap();
    assert!(!c.respect_gitignore);
    assert_eq!(c.fix_size_limit, None);
    assert!(c.nested_configs);
    assert_eq!(c.version, 1);

    // A drop-in that DOES set a value still wins.
    std::fs::write(
        tmp.path().join(".alint.d/60-local.yml"),
        "respect_gitignore: true\nfix_size_limit: 10\nrules: []\n",
    )
    .unwrap();
    let c = load(&cfg).unwrap();
    assert!(c.respect_gitignore);
    assert_eq!(c.fix_size_limit, Some(10));
}

#[test]
fn extended_config_top_level_settings_are_ignored() {
    // An extended config's top-level settings were never honored (the extending
    // config's default replaced them); keep that explicit now that unset values
    // no longer clobber set ones.
    let c = load_local_extends(
        "version: 1\nrespect_gitignore: false\nfix_size_limit: null\nrules: []\n",
    )
    .unwrap();
    assert!(c.respect_gitignore);
    assert_eq!(c.fix_size_limit, Some(1 << 20));
}

#[test]
fn extends_entry_typo_is_a_load_error() {
    // `excpet:` used to be ignored, silently loading every rule including the
    // one the user meant to drop.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("base.yml"),
        "version: 1\nrules:\n  - id: r1\n    kind: file_exists\n    paths: README.md\n    \
         level: warning\n",
    )
    .unwrap();
    let cfg = tmp.path().join(".alint.yml");
    std::fs::write(
        &cfg,
        "version: 1\nextends:\n  - url: ./base.yml\n    excpet: [r1]\nrules: []\n",
    )
    .unwrap();
    let err = load(&cfg).unwrap_err().to_string();
    assert!(err.contains("excpet"), "{err}");

    std::fs::write(
        &cfg,
        "version: 1\nextends:\n  - url: ./base.yml\n    except: [r1]\nrules: []\n",
    )
    .unwrap();
    assert!(load(&cfg).unwrap().rules.is_empty());
}

#[test]
fn fact_with_extra_keys_is_a_load_error() {
    for (body, needle) in [
        (
            "facts:\n  - id: f\n    any_file_exists: a\n    count_files: \"*\"\n",
            "exactly one kind",
        ),
        (
            "facts:\n  - id: f\n    any_file_exists: a\n    bogus: 1\n",
            "unknown field `bogus`",
        ),
        ("facts:\n  - id: f\n", "exactly one kind"),
    ] {
        let err = parse(&format!("version: 1\n{body}rules: []\n"))
            .unwrap_err()
            .to_string();
        assert!(err.contains(needle), "{body}: {err}");
    }
    let ok =
        parse("version: 1\nfacts:\n  - id: f\n    any_file_exists: [a, b]\nrules: []\n").unwrap();
    assert_eq!(ok.facts[0].kind.name(), "any_file_exists");
}

#[test]
fn parse_error_in_an_extended_config_names_that_file() {
    let err = load_local_extends("version: 1\nrulez: []\n")
        .unwrap_err()
        .to_string();
    assert!(err.contains("base.yml"), "{err}");
    // The serde message appears once (it used to repeat as the error's source).
    assert_eq!(err.matches("unknown field").count(), 1, "{err}");
}

#[test]
fn nested_configs_enabled_matches_load() {
    // Same resolution as `load`: a `.alint.d/` drop-in may enable it, an
    // `extends:`'d ruleset's top-level settings are dropped.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("base.yml"),
        "version: 1\nnested_configs: true\nrules: []\n",
    )
    .unwrap();
    let top = tmp.path().join(".alint.yml");
    std::fs::write(&top, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();
    assert_eq!(
        nested_configs_enabled(&top).unwrap(),
        load(&top).unwrap().nested_configs
    );
    assert!(!nested_configs_enabled(&top).unwrap());
    std::fs::create_dir(tmp.path().join(".alint.d")).unwrap();
    std::fs::write(
        tmp.path().join(".alint.d/10.yml"),
        "version: 1\nnested_configs: true\nrules: []\n",
    )
    .unwrap();
    assert!(nested_configs_enabled(&top).unwrap());
    assert!(load(&top).unwrap().nested_configs);
}

#[test]
fn remote_yaml_tags_cannot_hide_fields_from_the_trust_gates() {
    // Audit R2 (CRITICAL): the gates read raw mappings with `get("kind")` /
    // `as_str()`, which miss a tagged KEY (`!x kind:`) or a tagged VALUE
    // (`kind: !x command`), while serde strips the tag and builds the real field.
    // Each of these loaded and then spawned / auto-deleted / read the env.
    let cases = [
        (
            "!x kind: command\n    command: [\"sh\", \"-c\", \"touch pwned\"]\n    \
             paths: \"*.txt\"",
            "rules[0]",
        ),
        (
            "kind: !x command\n    command: [\"sh\", \"-c\", \"touch pwned\"]\n    \
             paths: \"*.txt\"",
            "rules[0].kind",
        ),
        (
            "kind: file_absent\n    paths: \"*.txt\"\n    \
             !x fix: {file_remove: {applicability: safe}}",
            "rules[0]",
        ),
        (
            "kind: file_absent\n    paths: \"*.txt\"\n    \
             fix: {file_remove: {applicability: !x safe}}",
            "rules[0].fix.file_remove.applicability",
        ),
        (
            "kind: git_commit_message\n    subject_max_length: 72\n    \
             !x since: \"${FAKE_SECRET_TOKEN}\"",
            "rules[0]",
        ),
        (
            "kind: file_exists\n    paths: README.md\n    <<: {kind: command}",
            "merge key",
        ),
        (
            "kind: file_exists\n    paths: README.md\n    1: x",
            "non-string key",
        ),
    ];
    for (fields, needle) in cases {
        let remote = format!("version: 1\nrules:\n  - id: r\n    level: error\n    {fields}\n");
        let err = try_load_extending(&remote, "rules: []\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("example.invalid"), "{fields}: {err}");
        assert!(err.contains(needle), "{fields}: {err}");
    }
    // A tagged key inside a template is refused the same way.
    let remote = "version: 1\ntemplates:\n  - id: t\n    !x kind: command\n    \
        command: [\"true\"]\n    level: error\nrules: []\n";
    let err = try_load_extending(remote, "rules: []\n")
        .unwrap_err()
        .to_string();
    assert!(err.contains("templates[0]"), "{err}");
}

#[test]
fn yaml_tags_are_refused_in_local_and_nested_configs() {
    // Every entry point shares the check: the top-level config (both the
    // interpolating and the fast parse path), a local `extends:` target, and a
    // nested config (whose `!x paths:` would otherwise skip subtree scoping).
    let tag = "version: 1\nrules:\n  - id: r\n    kind: file_exists\n    \
        !x paths: README.md\n    level: error\n";
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join(".alint.yml");
    std::fs::write(&cfg, tag).unwrap();
    let err = load(&cfg).unwrap_err().to_string();
    assert!(err.contains("YAML tag `!x`"), "{err}");
    std::fs::write(
        &cfg,
        format!("{tag}    message: \"{{{{env.HOME | default('x')}}}}\"\n"),
    )
    .unwrap();
    let err = load(&cfg).unwrap_err().to_string();
    assert!(err.contains("YAML tag `!x`"), "{err}");

    let err = load_local_extends(tag).unwrap_err().to_string();
    assert!(err.contains("base.yml"), "{err}");

    std::fs::write(&cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
    let pkg = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join(".alint.yml"),
        "version: 1\nrules:\n  - id: n\n    kind: file_exists\n    \
         !x paths: ../../README.md\n    level: error\n",
    )
    .unwrap();
    let err = load(&cfg).unwrap_err().to_string();
    assert!(err.contains("YAML tag `!x`"), "{err}");

    // The core `!!str` tag is resolved by the parser and stays accepted.
    std::fs::write(
        &cfg,
        "version: 1\nrules:\n  - id: r\n    !!str kind: file_exists\n    \
         paths: README.md\n    level: error\n",
    )
    .unwrap();
    assert_eq!(load(&cfg).unwrap().rules[0].kind, "file_exists");
}

#[test]
fn extended_source_cannot_field_merge_into_a_spawning_rule() {
    // Audit R2 (HIGH): a remote `{id: gen-fresh, workdir: vendor/evilpkg}` has no
    // `kind`, so it passed every per-source gate, then field-merged into the
    // user's own `generated_file_fresh` rule, which ran `vendor/evilpkg/gen.sh`.
    let top = "rules:\n  - id: gen-fresh\n    kind: generated_file_fresh\n    \
        file: out.txt\n    command: [\"./gen.sh\"]\n    level: error\n";
    let remote = "version: 1\nrules:\n  - id: gen-fresh\n    workdir: vendor/evilpkg\n";
    let err = try_load_extending(remote, top).unwrap_err().to_string();
    assert!(err.contains("generated_file_fresh"), "{err}");
    assert!(err.contains("example.invalid"), "{err}");

    // The converse: the user's spawning rule shares the id of a remote rule, so
    // the remote's `paths:` would choose what the command runs on.
    let remote = "version: 1\nrules:\n  - id: lint\n    kind: file_exists\n    \
        paths: \"vendor/**\"\n    level: error\n";
    let top = "rules:\n  - id: lint\n    kind: command\n    \
        command: [\"true\"]\n";
    let err = try_load_extending(remote, top).unwrap_err().to_string();
    assert!(err.contains("`kind: command`"), "{err}");

    // ...or the spawning rule instantiates a template an extended config defines.
    let remote = "version: 1\ntemplates:\n  - id: t\n    paths: \"vendor/**\"\n    \
        level: error\nrules: []\n";
    let top = "rules:\n  - id: mine\n    extends_template: t\n    kind: command\n    \
        command: [\"true\"]\n";
    let err = try_load_extending(remote, top).unwrap_err().to_string();
    assert!(err.contains("template"), "{err}");

    // ...including a spawning FIX op on an otherwise ordinary kind.
    let remote = "version: 1\nrules:\n  - id: untrack\n    paths: \"**/*\"\n";
    let top = "rules:\n  - id: untrack\n    kind: file_absent\n    paths: \"*.log\"\n    \
        level: error\n    fix: { git_untrack: {} }\n";
    let err = try_load_extending(remote, top).unwrap_err().to_string();
    assert!(err.contains("fix.git_untrack"), "{err}");

    // A local `extends:` is an extended source too, and the mark survives a chain.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("leaf.yml"),
        "version: 1\nrules:\n  - id: gen-fresh\n    workdir: vendor/evilpkg\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("mid.yml"),
        "version: 1\nextends: [./leaf.yml]\nrules: []\n",
    )
    .unwrap();
    let cfg = tmp.path().join(".alint.yml");
    std::fs::write(
        &cfg,
        "version: 1\nextends: [./mid.yml]\nrules:\n  - id: gen-fresh\n    \
         kind: generated_file_fresh\n    file: out.txt\n    command: [\"./gen.sh\"]\n    \
         level: error\n",
    )
    .unwrap();
    let err = load(&cfg).unwrap_err().to_string();
    assert!(err.contains("leaf.yml"), "{err}");

    // Unaffected: an extended config tuning an ordinary rule, and a spawning rule
    // the user declares (and a drop-in tunes) without any extended contribution.
    let remote = "version: 1\nrules:\n  - id: readme\n    level: warning\n";
    let top = "rules:\n  - id: readme\n    kind: file_exists\n    paths: README.md\n    \
        level: error\n  - id: gen-fresh\n    kind: generated_file_fresh\n    \
        file: out.txt\n    command: [\"./gen.sh\"]\n    level: error\n";
    let cfg = try_load_extending(remote, top).unwrap();
    assert_eq!(cfg.rules.len(), 2);
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join(".alint.yml");
    std::fs::write(
        &cfg,
        "version: 1\nrules:\n  - id: gen-fresh\n    kind: generated_file_fresh\n    \
         file: out.txt\n    command: [\"./gen.sh\"]\n    level: error\n",
    )
    .unwrap();
    std::fs::create_dir(tmp.path().join(".alint.d")).unwrap();
    std::fs::write(
        tmp.path().join(".alint.d/50-local.yml"),
        "rules:\n  - id: gen-fresh\n    level: warning\n",
    )
    .unwrap();
    assert_eq!(
        load(&cfg).unwrap().rules[0].level,
        alint_core::Level::Warning
    );
}

#[test]
fn remote_template_vars_cannot_assemble_an_env_ref_in_since() {
    // Audit R2 (HIGH): the per-source gate looked for `${` in each raw value, so a
    // remote instance splitting it across two vars (`$` + `{SECRET}`) expanded
    // into `since: "${SECRET}"` through the user's own template.
    let top = "templates:\n  - id: user_cm\n    kind: git_commit_message\n    \
        since: \"{{vars.a}}{{vars.b}}\"\n    subject_max_length: 72\n    level: warning\n\
        rules: []\n";
    let remote = "version: 1\nrules:\n  - id: cm\n    extends_template: user_cm\n    \
        vars: {a: \"$\", b: \"{FAKE_SECRET_TOKEN}\"}\n";
    let err = try_load_extending(remote, top).unwrap_err().to_string();
    assert!(err.contains("`since`"), "{err}");
    assert!(err.contains("example.invalid"), "{err}");

    // ...including inside a nested `require:` rule, and when the remote only
    // contributes the `vars:` to the user's own instance by sharing its id.
    let top = "templates:\n  - id: t\n    kind: for_each_dir\n    select: \"*\"\n    \
        level: warning\n    require:\n      - kind: git_commit_message\n        \
        since: \"{{vars.a}}{{vars.b}}\"\n        subject_max_length: 72\n\
        rules:\n  - id: cm\n    extends_template: t\n";
    let remote = "version: 1\nrules:\n  - id: cm\n    \
        vars: {a: \"$\", b: \"{FAKE_SECRET_TOKEN}\"}\n";
    let err = try_load_extending(remote, top).unwrap_err().to_string();
    assert!(err.contains("require[0].since"), "{err}");

    // Unaffected: the user's own instance assembling the same value, and a remote
    // instance of a template whose `${...}` the user wrote literally.
    let top = "templates:\n  - id: user_cm\n    kind: git_commit_message\n    \
        since: \"{{vars.a}}{{vars.b}}\"\n    subject_max_length: 72\n    level: warning\n\
        rules:\n  - id: mine\n    extends_template: user_cm\n    \
        vars: {a: \"$\", b: \"{ALINT_BASE_SHA}\"}\n";
    let cfg = try_load_extending("version: 1\nrules: []\n", top).unwrap();
    assert_eq!(cfg.rules[0].extra["since"], "${ALINT_BASE_SHA}");
    let top = "templates:\n  - id: user_cm\n    kind: git_commit_message\n    \
        since: \"${ALINT_BASE_SHA}\"\n    subject_max_length: 72\n    level: warning\n\
        rules: []\n";
    let remote = "version: 1\nrules:\n  - id: cm\n    extends_template: user_cm\n";
    assert!(try_load_extending(remote, top).is_ok());
}

#[test]
fn untrusted_remote_when_cannot_read_the_environment() {
    // Audit R2: an untrusted remote's `when: env.X matches "^g"|"^h"|"^i"` rules
    // leaked a secret one character at a time through which rule fired.
    let oracle = |c: char| {
        format!(
            "  - id: probe-{c}\n    kind: file_exists\n    paths: README.md\n    \
             level: warning\n    when: env.FAKE_SECRET matches \"^{c}\"\n"
        )
    };
    let remote = format!(
        "version: 1\nrules:\n{}{}{}",
        oracle('g'),
        oracle('h'),
        oracle('i')
    );
    let err = try_load_extending(&remote, "rules: []\n")
        .unwrap_err()
        .to_string();
    assert!(
        err.contains(
            "rule `probe-g`: `when:` reads `env.FAKE_SECRET`; an untrusted extends source"
        ),
        "{err}"
    );
    assert!(err.contains("example.invalid"), "{err}");
    assert!(err.contains("trusted_extends:"), "{err}");

    // An allowlisted remote keeps working as before.
    let trusted = "trusted_extends: [\"https://example.invalid/remote.yml\"]\nrules: []\n";
    assert_eq!(try_load_extending(&remote, trusted).unwrap().rules.len(), 3);

    // Nested `require:` rules and a `when_iter:` filter are refused too.
    for body in [
        "  - id: each\n    kind: for_each_dir\n    select: \"*\"\n    level: warning\n    \
         require:\n      - kind: file_exists\n        paths: \"{path}/x\"\n        \
         when: (env.CI)\n",
        "  - id: each\n    kind: for_each_dir\n    select: \"*\"\n    level: warning\n    \
         when_iter: \"iter.has_file(env.CI)\"\n    require:\n      - kind: file_exists\n        \
         paths: \"{path}/x\"\n",
    ] {
        let err = try_load_extending(&format!("version: 1\nrules:\n{body}"), "rules: []\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("reads `env.CI`"), "{err}");
    }

    // A remote template may neither read the environment nor leave its `when`
    // to an instance's variables.
    for (when, needle) in [
        ("env.CI", "reads `env.CI`"),
        ("\"{{vars.cond}}\"", "placeholder"),
    ] {
        let remote = format!(
            "version: 1\ntemplates:\n  - id: t\n    kind: file_exists\n    \
             paths: README.md\n    level: warning\n    when: {when}\nrules: []\n"
        );
        let err = try_load_extending(&remote, "rules: []\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("template `t`"), "{err}");
        assert!(err.contains(needle), "{err}");
    }

    // An untrusted instance cannot fill a trusted template's `when` placeholder
    // with an env read.
    let top = "templates:\n  - id: gated\n    kind: file_exists\n    paths: README.md\n    \
        level: warning\n    when: \"{{vars.cond}}\"\nrules: []\n";
    let remote = "version: 1\nrules:\n  - id: probe\n    extends_template: gated\n    \
        vars: {cond: \"env.FAKE_SECRET matches '^g'\"}\n";
    let err = try_load_extending(remote, top).unwrap_err().to_string();
    assert!(err.contains("reads `env.FAKE_SECRET`"), "{err}");

    // Unaffected: a remote `when` on facts, and a remote instance of a template
    // whose env read the user wrote literally.
    let remote = "version: 1\nrules:\n  - id: r\n    kind: file_exists\n    \
        paths: README.md\n    level: warning\n    when: facts.is_rust\n";
    assert!(try_load_extending(remote, "rules: []\n").is_ok());
    let top = "templates:\n  - id: ci_only\n    kind: file_exists\n    paths: README.md\n    \
        level: warning\n    when: env.CI\nrules: []\n";
    let remote = "version: 1\nrules:\n  - id: r\n    extends_template: ci_only\n";
    assert!(try_load_extending(remote, top).is_ok());
}

#[test]
fn untrusted_remote_cannot_declare_ignore() {
    // Audit R2: a remote `ignore: ["src/**"]` silently removed files from every
    // rule, the user's own included.
    let remote = "version: 1\nignore: [\"src/**\"]\nrules: []\n";
    let err = try_load_extending(remote, "rules: []\n")
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("`ignore:` is not allowed from an untrusted extends source"),
        "{err}"
    );
    assert!(err.contains("example.invalid"), "{err}");

    // An allowlisted remote, and a local `extends:`, still contribute it.
    let trusted = "trusted_extends: [\"https://example.invalid/remote.yml\"]\nrules: []\n";
    let cfg = try_load_extending(remote, trusted).unwrap();
    assert_eq!(cfg.ignore, vec!["src/**".to_string()]);
    let cfg = load_local_extends(remote).unwrap();
    assert_eq!(cfg.ignore, vec!["src/**".to_string()]);
}

/// Like [`try_load_extending`], plus extra local files (`.alint.d/` drop-ins,
/// local `extends:` targets) written next to the top-level config first. The
/// `{remote}` token in `top_extends` is replaced by the remote's URL.
fn try_load_with_files(
    remote_body: &str,
    top_extends: &str,
    top_extra: &str,
    files: &[(&str, &str)],
) -> Result<alint_core::Config> {
    let tmp = tempfile::tempdir().unwrap();
    let cache = extends::Cache::at(tmp.path().join("cache"));
    let url = seed_remote(&cache, remote_body);
    for (rel, body) in files {
        let path = tmp.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    let config_path = tmp.path().join(".alint.yml");
    std::fs::write(
        &config_path,
        format!(
            "version: 1\nextends: [{}]\n{top_extra}",
            top_extends.replace("{remote}", &url)
        ),
    )
    .unwrap();
    load_with(&config_path, &LoadOptions::with_cache(cache))
}

#[test]
fn untrusted_remote_when_cannot_read_an_env_derived_var() {
    // Audit R2 follow-up: `vars: {token: "{{env.NPM_TOKEN}}"}` turned the
    // environment-read refusal into a `vars.token matches "^g"` oracle. The
    // `| default(...)` keeps the test independent of the real environment; the
    // value is still env-derived.
    let raw = "{{env.ALINT_TEST_UNSET_TOKEN | default('ghp_x')}}";
    let top = format!("vars:\n  token: \"{raw}\"\n  org: acme\nrules: []\n");
    let oracle = |c: char| {
        format!(
            "  - id: probe-{c}\n    kind: file_exists\n    paths: README.md\n    \
             level: warning\n    when: vars.token matches \"^{c}\"\n"
        )
    };
    let remote = format!("version: 1\nrules:\n{}{}", oracle('g'), oracle('h'));
    let err = try_load_extending(&remote, &top).unwrap_err().to_string();
    assert!(
        err.contains(&format!(
            "rule `probe-g`: `when:` reads `vars.token`, whose value comes from the \
             environment (`{raw}`); an untrusted extends source may not read the environment"
        )),
        "{err}"
    );
    assert!(err.contains("example.invalid"), "{err}");
    assert!(err.contains("trusted_extends:"), "{err}");

    // Whitespace inside the span changes nothing.
    let spaced =
        "vars:\n  token: \"{{ env . ALINT_TEST_UNSET_TOKEN | default('g') }}\"\nrules: []\n";
    let err = try_load_extending(&remote, spaced).unwrap_err().to_string();
    assert!(err.contains("reads `vars.token`, whose value"), "{err}");

    // An allowlisted remote keeps working, and so does an ordinary var.
    let trusted = format!("trusted_extends: [\"https://example.invalid/remote.yml\"]\n{top}");
    assert_eq!(
        try_load_extending(&remote, &trusted).unwrap().rules.len(),
        2
    );
    let ordinary = "version: 1\nrules:\n  - id: org\n    kind: file_exists\n    \
        paths: README.md\n    level: warning\n    when: vars.org == \"acme\"\n";
    assert!(try_load_extending(ordinary, &top).is_ok());

    // Nested `require:` / `when_iter:` reads, and a remote template's read.
    for body in [
        "rules:\n  - id: each\n    kind: for_each_dir\n    select: \"*\"\n    level: warning\n    \
         require:\n      - kind: file_exists\n        paths: \"{path}/x\"\n        \
         when: (vars.token == \"x\")\n",
        "rules:\n  - id: each\n    kind: for_each_dir\n    select: \"*\"\n    level: warning\n    \
         when_iter: \"iter.has_file(vars.token)\"\n    require:\n      - kind: file_exists\n        \
         paths: \"{path}/x\"\n",
        "templates:\n  - id: t\n    kind: file_exists\n    paths: README.md\n    \
         level: warning\n    when: vars.token == \"x\"\nrules: []\n",
    ] {
        let err = try_load_extending(&format!("version: 1\n{body}"), &top)
            .unwrap_err()
            .to_string();
        assert!(err.contains("reads `vars.token`, whose value"), "{err}");
    }

    // An untrusted instance cannot fill a trusted template's `when` placeholder
    // with a read of the env-derived var either.
    let top_tpl = format!(
        "{top}templates:\n  - id: gated\n    kind: file_exists\n    paths: README.md\n    \
         level: warning\n    when: \"{{{{vars.cond}}}}\"\n"
    );
    let remote = "version: 1\nrules:\n  - id: probe\n    extends_template: gated\n    \
        vars: {cond: \"vars.token matches '^g'\"}\n";
    let err = try_load_extending(remote, &top_tpl)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("rule `probe`: `when:` reads `vars.token`"),
        "{err}"
    );
}

#[test]
fn env_derived_var_mark_follows_the_effective_value_across_sources() {
    let remote = "version: 1\nrules:\n  - id: probe\n    kind: file_exists\n    \
        paths: README.md\n    level: warning\n    when: vars.token == \"x\"\n";
    let env = "vars:\n  token: \"{{env.ALINT_TEST_UNSET_TOKEN | default('s')}}\"\n";
    let literal = "vars:\n  token: plain\n";
    let refused = |r: Result<alint_core::Config>| {
        r.unwrap_err()
            .to_string()
            .contains("reads `vars.token`, whose value")
    };
    let only_remote = "\"{remote}\"";

    // Declared in a `.alint.d/` drop-in (merged after the remote has loaded).
    assert!(refused(try_load_with_files(
        remote,
        only_remote,
        "rules: []\n",
        &[(".alint.d/10-secret.yml", env)],
    )));
    // A later literal value clears the mark...
    assert!(
        try_load_with_files(
            remote,
            only_remote,
            &format!("{env}rules: []\n"),
            &[(".alint.d/10-plain.yml", literal)],
        )
        .is_ok()
    );
    // ...and a later env-derived value sets it again.
    assert!(refused(try_load_with_files(
        remote,
        only_remote,
        &format!("{literal}rules: []\n"),
        &[(".alint.d/10-secret.yml", env)],
    )));
    // Declared in a LOCAL `extends:` target; the top level may override it.
    let base = format!("version: 1\n{env}");
    let both = "\"./base.yml\", \"{remote}\"";
    assert!(refused(try_load_with_files(
        remote,
        both,
        "rules: []\n",
        &[("base.yml", &base)],
    )));
    assert!(
        try_load_with_files(
            remote,
            both,
            &format!("{literal}rules: []\n"),
            &[("base.yml", &base)],
        )
        .is_ok()
    );
}

#[test]
fn untrusted_remote_template_cannot_splice_an_env_derived_instance_var() {
    // The user's own instance passes an env-derived var to the user's own
    // template; a remote template sharing the id adds a field that echoes it.
    let raw = "{{env.ALINT_TEST_UNSET_TOKEN | default('s3cret')}}";
    let top = format!(
        "templates:\n  - id: tpl\n    kind: file_exists\n    paths: README.md\n    \
         level: warning\nrules:\n  - id: r\n    extends_template: tpl\n    \
         vars:\n      token: \"{raw}\"\n"
    );
    let remote = "version: 1\ntemplates:\n  - id: tpl\n    \
        message: \"leaked {{ vars.token }}\"\nrules: []\n";
    let err = try_load_extending(remote, &top).unwrap_err().to_string();
    assert!(
        err.contains(&format!(
            "template `tpl` substitutes `{{{{vars.token}}}}`, whose value comes from the \
             environment (`{raw}`)"
        )),
        "{err}"
    );
    assert!(err.contains("example.invalid"), "{err}");

    // Allowlisted: fine, and the value is substituted as before.
    let trusted = format!("trusted_extends: [\"https://example.invalid/remote.yml\"]\n{top}");
    let cfg = try_load_extending(remote, &trusted).unwrap();
    assert_eq!(cfg.rules[0].message.as_deref(), Some("leaked s3cret"));

    // A remote that touches neither the template nor the rule changes nothing,
    // even when the user's own template echoes the var.
    let own = top.replace(
        "level: warning\nrules:",
        "level: warning\n    message: \"mine {{vars.token}}\"\nrules:",
    );
    assert!(try_load_extending("version: 1\nrules: []\n", &own).is_ok());

    // A literal instance var stays usable from a remote-shaped template.
    let literal = top.replace(&format!("\"{raw}\""), "plain");
    assert!(try_load_extending(remote, &literal).is_ok());
}
