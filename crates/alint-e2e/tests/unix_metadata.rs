//! Unix-metadata-dependent rule scenarios.
//!
//! The tree-spec YAML scenario DSL deliberately does not model the
//! +x bit or symlinks (see `TREE_SPEC.md` §Limitations). The rules
//! added in Phase 6 — `no_symlinks`, `executable_bit`,
//! `executable_has_shebang`, `shebang_has_executable` — need those
//! primitives for their negative-path tests. This file covers that
//! gap with direct-to-disk setup, then drives the normal engine.
//!
//! Everything here is `#[cfg(unix)]`: on Windows the rules are
//! no-ops by design, and Windows lacks a portable +x or symlink
//! story in `std`.

#![cfg(unix)]

use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

use alint_core::{Engine, Level, Report, RuleEntry, WalkOptions, walk};
use alint_testkit::treespec::{TreeSpec, materialize};

fn chmod(path: &Path, mode: u32) {
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(mode);
    std::fs::set_permissions(path, perms).unwrap();
}

fn load_config(root: &Path, config_yaml: &str) -> alint_core::Config {
    let config_path = root.join(".alint.yml");
    std::fs::write(&config_path, config_yaml).unwrap();
    let cache = alint_dsl::extends::Cache::at(root.join(".alint-cache"));
    let opts = alint_dsl::LoadOptions::with_cache(cache);
    alint_dsl::load_with(&config_path, &opts).unwrap()
}

fn build_engine(
    config: &alint_core::Config,
    registry: alint_core::RuleRegistry,
) -> (Engine, WalkOptions) {
    let mut entries: Vec<RuleEntry> = Vec::with_capacity(config.rules.len());
    for spec in &config.rules {
        if matches!(spec.level, Level::Off) {
            continue;
        }
        let rule = registry.build(spec).unwrap();
        entries.push(RuleEntry::new(rule));
    }
    let engine = Engine::from_entries(entries, registry)
        .with_facts(config.facts.clone())
        .with_vars(config.vars.clone())
        .with_fix_size_limit(config.fix_size_limit);
    let walk_opts = WalkOptions {
        respect_gitignore: config.respect_gitignore,
        extra_ignores: config.ignore.clone(),
    };
    (engine, walk_opts)
}

fn run_check(root: &Path, config_yaml: &str) -> Report {
    let config = load_config(root, config_yaml);
    let (engine, walk_opts) = build_engine(&config, alint_rules::builtin_registry());
    let index = walk(root, &walk_opts).unwrap();
    engine.run(root, &index).unwrap()
}

fn run_fix(root: &Path, config_yaml: &str) -> alint_core::FixReport {
    let config = load_config(root, config_yaml);
    let (engine, walk_opts) = build_engine(&config, alint_rules::builtin_registry());
    let index = walk(root, &walk_opts).unwrap();
    engine.fix(root, &index, false).unwrap()
}

fn tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("alint-unix-metadata-")
        .tempdir()
        .unwrap()
}

fn tree_from(yaml: &str) -> TreeSpec {
    serde_yaml_ng::from_str(yaml).unwrap()
}

fn violation_count(report: &Report, rule_id: &str) -> usize {
    report
        .results
        .iter()
        .filter(|r| &*r.rule_id == rule_id)
        .map(|r| r.violations.len())
        .sum()
}

// ------------------------------------------------------------------
// no_symlinks
// ------------------------------------------------------------------

#[test]
fn no_symlinks_flags_symlinked_file() {
    let tmp = tempdir();
    let root = tmp.path();
    materialize(&tree_from("target.txt: \"real file\"\n"), root).unwrap();
    symlink("target.txt", root.join("alias.txt")).unwrap();

    let report = run_check(
        root,
        "version: 1\nrules:\n  - id: no-links\n    kind: no_symlinks\n    paths: \"**\"\n    level: error\n",
    );
    assert_eq!(violation_count(&report, "no-links"), 1);
}

#[test]
fn no_symlinks_fix_removes_the_link_but_keeps_the_target() {
    let tmp = tempdir();
    let root = tmp.path();
    materialize(&tree_from("target.txt: \"real file\"\n"), root).unwrap();
    symlink("target.txt", root.join("alias.txt")).unwrap();

    let report = run_fix(
        root,
        "version: 1\nrules:\n  - id: no-links\n    kind: no_symlinks\n    paths: \"**\"\n    level: error\n    fix:\n      file_remove: {}\n",
    );
    assert_eq!(report.applied(), 1);
    assert!(!root.join("alias.txt").exists());
    assert!(root.join("target.txt").exists());
}

// ------------------------------------------------------------------
// executable_bit
// ------------------------------------------------------------------

const SHEBANG_TREE: &str = "scripts:\n  hello.sh: \"#!/bin/sh\\necho hi\\n\"\n";

#[test]
fn executable_bit_require_true_fails_when_x_missing() {
    let tmp = tempdir();
    let root = tmp.path();
    materialize(&tree_from(SHEBANG_TREE), root).unwrap();
    // No chmod — file lacks +x.

    let report = run_check(
        root,
        "version: 1\nrules:\n  - id: scripts-are-exec\n    kind: executable_bit\n    paths: \"scripts/**\"\n    level: error\n    require: true\n",
    );
    assert_eq!(violation_count(&report, "scripts-are-exec"), 1);
}

#[test]
fn executable_bit_require_true_passes_when_x_is_set() {
    let tmp = tempdir();
    let root = tmp.path();
    materialize(&tree_from(SHEBANG_TREE), root).unwrap();
    chmod(&root.join("scripts/hello.sh"), 0o755);

    let report = run_check(
        root,
        "version: 1\nrules:\n  - id: scripts-are-exec\n    kind: executable_bit\n    paths: \"scripts/**\"\n    level: error\n    require: true\n",
    );
    assert_eq!(violation_count(&report, "scripts-are-exec"), 0);
}

#[test]
fn executable_bit_require_false_fails_when_x_is_set() {
    let tmp = tempdir();
    let root = tmp.path();
    materialize(&tree_from("docs:\n  intro.md: \"hello\\n\"\n"), root).unwrap();
    chmod(&root.join("docs/intro.md"), 0o755);

    let report = run_check(
        root,
        "version: 1\nrules:\n  - id: docs-not-exec\n    kind: executable_bit\n    paths: \"docs/**\"\n    level: error\n    require: false\n",
    );
    assert_eq!(violation_count(&report, "docs-not-exec"), 1);
}

// ------------------------------------------------------------------
// executable_has_shebang
// ------------------------------------------------------------------

#[test]
fn executable_has_shebang_fails_when_exec_has_no_shebang() {
    let tmp = tempdir();
    let root = tmp.path();
    materialize(
        &tree_from("bin:\n  tool: \"not a shebang\\n\"\n  script.sh: \"#!/bin/sh\\necho ok\\n\"\n"),
        root,
    )
    .unwrap();
    chmod(&root.join("bin/tool"), 0o755);
    chmod(&root.join("bin/script.sh"), 0o755);

    let report = run_check(
        root,
        "version: 1\nrules:\n  - id: exec-has-shebang\n    kind: executable_has_shebang\n    paths: \"bin/**\"\n    level: error\n",
    );
    assert_eq!(violation_count(&report, "exec-has-shebang"), 1);
}

#[test]
fn executable_has_shebang_ignores_non_executables() {
    let tmp = tempdir();
    let root = tmp.path();
    materialize(&tree_from("bin:\n  tool: \"not a shebang\\n\"\n"), root).unwrap();
    // Intentionally NOT chmod-ing +x.

    let report = run_check(
        root,
        "version: 1\nrules:\n  - id: exec-has-shebang\n    kind: executable_has_shebang\n    paths: \"bin/**\"\n    level: error\n",
    );
    assert_eq!(violation_count(&report, "exec-has-shebang"), 0);
}

// ------------------------------------------------------------------
// shebang_has_executable
// ------------------------------------------------------------------

#[test]
fn shebang_has_executable_passes_when_shebang_file_is_exec() {
    let tmp = tempdir();
    let root = tmp.path();
    materialize(
        &tree_from("scripts:\n  ok.sh: \"#!/bin/sh\\necho hi\\n\"\n"),
        root,
    )
    .unwrap();
    chmod(&root.join("scripts/ok.sh"), 0o755);

    let report = run_check(
        root,
        "version: 1\nrules:\n  - id: shebang-needs-x\n    kind: shebang_has_executable\n    paths: \"scripts/**\"\n    level: error\n",
    );
    assert_eq!(violation_count(&report, "shebang-needs-x"), 0);
}
