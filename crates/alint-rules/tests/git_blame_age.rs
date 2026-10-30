//! Integration tests for the `git_blame_age` rule kind.
//!
//! Unit tests in `git_blame_age.rs` exercise the build /
//! options / no-cache paths without shelling out to git. These
//! tests stand up a real git repo with two commits at
//! controlled `GIT_AUTHOR_DATE` / `GIT_COMMITTER_DATE`
//! timestamps to verify the rule fires only on the older of
//! two matching lines.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use alint_core::{Engine, Level, RuleEntry, WalkOptions, walk};
use alint_rules::builtin_registry;

/// Initialise a git repo at `root` with a stable identity that
/// `git commit` can use without consulting the user's global
/// config.
fn git_init(root: &Path) {
    run_git(root, &["init", "-q", "-b", "main"], &[]);
    run_git(root, &["config", "user.name", "alint test"], &[]);
    run_git(root, &["config", "user.email", "test@alint.test"], &[]);
}

/// `git add <path>` then `git commit -m <msg>` with author and
/// committer dates pinned via env vars. `date` is anything git
/// accepts, e.g. `"2020-01-01T00:00:00Z"` or `"@1577836800 +0000"`.
fn commit_with_date(root: &Path, path: &str, msg: &str, date: &str) {
    run_git(root, &["add", path], &[]);
    run_git(
        root,
        &["commit", "-q", "-m", msg],
        &[("GIT_AUTHOR_DATE", date), ("GIT_COMMITTER_DATE", date)],
    );
}

fn run_git(root: &Path, args: &[&str], envs: &[(&str, &str)]) {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(root).args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("git invocation");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Build a one-rule engine running `git_blame_age` against the
/// given pattern + threshold.
fn build_engine(pattern: &str, max_age_days: u64) -> Engine {
    let yaml = format!(
        "id: stale\n\
         kind: git_blame_age\n\
         paths: \"**/*.rs\"\n\
         pattern: '{pattern}'\n\
         max_age_days: {max_age_days}\n\
         level: warning\n",
    );
    let spec: Arc<alint_core::RuleSpec> = Arc::new(ayaml::from_str(&yaml).unwrap());
    let registry = builtin_registry();
    let rule = registry.build(&spec).unwrap();
    Engine::from_entries(vec![RuleEntry::new(rule).with_spec(spec)], registry)
}

#[test]
fn fires_on_old_line_silent_on_recent_line() {
    // Skip silently if `git` isn't on PATH — the rest of the
    // test suite handles non-git environments uniformly.
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("git unavailable; skipping git_blame_age integration test");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);

    // First commit: ten years ago. Includes a TODO marker
    // that the rule should fire on.
    std::fs::write(root.join("a.rs"), "// TODO old marker\nfn main() {}\n").unwrap();
    commit_with_date(
        root,
        "a.rs",
        "ancient",
        "@1500000000 +0000", // 2017-07-14
    );

    // Second commit has a fixed reference date. The engine's default
    // analysis time is this HEAD committer timestamp.
    let mut bytes = std::fs::read(root.join("a.rs")).unwrap();
    bytes.extend_from_slice(b"// TODO recent marker\n");
    std::fs::write(root.join("a.rs"), bytes).unwrap();
    commit_with_date(root, "a.rs", "fresh", "2026-10-16T12:00:00Z");

    let engine = build_engine(r"\bTODO\b", 180);
    let walk_opts = WalkOptions {
        respect_gitignore: true,
        extra_ignores: Vec::new(),
    };
    let index = walk(root, &walk_opts).unwrap();
    let report = engine.run(root, &index).unwrap();

    let violations: Vec<_> = report
        .results
        .iter()
        .flat_map(|r| r.violations.iter())
        .collect();
    assert_eq!(
        violations.len(),
        1,
        "expected exactly one violation (the old TODO); got: {violations:?}"
    );
    let v = violations[0];
    assert_eq!(v.line, Some(1), "old TODO is on line 1");
    assert!(
        v.message.contains("TODO"),
        "message should reference the matched marker: {}",
        v.message
    );
    assert!(
        v.message.ends_with("(as of 2026-10-16)"),
        "finding should expose the deterministic reference date: {}",
        v.message
    );
}

#[test]
fn strict_boundary_young_future_and_uncommitted_lines_do_not_fire() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);

    std::fs::write(root.join("a.rs"), "// TODO boundary\n").unwrap();
    commit_with_date(root, "a.rs", "boundary", "2026-10-06T00:00:00Z");
    std::fs::write(root.join("a.rs"), "// TODO boundary\n// TODO old\n").unwrap();
    commit_with_date(root, "a.rs", "old", "2026-10-05T23:59:59Z");
    std::fs::write(
        root.join("a.rs"),
        "// TODO boundary\n// TODO old\n// TODO future\n",
    )
    .unwrap();
    commit_with_date(root, "a.rs", "future", "2026-10-17T00:00:00Z");
    std::fs::write(
        root.join("a.rs"),
        "// TODO boundary\n// TODO old\n// TODO future\n// TODO uncommitted\n",
    )
    .unwrap();

    let engine = build_engine(r"\bTODO\b", 10).with_analysis_time("2026-10-16".parse().unwrap());
    let index = walk(
        root,
        &WalkOptions {
            respect_gitignore: true,
            extra_ignores: Vec::new(),
        },
    )
    .unwrap();
    let report = engine.run(root, &index).unwrap();
    let violations: Vec<_> = report
        .results
        .iter()
        .flat_map(|result| &result.violations)
        .collect();
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(violations[0].line, Some(2));
    assert!(violations[0].message.ends_with("(as of 2026-10-16)"));
}

#[test]
fn ctx_match_placeholder_substitutes_capture_group() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);

    std::fs::write(
        root.join("a.rs"),
        "// FIXME this is broken\n// TODO untouched\n",
    )
    .unwrap();
    commit_with_date(root, "a.rs", "ancient", "@1500000000 +0000");

    // Custom message uses `{{ctx.match}}` to surface the
    // captured marker text. Rule pattern has a capture group
    // around the marker word.
    let yaml = "\
id: stale-markers
kind: git_blame_age
paths: \"**/*.rs\"
pattern: '\\b(FIXME|TODO)\\b'
max_age_days: 180
level: warning
message: \"`{{ctx.match}}` marker is ancient — resolve or remove\"
";
    let spec: alint_core::RuleSpec = ayaml::from_str(yaml).unwrap();
    let registry = builtin_registry();
    let rule = registry.build(&spec).unwrap();
    let engine = Engine::from_entries(vec![RuleEntry::new(rule)], registry)
        .with_analysis_time("2026-10-16".parse().unwrap());

    let walk_opts = WalkOptions {
        respect_gitignore: true,
        extra_ignores: Vec::new(),
    };
    let index = walk(root, &walk_opts).unwrap();
    let report = engine.run(root, &index).unwrap();

    let messages: Vec<&str> = report
        .results
        .iter()
        .flat_map(|r| r.violations.iter())
        .map(|v| v.message.as_ref())
        .collect();
    assert_eq!(messages.len(), 2, "two markers should fire: {messages:?}");
    let combined = messages.join("\n");
    assert!(
        combined.contains("`FIXME`"),
        "FIXME capture should render: {combined}"
    );
    assert!(
        combined.contains("`TODO`"),
        "TODO capture should render: {combined}"
    );
    assert!(
        messages
            .iter()
            .all(|message| message.ends_with("(as of 2026-10-16)")),
        "custom messages must retain the reference date: {messages:?}"
    );
}

#[test]
fn nested_parent_message_preserves_analysis_date() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);
    std::fs::create_dir_all(root.join("pkg/src")).unwrap();
    std::fs::write(root.join("pkg/src/a.rs"), "// TODO old marker\n").unwrap();
    commit_with_date(root, "pkg/src/a.rs", "ancient", "2000-01-01T00:00:00Z");

    let spec: alint_core::RuleSpec = ayaml::from_str(
        "id: per-package\n\
         kind: for_each_dir\n\
         select: pkg\n\
         message: review package debt\n\
         level: warning\n\
         require:\n\
         \x20 - kind: git_blame_age\n\
         \x20   paths: '{path}/**/*.rs'\n\
         \x20   pattern: '\\bTODO\\b'\n\
         \x20   max_age_days: 30\n",
    )
    .unwrap();
    let registry = builtin_registry();
    let rule = registry.build(&spec).unwrap();
    let engine = Engine::from_entries(vec![RuleEntry::new(rule)], registry)
        .with_analysis_time("2026-10-16".parse().unwrap());
    let index = walk(
        root,
        &WalkOptions {
            respect_gitignore: true,
            extra_ignores: Vec::new(),
        },
    )
    .unwrap();

    let report = engine.run(root, &index).unwrap();
    let violations = &report.results[0].violations;
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(
        violations[0].message,
        "review package debt (as of 2026-10-16)"
    );
}

#[test]
fn reports_resource_read_failure_outside_git() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // No git_init — bare tempdir.
    std::fs::write(root.join("a.rs"), "// TODO old marker\n").unwrap();

    let engine = build_engine(r"\bTODO\b", 180);
    let walk_opts = WalkOptions {
        respect_gitignore: true,
        extra_ignores: Vec::new(),
    };
    let index = walk(root, &walk_opts).unwrap();
    let report = engine.run(root, &index).unwrap();

    assert_eq!(report.results.len(), 1);
    let result = &report.results[0];
    assert!(result.violations.is_empty());
    assert_eq!(result.analysis_failures.len(), 1);
    let failure = &result.analysis_failures[0];
    assert_eq!(failure.kind.as_str(), "resource_read");
    assert_eq!(
        failure.message,
        "Git is unavailable or the lint root is not a usable Git repository"
    );
}

#[test]
fn level_promotion_to_error_works() {
    // `level: error` is allowed even though the rule is
    // heuristic — users escalating per-rule once their
    // workflow proves the FP rate is acceptable should be
    // free to do so.
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);
    std::fs::write(root.join("a.rs"), "// TODO old\n").unwrap();
    commit_with_date(root, "a.rs", "ancient", "@1500000000 +0000");

    let yaml = "\
id: stale-todos
kind: git_blame_age
paths: \"**/*.rs\"
pattern: 'TODO'
max_age_days: 180
level: error
";
    let spec: alint_core::RuleSpec = ayaml::from_str(yaml).unwrap();
    let registry = builtin_registry();
    let rule = registry.build(&spec).unwrap();
    let engine = Engine::from_entries(vec![RuleEntry::new(rule)], registry)
        .with_analysis_time("2026-10-16".parse().unwrap());

    let walk_opts = WalkOptions {
        respect_gitignore: true,
        extra_ignores: Vec::new(),
    };
    let index = walk(root, &walk_opts).unwrap();
    let report = engine.run(root, &index).unwrap();
    let levels: Vec<Level> = report
        .results
        .iter()
        .filter(|r| !r.violations.is_empty())
        .map(|r| r.level)
        .collect();
    assert_eq!(levels, vec![Level::Error]);
}
