//! Validating, confined runner for versioned public policy tests.

mod matching;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use alint_core::{
    Applicability, Capability, Engine, FactKind, FileIndex, FixReport, FixStatus, Level, Report,
    RuleEntry, RuleSpec, WalkOptions, walk,
};
use schemars::JsonSchema;
use serde::Serialize;
use tempfile::TempDir;

use matching::match_step;

use crate::confinement::{ConfinementError, PackageEntryKind, PackageRoot};
use crate::error::{Error, Result};
use crate::policy_test::{
    ExpectedExit, PolicyCommit, PolicyGit, PolicyTest, StepExpectation, TestStep,
    TreeExpectationMode,
};
use crate::runner_artifacts::{
    PendingSnapshotUpdate, reconcile_snapshot, render_fix_diff, repository_files, staged_between,
};
use crate::treespec::{Discrepancy, VerifyMode, materialize, verify};

/// Host authority granted to one runner instance.
#[derive(Debug, Clone, Default)]
pub struct RunnerOptions {
    pub allow_spawn: bool,
    pub allow_host_keyring: bool,
    pub update_snapshots: bool,
}

/// Validating high-level policy-test runner. No unchecked execution API is exposed.
#[derive(Debug, Clone)]
pub struct Runner {
    options: RunnerOptions,
    registry: alint_core::RuleRegistry,
}

struct ExecutionContext<'a> {
    repository: &'a Path,
    sandbox: &'a Path,
    config_path: &'a Path,
    source_provider: Arc<dyn alint_dsl::ConfigSourceProvider>,
    environment: Arc<dyn alint_dsl::EnvironmentProvider>,
    process_environment: &'a BTreeMap<String, String>,
    test: &'a PolicyTest,
    package: &'a PackageRoot,
}

struct PreparedPolicyTest {
    package: PackageRoot,
    sandbox: TempDir,
    repository: PathBuf,
    config_path: PathBuf,
    source_provider: Arc<dyn alint_dsl::ConfigSourceProvider>,
    environment: Arc<dyn alint_dsl::EnvironmentProvider>,
    process_environment: BTreeMap<String, String>,
    _git_environment: alint_core::git::ScopedProcessEnvironment,
}

// Stack-only transient avoids a heap allocation on every policy-test step.
#[allow(clippy::large_enum_variant)]
enum PreparedStep {
    Ready {
        engine: Engine,
        index: FileIndex,
        walk_options: WalkOptions,
    },
    ConfigurationError(StepObservation),
}

impl Default for Runner {
    fn default() -> Self {
        Self::new(RunnerOptions::default())
    }
}

impl Runner {
    #[must_use]
    pub fn new(options: RunnerOptions) -> Self {
        Self {
            options,
            registry: alint_rules::builtin_registry(),
        }
    }

    /// Replace the built-in rule registry, e.g. for a host that adds plugin
    /// kinds. The registry also drives config loading and capability checks.
    #[must_use]
    pub fn with_registry(mut self, registry: alint_core::RuleRegistry) -> Self {
        self.registry = registry;
        self
    }

    /// Execute one parsed test document from `source`.
    ///
    /// Definition, package-path, and capability failures return a typed runner
    /// error. A config error produced by the config under test is an observation
    /// and can therefore match `exit: configuration_error`.
    pub fn run(&self, test: &PolicyTest, source: &Path) -> Result<PolicyTestRun> {
        self.run_inner(test, source, false)
    }

    /// Execute one test while deferring an enabled snapshot update.
    ///
    /// The caller can execute a batch and commit each pending update only after
    /// every test's inline expectations pass. `RunnerOptions::update_snapshots`
    /// must be enabled.
    pub fn run_deferred_snapshot_update(
        &self,
        test: &PolicyTest,
        source: &Path,
    ) -> Result<PolicyTestRun> {
        if !self.options.update_snapshots {
            return Err(Error::runtime(
                "deferred snapshot execution requires `update_snapshots: true`",
            ));
        }
        self.run_inner(test, source, true)
    }

    /// Validate and prepare one test without evaluating any rule or fixer.
    ///
    /// Batch callers use this before execution so definition, package-path,
    /// config-load, and capability failures cannot occur after an earlier test
    /// has already executed a user command.
    pub fn preflight(&self, test: &PolicyTest, source: &Path) -> Result<()> {
        if let Err(diagnostics) = test.validate() {
            return Err(Error::definition(diagnostics));
        }
        if platform_skip_reason(&test.tags).is_some() {
            return Ok(());
        }
        let prepared = self.prepare_policy_test(test, source)?;
        let context = ExecutionContext {
            repository: &prepared.repository,
            sandbox: prepared.sandbox.path(),
            config_path: &prepared.config_path,
            source_provider: prepared.source_provider.clone(),
            environment: prepared.environment.clone(),
            process_environment: &prepared.process_environment,
            test,
            package: &prepared.package,
        };
        let step = test.when[0];
        let _ = self.prepare_step(step, &context)?;
        Ok(())
    }

    fn prepare_policy_test(&self, test: &PolicyTest, source: &Path) -> Result<PreparedPolicyTest> {
        let package = PackageRoot::for_policy_test(source).map_err(Error::from)?;
        if let Some(snapshot) = test.snapshot.as_deref() {
            package
                .resolve_snapshot_target(snapshot)
                .map_err(Error::from)?;
        }
        let sandbox = tempfile::Builder::new()
            .prefix("alint-policy-test-")
            .tempdir()
            .map_err(|source_error| Error::Io {
                path: std::env::temp_dir(),
                source: source_error,
            })?;
        let repository = sandbox.path().join("repository");
        std::fs::create_dir(&repository).map_err(|source_error| Error::Io {
            path: repository.clone(),
            source: source_error,
        })?;
        materialize_repository(test, &package, &repository)?;
        let process_environment = deterministic_process_environment(
            test,
            sandbox.path(),
            self.options.allow_host_keyring && test.capabilities.host_keyring,
        )?;
        let git_environment =
            alint_core::git::scoped_process_environment(&repository, process_environment.clone());
        if let Some(git_spec) = &test.given.git {
            setup_git(&repository, git_spec, &process_environment)?;
        }
        let (config_path, source_provider) = config_source(test, &package, source)?;
        let environment: Arc<dyn alint_dsl::EnvironmentProvider> =
            Arc::new(FixedEnvironment(test.given.env.clone()));
        Ok(PreparedPolicyTest {
            package,
            sandbox,
            repository,
            config_path,
            source_provider,
            environment,
            process_environment,
            _git_environment: git_environment,
        })
    }

    fn run_inner(
        &self,
        test: &PolicyTest,
        source: &Path,
        defer_snapshot_update: bool,
    ) -> Result<PolicyTestRun> {
        if let Err(diagnostics) = test.validate() {
            return Err(Error::definition(diagnostics));
        }
        if let Some(reason) = platform_skip_reason(&test.tags) {
            return Ok(PolicyTestRun::skipped(source, test, reason));
        }

        let prepared = self.prepare_policy_test(test, source)?;
        let repository = &prepared.repository;
        let package = &prepared.package;
        let (observations, mut mismatches) = {
            let context = ExecutionContext {
                repository,
                sandbox: prepared.sandbox.path(),
                config_path: &prepared.config_path,
                source_provider: prepared.source_provider.clone(),
                environment: prepared.environment.clone(),
                process_environment: &prepared.process_environment,
                test,
                package,
            };
            let mut observations = Vec::with_capacity(test.when.len());
            let mut mismatches = Vec::new();
            for (index, (step, expectation)) in test.when.iter().zip(&test.expect).enumerate() {
                let mut observation = self.execute_step(*step, expectation, &context)?;
                sanitize_observation(&mut observation, &context);
                mismatches.extend(match_step(index, expectation, &observation));
                observations.push(observation);
            }
            (observations, mismatches)
        };

        if let Some(expected_tree) = &test.expect_tree {
            let mode = match test.expect_tree_mode {
                TreeExpectationMode::Strict => VerifyMode::Strict,
                TreeExpectationMode::Contains => VerifyMode::Contains,
            };
            let mut tree_report = verify(expected_tree, repository, mode)?;
            tree_report
                .discrepancies
                .retain(|item| !runner_owned_tree_path(item));
            for item in tree_report.discrepancies {
                mismatches.push(Mismatch {
                    step: None,
                    channel: MismatchChannel::Tree,
                    detail: safe_tree_mismatch(&item),
                });
            }
        }

        let pending_snapshot = reconcile_snapshot(
            test.snapshot.as_deref(),
            package,
            &observations,
            &mut mismatches,
            self.options.update_snapshots,
            defer_snapshot_update,
        )?;
        for mismatch in &mut mismatches {
            sanitize_field(&mut mismatch.detail, test, repository, package.path());
        }

        let status = if mismatches.is_empty() {
            TestStatus::Passed
        } else {
            TestStatus::Failed
        };
        Ok(PolicyTestRun {
            source: normalized_path(source),
            name: test.name.clone(),
            tags: test.tags.clone(),
            status,
            observations,
            mismatches,
            root: prepared.repository,
            pending_snapshot,
            _sandbox: Some(prepared.sandbox),
        })
    }

    fn execute_step(
        &self,
        step: TestStep,
        expectation: &StepExpectation,
        context: &ExecutionContext<'_>,
    ) -> Result<StepObservation> {
        let (engine, index, walk_options) = match self.prepare_step(step, context)? {
            PreparedStep::Ready {
                engine,
                index,
                walk_options,
            } => (engine, index, walk_options),
            PreparedStep::ConfigurationError(observation) => return Ok(observation),
        };
        let repository = context.repository;
        match step {
            TestStep::Check | TestStep::CheckChanged => {
                let report = engine.run(repository, &index)?;
                Ok(observe_check(report, repository))
            }
            TestStep::Fix | TestStep::FixUnsafe | TestStep::FixDryRun => {
                let threshold = if step == TestStep::FixUnsafe {
                    Applicability::Unsafe
                } else {
                    Applicability::Safe
                };
                if expectation.diff.is_some() && step == TestStep::FixDryRun {
                    let (report, staged) = engine.stage_fixes(repository, &index, threshold)?;
                    return Ok(observe_fix(
                        &report,
                        Some(render_fix_diff(repository, &staged)?),
                    ));
                }
                let before = expectation
                    .diff
                    .as_ref()
                    .map(|_| repository_files(repository))
                    .transpose()?;
                let report = engine.fix(
                    repository,
                    &index,
                    &walk_options,
                    step == TestStep::FixDryRun,
                    threshold,
                )?;
                let diff = before
                    .map(|before| {
                        let after = repository_files(repository)?;
                        render_fix_diff(repository, &staged_between(before, after))
                    })
                    .transpose()?;
                Ok(observe_fix(&report, diff))
            }
        }
    }

    fn prepare_step(&self, step: TestStep, context: &ExecutionContext<'_>) -> Result<PreparedStep> {
        let registry = self.registry.clone();
        let options = alint_dsl::LoadOptions {
            kind_catalog: Some(registry.catalog()),
            cache: Some(alint_dsl::extends::Cache::at(context.sandbox.join("cache"))),
            fetcher: None,
            environment: context.environment.clone(),
            config_source: context.source_provider.clone(),
            remote_policy: alint_dsl::RemotePolicy::Deny,
            repository_root: Some(context.repository.to_path_buf()),
        };
        let config = match alint_dsl::load_with(context.config_path, &options) {
            Ok(config) => config,
            Err(error) => {
                return Ok(prepared_configuration_error(&error.to_string(), context));
            }
        };
        if !config.allow_out_of_root.is_confined() {
            return Ok(prepared_configuration_error(
                "allow_out_of_root is disabled in public policy tests",
                context,
            ));
        }
        if let Err(error) = enforce_capabilities(
            context.test,
            &self.options,
            &config.rules,
            &config.facts,
            &registry,
        ) {
            return Err(match error {
                Error::Capability(message) => Error::capability(sanitize_text(
                    &message,
                    context.test,
                    context.repository,
                    context.package.path(),
                )),
                other => other,
            });
        }

        let mut entries = Vec::with_capacity(config.rules.len());
        for spec in &config.rules {
            if spec.level == Level::Off {
                continue;
            }
            let rule = match registry.build(spec) {
                Ok(rule) => rule,
                Err(error) => {
                    return Ok(prepared_configuration_error(&error.to_string(), context));
                }
            };
            if let Err(error) = rule.validate_nested(&registry) {
                let error = spec.redact_configuration_error(error);
                return Ok(prepared_configuration_error(&error.to_string(), context));
            }
            let mut entry = RuleEntry::new(rule).with_spec(Arc::new(spec.clone()));
            if let Some(source) = &spec.when {
                let expression = match alint_core::when::parse(source) {
                    Ok(expression) => expression,
                    Err(error) => {
                        let message = spec.redact_configuration_message(error);
                        return Ok(prepared_configuration_error(&message, context));
                    }
                };
                entry = entry.with_when(expression);
            }
            entries.push(entry);
        }
        let mut engine = Engine::from_entries(entries, registry)
            .with_facts(config.facts.clone())
            .with_vars(config.vars.clone())
            .with_fix_size_limit(config.fix_size_limit)
            .with_process_environment(context.process_environment.clone());
        if let Some(as_of) = &context.test.given.as_of {
            let analysis_time = as_of.parse().map_err(|error| {
                Error::runtime(format!(
                    "invalid validated analysis date {as_of:?}: {error}"
                ))
            })?;
            engine = engine.with_analysis_time(analysis_time);
        }
        let walk_options = WalkOptions {
            respect_gitignore: config.respect_gitignore,
            extra_ignores: config.ignore.clone(),
        };
        let index = walk(context.repository, &walk_options)?;
        if step == TestStep::CheckChanged {
            let changed = alint_core::git::collect_changed_paths(context.repository, None)
                .ok_or_else(|| {
                    Error::runtime(
                        "check_changed requires an initialized Git repository".to_string(),
                    )
                })?;
            engine = engine.with_changed_paths(changed);
        }
        Ok(PreparedStep::Ready {
            engine,
            index,
            walk_options,
        })
    }
}

fn prepared_configuration_error(message: &str, context: &ExecutionContext<'_>) -> PreparedStep {
    PreparedStep::ConfigurationError(StepObservation::configuration_error(sanitize_text(
        message,
        context.test,
        context.repository,
        context.package.path(),
    )))
}

/// One test execution result. `root` remains available until this value drops.
#[derive(Debug)]
pub struct PolicyTestRun {
    pub source: String,
    pub name: String,
    pub tags: Vec<String>,
    pub status: TestStatus,
    pub observations: Vec<StepObservation>,
    pub mismatches: Vec<Mismatch>,
    pub root: PathBuf,
    pending_snapshot: Option<PendingSnapshotUpdate>,
    _sandbox: Option<TempDir>,
}

impl PolicyTestRun {
    fn skipped(source: &Path, test: &PolicyTest, reason: String) -> Self {
        Self {
            source: normalized_path(source),
            name: test.name.clone(),
            tags: test.tags.clone(),
            status: TestStatus::Skipped,
            observations: Vec::new(),
            mismatches: vec![Mismatch {
                step: None,
                channel: MismatchChannel::Platform,
                detail: reason,
            }],
            root: PathBuf::new(),
            pending_snapshot: None,
            _sandbox: None,
        }
    }

    #[must_use]
    pub fn passed(&self) -> bool {
        self.status == TestStatus::Passed
    }

    /// Commit a snapshot update deferred by
    /// [`Runner::run_deferred_snapshot_update`].
    ///
    /// Returns `Ok(false)` when the test has no snapshot or its inline
    /// expectations failed, so no update was prepared.
    pub fn commit_snapshot_update(&mut self) -> Result<bool> {
        let Some(update) = self.pending_snapshot.as_ref() else {
            return Ok(false);
        };
        update.commit()?;
        self.pending_snapshot = None;
        Ok(true)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TestStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct StepObservation {
    pub exit: ExpectedExit,
    pub violations: Vec<ObservedViolation>,
    pub analysis_failures: Vec<ObservedAnalysisFailure>,
    pub notes: Vec<ObservedNote>,
    pub applied: Vec<String>,
    pub skipped: Vec<String>,
    pub suggested: Vec<String>,
    pub unfixable: Vec<String>,
    pub non_convergent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration_error: Option<String>,
}

impl StepObservation {
    fn configuration_error(message: String) -> Self {
        Self {
            exit: ExpectedExit::ConfigurationError,
            violations: Vec::new(),
            analysis_failures: Vec::new(),
            notes: Vec::new(),
            applied: Vec::new(),
            skipped: Vec::new(),
            suggested: Vec::new(),
            unfixable: Vec::new(),
            non_convergent: false,
            diff: None,
            configuration_error: Some(message),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ObservedViolation {
    pub rule: String,
    pub level: String,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub end_line: Option<usize>,
    pub end_column: Option<usize>,
    pub message: String,
    pub is_fixable: bool,
    pub baseline_key: Option<String>,
    pub fingerprint: String,
    pub related: Vec<ObservedRelatedLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ObservedAnalysisFailure {
    pub rule: String,
    pub kind: String,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub end_line: Option<usize>,
    pub end_column: Option<usize>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ObservedNote {
    pub kind: String,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub end_line: Option<usize>,
    pub end_column: Option<usize>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ObservedRelatedLocation {
    pub label: String,
    pub path: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub end_line: Option<usize>,
    pub end_column: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Mismatch {
    pub step: Option<usize>,
    pub channel: MismatchChannel,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MismatchChannel {
    Exit,
    Violations,
    AnalysisFailures,
    Notes,
    Applied,
    Skipped,
    Suggested,
    Unfixable,
    NonConvergent,
    Diff,
    ConfigurationError,
    Tree,
    Snapshot,
    Platform,
}

fn observe_check(report: Report, repository: &Path) -> StepObservation {
    let exit = if report.has_errors() {
        ExpectedExit::PolicyFailure
    } else {
        ExpectedExit::Success
    };
    let mut violations = Vec::new();
    let mut analysis_failures = Vec::new();
    let mut notes = Vec::new();
    let mut content_cache: BTreeMap<PathBuf, Option<Vec<u8>>> = BTreeMap::new();
    for result in report.results {
        for violation in result.violations {
            violations.push(observed_violation(
                &result.rule_id,
                result.level,
                &violation,
                repository,
                &mut content_cache,
            ));
        }
        for failure in result.analysis_failures {
            let path_sensitive = failure.path_is_sensitive();
            analysis_failures.push(ObservedAnalysisFailure {
                rule: result.rule_id.to_string(),
                kind: failure.kind.as_str().to_string(),
                path: failure.report_path().map(normalized_path),
                line: (!path_sensitive).then_some(failure.line).flatten(),
                column: (!path_sensitive).then_some(failure.column).flatten(),
                end_line: None,
                end_column: None,
                message: failure.message.into_owned(),
            });
        }
        for note in result.notes {
            let path_sensitive = note.path_is_sensitive();
            let range = (!path_sensitive).then_some(note.range.as_ref()).flatten();
            notes.push(ObservedNote {
                kind: note
                    .note_kind()
                    .map_or("informational", |kind| kind.as_str())
                    .to_string(),
                path: note.report_path().map(normalized_path),
                line: range.map_or((!path_sensitive).then_some(note.line).flatten(), |value| {
                    Some(value.start_line())
                }),
                column: range.map_or(
                    (!path_sensitive).then_some(note.column).flatten(),
                    |value| Some(value.start_column()),
                ),
                end_line: range.map(alint_core::SourceRange::end_line),
                end_column: range.map(alint_core::SourceRange::end_column),
                message: note.message.into_owned(),
            });
        }
    }
    sort_observations(&mut violations, &mut analysis_failures, &mut notes);
    StepObservation {
        exit,
        violations,
        analysis_failures,
        notes,
        applied: Vec::new(),
        skipped: Vec::new(),
        suggested: Vec::new(),
        unfixable: Vec::new(),
        non_convergent: false,
        diff: None,
        configuration_error: None,
    }
}

fn observe_fix(report: &FixReport, diff: Option<String>) -> StepObservation {
    let exit = if report.non_convergent {
        ExpectedExit::FixNonConvergent
    } else if report.had_fix_error() || report.has_unfixable_errors() {
        ExpectedExit::PolicyFailure
    } else {
        ExpectedExit::Success
    };
    let mut applied = Vec::new();
    let mut skipped = Vec::new();
    let mut suggested = Vec::new();
    let mut unfixable = Vec::new();
    let mut analysis_failures = Vec::new();
    for result in &report.results {
        if result
            .items
            .iter()
            .any(|item| matches!(item.status, FixStatus::Applied(_)))
        {
            applied.push(result.rule_id.to_string());
        }
        if result
            .items
            .iter()
            .any(|item| matches!(item.status, FixStatus::Skipped { .. }))
        {
            skipped.push(result.rule_id.to_string());
        }
        if result
            .items
            .iter()
            .any(|item| matches!(item.status, FixStatus::Suggested { .. }))
        {
            suggested.push(result.rule_id.to_string());
        }
        if result
            .items
            .iter()
            .any(|item| matches!(item.status, FixStatus::Unfixable))
        {
            unfixable.push(result.rule_id.to_string());
        }
        for failure in &result.analysis_failures {
            let path_sensitive = failure.path_is_sensitive();
            analysis_failures.push(ObservedAnalysisFailure {
                rule: result.rule_id.to_string(),
                kind: failure.kind.as_str().to_string(),
                path: failure.report_path().map(normalized_path),
                line: (!path_sensitive).then_some(failure.line).flatten(),
                column: (!path_sensitive).then_some(failure.column).flatten(),
                end_line: None,
                end_column: None,
                message: failure.message.to_string(),
            });
        }
    }
    for list in [&mut applied, &mut skipped, &mut suggested, &mut unfixable] {
        list.sort();
    }
    analysis_failures.sort_by(observed_failure_key);
    StepObservation {
        exit,
        violations: Vec::new(),
        analysis_failures,
        notes: Vec::new(),
        applied,
        skipped,
        suggested,
        unfixable,
        non_convergent: report.non_convergent,
        diff,
        configuration_error: None,
    }
}

fn observed_violation(
    rule: &str,
    level: Level,
    violation: &alint_core::Violation,
    repository: &Path,
    content_cache: &mut BTreeMap<PathBuf, Option<Vec<u8>>>,
) -> ObservedViolation {
    let path_sensitive = violation.path_is_sensitive();
    let range = (!path_sensitive)
        .then_some(violation.range.as_ref())
        .flatten();
    let mut related: Vec<_> = if path_sensitive {
        Vec::new()
    } else {
        violation
            .related_locations
            .iter()
            .map(|location| {
                let range = location.range.as_ref();
                ObservedRelatedLocation {
                    label: location.label.clone(),
                    path: normalized_path(&location.path),
                    line: range.map(alint_core::SourceRange::start_line),
                    column: range.map(alint_core::SourceRange::start_column),
                    end_line: range.map(alint_core::SourceRange::end_line),
                    end_column: range.map(alint_core::SourceRange::end_column),
                }
            })
            .collect()
    };
    related.sort_by(|left, right| {
        (&left.path, left.line, left.column, &left.label).cmp(&(
            &right.path,
            right.line,
            right.column,
            &right.label,
        ))
    });
    let contents = violation.path.as_ref().and_then(|path| {
        content_cache
            .entry(path.to_path_buf())
            .or_insert_with(|| std::fs::read(repository.join(path)).ok())
            .as_deref()
    });
    ObservedViolation {
        rule: rule.to_string(),
        level: level.as_str().to_string(),
        path: violation.report_path().map(normalized_path),
        line: range.map_or(
            (!path_sensitive).then_some(violation.line).flatten(),
            |value| Some(value.start_line()),
        ),
        column: range.map_or(
            (!path_sensitive).then_some(violation.column).flatten(),
            |value| Some(value.start_column()),
        ),
        end_line: range.map(alint_core::SourceRange::end_line),
        end_column: range.map(alint_core::SourceRange::end_column),
        is_fixable: violation.is_fixable,
        message: violation.message.to_string(),
        baseline_key: violation.baseline_key.as_deref().map(str::to_string),
        fingerprint: alint_core::baseline::fingerprint(rule, violation, contents),
        related,
    }
}

fn sort_observations(
    violations: &mut [ObservedViolation],
    failures: &mut [ObservedAnalysisFailure],
    notes: &mut [ObservedNote],
) {
    violations.sort_by(|left, right| {
        (
            &left.rule,
            &left.path,
            left.line,
            left.column,
            &left.message,
        )
            .cmp(&(
                &right.rule,
                &right.path,
                right.line,
                right.column,
                &right.message,
            ))
    });
    failures.sort_by(observed_failure_key);
    notes.sort_by(|left, right| {
        (
            &left.kind,
            &left.path,
            left.line,
            left.column,
            &left.message,
        )
            .cmp(&(
                &right.kind,
                &right.path,
                right.line,
                right.column,
                &right.message,
            ))
    });
}

fn observed_failure_key(
    left: &ObservedAnalysisFailure,
    right: &ObservedAnalysisFailure,
) -> std::cmp::Ordering {
    (
        &left.rule,
        &left.kind,
        &left.path,
        left.line,
        left.column,
        &left.message,
    )
        .cmp(&(
            &right.rule,
            &right.kind,
            &right.path,
            right.line,
            right.column,
            &right.message,
        ))
}

fn enforce_capabilities(
    test: &PolicyTest,
    options: &RunnerOptions,
    rules: &[RuleSpec],
    facts: &[alint_core::FactSpec],
    registry: &alint_core::RuleRegistry,
) -> Result<()> {
    let custom_fact = facts
        .iter()
        .find(|fact| matches!(fact.kind, FactKind::Custom { .. }))
        .map(|fact| format!("custom fact {:?}", fact.id));
    let mut spawn_use = custom_fact;
    let mut keyring_use = None;
    for rule in rules {
        inspect_rule_capabilities(rule, registry.catalog(), &mut spawn_use, &mut keyring_use);
    }
    if let Some(usage) = spawn_use {
        require_capability(
            "spawn",
            test.capabilities.spawn,
            options.allow_spawn,
            &usage,
        )?;
    }
    if let Some(usage) = keyring_use {
        require_capability(
            "host_keyring",
            test.capabilities.host_keyring,
            options.allow_host_keyring,
            &usage,
        )?;
    }
    Ok(())
}

fn inspect_rule_capabilities(
    rule: &RuleSpec,
    catalog: alint_core::KindCatalog<'_>,
    spawn_use: &mut Option<String>,
    keyring_use: &mut Option<String>,
) {
    if let Some(meta) = catalog.meta(&rule.kind) {
        for capability in meta.effective_capabilities() {
            match capability.capability {
                Capability::SpawnUserCommand if spawn_use.is_none() => {
                    *spawn_use = Some(format!("rule {:?} ({})", rule.id, rule.kind));
                }
                Capability::HostKeyring if keyring_use.is_none() => {
                    *keyring_use = Some(format!("rule {:?} ({})", rule.id, rule.kind));
                }
                _ => {}
            }
        }
    }
    if let Some(fix) = &rule.fix
        && catalog
            .fix_op(fix.op_name())
            .is_some_and(|meta| meta.trust == alint_core::FixTrustClass::Spawning)
        && spawn_use.is_none()
    {
        *spawn_use = Some(format!("rule {:?} fix.{}", rule.id, fix.op_name()));
    }
    inspect_nested_values(&rule.extra, catalog, spawn_use, keyring_use);
}

fn inspect_nested_values(
    mapping: &yaml_serde::Mapping,
    catalog: alint_core::KindCatalog<'_>,
    spawn_use: &mut Option<String>,
    keyring_use: &mut Option<String>,
) {
    for value in mapping.values() {
        match value {
            yaml_serde::Value::Mapping(nested) => {
                if nested.contains_key("kind")
                    && let Ok(rule) = yaml_serde::from_value::<RuleSpec>(
                        yaml_serde::Value::Mapping(nested.clone()),
                    )
                {
                    inspect_rule_capabilities(&rule, catalog, spawn_use, keyring_use);
                } else {
                    inspect_nested_values(nested, catalog, spawn_use, keyring_use);
                }
            }
            yaml_serde::Value::Sequence(values) => {
                for value in values {
                    if let yaml_serde::Value::Mapping(nested) = value {
                        if nested.contains_key("kind")
                            && let Ok(rule) = yaml_serde::from_value::<RuleSpec>(
                                yaml_serde::Value::Mapping(nested.clone()),
                            )
                        {
                            inspect_rule_capabilities(&rule, catalog, spawn_use, keyring_use);
                        } else {
                            inspect_nested_values(nested, catalog, spawn_use, keyring_use);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn require_capability(name: &str, declared: bool, granted: bool, usage: &str) -> Result<()> {
    if declared && granted {
        return Ok(());
    }
    Err(Error::capability(format!(
        "{usage} requires capability {name:?}; document declaration={declared}, host grant={granted}"
    )))
}

#[derive(Debug)]
struct FixedEnvironment(BTreeMap<String, String>);

impl alint_dsl::EnvironmentProvider for FixedEnvironment {
    fn get(&self, name: &str) -> Option<String> {
        self.0.get(name).cloned()
    }
}

#[derive(Debug)]
struct PackageConfigSource {
    root: PathBuf,
    inline: Option<(PathBuf, String)>,
    memo: Mutex<HashMap<PathBuf, String>>,
}

impl PackageConfigSource {
    fn new(root: &Path, inline: Option<(PathBuf, String)>) -> Self {
        Self {
            root: root.to_path_buf(),
            inline,
            memo: Mutex::new(HashMap::new()),
        }
    }

    fn confined_canonical(&self, path: &Path) -> std::io::Result<PathBuf> {
        if self
            .inline
            .as_ref()
            .is_some_and(|(inline, _)| path == inline)
        {
            return Ok(path.to_path_buf());
        }
        let canonical = path.canonicalize()?;
        if !canonical.starts_with(&self.root) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "config source escapes the policy-test package root",
            ));
        }
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "config source must be a regular non-symlink file",
            ));
        }
        Ok(canonical)
    }
}

impl alint_dsl::ConfigSourceProvider for PackageConfigSource {
    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        self.confined_canonical(path)
    }

    fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
        let canonical = self.confined_canonical(path)?;
        if let Some(cached) = self
            .memo
            .lock()
            .map_err(|_| std::io::Error::other("config source cache lock poisoned"))?
            .get(&canonical)
            .cloned()
        {
            return Ok(cached);
        }
        let contents = if let Some((inline, contents)) = &self.inline {
            if &canonical == inline {
                contents.clone()
            } else {
                std::fs::read_to_string(&canonical)?
            }
        } else {
            std::fs::read_to_string(&canonical)?
        };
        self.memo
            .lock()
            .map_err(|_| std::io::Error::other("config source cache lock poisoned"))?
            .insert(canonical, contents.clone());
        Ok(contents)
    }

    fn yaml_files(&self, dir: &Path) -> std::io::Result<Vec<PathBuf>> {
        let canonical_dir = match dir.canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        if !canonical_dir.starts_with(&self.root) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "drop-in directory escapes the policy-test package root",
            ));
        }
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(&canonical_dir)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.file_type().is_file()
                && matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("yml" | "yaml")
                )
            {
                paths.push(path);
            }
        }
        paths.sort();
        Ok(paths)
    }
}

fn config_source(
    test: &PolicyTest,
    package: &PackageRoot,
    source: &Path,
) -> Result<(PathBuf, Arc<dyn alint_dsl::ConfigSourceProvider>)> {
    if let Some(contents) = &test.given.config {
        let file_name = source
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("policy-test");
        let virtual_path = package
            .path()
            .join(format!(".{file_name}.inline.alint.yml"));
        let provider = PackageConfigSource::new(
            package.path(),
            Some((virtual_path.clone(), contents.clone())),
        );
        Ok((virtual_path, Arc::new(provider)))
    } else {
        let relative = test
            .given
            .config_file
            .as_deref()
            .ok_or_else(|| Error::runtime("validated test has no config source"))?;
        let path = package
            .resolve_existing(relative, PackageEntryKind::File)
            .map_err(Error::from)?;
        Ok((
            path,
            Arc::new(PackageConfigSource::new(package.path(), None)),
        ))
    }
}

fn materialize_repository(
    test: &PolicyTest,
    package: &PackageRoot,
    repository: &Path,
) -> Result<()> {
    if let Some(tree) = &test.given.tree {
        return materialize(tree, repository);
    }
    let relative = test
        .given
        .fixture
        .as_deref()
        .ok_or_else(|| Error::runtime("validated test has no repository source"))?;
    let fixture = package.validate_fixture(relative).map_err(Error::from)?;
    copy_fixture_dir(&fixture, &fixture, repository)
}

fn copy_fixture_dir(fixture_root: &Path, source: &Path, destination: &Path) -> Result<()> {
    for entry in std::fs::read_dir(source).map_err(|source_error| Error::Io {
        path: source.to_path_buf(),
        source: source_error,
    })? {
        let entry = entry.map_err(|source_error| Error::Io {
            path: source.to_path_buf(),
            source: source_error,
        })?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let metadata = std::fs::symlink_metadata(&from).map_err(|source_error| Error::Io {
            path: from.clone(),
            source: source_error,
        })?;
        if metadata.file_type().is_dir() {
            std::fs::create_dir(&to).map_err(|source_error| Error::Io {
                path: to.clone(),
                source: source_error,
            })?;
            copy_fixture_dir(fixture_root, &from, &to)?;
        } else if metadata.file_type().is_file() {
            let canonical = from.canonicalize().map_err(|source_error| Error::Io {
                path: from.clone(),
                source: source_error,
            })?;
            if !canonical.starts_with(fixture_root) {
                return Err(Error::from(ConfinementError::Escape {
                    path: normalized_path(&from),
                }));
            }
            std::fs::copy(&from, &to).map_err(|source_error| Error::Io {
                path: from.clone(),
                source: source_error,
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                let mode = if metadata.permissions().mode() & 0o111 == 0 {
                    0o644
                } else {
                    0o755
                };
                std::fs::set_permissions(&to, std::fs::Permissions::from_mode(mode)).map_err(
                    |source_error| Error::Io {
                        path: to.clone(),
                        source: source_error,
                    },
                )?;
            }
        } else if metadata.file_type().is_symlink() {
            let target = std::fs::read_link(&from).map_err(|source_error| Error::Io {
                path: from.clone(),
                source: source_error,
            })?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, &to).map_err(|source_error| Error::Io {
                path: to.clone(),
                source: source_error,
            })?;
            #[cfg(windows)]
            std::os::windows::fs::symlink_file(&target, &to).map_err(|source_error| Error::Io {
                path: to.clone(),
                source: source_error,
            })?;
        }
    }
    Ok(())
}

/// Deterministic, isolated Git setup used by the public runner and docs exporter.
pub fn setup_git(
    root: &Path,
    spec: &PolicyGit,
    environment: &BTreeMap<String, String>,
) -> Result<()> {
    if !spec.init {
        return Ok(());
    }
    git(root, &["init", "-q", "-b", "main"], &[], environment)?;
    if !spec.add.is_empty() {
        let mut args = vec!["add", "--"];
        args.extend(spec.add.iter().map(String::as_str));
        git(root, &args, &[], environment)?;
    }
    if !spec.add_force.is_empty() {
        let mut args = vec!["add", "-f", "--"];
        args.extend(spec.add_force.iter().map(String::as_str));
        git(root, &args, &[], environment)?;
    }
    if !spec.commits.is_empty() {
        for (index, commit) in spec.commits.iter().enumerate() {
            commit_one(root, commit, index, environment)?;
        }
    } else if spec.commit && (!spec.add.is_empty() || !spec.add_force.is_empty()) {
        commit_message(
            root,
            "policy-test commit",
            false,
            "2000-01-01T00:00:00Z",
            environment,
        )?;
    }
    Ok(())
}

fn commit_one(
    root: &Path,
    commit: &PolicyCommit,
    index: usize,
    environment: &BTreeMap<String, String>,
) -> Result<()> {
    let (message, add, date) = match commit {
        PolicyCommit::Subject(message) => (message.as_str(), &[][..], None),
        PolicyCommit::Detailed(commit) => (
            commit.message.as_str(),
            commit.add.as_slice(),
            commit.date.as_deref(),
        ),
    };
    if !add.is_empty() {
        let mut args = vec!["add", "--"];
        args.extend(add.iter().map(String::as_str));
        git(root, &args, &[], environment)?;
    }
    let default_date = format!("2000-01-01T00:{:02}:00Z", index % 60);
    commit_message(
        root,
        message,
        add.is_empty(),
        date.unwrap_or(&default_date),
        environment,
    )
}

fn commit_message(
    root: &Path,
    message: &str,
    allow_empty: bool,
    date: &str,
    environment: &BTreeMap<String, String>,
) -> Result<()> {
    let mut args = vec![
        "-c",
        "user.name=alint policy test",
        "-c",
        "user.email=policy-test@alint.test",
        "-c",
        "commit.gpgSign=false",
        "-c",
        "core.hooksPath=",
        "commit",
        "-q",
    ];
    if allow_empty {
        args.push("--allow-empty");
    }
    args.extend(["-m", message]);
    git(
        root,
        &args,
        &[("GIT_AUTHOR_DATE", date), ("GIT_COMMITTER_DATE", date)],
        environment,
    )
}

fn git(
    root: &Path,
    args: &[&str],
    extra_environment: &[(&str, &str)],
    environment: &BTreeMap<String, String>,
) -> Result<()> {
    let mut command = Command::new("git");
    command
        .env_clear()
        .envs(environment)
        .arg("--literal-pathspecs")
        .arg("--no-replace-objects")
        .arg("-C")
        .arg(root)
        .args(args);
    for (name, value) in extra_environment {
        command.env(name, value);
    }
    let output = command.output().map_err(|source_error| Error::Io {
        path: root.to_path_buf(),
        source: source_error,
    })?;
    if !output.status.success() {
        return Err(Error::git_fixture(format!(
            "isolated git command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

fn deterministic_process_environment(
    test: &PolicyTest,
    sandbox: &Path,
    host_keyring: bool,
) -> Result<BTreeMap<String, String>> {
    let home = sandbox.join("home");
    std::fs::create_dir(&home).map_err(|source_error| Error::Io {
        path: home.clone(),
        source: source_error,
    })?;
    let mut environment = test.given.env.clone();
    if let Some(path) = std::env::var_os("PATH") {
        environment.insert("PATH".to_string(), path.to_string_lossy().into_owned());
    }
    if host_keyring {
        for name in ["HOME", "GNUPGHOME"] {
            if let Ok(value) = std::env::var(name) {
                environment.insert(name.to_string(), value);
            }
        }
    } else {
        environment.insert("HOME".to_string(), home.to_string_lossy().into_owned());
        environment.insert(
            "GNUPGHOME".to_string(),
            home.join("gnupg").to_string_lossy().into_owned(),
        );
    }
    environment.insert("GIT_CONFIG_NOSYSTEM".to_string(), "1".to_string());
    environment.insert(
        "GIT_CONFIG_GLOBAL".to_string(),
        if cfg!(windows) { "NUL" } else { "/dev/null" }.to_string(),
    );
    environment.insert("GIT_TERMINAL_PROMPT".to_string(), "0".to_string());
    environment.insert("GIT_OPTIONAL_LOCKS".to_string(), "0".to_string());
    environment.insert("LC_ALL".to_string(), "C".to_string());
    #[cfg(windows)]
    for name in ["SystemRoot", "WINDIR", "COMSPEC", "PATHEXT", "TEMP", "TMP"] {
        if let Ok(value) = std::env::var(name) {
            environment.entry(name.to_string()).or_insert(value);
        }
    }
    Ok(environment)
}

fn sanitize_observation(observation: &mut StepObservation, context: &ExecutionContext<'_>) {
    for violation in &mut observation.violations {
        let mut sensitive = sanitize_field(
            &mut violation.message,
            context.test,
            context.repository,
            context.package.path(),
        );
        if let Some(path) = &mut violation.path {
            sensitive |= sanitize_field(
                path,
                context.test,
                context.repository,
                context.package.path(),
            );
        }
        if let Some(key) = &mut violation.baseline_key {
            sensitive |= sanitize_field(
                key,
                context.test,
                context.repository,
                context.package.path(),
            );
        }
        for related in &mut violation.related {
            sensitive |= sanitize_field(
                &mut related.label,
                context.test,
                context.repository,
                context.package.path(),
            );
            sensitive |= sanitize_field(
                &mut related.path,
                context.test,
                context.repository,
                context.package.path(),
            );
        }
        if sensitive {
            let locator = format!(
                "policy-test\0observation\0{}\0{}",
                violation.rule,
                violation.path.as_deref().unwrap_or("")
            );
            violation.baseline_key = None;
            violation.fingerprint = alint_core::redaction_marker(&locator);
        }
    }
    for failure in &mut observation.analysis_failures {
        sanitize_field(
            &mut failure.message,
            context.test,
            context.repository,
            context.package.path(),
        );
        if let Some(path) = &mut failure.path {
            sanitize_field(
                path,
                context.test,
                context.repository,
                context.package.path(),
            );
        }
    }
    for note in &mut observation.notes {
        sanitize_field(
            &mut note.message,
            context.test,
            context.repository,
            context.package.path(),
        );
        if let Some(path) = &mut note.path {
            sanitize_field(
                path,
                context.test,
                context.repository,
                context.package.path(),
            );
        }
    }
    if let Some(diff) = &mut observation.diff {
        sanitize_field(
            diff,
            context.test,
            context.repository,
            context.package.path(),
        );
    }
    if let Some(error) = &mut observation.configuration_error {
        sanitize_field(
            error,
            context.test,
            context.repository,
            context.package.path(),
        );
    }
}

fn sanitize_field(
    value: &mut String,
    test: &PolicyTest,
    repository: &Path,
    package: &Path,
) -> bool {
    let sanitized = sanitize_text(value, test, repository, package);
    if sanitized == *value {
        false
    } else {
        *value = sanitized;
        true
    }
}

fn sanitize_text(message: &str, test: &PolicyTest, repository: &Path, package: &Path) -> String {
    let repository = repository.to_string_lossy();
    let package = package.to_string_lossy();
    let evidence = test
        .given
        .env
        .values()
        .map(String::as_str)
        .chain([repository.as_ref(), package.as_ref()]);
    alint_core::redact_evidence(message, "policy-test\0diagnostic", evidence)
}

fn runner_owned_tree_path(discrepancy: &Discrepancy) -> bool {
    let path = match discrepancy {
        Discrepancy::Missing { path }
        | Discrepancy::Extra { path }
        | Discrepancy::Kind { path, .. }
        | Discrepancy::Content { path, .. } => path,
    };
    path == ".git" || path.starts_with(".git/")
}

fn safe_tree_mismatch(discrepancy: &Discrepancy) -> String {
    match discrepancy {
        Discrepancy::Missing { path } => format!("missing path {path}"),
        Discrepancy::Extra { path } => format!("unexpected path {path}"),
        Discrepancy::Kind {
            path,
            expected,
            actual,
        } => {
            format!("kind mismatch at {path}: expected {expected}, got {actual}")
        }
        Discrepancy::Content { path, .. } => format!("content mismatch at {path}"),
    }
}

fn platform_skip_reason(tags: &[String]) -> Option<String> {
    if tags.iter().any(|tag| tag == "linux-only") && !cfg!(target_os = "linux") {
        return Some("requires Linux".to_string());
    }
    if tags.iter().any(|tag| tag == "unix-only") && !cfg!(unix) {
        return Some("requires Unix".to_string());
    }
    if tags.iter().any(|tag| tag == "requires-case-sensitive-fs")
        && cfg!(any(target_os = "macos", target_os = "windows"))
    {
        return Some("requires a case-sensitive filesystem".to_string());
    }
    None
}

fn normalized_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
#[path = "policy_runner_tests.rs"]
mod tests;
