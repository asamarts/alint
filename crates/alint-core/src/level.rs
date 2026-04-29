use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warning,
    Info,
    Off,
}

impl Level {
    pub fn is_actionable(self) -> bool {
        matches!(self, Self::Error | Self::Warning | Self::Info)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
            Self::Off => "off",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_actionable_returns_true_for_emitted_severities() {
        assert!(Level::Error.is_actionable());
        assert!(Level::Warning.is_actionable());
        assert!(Level::Info.is_actionable());
    }

    #[test]
    fn is_actionable_returns_false_for_off() {
        // `off` is the disabled state — rules at this severity
        // never produce a `RuleResult` in the report, so the
        // engine's "is this worth showing the user?" check
        // returns false.
        assert!(!Level::Off.is_actionable());
    }

    #[test]
    fn as_str_round_trips_with_serde_lowercase_rename() {
        assert_eq!(Level::Error.as_str(), "error");
        assert_eq!(Level::Warning.as_str(), "warning");
        assert_eq!(Level::Info.as_str(), "info");
        assert_eq!(Level::Off.as_str(), "off");
    }

    #[derive(serde::Deserialize)]
    struct Wrap {
        level: Level,
    }

    #[test]
    fn deserializes_from_lowercase_yaml_string() {
        let yaml = "level: warning\n";
        let w: Wrap = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(w.level, Level::Warning);
    }

    #[test]
    fn rejects_uppercase_yaml_string() {
        // The `rename_all = "lowercase"` attribute means the
        // serialised form is strictly lowercase. Users typing
        // `level: Error` get a clear deserialise error rather
        // than a silent default.
        let yaml = "level: Error\n";
        assert!(serde_yaml_ng::from_str::<Wrap>(yaml).is_err());
    }
}
