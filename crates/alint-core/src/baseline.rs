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

/// Maximum preferred paths accepted on a single baseline entry during load
/// (matches the ceiling of `limits.findings_per_rule`), preventing a crafted
/// baseline file from expanding into unbounded memory inside `serde_json`.
const MAX_ENTRY_PATHS: usize = 1_000_000;

/// Maximum total preferred paths accepted across an entire loaded baseline.
const MAX_TOTAL_BASELINE_PATHS: usize = 2_000_000;

fn deserialize_paths<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct PathsVisitor;

    impl<'de> serde::de::Visitor<'de> for PathsVisitor {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a sequence of repo-relative path strings")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut paths = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(1024));
            let mut seen = 0usize;
            while let Some(raw) = seq.next_element::<String>()? {
                seen += 1;
                if seen > MAX_ENTRY_PATHS {
                    return Err(serde::de::Error::custom(format!(
                        "baseline entry exceeds maximum preferred paths limit ({MAX_ENTRY_PATHS})"
                    )));
                }
                if !raw.is_empty() {
                    let normalized = canonical_order_path(Path::new(&raw));
                    paths.push(match normalized {
                        Cow::Borrowed(_) => raw,
                        Cow::Owned(s) => s,
                    });
                }
            }
            Ok(paths)
        }
    }

    deserializer.deserialize_seq(PathsVisitor)
}

/// Whether `entry` carries a hidden-path fingerprint or redacted advisory path
/// and therefore must never retain or consult preferred `paths` (ADR-0030).
fn is_hidden_entry(entry: &BaselineEntry) -> bool {
    entry.fingerprint == hidden_path_fingerprint(&entry.rule_id)
        || entry.path.as_deref() == Some(crate::CONFIGURATION_MASK)
}

/// One grandfathered violation. Matching uses `fingerprint` and `count`,
/// preferring visible paths listed in `paths` when present (ADR-0049);
/// `rule_id`, `path`, and `message` are advisory: they exist so a reviewer
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
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_paths"
    )]
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
        let mut total_paths = 0usize;
        for line in lines {
            let mut entry: BaselineEntry =
                serde_json::from_str(line).map_err(|source| BaselineError::Parse {
                    what: "entry",
                    source,
                })?;
            let incoming_hidden = is_hidden_entry(&entry);
            if incoming_hidden {
                entry.paths.clear();
            }
            if let Some(&i) = pos.get(&entry.fingerprint) {
                entries[i].count = entries[i].count.saturating_add(entry.count);
                if incoming_hidden || is_hidden_entry(&entries[i]) {
                    total_paths = total_paths.saturating_sub(entries[i].paths.len());
                    entries[i].paths.clear();
                } else if !entry.paths.is_empty() {
                    total_paths = total_paths.saturating_add(entry.paths.len());
                    if total_paths > MAX_TOTAL_BASELINE_PATHS {
                        return Err(BaselineError::Parse {
                            what: "entry",
                            source: serde::de::Error::custom(format!(
                                "baseline exceeds maximum total preferred paths limit ({MAX_TOTAL_BASELINE_PATHS})"
                            )),
                        });
                    }
                    entries[i].paths.extend(entry.paths);
                }
            } else {
                if !entry.paths.is_empty() {
                    total_paths = total_paths.saturating_add(entry.paths.len());
                    if total_paths > MAX_TOTAL_BASELINE_PATHS {
                        return Err(BaselineError::Parse {
                            what: "entry",
                            source: serde::de::Error::custom(format!(
                                "baseline exceeds maximum total preferred paths limit ({MAX_TOTAL_BASELINE_PATHS})"
                            )),
                        });
                    }
                }
                pos.insert(entry.fingerprint.clone(), entries.len());
                entries.push(entry);
            }
        }
        // Sort each entry's preferred paths once after merging duplicates: avoids
        // quadratic O(K^2) re-sorting on repeated lines and enforces the sorted
        // multiset invariant on single (non-merged) entries as well.
        for entry in &mut entries {
            if entry.paths.len() > 1 {
                entry.paths.sort();
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

    /// Whether this baseline's `alint_version` header is strictly earlier than
    /// `0.22.0`, `applied` still has live hidden-path findings, and at least
    /// one pre-0.22 entry for a matching rule (or a `<redacted>` path) matched
    /// nothing in `applied` (#547).
    ///
    /// Requiring at least one live hidden-path violation catches pre-0.22
    /// baselines written in every historical shape (v0.20-0.21 `<redacted>`,
    /// v0.20-0.21 pathless `null` such as `file_exists`, and v0.14-0.19
    /// unredacted paths) while avoiding a misleading `--accept-new` hint when
    /// a hidden-path finding was genuinely fixed.
    #[must_use]
    pub fn has_unmatched_pre_022_hidden_path_entries(&self, applied: &AppliedBaseline) -> bool {
        if !self
            .alint_version
            .as_deref()
            .is_some_and(is_pre_022_version)
        {
            return false;
        }
        let live_hidden_rules: std::collections::HashSet<&str> = applied
            .live
            .results
            .iter()
            .filter(|result| result.violations.iter().any(Violation::path_is_sensitive))
            .map(|result| result.rule_id.as_ref())
            .collect();
        if live_hidden_rules.is_empty() {
            return false;
        }
        let stale_by_fp: std::collections::HashMap<&str, u32> = applied
            .stale
            .iter()
            .map(|e| (e.fingerprint.as_str(), e.count))
            .collect();
        self.entries.iter().any(|entry| {
            entry.count > 0
                && stale_by_fp.get(entry.fingerprint.as_str()).copied() == Some(entry.count)
                && entry.fingerprint != hidden_path_fingerprint(&entry.rule_id)
                && (live_hidden_rules.contains(entry.rule_id.as_str())
                    || entry.path.as_deref() == Some(crate::CONFIGURATION_MASK))
        })
    }
}

/// Whether `version` parses as a valid SemVer-like alint version strictly
/// earlier than `0.22.0` (`0.22.0-rc.1` counts as earlier than `0.22.0`,
/// matching [`crate::RunningVersion`]). Rejects leading signs, whitespace,
/// trailing prose, and oversized strings so untrusted baseline headers cannot
/// inject text into stderr hints.
fn is_pre_022_version(version: &str) -> bool {
    const MAX_VERSION_LEN: usize = 64;
    if version.is_empty() || version.len() > MAX_VERSION_LEN {
        return false;
    }
    let body = version.strip_prefix('v').unwrap_or(version);
    // Strip optional `+build` metadata (must contain only safe SemVer chars).
    let core_and_pre = match body.split_once('+') {
        Some((before, build)) => {
            if build.is_empty()
                || !build
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
            {
                return false;
            }
            before
        }
        None => body,
    };
    // Strip optional `-prerelease` label.
    let (core, has_prerelease) = match core_and_pre.split_once('-') {
        Some((before, pre)) => {
            if pre.is_empty()
                || !pre
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
            {
                return false;
            }
            (before, true)
        }
        None => (core_and_pre, false),
    };
    let parse_digits = |s: &str| -> Option<u64> {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse::<u64>().ok())
            .flatten()
    };
    let mut parts = core.split('.');
    let Some(major) = parts.next().and_then(parse_digits) else {
        return false;
    };
    let Some(minor) = parts.next().and_then(parse_digits) else {
        return false;
    };
    let patch = match parts.next() {
        Some(raw) => {
            let Some(p) = parse_digits(raw) else {
                return false;
            };
            p
        }
        None => 0,
    };
    if parts.next().is_some() {
        return false;
    }
    (major, minor) < (0, 22) || (major == 0 && minor == 22 && patch == 0 && has_prerelease)
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
/// Build the per-fingerprint preferred-path multiset for Pass 1 of [`apply`],
/// restricted to entries with `count > 0`, non-hidden paths, and fingerprints
/// actually present in the current report.
fn build_preferred_paths<'a>(
    baseline: &'a Baseline,
    active_fps: &std::collections::HashSet<&str>,
) -> std::collections::HashMap<&'a str, std::collections::HashMap<&'a str, u32>> {
    use std::collections::HashMap;
    let mut preferred: HashMap<&'a str, HashMap<&'a str, u32>> = HashMap::new();
    if active_fps.is_empty() {
        return preferred;
    }
    for e in &baseline.entries {
        if e.count > 0
            && !e.paths.is_empty()
            && active_fps.contains(e.fingerprint.as_str())
            && !is_hidden_entry(e)
        {
            let counts = preferred.entry(e.fingerprint.as_str()).or_default();
            for path in &e.paths {
                let slot = counts.entry(path.as_str()).or_insert(0);
                *slot = slot.saturating_add(1);
            }
        }
    }
    preferred
}

/// Pass 1 (ADR-0049) across the entire report: mark visible-path findings that
/// match a preferred path recorded on the baseline entry. Hidden-path findings
/// never enter Pass 1 (ADR-0030), checked by both `path_is_sensitive()` and
/// the rule's canonical hidden-path fingerprint.
fn mark_preferred_paths(
    report: &Report,
    prepared: &mut [(Vec<&Violation>, Vec<String>, Vec<bool>)],
    preferred: &mut std::collections::HashMap<&str, std::collections::HashMap<&str, u32>>,
    remaining: &mut std::collections::HashMap<&str, u32>,
) {
    if preferred.is_empty() {
        return;
    }
    for (result, (ordered, fps, matched_preferred)) in
        report.results.iter().zip(prepared.iter_mut())
    {
        let hidden_fp = hidden_path_fingerprint(&result.rule_id);
        for (index, (v, fp)) in ordered.iter().zip(fps.iter()).enumerate() {
            if v.path_is_sensitive() || *fp == hidden_fp {
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
}

pub fn apply<F>(report: &Report, baseline: &Baseline, mut fingerprint_of: F) -> AppliedBaseline
where
    F: FnMut(&str, &Violation) -> String,
{
    use std::collections::{HashMap, HashSet};

    // Remaining suppression budget per fingerprint, drawn down as we go.
    let mut remaining: HashMap<&str, u32> = baseline
        .entries
        .iter()
        .map(|e| (e.fingerprint.as_str(), e.count))
        .collect();

    // Order each rule's violations deterministically and compute their
    // fingerprints once across the report. Collecting the active fingerprints
    // first lets us build `preferred` ONLY for entries actually referenced by
    // this report (avoiding O(baseline) allocation on single-rule `fix` passes
    // or clean runs, and ignoring entries with `count == 0`).
    let mut prepared: Vec<(Vec<&Violation>, Vec<String>, Vec<bool>)> =
        Vec::with_capacity(report.results.len());
    for result in &report.results {
        let mut ordered: Vec<&Violation> = result.violations.iter().collect();
        ordered.sort_by_cached_key(|v| order_key(v));
        let fps: Vec<String> = ordered
            .iter()
            .map(|v| fingerprint_of(&result.rule_id, v))
            .collect();
        let matched = vec![false; ordered.len()];
        prepared.push((ordered, fps, matched));
    }
    let active_fps: HashSet<&str> = prepared
        .iter()
        .flat_map(|(_, fps, _)| fps.iter().map(String::as_str))
        .collect();
    let mut preferred = build_preferred_paths(baseline, &active_fps);
    mark_preferred_paths(report, &mut prepared, &mut preferred, &mut remaining);

    // Pass 2 across the entire report: emit Pass-1 suppressions and fill any
    // remaining budget in deterministic order so both `suppressed` and `live`
    // preserve order.
    let mut suppressed = Vec::new();
    let mut suppressed_total = 0u64;
    let mut live_results = Vec::with_capacity(report.results.len());
    let mut live_fingerprints: Vec<Vec<String>> = Vec::with_capacity(report.results.len());

    for (result, (ordered, fps, matched_preferred)) in report.results.iter().zip(prepared) {
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
                paths: Vec::new(),
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
#[path = "baseline_tests.rs"]
mod tests;
