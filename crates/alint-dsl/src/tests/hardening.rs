//! Adversarial parser, composition, compatibility, and trust-gate coverage.

use super::*;

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
        (
            "kind: file_exists\n    paths: README.md\n    \
             __alint_internal_field_sources: {paths: forged}",
            "reserved internal field",
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

    // Raw-composition markers are implementation state, not user-facing DSL
    // fields. Accepting one would let authored YAML spoof or erase provenance.
    std::fs::write(
        &cfg,
        "version: 1\nrules:\n  - id: r\n    kind: file_exists\n    \
         paths: README.md\n    __alint_internal_field_sources: {paths: forged}\n    \
         level: error\n",
    )
    .unwrap();
    let err = load(&cfg).unwrap_err().to_string();
    assert!(err.contains("reserved internal field"), "{err}");

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
fn untrusted_remote_template_error_leaves_out_a_template_id_read_from_the_environment() {
    // The same refusal, with the instance's `extends_template` read from the
    // environment: the error says "a template" instead of quoting the id.
    let raw = "{{env.ALINT_TEST_UNSET_TOKEN | default('s3cret')}}";
    let top = format!(
        "templates:\n  - id: canary-template\n    kind: file_exists\n    \
         paths: README.md\n    level: warning\nrules:\n  - id: r\n    \
         extends_template: \"{{{{env.ALINT_TEST_UNSET_TPL | default('canary-template')}}}}\"\n    \
         vars:\n      token: \"{raw}\"\n"
    );
    let remote = "version: 1\ntemplates:\n  - id: canary-template\n    \
        message: \"leaked {{ vars.token }}\"\nrules: []\n";
    let err = try_load_extending(remote, &top).unwrap_err().to_string();
    assert!(
        err.contains("a template substitutes `{{vars.token}}`"),
        "{err}"
    );
    assert!(!err.contains("canary-template"), "{err}");
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

#[test]
fn extended_rule_loading_without_a_catalog_fails_closed() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("base.yml"),
        "version: 1\nrules:\n  - id: inherited\n    kind: file_exists\n    \
         paths: README.md\n    level: error\n",
    )
    .unwrap();
    let child = tmp.path().join(".alint.yml");
    std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();

    let error = crate::load(&child).unwrap_err().to_string();
    assert!(
        error.contains("not present in the injected kind catalog"),
        "{error}"
    );
    assert_eq!(load(&child).unwrap().rules[0].id, "inherited");
}

#[test]
fn extended_unknown_kind_is_denied_by_the_injected_catalog() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("base.yml"),
        "version: 1\nrules:\n  - id: inherited\n    kind: plugin_kind_missing_from_catalog\n    \
         paths: README.md\n    level: error\n",
    )
    .unwrap();
    let child = tmp.path().join(".alint.yml");
    std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();

    let error = load(&child).unwrap_err().to_string();
    assert!(
        error.contains("plugin_kind_missing_from_catalog")
            && error.contains("not present in the injected kind catalog"),
        "{error}"
    );
}

#[test]
fn extended_unknown_fix_operation_is_denied_by_the_injected_catalog() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("base.yml"),
        "version: 1\nrules:\n  - id: inherited\n    kind: file_exists\n    \
         paths: README.md\n    level: error\n    fix: { plugin_fix_missing_from_catalog: {} }\n",
    )
    .unwrap();
    let child = tmp.path().join(".alint.yml");
    std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();

    let error = load(&child).unwrap_err().to_string();
    assert!(
        error.contains("plugin_fix_missing_from_catalog")
            && error.contains("not present in the injected fix-op catalog"),
        "{error}"
    );
}

#[test]
fn injected_plugin_metadata_allows_an_extended_plugin_kind() {
    let mut registry = alint_rules::builtin_registry();
    let metadata = std::sync::Arc::new(registry.meta("file_exists").unwrap().clone());
    registry.register_owned(
        "plugin/file_exists",
        metadata,
        alint_rules::file_exists::build,
    );

    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("base.yml"),
        "version: 1\nrules:\n  - id: inherited-plugin\n    kind: plugin/file_exists\n    \
         paths: README.md\n    level: error\n",
    )
    .unwrap();
    let child = tmp.path().join(".alint.yml");
    std::fs::write(&child, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();

    let config = crate::load_with(&child, &LoadOptions::with_catalog(registry.catalog())).unwrap();
    assert_eq!(config.rules[0].kind, "plugin/file_exists");
    registry.build(&config.rules[0]).unwrap();
}

#[test]
fn compatibility_records_every_composed_source_class() {
    use alint_core::ConfigSourceClass;

    let tmp = tempfile::tempdir().unwrap();
    let cache = extends::Cache::at(tmp.path().join("cache"));
    let remote_url = seed_remote(
        &cache,
        "version: 1\nrequires:\n  alint: '0.19.0'\nrules: []\n",
    );
    std::fs::write(tmp.path().join("base.yml"), "version: 1\nrules: []\n").unwrap();
    std::fs::create_dir(tmp.path().join(".alint.d")).unwrap();
    std::fs::write(tmp.path().join(".alint.d/10-team.yml"), "rules: []\n").unwrap();
    std::fs::create_dir(tmp.path().join("sub")).unwrap();
    std::fs::write(
        tmp.path().join("sub/.alint.yml"),
        "version: 1\nrules:\n  - id: nested-readme\n    kind: file_exists\n    paths: README.md\n    level: warning\n",
    )
    .unwrap();
    let root = tmp.path().join(".alint.yml");
    std::fs::write(
        &root,
        format!(
            "version: 1\nnested_configs: true\nextends:\n  - ./base.yml\n  - alint://bundled/hygiene/no-tracked-artifacts@v1\n  - \"{remote_url}\"\nrules: []\n"
        ),
    )
    .unwrap();

    let config = load_with(&root, &LoadOptions::with_cache(cache)).unwrap();
    let classes: std::collections::HashSet<ConfigSourceClass> = config
        .compatibility
        .iter()
        .map(|record| record.source_class)
        .collect();
    assert_eq!(
        classes,
        [
            ConfigSourceClass::Root,
            ConfigSourceClass::LocalExtend,
            ConfigSourceClass::Bundled,
            ConfigSourceClass::Remote,
            ConfigSourceClass::DropIn,
            ConfigSourceClass::Nested,
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(
        config.requires.unwrap().alint,
        alint_core::StableVersion::new(0, 19, 0)
    );
}

#[test]
fn complete_extended_source_must_declare_version() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("base.yml"), "rules: []\n").unwrap();
    let root = tmp.path().join(".alint.yml");
    std::fs::write(&root, "version: 1\nextends: [./base.yml]\nrules: []\n").unwrap();
    let error = load(&root).unwrap_err().to_string();
    assert!(
        error.contains("base.yml") && error.contains("missing required config `version`"),
        "{error}"
    );
}

#[test]
fn remote_requirement_diagnostic_redacts_url_secrets() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = extends::Cache::at(tmp.path().join("cache"));
    let url = seed_remote(
        &cache,
        "version: 1\nrequires:\n  alint: '999.0.0'\nrules: []\n",
    );
    let secret_url = url
        .replacen("https://", "https://user:secret@", 1)
        .replacen(".yml#", ".yml?token=secret#", 1);
    let root = tmp.path().join(".alint.yml");
    std::fs::write(
        &root,
        format!("version: 1\nextends: [\"{secret_url}\"]\nrules: []\n"),
    )
    .unwrap();
    let error = load_with(&root, &LoadOptions::with_cache(cache))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("https://example.invalid/remote.yml")
            && error.contains("requires alint 999.0.0 or newer"),
        "{error}"
    );
    assert!(!error.contains("user:secret"), "{error}");
    assert!(!error.contains("token=secret"), "{error}");
}

#[test]
fn every_remote_diagnostic_path_redacts_url_secrets() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = extends::Cache::at(tmp.path().join("cache"));

    for (body, filtered) in [
        ("version: 1\nrequires: null\nrules: []\n", false),
        ("version: 1\nrules: []\n", true),
    ] {
        let url = seed_remote(&cache, body);
        let secret_url = url
            .replacen("https://", "https://user:secret@", 1)
            .replacen(".yml#", ".yml?token=secret#", 1);
        let root = tmp.path().join(if filtered {
            "filtered.yml"
        } else {
            "malformed.yml"
        });
        let extends = if filtered {
            format!("extends:\n  - url: \"{secret_url}\"\n    only: []\n")
        } else {
            format!("extends: [\"{secret_url}\"]\n")
        };
        std::fs::write(&root, format!("version: 1\n{extends}rules: []\n")).unwrap();
        let error = load_with(&root, &LoadOptions::with_cache(cache.clone()))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("https://example.invalid/remote.yml"),
            "{error}"
        );
        assert!(!error.contains("user:secret"), "{error}");
        assert!(!error.contains("token=secret"), "{error}");
    }

    let root = tmp.path().join("missing-integrity.yml");
    std::fs::write(
        &root,
        "version: 1\nextends: [\"https://user:secret@example.invalid/remote.yml?token=secret\"]\nrules: []\n",
    )
    .unwrap();
    let error = load_with(&root, &LoadOptions::with_cache(cache))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("https://example.invalid/remote.yml"),
        "{error}"
    );
    assert!(!error.contains("user:secret"), "{error}");
    assert!(!error.contains("token=secret"), "{error}");
}

#[test]
fn direct_parse_enforces_literal_requirement_and_records_inline_source() {
    let config = parse("version: 1\nrequires:\n  alint: '0.19.0'\nrules: []\n").unwrap();
    assert_eq!(config.compatibility.len(), 1);
    assert_eq!(
        config.compatibility[0].source_class,
        alint_core::ConfigSourceClass::Inline
    );

    for invalid in [
        "0.19",
        "0.19.0-alpha.1",
        ">=0.19.0",
        "00.19.0",
        "0.19.0+build",
    ] {
        let error = parse(&format!(
            "version: 1\nrequires:\n  alint: '{invalid}'\nrules: []\n"
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains("requires"), "{invalid}: {error}");
    }

    for invalid in ["requires: null", "requires: {}", "requires:\n  alint: null"] {
        let error = parse(&format!("version: 1\n{invalid}\nrules: []\n"))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("requires") || error.contains("alint"),
            "{error}"
        );
    }
}

/// A distinct value per name, so a test can tell which were recorded.
#[derive(Debug)]
struct NamedEnvironment;
impl crate::EnvironmentProvider for NamedEnvironment {
    fn get(&self, name: &str) -> Option<String> {
        Some(format!("value-of-{}", name.to_lowercase()))
    }
}

#[test]
fn raw_config_rerun_keeps_the_type_interpolation_gave_each_value() {
    #[derive(Debug)]
    struct TypedEnvironment;
    impl crate::EnvironmentProvider for TypedEnvironment {
        fn get(&self, name: &str) -> Option<String> {
            Some(
                match name {
                    "NUMBER" => "123456789012",
                    "FLAG" => "true",
                    // `version`'s maximum: the rerun moving it up fails.
                    "MAX" => "4294967295",
                    "NEGATIVE" => "-5",
                    _ => return None,
                }
                .to_string(),
            )
        }
    }
    let parse = |document: &str| {
        crate::loader::parse_config_interpolated_with_environment(
            document,
            Path::new(".alint.yml"),
            &TypedEnvironment,
        )
        .map(|_| ())
        .unwrap_err()
        .to_string()
    };
    // A number, a bool or a number at the edge of its range before an
    // unrelated error: the error does not quote them, so it stays readable.
    for value in [
        "fix_size_limit: '{{env.NUMBER}}'",
        "respect_gitignore: '{{env.FLAG}}'",
        "version: '{{env.MAX}}'",
    ] {
        let error = parse(&format!("{value}\nnested_configs: maybe\nrules: []\n"));
        assert!(
            error.contains("maybe") && !error.contains("<redacted>"),
            "{value}: {error}"
        );
    }
    // An error that quotes the value stays redacted.
    let error = parse("fix_size_limit: '{{env.NEGATIVE}}'\nrules: []\n");
    assert!(
        error.contains("<redacted>") && !error.contains("-5"),
        "{error}"
    );
    let error = parse("nested_configs: '{{env.NUMBER}}'\nrules: []\n");
    assert!(
        error.contains("<redacted>") && !error.contains("123456789012"),
        "{error}"
    );
}

#[test]
fn an_extends_filter_read_from_the_environment_leaves_the_url_readable() {
    let raw = crate::loader::parse_config_interpolated_with_environment(
        "version: 1\nextends:\n  - url: ./base.yml\n    only: ['{{env.VALUE}}']\n  \
         - url: './{{env.VALUE}}.yml'\n  - '{{env.VALUE}}'\n  - ./plain.yml\nrules: []\n",
        Path::new(".alint.yml"),
        &NamedEnvironment,
    )
    .unwrap();
    assert_eq!(raw.extends_environment, [false, true, true, false]);
    // The filter's value is still removed from errors.
    assert!(
        raw.environment_values
            .contains(&"value-of-value".to_string()),
        "{:?}",
        raw.environment_values
    );
}
