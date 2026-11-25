//! Pure-Rust semantic version and range evaluator for `constraint` assertions
//! (`satisfies:` and `compare: { order: semver }`, ADR-0050 D5).
//!
//! Zero third-party dependencies, zero `unsafe`, deterministic and bounded
//! against crafted range denial-of-service (`MAX_RANGE_BYTES`, `MAX_CLAUSES`,
//! `MAX_COMPARATORS_PER_CLAUSE`).

use std::cmp::Ordering;
use std::fmt;

/// Maximum accepted byte length of a semver range expression.
pub const MAX_RANGE_BYTES: usize = 1_024;

/// Maximum accepted byte length of a single version string.
pub const MAX_VERSION_BYTES: usize = 256;

/// Maximum number of `||`-separated clauses in a range expression.
pub const MAX_CLAUSES: usize = 64;

/// Maximum number of comparators in a single clause.
pub const MAX_COMPARATORS_PER_CLAUSE: usize = 64;

/// Supported range syntax dialects for `assert.satisfies`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SemverDialect {
    /// npm / `node-semver`: `||` union, space-separated AND comparators,
    /// `^`, `~`, hyphen `A - B`, `x`/`X`/`*`, leading `v`, empty `""` -> `*`.
    /// Commas are rejected so typos fail loudly.
    #[default]
    Node,
    /// Cargo `VersionReq`: comma-separated AND comparators, bare version means
    /// `^`, `^0.0.x` pins patch, `^0.x` pins minor.
    Cargo,
    /// PEP 440 `requires-python` subset: comma-separated `>=`, `<=`, `>`, `<`,
    /// `==`, `!=`, `~=` compatible release, and `==X.*` / `!=X.*` wildcards.
    /// Explicitly fails loud on epochs (`1!`), `.post`, `.dev`, and `===`.
    Pep440Basic,
}

impl SemverDialect {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Cargo => "cargo",
            Self::Pep440Basic => "pep440-basic",
        }
    }
}

/// One dot-separated `SemVer` 2.0.0 prerelease identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PrereleaseId {
    Numeric(u64),
    AlphaNum(String),
}

impl Ord for PrereleaseId {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Numeric(a), Self::Numeric(b)) => a.cmp(b),
            (Self::Numeric(_), Self::AlphaNum(_)) => Ordering::Less,
            (Self::AlphaNum(_), Self::Numeric(_)) => Ordering::Greater,
            (Self::AlphaNum(a), Self::AlphaNum(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for PrereleaseId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for PrereleaseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Numeric(n) => write!(f, "{n}"),
            Self::AlphaNum(s) => f.write_str(s),
        }
    }
}

/// A parsed semantic version with `SemVer` 2.0.0 precedence (build metadata is
/// validated on parse and ignored during equality and ordering).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SemverVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Vec<PrereleaseId>,
}

impl SemverVersion {
    #[must_use]
    pub const fn release(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
            pre: Vec::new(),
        }
    }

    #[must_use]
    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }

    /// The lowest possible prerelease version for `(major, minor, patch)`,
    /// used as the exclusive upper bound for caret/tilde/x-ranges (`<X.Y.Z-0`).
    fn prerelease_floor(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
            pre: vec![PrereleaseId::Numeric(0)],
        }
    }
}

impl Ord for SemverVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                // Release > prerelease (SemVer 2.0.0 section 11.3).
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self.pre.cmp(&other.pre),
            })
    }
}

impl PartialOrd for SemverVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for SemverVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            f.write_str("-")?;
            for (idx, id) in self.pre.iter().enumerate() {
                if idx > 0 {
                    f.write_str(".")?;
                }
                write!(f, "{id}")?;
            }
        }
        Ok(())
    }
}

/// A parsed semver range ready for repeated matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemverRange {
    dialect: SemverDialect,
    clauses: Vec<Vec<Comparator>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Comparator {
    /// Matches any release (or any prerelease when `include_prerelease` is true).
    Any,
    /// Never matches any version (e.g., `>*`, `<*`).
    Never,
    /// Single relational comparison against `version`.
    Rel {
        op: CompOp,
        version: SemverVersion,
        /// True when the comparator in the range explicitly carried a
        /// prerelease tag (controls strict `node-semver` prerelease opt-in).
        explicit_pre: bool,
        /// When `true` and `include_prerelease` is enabled at match time,
        /// lower-bound `Ge` compares against `prerelease_floor(major, minor, patch)`
        /// so partial/wildcard/hyphen lower bounds include prereleases of the
        /// lower tuple (matching `node-semver`, Cargo, and PEP 440 `==X.*`).
        pre_floor_when_included: bool,
    },
    /// PEP 440 negated wildcard (`!=3.*` or `!=3.10.*`).
    NotPrefix { major: u64, minor: Option<u64> },
}

impl Comparator {
    fn matches(&self, version: &SemverVersion, include_prerelease: bool) -> bool {
        match self {
            Self::Any => true,
            Self::Never => false,
            Self::Rel {
                op,
                version: bound,
                pre_floor_when_included,
                ..
            } => {
                let ord = if *pre_floor_when_included && include_prerelease {
                    let floor =
                        SemverVersion::prerelease_floor(bound.major, bound.minor, bound.patch);
                    version.cmp(&floor)
                } else {
                    version.cmp(bound)
                };
                match op {
                    CompOp::Eq => ord == Ordering::Equal,
                    CompOp::Ne => ord != Ordering::Equal,
                    CompOp::Lt => ord == Ordering::Less,
                    CompOp::Le => matches!(ord, Ordering::Less | Ordering::Equal),
                    CompOp::Gt => ord == Ordering::Greater,
                    CompOp::Ge => matches!(ord, Ordering::Greater | Ordering::Equal),
                }
            }
            Self::NotPrefix { major, minor } => {
                if version.major != *major {
                    return true;
                }
                match minor {
                    Some(m) => version.minor != *m,
                    None => false,
                }
            }
        }
    }

    fn allows_prerelease_tuple(&self, version: &SemverVersion) -> bool {
        match self {
            Self::Rel {
                version: bound,
                explicit_pre: true,
                ..
            } => {
                bound.major == version.major
                    && bound.minor == version.minor
                    && bound.patch == version.patch
            }
            _ => false,
        }
    }
}

impl SemverRange {
    /// Return the dialect this range was parsed under.
    #[must_use]
    pub const fn dialect(&self) -> SemverDialect {
        self.dialect
    }

    /// Evaluate whether concrete `version` satisfies this range.
    ///
    /// When `include_prerelease` is `false`, a prerelease version matches a
    /// clause only if every comparator in the clause succeeds AND at least one
    /// explicit comparator in that clause has the exact same
    /// `(major, minor, patch)` tuple with a prerelease tag.
    #[must_use]
    pub fn matches(&self, version: &SemverVersion, include_prerelease: bool) -> bool {
        self.clauses.iter().any(|clause| {
            if !clause
                .iter()
                .all(|comp| comp.matches(version, include_prerelease))
            {
                return false;
            }
            if !version.is_prerelease() || include_prerelease {
                return true;
            }
            clause
                .iter()
                .any(|comp| comp.allows_prerelease_tuple(version))
        })
    }
}

/// Parse a concrete semantic version, normalizing partial versions (`22` ->
/// `22.0.0`, `22.13` -> `22.13.0`) and stripping an optional leading `v`/`V`.
///
/// # Errors
///
/// Returns a descriptive error string when `raw` is empty, exceeds
/// [`MAX_VERSION_BYTES`], carries unsupported PEP 440 core markers (`!`,
/// `.post`, `.dev`), or is not valid `MAJOR[.MINOR[.PATCH[-PRE][+BUILD]]]`.
pub fn parse_version(raw: &str) -> Result<SemverVersion, String> {
    if raw.is_empty() {
        return Err("empty version string".to_string());
    }
    if raw.len() > MAX_VERSION_BYTES {
        return Err(format!(
            "version exceeds maximum supported length ({MAX_VERSION_BYTES} bytes)"
        ));
    }
    if raw.bytes().any(|b| b.is_ascii_whitespace()) {
        return Err("version must not contain whitespace".to_string());
    }
    // Check PEP 440 markers only on the core segment (before `-pre` and `+build`)
    // so valid SemVer 2.0.0 tags like `1.0.0-alpha.dev.1` or `1.0.0+build.postcss`
    // are accepted while `1!2.0.0`, `3.10.post1`, and `3.10.dev0` fail loudly.
    let without_v = match raw.as_bytes().first() {
        Some(b'v' | b'V') if raw.len() > 1 => &raw[1..],
        _ => raw,
    };
    let core_part = without_v
        .split_once('+')
        .map_or(without_v, |(h, _)| h)
        .split_once('-')
        .map_or_else(
            || without_v.split_once('+').map_or(without_v, |(h, _)| h),
            |(h, _)| h,
        );
    reject_unsupported_pep440_core_markers(core_part, raw)?;
    let partial = parse_partial(raw, false)?;
    let Some(major) = partial.major else {
        return Err("concrete version cannot be a wildcard".to_string());
    };
    Ok(SemverVersion {
        major,
        minor: partial.minor.unwrap_or(0),
        patch: partial.patch.unwrap_or(0),
        pre: partial.pre,
    })
}

/// Parse a semver range expression under `dialect`.
///
/// # Errors
///
/// Returns a descriptive error string when `raw` exceeds [`MAX_RANGE_BYTES`],
/// exceeds clause/comparator limits, uses syntax invalid for `dialect`, or
/// contains unsupported PEP 440 features.
pub fn parse_range(raw: &str, dialect: SemverDialect) -> Result<SemverRange, String> {
    if raw.len() > MAX_RANGE_BYTES {
        return Err(format!(
            "range exceeds maximum supported length ({MAX_RANGE_BYTES} bytes)"
        ));
    }
    let clauses = match dialect {
        SemverDialect::Node => parse_node_range(raw)?,
        SemverDialect::Cargo => parse_cargo_range(raw)?,
        SemverDialect::Pep440Basic => parse_pep440_range(raw)?,
    };
    Ok(SemverRange { dialect, clauses })
}

#[derive(Debug, Clone)]
struct PartialVersion {
    major: Option<u64>,
    minor: Option<u64>,
    patch: Option<u64>,
    pre: Vec<PrereleaseId>,
    /// True when any segment was explicitly written as a wildcard (`*`/`x`/`X`).
    had_wildcard: bool,
}

fn is_wildcard(part: &str) -> bool {
    matches!(part, "*" | "x" | "X")
}

fn parse_u64_strict(part: &str, label: &str) -> Result<u64, String> {
    if part.is_empty() {
        return Err(format!("empty {label} version segment"));
    }
    if !part.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("invalid {label} version segment `{part}`"));
    }
    if part.len() > 1 && part.starts_with('0') {
        return Err(format!(
            "leading zero in {label} version segment `{part}` is not valid semver"
        ));
    }
    part.parse::<u64>()
        .map_err(|_| format!("{label} version segment `{part}` overflows 64-bit integer"))
}

fn validate_build_metadata(build: &str) -> Result<(), String> {
    if build.is_empty() {
        return Err("empty build metadata after `+`".to_string());
    }
    for segment in build.split('.') {
        if segment.is_empty()
            || !segment
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(format!("invalid build metadata identifier `{segment}`"));
        }
    }
    Ok(())
}

fn parse_prerelease(pre: &str) -> Result<Vec<PrereleaseId>, String> {
    if pre.is_empty() {
        return Err("empty prerelease tag after `-`".to_string());
    }
    let mut out = Vec::new();
    for segment in pre.split('.') {
        if segment.is_empty()
            || !segment
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(format!("invalid prerelease identifier `{segment}`"));
        }
        if segment.bytes().all(|b| b.is_ascii_digit()) {
            if segment.len() > 1 && segment.starts_with('0') {
                return Err(format!(
                    "numeric prerelease identifier `{segment}` must not have a leading zero"
                ));
            }
            let num = segment
                .parse::<u64>()
                .map_err(|_| format!("prerelease identifier `{segment}` overflows 64-bit"))?;
            out.push(PrereleaseId::Numeric(num));
        } else {
            out.push(PrereleaseId::AlphaNum(segment.to_string()));
        }
    }
    Ok(out)
}

fn parse_partial(raw: &str, allow_wildcards: bool) -> Result<PartialVersion, String> {
    parse_partial_with(raw, allow_wildcards, true)
}

fn parse_partial_with(
    raw: &str,
    allow_wildcards: bool,
    allow_leading_v: bool,
) -> Result<PartialVersion, String> {
    if raw.is_empty() {
        return Err("expected a version".to_string());
    }
    let stripped = match raw.as_bytes().first() {
        Some(b'v' | b'V') if allow_leading_v && raw.len() > 1 => &raw[1..],
        _ => raw,
    };
    let (without_build, build) = match stripped.split_once('+') {
        Some((head, tail)) => (head, Some(tail)),
        None => (stripped, None),
    };
    let (core, pre_str) = match without_build.split_once('-') {
        Some((head, tail)) => (head, Some(tail)),
        None => (without_build, None),
    };
    if let Some(build) = build {
        validate_build_metadata(build)?;
    }
    let pre = match pre_str {
        Some(pre) => parse_prerelease(pre)?,
        None => Vec::new(),
    };

    let parts: Vec<&str> = core.split('.').collect();
    if parts.is_empty() || parts.len() > 3 {
        return Err(format!(
            "invalid version `{raw}`: expected 1 to 3 dot-separated numeric parts"
        ));
    }
    let mut major = None;
    let mut minor = None;
    let mut patch = None;
    let mut saw_wildcard = false;

    for (idx, part) in parts.iter().enumerate() {
        if is_wildcard(part) {
            if !allow_wildcards {
                return Err(format!(
                    "wildcard `{part}` is not allowed in concrete version `{raw}`"
                ));
            }
            saw_wildcard = true;
            continue;
        }
        if saw_wildcard {
            return Err(format!(
                "invalid version `{raw}`: numeric segment `{part}` cannot follow a wildcard"
            ));
        }
        let label = match idx {
            0 => "major",
            1 => "minor",
            _ => "patch",
        };
        let value = parse_u64_strict(part, label)?;
        match idx {
            0 => major = Some(value),
            1 => minor = Some(value),
            _ => patch = Some(value),
        }
    }

    if (!pre.is_empty() || build.is_some()) && patch.is_none() {
        return Err(format!(
            "invalid version `{raw}`: prerelease or build metadata requires MAJOR.MINOR.PATCH"
        ));
    }

    Ok(PartialVersion {
        major,
        minor,
        patch,
        pre,
        had_wildcard: saw_wildcard,
    })
}

fn checked_inc(value: u64, what: &str) -> Result<u64, String> {
    value
        .checked_add(1)
        .ok_or_else(|| format!("{what} version segment overflow"))
}

fn upper_exclusive(major: u64, minor: u64, patch: u64) -> Comparator {
    Comparator::Rel {
        op: CompOp::Lt,
        version: SemverVersion::prerelease_floor(major, minor, patch),
        explicit_pre: false,
        pre_floor_when_included: false,
    }
}

fn rel(op: CompOp, version: SemverVersion, explicit_pre: bool) -> Comparator {
    Comparator::Rel {
        op,
        version,
        explicit_pre,
        pre_floor_when_included: false,
    }
}

fn rel_ge_pre_floor(major: u64, minor: u64, patch: u64) -> Comparator {
    Comparator::Rel {
        op: CompOp::Ge,
        version: SemverVersion::release(major, minor, patch),
        explicit_pre: false,
        pre_floor_when_included: true,
    }
}

fn push_bounded(out: &mut Vec<Comparator>, comp: Comparator) -> Result<(), String> {
    if out.len() >= MAX_COMPARATORS_PER_CLAUSE {
        return Err(format!(
            "range clause exceeds maximum of {MAX_COMPARATORS_PER_CLAUSE} comparators"
        ));
    }
    out.push(comp);
    Ok(())
}

// ---------------------------------------------------------------------------
// Node (npm / node-semver) dialect
// ---------------------------------------------------------------------------

fn parse_node_range(raw: &str) -> Result<Vec<Vec<Comparator>>, String> {
    if raw.contains(',') {
        return Err(
            "commas are not valid in `node` semver ranges; separate comparators with spaces \
             (or set `dialect: cargo` or `dialect: pep440-basic`)"
                .to_string(),
        );
    }
    let trimmed = raw.trim_ascii();
    if trimmed.is_empty() {
        return Ok(vec![vec![Comparator::Any]]);
    }
    let mut clauses = Vec::new();
    for clause_src in trimmed.split("||") {
        if clauses.len() >= MAX_CLAUSES {
            return Err(format!(
                "range exceeds maximum of {MAX_CLAUSES} `||` clauses"
            ));
        }
        let clause_trimmed = clause_src.trim_ascii();
        if clause_trimmed.is_empty() {
            return Err("empty clause around `||` in semver range".to_string());
        }
        clauses.push(parse_node_clause(clause_trimmed)?);
    }
    Ok(clauses)
}

fn parse_node_clause(clause: &str) -> Result<Vec<Comparator>, String> {
    let tokens: Vec<&str> = clause.split_ascii_whitespace().collect();
    if tokens.contains(&"-") {
        if tokens.len() != 3 || tokens[1] != "-" {
            return Err(format!(
                "invalid hyphen range `{clause}`: expected `<low> - <high>`"
            ));
        }
        return desugar_hyphen(tokens[0], tokens[2]);
    }

    let mut out = Vec::new();
    let mut idx = 0;
    while idx < tokens.len() {
        let token = tokens[idx];
        let (op, version_str) = if matches!(token, "=" | ">" | ">=" | "<" | "<=" | "^" | "~") {
            idx += 1;
            let Some(&next) = tokens.get(idx) else {
                return Err(format!("dangling operator `{token}` in range `{clause}`"));
            };
            (Some(token), next)
        } else {
            split_node_operator(token)
        };
        idx += 1;
        desugar_node_comparator(op, version_str, &mut out)?;
    }
    Ok(out)
}

fn split_node_operator(token: &str) -> (Option<&'static str>, &str) {
    for prefix in [">=", "<=", ">", "<", "=", "^", "~"] {
        if let Some(rest) = token.strip_prefix(prefix) {
            return (Some(prefix), rest);
        }
    }
    (None, token)
}

fn desugar_hyphen(lhs_raw: &str, rhs_raw: &str) -> Result<Vec<Comparator>, String> {
    let lhs = parse_partial(lhs_raw, true)?;
    let rhs = parse_partial(rhs_raw, true)?;
    let mut out = Vec::new();

    if let Some(maj) = lhs.major {
        let explicit_pre = !lhs.pre.is_empty();
        let ver = SemverVersion {
            major: maj,
            minor: lhs.minor.unwrap_or(0),
            patch: lhs.patch.unwrap_or(0),
            pre: lhs.pre,
        };
        push_bounded(
            &mut out,
            Comparator::Rel {
                op: CompOp::Ge,
                version: ver,
                explicit_pre,
                pre_floor_when_included: !explicit_pre,
            },
        )?;
    } else {
        push_bounded(&mut out, Comparator::Any)?;
    }

    match (rhs.major, rhs.minor, rhs.patch) {
        (None, _, _) => {}
        (Some(maj), None, _) => {
            push_bounded(&mut out, upper_exclusive(checked_inc(maj, "major")?, 0, 0))?;
        }
        (Some(maj), Some(min), None) => {
            push_bounded(
                &mut out,
                upper_exclusive(maj, checked_inc(min, "minor")?, 0),
            )?;
        }
        (Some(maj), Some(min), Some(pat)) => {
            let explicit_pre = !rhs.pre.is_empty();
            let ver = SemverVersion {
                major: maj,
                minor: min,
                patch: pat,
                pre: rhs.pre,
            };
            push_bounded(&mut out, rel(CompOp::Le, ver, explicit_pre))?;
        }
    }
    Ok(out)
}

fn full_version(major: u64, minor: u64, patch: u64, pre: Vec<PrereleaseId>) -> SemverVersion {
    SemverVersion {
        major,
        minor,
        patch,
        pre,
    }
}

fn desugar_node_eq(p: PartialVersion, out: &mut Vec<Comparator>) -> Result<(), String> {
    let explicit_pre = !p.pre.is_empty();
    match (p.major, p.minor, p.patch) {
        (None, _, _) => push_bounded(out, Comparator::Any),
        (Some(maj), None, _) => {
            push_bounded(out, rel_ge_pre_floor(maj, 0, 0))?;
            push_bounded(out, upper_exclusive(checked_inc(maj, "major")?, 0, 0))
        }
        (Some(maj), Some(min), None) => {
            push_bounded(out, rel_ge_pre_floor(maj, min, 0))?;
            push_bounded(out, upper_exclusive(maj, checked_inc(min, "minor")?, 0))
        }
        (Some(maj), Some(min), Some(pat)) => push_bounded(
            out,
            rel(CompOp::Eq, full_version(maj, min, pat, p.pre), explicit_pre),
        ),
    }
}

fn desugar_node_ordered(
    op: &str,
    p: PartialVersion,
    out: &mut Vec<Comparator>,
) -> Result<(), String> {
    let explicit_pre = !p.pre.is_empty();
    match (op, p.major, p.minor, p.patch) {
        (">" | "<", None, _, _) => push_bounded(out, Comparator::Never),
        (">=" | "<=", None, _, _) => push_bounded(out, Comparator::Any),
        (">", Some(maj), None, _) => {
            push_bounded(out, rel_ge_pre_floor(checked_inc(maj, "major")?, 0, 0))
        }
        (">", Some(maj), Some(min), None) => {
            push_bounded(out, rel_ge_pre_floor(maj, checked_inc(min, "minor")?, 0))
        }
        (">", Some(maj), Some(min), Some(pat)) => push_bounded(
            out,
            rel(CompOp::Gt, full_version(maj, min, pat, p.pre), explicit_pre),
        ),
        (">=", Some(maj), min, pat) => push_bounded(
            out,
            Comparator::Rel {
                op: CompOp::Ge,
                version: full_version(maj, min.unwrap_or(0), pat.unwrap_or(0), p.pre),
                explicit_pre,
                pre_floor_when_included: pat.is_none() && !explicit_pre,
            },
        ),
        ("<", Some(maj), None, _) => push_bounded(out, upper_exclusive(maj, 0, 0)),
        ("<", Some(maj), Some(min), None) => push_bounded(out, upper_exclusive(maj, min, 0)),
        ("<", Some(maj), Some(min), Some(pat)) => push_bounded(
            out,
            rel(CompOp::Lt, full_version(maj, min, pat, p.pre), explicit_pre),
        ),
        ("<=", Some(maj), None, _) => {
            push_bounded(out, upper_exclusive(checked_inc(maj, "major")?, 0, 0))
        }
        ("<=", Some(maj), Some(min), None) => {
            push_bounded(out, upper_exclusive(maj, checked_inc(min, "minor")?, 0))
        }
        ("<=", Some(maj), Some(min), Some(pat)) => push_bounded(
            out,
            rel(CompOp::Le, full_version(maj, min, pat, p.pre), explicit_pre),
        ),
        (other, ..) => Err(format!("unsupported comparator operator `{other}`")),
    }
}

fn desugar_node_comparator(
    op: Option<&str>,
    version_str: &str,
    out: &mut Vec<Comparator>,
) -> Result<(), String> {
    let p = parse_partial(version_str, true)?;
    match op {
        None | Some("=") => desugar_node_eq(p, out),
        Some("~") => desugar_tilde(p, out),
        Some("^") => desugar_caret(p, out),
        Some(other) => desugar_node_ordered(other, p, out),
    }
}

fn desugar_tilde(p: PartialVersion, out: &mut Vec<Comparator>) -> Result<(), String> {
    let explicit_pre = !p.pre.is_empty();
    match (p.major, p.minor, p.patch) {
        (None, _, _) => push_bounded(out, Comparator::Any),
        (Some(maj), None, _) => {
            push_bounded(
                out,
                rel(CompOp::Ge, SemverVersion::release(maj, 0, 0), false),
            )?;
            push_bounded(out, upper_exclusive(checked_inc(maj, "major")?, 0, 0))
        }
        (Some(maj), Some(min), pat) => {
            push_bounded(
                out,
                rel(
                    CompOp::Ge,
                    SemverVersion {
                        major: maj,
                        minor: min,
                        patch: pat.unwrap_or(0),
                        pre: p.pre,
                    },
                    explicit_pre,
                ),
            )?;
            push_bounded(out, upper_exclusive(maj, checked_inc(min, "minor")?, 0))
        }
    }
}

fn desugar_caret(p: PartialVersion, out: &mut Vec<Comparator>) -> Result<(), String> {
    let explicit_pre = !p.pre.is_empty();
    match (p.major, p.minor, p.patch) {
        (None, _, _) => push_bounded(out, Comparator::Any),
        (Some(maj), None, _) => {
            push_bounded(out, rel_ge_pre_floor(maj, 0, 0))?;
            push_bounded(out, upper_exclusive(checked_inc(maj, "major")?, 0, 0))
        }
        (Some(maj), Some(min), None) => {
            push_bounded(out, rel_ge_pre_floor(maj, min, 0))?;
            if maj > 0 {
                push_bounded(out, upper_exclusive(checked_inc(maj, "major")?, 0, 0))
            } else {
                push_bounded(out, upper_exclusive(0, checked_inc(min, "minor")?, 0))
            }
        }
        (Some(maj), Some(min), Some(pat)) => {
            push_bounded(
                out,
                rel(
                    CompOp::Ge,
                    SemverVersion {
                        major: maj,
                        minor: min,
                        patch: pat,
                        pre: p.pre,
                    },
                    explicit_pre,
                ),
            )?;
            if maj > 0 {
                push_bounded(out, upper_exclusive(checked_inc(maj, "major")?, 0, 0))
            } else if min > 0 {
                push_bounded(out, upper_exclusive(0, checked_inc(min, "minor")?, 0))
            } else {
                push_bounded(out, upper_exclusive(0, 0, checked_inc(pat, "patch")?))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Cargo VersionReq dialect
// ---------------------------------------------------------------------------

fn parse_cargo_range(raw: &str) -> Result<Vec<Vec<Comparator>>, String> {
    if raw.contains("||") {
        return Err(
            "`||` is not valid in `cargo` version requirements; use comma-separated comparators \
             (or `dialect: node`)"
                .to_string(),
        );
    }
    let trimmed = raw.trim_ascii();
    if trimmed.is_empty() {
        return Err("empty `cargo` version requirement".to_string());
    }
    let segments: Vec<&str> = trimmed.split(',').collect();
    let mut out = Vec::new();
    for segment in &segments {
        let item = segment.trim_ascii();
        if item.is_empty() {
            return Err(format!("empty comparator in `cargo` range `{raw}`"));
        }
        let (op, rest) = split_node_operator(item);
        let version_str = rest.trim_ascii_start();
        if version_str.is_empty() || version_str.bytes().any(|b| b.is_ascii_whitespace()) {
            return Err(format!(
                "invalid comparator `{item}` in `cargo` range `{raw}`"
            ));
        }
        let p = parse_partial_with(version_str, true, false)?;
        if p.major.is_none() {
            if version_str.contains('.') {
                return Err(format!(
                    "invalid wildcard `{version_str}` in `cargo` range `{raw}`"
                ));
            }
            if op.is_some() {
                return Err(format!(
                    "wildcard `{version_str}` cannot be combined with operator in `cargo` range \
                     `{raw}`"
                ));
            }
            if segments.len() > 1 {
                return Err(format!(
                    "wildcard `*` must be the sole comparator in `cargo` range `{raw}`"
                ));
            }
            push_bounded(&mut out, Comparator::Any)?;
            continue;
        }
        match op {
            None => {
                // In Cargo `VersionReq`, a bare version without wildcard is
                // caret (`1.2.3` == `^1.2.3`), whereas a wildcard (`1.*`,
                // `1.2.*`) matches that prefix.
                if p.had_wildcard {
                    desugar_node_eq(p, &mut out)?;
                } else {
                    desugar_caret(p, &mut out)?;
                }
            }
            Some("=") => desugar_node_eq(p, &mut out)?,
            Some("~") => desugar_tilde(p, &mut out)?,
            Some("^") => desugar_caret(p, &mut out)?,
            Some(other) => desugar_node_ordered(other, p, &mut out)?,
        }
    }
    Ok(vec![out])
}

// ---------------------------------------------------------------------------
// PEP 440 `requires-python` basic dialect
// ---------------------------------------------------------------------------

fn reject_unsupported_pep440_core_markers(core: &str, full: &str) -> Result<(), String> {
    if core.contains('!') {
        return Err(format!(
            "unsupported PEP 440 epoch in `{full}` (epochs `N!` are not supported in \
             `pep440-basic`)"
        ));
    }
    let lower = core.to_ascii_lowercase();
    if lower.contains(".post") {
        return Err(format!(
            "unsupported PEP 440 post-release in `{full}` (`.post` releases are not supported in \
             `pep440-basic`)"
        ));
    }
    if lower.contains(".dev") {
        return Err(format!(
            "unsupported PEP 440 dev-release in `{full}` (`.dev` releases are not supported in \
             `pep440-basic`)"
        ));
    }
    Ok(())
}

fn reject_unsupported_pep440_range_token(token: &str) -> Result<(), String> {
    if token.contains('+') {
        return Err(format!(
            "unsupported PEP 440 local version `+` in `{token}` (`+local` versions are not \
             supported in `pep440-basic`)"
        ));
    }
    if token.contains('!') {
        return Err(format!(
            "unsupported PEP 440 epoch in `{token}` (epochs `N!` are not supported in \
             `pep440-basic`)"
        ));
    }
    let lower = token.to_ascii_lowercase();
    if lower.contains(".post") || lower.contains("-post") {
        return Err(format!(
            "unsupported PEP 440 post-release in `{token}` (post-releases are not supported in \
             `pep440-basic`)"
        ));
    }
    if lower.contains(".dev") || lower.contains("-dev") {
        return Err(format!(
            "unsupported PEP 440 dev-release in `{token}` (dev-releases are not supported in \
             `pep440-basic`)"
        ));
    }
    Ok(())
}

fn parse_pep440_range(raw: &str) -> Result<Vec<Vec<Comparator>>, String> {
    if raw.contains("||") {
        return Err(
            "`||` is not valid in `pep440-basic` version specifiers; separate comparators with \
             commas"
                .to_string(),
        );
    }
    let trimmed = raw.trim_ascii();
    if trimmed.is_empty() {
        return Err("empty `pep440-basic` version specifier".to_string());
    }
    let mut out = Vec::new();
    for segment in trimmed.split(',') {
        let item = segment.trim_ascii();
        if item.is_empty() {
            return Err(format!("empty comparator in `pep440-basic` range `{raw}`"));
        }
        if item.starts_with("===") {
            return Err(format!(
                "unsupported PEP 440 arbitrary equality `===` in `{item}`"
            ));
        }
        let (op, rest) = split_pep440_operator(item).ok_or_else(|| {
            format!(
                "missing or unsupported operator in `pep440-basic` comparator `{item}`; expected \
                 one of `>=`, `<=`, `>`, `<`, `==`, `!=`, `~=`"
            )
        })?;
        let version_str = rest.trim_ascii_start();
        if version_str.is_empty() || version_str.bytes().any(|b| b.is_ascii_whitespace()) {
            return Err(format!(
                "invalid version `{version_str}` in `pep440-basic` comparator `{item}`"
            ));
        }
        reject_unsupported_pep440_range_token(version_str)?;
        desugar_pep440_comparator(op, version_str, &mut out)?;
    }
    Ok(vec![out])
}

fn split_pep440_operator(item: &str) -> Option<(&'static str, &str)> {
    for prefix in [">=", "<=", "==", "!=", "~=", ">", "<"] {
        if let Some(rest) = item.strip_prefix(prefix) {
            return Some((prefix, rest));
        }
    }
    None
}

fn validate_pep440_wildcard_syntax(version_str: &str, allow_wildcards: bool) -> Result<(), String> {
    let stripped = match version_str.as_bytes().first() {
        Some(b'v' | b'V') if version_str.len() > 1 => &version_str[1..],
        _ => version_str,
    };
    let core = stripped.split_once('-').map_or(stripped, |(h, _)| h);
    let parts: Vec<&str> = core.split('.').collect();
    for (idx, part) in parts.iter().enumerate() {
        if matches!(*part, "x" | "X") {
            return Err(format!(
                "invalid wildcard `{part}` in `pep440-basic` version `{version_str}`; PEP 440 \
                 uses trailing `.*` only"
            ));
        }
        if *part == "*" {
            if !allow_wildcards {
                return Err(format!(
                    "wildcard `*` is only allowed with `==` and `!=` in `pep440-basic` \
                     (`{version_str}`)"
                ));
            }
            if idx == 0 || idx + 1 != parts.len() {
                return Err(format!(
                    "invalid wildcard placement in `pep440-basic` version `{version_str}`; \
                     expected `MAJOR.*` or `MAJOR.MINOR.*`"
                ));
            }
        }
    }
    Ok(())
}

fn desugar_pep440_comparator(
    op: &str,
    version_str: &str,
    out: &mut Vec<Comparator>,
) -> Result<(), String> {
    let allow_wildcards = matches!(op, "==" | "!=");
    validate_pep440_wildcard_syntax(version_str, allow_wildcards)?;
    let p = parse_partial(version_str, allow_wildcards)?;
    let explicit_pre = !p.pre.is_empty();
    match op {
        "~=" => {
            // PEP 440 compatible release requires at least MAJOR.MINOR.
            let (Some(maj), Some(min)) = (p.major, p.minor) else {
                return Err(format!(
                    "`~={version_str}` requires at least MAJOR.MINOR in `pep440-basic`"
                ));
            };
            let lower = SemverVersion {
                major: maj,
                minor: min,
                patch: p.patch.unwrap_or(0),
                pre: p.pre,
            };
            push_bounded(out, rel(CompOp::Ge, lower, explicit_pre))?;
            if p.patch.is_some() {
                push_bounded(out, upper_exclusive(maj, checked_inc(min, "minor")?, 0))
            } else {
                push_bounded(out, upper_exclusive(checked_inc(maj, "major")?, 0, 0))
            }
        }
        "==" => {
            if p.had_wildcard {
                let maj = p.major.expect("PEP 440 wildcard requires numeric major");
                match p.minor {
                    None => {
                        push_bounded(out, rel_ge_pre_floor(maj, 0, 0))?;
                        push_bounded(out, upper_exclusive(checked_inc(maj, "major")?, 0, 0))
                    }
                    Some(min) => {
                        push_bounded(out, rel_ge_pre_floor(maj, min, 0))?;
                        push_bounded(out, upper_exclusive(maj, checked_inc(min, "minor")?, 0))
                    }
                }
            } else {
                let maj = p.major.expect("no wildcard means major is present");
                let ver = SemverVersion {
                    major: maj,
                    minor: p.minor.unwrap_or(0),
                    patch: p.patch.unwrap_or(0),
                    pre: p.pre,
                };
                push_bounded(out, rel(CompOp::Eq, ver, explicit_pre))
            }
        }
        "!=" => {
            if p.had_wildcard {
                let maj = p.major.expect("PEP 440 wildcard requires numeric major");
                push_bounded(
                    out,
                    Comparator::NotPrefix {
                        major: maj,
                        minor: p.minor,
                    },
                )
            } else {
                let maj = p.major.expect("no wildcard means major is present");
                let ver = SemverVersion {
                    major: maj,
                    minor: p.minor.unwrap_or(0),
                    patch: p.patch.unwrap_or(0),
                    pre: p.pre,
                };
                push_bounded(out, rel(CompOp::Ne, ver, explicit_pre))
            }
        }
        ">=" | "<=" | ">" | "<" => {
            let maj = p.major.expect("wildcards rejected for ordered PEP 440 ops");
            let ver = SemverVersion {
                major: maj,
                minor: p.minor.unwrap_or(0),
                patch: p.patch.unwrap_or(0),
                pre: p.pre,
            };
            if op == "<" && !explicit_pre {
                // PEP 440 exclusive ordered `< V` must not match prereleases of V
                // even when prereleases are enabled.
                return push_bounded(out, upper_exclusive(ver.major, ver.minor, ver.patch));
            }
            let comp_op = match op {
                ">=" => CompOp::Ge,
                "<=" => CompOp::Le,
                ">" => CompOp::Gt,
                _ => CompOp::Lt,
            };
            push_bounded(out, rel(comp_op, ver, explicit_pre))
        }
        other => Err(format!("unsupported `pep440-basic` operator `{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn satisfies(ver: &str, range: &str, dialect: SemverDialect, include_pre: bool) -> bool {
        let v = parse_version(ver).unwrap_or_else(|e| panic!("version {ver:?}: {e}"));
        let r = parse_range(range, dialect).unwrap_or_else(|e| panic!("range {range:?}: {e}"));
        r.matches(&v, include_pre)
    }

    #[test]
    fn concrete_version_parsing_and_semver_precedence() {
        assert_eq!(
            parse_version("22").unwrap(),
            SemverVersion::release(22, 0, 0)
        );
        assert_eq!(
            parse_version("v22.13").unwrap(),
            SemverVersion::release(22, 13, 0)
        );
        assert_eq!(
            parse_version("V22.0.0+build.123").unwrap(),
            parse_version("22").unwrap()
        );
        // SemVer 2.0.0 section 11 precedence chain.
        let ordered = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.1.0",
            "2.0.0",
        ];
        for pair in ordered.windows(2) {
            let a = parse_version(pair[0]).unwrap();
            let b = parse_version(pair[1]).unwrap();
            assert!(a < b, "expected {} < {}", pair[0], pair[1]);
        }
    }

    #[test]
    fn node_semver_differential_vectors() {
        let cases = [
            // Basic comparators, empty, *, x-ranges, partials
            ("22.13.0", "", false, true),
            ("22.13.0", "*", false, true),
            ("22.13.0", ">=20 <23", false, true),
            ("23.0.0", ">=20 <23", false, false),
            ("22.13.0", "=v22.13.0", false, true),
            ("22.13.0", "22.x", false, true),
            ("23.0.0", "22.x", false, false),
            ("22.13.5", "22.13.X", false, true),
            // Caret and tilde
            ("22.13.0", "^22.0.0", false, true),
            ("23.0.0", "^22.0.0", false, false),
            ("0.2.5", "^0.2.3", false, true),
            ("0.3.0", "^0.2.3", false, false),
            ("0.0.3", "^0.0.3", false, true),
            ("0.0.4", "^0.0.3", false, false),
            ("22.13.9", "~22.13", false, true),
            ("22.14.0", "~22.13", false, false),
            ("22.5.0", "~22", false, true),
            ("23.0.0", "~22", false, false),
            // Hyphen and || unions
            ("1.5.0", "1.2.3 - 2.3.4", false, true),
            ("2.3.4", "1.2.3 - 2.3.4", false, true),
            ("2.3.5", "1.2.3 - 2.3.4", false, false),
            ("2.3.9", "1.2 - 2.3", false, true),
            ("2.4.0", "1.2 - 2.3", false, false),
            ("18.20.0", "^18.18.0 || >=20.0.0", false, true),
            ("19.0.0", "^18.18.0 || >=20.0.0", false, false),
            ("22.0.0", "^18.18.0 || >=20.0.0", false, true),
            // Strict node-semver prerelease rules
            ("22.0.0-rc.1", ">=20.0.0", false, false),
            ("22.0.0-rc.1", ">=20.0.0", true, true),
            ("22.0.0-rc.2", ">=22.0.0-rc.1 <23.0.0", false, true),
            ("22.0.1-rc.1", ">=22.0.0-rc.1 <23.0.0", false, false),
            ("22.0.1-rc.1", ">=22.0.0-rc.1 <23.0.0", true, true),
            // Upper exclusive bound on caret rejects next-major prereleases even with include_prerelease
            ("2.0.0-alpha.1", "^1.2.3", true, false),
            ("2.0.0-alpha.1", "^1.2.3", false, false),
            ("1.2.4-alpha.1", "^1.2.3", true, true),
            ("1.2.4-alpha.1", "^1.2.3", false, false),
            // Spaces after operator
            ("22.1.0", ">= 20.0.0 < 23.0.0", false, true),
        ];
        for (ver, range, pre, want) in cases {
            assert_eq!(
                satisfies(ver, range, SemverDialect::Node, pre),
                want,
                "node: ver={ver} range={range} pre={pre}"
            );
        }
    }

    #[test]
    fn cargo_version_req_vectors() {
        let cases = [
            ("1.5.0", "1.2.3", false, true),
            ("2.0.0", "1.2.3", false, false),
            ("0.2.9", "^0.2.3", false, true),
            ("0.3.0", "^0.2.3", false, false),
            ("0.0.3", "^0.0.3", false, true),
            ("0.0.4", "^0.0.3", false, false),
            ("0.0.9", "^0.0", false, true),
            ("0.1.0", "^0.0", false, false),
            ("1.9.0", "~1", false, true),
            ("2.0.0", "~1", false, false),
            ("1.2.9", "~1.2", false, true),
            ("1.3.0", "~1.2", false, false),
            ("1.88.0", ">=1.85.0, <2.0.0", false, true),
            ("1.84.0", ">=1.85.0, <2.0.0", false, false),
            ("1.2.3", "=1.2.3", false, true),
            ("1.2.4", "=1.2.3", false, false),
            ("1.2.9", "1.2.*", false, true),
            ("1.3.0", "1.2.*", false, false),
        ];
        for (ver, range, pre, want) in cases {
            assert_eq!(
                satisfies(ver, range, SemverDialect::Cargo, pre),
                want,
                "cargo: ver={ver} range={range} pre={pre}"
            );
        }
    }

    #[test]
    fn pep440_basic_vectors() {
        let cases = [
            ("3.12", ">=3.10,<3.13", false, true),
            ("3.13.0", ">=3.10,<3.13", false, false),
            ("3.12.4", "~=3.10", false, true),
            ("4.0.0", "~=3.10", false, false),
            ("3.10.9", "~=3.10.2", false, true),
            ("3.11.0", "~=3.10.2", false, false),
            ("3.10.5", "==3.10.*", false, true),
            ("3.11.0", "==3.10.*", false, false),
            ("3.11.0", ">=3.10,!=3.10.*", false, true),
            ("3.10.5", ">=3.10,!=3.10.*", false, false),
            ("3.10.0", "==3.10", false, true),
            ("3.10.1", "==3.10", false, false),
        ];
        for (ver, range, pre, want) in cases {
            assert_eq!(
                satisfies(ver, range, SemverDialect::Pep440Basic, pre),
                want,
                "pep440: ver={ver} range={range} pre={pre}"
            );
        }
    }

    #[test]
    fn malformed_inputs_and_dos_caps_fail_loud() {
        // Commas rejected in node dialect
        assert!(parse_range(">=20,<23", SemverDialect::Node).is_err());
        // || rejected in cargo and pep440-basic
        assert!(parse_range("^1.0 || ^2.0", SemverDialect::Cargo).is_err());
        assert!(parse_range(">=3.10 || >=3.11", SemverDialect::Pep440Basic).is_err());
        // Unsupported PEP 440 features fail loud
        assert!(parse_range(">=1!2.0", SemverDialect::Pep440Basic).is_err());
        assert!(parse_range(">=3.10.post1", SemverDialect::Pep440Basic).is_err());
        assert!(parse_range(">=3.10.dev0", SemverDialect::Pep440Basic).is_err());
        assert!(parse_range("===3.10", SemverDialect::Pep440Basic).is_err());
        assert!(parse_version("1!2.0.0").is_err());
        assert!(parse_version("3.10.post1").is_err());
        assert!(parse_version("3.10.dev0").is_err());
        // Non-semver specifiers fail loud
        for bad in [
            "",
            "latest",
            "lts/*",
            "workspace:*",
            "01.0.0",
            "1.2.3.4",
            "1.2-rc.1",
        ] {
            assert!(parse_version(bad).is_err(), "expected error for {bad:?}");
        }
        // Length cap
        let huge = format!(">={}", "1".repeat(MAX_RANGE_BYTES));
        assert!(parse_range(&huge, SemverDialect::Node).is_err());
        // Clause cap
        let many_clauses = vec!["1.0.0"; MAX_CLAUSES + 1].join(" || ");
        assert!(parse_range(&many_clauses, SemverDialect::Node).is_err());
        // Comparator cap
        let many_comps = vec![">=1.0.0"; MAX_COMPARATORS_PER_CLAUSE + 1].join(" ");
        assert!(parse_range(&many_comps, SemverDialect::Node).is_err());
        // u64::MAX overflow safety
        let max_ver = format!("^{}.0.0", u64::MAX);
        assert!(parse_range(&max_ver, SemverDialect::Node).is_err());
    }

    #[test]
    fn audit_regression_semver_prerelease_bounds() {
        // D5-H1: valid SemVer 2.0.0 with `.dev`/`.post` in prerelease or build metadata.
        for valid in [
            "1.0.0-alpha.dev.1",
            "1.0.0-0.dev",
            "1.0.0+build.dev",
            "1.0.0+build.postcss",
            "1.0.0-canary.devops",
            "1.0.0-rc.1+2026.post.3",
        ] {
            assert!(
                parse_version(valid).is_ok(),
                "valid SemVer 2.0.0 rejected: {valid}"
            );
        }
        // D5-H2: include_prerelease: true on partial/wildcard/caret/hyphen lower bounds.
        for (ver, range, dialect, pre, want) in [
            ("2.0.0-alpha.1", "^2", SemverDialect::Node, true, true),
            ("2.0.0-alpha.1", "^2.0", SemverDialect::Node, true, true),
            ("0.0.0-alpha", "^0", SemverDialect::Node, true, true),
            ("0.0.0-alpha", "^0.0", SemverDialect::Node, true, true),
            ("2.0.0-alpha.1", "2.x", SemverDialect::Node, true, true),
            ("2.0.0-alpha.1", "2", SemverDialect::Node, true, true),
            ("2.0.0-alpha.1", ">=2", SemverDialect::Node, true, true),
            ("2.0.0-alpha.1", ">1", SemverDialect::Node, true, true),
            (
                "1.2.0-alpha.1",
                "1.2 - 2.3",
                SemverDialect::Node,
                true,
                true,
            ),
            (
                "1.2.3-alpha.1",
                "1.2.3 - 2.3.4",
                SemverDialect::Node,
                true,
                true,
            ),
            // Tilde stays >=2.0.0 even with include_prerelease (node-semver parity).
            ("2.0.0-alpha.1", "~2", SemverDialect::Node, true, false),
            // Cargo wildcard lower-tuple prerelease consistency.
            ("1.2.0-alpha", "1.2.*", SemverDialect::Cargo, true, true),
            ("1.2.0-alpha", "1.2.*", SemverDialect::Cargo, false, false),
            // Cargo operator + wildcard forms supported by Cargo semver 1.0.28.
            ("1.5.0", ">=1.*", SemverDialect::Cargo, false, true),
            ("1.5.0", "=1.*", SemverDialect::Cargo, false, true),
            ("2.0.0", "=1.*", SemverDialect::Cargo, false, false),
            ("1.5.0", "^1.*", SemverDialect::Cargo, false, true),
            ("1.2.9", "~1.2.*", SemverDialect::Cargo, false, true),
            // PEP 440 ==X.* and !=X.* stay exact complements with include_prerelease.
            (
                "3.10.0-rc.1",
                "==3.10.*",
                SemverDialect::Pep440Basic,
                true,
                true,
            ),
            (
                "3.10.0-rc.1",
                "!=3.10.*",
                SemverDialect::Pep440Basic,
                true,
                false,
            ),
            // PEP 440 ~=3.10 rejects 3.10.0-rc.1 even with include_prerelease.
            (
                "3.10.0-rc.1",
                "~=3.10",
                SemverDialect::Pep440Basic,
                true,
                false,
            ),
            // D5-M1: PEP 440 `< V` rejects prereleases of V even with include_prerelease.
            (
                "3.13.0-rc.1",
                ">=3.10,<3.13",
                SemverDialect::Pep440Basic,
                true,
                false,
            ),
            (
                "3.12.9-rc.1",
                ">=3.10,<3.13",
                SemverDialect::Pep440Basic,
                true,
                true,
            ),
        ] {
            assert_eq!(
                satisfies(ver, range, dialect, pre),
                want,
                "{dialect:?}: ver={ver} range={range} pre={pre}"
            );
        }
    }

    #[test]
    fn audit_regression_semver_dialect_syntax_rejections() {
        // D5-M2: PEP 440 strict syntax rejections.
        for bad_pep in [
            "==3.x",
            "==3.10.X",
            "!=3.x",
            "==*",
            "!=*",
            "==3.*.*",
            "==*.*",
            "==v*",
            ">=3.10.0+local",
            "==3.10.0+foo",
            ">=3.10.0-post1",
            ">=3.10.0-dev0",
        ] {
            assert!(
                parse_range(bad_pep, SemverDialect::Pep440Basic).is_err(),
                "expected pep440-basic rejection for {bad_pep:?}"
            );
        }

        // D5-M3: Cargo strict rejections (leading v, non-sole *, *.*, op + *).
        for bad_cargo in [
            "v1.2.3",
            "^v1.2.3",
            "V1.2.3",
            "*, >=1.0.0",
            "1.0.0, *",
            "*.*",
            ">=*",
            "^*",
        ] {
            assert!(
                parse_range(bad_cargo, SemverDialect::Cargo).is_err(),
                "expected cargo rejection for {bad_cargo:?}"
            );
        }
    }
}
