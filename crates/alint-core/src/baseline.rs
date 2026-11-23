//! Baseline (grandfathering) primitives: the per-violation fingerprint
//! and the committed JSON-Lines baseline file.
//!
//! Design: [`docs/design/baseline.md`](../../../docs/design/baseline.md),
//! [ADR-0006](../../../docs/adr/0006-baseline-suppression.md).
//!
//! This module is the dependency-free core: the fingerprint function
//! ([`violation_fingerprint`]), the JSON-Lines baseline file
//! ([`Baseline`]), and the report-suppression transform ([`apply`]). The
//! per-violation [`Violation::baseline_key`] is set by the rules whose
//! identity isn't `(rule_id, path)`; the boundary is enforced by the
//! `coverage_audit_baseline_safety` collision-invariant in `alint-e2e`.
//! `check --baseline` wires this in between `Engine::run` and the
//! formatters (see `crates/alint/src/check.rs`).

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::report::Report;
use crate::rule::Violation;

/// The baseline file format this binary reads and writes. A file with
/// any other version is rejected (§3.9): a newer file may use a
/// fingerprint scheme this binary computes differently, so silently
/// proceeding could mis-suppress.
pub const SCHEMA_VERSION: u32 = 1;

/// Compute a violation's baseline fingerprint (64-hex SHA-256).
///
/// The hash is `sha256(lp(rule_id) ‖ lp(path) ‖ lp(discriminator))`,
/// where `lp(x)` prefixes `x` with its 4-byte little-endian length so
/// distinct component tuples cannot collide by concatenation. The
/// **discriminator** is, in priority order (see design §3.1):
///
/// 1. `baseline_key`, if the rule supplied one (its own stable identity);
/// 2. else, for a line-anchored violation, the offending line's content
///    with any trailing `\r?\n` stripped (so line *number* and CRLF↔LF
///    don't churn the baseline) — read from `file_bytes`;
/// 3. else, for a **path-bearing** violation, **empty** — so the identity is
///    `(rule_id, path)` alone (v3). The (often volatile) message is *not*
///    hashed; `path` already disambiguates a rule that emits one finding per
///    path. A rule that emits several findings per `(rule_id, path)`, or has no
///    path, must set `baseline_key` instead (enforced by the coverage audit).
/// 4. else (no path, no line, no key), the trimmed message — a last-resort
///    *anti-panic* fallback only; the audit forbids a kind from relying on it.
///
/// `line` / `column` numbers and the violation `level` are never hashed.
/// `path` is normalised to forward slashes; pass `None` for repo-level
/// violations. `file_bytes` is the content of the violation's file (only
/// consulted for case 2); pass `None` when unavailable or irrelevant.
#[must_use]
pub fn violation_fingerprint(
    rule_id: &str,
    path: Option<&Path>,
    line: Option<usize>,
    baseline_key: Option<&str>,
    message: &str,
    file_bytes: Option<&[u8]>,
) -> String {
    let path_norm: Cow<'_, [u8]> = path.map_or(Cow::Borrowed(&[]), normalize_path);

    let discriminator: &[u8] = if let Some(key) = baseline_key {
        key.as_bytes()
    } else if let Some(content) = line.and_then(|n| file_bytes.and_then(|b| offending_line(b, n))) {
        content
    } else if path.is_some() {
        // v3: path-bearing, key-less, line-less → identity is (rule_id, path).
        // The message is deliberately not hashed (it is frequently volatile);
        // multi-finding / no-path rules set `baseline_key` instead.
        b""
    } else {
        // No path, no line, no key: anti-panic fallback only.
        message.trim().as_bytes()
    };

    hash_components([rule_id.as_bytes(), path_norm.as_ref(), discriminator])
}

/// `sha256(lp(a) ‖ lp(b) ‖ lp(c))`, as 64 hex digits.
fn hash_components(components: [&[u8]; 3]) -> String {
    let mut hasher = Sha256::new();
    for component in components {
        let len = u32::try_from(component.len()).unwrap_or(u32::MAX);
        hasher.update(len.to_le_bytes());
        hasher.update(component);
    }
    to_hex(&hasher.finalize())
}

/// Fingerprint a [`Violation`] directly (a convenience over
/// [`violation_fingerprint`] that reads the violation's fields). `rule_id`
/// is passed separately because it lives on the owning `RuleResult`, not
/// the violation. `file_bytes` is the violation's file content, consulted
/// only for the line-content discriminator.
///
/// Sensitive findings always use a constant discriminator and never consult
/// their producer key, message, or offending-line bytes. The path and rule id
/// remain structural identity, while duplicate findings collapse through the
/// baseline count.
///
/// A finding whose path alint hides ([`Violation::path_is_sensitive`]) leaves
/// the path out as well (ADR-0030 decision 1): the hash is
/// `sha256(lp(rule_id) ‖ lp("\0hidden-path") ‖ lp("sensitive"))`, so a
/// baseline file, SARIF or GitLab report holding it cannot confirm a guessed
/// path. No real path contains a NUL byte, so no finding with a path shares
/// it. A rule's hidden-path findings share the one fingerprint and match by
/// count.
///
/// A finding that asks for a path-free identity
/// ([`Violation::path_free_identity`], the `constraint` kind's) hashes
/// `sha256(lp(rule_id) ‖ lp("\0constraint") ‖ lp(baseline_key))`, or
/// `"sensitive"` for the key of a sensitive finding: where it sits moves when
/// rows are added or renamed, and its key names what it is about. No path
/// starts with a NUL byte, so no finding with a path shares the form.
#[must_use]
pub fn fingerprint(rule_id: &str, v: &Violation, file_bytes: Option<&[u8]>) -> String {
    if v.path_is_sensitive() {
        return hash_components([rule_id.as_bytes(), HIDDEN_PATH, b"sensitive"]);
    }
    if v.path_free_identity {
        let key: &[u8] = if v.is_sensitive() {
            b"sensitive"
        } else {
            v.baseline_key.as_deref().unwrap_or_default().as_bytes()
        };
        return hash_components([rule_id.as_bytes(), PATH_FREE, key]);
    }
    path_identity_fingerprint(rule_id, v, file_bytes)
}

/// What stands for the path in a hidden-path finding's fingerprint.
const HIDDEN_PATH: &[u8] = b"\0hidden-path";

/// What stands for the path in the fingerprint of a finding identified by its
/// key alone ([`Violation::path_free_identity`]).
const PATH_FREE: &[u8] = b"\0constraint";

/// [`fingerprint`] as if no path were hidden: `rule_id` and path always take
/// part. The engine keys findings by it within a run, where two hidden paths
/// must stay apart. (It is also the fingerprint v0.21 wrote for a hidden-path
/// finding, which baselines stopped matching in v0.23.)
#[must_use]
pub(crate) fn path_identity_fingerprint(
    rule_id: &str,
    v: &Violation,
    file_bytes: Option<&[u8]>,
) -> String {
    if v.is_sensitive() {
        return violation_fingerprint(
            rule_id,
            v.path.as_deref(),
            None,
            Some("sensitive"),
            "",
            None,
        );
    }
    violation_fingerprint(
        rule_id,
        v.path.as_deref(),
        identity_line(v),
        v.baseline_key.as_deref(),
        &v.message,
        file_bytes,
    )
}

/// The line whose content identifies `v` in its fingerprint.
fn identity_line(v: &Violation) -> Option<usize> {
    v.range
        .as_ref()
        .map_or(v.line, crate::SourceRange::baseline_identity_line)
}

/// The file content [`fingerprint`] uses for `v`, read from under `root` once
/// per path through `cache`. `None`, without reading the file, when the
/// fingerprint leaves the content out: for a sensitive finding, one with a
/// `baseline_key`, and one without a line. `None` also for a directory (a
/// nested rule's entry) and for a file it cannot read within the analysis cap,
/// with a warning that names the file unless its path is one alint hides.
pub fn fingerprint_content<'c, S: std::hash::BuildHasher>(
    root: &Path,
    v: &Violation,
    cache: &'c mut std::collections::HashMap<std::path::PathBuf, Option<Vec<u8>>, S>,
) -> Option<&'c [u8]> {
    if v.is_sensitive()
        || v.baseline_key.is_some()
        || v.path_free_identity
        || identity_line(v).is_none()
    {
        return None;
    }
    let path = v.path.as_deref()?;
    cache
        .entry(path.to_path_buf())
        .or_insert_with(|| {
            let full = root.join(path);
            let size = std::fs::metadata(&full)
                .ok()
                .filter(std::fs::Metadata::is_file)?
                .len();
            let reason = match crate::walker::read_for_analysis(&full, size) {
                crate::walker::AnalysisRead::Bytes(bytes) => return Some(bytes),
                crate::walker::AnalysisRead::Missing => return None,
                crate::walker::AnalysisRead::TooLarge(size) => {
                    format!("it exceeds the analysis cap ({size} bytes)")
                }
                crate::walker::AnalysisRead::Unreadable(error) => error.to_string(),
            };
            let shown = if v.path_is_sensitive() {
                crate::CONFIGURATION_MASK.to_string()
            } else {
                path.display().to_string()
            };
            tracing::warn!(
                path = %shown,
                "the baseline fingerprint leaves out the content of a file it cannot read: {reason}"
            );
            None
        })
        .as_deref()
}

/// Normalise a path to stable bytes. Windows separators are rewritten to `/`;
/// Unix paths retain their exact bytes so lossy UTF-8 replacement cannot merge
/// distinct filenames. Portable UTF-8 paths fingerprint exactly as before and
/// remain identical across operating systems.
#[cfg(unix)]
fn normalize_path(path: &Path) -> Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt as _;

    Cow::Borrowed(path.as_os_str().as_bytes())
}

#[cfg(windows)]
fn normalize_path(path: &Path) -> Cow<'_, [u8]> {
    Cow::Owned(path.to_string_lossy().replace('\\', "/").into_bytes())
}

#[cfg(not(any(unix, windows)))]
fn normalize_path(path: &Path) -> Cow<'_, [u8]> {
    match path.to_string_lossy() {
        Cow::Borrowed(path) => Cow::Borrowed(path.as_bytes()),
        Cow::Owned(path) => Cow::Owned(path.into_bytes()),
    }
}

/// The bytes of 1-based line `n` in `bytes`, with a trailing `\r`
/// stripped (the `\n` is consumed by the split). `None` if the line
/// doesn't exist or `n == 0`.
fn offending_line(bytes: &[u8], n: usize) -> Option<&[u8]> {
    let idx = n.checked_sub(1)?;
    let line = bytes.split(|&b| b == b'\n').nth(idx)?;
    Some(line.strip_suffix(b"\r").unwrap_or(line))
}

fn to_hex(digest: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

/// Errors loading a baseline file.
#[derive(Debug, thiserror::Error)]
pub enum BaselineError {
    /// The file (or its header line) was empty.
    #[error("baseline file is empty; expected a header line")]
    Empty,
    /// A line was not valid JSON.
    #[error("baseline {what} is not valid JSON: {source}")]
    Parse {
        what: &'static str,
        #[source]
        source: serde_json::Error,
    },
    /// The file's `schema_version` is not one this binary understands.
    #[error(
        "baseline schema_version {found} is unsupported (this alint reads version {SCHEMA_VERSION}); \
         regenerate with `alint baseline`, or upgrade alint"
    )]
    UnsupportedSchema { found: u32 },
}

/// The header line of a baseline file.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Header {
    schema_version: u32,
    /// Advisory provenance only; excluded from matching and from
    /// byte-identical regeneration comparisons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    alint_version: Option<String>,
}

/// The canonical path-free fingerprint for a hidden-path finding of `rule_id`
/// (ADR-0030 decision 1).
fn hidden_path_fingerprint(rule_id: &str) -> String {
    hash_components([rule_id.as_bytes(), HIDDEN_PATH, b"sensitive"])
}

/// One grandfathered violation. Matching uses `fingerprint` and `count`,
/// preferring visible paths listed in `paths` when present (ADR-0049);
/// `rule_id`, `path`, and `message` are advisory — they exist so a reviewer
/// reading the file's diff can see what is being suppressed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineEntry {
    pub rule_id: String,
    #[serde(default)]
    pub path: Option<String>,
    pub fingerprint: String,
    pub count: u32,
    /// Advisory: the rendered violation message, for human review.
    /// Never matched on; may drift freely between regenerations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Optional sorted multiset of visible slash-normalized paths suppressed
    /// by this entry when a fingerprint can repeat across files (e.g.
    /// `constraint` rules; ADR-0049). Omitted for ordinary path-bound single-
    /// file entries and always empty for hidden-path entries (ADR-0030).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
}

/// A parsed baseline: an advisory header plus the suppressed entries,
/// sorted by `(rule_id, path, fingerprint)` for deterministic output.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Baseline {
    pub alint_version: Option<String>,
    pub entries: Vec<BaselineEntry>,
}

impl Baseline {
    /// Build a baseline by aggregating fingerprinted violations: entries
    /// with the same fingerprint collapse into one with a `count`. The
    /// advisory `path` and `message` are pinned to the lexicographically
    /// smallest of the group, so the file is byte-identical across
    /// regenerations regardless of the order the engine collected violations
    /// in. Non-hidden entries that span multiple visible paths or come from a
    /// `path_free_identity` finding also record their sorted visible paths in
    /// [`BaselineEntry::paths`] (ADR-0049). Output entries are sorted for a
    /// deterministic, merge-friendly file.
    pub fn from_fingerprints<I>(alint_version: Option<String>, items: I) -> Self
    where
        I: IntoIterator<Item = FingerprintedViolation>,
    {
        struct Group {
            entry: BaselineEntry,
            visible_paths: Vec<String>,
            any_path_free: bool,
        }

        let mut by_fp: BTreeMap<String, Group> = BTreeMap::new();
        for item in items {
            let is_hidden = item.fingerprint == hidden_path_fingerprint(&item.rule_id);
            let group = by_fp
                .entry(item.fingerprint.clone())
                .or_insert_with(|| Group {
                    entry: BaselineEntry {
                        rule_id: item.rule_id.clone(),
                        path: item.path.clone(),
                        fingerprint: item.fingerprint.clone(),
                        count: 0,
                        message: item.message.clone(),
                        paths: Vec::new(),
                    },
                    visible_paths: Vec::new(),
                    any_path_free: false,
                });
            group.entry.count = group.entry.count.saturating_add(1);
            if item.path < group.entry.path {
                group.entry.path.clone_from(&item.path);
            }
            if item.message < group.entry.message {
                group.entry.message = item.message;
            }
            if !is_hidden {
                group.any_path_free |= item.path_free_identity;
                if let Some(path) = item.path {
                    group.visible_paths.push(path);
                }
            }
        }
        let mut entries: Vec<BaselineEntry> = by_fp
            .into_values()
            .map(|mut group| {
                if !group.visible_paths.is_empty() {
                    group.visible_paths.sort();
                    let multiple_distinct = group
                        .visible_paths
                        .first()
                        .zip(group.visible_paths.last())
                        .is_some_and(|(first, last)| first != last);
                    if group.any_path_free || multiple_distinct {
                        group.entry.paths = group.visible_paths;
                    }
                }
                group.entry
            })
            .collect();
        entries.sort_by(|a, b| {
            a.rule_id
                .cmp(&b.rule_id)
                .then_with(|| a.path.cmp(&b.path))
                .then_with(|| a.fingerprint.cmp(&b.fingerprint))
        });
        Self {
            alint_version,
            entries,
        }
    }

    /// Render the baseline as JSON Lines: a header line followed by one
    /// sorted entry per line. One-entry-per-line keeps the committed file
    /// merge-friendly (disjoint additions don't conflict).
    #[must_use]
    pub fn to_jsonl(&self) -> String {
        let header = Header {
            schema_version: SCHEMA_VERSION,
            alint_version: self.alint_version.clone(),
        };
        let mut out = serde_json::to_string(&header).expect("header serializes");
        out.push('\n');
        for entry in &self.entries {
            out.push_str(&serde_json::to_string(entry).expect("entry serializes"));
            out.push('\n');
        }
        out
    }

    /// Read and parse the baseline file at `path`, for `check`, `fix`,
    /// `baseline` and the language server alike. Only a regular file (after
    /// symlinks) of at most [`crate::MAX_ANALYZE_BYTES`] is read: a FIFO would
    /// block the read and a device such as `/dev/zero` never ends.
    ///
    /// `repository_root` is `Some` when the path comes from the repository,
    /// a config's `baseline:` key (or the default name), rather than from the
    /// user's own `--baseline` / `--output`: the file must then resolve inside
    /// that root and outside any Git directory there
    /// ([`crate::resolve_named_read`]), as the file `alint baseline` writes
    /// there must.
    ///
    /// # Errors
    ///
    /// A message naming `path` (as given) for a file that is refused, cannot
    /// be read or does not parse.
    pub fn load_file(path: &Path, repository_root: Option<&Path>) -> Result<Self, String> {
        Self::load_file_redacted(path, repository_root, false)
    }

    /// Like [`Self::load_file`], replacing `path` with
    /// [`crate::CONFIGURATION_MASK`] in every error message when `redact_path`
    /// is true (e.g. a top-level `baseline:` value interpolated from the
    /// environment, including values shorter than four characters).
    #[doc(hidden)]
    pub fn load_file_redacted(
        path: &Path,
        repository_root: Option<&Path>,
        redact_path: bool,
    ) -> Result<Self, String> {
        let shown = if redact_path {
            std::borrow::Cow::Borrowed(crate::CONFIGURATION_MASK)
        } else {
            std::borrow::Cow::Owned(path.display().to_string())
        };
        if let Some(root) = repository_root {
            let refusal = match crate::resolve_named_read(root, path) {
                crate::NamedRead::InRoot => None,
                crate::NamedRead::GitDirectory => Some(
                    "is in a Git directory, where alint never reads; the `baseline:` key \
                     names a file in the repository",
                ),
                crate::NamedRead::OutsideRoot => Some(
                    "resolves outside the repository; the `baseline:` key names a file in \
                     the repository (`--baseline` names one anywhere)",
                ),
            };
            if let Some(refusal) = refusal {
                return Err(format!("baseline file {shown} {refusal}"));
            }
        }
        let bytes = crate::bounded_file::read_regular_capped(path, crate::MAX_ANALYZE_BYTES)
            .map_err(|e| {
                format!("reading baseline file {shown} (run `alint baseline` to create it): {e}")
            })?;
        let text = String::from_utf8(bytes).map_err(|_| {
            format!("reading baseline file {shown}: stream did not contain valid UTF-8")
        })?;
        Self::load(&text).map_err(|e| format!("invalid baseline file {shown}: {e}"))
    }

    /// Parse a baseline from JSON-Lines text. The first non-empty line is
    /// the header (its `schema_version` is gated); each remaining
    /// non-empty line is an entry.
    pub fn load(text: &str) -> Result<Self, BaselineError> {
        let mut lines = text.lines().filter(|l| !l.trim().is_empty());

        let header_line = lines.next().ok_or(BaselineError::Empty)?;
        let header: Header =
            serde_json::from_str(header_line).map_err(|source| BaselineError::Parse {
                what: "header",
                source,
            })?;
        if header.schema_version != SCHEMA_VERSION {
            return Err(BaselineError::UnsupportedSchema {
                found: header.schema_version,
            });
        }

        // Collapse duplicate fingerprints by SUMMING counts. A freshly
        // written baseline has none (`from_fingerprints` dedups), but the
        // file is line-oriented to be merge-friendly: a git merge of two
        // baselines that each recorded the same finding — or a hand-edit —
        // can leave two entries with one fingerprint. Summing keeps the
        // suppression budget the total (not the last writer's) and gives
        // `apply`/`stale` the one-entry-per-fingerprint invariant they
        // assume. First-appearance order and the first entry's advisory
        // fields win.
        let mut entries: Vec<BaselineEntry> = Vec::new();
        let mut pos: BTreeMap<String, usize> = BTreeMap::new();
        for line in lines {
            let entry: BaselineEntry =
                serde_json::from_str(line).map_err(|source| BaselineError::Parse {
                    what: "entry",
                    source,
                })?;
            if let Some(&i) = pos.get(&entry.fingerprint) {
                entries[i].count = entries[i].count.saturating_add(entry.count);
                if !entry.paths.is_empty()
                    && entry.fingerprint != hidden_path_fingerprint(&entries[i].rule_id)
                {
                    entries[i].paths.extend(entry.paths);
                    entries[i].paths.sort();
                }
            } else {
                let mut entry = entry;
                if entry.fingerprint == hidden_path_fingerprint(&entry.rule_id) {
                    entry.paths.clear();
                }
                pos.insert(entry.fingerprint.clone(), entries.len());
                entries.push(entry);
            }
        }
        Ok(Self {
            alint_version: header.alint_version,
            entries,
        })
    }

    /// Total grandfathered occurrences (sum of `count`).
    #[must_use]
    pub fn total(&self) -> u64 {
        self.entries.iter().map(|e| u64::from(e.count)).sum()
    }

    /// Whether this baseline's `alint_version` header is below `0.22` and at
    /// least one of its hidden-path entries matched nothing in `applied`
    /// (#547).
    #[must_use]
    pub fn has_unmatched_pre_022_hidden_path_entries(&self, applied: &AppliedBaseline) -> bool {
        if !self
            .alint_version
            .as_deref()
            .is_some_and(is_pre_022_version)
        {
            return false;
        }
        let stale_by_fp: std::collections::HashMap<&str, u32> = applied
            .stale
            .iter()
            .map(|e| (e.fingerprint.as_str(), e.count))
            .collect();
        self.entries.iter().any(|entry| {
            entry.count > 0
                && entry.path.as_deref() == Some(crate::CONFIGURATION_MASK)
                && stale_by_fp.get(entry.fingerprint.as_str()).copied() == Some(entry.count)
        })
    }
}

/// Whether `version` parses as an alint version earlier than `0.22.0`.
fn is_pre_022_version(version: &str) -> bool {
    let trimmed = version.trim().strip_prefix('v').unwrap_or(version.trim());
    let mut parts = trimmed.split('.');
    let Some(major) = parts.next().and_then(|s| s.parse::<u64>().ok()) else {
        return false;
    };
    let Some(minor_raw) = parts.next() else {
        return false;
    };
    let minor_digits = minor_raw.split(['-', '+']).next().unwrap_or(minor_raw);
    let Ok(minor) = minor_digits.parse::<u64>() else {
        return false;
    };
    (major, minor) < (0, 22)
}

/// A single fingerprinted violation, the input to [`Baseline::from_fingerprints`].
#[derive(Debug, Clone)]
pub struct FingerprintedViolation {
    pub rule_id: String,
    pub path: Option<String>,
    pub fingerprint: String,
    pub message: Option<String>,
    /// Whether this violation's fingerprint leaves out its visible path (e.g.
    /// `constraint` findings), so even a single-occurrence baseline entry
    /// records its visible path in [`BaselineEntry::paths`] (ADR-0049).
    pub path_free_identity: bool,
}

impl FingerprintedViolation {
    /// Build a [`FingerprintedViolation`] from a [`Violation`] and its
    /// computed canonical `fingerprint`.
    #[must_use]
    pub fn from_violation(
        rule_id: impl Into<String>,
        violation: &Violation,
        fingerprint: String,
    ) -> Self {
        Self {
            rule_id: rule_id.into(),
            path: violation
                .report_path()
                .map(|p| canonical_order_path(p).into_owned()),
            fingerprint,
            message: Some(violation.message.to_string()),
            path_free_identity: violation.path_free_identity && !violation.path_is_sensitive(),
        }
    }
}

/// A violation suppressed by the baseline, with the id of the rule that
/// produced it. Kept so `--show-baselined` and the SARIF formatter can
/// render suppressed findings without re-deriving them.
#[derive(Debug, Clone)]
pub struct SuppressedViolation {
    pub rule_id: std::sync::Arc<str>,
    pub violation: Violation,
    /// The matched baseline fingerprint — emitted as SARIF
    /// `partialFingerprints` so Code Scanning correlation aligns with the
    /// baseline.
    pub fingerprint: String,
}

/// The result of applying a baseline to a [`Report`].
#[derive(Debug, Clone)]
pub struct AppliedBaseline {
    /// The report with baselined violations removed — only **new**
    /// violations remain, so the exit code and the primary formatter
    /// output reflect the delta. SARIF re-emits the suppressed findings
    /// "marked not removed" (see [`Self::suppressed`] + [`Self::live_fingerprints`]).
    pub live: Report,
    /// Every suppressed violation (for `--show-baselined` / SARIF marking).
    pub suppressed: Vec<SuppressedViolation>,
    /// Baseline entries that matched fewer occurrences than recorded
    /// (the issue was fixed or its discriminator changed); `count` is the
    /// unfilled remainder. Surfaced as a warning (or a failure under
    /// `--strict-baseline`).
    pub stale: Vec<BaselineEntry>,
    /// Total suppressed occurrences (sum across all fingerprints).
    pub suppressed_total: u64,
    /// Fingerprints of the **live** violations, parallel to
    /// [`live`](Self::live)`.results` (outer) and each result's `violations`
    /// (inner, in the same deterministic order `apply` emits them). Lets SARIF
    /// stamp `partialFingerprints` on new findings without recomputing them.
    pub live_fingerprints: Vec<Vec<String>>,
}

/// Apply a baseline to a report: suppress up to each fingerprint's
/// recorded `count` of matching violations and surface the rest as live.
///
/// Pure and deterministic: within each rule, violations are matched in a
/// stable `(path, line, column, message)` order, so *which* of several
/// byte-identical occurrences is left live (when the baseline count is
/// below the current count) does not depend on the engine's parallel
/// collection order. `fingerprint_of(rule_id, violation)` is supplied by
/// the caller, which holds the file bytes the line-content discriminator
/// needs (typically `|rid, v| fingerprint(rid, v, file_bytes_for(v))`).
pub fn apply<F>(report: &Report, baseline: &Baseline, mut fingerprint_of: F) -> AppliedBaseline
where
    F: FnMut(&str, &Violation) -> String,
{
    use std::collections::HashMap;

    // Remaining suppression budget per fingerprint, drawn down as we go.
    let mut remaining: HashMap<&str, u32> = baseline
        .entries
        .iter()
        .map(|e| (e.fingerprint.as_str(), e.count))
        .collect();
    // Preferred visible paths per fingerprint (ADR-0049): suppressed in Pass 1
    // before filling any remaining count in deterministic order in Pass 2.
    let mut preferred: HashMap<&str, HashMap<&str, u32>> = HashMap::new();
    for e in &baseline.entries {
        if !e.paths.is_empty() && e.fingerprint != hidden_path_fingerprint(&e.rule_id) {
            let counts = preferred.entry(e.fingerprint.as_str()).or_default();
            for path in &e.paths {
                *counts.entry(path.as_str()).or_insert(0) += 1;
            }
        }
    }

    let mut suppressed = Vec::new();
    let mut suppressed_total = 0u64;
    let mut live_results = Vec::with_capacity(report.results.len());
    let mut live_fingerprints: Vec<Vec<String>> = Vec::with_capacity(report.results.len());

    for result in &report.results {
        // Stable match order so suppression is deterministic regardless
        // of the engine's parallel collection order. `sort_by_cached_key`
        // computes each key once (one path allocation per violation) rather
        // than on every comparison.
        let mut ordered: Vec<&Violation> = result.violations.iter().collect();
        ordered.sort_by_cached_key(|v| order_key(v));

        // Compute every violation's fingerprint once.
        let fps: Vec<String> = ordered
            .iter()
            .map(|v| fingerprint_of(&result.rule_id, v))
            .collect();

        // Pass 1 (ADR-0049): suppress visible-path findings that match a
        // preferred path recorded on the baseline entry. Hidden-path findings
        // never enter Pass 1 (ADR-0030).
        let mut matched_preferred = vec![false; ordered.len()];
        if !preferred.is_empty() {
            for (index, (v, fp)) in ordered.iter().zip(&fps).enumerate() {
                if v.path_is_sensitive() {
                    continue;
                }
                let Some(path) = v.path.as_deref() else {
                    continue;
                };
                if remaining.get(fp.as_str()).copied().unwrap_or(0) == 0 {
                    continue;
                }
                let slash = canonical_order_path(path);
                if let Some(pref_count) = preferred
                    .get_mut(fp.as_str())
                    .and_then(|paths| paths.get_mut(slash.as_ref()))
                    .filter(|c| **c > 0)
                {
                    *pref_count -= 1;
                    if let Some(rem) = remaining.get_mut(fp.as_str()) {
                        *rem -= 1;
                    }
                    matched_preferred[index] = true;
                }
            }
        }

        // Pass 2: emit Pass-1 suppressions and fill any remaining budget in
        // deterministic order so both `suppressed` and `live` preserve order.
        let mut live = Vec::new();
        let mut live_fps = Vec::new();
        for ((v, fp), from_pass1) in ordered.into_iter().zip(fps).zip(matched_preferred) {
            if from_pass1 {
                suppressed_total += 1;
                suppressed.push(SuppressedViolation {
                    rule_id: result.rule_id.clone(),
                    violation: v.clone(),
                    fingerprint: fp,
                });
            } else if let Some(count) = remaining.get_mut(fp.as_str()).filter(|c| **c > 0) {
                *count -= 1;
                suppressed_total += 1;
                suppressed.push(SuppressedViolation {
                    rule_id: result.rule_id.clone(),
                    violation: v.clone(),
                    fingerprint: fp,
                });
            } else {
                live.push(v.clone());
                live_fps.push(fp);
            }
        }

        let mut r = result.clone();
        r.violations = live;
        live_results.push(r);
        live_fingerprints.push(live_fps);
    }

    let stale = baseline
        .entries
        .iter()
        .filter_map(|e| {
            let unfilled = remaining.get(e.fingerprint.as_str()).copied().unwrap_or(0);
            (unfilled > 0).then(|| BaselineEntry {
                count: unfilled,
                ..e.clone()
            })
        })
        .collect();

    AppliedBaseline {
        live: Report {
            results: live_results,
        },
        suppressed,
        stale,
        suppressed_total,
        live_fingerprints,
    }
}

/// Canonical slash-separated path string for deterministic cross-platform
/// ordering shared between [`apply`] and output occurrence numbering.
#[doc(hidden)]
#[must_use]
pub fn canonical_order_path(path: &Path) -> std::borrow::Cow<'_, str> {
    let text = path.to_string_lossy();
    if std::path::MAIN_SEPARATOR == '/' {
        text
    } else {
        std::borrow::Cow::Owned(text.replace(std::path::MAIN_SEPARATOR, "/"))
    }
}

/// Deterministic match-order key for a violation: by slash-normalized path,
/// then line, column, and message. Used only to order suppression, not the hash.
fn order_key(v: &Violation) -> (Option<std::borrow::Cow<'_, str>>, usize, usize, &str) {
    (
        v.path.as_deref().map(canonical_order_path),
        v.line.unwrap_or(0),
        v.column.unwrap_or(0),
        v.message.as_ref(),
    )
}

/// The baseline file's path relative to the lint `root`, as a walk-exclude
/// pattern ([`crate::WalkOptions::extra_ignores`]) that matches only that file,
/// so a broad-glob content rule can't lint alint's own committed JSON-Lines
/// artifact as a spurious "new" violation. `None` when the baseline lives
/// outside the root (then it isn't walked anyway) or doesn't exist yet (the
/// missing-baseline error surfaces at load time).
#[must_use]
pub fn walk_exclude(root: &Path, baseline: &Path) -> Option<String> {
    let root_abs = root.canonicalize().ok()?;
    let base_abs = baseline.canonicalize().ok()?;
    let rel = base_abs.strip_prefix(&root_abs).ok()?;
    // Root-anchored (leading `/`): the walker turns this into an override
    // `!/…` that matches ONLY the baseline at the repo root, never a same-named
    // file in a subdirectory. Without the anchor a separator-less pattern
    // matches at any depth, silently dropping a real violation in e.g.
    // `sub/.alint-baseline.json`. Each name is escaped, so `[`, `*` or `\` in
    // it match only themselves.
    let mut pattern = String::new();
    for name in rel {
        pattern.push('/');
        pattern.push_str(&crate::template::glob_escape(&name.to_string_lossy()));
    }
    // An ignore pattern drops trailing whitespace; a class keeps it.
    if let Some(last) = pattern.chars().last().filter(|c| c.is_whitespace()) {
        pattern.pop();
        pattern.push('[');
        pattern.push(last);
        pattern.push(']');
    }
    Some(pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_content_reads_each_file_once_and_only_files() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.txt"), "line\n").unwrap();
        std::fs::create_dir(root.path().join("dir")).unwrap();
        let mut cache = std::collections::HashMap::new();
        let on_line = |path: &str| {
            Violation::new("x")
                .with_path(Path::new(path))
                .with_location(1, 1)
        };
        let file = on_line("a.txt");
        assert_eq!(
            fingerprint_content(root.path(), &file, &mut cache),
            Some(&b"line\n"[..])
        );
        // Cached: a second finding on the file reuses the first read.
        std::fs::write(root.path().join("a.txt"), "changed\n").unwrap();
        assert_eq!(
            fingerprint_content(root.path(), &file, &mut cache),
            Some(&b"line\n"[..])
        );
        for path in ["dir", "missing.txt"] {
            assert_eq!(
                fingerprint_content(root.path(), &on_line(path), &mut cache),
                None
            );
        }
        assert_eq!(
            fingerprint_content(root.path(), &Violation::new("x"), &mut cache),
            None
        );
        // A fingerprint that leaves the content out does not read the file: a
        // sensitive finding, one with a `baseline_key`, and one without a line.
        std::fs::write(root.path().join("b.txt"), "secret\n").unwrap();
        let mut sensitive = on_line("b.txt");
        sensitive.mark_sensitive("test");
        let mut keyed = on_line("b.txt");
        keyed.baseline_key = Some("key".into());
        let lineless = Violation::new("x").with_path(Path::new("b.txt"));
        for v in [sensitive, keyed, lineless] {
            assert_eq!(fingerprint_content(root.path(), &v, &mut cache), None);
            assert!(!cache.contains_key(Path::new("b.txt")));
        }
    }

    #[test]
    fn walk_exclude_leaves_out_only_the_baseline() {
        let root = tempfile::Builder::new()
            .prefix("alint-baseline-test-")
            .tempdir()
            .unwrap();
        let mut names = vec!["base[1].json", "base1.json", "sub/base[1].json"];
        if cfg!(unix) {
            // Legal names only off Windows.
            names.extend(["b*.json", "a\\b.json", "a/b.json", "trailing ", "trailing"]);
        }
        for name in &names {
            let path = root.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "{}").unwrap();
        }
        let walked = |baseline: &str| {
            let pattern = walk_exclude(root.path(), &root.path().join(baseline)).unwrap();
            let opts = crate::WalkOptions {
                respect_gitignore: false,
                extra_ignores: vec![pattern],
                global_git_excludes: false,
            };
            let index = crate::walker::walk(root.path(), &opts).unwrap();
            let mut left_out: Vec<&str> = names
                .iter()
                .copied()
                .filter(|name| !index.contains_file(Path::new(name)))
                .collect();
            left_out.sort_unstable();
            left_out
        };
        assert_eq!(walked("base[1].json"), ["base[1].json"]);
        if cfg!(unix) {
            assert_eq!(walked("b*.json"), ["b*.json"]);
            assert_eq!(walked("a\\b.json"), ["a\\b.json"]);
            assert_eq!(walked("trailing "), ["trailing "]);
        }
        assert_eq!(
            walk_exclude(root.path(), &root.path().join("missing.json")),
            None
        );
        let outside = tempfile::NamedTempFile::new_in(root.path().parent().unwrap()).unwrap();
        assert_eq!(walk_exclude(root.path(), outside.path()), None);
    }

    fn fp(
        rule: &str,
        path: Option<&str>,
        line: Option<usize>,
        key: Option<&str>,
        msg: &str,
        bytes: Option<&[u8]>,
    ) -> String {
        violation_fingerprint(rule, path.map(Path::new), line, key, msg, bytes)
    }

    #[test]
    fn fingerprint_of_an_ordinary_path_is_pinned() {
        // Compatibility pin: existing `.alint-baseline` files hold these exact
        // digests, so a change to path normalization must not move them (audit
        // 2026-10 finding 10 changed only the backslash case below).
        assert_eq!(
            fp("r", Some("src/a.rs"), None, None, "m", None),
            "e442d61e4daffddbab1d9ef55e55aef8974c9cf2983570b0cbbf6e8b869c1817"
        );
        assert_eq!(
            fp(
                "r",
                Some("src/a.rs"),
                Some(2),
                None,
                "m",
                Some(b"x\nbad\ny")
            ),
            "232ea2d0041a143141b5bf4abadf6334080156e142cca88872c37497635b3dd4"
        );
    }

    #[test]
    fn backslash_is_a_separator_only_on_windows() {
        // Audit 2026-10 finding 10: `\` was rewritten to `/` on every OS, so on
        // unix (where `\` is a legal filename byte) `a\b.txt` and the different
        // file `a/b.txt` shared one fingerprint -- a baseline entry for one
        // grandfathered the other, and the fix merge conflated them.
        let slash = fp("r", Some("a/b.txt"), None, None, "m", None);
        let back = fp("r", Some("a\\b.txt"), None, None, "m", None);
        if cfg!(windows) {
            assert_eq!(slash, back, "on Windows `\\` IS the separator");
        } else {
            assert_ne!(slash, back, "on unix `a\\b.txt` is a different file");
        }
    }

    #[cfg(unix)]
    #[test]
    fn distinct_non_utf8_paths_have_distinct_fingerprints() {
        use std::os::unix::ffi::OsStringExt as _;

        let first = std::path::PathBuf::from(std::ffi::OsString::from_vec(vec![
            b'a', b'/', 0x80, b'.', b'r', b's',
        ]));
        let second = std::path::PathBuf::from(std::ffi::OsString::from_vec(vec![
            b'a', b'/', 0x81, b'.', b'r', b's',
        ]));
        assert_eq!(first.to_string_lossy(), second.to_string_lossy());
        assert_ne!(
            violation_fingerprint("r", Some(&first), None, None, "m", None),
            violation_fingerprint("r", Some(&second), None, None, "m", None),
            "lossy path rendering must not merge distinct Unix filenames"
        );
    }

    #[test]
    fn fingerprint_is_stable_and_64_hex() {
        let a = fp("r", Some("a.rs"), Some(2), None, "m", Some(b"x\nbad\ny"));
        let b = fp("r", Some("a.rs"), Some(2), None, "m", Some(b"x\nbad\ny"));
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn line_number_is_excluded_only_content_matters() {
        // Same offending content, different line position → same hash.
        let high = fp("r", Some("a.rs"), Some(2), None, "m", Some(b"x\nbad\n"));
        let low = fp(
            "r",
            Some("a.rs"),
            Some(4),
            None,
            "m",
            Some(b"q\nw\ne\nbad\n"),
        );
        assert_eq!(high, low);
    }

    #[test]
    fn crlf_and_lf_offending_lines_match() {
        let lf = fp("r", Some("a.rs"), Some(1), None, "m", Some(b"bad\nrest"));
        let crlf = fp("r", Some("a.rs"), Some(1), None, "m", Some(b"bad\r\nrest"));
        assert_eq!(lf, crlf, "trailing \\r must be stripped from the content");
    }

    #[test]
    fn editing_the_offending_line_changes_the_hash() {
        let before = fp("r", Some("a.rs"), Some(1), None, "m", Some(b"bad\n"));
        let after = fp("r", Some("a.rs"), Some(1), None, "m", Some(b"worse\n"));
        assert_ne!(before, after);
    }

    #[test]
    fn baseline_key_takes_precedence_over_content() {
        // Same key, different file content → same hash (key wins).
        let a = fp(
            "r",
            Some("a.rs"),
            Some(1),
            Some("$.license"),
            "m",
            Some(b"aaa\n"),
        );
        let b = fp(
            "r",
            Some("a.rs"),
            Some(1),
            Some("$.license"),
            "m",
            Some(b"bbb\n"),
        );
        assert_eq!(a, b);
        // Different key → different hash.
        let c = fp(
            "r",
            Some("a.rs"),
            Some(1),
            Some("$.private"),
            "m",
            Some(b"aaa\n"),
        );
        assert_ne!(a, c);
    }

    #[test]
    fn path_shape_default_ignores_message() {
        // v3: a path-bearing, key-less, line-less violation's identity is
        // (rule_id, path); the (volatile) message is NOT hashed. Two
        // different messages on the same path → the SAME fingerprint.
        let a = fp(
            "r",
            Some("a.rs"),
            None,
            None,
            "has 320 lines (max 300)",
            None,
        );
        let b = fp(
            "r",
            Some("a.rs"),
            None,
            None,
            "has 999 lines (max 300)",
            None,
        );
        assert_eq!(a, b, "message must not affect a path-only fingerprint");
        // A different path → a different fingerprint.
        let c = fp(
            "r",
            Some("b.rs"),
            None,
            None,
            "has 320 lines (max 300)",
            None,
        );
        assert_ne!(a, c);
    }

    #[test]
    fn message_fallback_only_when_no_path() {
        // The message is the discriminator ONLY for a no-path, no-line,
        // no-key violation (the anti-panic branch).
        let a = fp("r", None, None, None, "multiple lockfiles", None);
        let b = fp("r", None, None, None, "multiple lockfiles", None);
        let c = fp("r", None, None, None, "something else", None);
        assert_eq!(a, b);
        assert_ne!(a, c, "no-path violations still distinguish by message");
    }

    #[test]
    fn length_prefix_prevents_concatenation_collisions() {
        // ("ab","c") vs ("a","bc") must not collide via the join.
        let x = fp("ab", Some("c"), None, None, "m", None);
        let y = fp("a", Some("bc"), None, None, "m", None);
        assert_ne!(x, y);
        // Same for baseline_key components: {"a b"} vs {"a","b"}-joined.
        let p = fp("r", None, None, Some("a b"), "m", None);
        let q = fp("r", None, None, Some("a"), "b", None); // "b" is the message, unused since key set
        assert_ne!(p, q);
    }

    #[test]
    fn paths_normalize_to_forward_slashes() {
        let unix = violation_fingerprint("r", Some(Path::new("a/b.rs")), None, None, "m", None);
        // On Windows a backslash path hashes the same as its forward-slash form
        // (so a baseline is portable across OSes); on unix `\` is a filename
        // byte, so it must NOT fold (see `backslash_is_a_separator_only_on_windows`).
        let win = violation_fingerprint("r", Some(Path::new("a\\b.rs")), None, None, "m", None);
        assert_eq!(unix == win, cfg!(windows));
    }

    #[test]
    fn missing_line_falls_through_to_path_identity() {
        // line points past EOF → no content → path identity (v3), no panic.
        let a = fp(
            "r",
            Some("a.rs"),
            Some(99),
            None,
            "msg",
            Some(b"only\none\n"),
        );
        let b = fp("r", Some("a.rs"), None, None, "msg", None);
        assert_eq!(a, b);
        // The message is irrelevant in this branch (path identity wins).
        let c = fp(
            "r",
            Some("a.rs"),
            Some(99),
            None,
            "different message",
            Some(b"only\none\n"),
        );
        assert_eq!(a, c);
    }

    #[test]
    fn ranges_related_locations_and_fix_keys_do_not_change_existing_fingerprints() {
        let original = Violation::new("bad")
            .with_path(Path::new("a.rs"))
            .with_location(2, 1);
        let enriched = original
            .clone()
            .with_range(crate::SourceRange::new(3, 1, 3, 2).unwrap())
            .with_related_location(crate::RelatedLocation::new("b.rs", "secondary"))
            .with_fix_key("occurrence\0one");

        assert_eq!(
            fingerprint("rule", &original, Some(b"ok\nbad\nmore\n")),
            fingerprint("rule", &enriched, Some(b"ok\nbad\nmore\n"))
        );
    }

    #[test]
    fn adding_a_range_to_path_only_finding_preserves_path_identity() {
        let original = Violation::new("bad").with_path(Path::new("a.rs"));
        let enriched = original
            .clone()
            .with_range(crate::SourceRange::new(2, 1, 2, 2).unwrap());
        assert_eq!(
            fingerprint("rule", &original, Some(b"ok\nbad\n")),
            fingerprint("rule", &enriched, Some(b"ok\nbad\n"))
        );
    }

    #[test]
    fn sensitive_fingerprint_ignores_message_key_and_file_bytes() {
        let make = |message: &str, key: &str, line: usize| {
            let mut violation = Violation::new(message.to_string())
                .with_path(std::path::PathBuf::from("secrets.env"))
                .with_location(line, 1)
                .with_baseline_key(key.to_string());
            violation.redact_message("rule");
            violation
        };
        let first = make("TOKEN_VALUE_DO_NOT_REPORT", "raw-key-one", 1);
        let second = make("different secret", "raw-key-two", 2);
        assert_eq!(
            fingerprint("rule", &first, Some(b"TOKEN_VALUE_DO_NOT_REPORT\nother\n")),
            fingerprint("rule", &second, Some(b"other\ndifferent secret\n")),
        );
    }

    /// A finding whose path alint hides, as the engine leaves it.
    fn hidden(path: &str) -> Violation {
        let mut violation = Violation::new("m".to_string())
            .with_path(std::path::PathBuf::from(path))
            .with_location(1, 1);
        violation.mark_path_sensitive();
        violation.redact_message("rule");
        violation
    }

    #[test]
    fn a_hidden_path_stays_out_of_the_fingerprint() {
        let (one, two) = (hidden("secret-one.txt"), hidden("dir/secret-two.txt"));
        let current = fingerprint("rule", &one, Some(b"x\n"));
        // Pinned: v0.22 baseline files hold this digest.
        assert_eq!(
            current,
            "61312ccbd0108b3d383c3d2eaadf02298387d8d9f8801aecb7534d21a78dbb19"
        );
        assert_eq!(fingerprint("rule", &two, None), current);
        assert_ne!(fingerprint("other", &one, None), current, "the rule counts");
        // A redacted finding in a file that is really named `hidden-path`.
        let mut named = Violation::new("m".to_string()).with_path(Path::new("hidden-path"));
        named.redact_message("rule");
        assert_ne!(fingerprint("rule", &named, None), current);

        // The engine's run-local key keeps the path.
        assert_ne!(
            path_identity_fingerprint("rule", &one, None),
            path_identity_fingerprint("rule", &two, None)
        );
    }

    /// A finding identified by its key alone, as the `constraint` kind makes
    /// one.
    fn path_free(path: &str, key: &str) -> Violation {
        Violation::new(format!("m at {path}"))
            .with_path(Path::new(path))
            .with_location(3, 1)
            .with_baseline_key(key.to_string())
            .with_path_free_identity()
    }

    #[test]
    fn a_path_free_finding_hashes_its_rule_and_key_alone() {
        let first = path_free("packages/a/package.json", "6:unique");
        let current = fingerprint("rule", &first, Some(b"a\nb\nc\n"));
        // Pinned: the form the design writes,
        // sha256(lp(rule_id) || lp("\0constraint") || lp(baseline_key)).
        assert_eq!(
            current,
            hash_components([b"rule", b"\0constraint", b"6:unique"])
        );
        // Another path, line and message: the same finding.
        assert_eq!(
            fingerprint("rule", &path_free("b/package.json", "6:unique"), None),
            current
        );
        assert_ne!(
            fingerprint("rule", &path_free("a", "6:unique1:x"), None),
            current
        );
        assert_ne!(fingerprint("other", &first, None), current);
        // Without the flag the path counts, as for every other kind.
        let flagless = Violation::new("m")
            .with_path(Path::new("packages/a/package.json"))
            .with_baseline_key("6:unique");
        assert_eq!(
            fingerprint("rule", &flagless, None),
            fp(
                "rule",
                Some("packages/a/package.json"),
                None,
                Some("6:unique"),
                "",
                None
            )
        );
        assert_ne!(fingerprint("rule", &flagless, None), current);
        // A sensitive one hashes no key; a hidden path still wins.
        let mut sensitive = path_free("a", "6:unique");
        sensitive.redact_message("rule");
        assert_eq!(
            fingerprint("rule", &sensitive, None),
            hash_components([b"rule", b"\0constraint", b"sensitive"])
        );
        let mut hidden = path_free("a", "6:unique");
        hidden.mark_path_sensitive();
        assert_eq!(
            fingerprint("rule", &hidden, None),
            hash_components([b"rule", HIDDEN_PATH, b"sensitive"])
        );
        // The content of the file it sits in is never read for it.
        let mut cache = std::collections::HashMap::new();
        let mut keyless = Violation::new("m")
            .with_path(Path::new("a"))
            .with_location(1, 1)
            .with_path_free_identity();
        keyless.baseline_key = None;
        assert!(fingerprint_content(Path::new("."), &keyless, &mut cache).is_none());
        assert!(cache.is_empty());
    }

    fn item(rule: &str, path: Option<&str>, fp: &str) -> FingerprintedViolation {
        FingerprintedViolation {
            rule_id: rule.into(),
            path: path.map(str::to_string),
            fingerprint: fp.into(),
            message: Some("m".into()),
            path_free_identity: false,
        }
    }

    #[test]
    fn aggregate_collapses_identical_fingerprints_into_counts() {
        let b = Baseline::from_fingerprints(
            None,
            vec![
                item("r", Some("a.rs"), "ff"),
                item("r", Some("a.rs"), "ff"),
                item("r", Some("a.rs"), "ff"),
                item("r", Some("b.rs"), "aa"),
            ],
        );
        assert_eq!(b.entries.len(), 2);
        let ff = b.entries.iter().find(|e| e.fingerprint == "ff").unwrap();
        assert_eq!(ff.count, 3);
        assert_eq!(b.total(), 4);
    }

    #[test]
    fn jsonl_round_trips_and_is_deterministic() {
        let b = Baseline::from_fingerprints(
            Some("0.13.0".into()),
            vec![
                item("z-rule", Some("z.rs"), "fff"),
                item("a-rule", None, "aaa"),
                item("a-rule", None, "aaa"),
            ],
        );
        let text = b.to_jsonl();
        // Header first, then one entry per line.
        assert!(
            text.lines()
                .next()
                .unwrap()
                .contains("\"schema_version\":1")
        );
        let parsed = Baseline::load(&text).unwrap();
        assert_eq!(parsed, b);
        // Byte-identical re-serialization (deterministic sort).
        assert_eq!(parsed.to_jsonl(), text);
        // Sorted by rule_id: a-rule before z-rule.
        assert_eq!(parsed.entries[0].rule_id, "a-rule");
        assert_eq!(parsed.entries[0].count, 2);
    }

    #[test]
    fn load_rejects_unsupported_schema_version() {
        let text = "{\"schema_version\":999}\n";
        match Baseline::load(text) {
            Err(BaselineError::UnsupportedSchema { found: 999 }) => {}
            other => panic!("expected UnsupportedSchema, got {other:?}"),
        }
    }

    #[test]
    fn load_rejects_empty_and_malformed() {
        assert!(matches!(Baseline::load("   \n"), Err(BaselineError::Empty)));
        assert!(matches!(
            Baseline::load("not json\n"),
            Err(BaselineError::Parse { what: "header", .. })
        ));
        let bad_entry = format!("{{\"schema_version\":{SCHEMA_VERSION}}}\nnope\n");
        assert!(matches!(
            Baseline::load(&bad_entry),
            Err(BaselineError::Parse { what: "entry", .. })
        ));
    }

    #[test]
    fn fingerprint_violation_wrapper_reads_fields_incl_baseline_key() {
        use std::path::Path as P;
        // Wrapper agrees with the decomposed call for a line violation.
        let v = Violation::new("msg")
            .with_path(P::new("a.rs"))
            .with_location(2, 1);
        let bytes = b"x\nbad\ny";
        assert_eq!(
            fingerprint("r", &v, Some(bytes)),
            fp("r", Some("a.rs"), Some(2), None, "msg", Some(bytes))
        );
        // A baseline_key overrides the line content.
        let keyed = Violation::new("msg")
            .with_path(P::new("a.rs"))
            .with_location(2, 1)
            .with_baseline_key("$.license");
        assert_eq!(
            fingerprint("r", &keyed, Some(bytes)),
            fp(
                "r",
                Some("a.rs"),
                Some(2),
                Some("$.license"),
                "msg",
                Some(bytes)
            )
        );
        assert_ne!(
            fingerprint("r", &keyed, Some(bytes)),
            fingerprint("r", &v, Some(bytes))
        );
    }

    #[test]
    fn empty_baseline_is_valid_and_suppresses_nothing() {
        let text = format!("{{\"schema_version\":{SCHEMA_VERSION}}}\n");
        let b = Baseline::load(&text).unwrap();
        assert_eq!(b.entries, Vec::new());
        assert_eq!(b.total(), 0);
    }

    #[test]
    fn load_sums_duplicate_fingerprints() {
        // Two entries with the SAME fingerprint (e.g. a git merge of two
        // baselines that each recorded the finding) must SUM, not
        // last-writer-wins, so the suppression budget is the total.
        let text = format!(
            "{{\"schema_version\":{SCHEMA_VERSION}}}\n\
             {{\"rule_id\":\"r\",\"fingerprint\":\"ff\",\"count\":2}}\n\
             {{\"rule_id\":\"r\",\"fingerprint\":\"ff\",\"count\":3}}\n"
        );
        let b = Baseline::load(&text).unwrap();
        assert_eq!(b.entries.len(), 1, "duplicates collapse to one entry");
        assert_eq!(b.entries[0].count, 5, "counts sum (2 + 3)");
        assert_eq!(b.total(), 5);
    }

    #[test]
    fn from_fingerprints_advisory_message_is_order_independent() {
        // A path-identity group collapses violations whose messages differ;
        // the committed advisory message must not depend on input order.
        let mk = |msg: &str| FingerprintedViolation {
            rule_id: "r".into(),
            path: Some("a.rs".into()),
            fingerprint: "shared".into(),
            message: Some(msg.into()),
            path_free_identity: false,
        };
        let one = Baseline::from_fingerprints(None, vec![mk("zeta"), mk("alpha")]);
        let two = Baseline::from_fingerprints(None, vec![mk("alpha"), mk("zeta")]);
        assert_eq!(one, two, "advisory message must not depend on input order");
        assert_eq!(one.entries.len(), 1);
        assert_eq!(one.entries[0].count, 2);
        assert_eq!(
            one.entries[0].message.as_deref(),
            Some("alpha"),
            "the lexicographically smallest message is pinned"
        );
    }

    // ─── apply (the suppression transform) ──────────────────────────

    use crate::Level;
    use crate::report::Report;
    use crate::rule::RuleResult;

    /// A report with one rule emitting violations `(message, line)`.
    fn report(rule: &str, level: Level, vs: &[(&str, Option<usize>)]) -> Report {
        let raw = vs
            .iter()
            .map(|(m, line)| {
                let v = Violation::new((*m).to_string()).with_path(Path::new("f.rs"));
                match line {
                    Some(l) => v.with_location(*l, 1),
                    None => v,
                }
            })
            .collect();
        Report {
            results: vec![RuleResult::new(
                std::sync::Arc::from(rule),
                level,
                None,
                raw,
                false,
            )],
        }
    }

    fn entry(fp: &str, count: u32) -> BaselineEntry {
        BaselineEntry {
            rule_id: "r".into(),
            path: None,
            fingerprint: fp.into(),
            count,
            message: None,
            paths: Vec::new(),
        }
    }

    // Stub fingerprinter: each distinct message is its own fingerprint;
    // identical messages collide (the count/tie-break case). Decouples
    // the transform test from the hash.
    fn by_message(_rule: &str, v: &Violation) -> String {
        v.message.to_string()
    }

    #[test]
    fn apply_matches_hidden_paths_by_count_and_not_by_the_v021_form() {
        let report = |paths: &[&str]| Report {
            results: vec![RuleResult::new(
                std::sync::Arc::from("rule"),
                Level::Error,
                None,
                paths.iter().map(|path| hidden(path)).collect(),
                false,
            )],
        };
        let current = fingerprint("rule", &hidden("a.txt"), None);
        let by_fingerprint = |rule: &str, v: &Violation| fingerprint(rule, v, None);
        let baseline = |entries: Vec<BaselineEntry>| Baseline {
            alint_version: None,
            entries,
        };

        // The shared fingerprint grandfathers as many findings as it counts.
        let shared = baseline(vec![entry(&current, 2)]);
        let out = apply(
            &report(&["a.txt", "b.txt", "c.txt"]),
            &shared,
            by_fingerprint,
        );
        assert_eq!(out.suppressed_total, 2);
        assert_eq!(out.stale, Vec::new());
        assert!(out.suppressed.iter().all(|s| s.fingerprint == current));
        assert_eq!(out.live_fingerprints, [vec![current.clone()]], "one is new");

        // A v0.21 file hashed each path. v0.23 no longer matches that form
        // (ADR-0030 decision 1): its entries are stale and the findings new.
        let v021 = |path: &str| path_identity_fingerprint("rule", &hidden(path), None);
        let old = baseline(vec![entry(&v021("a.txt"), 1), entry(&v021("b.txt"), 1)]);
        let out = apply(&report(&["a.txt", "b.txt"]), &old, by_fingerprint);
        assert_eq!((out.suppressed_total, out.stale.len()), (0, 2));
        assert_eq!(out.live_fingerprints, [vec![current.clone(), current]]);
    }

    #[test]
    fn apply_suppresses_up_to_count_then_reports_new() {
        let rep = report(
            "r",
            Level::Error,
            &[("X", Some(1)), ("X", Some(2)), ("X", Some(3))],
        );
        let base = Baseline {
            alint_version: None,
            entries: vec![entry("X", 2)],
        };
        let out = apply(&rep, &base, by_message);
        assert_eq!(out.suppressed_total, 2);
        assert_eq!(out.live.total_violations(), 1, "the 3rd X is new");
        assert_eq!(out.stale, Vec::new());
    }

    #[test]
    fn apply_reports_unbaselined_as_new() {
        let rep = report("r", Level::Error, &[("Y", Some(1))]);
        let base = Baseline {
            alint_version: None,
            entries: vec![entry("X", 5)],
        };
        let out = apply(&rep, &base, by_message);
        assert_eq!(out.suppressed_total, 0);
        assert_eq!(out.live.total_violations(), 1);
        // X matched nothing → stale with its full count.
        assert_eq!(out.stale.len(), 1);
        assert_eq!(out.stale[0].count, 5);
    }

    #[test]
    fn apply_detects_partial_stale() {
        let rep = report("r", Level::Error, &[("X", Some(1))]);
        let base = Baseline {
            alint_version: None,
            entries: vec![entry("X", 3)],
        };
        let out = apply(&rep, &base, by_message);
        assert_eq!(out.suppressed_total, 1);
        assert_eq!(out.live.total_violations(), 0);
        assert_eq!(out.stale.len(), 1);
        assert_eq!(out.stale[0].count, 2, "unfilled remainder");
    }

    #[test]
    fn apply_tiebreak_is_deterministic_by_line() {
        // Same message (same fp), three lines, baseline count 1.
        let rep = report(
            "r",
            Level::Error,
            &[("X", Some(50)), ("X", Some(1)), ("X", Some(10))],
        );
        let base = Baseline {
            alint_version: None,
            entries: vec![entry("X", 1)],
        };
        let out = apply(&rep, &base, by_message);
        assert_eq!(out.suppressed_total, 1);
        // The lowest-ordered occurrence (line 1) is suppressed; lines
        // 10 and 50 remain live, in sorted order.
        let live_lines: Vec<usize> = out.live.results[0]
            .violations
            .iter()
            .map(|v| v.line.unwrap())
            .collect();
        assert_eq!(live_lines, vec![10, 50]);
        assert_eq!(out.suppressed[0].violation.line, Some(1));
    }

    #[test]
    fn apply_empty_baseline_leaves_everything_live() {
        let rep = report("r", Level::Error, &[("X", Some(1)), ("Y", Some(2))]);
        let out = apply(&rep, &Baseline::default(), by_message);
        assert_eq!(out.suppressed_total, 0);
        assert_eq!(out.live.total_violations(), 2);
        assert_eq!(out.stale, Vec::new());
    }

    #[test]
    fn apply_preserves_notes_without_fingerprinting_or_suppressing_them() {
        let rep = Report {
            results: vec![RuleResult::new(
                "r".into(),
                Level::Error,
                None,
                vec![
                    Violation::new("X"),
                    Violation::new("bounded detail").as_analysis_truncated_note(),
                ],
                false,
            )],
        };
        let base = Baseline {
            alint_version: None,
            entries: vec![entry("X", 1)],
        };
        let mut fingerprint_calls = 0;
        let out = apply(&rep, &base, |rule, violation| {
            fingerprint_calls += 1;
            by_message(rule, violation)
        });

        assert_eq!(fingerprint_calls, 1, "notes are not baseline candidates");
        assert!(out.live.results[0].violations.is_empty());
        assert_eq!(out.live.total_notes(), 1);
        assert_eq!(
            out.live.results[0].notes[0].note_kind(),
            Some(crate::NoteKind::AnalysisTruncated)
        );
    }

    #[test]
    fn apply_suppressing_all_errors_clears_the_exit_signal() {
        let rep = report("r", Level::Error, &[("X", Some(1))]);
        assert!(rep.has_errors());
        let base = Baseline {
            alint_version: None,
            entries: vec![entry("X", 1)],
        };
        let out = apply(&rep, &base, by_message);
        assert!(
            !out.live.has_errors(),
            "a fully-baselined error report must exit clean"
        );
    }

    #[test]
    fn apply_honors_summed_budget_from_merged_duplicates() {
        // A merged baseline with the same fingerprint twice (2 + 3) must
        // suppress all five occurrences, not just the last writer's three.
        let text = format!(
            "{{\"schema_version\":{SCHEMA_VERSION}}}\n\
             {{\"rule_id\":\"r\",\"fingerprint\":\"X\",\"count\":2}}\n\
             {{\"rule_id\":\"r\",\"fingerprint\":\"X\",\"count\":3}}\n"
        );
        let base = Baseline::load(&text).unwrap();
        let rep = report(
            "r",
            Level::Error,
            &[
                ("X", Some(1)),
                ("X", Some(2)),
                ("X", Some(3)),
                ("X", Some(4)),
                ("X", Some(5)),
            ],
        );
        let out = apply(&rep, &base, by_message);
        assert_eq!(out.suppressed_total, 5, "summed budget suppresses all five");
        assert_eq!(out.live.total_violations(), 0);
        assert_eq!(out.stale, Vec::new());
    }

    #[test]
    fn apply_live_fingerprints_align_with_live_violations() {
        // Two rules, each with one suppressed + one live finding. Every
        // stored live fingerprint must equal recomputing it on the SAME
        // live violation it sits beside — the invariant the SARIF/JSON
        // formatters rely on via positional indexing.
        let lined = |m: &str, l: usize| {
            Violation::new(m.to_string())
                .with_path(Path::new("f.rs"))
                .with_location(l, 1)
        };
        let r1 = RuleResult::new(
            std::sync::Arc::from("r1"),
            Level::Error,
            None,
            vec![lined("A", 1), lined("B", 2)],
            false,
        );
        let r2 = RuleResult::new(
            std::sync::Arc::from("r2"),
            Level::Error,
            None,
            vec![lined("C", 1), lined("D", 2)],
            false,
        );
        let rep = Report {
            results: vec![r1, r2],
        };
        // Baseline A and C (by_message fingerprints) → B and D stay live.
        let base = Baseline {
            alint_version: None,
            entries: vec![entry("A", 1), entry("C", 1)],
        };
        let out = apply(&rep, &base, by_message);
        for (ri, rr) in out.live.results.iter().enumerate() {
            for (vi, v) in rr.violations.iter().enumerate() {
                assert_eq!(
                    out.live_fingerprints[ri][vi],
                    by_message(&rr.rule_id, v),
                    "live_fingerprints[{ri}][{vi}] must match its own violation"
                );
            }
        }
        assert_eq!(out.live.results[0].violations[0].message.as_ref(), "B");
        assert_eq!(out.live.results[1].violations[0].message.as_ref(), "D");
    }

    #[test]
    fn apply_never_fingerprints_or_suppresses_analysis_failures() {
        use crate::AnalysisFailureKind;

        let rep = Report {
            results: vec![RuleResult::new(
                std::sync::Arc::from("r"),
                Level::Warning,
                None,
                vec![
                    Violation::new("same").with_path(Path::new("f.rs")),
                    Violation::new("same")
                        .with_path(Path::new("f.rs"))
                        .as_analysis_failure(AnalysisFailureKind::ResourceRead),
                ],
                false,
            )],
        };
        let base = Baseline {
            alint_version: None,
            entries: vec![entry("same", 2)],
        };
        let mut fingerprint_calls = 0;
        let out = apply(&rep, &base, |rule, violation| {
            fingerprint_calls += 1;
            by_message(rule, violation)
        });

        assert_eq!(
            fingerprint_calls, 1,
            "only policy violations are fingerprinted"
        );
        assert_eq!(out.suppressed_total, 1);
        assert!(out.live.results[0].violations.is_empty());
        assert_eq!(out.live.results[0].analysis_failures.len(), 1);
        assert!(out.live.has_errors(), "baseline cannot downgrade a failure");
    }

    #[test]
    fn preferred_paths_keep_baselined_repeats_suppressed_when_new_repeat_sorts_first() {
        let mk_v = |path: &str| {
            Violation::new("dup")
                .with_path(Path::new(path))
                .with_baseline_key("dup")
                .with_path_free_identity()
        };
        let fp = fingerprint("names-unique", &mk_v("p14/package.json"), None);
        // Baseline records p14 and p60 (count 2, paths ["p14/...", "p60/..."]).
        let baseline = Baseline::from_fingerprints(
            Some("0.24.0".into()),
            [
                FingerprintedViolation::from_violation(
                    "names-unique",
                    &mk_v("p60/package.json"),
                    fp.clone(),
                ),
                FingerprintedViolation::from_violation(
                    "names-unique",
                    &mk_v("p14/package.json"),
                    fp.clone(),
                ),
            ],
        );
        assert_eq!(baseline.entries.len(), 1);
        assert_eq!(baseline.entries[0].count, 2);
        assert_eq!(
            baseline.entries[0].path.as_deref(),
            Some("p14/package.json"),
            "advisory path is pinned deterministically"
        );
        assert_eq!(
            baseline.entries[0].paths,
            ["p14/package.json", "p60/package.json"]
        );

        // Add p13 (sorts before p14 and p60): p14 and p60 stay suppressed, p13 is live.
        let rep = Report {
            results: vec![RuleResult::new(
                std::sync::Arc::from("names-unique"),
                Level::Error,
                None,
                vec![
                    mk_v("p13/package.json"),
                    mk_v("p14/package.json"),
                    mk_v("p60/package.json"),
                ],
                false,
            )],
        };
        let out = apply(&rep, &baseline, |r, v| fingerprint(r, v, None));
        assert_eq!(out.suppressed_total, 2);
        let supp_paths: Vec<&Path> = out
            .suppressed
            .iter()
            .filter_map(|s| s.violation.path.as_deref())
            .collect();
        assert_eq!(
            supp_paths,
            [Path::new("p14/package.json"), Path::new("p60/package.json")]
        );
        let live_paths: Vec<&Path> = out.live.results[0]
            .violations
            .iter()
            .filter_map(|v| v.path.as_deref())
            .collect();
        assert_eq!(live_paths, [Path::new("p13/package.json")]);

        // Rename fallback (Pass 2): if p60 is renamed to p13 (2 total findings),
        // Pass 1 suppresses p14 and Pass 2 fills the remaining slot with p13.
        let renamed = Report {
            results: vec![RuleResult::new(
                std::sync::Arc::from("names-unique"),
                Level::Error,
                None,
                vec![mk_v("p13/package.json"), mk_v("p14/package.json")],
                false,
            )],
        };
        let out_renamed = apply(&renamed, &baseline, |r, v| fingerprint(r, v, None));
        assert_eq!(out_renamed.suppressed_total, 2);
        assert!(out_renamed.live.results[0].violations.is_empty());
    }

    #[test]
    fn single_count_path_free_entry_preserves_original_when_earlier_repeat_added() {
        let mk_v = |path: &str| {
            Violation::new("dup")
                .with_path(Path::new(path))
                .with_baseline_key("dup")
                .with_path_free_identity()
        };
        let fp = fingerprint("names-unique", &mk_v("p60/package.json"), None);
        let single = Baseline::from_fingerprints(
            Some("0.24.0".into()),
            [FingerprintedViolation::from_violation(
                "names-unique",
                &mk_v("p60/package.json"),
                fp,
            )],
        );
        assert_eq!(single.entries[0].paths, ["p60/package.json"]);
        let two = Report {
            results: vec![RuleResult::new(
                std::sync::Arc::from("names-unique"),
                Level::Error,
                None,
                vec![mk_v("p13/package.json"), mk_v("p60/package.json")],
                false,
            )],
        };
        let out_single = apply(&two, &single, |r, v| fingerprint(r, v, None));
        assert_eq!(
            out_single.suppressed[0].violation.path.as_deref(),
            Some(Path::new("p60/package.json"))
        );
        assert_eq!(
            out_single.live.results[0].violations[0].path.as_deref(),
            Some(Path::new("p13/package.json"))
        );
    }

    #[test]
    fn preferred_paths_never_widen_count_and_never_store_or_match_hidden_paths() {
        // Hand-edited entry with more paths than count still caps at count.
        let cap_entry = BaselineEntry {
            rule_id: "r".into(),
            path: Some("a.json".into()),
            fingerprint: "dup".into(),
            count: 1,
            message: None,
            paths: vec!["b.json".into(), "c.json".into()],
        };
        let rep = Report {
            results: vec![RuleResult::new(
                std::sync::Arc::from("r"),
                Level::Error,
                None,
                vec![
                    Violation::new("dup").with_path(Path::new("a.json")),
                    Violation::new("dup").with_path(Path::new("b.json")),
                    Violation::new("dup").with_path(Path::new("c.json")),
                ],
                false,
            )],
        };
        let out = apply(
            &rep,
            &Baseline {
                alint_version: None,
                entries: vec![cap_entry],
            },
            by_message,
        );
        assert_eq!(out.suppressed_total, 1);
        assert_eq!(
            out.suppressed[0].violation.path.as_deref(),
            Some(Path::new("b.json"))
        );

        // Hidden-path entries store no `paths` and never match `paths` even if hand-edited.
        let h1 = hidden("secret-a.txt");
        let h2 = hidden("secret-b.txt");
        let hfp = fingerprint("rule", &h1, None);
        let hidden_base = Baseline::from_fingerprints(
            Some("0.24.0".into()),
            [
                FingerprintedViolation::from_violation("rule", &h1, hfp.clone()),
                FingerprintedViolation::from_violation("rule", &h2, hfp.clone()),
            ],
        );
        assert_eq!(hidden_base.entries[0].paths, Vec::<String>::new());
        assert!(!hidden_base.to_jsonl().contains("\"paths\""));

        // Even if someone hand-edits `paths: ["secret-b.txt"]` on a hidden-path entry,
        // `Baseline::load` drops it and `apply` ignores `paths` for sensitive paths.
        let crafted = format!(
            "{{\"schema_version\":{SCHEMA_VERSION}}}\n\
             {{\"rule_id\":\"rule\",\"path\":\"<redacted>\",\"fingerprint\":\"{hfp}\",\"count\":1,\"paths\":[\"secret-b.txt\"]}}\n"
        );
        let loaded = Baseline::load(&crafted).unwrap();
        assert_eq!(loaded.entries[0].paths, Vec::<String>::new());

        let direct_crafted = Baseline {
            alint_version: None,
            entries: vec![BaselineEntry {
                rule_id: "rule".into(),
                path: Some("<redacted>".into()),
                fingerprint: hfp,
                count: 1,
                message: None,
                paths: vec!["secret-b.txt".into()],
            }],
        };
        let hidden_rep = Report {
            results: vec![RuleResult::new(
                std::sync::Arc::from("rule"),
                Level::Error,
                None,
                vec![h1, h2],
                false,
            )],
        };
        let out_hidden = apply(&hidden_rep, &direct_crafted, |r, v| fingerprint(r, v, None));
        assert_eq!(out_hidden.suppressed_total, 1);
        assert_eq!(
            out_hidden.suppressed[0].violation.path.as_deref(),
            Some(Path::new("secret-a.txt")),
            "hidden-path matching must ignore preferred paths and use Pass 2 order"
        );
    }

    #[test]
    fn load_merges_duplicate_entry_paths_and_pre_022_hint_checks_version_and_hidden_paths() {
        let merged_text = format!(
            "{{\"schema_version\":{SCHEMA_VERSION}}}\n\
             {{\"rule_id\":\"r\",\"fingerprint\":\"ff\",\"count\":1,\"paths\":[\"z.json\"]}}\n\
             {{\"rule_id\":\"r\",\"fingerprint\":\"ff\",\"count\":1,\"paths\":[\"a.json\"]}}\n"
        );
        let merged = Baseline::load(&merged_text).unwrap();
        assert_eq!(merged.entries.len(), 1);
        assert_eq!(merged.entries[0].count, 2);
        assert_eq!(merged.entries[0].paths, ["a.json", "z.json"]);

        // Version predicate (#547).
        assert!(is_pre_022_version("0.21.2"));
        assert!(is_pre_022_version("0.21.0"));
        assert!(is_pre_022_version("0.19.5-rc.1"));
        assert!(is_pre_022_version("v0.21.0"));
        assert!(!is_pre_022_version("0.22.0"));
        assert!(!is_pre_022_version("0.23.0"));
        assert!(!is_pre_022_version("1.0.0"));
        assert!(!is_pre_022_version("dev"));

        let empty_report = Report { results: vec![] };
        let hidden_stale = BaselineEntry {
            rule_id: "r".into(),
            path: Some("<redacted>".into()),
            fingerprint: "old-fp".into(),
            count: 1,
            message: None,
            paths: Vec::new(),
        };
        let old_base = Baseline {
            alint_version: Some("0.21.2".into()),
            entries: vec![hidden_stale.clone()],
        };
        let applied_old = apply(&empty_report, &old_base, by_message);
        assert!(old_base.has_unmatched_pre_022_hidden_path_entries(&applied_old));

        let current_base = Baseline {
            alint_version: Some("0.23.0".into()),
            entries: vec![hidden_stale],
        };
        let applied_cur = apply(&empty_report, &current_base, by_message);
        assert!(!current_base.has_unmatched_pre_022_hidden_path_entries(&applied_cur));
    }
}
