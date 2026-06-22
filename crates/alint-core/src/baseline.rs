//! Baseline (grandfathering) primitives: the per-violation fingerprint
//! and the committed JSON-Lines baseline file.
//!
//! Design: [`docs/design/baseline.md`](../../../docs/design/baseline.md),
//! [ADR-0006](../../../docs/adr/0006-baseline-suppression.md).
//!
//! This module is the dependency-free core (slice 1): the fingerprint
//! function and the file (de)serialization. The report-suppression
//! transform, the `Violation.baseline_key` field, the per-rule key
//! audit, and the CLI/formatter wiring land in later slices. Nothing
//! here is wired into `check`/`fix` yet, so it changes no behavior.

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
/// 3. else, the trimmed message (a last-resort fallback).
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
    let path_norm = path.map(normalize_path).unwrap_or_default();

    let discriminator: &[u8] = if let Some(key) = baseline_key {
        key.as_bytes()
    } else if let Some(content) = line.and_then(|n| file_bytes.and_then(|b| offending_line(b, n))) {
        content
    } else {
        message.trim().as_bytes()
    };

    let mut hasher = Sha256::new();
    for component in [rule_id.as_bytes(), path_norm.as_bytes(), discriminator] {
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
#[must_use]
pub fn fingerprint(rule_id: &str, v: &Violation, file_bytes: Option<&[u8]>) -> String {
    violation_fingerprint(
        rule_id,
        v.path.as_deref(),
        v.line,
        v.baseline_key.as_deref(),
        &v.message,
        file_bytes,
    )
}

/// Normalise a path to a stable, forward-slashed string so a fingerprint
/// is identical across operating systems.
fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
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

/// One grandfathered violation. Matching uses **only** `fingerprint`
/// (and `count`); `rule_id`, `path`, and `message` are advisory — they
/// exist so a reviewer reading the file's diff can see what is being
/// suppressed.
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
    /// advisory `rule_id`/`path`/`message` come from the first
    /// occurrence of each fingerprint. Output entries are sorted for a
    /// deterministic, merge-friendly file.
    pub fn from_fingerprints<I>(alint_version: Option<String>, items: I) -> Self
    where
        I: IntoIterator<Item = FingerprintedViolation>,
    {
        // Group by fingerprint, preserving first-seen advisory fields.
        let mut by_fp: BTreeMap<String, BaselineEntry> = BTreeMap::new();
        for item in items {
            by_fp
                .entry(item.fingerprint.clone())
                .and_modify(|e| e.count += 1)
                .or_insert(BaselineEntry {
                    rule_id: item.rule_id,
                    path: item.path,
                    fingerprint: item.fingerprint,
                    count: 1,
                    message: item.message,
                });
        }
        let mut entries: Vec<BaselineEntry> = by_fp.into_values().collect();
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

        let mut entries = Vec::new();
        for line in lines {
            let entry: BaselineEntry =
                serde_json::from_str(line).map_err(|source| BaselineError::Parse {
                    what: "entry",
                    source,
                })?;
            entries.push(entry);
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
}

/// A single fingerprinted violation, the input to [`Baseline::from_fingerprints`].
#[derive(Debug, Clone)]
pub struct FingerprintedViolation {
    pub rule_id: String,
    pub path: Option<String>,
    pub fingerprint: String,
    pub message: Option<String>,
}

/// A violation suppressed by the baseline, with the id of the rule that
/// produced it. Kept so `--show-baselined` and the SARIF formatter can
/// render suppressed findings without re-deriving them.
#[derive(Debug, Clone)]
pub struct SuppressedViolation {
    pub rule_id: std::sync::Arc<str>,
    pub violation: Violation,
}

/// The result of applying a baseline to a [`Report`].
#[derive(Debug, Clone)]
pub struct AppliedBaseline {
    /// The report with baselined violations removed — only **new**
    /// violations remain, so the exit code and the primary formatter
    /// output reflect the delta. (Per-format "mark not remove" — keeping
    /// suppressed results in SARIF — is wired in a later slice using
    /// [`Self::suppressed`].)
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

    let mut suppressed = Vec::new();
    let mut suppressed_total = 0u64;
    let mut live_results = Vec::with_capacity(report.results.len());

    for result in &report.results {
        // Stable match order so suppression is deterministic regardless
        // of the engine's parallel collection order.
        let mut ordered: Vec<&Violation> = result.violations.iter().collect();
        ordered.sort_by(|a, b| order_key(a).cmp(&order_key(b)));

        let mut live = Vec::new();
        for v in ordered {
            let fp = fingerprint_of(&result.rule_id, v);
            if let Some(count) = remaining.get_mut(fp.as_str()).filter(|c| **c > 0) {
                *count -= 1;
                suppressed_total += 1;
                suppressed.push(SuppressedViolation {
                    rule_id: result.rule_id.clone(),
                    violation: v.clone(),
                });
            } else {
                live.push(v.clone());
            }
        }

        let mut r = result.clone();
        r.violations = live;
        live_results.push(r);
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
    }
}

/// Deterministic match-order key for a violation: by path, then line,
/// column, and message. Used only to order suppression, not the hash.
fn order_key(v: &Violation) -> (Option<String>, usize, usize, &str) {
    (
        v.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
        v.line.unwrap_or(0),
        v.column.unwrap_or(0),
        v.message.as_ref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn message_fallback_when_no_line_no_key() {
        let a = fp("r", Some("a.rs"), None, None, "multiple lockfiles", None);
        let b = fp("r", Some("a.rs"), None, None, "multiple lockfiles", None);
        let c = fp("r", Some("a.rs"), None, None, "something else", None);
        assert_eq!(a, b);
        assert_ne!(a, c);
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
        // A backslash path hashes the same as its forward-slash form.
        let win = violation_fingerprint("r", Some(Path::new("a\\b.rs")), None, None, "m", None);
        assert_eq!(unix, win);
    }

    #[test]
    fn missing_line_falls_through_to_message() {
        // line points past EOF → no content → message fallback (no panic).
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
    }

    fn item(rule: &str, path: Option<&str>, fp: &str) -> FingerprintedViolation {
        FingerprintedViolation {
            rule_id: rule.into(),
            path: path.map(str::to_string),
            fingerprint: fp.into(),
            message: Some("m".into()),
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
        assert!(b.entries.is_empty());
        assert_eq!(b.total(), 0);
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
        }
    }

    // Stub fingerprinter: each distinct message is its own fingerprint;
    // identical messages collide (the count/tie-break case). Decouples
    // the transform test from the hash.
    fn by_message(_rule: &str, v: &Violation) -> String {
        v.message.to_string()
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
        assert!(out.stale.is_empty());
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
        assert!(out.stale.is_empty());
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
}
