//! Drive a [`Scenario`] against an
//! isolated tempdir and collect per-step observations for the
//! harness to assert on.

use std::path::{Path, PathBuf};
use std::process::Command;

use alint_core::{Engine, FixReport, FixStatus, Level, Report, RuleEntry, WalkOptions, walk};
use tempfile::TempDir;

use crate::error::{Error, Result};
use crate::scenario::{CommitSpec, ExpectStep, GivenGit, Scenario, Step};
use crate::treespec::{VerifyReport, materialize, verify};

/// Concrete outcome of one step. The runner collects these in order;
/// the harness (or `assert_scenario`) checks them against the
/// scenario's `expect:` list.
#[derive(Debug)]
pub enum StepOutcome {
    Check(Report),
    Fix(FixReport),
}

/// Everything a harness learns from running a scenario.
#[derive(Debug)]
pub struct ScenarioRun {
    pub root: PathBuf,
    /// Kept alive so `root` stays valid; drops clean up the tempdir.
    _tmp: TempDir,
    pub steps: Vec<StepOutcome>,
}

/// Materialize the scenario, drive its steps, collect outcomes.
/// Caller asserts against [`ScenarioRun`] via
/// [`assert_scenario`] (or by hand).
pub fn run_scenario(scenario: &Scenario) -> Result<ScenarioRun> {
    scenario.validate()?;
    let tmp = tempfile::Builder::new()
        .prefix("alint-testkit-")
        .tempdir()
        .map_err(|source| Error::Io {
            path: std::env::temp_dir(),
            source,
        })?;
    let root = tmp.path().to_path_buf();

    materialize(&scenario.given.tree, &root)?;
    let config_path = root.join(".alint.yml");
    std::fs::write(&config_path, &scenario.given.config).map_err(|source| Error::Io {
        path: config_path.clone(),
        source,
    })?;

    if let Some(git_spec) = &scenario.given.git {
        init_git_for_scenario(&root, git_spec)?;
    }

    let mut steps = Vec::with_capacity(scenario.when.len());
    for step in &scenario.when {
        let outcome = run_step(*step, &root)?;
        steps.push(outcome);
    }

    Ok(ScenarioRun {
        root,
        _tmp: tmp,
        steps,
    })
}

/// Run the scenario's `given.git:` setup against the tempdir.
/// Each git invocation is shelled out via the `git` on PATH;
/// scenarios in environments without git skip the setup with a
/// clear error rather than silently producing a non-git tempdir.
///
/// The runner sets minimal user.name / user.email config so
/// `git commit` doesn't fail in CI environments whose default
/// identity isn't configured. Both values are scoped to the
/// scenario tempdir (`-c user.…=`) and don't touch the host's
/// global config.
fn init_git_for_scenario(root: &Path, spec: &GivenGit) -> Result<()> {
    if !spec.init {
        return Ok(());
    }
    git(root, &["init", "-q", "-b", "main"])?;
    if !spec.add.is_empty() {
        let mut args: Vec<&str> = vec!["add", "--"];
        args.extend(spec.add.iter().map(String::as_str));
        git(root, &args)?;
    }
    if !spec.add_force.is_empty() {
        // `git add -f` forces tracking of paths that would
        // normally be excluded by `.gitignore`. The bazel-style
        // tracked-AND-gitignored pattern needs this — the file
        // is committed upstream (so future clones see it) AND
        // gitignored (so contributor edits don't accidentally
        // get committed). The `--` is positional-arg disambiguator;
        // `-f` must precede it.
        let mut args: Vec<&str> = vec!["add", "-f", "--"];
        args.extend(spec.add_force.iter().map(String::as_str));
        git(root, &args)?;
    }
    // Chain of empty commits (oldest first). Mutually exclusive
    // with the add/commit single-commit path above. Use one or the
    // other in any given scenario; mixing them is a fixture smell.
    if !spec.commits.is_empty() {
        if !spec.add.is_empty() || !spec.add_force.is_empty() {
            return Err(Error::scenario(
                "git: cannot combine `add:`/`add_force:` with `commits:` — use one path or the other"
                    .to_string(),
            ));
        }
        for commit in &spec.commits {
            commit_one(root, commit)?;
        }
        return Ok(());
    }
    if spec.add.is_empty() && spec.add_force.is_empty() {
        return Ok(());
    }
    if spec.commit {
        git(
            root,
            &[
                "-c",
                "user.name=alint scenario",
                "-c",
                "user.email=scenario@alint.test",
                "commit",
                "-q",
                "-m",
                "scenario commit",
            ],
        )?;
    }
    Ok(())
}

/// Apply one `commits:` entry. A bare subject is an empty commit; a detailed
/// entry stages its `add` paths (or `--allow-empty` when it stages none),
/// commits with the given message, and backdates when `date` is set.
fn commit_one(root: &Path, commit: &CommitSpec) -> Result<()> {
    let (message, add, date): (&str, &[String], Option<&str>) = match commit {
        CommitSpec::Subject(subject) => (subject.as_str(), &[], None),
        CommitSpec::Detailed(d) => (d.message.as_str(), d.add.as_slice(), d.date.as_deref()),
    };
    if !add.is_empty() {
        let mut args: Vec<&str> = vec!["add", "--"];
        args.extend(add.iter().map(String::as_str));
        git(root, &args)?;
    }
    let mut args: Vec<&str> = vec![
        "-c",
        "user.name=alint scenario",
        "-c",
        "user.email=scenario@alint.test",
        "commit",
        "-q",
    ];
    if add.is_empty() {
        args.push("--allow-empty");
    }
    args.push("-m");
    args.push(message);
    match date {
        Some(date) => git_env(
            root,
            &args,
            &[("GIT_AUTHOR_DATE", date), ("GIT_COMMITTER_DATE", date)],
        ),
        None => git(root, &args),
    }
}

fn git(root: &Path, args: &[&str]) -> Result<()> {
    git_env(root, args, &[])
}

fn git_env(root: &Path, args: &[&str], envs: &[(&str, &str)]) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(root).args(args);
    for (key, val) in envs {
        cmd.env(key, val);
    }
    let out = cmd.output().map_err(|source| Error::Io {
        path: root.to_path_buf(),
        source,
    })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(Error::scenario(format!(
            "git {args:?} in {} failed: {stderr}",
            root.display()
        )));
    }
    Ok(())
}

fn run_step(step: Step, root: &Path) -> Result<StepOutcome> {
    // Scenarios don't share state with the user's real cache:
    // HTTPS `extends:` (if the scenario uses it) resolves into
    // a per-scenario subdirectory of the scenario's tempdir.
    let cache = alint_dsl::extends::Cache::at(root.join(".alint-cache"));
    let opts = alint_dsl::LoadOptions::with_cache(cache);
    let config = alint_dsl::load_with(&root.join(".alint.yml"), &opts)?;
    let registry = alint_rules::builtin_registry();

    let mut entries: Vec<RuleEntry> = Vec::with_capacity(config.rules.len());
    for spec in &config.rules {
        if matches!(spec.level, Level::Off) {
            continue;
        }
        let rule = registry.build(spec)?;
        let mut entry = RuleEntry::new(rule);
        if let Some(src) = &spec.when {
            let expr = alint_core::when::parse(src)?;
            entry = entry.with_when(expr);
        }
        entries.push(entry);
    }
    let mut engine = Engine::from_entries(entries, registry)
        .with_facts(config.facts.clone())
        .with_vars(config.vars.clone())
        .with_fix_size_limit(config.fix_size_limit);

    let walk_opts = WalkOptions {
        respect_gitignore: config.respect_gitignore,
        extra_ignores: config.ignore.clone(),
    };
    let index = walk(root, &walk_opts)?;

    if matches!(step, Step::CheckChanged) {
        let set = alint_core::git::collect_changed_paths(root, None).ok_or_else(|| {
            Error::scenario(format!(
                "scenario uses `check_changed` but `git ls-files --modified` failed at {} \
                 — make sure the scenario has a `given.git:` block",
                root.display()
            ))
        })?;
        engine = engine.with_changed_paths(set);
    }

    Ok(match step {
        Step::Check | Step::CheckChanged => StepOutcome::Check(engine.run(root, &index)?),
        Step::Fix => StepOutcome::Fix(engine.fix(root, &index, false)?),
        Step::FixDryRun => StepOutcome::Fix(engine.fix(root, &index, true)?),
    })
}

/// Assert a [`ScenarioRun`] matches its scenario's `expect:` list
/// and `expect_tree:` block. Returns `Ok(())` if every assertion
/// passes; otherwise a detailed error describing the first failure.
pub fn assert_scenario(scenario: &Scenario, run: &ScenarioRun) -> Result<()> {
    if scenario.expect.len() != run.steps.len() {
        return Err(Error::scenario(format!(
            "scenario {:?}: expected {} step result(s), got {}",
            scenario.name,
            scenario.expect.len(),
            run.steps.len()
        )));
    }
    for (i, (exp, got)) in scenario.expect.iter().zip(&run.steps).enumerate() {
        assert_step(scenario, i, exp, got)?;
    }
    if let Some(expected) = &scenario.expect_tree {
        let mode = scenario.expect_tree_mode.into();
        let mut report = verify(expected, &run.root, mode)?;
        // The runner writes `.alint.yml` into the tempdir so alint
        // can find its config; that file is scenario machinery, not
        // tree-under-test content. Strict verification should not
        // flag it as `Extra`.
        report.discrepancies.retain(|d| !is_runner_machinery(d));
        if !report.is_match() {
            return Err(Error::scenario(format!(
                "scenario {:?}: expect_tree mismatch:\n{report}",
                scenario.name,
            )));
        }
        let _: VerifyReport = report; // type-check the return
    }
    Ok(())
}

fn is_runner_machinery(d: &crate::treespec::Discrepancy) -> bool {
    matches!(
        d,
        crate::treespec::Discrepancy::Extra { path } if is_machinery_path(path)
    )
}

fn is_machinery_path(path: &str) -> bool {
    // The runner writes `.alint.yml` (scenario config) and a
    // `.alint-cache/` tree (scoped HTTPS extends cache) into the
    // scenario tempdir. Neither is content-under-test, so strict
    // tree comparisons silently ignore them.
    path == ".alint.yml" || path == ".alint-cache" || path.starts_with(".alint-cache/")
}

fn assert_step(
    scenario: &Scenario,
    idx: usize,
    expect: &ExpectStep,
    outcome: &StepOutcome,
) -> Result<()> {
    let prefix = format!("scenario {:?} step {idx}", scenario.name);

    match (outcome, expect.violations.as_ref()) {
        (StepOutcome::Check(report), Some(wanted)) => {
            assert_violations(&prefix, report, wanted)?;
        }
        (StepOutcome::Check(_), None)
            if expect.applied.is_some()
                || expect.skipped.is_some()
                || expect.unfixable.is_some() =>
        {
            return Err(Error::scenario(format!(
                "{prefix}: check step cannot carry applied/skipped/unfixable expectations"
            )));
        }
        (StepOutcome::Check(_), None) => {}
        (StepOutcome::Fix(report), _) => {
            if let Some(applied) = &expect.applied {
                assert_fix_status(&prefix, "applied", report, applied, |s| {
                    matches!(s, FixStatus::Applied(_))
                })?;
            }
            if let Some(skipped) = &expect.skipped {
                assert_fix_status(&prefix, "skipped", report, skipped, |s| {
                    matches!(s, FixStatus::Skipped(_))
                })?;
            }
            if let Some(unfixable) = &expect.unfixable {
                assert_fix_status(&prefix, "unfixable", report, unfixable, |s| {
                    matches!(s, FixStatus::Unfixable)
                })?;
            }
            if expect.violations.is_some() {
                return Err(Error::scenario(format!(
                    "{prefix}: fix step cannot carry `violations` expectation"
                )));
            }
        }
    }
    Ok(())
}

fn assert_violations(
    prefix: &str,
    report: &Report,
    wanted: &[crate::scenario::ExpectViolation],
) -> Result<()> {
    let mut actual: Vec<(String, alint_core::Level, Option<String>)> = Vec::new();
    for r in &report.results {
        for v in &r.violations {
            // Normalise to forward slashes so scenario YAML can
            // assert `src/main.rs` regardless of host OS.
            // Windows' Path::display() emits `src\main.rs`.
            actual.push((
                r.rule_id.to_string(),
                r.level,
                v.path
                    .as_ref()
                    .map(|p| p.display().to_string().replace('\\', "/")),
            ));
        }
    }
    if actual.len() != wanted.len() {
        return Err(Error::scenario(format!(
            "{prefix}: expected {} violation(s), got {}: {:?}",
            wanted.len(),
            actual.len(),
            actual.iter().map(|(r, _, _)| r).collect::<Vec<_>>()
        )));
    }
    // Order-insensitive match: every wanted must find one actual
    // that hasn't been claimed yet.
    let mut claimed = vec![false; actual.len()];
    for w in wanted {
        let found = actual.iter().enumerate().position(|(i, a)| {
            !claimed[i]
                && a.0 == w.rule
                && w.level.is_none_or(|lv| lv.matches(a.1))
                && match (&w.path, &a.2) {
                    (Some(p), Some(ap)) => p == ap,
                    (None, _) => true,
                    (Some(_), None) => false,
                }
        });
        match found {
            Some(i) => claimed[i] = true,
            None => {
                return Err(Error::scenario(format!(
                    "{prefix}: no violation matching rule={:?} level={:?} path={:?}. Actual: {actual:?}",
                    w.rule, w.level, w.path,
                )));
            }
        }
    }
    Ok(())
}

fn assert_fix_status(
    prefix: &str,
    label: &str,
    report: &FixReport,
    wanted_rules: &[String],
    pred: impl Fn(&FixStatus) -> bool,
) -> Result<()> {
    let actual: Vec<String> = report
        .results
        .iter()
        .filter(|r| r.items.iter().any(|it| pred(&it.status)))
        .map(|r| r.rule_id.to_string())
        .collect();
    let mut expected = wanted_rules.to_vec();
    expected.sort();
    let mut got = actual.clone();
    got.sort();
    if expected != got {
        return Err(Error::scenario(format!(
            "{prefix}: {label} rule set mismatch. expected={expected:?}, got={got:?}"
        )));
    }
    Ok(())
}
