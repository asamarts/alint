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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_config_constructor_accepts_strings_and_str_refs() {
        let e1 = Error::rule_config("foo", "bad");
        let e2 = Error::rule_config(String::from("foo"), String::from("bad"));
        assert_eq!(e1.to_string(), e2.to_string());
    }

    #[test]
    fn rule_config_display_includes_rule_id_and_message() {
        let e = Error::rule_config("my-rule", "missing field");
        let s = e.to_string();
        assert!(s.contains("my-rule"), "missing rule id: {s}");
        assert!(s.contains("missing field"), "missing message: {s}");
    }

    #[test]
    fn unknown_rule_kind_display_quotes_the_kind() {
        let e = Error::UnknownRuleKind("not_a_real_kind".into());
        assert!(e.to_string().contains("not_a_real_kind"));
    }

    #[test]
    fn glob_error_display_includes_pattern() {
        let bad = globset::Glob::new("[unterminated").unwrap_err();
        let e = Error::Glob {
            pattern: "[unterminated".into(),
            source: bad,
        };
        let s = e.to_string();
        assert!(s.contains("[unterminated"), "missing pattern: {s}");
    }

    #[test]
    fn yaml_error_propagates_via_from_impl() {
        // The `#[from] serde_yaml_ng::Error` derive lets `?` lift
        // a YAML parse failure into our Error type without
        // boilerplate. Sanity-check the impl is wired.
        let parse: std::result::Result<i32, _> = serde_yaml_ng::from_str("not: yaml: [");
        let yaml_err = parse.unwrap_err();
        let our_err: Error = yaml_err.into();
        assert!(matches!(our_err, Error::Yaml(_)));
    }

    #[test]
    fn other_variant_carries_arbitrary_text() {
        let e = Error::Other("something went sideways".into());
        assert_eq!(e.to_string(), "something went sideways");
    }
}
