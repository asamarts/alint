//! `alint test`: deterministic discovery and execution of public policy tests.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use alint_output::{sanitize_terminal as term, sanitize_terminal_multiline as term_multiline};
use alint_testkit::{
    Error as RunnerError, MismatchChannel, PackageRoot, PolicyTest, PolicyTestIssue,
    PolicyTestRecord, PolicyTestReport, PolicyTestStatus, Runner, RunnerOptions, TestStep,
    has_policy_test_discriminator,
};
use anyhow::{Result, bail};

use crate::cli::Cli;

#[derive(Debug)]
pub(crate) struct RunOptions<'a> {
    pub(crate) paths: &'a [PathBuf],
    pub(crate) tags: &'a [String],
    pub(crate) filter: Option<&'a str>,
    pub(crate) update_snapshots: bool,
    pub(crate) allow_spawn: bool,
    pub(crate) allow_host_keyring: bool,
    pub(crate) format: &'a str,
}

#[derive(Debug)]
struct DiscoveredTest {
    path: PathBuf,
    source: String,
    yaml: Option<String>,
}

#[derive(Debug)]
struct SelectedTest {
    path: PathBuf,
    source: String,
    test: PolicyTest,
    snapshot_target: Option<PathBuf>,
}

pub(crate) fn run(options: &RunOptions<'_>, cli: &Cli) -> Result<ExitCode> {
    reject_incompatible_globals(cli)?;
    let current = std::env::current_dir()?;
    let current = current.canonicalize()?;
    let discovered = match discover(options.paths, &current) {
        Ok(discovered) => discovered,
        Err(issues) => {
            render_report(
                &PolicyTestReport::with_issues(0, Vec::new(), issues),
                options.format,
            )?;
            return Ok(ExitCode::from(2));
        }
    };
    if discovered.is_empty() {
        let source = if options.paths.is_empty() {
            "alint-tests"
        } else {
            "the requested paths"
        };
        render_report(
            &PolicyTestReport::with_issues(
                0,
                Vec::new(),
                vec![PolicyTestIssue::new(
                    "no_tests",
                    format!("no policy-test YAML documents found below {source}"),
                )],
            ),
            options.format,
        )?;
        return Ok(ExitCode::from(2));
    }

    let mut invalid = Vec::new();
    let mut parsed = Vec::new();
    for source in discovered {
        match parse_test(&source) {
            Ok(test) => parsed.push(SelectedTest {
                path: source.path,
                source: source.source,
                test,
                snapshot_target: None,
            }),
            Err(record) => invalid.push(*record),
        }
    }

    parsed.retain(|candidate| selected(candidate, options.tags, options.filter));
    if parsed.is_empty() && invalid.is_empty() {
        render_report(
            &PolicyTestReport::with_issues(
                0,
                Vec::new(),
                vec![PolicyTestIssue::new(
                    "no_selected_tests",
                    "policy-test filters selected no documents",
                )],
            ),
            options.format,
        )?;
        return Ok(ExitCode::from(2));
    }
    parsed.sort_by(|left, right| {
        left.source
            .as_bytes()
            .cmp(right.source.as_bytes())
            .then_with(|| left.test.name.as_bytes().cmp(right.test.name.as_bytes()))
    });

    let selected_count = parsed.len() + invalid.len();
    let runner = Runner::new(RunnerOptions {
        allow_spawn: options.allow_spawn,
        allow_host_keyring: options.allow_host_keyring,
        update_snapshots: options.update_snapshots,
    })
    .with_registry(cli.registry());
    let mut operational_issues = Vec::new();
    collect_preflight_issues(&mut parsed, &runner, &mut invalid, &mut operational_issues);
    if let Some(exit) =
        render_preflight_failures(invalid, operational_issues, selected_count, options.format)?
    {
        return Ok(exit);
    }
    execute_selected(&parsed, &runner, options, selected_count)
}

fn render_preflight_failures(
    mut invalid: Vec<PolicyTestRecord>,
    operational_issues: Vec<PolicyTestIssue>,
    selected_count: usize,
    format: &str,
) -> Result<Option<ExitCode>> {
    if invalid.is_empty() && operational_issues.is_empty() {
        return Ok(None);
    }
    invalid.sort_by(|left, right| {
        left.source
            .as_bytes()
            .cmp(right.source.as_bytes())
            .then_with(|| left.name.as_bytes().cmp(right.name.as_bytes()))
    });
    let mut batch_issues = Vec::new();
    if !invalid.is_empty() {
        batch_issues.push(PolicyTestIssue::new(
            "validation_aborted",
            "policy-test execution was skipped because the selected batch is invalid",
        ));
    }
    let operational = !operational_issues.is_empty();
    if operational {
        batch_issues.push(PolicyTestIssue::new(
            "preflight_aborted",
            "policy-test execution was skipped after an operational preflight failure",
        ));
        batch_issues.extend(operational_issues);
    }
    render_report(
        &PolicyTestReport::with_issues(selected_count, invalid, batch_issues),
        format,
    )?;
    Ok(Some(ExitCode::from(if operational { 3 } else { 2 })))
}

fn collect_preflight_issues(
    candidates: &mut [SelectedTest],
    runner: &Runner,
    invalid: &mut Vec<PolicyTestRecord>,
    operational_issues: &mut Vec<PolicyTestIssue>,
) {
    let mut issues: Vec<Vec<PolicyTestIssue>> = (0..candidates.len()).map(|_| Vec::new()).collect();
    preflight_snapshot_targets(candidates, &mut issues);
    for (index, candidate) in candidates.iter().enumerate() {
        if let Err(error) = runner.preflight(&candidate.test, &candidate.path) {
            if runner_error_is_operational(&error) {
                operational_issues.extend(runner_issues(&error, &candidate.path).into_iter().map(
                    |mut issue| {
                        issue.message = format!("{}: {}", candidate.source, issue.message);
                        issue
                    },
                ));
            } else {
                issues[index].extend(runner_issues(&error, &candidate.path));
            }
        }
    }
    for (candidate, issues) in candidates.iter().zip(issues) {
        if !issues.is_empty() {
            invalid.push(PolicyTestRecord::invalid(
                candidate.source.clone(),
                Some(&candidate.test),
                issues,
            ));
        }
    }
}

fn execute_selected(
    candidates: &[SelectedTest],
    runner: &Runner,
    options: &RunOptions<'_>,
    selected_count: usize,
) -> Result<ExitCode> {
    let mut runs = Vec::with_capacity(selected_count);
    for candidate in candidates {
        let result = if options.update_snapshots {
            runner.run_deferred_snapshot_update(&candidate.test, &candidate.path)
        } else {
            runner.run(&candidate.test, &candidate.path)
        };
        match result {
            Ok(run) => runs.push(run),
            Err(error) => {
                let operational = runner_error_is_operational(&error);
                let mut records = executed_records(&candidates[..runs.len()], &runs);
                let mut report_issues = vec![PolicyTestIssue::new(
                    "execution_aborted",
                    if operational {
                        "policy-test execution aborted after an operational runner failure"
                    } else {
                        "policy-test execution aborted because a test became invalid after preflight"
                    },
                )];
                if operational {
                    report_issues.extend(runner_issues(&error, &candidate.path).into_iter().map(
                        |mut issue| {
                            issue.message = format!("{}: {}", candidate.source, issue.message);
                            issue
                        },
                    ));
                } else {
                    records.push(PolicyTestRecord::invalid(
                        candidate.source.clone(),
                        Some(&candidate.test),
                        runner_issues(&error, &candidate.path),
                    ));
                }
                let report = PolicyTestReport::with_issues(selected_count, records, report_issues);
                render_report(&report, options.format)?;
                return Ok(ExitCode::from(if operational { 3 } else { 2 }));
            }
        }
    }

    let failed = runs
        .iter()
        .any(|run| run.status == alint_testkit::TestStatus::Failed);
    if options.update_snapshots && !failed {
        let mut order: Vec<usize> = (0..candidates.len()).collect();
        order.sort_by(|left, right| {
            candidates[*left]
                .snapshot_target
                .cmp(&candidates[*right].snapshot_target)
        });
        for index in order {
            if let Err(error) = runs[index].commit_snapshot_update() {
                let report = PolicyTestReport::with_issues(
                    selected_count,
                    executed_records(candidates, &runs),
                    vec![PolicyTestIssue::new(
                        "snapshot_update_failed",
                        format!(
                            "snapshot updates stopped after a filesystem failure: {}",
                            safe_runner_error(&error, &candidates[index].path)
                        ),
                    )],
                );
                render_report(&report, options.format)?;
                return Ok(ExitCode::from(3));
            }
        }
    }

    let report = PolicyTestReport::with_issues(
        selected_count,
        executed_records(candidates, &runs),
        Vec::new(),
    );
    render_report(&report, options.format)?;
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn reject_incompatible_globals(cli: &Cli) -> Result<()> {
    let incompatible = if !cli.config.is_empty() {
        Some("--config")
    } else if cli.no_gitignore {
        Some("--no-gitignore")
    } else if cli.fail_on_warning {
        Some("--fail-on-warning")
    } else if cli.show_notes {
        Some("--show-notes")
    } else if cli.ascii {
        Some("--ascii")
    } else if cli.compact {
        Some("--compact")
    } else if cli.width.is_some() {
        Some("--width")
    } else if cli.no_docs {
        Some("--no-docs")
    } else if cli.progress != "auto" {
        Some("--progress")
    } else if cli.quiet {
        Some("--quiet")
    } else if cli.color != "auto" {
        Some("--color")
    } else {
        None
    };
    if let Some(flag) = incompatible {
        bail!("`{flag}` does not apply to `alint test`; each policy test owns its inputs")
    }
    Ok(())
}

fn discover(
    paths: &[PathBuf],
    current: &Path,
) -> Result<Vec<DiscoveredTest>, Vec<PolicyTestIssue>> {
    let defaults;
    let roots = if paths.is_empty() {
        defaults = vec![current.join("alint-tests")];
        defaults.as_slice()
    } else {
        paths
    };
    let mut files = BTreeMap::<PathBuf, (String, Option<String>)>::new();
    let mut issues = Vec::new();
    for root in roots {
        let explicit = !paths.is_empty();
        collect_path(root, explicit, current, &mut files, &mut issues);
    }
    if !issues.is_empty() {
        return Err(issues);
    }
    let mut discovered: Vec<_> = files
        .into_iter()
        .map(|(path, (source, yaml))| DiscoveredTest { path, source, yaml })
        .collect();
    discovered.sort_by(|left, right| left.source.as_bytes().cmp(right.source.as_bytes()));
    Ok(discovered)
}

fn collect_path(
    path: &Path,
    explicit: bool,
    current: &Path,
    files: &mut BTreeMap<PathBuf, (String, Option<String>)>,
    issues: &mut Vec<PolicyTestIssue>,
) {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if !explicit && error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            issues.push(path_issue("discovery_io", path, &error.to_string()));
            return;
        }
    };
    if metadata.file_type().is_symlink() {
        if explicit {
            issues.push(path_issue(
                "symlink_path",
                path,
                "explicit policy-test paths must not be symlinks",
            ));
        }
        return;
    }
    if metadata.is_file() {
        if !is_yaml(path) {
            if explicit {
                issues.push(path_issue(
                    "unsupported_extension",
                    path,
                    "policy-test files must use .yml or .yaml",
                ));
            }
            return;
        }
        match path.canonicalize() {
            Ok(canonical) => match normalized_source(&canonical, current) {
                Ok(source) => {
                    if files.contains_key(&canonical) {
                        return;
                    }
                    let yaml = if explicit {
                        None
                    } else {
                        match std::fs::read_to_string(&canonical) {
                            Ok(yaml) if has_policy_test_discriminator(&yaml) => Some(yaml),
                            Ok(_) => return,
                            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => return,
                            Err(error) => {
                                issues.push(path_issue(
                                    "discovery_io",
                                    &canonical,
                                    &error.to_string(),
                                ));
                                return;
                            }
                        }
                    };
                    files.insert(canonical, (source, yaml));
                }
                Err(issue) => issues.push(issue),
            },
            Err(error) => issues.push(path_issue("discovery_io", path, &error.to_string())),
        }
        return;
    }
    if !metadata.is_dir() {
        if explicit {
            issues.push(path_issue(
                "unsupported_path_type",
                path,
                "policy-test path must be a regular file or directory",
            ));
        }
        return;
    }
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => {
            issues.push(path_issue("discovery_io", path, &error.to_string()));
            return;
        }
    };
    let mut children = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => children.push(entry.path()),
            Err(error) => issues.push(path_issue("discovery_io", path, &error.to_string())),
        }
    }
    children.sort();
    for child in children {
        collect_path(&child, false, current, files, issues);
    }
}

fn normalized_source(path: &Path, current: &Path) -> Result<String, PolicyTestIssue> {
    let display = path.strip_prefix(current).unwrap_or(path);
    let Some(display) = display.to_str() else {
        return Err(path_issue(
            "non_utf8_path",
            path,
            "policy-test source paths must be UTF-8",
        ));
    };
    Ok(display.replace('\\', "/"))
}

fn is_yaml(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("yml" | "yaml")
    )
}

fn path_issue(code: &str, path: &Path, detail: &str) -> PolicyTestIssue {
    PolicyTestIssue::new(code, format!("{}: {detail}", path.display()))
}

fn parse_test(source: &DiscoveredTest) -> Result<PolicyTest, Box<PolicyTestRecord>> {
    let owned;
    let yaml = if let Some(yaml) = source.yaml.as_deref() {
        yaml
    } else {
        owned = std::fs::read_to_string(&source.path).map_err(|error| {
            Box::new(PolicyTestRecord::invalid(
                source.source.clone(),
                None,
                vec![PolicyTestIssue::new("read_error", error.to_string())],
            ))
        })?;
        &owned
    };
    let test = PolicyTest::from_yaml(yaml).map_err(|error| {
        Box::new(PolicyTestRecord::invalid(
            source.source.clone(),
            None,
            vec![PolicyTestIssue::new("parse_error", error.to_string())],
        ))
    })?;
    if let Err(diagnostics) = test.validate() {
        return Err(Box::new(PolicyTestRecord::invalid(
            source.source.clone(),
            Some(&test),
            diagnostics
                .iter()
                .map(PolicyTestIssue::from_diagnostic)
                .collect(),
        )));
    }
    Ok(test)
}

fn selected(candidate: &SelectedTest, tags: &[String], filter: Option<&str>) -> bool {
    tags.iter()
        .all(|tag| candidate.test.tags.iter().any(|candidate| candidate == tag))
        && filter.is_none_or(|filter| {
            candidate.test.name.contains(filter) || candidate.source.contains(filter)
        })
}

fn preflight_snapshot_targets(
    candidates: &mut [SelectedTest],
    issues: &mut [Vec<PolicyTestIssue>],
) {
    let mut targets: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
    for (index, candidate) in candidates.iter_mut().enumerate() {
        let Some(snapshot) = candidate.test.snapshot.as_deref() else {
            continue;
        };
        match PackageRoot::for_policy_test(&candidate.path)
            .and_then(|package| package.resolve_snapshot_target(snapshot))
        {
            Ok(target) => {
                targets.entry(target.clone()).or_default().push(index);
                candidate.snapshot_target = Some(target);
            }
            Err(error) => {
                issues[index].push(PolicyTestIssue::new("snapshot_path", error.to_string()));
            }
        }
    }
    for indices in targets.values().filter(|indices| indices.len() > 1) {
        let sources: BTreeSet<&str> = indices
            .iter()
            .map(|index| candidates[*index].source.as_str())
            .collect();
        let message = format!(
            "snapshot target is shared by multiple policy tests: {}",
            sources.into_iter().collect::<Vec<_>>().join(", ")
        );
        for index in indices {
            issues[*index].push(PolicyTestIssue::new("duplicate_snapshot", &message));
        }
    }
}

fn runner_issues(error: &RunnerError, source: &Path) -> Vec<PolicyTestIssue> {
    if let RunnerError::Definition(diagnostics) = error {
        return diagnostics
            .iter()
            .map(PolicyTestIssue::from_diagnostic)
            .collect();
    }
    let code = match error {
        RunnerError::Confinement(_) => "confinement",
        RunnerError::Capability(_) => "capability",
        RunnerError::GitFixture(_) => "git_fixture",
        RunnerError::Yaml(_) => "parse_error",
        RunnerError::NotADirectory(_) => "fixture_path",
        RunnerError::Io { .. } => "runner_io",
        _ => "runner_error",
    };
    vec![PolicyTestIssue::new(code, safe_runner_error(error, source))]
}

fn runner_error_is_operational(error: &RunnerError) -> bool {
    matches!(
        error,
        RunnerError::Io { .. }
            | RunnerError::Alint(_)
            | RunnerError::When(_)
            | RunnerError::Runtime(_)
    )
}

fn safe_runner_error(error: &RunnerError, source: &Path) -> String {
    let text = error.to_string();
    let temp = std::env::temp_dir();
    let without_temp = text.replace(temp.to_string_lossy().as_ref(), "<temporary-directory>");
    let package = source.parent().unwrap_or_else(|| Path::new("."));
    if package.parent().is_none() {
        return without_temp;
    }
    without_temp.replace(package.to_string_lossy().as_ref(), "<policy-test-package>")
}

fn executed_records(
    candidates: &[SelectedTest],
    runs: &[alint_testkit::PolicyTestRun],
) -> Vec<PolicyTestRecord> {
    candidates
        .iter()
        .zip(runs)
        .map(|(candidate, run)| {
            PolicyTestRecord::executed(candidate.source.clone(), &candidate.test, run)
        })
        .collect()
}

fn render_report(report: &PolicyTestReport, format: &str) -> Result<()> {
    match format {
        "json" => {
            let rendered = serde_json::to_string_pretty(report)?;
            crate::print_stdout_line(&rendered)
        }
        "human" => crate::print_stdout_line(&render_human(report)),
        other => {
            bail!("`alint test` supports only `--format human` or `--format json` (got {other:?})")
        }
    }
}

fn render_human(report: &PolicyTestReport) -> String {
    let mut out = String::new();
    for issue in &report.issues {
        write_multiline(
            &mut out,
            &format!("INVALID {}: ", term(&issue.code)),
            &issue.message,
        );
    }
    for test in &report.tests {
        let status = match test.status {
            PolicyTestStatus::Passed => "PASS",
            PolicyTestStatus::Failed => "FAIL",
            PolicyTestStatus::Invalid => "INVALID",
            PolicyTestStatus::Skipped => "SKIP",
        };
        writeln!(
            out,
            "{status} {} - {}",
            term(&test.source),
            term(&test.name)
        )
        .unwrap();
        for step in &test.steps {
            let failed = test
                .mismatches
                .iter()
                .any(|mismatch| mismatch.step == Some(step.index));
            writeln!(
                out,
                "  {} step {}: {} ({:?})",
                if failed { "FAIL" } else { "PASS" },
                step.index + 1,
                step_name(step.operation),
                step.observation.exit,
            )
            .unwrap();
        }
        for mismatch in &test.mismatches {
            let prefix = if test.status == PolicyTestStatus::Skipped
                && mismatch.channel == MismatchChannel::Platform
            {
                "  reason: ".to_string()
            } else {
                format!("  mismatch {:?}: ", mismatch.channel)
            };
            write_multiline(&mut out, &prefix, &mismatch.detail);
        }
        for issue in &test.issues {
            let prefix = if let Some(field) = &issue.field {
                format!("  {} at {}: ", term(&issue.code), term(field))
            } else {
                format!("  {}: ", term(&issue.code))
            };
            write_multiline(&mut out, &prefix, &issue.message);
        }
    }
    write!(
        out,
        "Summary: {} selected; {} passed, {} failed, {} invalid, {} skipped",
        report.summary.selected,
        report.summary.passed,
        report.summary.failed,
        report.summary.invalid,
        report.summary.skipped,
    )
    .unwrap();
    out
}

fn write_multiline(out: &mut String, first_prefix: &str, message: &str) {
    let safe = term_multiline(message);
    for (index, line) in safe.split('\n').enumerate() {
        let prefix = if index == 0 { first_prefix } else { "  | " };
        writeln!(out, "{prefix}{line}").unwrap();
    }
}

fn step_name(step: TestStep) -> &'static str {
    match step {
        TestStep::Check => "check",
        TestStep::Fix => "fix",
        TestStep::FixUnsafe => "fix_unsafe",
        TestStep::FixDryRun => "fix_dry_run",
        TestStep::CheckChanged => "check_changed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_filter_is_an_and_filter_and_text_matches_path_or_name() {
        let test = PolicyTest::from_yaml(
            "test_version: 1\nname: named case\ntags: [fast, unix-only]\ngiven:\n  tree: {}\n  config: 'version: 1\\nrules: []\\n'\nwhen: [check]\nexpect:\n  - exit: success\n    violations: []\n",
        )
        .unwrap();
        let candidate = SelectedTest {
            path: PathBuf::from("case.yml"),
            source: "alint-tests/group/case.yml".to_string(),
            test,
            snapshot_target: None,
        };
        assert!(selected(&candidate, &["fast".to_string()], Some("group")));
        assert!(selected(
            &candidate,
            &["fast".to_string(), "unix-only".to_string()],
            Some("named")
        ));
        assert!(!selected(&candidate, &["missing".to_string()], None));
    }

    #[test]
    fn human_report_sanitizes_control_text() {
        let report = PolicyTestReport::with_issues(
            0,
            Vec::new(),
            vec![PolicyTestIssue::new("bad\u{1b}[31m", "line\nnext")],
        );
        let rendered = render_human(&report);
        assert!(!rendered.contains('\u{1b}'));
        assert!(rendered.contains("\\x1b"));
    }

    #[test]
    fn runner_errors_redact_temporary_and_package_paths() {
        let package = std::env::current_dir().unwrap().join("redaction-package");
        let source = package.join("case.yml");
        let error = RunnerError::runtime(format!(
            "package={} temp={}",
            package.join("config.yml").display(),
            std::env::temp_dir().join("runner").display()
        ));
        let rendered = safe_runner_error(&error, &source);
        assert!(!rendered.contains(package.to_string_lossy().as_ref()));
        assert!(!rendered.contains(std::env::temp_dir().to_string_lossy().as_ref()));
        assert!(rendered.contains("<policy-test-package>"));
        assert!(rendered.contains("<temporary-directory>"));
    }
}
