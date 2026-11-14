//! Remote-extends trust and fixer-applicability coverage.

use super::*;

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
fn w2_fix_trust_transition_lists_equal_metadata() {
    // FixOpMeta is operational. Keep the former hand-maintained lists as exact
    // one-release transition checks so the cutover cannot silently change trust.
    use std::collections::BTreeSet;
    // Fix ops that carry no ruleset-authored bytes and do not spawn.
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
    let catalog = test_catalog();
    let derive = |class| {
        alint_core::FixSpec::ALL_OP_NAMES
            .iter()
            .copied()
            .filter(|op| catalog.fix_op(op).is_some_and(|meta| meta.trust == class))
            .collect::<BTreeSet<_>>()
    };
    let content = derive(alint_core::FixTrustClass::ContentInjecting);
    let fixed = derive(alint_core::FixTrustClass::FixedBehaviour);
    let spawning = derive(alint_core::FixTrustClass::Spawning);

    assert_eq!(
        content,
        crate::trust::CONTENT_INJECTING_FIX_OPS
            .iter()
            .copied()
            .collect()
    );
    assert_eq!(fixed, FIXED_BEHAVIOR_FIX_OPS.iter().copied().collect());
    assert_eq!(spawning, crate::SPAWNING_FIX_OPS.iter().copied().collect());
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

#[test]
fn fused_extended_marking_matches_the_separate_passes() {
    use crate::trust::{
        SourceClass, mark_extended_source_in, mark_field_sources_in, mark_provenance_in,
    };
    use ayaml::Mapping;
    let docs = [
        "id: a\nkind: file_exists\n",
        "id: a\nkind: file_exists\n__alint_internal_field_sources: {id: old, kind: old}\n__alint_internal_provenance: {extended: old}\n",
        "id: a\n__alint_internal_provenance: {extended: old}\nkind: x\n",
        "id: a\nkind: x\nlevel: error\n__alint_internal_field_sources: {id: old}\n",
        "id: a\nkind: x\n__alint_internal_field_sources: {kind: old, id: old}\n",
        "__alint_internal_provenance: {remote: r}\n",
        "id: a\n__alint_internal_field_sources: 3\n__alint_internal_provenance: x\n",
        "id: a\n__alint_internal_field_sources: {id: src}\n__alint_internal_provenance: {extended: src}\n",
    ];
    let class_sets: [&[SourceClass]; 2] = [
        &[SourceClass::Extended],
        &[SourceClass::Extended, SourceClass::Remote],
    ];
    // Serialized YAML compares key order too (Mapping equality does not).
    let ordered = |m: &Mapping| ayaml::to_string(m).unwrap();
    for doc in docs {
        for classes in class_sets {
            let base: Mapping = ayaml::compat::from_str(doc).unwrap();
            let mut separate = vec![base.clone()];
            let mut fused = vec![base];
            // Two levels of an `extends:` chain, then the same source again.
            for source in ["src", "other", "other"] {
                mark_field_sources_in(&mut separate, source);
                for &class in classes {
                    mark_provenance_in(&mut separate, class, source);
                }
                mark_extended_source_in(&mut fused, source, classes);
                assert_eq!(
                    ordered(&fused[0]),
                    ordered(&separate[0]),
                    "{doc:?} {source}"
                );
            }
        }
    }
}
