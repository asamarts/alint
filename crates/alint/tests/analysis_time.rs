//! End-to-end CLI contract for deterministic analysis time.

use std::path::{Path, PathBuf};
use std::process::Output;

fn alint_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        "version: 1\nrules:\n  - id: stale\n    kind: git_blame_age\n    paths: '**/*.rs'\n    pattern: 'TODO'\n    max_age_days: 30\n    level: warning\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("a.rs"), "// TODO\n").unwrap();
    dir
}

fn run(root: &Path, args: &[&str], env_as_of: Option<&str>) -> Output {
    let mut command = crate::hermetic::command(alint_bin());
    command
        .args(args)
        .current_dir(root)
        .env_remove("ALINT_AS_OF")
        .env("NO_COLOR", "1");
    if let Some(value) = env_as_of {
        command.env("ALINT_AS_OF", value);
    }
    command.output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
fn git(root: &Path, args: &[&str], date: Option<&str>) {
    let mut command = crate::hermetic::git();
    command.arg("-C").arg(root).args(args);
    if let Some(date) = date {
        command
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn flag_overrides_environment_and_explain_reports_the_source() {
    let fixture = fixture();
    let output = run(
        fixture.path(),
        &["--as-of", "2025-03-04", "explain", "stale"],
        Some("2024-02-03"),
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("analysis_time: 2025-03-04 (explicit)"),
        "{}",
        stdout(&output)
    );

    let env_only = run(fixture.path(), &["explain", "stale"], Some("2024-02-03"));
    assert!(env_only.status.success(), "{}", stderr(&env_only));
    assert!(
        stdout(&env_only).contains("analysis_time: 2024-02-03 (explicit)"),
        "{}",
        stdout(&env_only)
    );
}
#[test]
fn explain_uses_head_committer_date_when_no_explicit_input_exists() {
    let fixture = fixture();
    git(fixture.path(), &["init", "-q"], None);
    git(
        fixture.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "add",
            ".",
        ],
        None,
    );
    git(
        fixture.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "fixed",
        ],
        Some("2024-06-07T23:59:58Z"),
    );
    let output = run(
        fixture.path(),
        &["--format", "json", "explain", "stale"],
        None,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["analysis_time"]["date"], "2024-06-07");
    assert_eq!(value["analysis_time"]["source"], "head");
}

#[test]
fn check_ignores_foreign_git_environment_for_time_and_blame() {
    let target = fixture();
    git(target.path(), &["init", "-q"], None);
    git(
        target.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "add",
            ".",
        ],
        None,
    );
    git(
        target.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "old todo",
        ],
        Some("2020-01-01T00:00:00Z"),
    );
    std::fs::write(target.path().join("marker"), "head\n").unwrap();
    git(target.path(), &["add", "marker"], None);
    git(
        target.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "new head",
        ],
        Some("2024-01-02T00:00:00Z"),
    );

    let foreign = fixture();
    git(foreign.path(), &["init", "-q"], None);
    git(
        foreign.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "add",
            ".",
        ],
        None,
    );
    git(
        foreign.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "foreign",
        ],
        Some("2030-01-01T00:00:00Z"),
    );

    let output = crate::hermetic::command(alint_bin())
        .args(["--fail-on-warning", "--format", "json", "check"])
        .current_dir(target.path())
        .env_remove("ALINT_AS_OF")
        .env("NO_COLOR", "1")
        .env("GIT_DIR", foreign.path().join(".git"))
        .env("GIT_WORK_TREE", foreign.path())
        .env("GIT_INDEX_FILE", foreign.path().join(".git/index"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["summary"]["total_violations"], 1);
    assert!(
        value["results"][0]["violations"][0]["message"]
            .as_str()
            .unwrap()
            .ends_with("(as of 2024-01-02)"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn explain_json_exposes_analysis_date_and_source() {
    let fixture = fixture();
    let output = run(
        fixture.path(),
        &[
            "--as-of",
            "2025-03-04",
            "--format",
            "json",
            "explain",
            "stale",
        ],
        None,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["analysis_time"]["date"], "2025-03-04");
    assert_eq!(value["analysis_time"]["source"], "explicit");
    git(fixture.path(), &["init", "-q"], None);
    git(fixture.path(), &["add", "."], None);
    git(
        fixture.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "fixture",
        ],
        Some("2024-01-01T00:00:00Z"),
    );

    let trace = run(
        fixture.path(),
        &[
            "--as-of",
            "2025-03-04",
            "--format",
            "json",
            "explain",
            "stale",
            "--trace",
        ],
        None,
    );
    assert!(trace.status.success(), "{}", stderr(&trace));
    let trace: serde_json::Value = serde_json::from_slice(&trace.stdout).unwrap();
    assert_eq!(trace["rule"]["analysis_time"]["date"], "2025-03-04");
    assert_eq!(trace["rule"]["analysis_time"]["source"], "explicit");
    assert!(
        trace["stages"].as_array().unwrap().iter().any(|stage| {
            stage["name"] == "analysis_time" && stage["reason"] == "analysis_time_available"
        }),
        "{trace:#}"
    );
}

#[test]
fn explain_reports_time_for_a_nested_time_dependent_rule() {
    let fixture = fixture();
    std::fs::create_dir(fixture.path().join("pkg")).unwrap();
    std::fs::write(
        fixture.path().join(".alint.yml"),
        "version: 1\n\
         rules:\n\
         \x20 - id: per-package\n\
         \x20   kind: for_each_dir\n\
         \x20   select: pkg\n\
         \x20   level: warning\n\
         \x20   require:\n\
         \x20     - kind: git_blame_age\n\
         \x20       paths: '{path}/**/*.rs'\n\
         \x20       pattern: 'TODO'\n\
         \x20       max_age_days: 30\n",
    )
    .unwrap();

    let human = run(
        fixture.path(),
        &["--as-of", "2025-03-04", "explain", "per-package"],
        None,
    );
    assert!(human.status.success(), "{}", stderr(&human));
    assert!(
        stdout(&human).contains("analysis_time: 2025-03-04 (explicit)"),
        "{}",
        stdout(&human)
    );

    let json = run(
        fixture.path(),
        &[
            "--as-of",
            "2025-03-04",
            "--format",
            "json",
            "explain",
            "per-package",
        ],
        None,
    );
    assert!(json.status.success(), "{}", stderr(&json));
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["analysis_time"]["date"], "2025-03-04");
    assert_eq!(value["analysis_time"]["source"], "explicit");
}

#[test]
fn expect_matches_findings_include_the_analysis_date_directly_and_nested() {
    let direct = "version: 1\n\
                  rules:\n\
                  \x20 - id: stale\n\
                  \x20   kind: git_blame_age\n\
                  \x20   paths: 'missing/**/*.rs'\n\
                  \x20   pattern: 'TODO'\n\
                  \x20   max_age_days: 30\n\
                  \x20   expect_matches: true\n\
                  \x20   message: 'review stale debt'\n\
                  \x20   level: warning\n";
    let nested = "version: 1\n\
                  rules:\n\
                  \x20 - id: per-package\n\
                  \x20   kind: for_each_dir\n\
                  \x20   select: pkg\n\
                  \x20   message: 'review package debt'\n\
                  \x20   level: warning\n\
                  \x20   require:\n\
                  \x20     - kind: git_blame_age\n\
                  \x20       paths: '{path}/missing/**/*.rs'\n\
                  \x20       pattern: 'TODO'\n\
                  \x20       max_age_days: 30\n\
                  \x20       expect_matches: true\n\
                  \x20       message: 'nested scope is empty'\n";

    for (config, expected) in [
        (direct, "review stale debt (as of 2025-03-04)"),
        (nested, "review package debt (as of 2025-03-04)"),
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("pkg")).unwrap();
        git(root.path(), &["init", "-q"], None);
        std::fs::write(root.path().join(".alint.yml"), config).unwrap();
        let output = run(
            root.path(),
            &[
                "--as-of",
                "2025-03-04",
                "--fail-on-warning",
                "--format",
                "json",
                "check",
            ],
            None,
        );
        assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            value["results"][0]["violations"][0]["message"],
            expected,
            "{}",
            stdout(&output)
        );
    }
}

#[test]
fn explicit_time_does_not_bypass_git_and_an_unborn_repo_exposes_the_missing_fallback() {
    let outside_git = fixture();
    let explicit = run(
        outside_git.path(),
        &["--as-of", "2025-03-04", "check"],
        None,
    );
    assert_eq!(explicit.status.code(), Some(1), "{}", stderr(&explicit));
    let combined = format!("{}{}", stdout(&explicit), stderr(&explicit));
    assert!(
        combined.contains("Git is unavailable") && !combined.contains("analysis time unavailable:"),
        "{combined}"
    );

    let unborn = fixture();
    git(unborn.path(), &["init", "-q"], None);
    let missing = run(unborn.path(), &["check"], None);
    assert_eq!(missing.status.code(), Some(1));
    let combined = format!("{}{}", stdout(&missing), stderr(&missing));
    assert!(
        combined.contains("analysis failure")
            && combined.contains("analysis time unavailable:")
            && combined.contains("pass --as-of")
            && combined.contains("YYYY-MM-DD"),
        "{combined}"
    );
}

#[test]
fn analysis_failure_reaches_every_cli_format_and_is_deterministic() {
    let fixture = fixture();
    git(fixture.path(), &["init", "-q"], None);
    for format in [
        "human", "json", "sarif", "github", "gitlab", "markdown", "junit", "agent",
    ] {
        let first = run(fixture.path(), &["--format", format, "check"], None);
        let second = run(fixture.path(), &["--format", format, "check"], None);
        assert_eq!(first.status.code(), Some(1), "{format}: {}", stderr(&first));
        assert_eq!(
            first.stdout, second.stdout,
            "{format} stdout was nondeterministic"
        );
        assert_eq!(
            first.stderr, second.stderr,
            "{format} stderr was nondeterministic"
        );
        let combined = format!("{}{}", stdout(&first), stderr(&first));
        for sentinel in ["stale", "evaluation", "analysis time unavailable"] {
            assert!(
                combined.contains(sentinel),
                "{format} dropped `{sentinel}`:\n{combined}",
            );
        }
    }
}

#[test]
fn baseline_refuses_and_every_fix_mode_fails_on_analysis_failure() {
    let fixture = fixture();
    git(fixture.path(), &["init", "-q"], None);
    let baseline_path = fixture.path().join("failure-baseline.json");
    let baseline = run(
        fixture.path(),
        &["baseline", "--output", baseline_path.to_str().unwrap()],
        None,
    );
    assert_eq!(baseline.status.code(), Some(1), "{}", stderr(&baseline));
    assert!(!baseline_path.exists(), "failure must not be baselined");
    assert!(
        stderr(&baseline).contains("refusing to write baseline")
            && stderr(&baseline).contains("analysis failure"),
        "{}",
        stderr(&baseline),
    );

    std::fs::write(
        fixture.path().join("empty-baseline.json"),
        "{\"schema_version\":1}\n",
    )
    .unwrap();
    for args in [
        &["fix"][..],
        &["fix", "--dry-run"][..],
        &["fix", "--diff"][..],
        &["fix", "--fix-only"][..],
        &["fix", "--baseline", "empty-baseline.json"][..],
    ] {
        let output = run(fixture.path(), args, None);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{args:?}: {}{}",
            stdout(&output),
            stderr(&output),
        );
        let combined = format!("{}{}", stdout(&output), stderr(&output));
        assert!(
            combined.contains("analysis failure") && combined.contains("analysis time unavailable"),
            "{args:?}: {combined}",
        );
    }
}

#[test]
fn malformed_flag_and_environment_values_fail_before_running() {
    let fixture = fixture();
    for (args, env_value) in [
        ((&["--as-of", "2025-2-03", "check"] as &[&str]), None),
        ((&["check"] as &[&str]), Some("tomorrow")),
    ] {
        let output = run(fixture.path(), args, env_value);
        assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
        assert!(
            stderr(&output).contains("analysis date must be exactly YYYY-MM-DD"),
            "{}",
            stderr(&output)
        );
    }
}

#[test]
fn unsupported_commands_reject_analysis_time() {
    let fixture = fixture();
    let output = run(fixture.path(), &["--as-of", "2025-03-04", "list"], None);
    assert!(!output.status.success());
    assert!(
        stderr(&output)
            .contains("`--as-of` applies only to `check`, `fix`, `baseline`, `explain`, and `lsp`"),
        "{}",
        stderr(&output)
    );
}
