//! The string transforms `cross_file`'s `normalize:` and a relation's
//! `transform:` share (design section 6), so the two cannot drift.

/// One string transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringTransform {
    /// Strip surrounding whitespace.
    Trim,
    /// Trim, then lower-case.
    Lower,
    /// Only the leading `MAJOR` token (the dotnet/runtime SDK-band shape:
    /// same feature band, not exact patch).
    SemverMajor,
    /// Only the leading `MAJOR.MINOR` band: drops patch and pre-release and
    /// takes the leading digits of each token, so `4.36-dev`, `4.36.0`,
    /// `pnpm@11.3.0` (`11.3`) and `>=22.13` all reconcile to one band.
    SemverMinor,
}

impl StringTransform {
    /// The transform as a configuration writes it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Trim => "trim",
            Self::Lower => "lower",
            Self::SemverMajor => "semver-major",
            Self::SemverMinor => "semver-minor",
        }
    }

    #[must_use]
    pub fn apply(self, value: &str) -> String {
        match self {
            Self::Trim => value.trim().to_string(),
            Self::Lower => value.trim().to_lowercase(),
            // Leading `.`-token, leading non-digits stripped. The trailing
            // `trim_end` keeps normalisation idempotent: `trim()` runs before
            // the split, so the pre-`.` token can still carry trailing
            // whitespace (e.g. `"0 ."` -> token `"0 "`) that a second pass
            // would strip. (Trailing non-whitespace like `-dev` is
            // intentionally kept, so released behaviour is unchanged for real
            // version strings.)
            Self::SemverMajor => value
                .trim()
                .split('.')
                .next()
                .unwrap_or("")
                .trim_start_matches(|c: char| !c.is_ascii_digit())
                .trim_end()
                .to_string(),
            Self::SemverMinor => semver_minor(value),
        }
    }
}

/// Leading digits of a semver token after stripping a non-digit prefix
/// (`pnpm@11` -> `11`, `>=22` -> `22`, `36-dev` -> `36`).
fn token_digits(token: &str) -> String {
    token
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .chars()
        .take_while(char::is_ascii_digit)
        .collect()
}

/// The `MAJOR.MINOR` band of a version string.
fn semver_minor(value: &str) -> String {
    let mut tokens = value.trim().split('.');
    let major = token_digits(tokens.next().unwrap_or(""));
    if major.is_empty() {
        return String::new();
    }
    match tokens
        .next()
        .map(token_digits)
        .filter(|minor| !minor.is_empty())
    {
        Some(minor) => format!("{major}.{minor}"),
        None => major,
    }
}
