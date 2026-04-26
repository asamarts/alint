//! End-to-end tests for the `command` plugin rule kind.
//!
//! Per-file unit coverage lives in `alint-rules::command::tests`;
//! the DSL-level trust gate is covered by
//! `alint-dsl::tests::load_rejects_command_rule_declared_in_local_extends`.
//! What's left to verify is that the rule wires correctly through
//! the full Engine + walker — including the `--changed` path
//! where filtered-index iteration must skip unchanged files.
//!
//! Gated `#[cfg(unix)]` because the tests rely on `/bin/sh` and
//! exit codes. The rule itself is portable; the test harness is
//! the only thing that needs a POSIX shell.

#![cfg(unix)]

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use alint_core::{Engine, Level, RuleEntry, WalkOptions, walk};

fn tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("alint-command-plugin-")
        .tempdir()
        .unwrap()
}

fn build_engine(config: &alint_core::Config) -> Engine {
    let registry = alint_rules::builtin_registry();
    let mut entries: Vec<RuleEntry> = Vec::with_capacity(config.rules.len());
    for spec in &config.rules {
        if matches!(spec.level, Level::Off) {
            continue;
        }
        let rule = registry.build(spec).unwrap();
        entries.push(RuleEntry::new(rule));
    }
    Engine::from_entries(entries, registry)
        .with_facts(config.facts.clone())
        .with_vars(config.vars.clone())
        .with_fix_size_limit(config.fix_size_limit)
}

fn write_config(root: &Path, body: &str) -> alint_core::Config {
    std::fs::write(root.join(".alint.yml"), body).unwrap();
    let cache = alint_dsl::extends::Cache::at(root.join(".alint-cache"));
    let opts = alint_dsl::LoadOptions::with_cache(cache);
    alint_dsl::load_with(&root.join(".alint.yml"), &opts).unwrap()
}

#[test]
fn command_rule_passes_via_full_engine_when_program_exits_zero() {
    let tmp = tempdir();
    let root = tmp.path();
    std::fs::write(root.join("a.txt"), b"x").unwrap();
    std::fs::write(root.join("b.txt"), b"y").unwrap();

    let cfg = write_config(
        root,
        r#"version: 1
rules:
  - id: noop
    kind: command
    paths: "*.txt"
    command: ["/bin/sh", "-c", "exit 0"]
    level: error
"#,
    );
    let engine = build_engine(&cfg);
    let index = walk(root, &WalkOptions::default()).unwrap();
    let report = engine.run(root, &index).unwrap();
    let violations: usize = report.results.iter().map(|r| r.violations.len()).sum();
    assert_eq!(violations, 0, "unexpected: {report:?}");
}

#[test]
fn command_rule_fires_one_violation_per_failing_file() {
    let tmp = tempdir();
    let root = tmp.path();
    std::fs::write(root.join("a.txt"), b"x").unwrap();
    std::fs::write(root.join("b.txt"), b"y").unwrap();
    std::fs::write(root.join("skip.md"), b"z").unwrap();

    // Fail on every .txt; .md is out of scope.
    let cfg = write_config(
        root,
        r#"version: 1
rules:
  - id: always-fail
    kind: command
    paths: "*.txt"
    command: ["/bin/sh", "-c", "echo nope >&2; exit 1"]
    level: error
"#,
    );
    let engine = build_engine(&cfg);
    let index = walk(root, &WalkOptions::default()).unwrap();
    let report = engine.run(root, &index).unwrap();

    let r = report
        .results
        .iter()
        .find(|r| r.rule_id == "always-fail")
        .expect("rule absent from report");
    assert_eq!(r.violations.len(), 2, "violations: {:?}", r.violations);
    let paths: HashSet<_> = r.violations.iter().filter_map(|v| v.path.clone()).collect();
    assert!(paths.contains(&PathBuf::from("a.txt")));
    assert!(paths.contains(&PathBuf::from("b.txt")));
    for v in &r.violations {
        assert!(v.message.contains("nope"), "message: {}", v.message);
    }
}

#[test]
fn command_rule_in_changed_mode_only_invokes_for_changed_files() {
    // The whole point of --changed: per-file rules iterate the
    // filtered index, so a `command` rule whose work is per-file
    // shouldn't spawn at all for unchanged files. We prove this
    // by writing a marker file from the child and counting how
    // many markers exist after a --changed run.
    let tmp = tempdir();
    let root = tmp.path();
    std::fs::write(root.join("a.txt"), b"x").unwrap();
    std::fs::write(root.join("b.txt"), b"y").unwrap();
    std::fs::write(root.join("c.txt"), b"z").unwrap();

    let markers = root.join("markers");
    std::fs::create_dir(&markers).unwrap();

    // Each child writes a marker named after $ALINT_PATH so we
    // can count invocations. Always exits 0 — we're measuring
    // side effects, not violations.
    let cfg = write_config(
        root,
        r#"version: 1
rules:
  - id: count-invocations
    kind: command
    paths: "*.txt"
    command: ["/bin/sh", "-c", "touch \"markers/$ALINT_PATH\""]
    level: info
"#,
    );

    // Pretend only b.txt is "changed."
    let mut changed = HashSet::new();
    changed.insert(PathBuf::from("b.txt"));
    let engine = build_engine(&cfg).with_changed_paths(changed);

    let index = walk(root, &WalkOptions::default()).unwrap();
    engine.run(root, &index).unwrap();

    let invoked: Vec<String> = std::fs::read_dir(&markers)
        .unwrap()
        .filter_map(std::result::Result::ok)
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    assert_eq!(
        invoked,
        vec!["b.txt".to_string()],
        "expected only b.txt to invoke; got {invoked:?}"
    );
}
