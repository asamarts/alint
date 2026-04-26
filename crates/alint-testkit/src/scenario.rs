//! Scenario: the YAML document a test harness loads.
//!
//! ```yaml
//! name: file_create creates missing README
//! tags: [fix, file_exists]
//!
//! given:
//!   tree:
//!     Cargo.toml: "[package]\nname = \"demo\"\n"
//!     src:
//!       main.rs: "fn main() {}\n"
//!   config: |
//!     version: 1
//!     rules:
//!       - id: has-readme
//!         kind: file_exists
//!         paths: README.md
//!         level: error
//!         fix:
//!           file_create:
//!             content: "# demo\n"
//!
//! when: [check, fix, check]
//!
//! expect:
//!   - violations: [{rule: has-readme, level: error}]
//!   - applied: [has-readme]
//!   - violations: []
//!
//! expect_tree:
//!   README.md: "# demo\n"
//!   Cargo.toml: "[package]\nname = \"demo\"\n"
//!   src:
//!     main.rs: "fn main() {}\n"
//! ```

use serde::Deserialize;

use crate::treespec::{TreeSpec, VerifyMode};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub given: Given,
    #[serde(default)]
    pub when: Vec<Step>,
    #[serde(default)]
    pub expect: Vec<ExpectStep>,
    #[serde(default)]
    pub expect_tree: Option<TreeSpec>,
    #[serde(default)]
    pub expect_tree_mode: ExpectTreeMode,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Given {
    pub tree: TreeSpec,
    /// Raw YAML body of the `.alint.yml` the scenario exercises.
    pub config: String,
    /// Optional git-init + commit step for scenarios that exercise
    /// `git_tracked_only` or any other primitive that needs a real
    /// git index. Nothing happens when this field is absent.
    #[serde(default)]
    pub git: Option<GivenGit>,
}

/// `given.git:` block — initialise a git repo in the scenario
/// tempdir, optionally `git add` listed paths, optionally commit.
/// Paths are relative to the tempdir (i.e. they reference files
/// already materialised by the `tree:` block).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GivenGit {
    /// Run `git init` in the tempdir. Defaults to `true` so a
    /// bare `git: {}` block is enough to enable a repo.
    #[serde(default = "default_true")]
    pub init: bool,
    /// Paths to `git add` after init. Empty means "no `git add`,
    /// the working tree stays untracked." When non-empty AND
    /// `commit` is `true`, the runner then `git commit`s.
    #[serde(default)]
    pub add: Vec<String>,
    /// Whether to make a commit after `git add`. Defaults to
    /// `true` because `git ls-files` reports both staged and
    /// committed files identically — but a never-committed repo
    /// is an unusual real-world state, so the default mirrors
    /// "fully checked-in repo."
    #[serde(default = "default_true")]
    pub commit: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Check,
    Fix,
    FixDryRun,
    /// `alint check --changed` (working-tree diff, no `--base`).
    /// The runner shells out to `git ls-files --modified --others
    /// --exclude-standard` to derive the changed-set, so the
    /// scenario's `given.git:` setup must produce one. Scenarios
    /// without a git block reach the runner's hard-error path.
    CheckChanged,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectTreeMode {
    #[default]
    Strict,
    Contains,
}

impl From<ExpectTreeMode> for VerifyMode {
    fn from(m: ExpectTreeMode) -> Self {
        match m {
            ExpectTreeMode::Strict => VerifyMode::Strict,
            ExpectTreeMode::Contains => VerifyMode::Contains,
        }
    }
}

/// Per-step expectation. `violations:` is checked against a `Check`
/// step's report; `applied:`, `skipped:`, `unfixable:` against a
/// `Fix` or `FixDryRun` step.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ExpectStep {
    /// Expected violations for a `check` step. Each item matches by
    /// `rule` id; `level` and `path` are optional refinements.
    pub violations: Option<Vec<ExpectViolation>>,
    /// Rule ids expected to report `Applied` status from a fix step.
    pub applied: Option<Vec<String>>,
    /// Rule ids expected to report `Skipped` status.
    pub skipped: Option<Vec<String>>,
    /// Rule ids expected to report `Unfixable` status.
    pub unfixable: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectViolation {
    pub rule: String,
    #[serde(default)]
    pub level: Option<LevelName>,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LevelName {
    Error,
    Warning,
    Info,
    Off,
}

impl LevelName {
    pub fn matches(self, level: alint_core::Level) -> bool {
        matches!(
            (self, level),
            (Self::Error, alint_core::Level::Error)
                | (Self::Warning, alint_core::Level::Warning)
                | (Self::Info, alint_core::Level::Info)
                | (Self::Off, alint_core::Level::Off)
        )
    }
}

impl Scenario {
    pub fn from_yaml(yaml: &str) -> crate::error::Result<Self> {
        Ok(serde_yaml_ng::from_str(yaml)?)
    }

    /// Validate that `when` and `expect` have consistent lengths.
    /// Returning a [`Result`] makes misaligned scenarios surface as
    /// clear errors rather than silent mis-assertions.
    pub fn validate(&self) -> crate::error::Result<()> {
        if self.expect.len() != self.when.len() {
            return Err(crate::error::Error::scenario(format!(
                "scenario {:?}: `when` has {} step(s) but `expect` has {}",
                self.name,
                self.when.len(),
                self.expect.len()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"
name: demo
tags: [fix]
given:
  tree:
    a.txt: "x"
  config: |
    version: 1
    rules: []
when: [check]
expect:
  - violations: []
expect_tree:
  a.txt: "x"
"#;

    #[test]
    fn parses_full_scenario_yaml() {
        let s = Scenario::from_yaml(SRC).unwrap();
        assert_eq!(s.name, "demo");
        assert_eq!(s.tags, vec!["fix"]);
        assert!(matches!(s.when.as_slice(), [Step::Check]));
        assert!(s.expect_tree.is_some());
    }

    #[test]
    fn validate_catches_when_expect_mismatch() {
        let src = r#"
name: x
given:
  tree: {}
  config: "version: 1\nrules: []\n"
when: [check, fix]
expect:
  - violations: []
"#;
        let s = Scenario::from_yaml(src).unwrap();
        assert!(s.validate().is_err());
    }
}
