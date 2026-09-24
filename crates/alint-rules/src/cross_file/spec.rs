//! `cross_file` config schema + parsing: the `source` / `targets` / `relation`
//! / `normalize` option types, their `serde` decoding, and the build-time
//! validation (`resolve_targets`, `validate_shape`, `validate_extract_regexes`).
//! The runtime evaluation lives in [`super::eval`]; assembly + `build` in
//! [`super`](super).

use alint_core::{Error, Extract, ExtractSpec, Result, Scope};
use serde::Deserialize;

/// The file whose extracted value(s) form the reference side of the relation:
/// a single `{ file, extract }`, or (set relations only) `{ files: <glob>,
/// extract }` whose matches are unioned into one set.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("oneOf" = [{"required": ["file"]}, {"required": ["files"]}]))]
pub(super) struct SourceSpec {
    /// A single source file.
    #[serde(default)]
    pub(super) file: Option<String>,
    /// A glob whose matches are read and whose extracted values are UNIONED
    /// into one set, for the set relations only (`subset` / `superset` /
    /// `set_equals`).
    #[serde(default)]
    pub(super) files: Option<String>,
    /// The extraction to apply. Absent for `identical` (whole-file); required
    /// otherwise.
    #[serde(default)]
    pub(super) extract: Option<ExtractSpec>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct TargetEntrySpec {
    /// A single target file.
    file: String,
    /// The extraction to apply to this target (absent for `identical`).
    #[serde(default)]
    extract: Option<ExtractSpec>,
}

/// `targets:` is either a `{ files: <glob>, extract: … }` map
/// (form a - one query applied per glob match) or a sequence of
/// `{ file, extract }` (form b - heterogeneous pins). A YAML map
/// vs a sequence are structurally distinct, so an untagged enum
/// decodes them unambiguously. `extract` is absent for `identical`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(
    untagged,
    expecting = "a `{ files, extract }` map, or a list of `{ file, extract }` entries"
)]
pub(super) enum TargetsSpec {
    /// Form a: one query applied per glob match.
    // The hand-written schema rejected unknown keys in this form; schemars does
    // not emit `additionalProperties: false` for an untagged struct variant, so
    // restore it schema-side (the loader stays lenient, as it was before).
    #[schemars(extend("additionalProperties" = false))]
    Glob {
        files: String,
        // Boxed so this inline variant does not dwarf `List` (a `Vec`): the
        // `ExtractSpec` one-of has grown one `Option<String>` per config format,
        // and unboxed it trips clippy's `large_enum_variant`. Transparent to
        // serde and schemars, and `resolve()` moves through the `Box` unchanged.
        #[serde(default)]
        extract: Option<Box<ExtractSpec>>,
    },
    /// Form b: a sequence of heterogeneous `{ file, extract }` pins.
    List(Vec<TargetEntrySpec>),
}

/// The relation the source must hold to each target. `equals` is the
/// 1:1 scalar case (the released `cross_file_value_equals`); the set
/// relations compare extracted sets; `identical` compares whole
/// files; `resolves` checks path existence on the filesystem.
#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum Relation {
    /// Source extracts exactly one value `v`; every target value
    /// must equal `v` (after normalize).
    #[default]
    Equals,
    /// `S ⊆ T` - every source value appears in the target
    /// (singleton `S` = membership).
    Subset,
    /// `S ⊇ T` - every target value appears in the source.
    Superset,
    /// `S == T` - the sets match exactly.
    SetEquals,
    /// Whole-file byte identity (optional `skip_header_lines`).
    Identical,
    /// Each extracted source path must exist on disk (file or dir).
    Resolves,
}

impl Relation {
    fn is_value(self) -> bool {
        matches!(
            self,
            Self::Equals | Self::Subset | Self::Superset | Self::SetEquals
        )
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(rename = "NormalizeTransform")]
pub(super) enum Normalize {
    #[default]
    None,
    Trim,
    Lower,
    /// Compare only the leading `MAJOR` token (the dotnet/runtime
    /// SDK-band shape: same feature band, not exact patch).
    SemverMajor,
    /// Compare only the leading `MAJOR.MINOR` band - drops patch and
    /// pre-release and takes the leading digits of each token, so
    /// `4.36-dev`, `4.36.0`, `pnpm@11.3.0` (→ `11.3`) and `>=22.13`
    /// all reconcile to one band (the protobuf / pnpm version-format
    /// case the v0.12 study surfaced).
    SemverMinor,
}

impl Normalize {
    fn apply(self, v: &str) -> String {
        match self {
            Self::None => v.to_string(),
            Self::Trim => v.trim().to_string(),
            Self::Lower => v.trim().to_lowercase(),
            // Leading `.`-token, leading non-digits stripped. The trailing
            // `trim_end` keeps normalisation idempotent: `trim()` runs before
            // the split, so the pre-`.` token can still carry trailing
            // whitespace (e.g. `"0 ."` -> token `"0 "`) that a second pass
            // would strip — found by `normalize_transforms_are_idempotent`.
            // (Trailing non-whitespace like `-dev` is intentionally kept, so
            // released behaviour is unchanged for real version strings.)
            Self::SemverMajor => v
                .trim()
                .split('.')
                .next()
                .unwrap_or("")
                .trim_start_matches(|c: char| !c.is_ascii_digit())
                .trim_end()
                .to_string(),
            Self::SemverMinor => semver_minor(v),
        }
    }
}

/// Leading digits of a semver token after stripping a non-digit
/// prefix (`pnpm@11` → `11`, `>=22` → `22`, `36-dev` → `36`).
fn token_digits(tok: &str) -> String {
    tok.trim_start_matches(|c: char| !c.is_ascii_digit())
        .chars()
        .take_while(char::is_ascii_digit)
        .collect()
}

/// The `MAJOR.MINOR` band of a version string.
fn semver_minor(v: &str) -> String {
    let mut it = v.trim().split('.');
    let major = token_digits(it.next().unwrap_or(""));
    if major.is_empty() {
        return String::new();
    }
    match it.next().map(token_digits).filter(|m| !m.is_empty()) {
        Some(minor) => format!("{major}.{minor}"),
        None => major,
    }
}

/// `normalize:` accepts a single transform (`trim`) or an ordered
/// list (`[trim, semver-minor]`), applied left-to-right. A scalar
/// vs a sequence are structurally distinct, so an untagged enum
/// decodes them unambiguously.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(
    untagged,
    expecting = "a normalize transform (`none`, `trim`, `lower`, `semver-major`, or `semver-minor`), or a list of them"
)]
pub(super) enum NormalizeSpec {
    One(Normalize),
    Many(Vec<Normalize>),
}

impl Default for NormalizeSpec {
    fn default() -> Self {
        Self::One(Normalize::None)
    }
}

impl NormalizeSpec {
    /// The ordered transform list, with `None` (a no-op marker)
    /// dropped - so an empty list means "no normalization".
    pub(super) fn into_list(self) -> Vec<Normalize> {
        let raw = match self {
            Self::One(n) => vec![n],
            Self::Many(v) => v,
        };
        raw.into_iter().filter(|n| *n != Normalize::None).collect()
    }
}

/// Apply an ordered list of transforms to a value (left-to-right).
pub(super) fn apply_normalize(transforms: &[Normalize], v: &str) -> String {
    transforms
        .iter()
        .fold(v.to_string(), |acc, t| t.apply(&acc))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Options {
    /// The file whose extracted value(s) form the reference side of the
    /// relation: a single `{ file, extract }`, or (set relations only)
    /// `{ files: <glob>, extract }` whose matches are unioned into one set.
    pub(super) source: SourceSpec,
    /// The file(s) compared against the source, one relation check per target.
    /// Absent for `resolves` (the target is the filesystem).
    #[serde(default)]
    pub(super) targets: Option<TargetsSpec>,
    /// The assertion checked between the source and each target: `equals`
    /// (default), `subset`, `superset`, `set_equals`, `identical` (whole file
    /// byte-for-byte), or `resolves` (each path the source extracts exists on
    /// disk).
    #[serde(default)]
    pub(super) relation: Relation,
    /// A normalize transform, or an ordered list of transforms applied
    /// left-to-right (`[trim, semver-minor]`). `semver-major` / `semver-minor`
    /// keep only the leading MAJOR / MAJOR.MINOR band.
    #[serde(default)]
    pub(super) normalize: NormalizeSpec,
    /// When true, an absent target file or a missing extracted target value is
    /// tolerated instead of reported as drift.
    #[serde(default)]
    pub(super) allow_missing_target: bool,
    /// For the `identical` relation only: drop this many leading lines from
    /// both files before comparison, to ignore a differing license or
    /// generated header.
    #[serde(default)]
    #[schemars(range(min = 0))]
    pub(super) skip_header_lines: Option<usize>,
}

crate::options_schema_for!(Options);

/// Resolved target shape. `extract` is `None` for `identical`
/// (whole-file), `Some` for the value relations.
#[derive(Debug)]
pub(super) enum Targets {
    Glob {
        scope: Scope,
        extract: Option<Extract>,
    },
    List(Vec<(String, Option<Extract>)>),
}

/// Resolve a `targets:` spec, validating each glob / list entry.
/// `extract` stays `Option` (absent for `identical`); the
/// relation-shape coupling is checked in `validate_shape`.
pub(super) fn resolve_targets(ts: TargetsSpec, cfg: &impl Fn(String) -> Error) -> Result<Targets> {
    match ts {
        TargetsSpec::Glob { files, extract } => {
            if files.trim().is_empty() {
                return Err(cfg("`targets.files` must not be empty".into()));
            }
            let scope = Scope::from_patterns(std::slice::from_ref(&files))
                .map_err(|e| cfg(format!("invalid `targets.files` glob: {e}")))?;
            let extract = match extract {
                Some(e) => Some(
                    e.resolve()
                        .map_err(|e| cfg(format!("invalid `targets.extract`: {e}")))?,
                ),
                None => None,
            };
            Ok(Targets::Glob { scope, extract })
        }
        TargetsSpec::List(list) => {
            if list.is_empty() {
                return Err(cfg("`targets` list must not be empty".into()));
            }
            let mut resolved = Vec::with_capacity(list.len());
            for (i, t) in list.into_iter().enumerate() {
                if t.file.trim().is_empty() {
                    return Err(cfg(format!("`targets[{i}].file` must not be empty")));
                }
                let ex = match t.extract {
                    Some(e) => Some(
                        e.resolve()
                            .map_err(|e| cfg(format!("invalid `targets[{i}].extract`: {e}")))?,
                    ),
                    None => None,
                };
                // Normalize the config-verbatim path (strip `./`, resolve `..`
                // lexically) at the single resolution point, so BOTH the check
                // (violation path) and the value fixer (its stored target path) use
                // git's canonical diff spelling -- otherwise `--changed` over-demotes
                // a `./`-prefixed target that IS the changed file, or the fixer can't
                // match the normalized violation path (audit F2). A lexical escape
                // keeps the raw string (read confinement then reports it out-of-root).
                let file = std::path::Path::new(&t.file);
                let file = crate::pathsafe::normalize_confined(file)
                    .map_or(t.file, |p| p.to_string_lossy().into_owned());
                resolved.push((file, ex));
            }
            Ok(Targets::List(resolved))
        }
    }
}

/// Enforce the per-relation shape: value relations need
/// `source.extract` + `targets` (with `extract`); `identical` needs
/// `targets` without `extract` and no `source.extract`; `resolves`
/// needs `source.extract` and no `targets`.
pub(super) fn validate_shape(
    relation: Relation,
    source_extract: Option<&Extract>,
    targets: Option<&Targets>,
    cfg: &impl Fn(String) -> Error,
) -> Result<()> {
    let (any_target_extract, all_target_extract) = match targets {
        Some(Targets::Glob { extract, .. }) => (extract.is_some(), extract.is_some()),
        Some(Targets::List(list)) => (
            list.iter().any(|(_, e)| e.is_some()),
            list.iter().all(|(_, e)| e.is_some()),
        ),
        None => (false, false),
    };
    if relation.is_value() {
        if source_extract.is_none() {
            return Err(cfg(format!(
                "`relation: {relation:?}` (a value relation) needs `source.extract`"
            )));
        }
        if targets.is_none() {
            return Err(cfg("a value relation needs `targets`".into()));
        }
        if !all_target_extract {
            return Err(cfg("a value relation's `targets` need `extract`".into()));
        }
    } else if relation == Relation::Identical {
        if source_extract.is_some() {
            return Err(cfg(
                "`relation: identical` compares whole files; remove `source.extract`".into(),
            ));
        }
        if targets.is_none() {
            return Err(cfg("`relation: identical` needs `targets`".into()));
        }
        if any_target_extract {
            return Err(cfg(
                "`relation: identical` compares whole files; remove `targets.extract`".into(),
            ));
        }
    } else {
        // resolves
        if source_extract.is_none() {
            return Err(cfg(
                "`relation: resolves` needs `source.extract` (the paths to check)".into(),
            ));
        }
        if targets.is_some() {
            return Err(cfg(
                "`relation: resolves` checks the filesystem; remove `targets`".into(),
            ));
        }
    }
    Ok(())
}

/// Reject a malformed regex extract at build time (like `file_graph` /
/// `registry_paths_resolve`), so a bad pattern is a clean config error
/// rather than an error-level eval-time violation.
pub(super) fn validate_extract_regexes(
    source_extract: Option<&Extract>,
    targets: Option<&Targets>,
    cfg: &impl Fn(String) -> Error,
) -> Result<()> {
    let check = |e: Option<&Extract>| -> Result<()> {
        if let Some(Extract::Regex(p)) = e {
            regex::Regex::new(p).map_err(|err| cfg(format!("invalid `extract.regex`: {err}")))?;
        }
        Ok(())
    };
    check(source_extract)?;
    if let Some(t) = targets {
        match t {
            Targets::Glob { extract, .. } => check(extract.as_ref())?,
            Targets::List(list) => {
                for (_, e) in list {
                    check(e.as_ref())?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::{prop_assert, prop_assert_eq, proptest};

    #[test]
    fn untagged_enums_name_accepted_forms_not_internal_enum() {
        // A value matching no variant reports the accepted forms (via
        // `#[serde(expecting)]`), not the internal untagged-enum name.
        let n = serde_yaml_ng::from_str::<NormalizeSpec>("42")
            .unwrap_err()
            .to_string();
        assert!(
            n.contains("a normalize transform") && !n.contains("NormalizeSpec"),
            "NormalizeSpec: {n}"
        );
        let t = serde_yaml_ng::from_str::<TargetsSpec>("42")
            .unwrap_err()
            .to_string();
        assert!(
            t.contains("{ files, extract }") && !t.contains("TargetsSpec"),
            "TargetsSpec: {t}"
        );
    }

    proptest! {
        /// Every `normalize` transform is idempotent: normalising an
        /// already-normalised value is a no-op. A non-idempotent
        /// transform would make `equals` comparisons depend on how many
        /// times normalisation ran - a latent correctness bug. (This is
        /// the behaviour spec; proptest is the partial proof.)
        #[test]
        fn normalize_transforms_are_idempotent(s in r"\PC{0,48}") {
            for t in [
                Normalize::Trim,
                Normalize::Lower,
                Normalize::SemverMajor,
                Normalize::SemverMinor,
            ] {
                let once = t.apply(&s);
                let twice = t.apply(&once);
                prop_assert_eq!(&twice, &once, "{:?} is not idempotent on {:?}", t, s);
            }
        }

        /// `apply_normalize` with a single transform equals that
        /// transform applied directly - the fold has no off-by-one.
        #[test]
        fn apply_normalize_single_equals_transform(s in r"\PC{0,48}") {
            let t = Normalize::SemverMinor;
            prop_assert_eq!(apply_normalize(&[t], &s), t.apply(&s));
        }

        /// `semver_minor` always yields a clean band: empty, or digits
        /// optionally followed by `.` and more digits (`MAJOR` or
        /// `MAJOR.MINOR`). No stray separators or non-digits survive.
        #[test]
        fn semver_minor_yields_a_clean_band(s in r"\PC{0,48}") {
            let band = semver_minor(&s);
            if !band.is_empty() {
                let mut parts = band.split('.');
                let major = parts.next().unwrap();
                prop_assert!(!major.is_empty() && major.bytes().all(|b| b.is_ascii_digit()));
                if let Some(minor) = parts.next() {
                    prop_assert!(!minor.is_empty() && minor.bytes().all(|b| b.is_ascii_digit()));
                }
                prop_assert!(parts.next().is_none(), "at most MAJOR.MINOR");
            }
        }
    }

    #[test]
    fn semver_major_is_idempotent_on_trailing_space_token() {
        // Regression (found by normalize_transforms_are_idempotent): `trim()`
        // runs before `split('.')`, so the pre-`.` token can carry trailing
        // whitespace (`"0 ."` -> token `"0 "`). Without the trailing `trim_end`
        // the first pass returned `"0 "` and a second pass `"0"` — not stable.
        assert_eq!(Normalize::SemverMajor.apply("0 ."), "0");
        let once = Normalize::SemverMajor.apply("0 .");
        assert_eq!(Normalize::SemverMajor.apply(&once), once);
    }

    #[test]
    fn semver_minor_reconciles_version_formats() {
        // The protobuf / pnpm version-format cases all collapse to
        // one MAJOR.MINOR band.
        assert_eq!(semver_minor("4.36-dev"), "4.36");
        assert_eq!(semver_minor("4.36.0"), "4.36");
        assert_eq!(semver_minor("pnpm@11.3.0"), "11.3");
        assert_eq!(semver_minor(">=22.13"), "22.13");
        assert_eq!(semver_minor("4"), "4");
        assert_eq!(semver_minor(""), "");
        // SemverMajor keeps its unchanged released behaviour.
        assert_eq!(Normalize::SemverMajor.apply("8.0.402"), "8");
    }

    #[test]
    fn normalize_list_applies_in_order_and_filters_none() {
        assert_eq!(
            apply_normalize(&[Normalize::Trim, Normalize::Lower], "  ABC  "),
            "abc"
        );
        // An empty list is the identity.
        assert_eq!(apply_normalize(&[], "  ABC  "), "  ABC  ");
        // `none` is dropped from the resolved list.
        assert!(NormalizeSpec::One(Normalize::None).into_list().is_empty());
        assert_eq!(
            NormalizeSpec::Many(vec![Normalize::None, Normalize::Trim]).into_list(),
            vec![Normalize::Trim]
        );
    }

    #[test]
    fn resolve_targets_normalizes_a_dot_slash_list_path() {
        // Audit F2: a `./`-prefixed list path is normalized at the single resolution
        // point, so the violation path AND the value fixer's stored target path both
        // match git's canonical diff spelling -- otherwise `--changed` over-demotes
        // the changed target, or the fixer cannot match the normalized violation.
        let cfg = |m: String| Error::rule_config("t", m);
        let ts = TargetsSpec::List(vec![TargetEntrySpec {
            file: "./sub/t.json".into(),
            extract: None,
        }]);
        match resolve_targets(ts, &cfg).unwrap() {
            Targets::List(list) => assert_eq!(list[0].0, "sub/t.json", "`./` stripped"),
            Targets::Glob { .. } => panic!("expected a List, got a Glob"),
        }
    }
}
