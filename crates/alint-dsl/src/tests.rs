use super::*;

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
