//! Versioned public policy-test document model.
//!
//! The serialized vocabulary is deliberately independent from the engine's
//! internal Rust types so the public document contract can evolve only through
//! explicit `test_version` changes.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::treespec::TreeSpec;

/// The only policy-test document version supported by this build.
pub const POLICY_TEST_VERSION: u32 = 1;
/// Maximum regex source length accepted in a matcher.
pub const MAX_MATCHER_REGEX_BYTES: usize = 8 * 1024;

/// A complete `test_version: 1` policy-test document.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyTest {
    /// Policy-test schema discriminator. Must be exactly `1`.
    pub test_version: u32,
    /// Human-readable policy-test name.
    #[schemars(length(min = 1))]
    pub name: String,
    /// Selection and explicit platform tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Host capabilities requested by this document. Declarations are not grants.
    #[serde(default)]
    pub capabilities: TestCapabilities,
    /// Repository, config, environment, time, and Git inputs.
    pub given: GivenV1,
    /// Ordered production operations.
    #[schemars(length(min = 1))]
    pub when: Vec<TestStep>,
    /// One expectation for each operation in `when`.
    #[schemars(length(min = 1))]
    pub expect: Vec<StepExpectation>,
    /// Optional final repository tree assertion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_tree: Option<TreeSpec>,
    /// Final-tree assertion mode.
    #[serde(default)]
    pub expect_tree_mode: TreeExpectationMode,
    /// Optional package-relative JSON snapshot path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    /// Optional executable documentation metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<PolicyDocsExample>,
}

/// Closed capability declaration. The CLI must grant each requested capability separately.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TestCapabilities {
    /// Permit rules/fixers that can execute native processes.
    #[serde(default)]
    pub spawn: bool,
    /// Permit rules that read the host keyring.
    #[serde(default)]
    pub host_keyring: bool,
}

/// Inputs owned by one policy test.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GivenV1 {
    /// Inline repository tree. Mutually exclusive with `fixture`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<TreeSpec>,
    /// Package-relative repository fixture directory. Mutually exclusive with `tree`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixture: Option<String>,
    /// Inline `.alint.yml` body. Mutually exclusive with `config_file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    /// Package-relative config path. Mutually exclusive with `config`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_file: Option<String>,
    /// Deterministic environment visible to interpolation and explicitly granted commands.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Explicit analysis date in `YYYY-MM-DD` form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of: Option<String>,
    /// Optional isolated Git history setup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<PolicyGit>,
}

/// Git setup for a public policy test.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyGit {
    /// Initialize a repository before running steps.
    #[serde(default = "default_true")]
    pub init: bool,
    /// Paths staged without force.
    #[serde(default)]
    pub add: Vec<String>,
    /// Paths staged with force.
    #[serde(default)]
    pub add_force: Vec<String>,
    /// Commit the staged paths in the single-commit form.
    #[serde(default = "default_true")]
    pub commit: bool,
    /// Ordered commit chain, mutually exclusive with `add` and `add_force`.
    #[serde(default)]
    pub commits: Vec<PolicyCommit>,
}

impl Default for PolicyGit {
    fn default() -> Self {
        Self {
            init: true,
            add: Vec::new(),
            add_force: Vec::new(),
            commit: true,
            commits: Vec::new(),
        }
    }
}

/// One commit in `given.git.commits`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum PolicyCommit {
    /// Empty commit with this subject.
    Subject(String),
    /// Commit with explicit staged paths and optional date.
    Detailed(PolicyDetailedCommit),
}

/// Detailed commit fixture.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyDetailedCommit {
    /// Commit message.
    pub message: String,
    /// Paths staged before the commit.
    #[serde(default)]
    pub add: Vec<String>,
    /// Git-compatible deterministic author/committer date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
}

/// Operation executed by one test step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TestStep {
    Check,
    Fix,
    FixUnsafe,
    FixDryRun,
    CheckChanged,
}

/// Final-tree comparison mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TreeExpectationMode {
    #[default]
    Strict,
    Contains,
}

/// Expected command-level outcome for a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExpectedExit {
    Success,
    PolicyFailure,
    FixNonConvergent,
    ConfigurationError,
}

/// Exact assertions for one step. Omitted result channels are deliberately unasserted.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepExpectation {
    /// Required command-level outcome.
    pub exit: ExpectedExit,
    /// Exact policy-finding channel when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub violations: Option<Vec<ViolationMatcher>>,
    /// Exact analysis-failure channel when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_failures: Option<Vec<AnalysisFailureMatcher>>,
    /// Exact note channel when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<Vec<NoteMatcher>>,
    /// Rule ids with an applied fix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied: Option<Vec<String>>,
    /// Rule ids with a skipped fix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped: Option<Vec<String>>,
    /// Rule ids with a suggested fix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested: Option<Vec<String>>,
    /// Rule ids with an unfixable result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unfixable: Option<Vec<String>>,
    /// Exact non-convergence flag when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub non_convergent: Option<bool>,
    /// Exact canonical unified diff when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    /// Required diagnostic assertion for `configuration_error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration_error: Option<ConfigurationErrorMatcher>,
}

/// Matcher for a configuration error raised before evaluation.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationErrorMatcher {
    /// Redacted diagnostic text matcher.
    pub message: MessageMatcher,
}

/// Policy-finding matcher.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViolationMatcher {
    #[schemars(length(min = 1))]
    pub rule: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<LevelMatcher>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub column: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub end_line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub end_column: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<MessageMatcher>,
    #[serde(default = "default_count")]
    #[schemars(range(min = 1))]
    pub count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related: Option<Vec<RelatedLocationMatcher>>,
}

/// Typed analysis-failure matcher.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnalysisFailureMatcher {
    #[schemars(length(min = 1))]
    pub rule: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<AnalysisFailureKindMatcher>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub column: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub end_line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub end_column: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<MessageMatcher>,
    #[serde(default = "default_count")]
    #[schemars(range(min = 1))]
    pub count: usize,
}

/// Informational-note matcher.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoteMatcher {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<MessageMatcher>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub column: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub end_line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub end_column: Option<usize>,
    #[serde(default = "default_count")]
    #[schemars(range(min = 1))]
    pub count: usize,
}

/// Related-location matcher. A present related list is exact after canonical ordering.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RelatedLocationMatcher {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[schemars(length(min = 1))]
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub column: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub end_line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub end_column: Option<usize>,
}

/// Exact or regex text matcher. Runtime validation requires exactly one arm.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MessageMatcher {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regex: Option<String>,
}

/// Finding severity matcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LevelMatcher {
    Error,
    Warning,
    Info,
    Off,
}

/// Stable analysis-failure class matcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisFailureKindMatcher {
    Evaluation,
    Condition,
    ResourceRead,
    ResourceParse,
    NestedRuleBuild,
}

/// Metadata consumed by the executable documentation exporter.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyDocsExample {
    #[schemars(length(min = 1))]
    pub title: String,
    pub case: PolicyDocsCase,
    #[schemars(length(min = 1))]
    pub kind: String,
    #[serde(default)]
    pub order: i32,
}

/// Whether an example demonstrates a finding or a compliant repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDocsCase {
    Fail,
    Pass,
}

/// Stable machine-readable validation category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub enum ValidationCode {
    UnsupportedVersion,
    EmptyName,
    RepositorySource,
    ConfigSource,
    InvalidPackagePath,
    InvalidTree,
    InvalidEnvironment,
    InvalidAnalysisTime,
    StepExpectationCount,
    MissingAssertion,
    InvalidStepExpectation,
    InvalidMessageMatcher,
    InvalidRegex,
    InvalidCount,
    InvalidRange,
    InvalidGitSetup,
    InvalidTag,
    InvalidDocsMetadata,
    InvalidMatcher,
}

impl ValidationCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedVersion => "unsupported_version",
            Self::EmptyName => "empty_name",
            Self::RepositorySource => "repository_source",
            Self::ConfigSource => "config_source",
            Self::InvalidPackagePath => "invalid_package_path",
            Self::InvalidTree => "invalid_tree",
            Self::InvalidEnvironment => "invalid_environment",
            Self::InvalidAnalysisTime => "invalid_analysis_time",
            Self::StepExpectationCount => "step_expectation_count",
            Self::MissingAssertion => "missing_assertion",
            Self::InvalidStepExpectation => "invalid_step_expectation",
            Self::InvalidMessageMatcher => "invalid_message_matcher",
            Self::InvalidRegex => "invalid_regex",
            Self::InvalidCount => "invalid_count",
            Self::InvalidRange => "invalid_range",
            Self::InvalidGitSetup => "invalid_git_setup",
            Self::InvalidTag => "invalid_tag",
            Self::InvalidDocsMetadata => "invalid_docs_metadata",
            Self::InvalidMatcher => "invalid_matcher",
        }
    }
}

impl Serialize for ValidationCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

/// Structured policy-test definition diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidationDiagnostic {
    pub code: ValidationCode,
    /// Stable dotted/indexed location in the policy-test document.
    pub field: String,
    pub message: String,
}

impl fmt::Display for ValidationDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.field, self.message)
    }
}

/// Guarded policy-test parse failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PolicyTestParseError {
    #[error("policy-test YAML flow nesting exceeds the maximum supported depth ({0})")]
    FlowDepth(usize),
    #[error("policy-test YAML alias expansion exceeds the maximum supported size")]
    AliasExpansion,
    #[error("policy-test YAML contains an unsupported explicit tag")]
    ExplicitTag,
    #[error("policy-test YAML uses explicit `null`; omit optional fields instead")]
    ExplicitNull,
    #[error("policy-test YAML parse error: {0}")]
    Yaml(#[from] ayaml::Error),
    #[error("policy-test document must be a YAML mapping")]
    NotMapping,
    #[error("policy-test document must declare integer `test_version: 1`")]
    MissingVersion,
    #[error("unsupported policy-test version {found}; this build supports version {supported}")]
    UnsupportedVersion { found: u64, supported: u32 },
}

/// Return whether recursively discovered YAML declares a top-level policy-test
/// version discriminator.
///
/// Discovery uses this bounded probe to distinguish scenario documents from
/// package-local YAML fixtures and config chains. A positive result still goes
/// through [`PolicyTest::from_yaml`], which reports malformed or unsupported
/// documents rather than silently skipping them.
#[must_use]
pub fn has_policy_test_discriminator(input: &str) -> bool {
    let has_top_level_marker = input.lines().any(|line| {
        if line.starts_with([' ', '\t']) {
            return false;
        }
        let line = line.strip_prefix('\u{feff}').unwrap_or(line);
        line.split_once(':').is_some_and(|(key, _)| {
            matches!(
                key.trim(),
                "test_version" | "'test_version'" | "\"test_version\""
            )
        })
    });
    if has_top_level_marker {
        return true;
    }
    let yaml_guards = alint_core::yaml_depth::scan_guards(input);
    if !yaml_guards.flow_within_limit() || !yaml_guards.expansion_within_limit(input) {
        return false;
    }
    ayaml::from_str::<ayaml::Value>(input).is_ok_and(|value| {
        value
            .as_mapping()
            .is_some_and(|mapping| mapping.contains_key("test_version"))
    })
}

impl PolicyTest {
    /// Parse one v1 document after shared YAML complexity guards and version dispatch.
    pub fn from_yaml(input: &str) -> Result<Self, PolicyTestParseError> {
        let yaml_guards = alint_core::yaml_depth::scan_guards(input);
        if !yaml_guards.flow_within_limit() {
            return Err(PolicyTestParseError::FlowDepth(
                alint_core::yaml_depth::MAX_YAML_FLOW_DEPTH,
            ));
        }
        if !yaml_guards.expansion_within_limit(input) {
            return Err(PolicyTestParseError::AliasExpansion);
        }
        let value: ayaml::Value = ayaml::from_str(input)?;
        if contains_tag(&value) {
            return Err(PolicyTestParseError::ExplicitTag);
        }
        if contains_null(&value) {
            return Err(PolicyTestParseError::ExplicitNull);
        }
        let mapping = value.as_mapping().ok_or(PolicyTestParseError::NotMapping)?;
        let version = mapping
            .get("test_version")
            .and_then(ayaml::Value::as_u64)
            .ok_or(PolicyTestParseError::MissingVersion)?;
        if version != u64::from(POLICY_TEST_VERSION) {
            return Err(PolicyTestParseError::UnsupportedVersion {
                found: version,
                supported: POLICY_TEST_VERSION,
            });
        }
        ayaml::from_value(value).map_err(PolicyTestParseError::Yaml)
    }

    /// Return every definition error in deterministic document order.
    #[must_use]
    pub fn validation_diagnostics(&self) -> Vec<ValidationDiagnostic> {
        let mut out = Vec::new();
        validate_policy_identity(self, &mut out);
        validate_policy_sources(self, &mut out);
        validate_policy_cross_fields(self, &mut out);
        validate_policy_git(self.given.git.as_ref(), &mut out);
        validate_policy_docs(self.docs.as_ref(), &mut out);
        for (index, expectation) in self.expect.iter().enumerate() {
            validate_expectation(self, index, expectation, &mut out);
        }
        out
    }

    /// Validate this document, returning all diagnostics on failure.
    pub fn validate(&self) -> Result<(), Vec<ValidationDiagnostic>> {
        let diagnostics = self.validation_diagnostics();
        if diagnostics.is_empty() {
            Ok(())
        } else {
            Err(diagnostics)
        }
    }
}

fn validate_policy_identity(test: &PolicyTest, out: &mut Vec<ValidationDiagnostic>) {
    if test.test_version != POLICY_TEST_VERSION {
        diagnostic(
            out,
            ValidationCode::UnsupportedVersion,
            "test_version",
            format!("must be {POLICY_TEST_VERSION}; found {}", test.test_version),
        );
    }
    if test.name.trim().is_empty() {
        diagnostic(out, ValidationCode::EmptyName, "name", "must not be empty");
    }
    let mut tags = HashSet::new();
    for (index, tag) in test.tags.iter().enumerate() {
        if tag.trim().is_empty() {
            diagnostic(
                out,
                ValidationCode::InvalidTag,
                format!("tags[{index}]"),
                "must not be empty",
            );
        } else if !tags.insert(tag) {
            diagnostic(
                out,
                ValidationCode::InvalidTag,
                format!("tags[{index}]"),
                format!("duplicates earlier tag {tag:?}"),
            );
        }
    }
}

fn validate_policy_sources(test: &PolicyTest, out: &mut Vec<ValidationDiagnostic>) {
    if usize::from(test.given.tree.is_some()) + usize::from(test.given.fixture.is_some()) != 1 {
        diagnostic(
            out,
            ValidationCode::RepositorySource,
            "given",
            "must declare exactly one of `tree` or `fixture`",
        );
    }
    if usize::from(test.given.config.is_some()) + usize::from(test.given.config_file.is_some()) != 1
    {
        diagnostic(
            out,
            ValidationCode::ConfigSource,
            "given",
            "must declare exactly one of `config` or `config_file`",
        );
    }
    for (field, tree, validate_symlinks) in [
        ("given.tree", test.given.tree.as_ref(), true),
        ("expect_tree", test.expect_tree.as_ref(), false),
    ] {
        if let Some(tree) = tree {
            for issue in crate::confinement::validate_tree_spec(tree, validate_symlinks) {
                diagnostic(
                    out,
                    ValidationCode::InvalidTree,
                    format!("{field}[{:?}]", issue.path),
                    issue.reason,
                );
            }
        }
    }
    for (field, path) in [
        ("given.fixture", test.given.fixture.as_deref()),
        ("given.config_file", test.given.config_file.as_deref()),
        ("snapshot", test.snapshot.as_deref()),
    ] {
        if let Some(path) = path
            && let Err(message) = crate::confinement::validate_package_path(path)
        {
            diagnostic(out, ValidationCode::InvalidPackagePath, field, message);
        }
    }
    for (name, value) in &test.given.env {
        if name.is_empty() || name.contains(['=', '\0']) {
            diagnostic(
                out,
                ValidationCode::InvalidEnvironment,
                format!("given.env[{name:?}]"),
                "environment names must be non-empty and contain neither `=` nor NUL",
            );
        }
        if value.contains('\0') {
            diagnostic(
                out,
                ValidationCode::InvalidEnvironment,
                format!("given.env[{name:?}]"),
                "environment values must not contain NUL",
            );
        }
    }
}

fn validate_policy_cross_fields(test: &PolicyTest, out: &mut Vec<ValidationDiagnostic>) {
    if let Some(snapshot) = &test.snapshot
        && Path::new(snapshot)
            .extension()
            .and_then(|value| value.to_str())
            != Some("json")
    {
        diagnostic(
            out,
            ValidationCode::InvalidPackagePath,
            "snapshot",
            "must use a `.json` filename",
        );
    }
    if test.expect_tree.is_none() && test.expect_tree_mode != TreeExpectationMode::Strict {
        diagnostic(
            out,
            ValidationCode::InvalidStepExpectation,
            "expect_tree_mode",
            "is meaningful only when `expect_tree` is present",
        );
    }
    if let Some(value) = &test.given.as_of
        && value.parse::<alint_core::AnalysisTime>().is_err()
    {
        diagnostic(
            out,
            ValidationCode::InvalidAnalysisTime,
            "given.as_of",
            "must be a valid UTC date in YYYY-MM-DD form",
        );
    }
    if test.when.is_empty() || test.when.len() != test.expect.len() {
        diagnostic(
            out,
            ValidationCode::StepExpectationCount,
            "expect",
            format!(
                "must contain one entry for each non-empty `when` step ({} step(s), {} expectation(s))",
                test.when.len(),
                test.expect.len()
            ),
        );
    }
}

fn validate_policy_git(git: Option<&PolicyGit>, out: &mut Vec<ValidationDiagnostic>) {
    let Some(git) = git else {
        return;
    };
    if !git.commits.is_empty() && (!git.add.is_empty() || !git.add_force.is_empty()) {
        diagnostic(
            out,
            ValidationCode::InvalidGitSetup,
            "given.git",
            "cannot combine `commits` with `add` or `add_force`",
        );
    }
    if !git.init && (!git.add.is_empty() || !git.add_force.is_empty() || !git.commits.is_empty()) {
        diagnostic(
            out,
            ValidationCode::InvalidGitSetup,
            "given.git",
            "cannot stage or commit paths when `init` is false",
        );
    }
    for (field, paths) in [
        ("given.git.add", git.add.as_slice()),
        ("given.git.add_force", git.add_force.as_slice()),
    ] {
        validate_repository_paths(field, paths, out);
    }
    for (index, commit) in git.commits.iter().enumerate() {
        match commit {
            PolicyCommit::Subject(message) => {
                if message.is_empty() || message.contains('\0') {
                    diagnostic(
                        out,
                        ValidationCode::InvalidGitSetup,
                        format!("given.git.commits[{index}]"),
                        "must be non-empty and must not contain NUL",
                    );
                }
            }
            PolicyCommit::Detailed(commit) => {
                validate_repository_paths(
                    &format!("given.git.commits[{index}].add"),
                    &commit.add,
                    out,
                );
                if commit.message.is_empty() || commit.message.contains('\0') {
                    diagnostic(
                        out,
                        ValidationCode::InvalidGitSetup,
                        format!("given.git.commits[{index}].message"),
                        "must be non-empty and must not contain NUL",
                    );
                }
                if commit
                    .date
                    .as_ref()
                    .is_some_and(|date| date.is_empty() || date.contains('\0'))
                {
                    diagnostic(
                        out,
                        ValidationCode::InvalidGitSetup,
                        format!("given.git.commits[{index}].date"),
                        "must be non-empty and must not contain NUL",
                    );
                }
            }
        }
    }
}

fn validate_policy_docs(docs: Option<&PolicyDocsExample>, out: &mut Vec<ValidationDiagnostic>) {
    let Some(docs) = docs else {
        return;
    };
    for (field, value) in [
        ("docs.title", docs.title.as_str()),
        ("docs.kind", docs.kind.as_str()),
    ] {
        if value.trim().is_empty() {
            diagnostic(
                out,
                ValidationCode::InvalidDocsMetadata,
                field,
                "must not be empty",
            );
        }
    }
}

fn validate_expectation(
    test: &PolicyTest,
    index: usize,
    expectation: &StepExpectation,
    out: &mut Vec<ValidationDiagnostic>,
) {
    let field = format!("expect[{index}]");
    validate_step_expectation_contract(test, index, expectation, &field, out);
    validate_fix_expectation_contract(test, index, expectation, &field, out);
    validate_expectation_rule_ids(expectation, &field, out);
    validate_configuration_error_matcher(expectation, &field, out);
    validate_violation_matchers(expectation.violations.as_deref(), &field, out);
    validate_analysis_failure_matchers(expectation.analysis_failures.as_deref(), &field, out);
    validate_note_matchers(expectation.notes.as_deref(), &field, out);
}

fn validate_step_expectation_contract(
    test: &PolicyTest,
    index: usize,
    expectation: &StepExpectation,
    field: &str,
    out: &mut Vec<ValidationDiagnostic>,
) {
    let asserted = expectation.violations.is_some()
        || expectation.analysis_failures.is_some()
        || expectation.notes.is_some()
        || expectation.applied.is_some()
        || expectation.skipped.is_some()
        || expectation.suggested.is_some()
        || expectation.unfixable.is_some()
        || expectation.non_convergent.is_some()
        || expectation.diff.is_some()
        || expectation.configuration_error.is_some();
    if !asserted {
        diagnostic(
            out,
            ValidationCode::MissingAssertion,
            field,
            "must assert at least one observation beyond `exit`",
        );
    }
    let step = test.when.get(index);
    if matches!(step, Some(TestStep::CheckChanged))
        && test.given.git.as_ref().is_none_or(|git| !git.init)
    {
        diagnostic(
            out,
            ValidationCode::InvalidStepExpectation,
            field,
            "`check_changed` requires `given.git.init: true`",
        );
    }
    let is_fix_step = matches!(
        step,
        Some(TestStep::Fix | TestStep::FixUnsafe | TestStep::FixDryRun)
    );
    if !is_fix_step && has_fix_channels(expectation) {
        diagnostic(
            out,
            ValidationCode::InvalidStepExpectation,
            field,
            "fix-result assertions are valid only for fix steps",
        );
    }
    let config_error = expectation.exit == ExpectedExit::ConfigurationError;
    if config_error {
        if test.when.len() != 1 || expectation.configuration_error.is_none() {
            diagnostic(
                out,
                ValidationCode::InvalidStepExpectation,
                field,
                "`configuration_error` requires a one-step policy test and a message matcher",
            );
        }
        if has_evaluation_channels(expectation) {
            diagnostic(
                out,
                ValidationCode::InvalidStepExpectation,
                field,
                "a configuration error cannot assert report or fix channels",
            );
        }
    } else if expectation.configuration_error.is_some() {
        diagnostic(
            out,
            ValidationCode::InvalidStepExpectation,
            field,
            "`configuration_error` is valid only with `exit: configuration_error`",
        );
    }
}

fn validate_fix_expectation_contract(
    test: &PolicyTest,
    index: usize,
    expectation: &StepExpectation,
    field: &str,
    out: &mut Vec<ValidationDiagnostic>,
) {
    if expectation.exit == ExpectedExit::FixNonConvergent
        && !matches!(
            test.when.get(index),
            Some(TestStep::Fix | TestStep::FixUnsafe)
        )
    {
        diagnostic(
            out,
            ValidationCode::InvalidStepExpectation,
            field,
            "`fix_non_convergent` is valid only for a mutating fix step",
        );
    }
    if expectation.exit == ExpectedExit::FixNonConvergent
        && expectation.non_convergent != Some(true)
    {
        diagnostic(
            out,
            ValidationCode::InvalidStepExpectation,
            field,
            "`fix_non_convergent` must assert `non_convergent: true`",
        );
    } else if expectation.exit != ExpectedExit::FixNonConvergent
        && expectation.non_convergent == Some(true)
    {
        diagnostic(
            out,
            ValidationCode::InvalidStepExpectation,
            field,
            "`non_convergent: true` requires `exit: fix_non_convergent`",
        );
    }
    if matches!(
        test.when.get(index),
        Some(TestStep::Fix | TestStep::FixUnsafe | TestStep::FixDryRun)
    ) && expectation.applied.is_none()
        && expectation.skipped.is_none()
        && expectation.suggested.is_none()
        && expectation.unfixable.is_none()
        && expectation.non_convergent.is_none()
        && expectation.diff.is_none()
        && test.expect_tree.is_none()
    {
        diagnostic(
            out,
            ValidationCode::MissingAssertion,
            field,
            "a fix step must assert a fix outcome, diff, or final tree",
        );
    }
}

fn validate_expectation_rule_ids(
    expectation: &StepExpectation,
    field: &str,
    out: &mut Vec<ValidationDiagnostic>,
) {
    for (channel, ids) in [
        ("applied", expectation.applied.as_deref()),
        ("skipped", expectation.skipped.as_deref()),
        ("suggested", expectation.suggested.as_deref()),
        ("unfixable", expectation.unfixable.as_deref()),
    ] {
        if let Some(ids) = ids {
            validate_rule_ids(&format!("{field}.{channel}"), ids, out);
        }
    }
}

fn validate_configuration_error_matcher(
    expectation: &StepExpectation,
    field: &str,
    out: &mut Vec<ValidationDiagnostic>,
) {
    if let Some(matcher) = expectation
        .configuration_error
        .as_ref()
        .map(|error| &error.message)
    {
        validate_message(
            matcher,
            &format!("{field}.configuration_error.message"),
            out,
        );
    }
}

fn validate_violation_matchers(
    values: Option<&[ViolationMatcher]>,
    field: &str,
    out: &mut Vec<ValidationDiagnostic>,
) {
    let Some(values) = values else {
        return;
    };
    for (index, matcher) in values.iter().enumerate() {
        let matcher_field = format!("{field}.violations[{index}]");
        if matcher.rule.trim().is_empty() {
            diagnostic(
                out,
                ValidationCode::InvalidMatcher,
                format!("{matcher_field}.rule"),
                "must not be empty",
            );
        }
        validate_optional_matcher_path(
            matcher.path.as_deref(),
            &format!("{matcher_field}.path"),
            out,
        );
        validate_count_and_message(matcher.count, matcher.message.as_ref(), &matcher_field, out);
        validate_range(
            matcher.line,
            matcher.column,
            matcher.end_line,
            matcher.end_column,
            &matcher_field,
            out,
        );
        if let Some(related) = &matcher.related {
            for (related_index, location) in related.iter().enumerate() {
                let related_field = format!("{matcher_field}.related[{related_index}]");
                validate_optional_matcher_path(
                    Some(&location.path),
                    &format!("{related_field}.path"),
                    out,
                );
                validate_range(
                    location.line,
                    location.column,
                    location.end_line,
                    location.end_column,
                    &related_field,
                    out,
                );
            }
        }
    }
}

fn validate_analysis_failure_matchers(
    values: Option<&[AnalysisFailureMatcher]>,
    field: &str,
    out: &mut Vec<ValidationDiagnostic>,
) {
    let Some(values) = values else {
        return;
    };
    for (index, matcher) in values.iter().enumerate() {
        let matcher_field = format!("{field}.analysis_failures[{index}]");
        if matcher.rule.trim().is_empty() {
            diagnostic(
                out,
                ValidationCode::InvalidMatcher,
                format!("{matcher_field}.rule"),
                "must not be empty",
            );
        }
        validate_optional_matcher_path(
            matcher.path.as_deref(),
            &format!("{matcher_field}.path"),
            out,
        );
        validate_count_and_message(matcher.count, matcher.message.as_ref(), &matcher_field, out);
        validate_range(
            matcher.line,
            matcher.column,
            matcher.end_line,
            matcher.end_column,
            &matcher_field,
            out,
        );
    }
}

fn validate_note_matchers(
    values: Option<&[NoteMatcher]>,
    field: &str,
    out: &mut Vec<ValidationDiagnostic>,
) {
    let Some(values) = values else {
        return;
    };
    for (index, matcher) in values.iter().enumerate() {
        let matcher_field = format!("{field}.notes[{index}]");
        if matcher
            .kind
            .as_ref()
            .is_some_and(|kind| kind.trim().is_empty())
        {
            diagnostic(
                out,
                ValidationCode::InvalidMatcher,
                format!("{matcher_field}.kind"),
                "must not be empty",
            );
        }
        validate_optional_matcher_path(
            matcher.path.as_deref(),
            &format!("{matcher_field}.path"),
            out,
        );
        validate_count_and_message(matcher.count, matcher.message.as_ref(), &matcher_field, out);
        validate_range(
            matcher.line,
            matcher.column,
            matcher.end_line,
            matcher.end_column,
            &matcher_field,
            out,
        );
    }
}

fn has_evaluation_channels(expectation: &StepExpectation) -> bool {
    expectation.violations.is_some()
        || expectation.analysis_failures.is_some()
        || expectation.notes.is_some()
        || expectation.applied.is_some()
        || expectation.skipped.is_some()
        || expectation.suggested.is_some()
        || expectation.unfixable.is_some()
        || expectation.non_convergent.is_some()
        || expectation.diff.is_some()
}

fn has_fix_channels(expectation: &StepExpectation) -> bool {
    expectation.applied.is_some()
        || expectation.skipped.is_some()
        || expectation.suggested.is_some()
        || expectation.unfixable.is_some()
        || expectation.non_convergent.is_some()
        || expectation.diff.is_some()
}

fn validate_repository_paths(field: &str, paths: &[String], out: &mut Vec<ValidationDiagnostic>) {
    for (index, path) in paths.iter().enumerate() {
        let result = if path == "." {
            Ok(())
        } else {
            crate::confinement::validate_package_path(path)
        };
        if let Err(message) = result {
            diagnostic(
                out,
                ValidationCode::InvalidPackagePath,
                format!("{field}[{index}]"),
                message,
            );
        }
    }
}

fn validate_rule_ids(field: &str, ids: &[String], out: &mut Vec<ValidationDiagnostic>) {
    let mut seen = HashSet::new();
    for (index, id) in ids.iter().enumerate() {
        if id.trim().is_empty() {
            diagnostic(
                out,
                ValidationCode::InvalidMatcher,
                format!("{field}[{index}]"),
                "rule id must not be empty",
            );
        } else if !seen.insert(id) {
            diagnostic(
                out,
                ValidationCode::InvalidMatcher,
                format!("{field}[{index}]"),
                format!("duplicates earlier rule id {id:?}"),
            );
        }
    }
}

fn validate_optional_matcher_path(
    path: Option<&str>,
    field: &str,
    out: &mut Vec<ValidationDiagnostic>,
) {
    if path.is_some_and(str::is_empty) {
        diagnostic(
            out,
            ValidationCode::InvalidMatcher,
            field,
            "must not be empty",
        );
    }
}

fn validate_count_and_message(
    count: usize,
    message: Option<&MessageMatcher>,
    field: &str,
    out: &mut Vec<ValidationDiagnostic>,
) {
    if count == 0 {
        diagnostic(
            out,
            ValidationCode::InvalidCount,
            format!("{field}.count"),
            "must be at least 1",
        );
    }
    if let Some(message) = message {
        validate_message(message, &format!("{field}.message"), out);
    }
}

fn validate_message(matcher: &MessageMatcher, field: &str, out: &mut Vec<ValidationDiagnostic>) {
    if usize::from(matcher.exact.is_some()) + usize::from(matcher.regex.is_some()) != 1 {
        diagnostic(
            out,
            ValidationCode::InvalidMessageMatcher,
            field,
            "must declare exactly one of `exact` or `regex`",
        );
        return;
    }
    if let Some(pattern) = &matcher.regex {
        if pattern.len() > MAX_MATCHER_REGEX_BYTES {
            diagnostic(
                out,
                ValidationCode::InvalidRegex,
                field,
                format!("regex exceeds {MAX_MATCHER_REGEX_BYTES} bytes"),
            );
        } else if let Err(error) = regex::Regex::new(pattern) {
            diagnostic(
                out,
                ValidationCode::InvalidRegex,
                field,
                format!("invalid Rust regex: {error}"),
            );
        }
    }
}

fn validate_range(
    line: Option<usize>,
    column: Option<usize>,
    end_line: Option<usize>,
    end_column: Option<usize>,
    field: &str,
    out: &mut Vec<ValidationDiagnostic>,
) {
    let supplied = [line, column, end_line, end_column]
        .into_iter()
        .flatten()
        .count();
    if supplied != 0 && supplied != 4 {
        diagnostic(
            out,
            ValidationCode::InvalidRange,
            field,
            "source ranges must provide line, column, end_line, and end_column together",
        );
        return;
    }
    let (Some(line), Some(column), Some(end_line), Some(end_column)) =
        (line, column, end_line, end_column)
    else {
        return;
    };
    if line == 0
        || column == 0
        || end_line == 0
        || end_column == 0
        || (end_line, end_column) < (line, column)
    {
        diagnostic(
            out,
            ValidationCode::InvalidRange,
            field,
            "source range must be one-based and half-open with end >= start",
        );
    }
}

fn contains_tag(value: &ayaml::Value) -> bool {
    match value {
        ayaml::Value::Tagged(_) => true,
        ayaml::Value::Sequence(values) => values.iter().any(contains_tag),
        ayaml::Value::Mapping(values) => values
            .iter()
            .any(|(key, value)| contains_tag(key) || contains_tag(value)),
        _ => false,
    }
}

fn contains_null(value: &ayaml::Value) -> bool {
    match value {
        ayaml::Value::Null => true,
        ayaml::Value::Sequence(values) => values.iter().any(contains_null),
        ayaml::Value::Mapping(values) => values
            .iter()
            .any(|(key, value)| contains_null(key) || contains_null(value)),
        ayaml::Value::Tagged(value) => contains_null(&value.value),
        _ => false,
    }
}

fn diagnostic(
    out: &mut Vec<ValidationDiagnostic>,
    code: ValidationCode,
    field: impl Into<String>,
    message: impl Into<String>,
) {
    out.push(ValidationDiagnostic {
        code,
        field: field.into(),
        message: message.into(),
    });
}

const fn default_true() -> bool {
    true
}

const fn default_count() -> usize {
    1
}

fn remove_null_schemas(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            let single_type = if let Some(serde_json::Value::Array(types)) = object.get_mut("type")
            {
                types.retain(|kind| kind.as_str() != Some("null"));
                (types.len() == 1).then(|| types[0].clone())
            } else {
                None
            };
            if let Some(kind) = single_type {
                object.insert("type".to_string(), kind);
            }
            for key in ["anyOf", "oneOf"] {
                if let Some(serde_json::Value::Array(branches)) = object.get_mut(key) {
                    branches.retain(|branch| {
                        branch.get("type").and_then(|kind| kind.as_str()) != Some("null")
                    });
                }
            }
            for child in object.values_mut() {
                remove_null_schemas(child);
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                remove_null_schemas(child);
            }
        }
        _ => {}
    }
}

fn package_path_schema() -> serde_json::Value {
    serde_json::json!({
        "minLength": 1,
        "allOf": [
            {"not": {"pattern": "^(?:/|[A-Za-z]:)"}},
            {"not": {"pattern": "\\\\"}},
            {"not": {"pattern": "(?:^|/)\\.\\.(?:/|$)"}},
            {"not": {"pattern": "(?:^|/)\\.(?:/|$)"}},
            {"not": {"pattern": "//|/$"}}
        ]
    })
}

fn tree_name_schema() -> serde_json::Value {
    serde_json::json!({
        "minLength": 1,
        "allOf": [
            {"not": {"enum": [".", ".."]}},
            {"not": {"pattern": r"[/\\\u0000]"}},
            {"not": {"pattern": "^[A-Za-z]:"}},
            {"not": {"pattern": "^\\.[gG][iI][tT]$"}}
        ]
    })
}

fn customize_given_schema(root: &mut serde_json::Value, path_schema: &serde_json::Value) {
    let given = root["$defs"]
        .get_mut("GivenV1")
        .expect("given definition exists");
    given["allOf"] = serde_json::json!([
        {"oneOf": [
            {"required": ["tree"], "not": {"required": ["fixture"]}},
            {"required": ["fixture"], "not": {"required": ["tree"]}}
        ]},
        {"oneOf": [
            {"required": ["config"], "not": {"required": ["config_file"]}},
            {"required": ["config_file"], "not": {"required": ["config"]}}
        ]}
    ]);
    given["properties"]["env"]["propertyNames"] = serde_json::json!({"pattern": r"^[^=\u0000]+$"});
    given["properties"]["env"]["additionalProperties"]["pattern"] =
        serde_json::Value::String(r"^[^\u0000]*$".to_string());
    for field in ["fixture", "config_file"] {
        given["properties"][field]
            .as_object_mut()
            .expect("path property is a schema object")
            .extend(
                path_schema
                    .as_object()
                    .expect("package path schema is an object")
                    .clone(),
            );
    }
}

fn customize_tree_schema(root: &mut serde_json::Value, name_schema: serde_json::Value) {
    root["$defs"]["TreeSpec"]["propertyNames"] = name_schema.clone();
    let directory_branch = root["$defs"]["TreeNode"]["anyOf"]
        .as_array_mut()
        .expect("tree node alternatives are an array")
        .iter_mut()
        .find(|branch| branch.get("type").and_then(serde_json::Value::as_str) == Some("object"))
        .expect("tree node has a directory branch");
    directory_branch["propertyNames"] = name_schema;
}

fn customize_matcher_schema(root: &mut serde_json::Value) {
    for field in ["applied", "skipped", "suggested", "unfixable"] {
        let property = &mut root["$defs"]["StepExpectation"]["properties"][field];
        property["items"]["pattern"] = serde_json::Value::String(r"\S".to_string());
        property["uniqueItems"] = serde_json::Value::Bool(true);
    }
    for definition in ["ViolationMatcher", "AnalysisFailureMatcher"] {
        root["$defs"][definition]["properties"]["rule"]["pattern"] =
            serde_json::Value::String(r"\S".to_string());
    }
    for definition in [
        "ViolationMatcher",
        "AnalysisFailureMatcher",
        "NoteMatcher",
        "RelatedLocationMatcher",
    ] {
        root["$defs"][definition]["properties"]["path"]["minLength"] = serde_json::Value::from(1);
    }
    root["$defs"]["NoteMatcher"]["properties"]["kind"]["pattern"] =
        serde_json::Value::String(r"\S".to_string());
    let message = root["$defs"]
        .get_mut("MessageMatcher")
        .expect("message matcher definition exists");
    message["oneOf"] = serde_json::json!([
        {"required": ["exact"], "not": {"required": ["regex"]}},
        {"required": ["regex"], "not": {"required": ["exact"]}}
    ]);
    message["properties"]["regex"]["maxLength"] = serde_json::Value::from(MAX_MATCHER_REGEX_BYTES);
}

fn customize_git_schema(root: &mut serde_json::Value, path_schema: &serde_json::Value) {
    let repository_path_schema = serde_json::json!({
        "oneOf": [
            {"const": "."},
            path_schema.clone()
        ]
    });
    for definition in ["PolicyGit", "PolicyDetailedCommit"] {
        for field in ["add", "add_force"] {
            if let Some(property) = root["$defs"][definition]["properties"].get_mut(field) {
                property["items"] = repository_path_schema.clone();
            }
        }
    }
    for field in ["message", "date"] {
        root["$defs"]["PolicyDetailedCommit"]["properties"][field]["pattern"] =
            serde_json::Value::String(r"^[^\u0000]+$".to_string());
    }
    let subject_branch = root["$defs"]["PolicyCommit"]["anyOf"]
        .as_array_mut()
        .expect("commit alternatives are an array")
        .iter_mut()
        .find(|branch| branch.get("type").and_then(serde_json::Value::as_str) == Some("string"))
        .expect("commit has a subject-string branch");
    subject_branch["pattern"] = serde_json::Value::String(r"^[^\u0000]+$".to_string());
}

fn customize_policy_root_schema(root: &mut serde_json::Value, path_schema: &serde_json::Value) {
    root["properties"]["name"]["pattern"] = serde_json::Value::String(r"\S".to_string());
    root["properties"]["tags"]["uniqueItems"] = serde_json::Value::Bool(true);
    root["properties"]["tags"]["items"]["pattern"] = serde_json::Value::String(r"\S".to_string());
    for field in ["kind", "title"] {
        root["$defs"]["PolicyTestDocs"]["properties"][field]["pattern"] =
            serde_json::Value::String(r"\S".to_string());
    }
    root["properties"]["snapshot"]
        .as_object_mut()
        .expect("snapshot is a schema object")
        .extend(
            path_schema
                .as_object()
                .expect("package path schema is an object")
                .clone(),
        );
    root["properties"]["snapshot"]["pattern"] = serde_json::Value::String(r"\.json$".to_string());
}

/// Generate the canonical policy-test v1 JSON Schema.
#[must_use]
pub fn policy_test_schema() -> schemars::Schema {
    let generated = schemars::schema_for!(PolicyTest);
    let mut schema =
        serde_json::to_value(generated).expect("schemars schema must serialize to JSON");
    remove_null_schemas(&mut schema);
    let path_schema = package_path_schema();
    let root = &mut schema;
    root["$schema"] =
        serde_json::Value::String("https://json-schema.org/draft/2020-12/schema".to_string());
    root["$id"] =
        serde_json::Value::String("https://alint.org/schemas/v1/policy-test.json".to_string());
    root["title"] = serde_json::Value::String("alint policy test v1".to_string());
    root["properties"]["test_version"] = serde_json::json!({"type": "integer", "const": 1});
    customize_given_schema(root, &path_schema);
    customize_tree_schema(root, tree_name_schema());
    customize_matcher_schema(root);
    customize_git_schema(root, &path_schema);
    customize_policy_root_schema(root, &path_schema);
    schemars::Schema::try_from(schema).expect("generated root remains a JSON Schema object")
}

/// Pretty canonical JSON for the committed schema artifact.
pub fn policy_test_schema_json() -> Result<String, serde_json::Error> {
    let mut rendered = serde_json::to_string_pretty(&policy_test_schema())?;
    rendered.push('\n');
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_yaml() -> &'static str {
        r#"test_version: 1
name: demo
given:
  tree: { a.txt: "x" }
  config: |
    version: 1
    rules: []
when: [check]
expect:
  - exit: success
    violations: []
"#
    }

    #[test]
    fn discovery_discriminator_accepts_top_level_forms_and_ignores_support_yaml() {
        for candidate in [
            "test_version: 1\n",
            "'test_version': 1\n",
            "{\"test_version\": 1}\n",
            "test_version: [\n",
        ] {
            assert!(has_policy_test_discriminator(candidate), "{candidate:?}");
        }
        for support in [
            "version: 1\nrules: []\n",
            "name: workflow\non: [push]\n",
            "fixture:\n  test_version: 1\n",
            "malformed: [\n",
        ] {
            assert!(!has_policy_test_discriminator(support), "{support:?}");
        }
    }

    #[test]
    fn guarded_parser_dispatches_version_before_typed_model() {
        let parsed = PolicyTest::from_yaml(valid_yaml()).unwrap();
        assert!(parsed.validate().is_ok());
        let error =
            PolicyTest::from_yaml(&valid_yaml().replace("test_version: 1", "test_version: 2"))
                .unwrap_err();
        assert!(matches!(
            error,
            PolicyTestParseError::UnsupportedVersion { found: 2, .. }
        ));
    }

    #[test]
    fn rejects_missing_version_tags_and_complex_flow() {
        assert!(matches!(
            PolicyTest::from_yaml(&valid_yaml().replacen("test_version: 1\n", "", 1)),
            Err(PolicyTestParseError::MissingVersion)
        ));
        assert!(matches!(
            PolicyTest::from_yaml(&valid_yaml().replace("name: demo", "name: !secret demo")),
            Err(PolicyTestParseError::ExplicitTag)
        ));
        let bomb = format!(
            "test_version: 1\nname: bomb\ngiven: {{ tree: {open}x{close}, config: x }}\nwhen: [check]\nexpect: []\n",
            open = "[".repeat(alint_core::yaml_depth::MAX_YAML_FLOW_DEPTH + 1),
            close = "]".repeat(alint_core::yaml_depth::MAX_YAML_FLOW_DEPTH + 1),
        );
        assert!(matches!(
            PolicyTest::from_yaml(&bomb),
            Err(PolicyTestParseError::FlowDepth(_))
        ));
    }

    #[test]
    fn guarded_parser_rejects_explicit_null_for_optional_fields() {
        let yaml = valid_yaml().replace("violations: []", "violations: null");
        assert!(matches!(
            PolicyTest::from_yaml(&yaml),
            Err(PolicyTestParseError::ExplicitNull)
        ));
    }

    #[test]
    fn validation_collects_cross_field_and_matcher_errors() {
        let yaml = r#"test_version: 1
name: ""
given:
  tree: {}
  fixture: fixtures/repo
  config: "version: 1\nrules: []\n"
  config_file: config.yml
  as_of: 2025-02-29
when: [fix]
expect:
  - exit: success
    violations:
      - rule: x
        count: 0
        message: { exact: x, regex: x }
"#;
        let test = PolicyTest::from_yaml(yaml).unwrap();
        let codes: Vec<ValidationCode> = test
            .validation_diagnostics()
            .into_iter()
            .map(|error| error.code)
            .collect();
        assert!(codes.contains(&ValidationCode::EmptyName));
        assert!(codes.contains(&ValidationCode::RepositorySource));
        assert!(codes.contains(&ValidationCode::ConfigSource));
        assert!(codes.contains(&ValidationCode::InvalidAnalysisTime));
        assert!(codes.contains(&ValidationCode::InvalidCount));
        assert!(codes.contains(&ValidationCode::InvalidMessageMatcher));
        assert!(codes.contains(&ValidationCode::MissingAssertion));
    }

    #[test]
    fn schema_encodes_version_closed_objects_and_mutual_exclusion() {
        let schema = policy_test_schema();
        let value = schema.as_value();
        assert_eq!(value["properties"]["test_version"]["const"], 1);
        assert_eq!(value["additionalProperties"], false);
        assert!(value["$defs"]["GivenV1"]["allOf"].is_array());
        assert!(value["$defs"]["MessageMatcher"]["oneOf"].is_array());
    }

    #[test]
    fn schema_compiles_and_accepts_the_public_model() {
        let schema = policy_test_schema();
        let validator =
            jsonschema::validator_for(schema.as_value()).expect("policy-test schema compiles");
        let instance = serde_json::to_value(PolicyTest::from_yaml(valid_yaml()).unwrap()).unwrap();
        let errors: Vec<String> = validator
            .iter_errors(&instance)
            .map(|error| error.to_string())
            .collect();
        assert!(errors.is_empty(), "{errors:#?}");
    }

    #[test]
    fn schema_rejects_explicit_null_instead_of_treating_it_as_omitted() {
        let schema = policy_test_schema();
        let validator =
            jsonschema::validator_for(schema.as_value()).expect("policy-test schema compiles");
        let mut instance =
            serde_json::to_value(PolicyTest::from_yaml(valid_yaml()).unwrap()).unwrap();
        instance["expect"][0]["violations"] = serde_json::Value::Null;
        assert!(!validator.is_valid(&instance));
        instance["expect"][0]["violations"] = serde_json::json!([]);
        instance["given"]["env"] = serde_json::json!({"BAD=KEY": "value"});
        assert!(!validator.is_valid(&instance));
    }

    #[test]
    fn schema_rejects_unconfined_fixture_paths() {
        let schema = policy_test_schema();
        let validator =
            jsonschema::validator_for(schema.as_value()).expect("policy-test schema compiles");
        let base = serde_json::to_value(PolicyTest::from_yaml(valid_yaml()).unwrap()).unwrap();

        for path in [
            "../outside",
            "/outside",
            r"C:outside",
            r"fixtures\outside",
            "fixtures/./outside",
            "fixtures//outside",
            "fixtures/outside/",
        ] {
            let mut instance = base.clone();
            let given = instance["given"].as_object_mut().unwrap();
            given.remove("tree");
            given.insert("fixture".to_string(), serde_json::Value::from(path));
            assert!(!validator.is_valid(&instance), "schema accepted `{path}`");
        }

        let mut confined = base;
        let given = confined["given"].as_object_mut().unwrap();
        given.remove("tree");
        given.insert(
            "fixture".to_string(),
            serde_json::Value::from("fixtures/base"),
        );
        assert!(validator.is_valid(&confined));

        let mut unsafe_tree = confined;
        let given = unsafe_tree["given"].as_object_mut().unwrap();
        given.remove("fixture");
        given.insert(
            "tree".to_string(),
            serde_json::json!({"C:outside": "content"}),
        );
        assert!(!validator.is_valid(&unsafe_tree));
    }

    #[test]
    fn guarded_parser_rejects_duplicate_keys_at_any_depth() {
        for yaml in [
            valid_yaml().replace("name: demo", "name: first\nname: second"),
            valid_yaml().replace(
                "config: |",
                "env:\n    TOKEN: first\n    TOKEN: second\n  config: |",
            ),
        ] {
            let error = PolicyTest::from_yaml(&yaml).unwrap_err().to_string();
            assert!(error.contains("duplicate"), "{error}");
        }
    }

    #[test]
    fn validation_enforces_step_specific_assertions_and_git_confinement() {
        let yaml = r#"test_version: 1
name: invalid steps
tags: [same, same]
given:
  tree: { "../outside": "x" }
  config: "version: 1\nrules: []\n"
  env: { "BAD=KEY": value }
  git:
    init: false
    add: [".", "../outside"]
    commits: [""]
when: [check_changed, check, fix]
expect:
  - { exit: success, violations: [] }
  - { exit: success, diff: "" }
  - { exit: fix_non_convergent, violations: [] }
docs: { title: "", case: pass, kind: "" }
"#;
        let test = PolicyTest::from_yaml(yaml).unwrap();
        let diagnostics = test.validation_diagnostics();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == ValidationCode::InvalidTag && diagnostic.field == "tags[1]"
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == ValidationCode::InvalidGitSetup
                && diagnostic.message.contains("`init` is false")
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == ValidationCode::InvalidGitSetup
                && diagnostic.field == "given.git.commits[0]"
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == ValidationCode::InvalidPackagePath
                && diagnostic.field == "given.git.add[1]"
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == ValidationCode::InvalidEnvironment
                && diagnostic.field.contains("BAD=KEY")
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == ValidationCode::InvalidTree
                && diagnostic.field.contains("../outside")
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == ValidationCode::InvalidStepExpectation
                && diagnostic.message.contains("given.git.init: true")
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == ValidationCode::InvalidStepExpectation
                && diagnostic.message.contains("fix-result assertions")
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == ValidationCode::InvalidStepExpectation
                && diagnostic.message.contains("non_convergent: true")
        }));
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == ValidationCode::InvalidDocsMetadata)
                .count(),
            2
        );
    }
}
