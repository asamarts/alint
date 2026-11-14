use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("path is not an existing directory: {0}")]
    NotADirectory(PathBuf),

    #[error("YAML parse error: {0}")]
    Yaml(#[from] ayaml::Error),

    #[error("alint error: {0}")]
    Alint(#[from] alint_core::Error),

    #[error("`when` expression error: {0}")]
    When(#[from] alint_core::WhenError),

    #[error("policy-test definition is invalid: {0:?}")]
    Definition(Vec<crate::policy_test::ValidationDiagnostic>),

    #[error("policy-test confinement error: {0}")]
    Confinement(#[from] crate::confinement::ConfinementError),

    #[error("policy-test capability error: {0}")]
    Capability(String),

    #[error("policy-test Git fixture error: {0}")]
    GitFixture(String),

    #[error("policy-test runner error: {0}")]
    Runtime(String),
}

impl Error {
    pub fn runtime(msg: impl Into<String>) -> Self {
        Self::Runtime(msg.into())
    }

    pub(crate) fn definition(diagnostics: Vec<crate::policy_test::ValidationDiagnostic>) -> Self {
        Self::Definition(diagnostics)
    }

    pub(crate) fn capability(message: impl Into<String>) -> Self {
        Self::Capability(message.into())
    }

    pub(crate) fn git_fixture(message: impl Into<String>) -> Self {
        Self::GitFixture(message.into())
    }
}
