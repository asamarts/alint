//! `git_commit_message`'s `since:` against a real repository: a ref git cannot
//! resolve is reported, and one read from the environment through the
//! deprecated `${VAR}` syntax is reported as written, never by its value.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use alint_core::{Engine, RuleEntry, WalkOptions, walk};
use alint_rules::builtin_registry;

fn run_git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git invocation");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The analysis failure a `since: <since>` rule reports in a repository
/// without that ref.
fn bad_range_failure(root: &Path, since: &str) -> String {
    let spec: Arc<alint_core::RuleSpec> = Arc::new(
        ayaml::compat::from_str(&format!(
            "id: message\nkind: git_commit_message\nrequires_body: true\nsince: '{since}'\n\
             level: error\n"
        ))
        .unwrap(),
    );
    let registry = builtin_registry();
    let rule = registry.build(&spec).unwrap();
    let engine = Engine::from_entries(vec![RuleEntry::new(rule).with_spec(spec)], registry);
    let index = walk(root, &WalkOptions::default()).unwrap();
    let report = engine.run(root, &index).unwrap();
    let failures = &report.results[0].analysis_failures;
    assert_eq!(failures.len(), 1, "{report:?}");
    failures[0].message.to_string()
}

#[test]
fn a_bad_range_from_a_legacy_reference_is_shown_as_written() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    run_git(root, &["init", "-q"]);
    run_git(
        root,
        &[
            "-c",
            "user.name=alint test",
            "-c",
            "user.email=test@alint.test",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "one",
        ],
    );

    // Cargo sets `CARGO_PKG_NAME` for the test process.
    let legacy = bad_range_failure(root, "${CARGO_PKG_NAME}");
    assert!(legacy.contains("`${CARGO_PKG_NAME}..HEAD`"), "{legacy}");
    assert!(!legacy.contains(env!("CARGO_PKG_NAME")), "{legacy}");
    // Git can quote only part of the ref, so its message is left out.
    assert!(
        legacy.contains("`${CARGO_PKG_NAME}..HEAD`: git's message is left out"),
        "{legacy}"
    );

    let literal = bad_range_failure(root, "no-such-base");
    assert!(literal.contains("`no-such-base..HEAD`"), "{literal}");
    assert!(
        literal.contains("no-such-base..HEAD'"),
        "git's message stays: {literal}"
    );
    assert!(!literal.contains("left out"), "{literal}");
}
