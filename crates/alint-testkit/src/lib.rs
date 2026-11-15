//! Versioned policy-test models, a validating hermetic runner, and declarative
//! repository fixture utilities.

// A public type in a private module has no documentation page, so the public-API
// gate (ci/scripts/public-api-parser-gate.sh) would not see what its fields name.
#![deny(unnameable_types)]

pub mod confinement;
mod error;
pub mod policy_runner;
pub mod policy_test;
mod runner_artifacts;
#[cfg(feature = "strategies")]
pub mod strategies;
pub mod test_report;
pub mod treespec;
pub use confinement::{ConfinementError, PackageEntryKind, PackageRoot, validate_package_path};
pub use error::{Error, Result};
pub use policy_runner::{
    Mismatch, MismatchChannel, ObservedAnalysisFailure, ObservedNote, ObservedRelatedLocation,
    ObservedViolation, PolicyTestRun, Runner, RunnerOptions, StepObservation, TestStatus,
    setup_git,
};
pub use policy_test::{
    AnalysisFailureKindMatcher, AnalysisFailureMatcher, ConfigurationErrorMatcher, ExpectedExit,
    GivenV1, LevelMatcher, MAX_MATCHER_REGEX_BYTES, MessageMatcher, NoteMatcher,
    POLICY_TEST_VERSION, PolicyCommit, PolicyDetailedCommit, PolicyDocsCase, PolicyDocsExample,
    PolicyGit, PolicyTest, PolicyTestParseError, RelatedLocationMatcher, StepExpectation,
    TestCapabilities, TestStep, TreeExpectationMode, ValidationCode, ValidationDiagnostic,
    ViolationMatcher, has_policy_test_discriminator, policy_test_schema, policy_test_schema_json,
};
pub use test_report::{
    POLICY_TEST_REPORT_KIND, POLICY_TEST_REPORT_SCHEMA_VERSION, PolicyTestIssue, PolicyTestRecord,
    PolicyTestReport, PolicyTestStatus, PolicyTestStepReport, PolicyTestSummary,
    policy_test_report_schema, policy_test_report_schema_json,
};
pub use treespec::{
    Discrepancy, ExecNode, ExtractOpts, SymlinkNode, TreeNode, TreeSpec, VerifyMode, VerifyReport,
    extract, materialize, verify,
};
