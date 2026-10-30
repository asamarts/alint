//! Configuration-source and running-binary compatibility metadata.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A strict stable release triplet used by `requires.alint`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StableVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

impl StableVersion {
    #[must_use]
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    #[must_use]
    pub const fn components(self) -> (u64, u64, u64) {
        (self.major, self.minor, self.patch)
    }
}

impl fmt::Display for StableVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Strict stable-triplet parse failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParseStableVersionError {
    #[error("expected exactly MAJOR.MINOR.PATCH with unsigned decimal components")]
    InvalidFormat,
    #[error("version components must not contain leading zeroes")]
    LeadingZero,
    #[error("version component exceeds u64")]
    Overflow,
}

impl FromStr for StableVersion {
    type Err = ParseStableVersionError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let mut parts = input.split('.');
        let major = parse_component(parts.next().ok_or(ParseStableVersionError::InvalidFormat)?)?;
        let minor = parse_component(parts.next().ok_or(ParseStableVersionError::InvalidFormat)?)?;
        let patch = parse_component(parts.next().ok_or(ParseStableVersionError::InvalidFormat)?)?;
        if parts.next().is_some() {
            return Err(ParseStableVersionError::InvalidFormat);
        }
        Ok(Self::new(major, minor, patch))
    }
}

fn parse_component(input: &str) -> Result<u64, ParseStableVersionError> {
    if input.is_empty() || !input.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ParseStableVersionError::InvalidFormat);
    }
    if input.len() > 1 && input.starts_with('0') {
        return Err(ParseStableVersionError::LeadingZero);
    }
    input
        .parse::<u64>()
        .map_err(|_| ParseStableVersionError::Overflow)
}

impl Serialize for StableVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for StableVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let input = String::deserialize(deserializer)?;
        input.parse().map_err(serde::de::Error::custom)
    }
}

/// Closed compatibility requirement mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigRequirements {
    pub alint: StableVersion,
}

/// Running alint version used for minimum-version checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunningVersion {
    stable: StableVersion,
    prerelease: Option<&'static str>,
}

impl RunningVersion {
    #[must_use]
    pub const fn new(stable: StableVersion, prerelease: bool) -> Self {
        Self {
            stable,
            prerelease: if prerelease { Some("prerelease") } else { None },
        }
    }

    /// The compile-time package version. Build metadata never affects ordering.
    #[must_use]
    pub fn current() -> Self {
        let major = env!("CARGO_PKG_VERSION_MAJOR")
            .parse()
            .expect("Cargo package major is a u64");
        let minor = env!("CARGO_PKG_VERSION_MINOR")
            .parse()
            .expect("Cargo package minor is a u64");
        let patch = env!("CARGO_PKG_VERSION_PATCH")
            .parse()
            .expect("Cargo package patch is a u64");
        let label = env!("CARGO_PKG_VERSION_PRE");
        Self {
            stable: StableVersion::new(major, minor, patch),
            prerelease: (!label.is_empty()).then_some(label),
        }
    }

    #[must_use]
    pub const fn stable(self) -> StableVersion {
        self.stable
    }

    #[must_use]
    pub const fn is_prerelease(self) -> bool {
        self.prerelease.is_some()
    }

    /// A prerelease does not satisfy its own stable triplet, but satisfies lower triplets.
    #[must_use]
    pub fn satisfies(self, required: StableVersion) -> bool {
        self.stable > required || (self.stable == required && self.prerelease.is_none())
    }
}

impl fmt::Display for RunningVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.prerelease {
            Some(label) => write!(formatter, "{}-{label}", self.stable),
            None => self.stable.fmt(formatter),
        }
    }
}

/// Source classes with distinct version-omission and composition rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSourceClass {
    Root,
    DropIn,
    LocalExtend,
    Bundled,
    Remote,
    Nested,
    Inline,
}

/// Compatibility metadata retained after source-local fields are removed for merge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigCompatibility {
    pub source: String,
    pub source_class: ConfigSourceClass,
    pub config_version: u32,
    pub requires_alint: Option<StableVersion>,
    pub consumer_version: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_triplet_parser_accepts_boundaries() {
        for accepted in [
            "0.0.0",
            "0.19.0",
            "1.2.3",
            "18446744073709551615.18446744073709551615.18446744073709551615",
        ] {
            assert_eq!(
                accepted.parse::<StableVersion>().unwrap().to_string(),
                accepted
            );
        }
    }

    #[test]
    fn stable_triplet_parser_rejects_non_literal_or_ambiguous_forms() {
        for rejected in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.02.3",
            "1.2.03",
            "v1.2.3",
            ">=1.2.3",
            "1.2.x",
            "1.2.3-alpha",
            "1.2.3+build",
            " 1.2.3",
            "1.2.3 ",
            "18446744073709551616.0.0",
        ] {
            assert!(
                rejected.parse::<StableVersion>().is_err(),
                "{rejected:?} should fail"
            );
        }
    }

    #[test]
    fn prerelease_comparison_requires_its_stable_release() {
        let prerelease = RunningVersion::new(StableVersion::new(1, 2, 3), true);
        assert!(prerelease.satisfies(StableVersion::new(1, 2, 2)));
        assert!(!prerelease.satisfies(StableVersion::new(1, 2, 3)));
        assert!(!prerelease.satisfies(StableVersion::new(1, 2, 4)));
        assert_eq!(prerelease.to_string(), "1.2.3-prerelease");
        let stable = RunningVersion::new(StableVersion::new(1, 2, 3), false);
        assert!(stable.satisfies(StableVersion::new(1, 2, 3)));
        assert!(stable.satisfies(StableVersion::new(1, 2, 2)));
        assert!(!stable.satisfies(StableVersion::new(1, 2, 4)));
    }

    #[test]
    fn requirements_are_closed_and_string_only() {
        let parsed: ConfigRequirements = ayaml::from_str("alint: \"0.19.0\"").unwrap();
        assert_eq!(parsed.alint, StableVersion::new(0, 19, 0));
        for invalid in [
            "{}",
            "alint: 0.19",
            "alint: [0, 19, 0]",
            "alint: true",
            "alint: \"0.19.0\"\nother: x",
            "alint: \"0.19.0\"\nalint: \"0.20.0\"",
        ] {
            assert!(
                ayaml::from_str::<ConfigRequirements>(invalid).is_err(),
                "{invalid:?} should fail"
            );
        }
    }
}
