use std::path::PathBuf;

use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("walk error: {0}")]
    Walk(#[from] ignore::Error),

    #[error("invalid glob {pattern:?}: {source}")]
    Glob {
        pattern: String,
        #[source]
        source: globset::Error,
    },

    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml_ng::Error),

    #[error("unknown rule kind {0:?}")]
    UnknownRuleKind(String),

    #[error("rule {rule_id:?}: {message}")]
    RuleConfig { rule_id: String, message: String },

    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn rule_config(rule_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self::RuleConfig {
            rule_id: rule_id.into(),
            message: message.into(),
        }
    }
}
