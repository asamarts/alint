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
    let parse_rule = |y: &str| -> Mapping { ayaml::from_str(y).unwrap() };

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
