//! The string transforms `cross_file`'s `normalize:` and a relation's
//! `transform:` share (design section 6, ADR-0050 D8), so the two cannot drift.

/// One string transform.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Split on a non-empty literal separator and take segment `part` (0-based;
    /// negative indices count from the end). When out of range, yields `None`
    /// (`Missing` cell when `required: false`, analysis failure when `required:
    /// true`).
    Split {
        on: String,
        part: i64,
        required: bool,
    },
    /// Strip a literal prefix once if present (no-op if absent).
    StripPrefix(String),
    /// Strip the first matching literal suffix once if present (no-op if none
    /// matches).
    StripSuffix(Vec<String>),
    /// Prepend a literal prefix.
    Prefix(String),
    /// Append a literal suffix.
    Suffix(String),
}

impl StringTransform {
    /// The transform as a configuration writes it.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Trim => "trim",
            Self::Lower => "lower",
            Self::SemverMajor => "semver-major",
            Self::SemverMinor => "semver-minor",
            Self::Split { .. } => "split",
            Self::StripPrefix(_) => "strip_prefix",
            Self::StripSuffix(_) => "strip_suffix",
            Self::Prefix(_) => "prefix",
            Self::Suffix(_) => "suffix",
        }
    }

    /// Apply this transform, returning `None` only when `Split` selects an
    /// out-of-range segment index.
    #[must_use]
    pub fn apply_optional(&self, value: &str) -> Option<String> {
        match self {
            Self::Trim => Some(value.trim().to_string()),
            Self::Lower => Some(value.trim().to_lowercase()),
            // Leading `.`-token, leading non-digits stripped. The trailing
            // `trim_end` keeps normalisation idempotent: `trim()` runs before
            // the split, so the pre-`.` token can still carry trailing
            // whitespace (e.g. `"0 ."` -> token `"0 "`) that a second pass
            // would strip. (Trailing non-whitespace like `-dev` is
            // intentionally kept, so released behaviour is unchanged for real
            // version strings.)
            Self::SemverMajor => Some(
                value
                    .trim()
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .trim_start_matches(|c: char| !c.is_ascii_digit())
                    .trim_end()
                    .to_string(),
            ),
            Self::SemverMinor => Some(semver_minor(value)),
            Self::Split { on, part, .. } => split_part(value, on, *part),
            Self::StripPrefix(prefix) => Some(
                value
                    .strip_prefix(prefix.as_str())
                    .unwrap_or(value)
                    .to_string(),
            ),
            Self::StripSuffix(suffixes) => {
                for suffix in suffixes {
                    if let Some(stripped) = value.strip_suffix(suffix.as_str()) {
                        return Some(stripped.to_string());
                    }
                }
                Some(value.to_string())
            }
            Self::Prefix(prefix) => Some(format!("{prefix}{value}")),
            Self::Suffix(suffix) => Some(format!("{value}{suffix}")),
        }
    }

    #[must_use]
    pub fn apply(&self, value: &str) -> String {
        self.apply_optional(value).unwrap_or_default()
    }
}

/// Split `value` on non-empty separator `on` and return segment `part`
/// (0-based; negative indices count from the end). Returns `None` when `on` is
/// empty or `part` is out of range.
fn split_part(value: &str, on: &str, part: i64) -> Option<String> {
    if on.is_empty() {
        return None;
    }
    let segments: Vec<&str> = value.split(on).collect();
    let idx = if part >= 0 {
        usize::try_from(part).ok()?
    } else {
        let abs = usize::try_from(part.unsigned_abs()).ok()?;
        segments.len().checked_sub(abs)?
    };
    segments.get(idx).map(|seg| (*seg).to_string())
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
