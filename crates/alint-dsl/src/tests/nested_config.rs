//! Nested-config discovery, rerooting, confinement, and trust boundaries.

use super::*;

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
fn nested_regex_select_is_not_rerooted() {
    // `for_each_match`'s `select:` is a line regex, not a path glob: prefixing it with
    // the nested directory (`packages/foo/TODO`) made it never match (v0.18.3).
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(&root_cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join(".alint.yml"),
        r#"version: 1
rules:
  - id: todo-lines
    kind: for_each_match
    paths: "**/*.md"
    select: '^TODO'
    require:
      matches: ['#\d+']
    level: warning
"#,
    )
    .unwrap();

    let cfg = load(&root_cfg).unwrap();
    let rule = &cfg.rules[0];
    let paths_dbg = format!("{:?}", rule.paths);
    assert!(
        paths_dbg.contains("packages/foo/**/*.md"),
        "paths not scoped: {paths_dbg}"
    );
    let select = rule.extra.get("select").and_then(|v| v.as_str());
    assert_eq!(
        select,
        Some("^TODO"),
        "the regex select must be left untouched"
    );
}

#[test]
fn nested_unconfinable_kind_is_rejected() {
    // A kind whose selectors are not `paths` / `select` / `primary` (here the commit
    // range of `git_commit_message`) cannot be confined to the subtree; a dummy
    // `paths:` used to let it run repo-wide from a nested config (v0.18.3).
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(&root_cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join(".alint.yml"),
        r#"version: 1
rules:
  - id: subject
    kind: git_commit_message
    paths: "dummy"
    pattern: '^feat'
    level: error
"#,
    )
    .unwrap();

    let err = load(&root_cfg).unwrap_err().to_string();
    assert!(
        err.contains("cannot be confined"),
        "unexpected error: {err}"
    );
}

#[test]
fn nested_rule_via_root_template_is_confined_after_expansion() {
    // A nested rule that `extends_template:`s a root template takes its kind and
    // selectors from the template. Confinement must run on the EXPANDED rule: the
    // unconfinable kind is refused, and template-provided selectors are re-rooted.
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(
        &root_cfg,
        r#"version: 1
nested_configs: true
templates:
  - id: subj
    kind: git_commit_message
    pattern: '^feat'
    level: error
  - id: per-dir
    kind: dir_contains
    select: "**"
    require: ["a.txt"]
    level: error
rules: []
"#,
    )
    .unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();

    std::fs::write(
        pkg_dir.join(".alint.yml"),
        "version: 1\nrules:\n  - id: sub-subject\n    extends_template: subj\n    paths: dummy\n",
    )
    .unwrap();
    let err = load(&root_cfg).unwrap_err().to_string();
    assert!(
        err.contains("cannot be confined"),
        "unexpected error: {err}"
    );

    std::fs::write(
        pkg_dir.join(".alint.yml"),
        "version: 1\nrules:\n  - id: sub-dirs\n    extends_template: per-dir\n",
    )
    .unwrap();
    let cfg = load(&root_cfg).unwrap();
    let select = cfg.rules[0].extra.get("select").and_then(|v| v.as_str());
    assert_eq!(
        select,
        Some("packages/foo/**"),
        "template select must be re-rooted"
    );
}

/// Load a root config with `nested_configs: true` plus `packages/foo/.alint.yml`
/// holding `nested`, returning the loaded config or the load error text.
fn load_with_nested(nested: &str) -> std::result::Result<Config, String> {
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(&root_cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(pkg_dir.join(".alint.yml"), nested).unwrap();
    load(&root_cfg).map_err(|e| e.to_string())
}

#[test]
fn nested_require_globs_are_rerooted_unless_token_anchored() {
    // A plain glob in a nested `require:` is repository-relative and used to escape
    // the subtree; a `{path}`-anchored one follows the re-rooted iteration entry.
    let cfg = load_with_nested(
        r#"version: 1
rules:
  - id: per-dir
    kind: for_each_dir
    select: "sub"
    require:
      - kind: file_content_forbidden
        paths: "**/*.txt"
        pattern: TODO
      - kind: file_exists
        paths: "{path}/README.md"
    level: error
"#,
    )
    .unwrap();
    let require = format!("{:?}", cfg.rules[0].extra.get("require"));
    assert!(
        require.contains("packages/foo/**/*.txt"),
        "plain glob not re-rooted: {require}"
    );
    assert!(
        require.contains("\"{path}/README.md\""),
        "token glob must stay: {require}"
    );
}

#[test]
fn nested_confinement_rejects_embedded_parent_escapes_and_platform_prefixes() {
    for bad in [
        "./../../Cargo.toml",
        "a/../../other/**",
        r"a\..\..\other\**",
        r"\server\share\x",
        "C:/Windows/win.ini",
        r"C:\Windows\win.ini",
        "{..,a}/Cargo.toml",
    ] {
        let err = load_with_nested(&format!(
            "version: 1\nrules:\n  - id: esc\n    kind: file_exists\n    paths: {bad:?}\n    level: error\n"
        ))
        .unwrap_err();
        assert!(
            err.contains("escape") || err.contains("absolute path"),
            "{bad:?}: unexpected error {err}"
        );
    }
}

#[test]
fn nested_require_does_not_leave_brace_globs_or_leaf_tokens_unprefixed() {
    // Arbitrary `{...}` brace globs, non-subtree tokens (`{basename}`, `{stem}`,
    // `{ext}`, `{parent_name}`), and `{dir}suffix` must be re-rooted under the
    // nested subtree rather than bypassing prefixing, and `{path}/../..` must
    // be rejected outright.
    let cfg = load_with_nested(
        r#"version: 1
rules:
  - id: per-dir
    kind: for_each_dir
    select: "sub"
    require:
      - kind: file_exists
        paths: "{**}/*.rs"
      - kind: file_exists
        paths: "{Cargo.toml}"
      - kind: file_exists
        paths: "{basename}/Cargo.toml"
      - kind: file_exists
        paths: "{dir}sibling/secret.txt"
      - kind: file_exists
        paths: "{dir}/README.md"
    level: error
"#,
    )
    .unwrap();
    let require = format!("{:?}", cfg.rules[0].extra.get("require"));
    assert!(
        require.contains("packages/foo/{**}/*.rs"),
        "brace glob escaped subtree: {require}"
    );
    assert!(
        require.contains("packages/foo/{Cargo.toml}"),
        "brace filename escaped subtree: {require}"
    );
    assert!(
        require.contains("packages/foo/{basename}/Cargo.toml"),
        "leaf token escaped subtree: {require}"
    );
    assert!(
        require.contains("packages/foo/{dir}sibling/secret.txt"),
        "`{{dir}}suffix` escaped subtree: {require}"
    );
    assert!(
        require.contains("\"{dir}/README.md\""),
        "valid `{{dir}}/` token should stay anchored: {require}"
    );

    let err = load_with_nested(
        r#"version: 1
rules:
  - id: per-dir
    kind: for_each_dir
    select: "sub"
    require:
      - kind: file_exists
        paths: "{path}/../../Cargo.toml"
    level: error
"#,
    )
    .unwrap_err();
    assert!(
        err.contains("escape"),
        "parent escape after `{{path}}` must be rejected: {err}"
    );
}

#[test]
fn nested_require_unconfinable_kind_is_rejected() {
    let err = load_with_nested(
        r#"version: 1
rules:
  - id: per-dir
    kind: for_each_dir
    select: "*"
    require:
      - kind: git_commit_message
        pattern: '^feat'
    level: error
"#,
    )
    .unwrap_err();
    assert!(
        err.contains("cannot be confined") && err.contains("require"),
        "{err}"
    );
}

#[test]
fn nested_kind_specific_paths_are_rerooted() {
    let cfg = load_with_nested(
        r#"version: 1
rules:
  - id: gate
    kind: import_gate
    paths: "**/*.py"
    language: python
    forbid: "^forbidden"
    allow: ["legacy/**"]
    level: error
  - id: schema
    kind: json_schema_passes
    paths: "data/*.json"
    schema_path: "schema.json"
    level: error
  - id: links
    kind: markdown_links_resolve
    paths: "**/*.md"
    root: { url_prefix: /docs/, dir: site }
    level: error
  - id: pairs
    kind: pair
    primary: "src/*.c"
    partner: "include/main.h"
    level: error
"#,
    )
    .unwrap();
    let dbg = |i: usize, key: &str| format!("{:?}", cfg.rules[i].extra.get(key));
    assert!(
        dbg(0, "allow").contains("packages/foo/legacy/**"),
        "{}",
        dbg(0, "allow")
    );
    assert!(
        dbg(1, "schema_path").contains("packages/foo/schema.json"),
        "{}",
        dbg(1, "schema_path")
    );
    assert!(
        dbg(2, "root").contains("packages/foo/site"),
        "{}",
        dbg(2, "root")
    );
    assert!(
        dbg(3, "partner").contains("packages/foo/include/main.h"),
        "{}",
        dbg(3, "partner")
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
fn nested_execution_settings_are_rejected_even_at_false_or_null() {
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(&root_cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let nested_cfg = pkg_dir.join(".alint.yml");

    for (name, declaration) in [
        ("respect_gitignore", "respect_gitignore: true"),
        ("fix_size_limit", "fix_size_limit: null"),
        ("nested_configs", "nested_configs: false"),
    ] {
        std::fs::write(
            &nested_cfg,
            format!("version: 1\n{declaration}\nrules: []\n"),
        )
        .unwrap();
        let err = load(&root_cfg).unwrap_err();
        assert!(err.to_string().contains(name), "{name}: {err}");
    }
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
        // file_exists is path-scoped, but omitting `paths` leaves nothing that
        // can be confined to the nested subtree.
        r"version: 1
rules:
  - id: no-file
    kind: file_exists
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

/// Inside a repository, discovery checks the repository's top (the nearest
/// directory holding a `.git` directory or file) and stops: a config above
/// it, in a directory someone else may write, never governs it.
#[test]
fn discover_stops_at_the_repository_top() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join(".alint.yml"), "version: 1\nrules: []\n").unwrap();
    let repo = tmp.path().join("repo");
    let nested = repo.join("a/b");
    std::fs::create_dir_all(&nested).unwrap();
    for git in ["directory", "file"] {
        if git == "directory" {
            std::fs::create_dir(repo.join(".git")).unwrap();
        } else {
            std::fs::remove_dir(repo.join(".git")).unwrap();
            std::fs::write(repo.join(".git"), "gitdir: ../elsewhere\n").unwrap();
        }
        assert_eq!(discover(&nested), None, ".git {git}");
        assert_eq!(discover(&repo), None, ".git {git}");

        // From a subdirectory, the repository's own config is found.
        std::fs::write(repo.join(".alint.yml"), "version: 1\nrules: []\n").unwrap();
        assert_eq!(
            discover(&nested),
            Some(repo.join(".alint.yml")),
            ".git {git}"
        );
        std::fs::remove_file(repo.join(".alint.yml")).unwrap();
    }
    // A directory without `.git` (a vendored package) is no repository top.
    let vendored = repo.join("node_modules/pkg");
    std::fs::create_dir_all(&vendored).unwrap();
    std::fs::write(repo.join(".alint.yml"), "version: 1\nrules: []\n").unwrap();
    assert_eq!(discover(&vendored), Some(repo.join(".alint.yml")));
}

/// Outside every repository, discovery climbs as before but skips a config
/// file another user owns in a directory above the start (here through the
/// ownership seam), listing it; one the current user owns is found, and so is
/// any where no owner is read (Windows). A config in the start directory
/// itself is used whoever owns it.
#[test]
fn discover_outside_a_repository_skips_another_users_config() {
    let tmp = tempfile::tempdir().unwrap();
    if alint_core::git::repository_top(tmp.path()).is_some() {
        eprintln!("skipped: the temporary directory is inside a repository");
        return;
    }
    let shared = tmp.path().join(".alint.yml");
    std::fs::write(&shared, "version: 1\nrules: []\n").unwrap();
    let mine = tmp.path().join("work");
    let nested = mine.join("a/b");
    std::fs::create_dir_all(&nested).unwrap();
    let foreign = |path: &Path| Some(path != shared.as_path() && path != mine.join("alint.yml"));

    let found = discover_config_with(&nested, &foreign);
    assert_eq!(found.skipped, std::slice::from_ref(&shared));
    assert!(
        found
            .path
            .as_deref()
            .is_none_or(|path| !path.starts_with(tmp.path())),
        "{found:?}"
    );

    // The nearer config the user owns wins; the skipped one is a sibling name.
    std::fs::write(mine.join("alint.yml"), "version: 1\nrules: []\n").unwrap();
    std::fs::write(mine.join("alint.yaml"), "version: 1\nrules: []\n").unwrap();
    let found = discover_config_with(&nested, &foreign);
    assert_eq!(found.path, Some(mine.join("alint.yaml")));
    assert_eq!(found.skipped, [mine.join("alint.yml")]);

    // In the start directory, another user's config is used: the run names
    // that directory (root in a container, on a directory the host's user
    // owns). From below, the same file is skipped.
    let found = discover_config_with(&mine, &foreign);
    assert_eq!(found.path, Some(mine.join("alint.yml")));
    assert_eq!(found.skipped, Vec::<PathBuf>::new());
    let found = discover_config_with(tmp.path(), &|_| Some(false));
    assert_eq!(found.path, Some(shared.clone()));

    // No owner read (Windows): nothing is skipped.
    let found = discover_config_with(&nested, &|_| None);
    assert_eq!(found.path, Some(mine.join("alint.yml")));
    assert_eq!(found.skipped, Vec::<PathBuf>::new());

    // The real check: the user's own file is found.
    let found = discover_config(&nested);
    assert_eq!(found.path, Some(mine.join("alint.yml")));
    assert_eq!(found.skipped, Vec::<PathBuf>::new());

    // Inside a repository, no file is skipped: the repository is the user's
    // to vouch for, as before. Here the user owns `.git` and the directories,
    // and another user the config files.
    std::fs::create_dir(mine.join(".git")).unwrap();
    let configs_foreign = |path: &Path| {
        Some(
            path.extension()
                .is_none_or(|ext| ext != "yml" && ext != "yaml"),
        )
    };
    let found = discover_config_with(&nested, &configs_foreign);
    assert_eq!(found.path, Some(mine.join("alint.yml")));
    assert_eq!(found.skipped, Vec::<PathBuf>::new());
}

/// A `.git` another user owns (an empty file planted in a shared directory is
/// enough), or one in a directory another user owns, makes no repository top:
/// discovery goes on climbing and skips another user's config above the start,
/// as outside every repository.
#[test]
fn discover_takes_no_repository_top_from_another_users_git() {
    let tmp = tempfile::tempdir().unwrap();
    if alint_core::git::repository_top(tmp.path()).is_some() {
        eprintln!("skipped: the temporary directory is inside a repository");
        return;
    }
    let shared = tmp.path().join(".alint.yml");
    std::fs::write(&shared, "version: 1\nrules: []\n").unwrap();
    let planted = tmp.path().join(".git");
    std::fs::write(&planted, "").unwrap();
    let nested = tmp.path().join("work/a/b");
    std::fs::create_dir_all(&nested).unwrap();
    let outside = |found: &Discovery| {
        found
            .path
            .as_deref()
            .is_none_or(|path| !path.starts_with(tmp.path()))
    };

    // Another user owns the config and the `.git` beside it.
    let foreign = |path: &Path| Some(path != shared.as_path() && path != planted.as_path());
    let found = discover_config_with(&nested, &foreign);
    assert_eq!(found.skipped, std::slice::from_ref(&shared));
    assert!(outside(&found), "{found:?}");

    // The user's own `.git`, in a directory another user owns.
    let foreign_directory = |path: &Path| Some(path != shared.as_path() && path != tmp.path());
    let found = discover_config_with(&nested, &foreign_directory);
    assert_eq!(found.skipped, std::slice::from_ref(&shared));
    assert!(outside(&found), "{found:?}");

    // As a directory too.
    std::fs::remove_file(&planted).unwrap();
    std::fs::create_dir(&planted).unwrap();
    let found = discover_config_with(&nested, &foreign);
    assert_eq!(found.skipped, std::slice::from_ref(&shared));

    // The positive control: the user's own `.git` in the user's own directory
    // is the top, and its config is used whoever owns that file.
    let found = discover_config_with(&nested, &|path: &Path| Some(path != shared.as_path()));
    assert_eq!(found.path, Some(shared.clone()));
    assert_eq!(found.skipped, Vec::<PathBuf>::new());
    // No owner read (Windows): any `.git` is the top.
    let found = discover_config_with(&nested, &|_| None);
    assert_eq!(found.path, Some(shared.clone()));
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
    use ayaml::Mapping;
    let parse_rule = |y: &str| -> Mapping { alint_core::yaml::from_str(y).unwrap() };

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

#[test]
fn an_inherited_applicability_that_is_not_a_tier_name_is_refused_and_stripped() {
    use ayaml::Mapping;
    let parse_rule = |y: &str| -> Mapping { alint_core::yaml::from_str(y).unwrap() };
    // serde writes a variant as a one-entry mapping or a tag; a gate that
    // compares strings only would let `{safe: ~}` through as a promotion.
    for written in ["{safe: ~}", "{SAFE: null}", "!safe ~", "[safe]", "1"] {
        for rule in [
            format!(
                "id: no-bak\nkind: file_absent\npaths: '**/*.bak'\nlevel: error\n\
                 fix: {{ file_remove: {{ applicability: {written} }} }}"
            ),
            format!(
                "id: parent\nkind: for_each_dir\nselect: '*'\nlevel: error\n\
                 require:\n  - kind: file_absent\n    paths: '**/*.bak'\n    \
                 fix: {{ file_remove: {{ applicability: {written} }} }}"
            ),
        ] {
            let mut rule = parse_rule(&rule);
            let error = crate::reject_fix_promotion_in(std::slice::from_ref(&rule), "./base.yml")
                .expect_err(written)
                .to_string();
            assert!(error.contains("is not a tier name"), "{written}: {error}");
            crate::strip_fix_promotions_in_rule(&mut rule);
            assert!(
                crate::reject_fix_promotion_in(std::slice::from_ref(&rule), "./base.yml").is_ok(),
                "{written}: not stripped"
            );
        }
    }
    // A null is no value: the op's default tier.
    let null = parse_rule(
        "id: no-bak\nkind: file_absent\npaths: '**/*.bak'\nlevel: error\n\
         fix: { file_remove: { applicability: ~ } }",
    );
    assert!(crate::reject_fix_promotion_in(std::slice::from_ref(&null), "./base.yml").is_ok());
}

#[test]
fn a_template_placeholder_anywhere_in_an_applicability_is_refused() {
    let template: ayaml::Mapping = alint_core::yaml::from_str(
        "id: t\nkind: file_absent\npaths: '**/*.bak'\n\
         fix: { file_remove: { applicability: { '{{vars.tier}}': ~ } } }",
    )
    .unwrap();
    assert_eq!(
        crate::find_template_placeholder_in_guarded_field(&template).as_deref(),
        Some("fix.file_remove.applicability")
    );
}

/// Every field of a nested config's rules named the config by its whole path
/// (its field-source marker). It names it by an index among the root's field
/// sources now, the same size at any depth, which resolves to the path.
#[test]
fn a_nested_rule_marker_does_not_grow_with_its_path() {
    let deep = deep_dir();
    let marker_at = |dir: &str| {
        let tmp = tempfile::tempdir().unwrap();
        let root_cfg = tmp.path().join(".alint.yml");
        std::fs::write(&root_cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
        let nested = tmp.path().join(dir);
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            nested.join(".alint.yml"),
            "version: 1\nrules:\n  - {id: r, kind: file_exists, paths: x, level: error, \
             message: m}\n",
        )
        .unwrap();
        let root = RawConfig {
            version: Some(1),
            ..RawConfig::default()
        };
        let opts = LoadOptions::with_catalog(test_catalog());
        let mut names = vec!["an extends: entry".to_string()];
        let env_values = Vec::new();
        let (rules, _, _) = nested::discover_nested(
            tmp.path(),
            &root_cfg.canonicalize().unwrap(),
            &root,
            &opts,
            RunningVersion::current(),
            &mut names,
            &env_values,
        )
        .unwrap();
        let source = format!("{dir}/.alint.yml");
        assert_eq!(names, ["an extends: entry".to_string(), source.clone()]);
        // It still names the config's path once resolved.
        let mut resolved = rules[0].clone();
        crate::trust::name_field_sources(&mut resolved, &names);
        assert_eq!(
            configured_field_source(&resolved, "paths"),
            Some(source.as_str())
        );
        let marker = rules[0][FIELD_SOURCES_MARKER].as_mapping().unwrap();
        template_budget::size_of_mapping(marker)
    };
    assert_eq!(marker_at("a"), marker_at(&deep));
}

#[test]
fn nested_rejects_empty_and_exclude_only_paths_top_level_and_in_require() {
    for bad_paths in [
        "[]",
        "[\"!dummy\"]",
        "\"!dummy\"",
        "{ exclude: [\"dummy\"] }",
        "{ include: [], exclude: [\"dummy\"] }",
    ] {
        let top = format!(
            "version: 1\nrules:\n  - id: bad\n    kind: no_zero_width_chars\n    paths: {bad_paths}\n    level: error\n"
        );
        let err = load_with_nested(&top).unwrap_err();
        assert!(
            err.contains("at least one non-negated pattern"),
            "top-level {bad_paths} must fail closed: {err}"
        );

        let req = format!(
            "version: 1\nrules:\n  - id: bad-req\n    kind: for_each_dir\n    select: \"sub\"\n    level: error\n    require:\n      - kind: no_zero_width_chars\n        paths: {bad_paths}\n"
        );
        let err_req = load_with_nested(&req).unwrap_err();
        assert!(
            err_req.contains("at least one non-negated pattern"),
            "require {bad_paths} must fail closed: {err_req}"
        );
    }
}

#[test]
fn nested_rejects_env_interpolation_and_env_when_oracles() {
    #[derive(Debug)]
    struct SecretEnv;
    impl crate::EnvironmentProvider for SecretEnv {
        fn get(&self, name: &str) -> Option<String> {
            (name == "SECRET").then(|| "super-secret-token".to_string())
        }
    }

    let load_nested_case = |root_yaml: &str, nested_yaml: &str| {
        let tmp = tempfile::tempdir().unwrap();
        let root_cfg = tmp.path().join(".alint.yml");
        std::fs::write(&root_cfg, root_yaml).unwrap();
        let pkg = tmp.path().join("packages/evil");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join(".alint.yml"), nested_yaml).unwrap();
        let mut opts = LoadOptions::with_catalog(test_catalog());
        opts.environment = std::sync::Arc::new(SecretEnv);
        load_with(&root_cfg, &opts).map_err(|e| e.to_string())
    };

    // 1. Direct {{env.SECRET}} interpolation in nested config.
    let err = load_nested_case(
        "version: 1\nnested_configs: true\nrules: []\n",
        "version: 1\nrules:\n  - id: r\n    kind: file_exists\n    paths: \"{{env.SECRET}}\"\n    level: error\n",
    )
    .unwrap_err();
    assert!(
        err.contains("environment interpolation") && !err.contains("super-secret-token"),
        "expected env interpolation refusal without secret leak: {err}"
    );

    // 2. Direct `when: env.SECRET ...`
    let err = load_nested_case(
        "version: 1\nnested_configs: true\nrules: []\n",
        "version: 1\nrules:\n  - id: r\n    kind: file_exists\n    paths: a.txt\n    when: 'env.SECRET != null'\n    level: error\n",
    )
    .unwrap_err();
    assert!(err.contains("env.SECRET"), "unexpected error: {err}");

    // 3. `when_iter: env.SECRET ...`
    let err = load_nested_case(
        "version: 1\nnested_configs: true\nrules: []\n",
        "version: 1\nrules:\n  - id: r\n    kind: for_each_dir\n    select: '*'\n    when_iter: 'env.SECRET != null'\n    require:\n      - kind: file_exists\n        paths: '{path}/a.txt'\n    level: error\n",
    )
    .unwrap_err();
    assert!(err.contains("env.SECRET"), "unexpected error: {err}");

    // 4. `when: vars.root_secret ...` where root interpolated `root_secret` from env.
    let err = load_nested_case(
        "version: 1\nnested_configs: true\nvars:\n  root_secret: '{{env.SECRET}}'\nrules: []\n",
        "version: 1\nrules:\n  - id: r\n    kind: file_exists\n    paths: a.txt\n    when: 'vars.root_secret == \"x\"'\n    level: error\n",
    )
    .unwrap_err();
    assert!(
        err.contains("vars.root_secret") && !err.contains("super-secret-token"),
        "unexpected error: {err}"
    );

    // 5. Template-assembled `when: "{{vars.cond}}"` from root template filled by nested config.
    let err = load_nested_case(
        "version: 1\nnested_configs: true\ntemplates:\n  - id: t\n    kind: file_exists\n    paths: a.txt\n    when: '{{vars.cond}}'\n    level: error\nrules: []\n",
        "version: 1\nrules:\n  - id: r\n    extends_template: t\n    vars:\n      cond: 'env.SECRET != null'\n",
    )
    .unwrap_err();
    assert!(err.contains("env.SECRET"), "unexpected error: {err}");
}

#[test]
fn nested_require_confines_key_parity_reference_and_scope_filter_manifest_source() {
    let cfg = load_with_nested(
        r#"version: 1
rules:
  - id: per-dir-parity
    kind: for_each_dir
    select: "sub"
    level: error
    require:
      - kind: key_parity
        reference: ".env"
        peers: "{path}/empty.env"
      - kind: file_content_forbidden
        paths: "{path}/a.txt"
        pattern: "TODO"
        scope_filter:
          include_manifest_paths:
            source: "Cargo.toml"
            extract: { toml: "$.workspace.members[*]" }
"#,
    )
    .unwrap();
    let require = format!("{:?}", cfg.rules[0].extra.get("require"));
    assert!(
        require.contains("packages/foo/.env"),
        "key_parity.reference must be re-rooted under nested prefix: {require}"
    );
    assert!(
        require.contains("packages/foo/Cargo.toml"),
        "require scope_filter manifest source must be re-rooted under nested prefix: {require}"
    );

    let bad = load_with_nested(
        r#"version: 1
rules:
  - id: escape-manifest
    kind: for_each_dir
    select: "sub"
    level: error
    require:
      - kind: file_content_forbidden
        paths: "{path}/a.txt"
        pattern: "TODO"
        scope_filter:
          include_manifest_paths:
            source: "../Cargo.toml"
            extract: { toml: "$.workspace.members[*]" }
"#,
    )
    .unwrap_err();
    assert!(bad.contains("escape"), "unexpected error: {bad}");
}

#[test]
fn scope_glob_rejects_escapes_even_with_empty_prefix_and_escapes_metachar_dirs() {
    assert!(nested::scope_glob("/etc/passwd", "").is_err());
    assert!(nested::scope_glob("../outside/**", "").is_err());
    assert!(nested::scope_glob(r"C:\Windows\win.ini", "").is_err());

    // Glob metacharacters in subdirectory names (Next.js `app/[id]/.alint.yml`)
    // must be escaped so `[id]` does not match sibling `app/i/`.
    let tmp = tempfile::tempdir().unwrap();
    let root_cfg = tmp.path().join(".alint.yml");
    std::fs::write(&root_cfg, "version: 1\nnested_configs: true\nrules: []\n").unwrap();
    let dir = tmp.path().join("app/[id]");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(".alint.yml"),
        "version: 1\nrules:\n  - id: id-txt\n    kind: file_absent\n    paths: '*.txt'\n    level: error\n",
    )
    .unwrap();
    let cfg = load(&root_cfg).unwrap();
    let scope = alint_core::Scope::from_spec(&cfg.rules[0]).unwrap();
    let idx = alint_core::FileIndex::from_entries(vec![]);
    assert!(scope.matches(Path::new("app/[id]/bad.txt"), &idx));
    assert!(
        !scope.matches(Path::new("app/i/bad.txt"), &idx),
        "bracket metacharacter in nested dir must not match sibling app/i/"
    );
}
