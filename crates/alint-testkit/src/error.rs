use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
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
    Yaml(#[from] serde_yaml_ng::Error),

    #[error("alint error: {0}")]
    Alint(#[from] alint_core::Error),

    #[error("`when` expression error: {0}")]
    When(#[from] alint_core::WhenError),

    #[error("scenario error: {0}")]
    Scenario(String),
}

impl Error {
    pub fn scenario(msg: impl Into<String>) -> Self {
        Self::Scenario(msg.into())
    }
}
