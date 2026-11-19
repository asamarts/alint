//! SARIF 2.1.0 output. Each `alint` invocation becomes one `run` with a
//! `tool.driver` section describing alint and every rule that contributed
//! to the report. Each violation becomes one `result` with a
//! `physicalLocation` anchored on the violating path (+ line/column when
//! the rule recorded them).
//!
//! Targets GitHub Code Scanning's SARIF uploader; fields are deliberately
//! a subset of SARIF 2.1.0 — just enough that GitHub renders the findings
//! in the Security → Code scanning tab.

use std::collections::BTreeMap;
use std::io::Write;

use alint_core::{Level, NoteKind, Report, Violation};
use serde::Serialize;

use crate::occurrence::{OccurrenceKey, occurrence_fingerprint, occurrences};
use crate::{BaselineMarks, ResultMarks};

/// `partialFingerprints` key carrying alint's baseline fingerprint, so GitHub
/// Code Scanning's alert correlation aligns with the baseline.
const FINGERPRINT_KEY: &str = "alint/v1";

/// The artifact a path-less finding (a missing file, a tree-level rule) is
/// anchored on when the caller doesn't supply the config's own path. GitHub
/// Code Scanning drops a result that has no location at all, so every result
/// carries one -- the config that declared the rule, matching the LSP.
pub const DEFAULT_CONFIG_URI: &str = ".alint.yml";

pub fn write_sarif(report: &Report, w: &mut dyn Write) -> std::io::Result<()> {
    write_sarif_with_baseline(report, None, w)
}

/// SARIF with baseline awareness. When `baseline` is supplied (a baseline is in
/// effect), every **live** finding gains `baselineState: "new"` +
/// `partialFingerprints`, and each **baselined** finding is re-emitted — not
/// dropped — with `suppressions: [{ "kind": "external" }]` +
/// `baselineState: "unchanged"`, so GitHub Code Scanning keeps the alert
/// dismissed-not-fixed (no close/reopen flapping). Without a baseline the
/// output is unchanged.
pub fn write_sarif_with_baseline(
    report: &Report,
    baseline: Option<&BaselineMarks>,
    w: &mut dyn Write,
) -> std::io::Result<()> {
    emit_sarif(&build_sarif(report, baseline, None, DEFAULT_CONFIG_URI), w)
}

/// SARIF with pre-computed canonical fingerprints but no baseline in effect.
///
/// `fingerprints` is indexed `[result][violation]`, aligned with
/// `report.results[i].violations[j]` (the same shape the GitLab path builds), so
/// every finding carries the canonical `baseline::violation_fingerprint` under
/// `partialFingerprints` — the same identity baseline mode and GitLab Code
/// Quality use. GitHub Code Scanning can then correlate alerts across runs even
/// without `--baseline`. No `baselineState` is emitted (no baseline is in
/// effect). `None` (generic dispatch / callers with no file access) emits no
/// fingerprints, matching [`write_sarif`]. Results that share a fingerprint
/// are numbered by the path, line and column they show, then rule and
/// message, and each after the first carries one derived from it and its
/// number, as GitLab output derives it, so that each finding stays its own
/// alert.
pub fn write_sarif_with_fingerprints(
    report: &Report,
    fingerprints: Option<&[Vec<String>]>,
    w: &mut dyn Write,
) -> std::io::Result<()> {
    emit_sarif(
        &build_sarif(report, None, fingerprints, DEFAULT_CONFIG_URI),
        w,
    )
}

/// The full-control SARIF writer the CLI uses: optional baseline marks (as
/// [`write_sarif_with_baseline`]), optional no-baseline fingerprints (as
/// [`write_sarif_with_fingerprints`]; ignored when `baseline` is set), and
/// `config_path` -- the repo-relative path of the config file, on which
/// path-less findings are anchored (see [`DEFAULT_CONFIG_URI`]).
pub fn write_sarif_for_config(
    report: &Report,
    baseline: Option<&BaselineMarks>,
    fingerprints: Option<&[Vec<String>]>,
    config_path: &std::path::Path,
    w: &mut dyn Write,
) -> std::io::Result<()> {
    let config_uri = path_to_uri(config_path);
    let fingerprints = if baseline.is_some() {
        None
    } else {
        fingerprints
    };
    emit_sarif(&build_sarif(report, baseline, fingerprints, &config_uri), w)
}

/// Serialize a built [`Sarif`] as pretty JSON with a trailing newline.
fn emit_sarif(sarif: &Sarif, w: &mut dyn Write) -> std::io::Result<()> {
    serde_json::to_writer_pretty(&mut *w, sarif)?;
    writeln!(w)?;
    Ok(())
}

// One pass keeps rule registration, result ordering, and fingerprint indexes aligned.
#[allow(clippy::too_many_lines)]
fn build_sarif(
    report: &Report,
    baseline: Option<&BaselineMarks>,
    fingerprints: Option<&[Vec<String>]>,
    config_uri: &str,
) -> Sarif {
    let mut rules = Vec::with_capacity(report.results.len());
    let mut results = Vec::new();
    let mut notifications = Vec::new();
    let mut execution_successful = true;
    // Each result with a fingerprint, for `number_repeated_fingerprints`: its
    // index in `results`, its fingerprint, and where it stands among those that
    // share it: whether it is live (a baselined one first), then its key.
    let mut numbered: Vec<(usize, &str, (bool, OccurrenceKey<'_>))> = Vec::new();

    for (idx, rr) in report.results.iter().enumerate() {
        rules.push(SarifRule {
            id: rr.rule_id.to_string(),
            short_description: SarifText {
                text: format!("alint rule `{}`", rr.rule_id),
            },
            help_uri: rr.policy_url.as_deref().map(str::to_string),
        });

        let marks: Option<&ResultMarks> = baseline.and_then(|b| b.per_result.get(idx));

        // Live (new) findings — drive the exit code; tagged `new` under a baseline.
        for (vi, v) in rr.violations.iter().enumerate() {
            let mut res = base_result(rr.rule_id.as_ref(), rr.level, v, config_uri);
            // `partialFingerprints` carries the canonical `violation_fingerprint`
            // so GitHub Code Scanning correlates alerts across runs. The value is
            // the baseline marks' fingerprint when a baseline is in effect, else
            // the pre-computed source the CLI supplies for the no-baseline path;
            // both are the same canonical identity. `baselineState: new` is
            // baseline-specific, so it stays gated on an active baseline while the
            // fingerprint is emitted either way.
            let fp = marks.and_then(|m| m.live_fingerprints.get(vi)).or_else(|| {
                fingerprints
                    .and_then(|f| f.get(idx))
                    .and_then(|r| r.get(vi))
            });
            if let Some(fp) = fp {
                if baseline.is_some() {
                    res.baseline_state = Some("new");
                }
                res.partial_fingerprints = Some(fingerprint_map(fp));
                numbered.push((results.len(), fp, (true, OccurrenceKey::of(&rr.rule_id, v))));
            }
            results.push(res);
        }

        for note in &rr.notes {
            match note.note_kind().unwrap_or(NoteKind::Informational) {
                NoteKind::Informational => {
                    let mut result = base_result(rr.rule_id.as_ref(), rr.level, note, config_uri);
                    result.level = "note";
                    result.fixes.clear();
                    results.push(result);
                }
                NoteKind::AnalysisTruncated => {
                    notifications.push(SarifNotification {
                        level: "note",
                        message: SarifText {
                            text: note.message.to_string(),
                        },
                        locations: finding_locations(note, config_uri),
                    });
                }
            }
        }
        for failure in &rr.analysis_failures {
            execution_successful = false;
            results.push(SarifResult {
                rule_id: rr.rule_id.to_string(),
                level: "error",
                kind: Some("fail"),
                message: SarifText {
                    text: format!(
                        "analysis failure ({}): {}",
                        failure.kind.as_str(),
                        failure.message
                    ),
                },
                locations: analysis_failure_locations(failure, config_uri),
                related_locations: Vec::new(),
                suppressions: Vec::new(),
                baseline_state: None,
                partial_fingerprints: None,
                fixes: Vec::new(),
                properties: Some(SarifResultProperties {
                    analysis_failure: failure.kind.as_str(),
                }),
            });
        }

        // Baselined findings — marked, not removed, so Code Scanning dismisses
        // (rather than closes-then-reopens) the alert.
        if let Some(m) = marks {
            for sf in &m.suppressed {
                let mut res = base_result(rr.rule_id.as_ref(), rr.level, &sf.violation, config_uri);
                res.suppressions = vec![SarifSuppression { kind: "external" }];
                res.baseline_state = Some("unchanged");
                res.partial_fingerprints = Some(fingerprint_map(&sf.fingerprint));
                numbered.push((
                    results.len(),
                    &sf.fingerprint,
                    (false, OccurrenceKey::of(&rr.rule_id, &sf.violation)),
                ));
                results.push(res);
            }
        }
    }

    number_repeated_fingerprints(&mut results, &numbered);

    Sarif {
        schema: "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json",
        version: "2.1.0",
        runs: vec![SarifRun {
            tool: SarifTool {
                driver: SarifDriver {
                    name: "alint",
                    information_uri: "https://github.com/asamarts/alint",
                    version: env!("CARGO_PKG_VERSION"),
                    rules,
                },
            },
            // alint's columns (and fix regions) count Unicode scalar values,
            // not UTF-16 code units (the SARIF default when absent).
            column_kind: "unicodeCodePoints",
            invocations: (!execution_successful || !notifications.is_empty()).then(|| {
                vec![SarifInvocation {
                    execution_successful,
                    tool_execution_notifications: notifications,
                }]
            }),
            results,
        }],
    }
}

fn level_to_sarif(l: Level) -> &'static str {
    match l {
        Level::Error => "error",
        Level::Warning => "warning",
        Level::Info => "note",
        Level::Off => "none",
    }
}

/// Render a repo-relative path as a SARIF `uri-reference` (RFC 3986):
/// forward-slash separators (a Windows `\` otherwise breaks GitHub Code
/// Scanning's repo-file mapping) and percent-encoding of every byte that
/// isn't URI-path-safe (space, `#`, `%`, controls, non-ASCII), so a path
/// like `src/a b#c.rs` neither mis-parses nor fails a format-asserting
/// SARIF validator. `:` is encoded too: in a relative reference a colon in
/// the first segment (`a:b.txt`) would otherwise parse as a URI scheme.
/// A backslash is a separator only on Windows; on Unix it is a filename byte
/// and must encode as `%5C` rather than aliasing a distinct slash path.
#[cfg(unix)]
fn path_to_uri(path: &std::path::Path) -> String {
    use std::os::unix::ffi::OsStrExt as _;

    let bytes = path.as_os_str().as_bytes();
    let mut out = String::with_capacity(bytes.len());
    for &byte in bytes {
        if is_uri_path_byte(byte) {
            out.push(char::from(byte));
        } else {
            push_percent_encoded_byte(&mut out, byte);
        }
    }
    out
}

#[cfg(not(unix))]
fn path_to_uri(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch == '\\' && std::path::MAIN_SEPARATOR == '\\' {
            out.push('/');
        } else if ch.is_ascii() && is_uri_path_byte(ch as u8) {
            out.push(ch);
        } else {
            let mut buf = [0u8; 4];
            for byte in ch.encode_utf8(&mut buf).bytes() {
                push_percent_encoded_byte(&mut out, byte);
            }
        }
    }
    out
}

#[cfg(unix)]
fn path_has_lossless_uri(_path: &std::path::Path) -> bool {
    true
}

#[cfg(not(unix))]
fn path_has_lossless_uri(path: &std::path::Path) -> bool {
    path.to_str().is_some()
}

fn is_uri_path_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'/'
            | b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~'
            | b'!'
            | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b';'
            | b'='
            | b'@'
    )
}

fn push_percent_encoded_byte(out: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push('%');
    out.push(char::from(HEX[usize::from(byte >> 4)]));
    out.push(char::from(HEX[usize::from(byte & 0x0f)]));
}

/// A SARIF result with the shared fields filled in (no baseline annotations).
fn base_result(rule_id: &str, level: Level, v: &Violation, config_uri: &str) -> SarifResult {
    SarifResult {
        rule_id: rule_id.to_string(),
        level: level_to_sarif(level),
        kind: None,
        message: SarifText {
            text: v.message.to_string(),
        },
        locations: finding_locations(v, config_uri),
        related_locations: finding_related_locations(v, config_uri),
        suppressions: Vec::new(),
        baseline_state: None,
        partial_fingerprints: None,
        fixes: build_fixes(rule_id, v),
        properties: None,
    }
}

fn finding_locations(v: &Violation, config_uri: &str) -> Vec<SarifLocation> {
    let region = (!v.path_is_sensitive())
        .then(|| {
            v.range.as_ref().map(source_range_to_sarif).or_else(|| {
                (v.line.is_some() || v.column.is_some()).then_some(SarifRegion {
                    start_line: v.line,
                    start_column: v.column,
                    end_line: None,
                    end_column: None,
                })
            })
        })
        .flatten();
    let (uri, region) = if let Some(path) = v.report_path() {
        (path_to_uri(path), region)
    } else {
        // GitHub Code Scanning drops a location-less result/notification.
        (
            config_uri.to_string(),
            region.or_else(|| {
                Some(SarifRegion {
                    start_line: Some(v.line.unwrap_or(1)),
                    start_column: v.column,
                    end_line: None,
                    end_column: None,
                })
            }),
        )
    };
    vec![SarifLocation {
        physical_location: SarifPhysicalLocation {
            artifact_location: SarifArtifactLocation { uri },
            region,
        },
    }]
}

fn finding_related_locations(v: &Violation, config_uri: &str) -> Vec<SarifRelatedLocation> {
    if v.path_is_sensitive() {
        return Vec::new();
    }
    let mut locations = Vec::with_capacity(
        v.related_locations.len() + usize::from(v.related_locations_omitted > 0),
    );
    locations.extend(
        v.related_locations
            .iter()
            .map(|related| SarifRelatedLocation {
                physical_location: SarifPhysicalLocation {
                    artifact_location: SarifArtifactLocation {
                        uri: path_to_uri(&related.path),
                    },
                    region: related.range.as_ref().map(source_range_to_sarif),
                },
                message: SarifText {
                    text: related.label.clone(),
                },
            }),
    );
    if v.related_locations_omitted > 0 {
        locations.push(SarifRelatedLocation {
            physical_location: SarifPhysicalLocation {
                artifact_location: SarifArtifactLocation {
                    uri: config_uri.to_string(),
                },
                region: None,
            },
            message: SarifText {
                text: format!("and {} more related locations", v.related_locations_omitted),
            },
        });
    }
    locations
}

fn source_range_to_sarif(range: &alint_core::SourceRange) -> SarifRegion {
    SarifRegion {
        start_line: Some(range.start_line()),
        start_column: Some(range.start_column()),
        end_line: Some(range.end_line()),
        end_column: Some(range.end_column()),
    }
}
fn analysis_failure_locations(
    failure: &alint_core::AnalysisFailure,
    config_uri: &str,
) -> Vec<SarifLocation> {
    let path = failure.report_path();
    let pathless = path.is_none();
    let uri = path.map_or_else(|| config_uri.to_string(), path_to_uri);
    let region = (!failure.path_is_sensitive())
        .then(|| {
            (pathless || failure.line.is_some() || failure.column.is_some()).then_some(
                SarifRegion {
                    start_line: failure
                        .line
                        .or((pathless || failure.column.is_some()).then_some(1)),
                    start_column: failure.column,
                    end_line: None,
                    end_column: None,
                },
            )
        })
        .flatten();
    vec![SarifLocation {
        physical_location: SarifPhysicalLocation {
            artifact_location: SarifArtifactLocation { uri },
            region,
        },
    }]
}

/// Build the SARIF `result.fixes` for a violation from its check-computed
/// [`proposed_edits`](alint_core::Violation). Empty (the common case, and every
/// non-fixable finding) yields no `fixes` key.
///
/// All of a violation's proposed edits target its own artifact, so they group
/// as one fix, one `artifactChange` per path, and one `replacement` per edit.
/// Every edit carries a concrete 1-based line/column `deletedRegion` (an
/// insertion is an empty region at its point). An empty replacement string is a
/// pure deletion (no `insertedContent`).
fn build_fixes(rule_id: &str, v: &Violation) -> Vec<SarifFix> {
    if v.proposed_edits.is_empty()
        || v.proposed_edits
            .iter()
            .any(|edit| !path_has_lossless_uri(&edit.path))
    {
        // A missing fix is safer than a URI that can alias another file. Unix
        // paths are encoded from raw bytes; other hosts require exact Unicode.
        return Vec::new();
    }
    let mut by_path: BTreeMap<String, Vec<SarifReplacement>> = BTreeMap::new();
    for pe in &v.proposed_edits {
        let r = &pe.region;
        let deleted_region = SarifRegion {
            start_line: Some(r.start_line),
            start_column: Some(r.start_column),
            end_line: Some(r.end_line),
            end_column: Some(r.end_column),
        };
        let inserted_content = (!pe.inserted.is_empty()).then(|| SarifText {
            text: pe.inserted.clone(),
        });
        by_path
            .entry(path_to_uri(&pe.path))
            .or_default()
            .push(SarifReplacement {
                deleted_region,
                inserted_content,
            });
    }
    let artifact_changes = by_path
        .into_iter()
        .map(|(uri, replacements)| SarifArtifactChange {
            artifact_location: SarifArtifactLocation { uri },
            replacements,
        })
        .collect();
    vec![SarifFix {
        description: SarifText {
            text: format!("alint: fix `{rule_id}`"),
        },
        artifact_changes,
    }]
}

/// Give every result whose fingerprint another result of the run carries a
/// fingerprint of its own (ADR-0047), as GitLab output does: the canonical
/// fingerprint is free of paths for some findings (a `constraint` rule's
/// `unique` without `determines` gives one per repeated row, at different
/// files), and GitHub Code Scanning may merge results that share one into a
/// single alert. The results that share a fingerprint are numbered in an
/// order the results decide, not the order they are written in: the baselined
/// ones first, then the live ones, each by the path, line and column they
/// show, then rule and message ([`OccurrenceKey`]). The baseline suppresses
/// the first findings of a fingerprint in path, line, column and message
/// order, up to its count, so the baselined results come first by their key
/// anyway, but for a hidden path, which shows as `<redacted>`. A repeat then
/// keeps its number while the repeats that sort before it stay; one that
/// sorts before it shifts it, baselined or not, as the new one can take a
/// baselined place. The first keeps the canonical fingerprint, the identity
/// the baseline gives the finding; the Nth (N > 0) gets
/// [`occurrence_fingerprint`]. A run with no repeat is written unchanged.
fn number_repeated_fingerprints(
    results: &mut [SarifResult],
    numbered: &[(usize, &str, (bool, OccurrenceKey<'_>))],
) {
    let findings: Vec<(&str, (bool, OccurrenceKey<'_>))> = numbered
        .iter()
        .map(|&(_, fingerprint, key)| (fingerprint, key))
        .collect();
    for (&(at, fingerprint, _), occurrence) in numbered.iter().zip(occurrences(&findings)) {
        if occurrence > 0 {
            results[at].partial_fingerprints = Some(fingerprint_map(&occurrence_fingerprint(
                fingerprint,
                occurrence,
            )));
        }
    }
}

fn fingerprint_map(fp: &str) -> BTreeMap<&'static str, String> {
    let mut m = BTreeMap::new();
    m.insert(FINGERPRINT_KEY, fp.to_string());
    m
}

// ─── SARIF serde types ───────────────────────────────────────────────

#[derive(Serialize)]
struct Sarif {
    #[serde(rename = "$schema")]
    schema: &'static str,
    version: &'static str,
    runs: Vec<SarifRun>,
}

#[derive(Serialize)]
struct SarifRun {
    tool: SarifTool,
    #[serde(rename = "columnKind")]
    column_kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    invocations: Option<Vec<SarifInvocation>>,
    results: Vec<SarifResult>,
}

#[derive(Serialize)]
struct SarifInvocation {
    #[serde(rename = "executionSuccessful")]
    execution_successful: bool,
    #[serde(rename = "toolExecutionNotifications")]
    tool_execution_notifications: Vec<SarifNotification>,
}

#[derive(Serialize)]
struct SarifNotification {
    level: &'static str,
    message: SarifText,
    locations: Vec<SarifLocation>,
}

#[derive(Serialize)]
struct SarifTool {
    driver: SarifDriver,
}

#[derive(Serialize)]
struct SarifDriver {
    name: &'static str,
    #[serde(rename = "informationUri")]
    information_uri: &'static str,
    version: &'static str,
    rules: Vec<SarifRule>,
}

#[derive(Serialize)]
struct SarifRule {
    id: String,
    #[serde(rename = "shortDescription")]
    short_description: SarifText,
    #[serde(rename = "helpUri", skip_serializing_if = "Option::is_none")]
    help_uri: Option<String>,
}

#[derive(Serialize)]
struct SarifText {
    text: String,
}

#[derive(Serialize)]
struct SarifResult {
    #[serde(rename = "ruleId")]
    rule_id: String,
    level: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'static str>,
    message: SarifText,
    locations: Vec<SarifLocation>,
    #[serde(rename = "relatedLocations", skip_serializing_if = "Vec::is_empty")]
    related_locations: Vec<SarifRelatedLocation>,
    /// `[{ "kind": "external" }]` for a baselined finding; empty otherwise.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    suppressions: Vec<SarifSuppression>,
    /// `"new"` for a live finding under a baseline, `"unchanged"` for a
    /// baselined one; absent when no baseline is in effect.
    #[serde(rename = "baselineState", skip_serializing_if = "Option::is_none")]
    baseline_state: Option<&'static str>,
    /// The alint baseline fingerprint, for Code Scanning alert correlation.
    #[serde(
        rename = "partialFingerprints",
        skip_serializing_if = "Option::is_none"
    )]
    partial_fingerprints: Option<BTreeMap<&'static str, String>>,
    /// Concrete fixes alint would apply for this finding (rendered from the
    /// violation's proposed edits). Empty (the common case) omits the key.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fixes: Vec<SarifFix>,
    #[serde(skip_serializing_if = "Option::is_none")]
    properties: Option<SarifResultProperties>,
}

#[derive(Serialize)]
struct SarifResultProperties {
    #[serde(rename = "analysisFailure")]
    analysis_failure: &'static str,
}

/// A SARIF `fix` — a described set of artifact changes a consumer can apply.
#[derive(Serialize)]
struct SarifFix {
    description: SarifText,
    #[serde(rename = "artifactChanges")]
    artifact_changes: Vec<SarifArtifactChange>,
}

/// A SARIF `artifactChange` — the replacements to make in one artifact.
#[derive(Serialize)]
struct SarifArtifactChange {
    #[serde(rename = "artifactLocation")]
    artifact_location: SarifArtifactLocation,
    replacements: Vec<SarifReplacement>,
}

/// A SARIF `replacement` — delete `deletedRegion`, insert `insertedContent`.
/// A region-less `deletedRegion` (all fields absent) replaces the whole
/// artifact; an absent `insertedContent` is a pure deletion.
#[derive(Serialize)]
struct SarifReplacement {
    #[serde(rename = "deletedRegion")]
    deleted_region: SarifRegion,
    #[serde(rename = "insertedContent", skip_serializing_if = "Option::is_none")]
    inserted_content: Option<SarifText>,
}

/// A SARIF `suppression` — alint emits `kind: "external"` for findings
/// dismissed by the committed baseline file.
#[derive(Serialize)]
struct SarifSuppression {
    kind: &'static str,
}

#[derive(Serialize)]
struct SarifLocation {
    #[serde(rename = "physicalLocation")]
    physical_location: SarifPhysicalLocation,
}

#[derive(Serialize)]
struct SarifRelatedLocation {
    #[serde(rename = "physicalLocation")]
    physical_location: SarifPhysicalLocation,
    message: SarifText,
}

#[derive(Serialize)]
struct SarifPhysicalLocation {
    #[serde(rename = "artifactLocation")]
    artifact_location: SarifArtifactLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    region: Option<SarifRegion>,
}

#[derive(Serialize)]
struct SarifArtifactLocation {
    uri: String,
}

#[derive(Serialize)]
struct SarifRegion {
    #[serde(rename = "startLine", skip_serializing_if = "Option::is_none")]
    start_line: Option<usize>,
    #[serde(rename = "startColumn", skip_serializing_if = "Option::is_none")]
    start_column: Option<usize>,
    #[serde(rename = "endLine", skip_serializing_if = "Option::is_none")]
    end_line: Option<usize>,
    #[serde(rename = "endColumn", skip_serializing_if = "Option::is_none")]
    end_column: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{
        EditRegion, ProposedEdit, RelatedLocation, Report, RuleResult, SourceRange, Violation,
    };
    use serde_json::Value;
    use std::path::{Path, PathBuf};

    #[test]
    fn path_to_uri_slashes_and_percent_encodes() {
        // Plain ASCII paths are unchanged (keeps existing snapshots stable).
        assert_eq!(path_to_uri(Path::new("src/lib.rs")), "src/lib.rs");
        // Only the host platform's separator becomes `/`; a Unix filename byte
        // must remain distinct from a path boundary.
        let expected_backslash = if cfg!(windows) {
            "src/a/b.rs"
        } else {
            "src%5Ca%5Cb.rs"
        };
        assert_eq!(path_to_uri(Path::new("src\\a\\b.rs")), expected_backslash);
        // Space / `#` / `%` are percent-encoded per RFC 3986 uri-reference.
        assert_eq!(path_to_uri(Path::new("a b#c%d.rs")), "a%20b%23c%25d.rs");
        // A `:` would make `a:b.txt` parse as scheme `a` -- encode it.
        assert_eq!(path_to_uri(Path::new("a:b.txt")), "a%3Ab.txt");
        assert_eq!(path_to_uri(Path::new("dir/x:y")), "dir/x%3Ay");
    }

    fn render(report: &Report) -> Value {
        let mut buf = Vec::new();
        write_sarif(report, &mut buf).unwrap();
        serde_json::from_slice(&buf).unwrap()
    }

    #[test]
    fn a_fixable_finding_renders_result_fixes_with_a_line_column_region() {
        let mut v = Violation::new("bad value").with_path(Path::new("app.json"));
        v.is_fixable = true;
        v.proposed_edits = vec![ProposedEdit {
            path: PathBuf::from("app.json"),
            region: EditRegion {
                start_line: 4,
                start_column: 12,
                end_line: 4,
                end_column: 17,
            },
            inserted: "\"v2.0\"".to_string(),
        }];
        let report = Report {
            results: vec![RuleResult {
                rule_id: "v-pins".into(),
                level: Level::Error,
                policy_url: None,
                violations: vec![v],
                notes: Vec::new(),
                is_fixable: true,
                analysis_failures: Vec::new(),
            }],
        };
        let out = render(&report);
        let fix = &out["runs"][0]["results"][0]["fixes"][0];
        assert_eq!(fix["description"]["text"], "alint: fix `v-pins`");
        let change = &fix["artifactChanges"][0];
        assert_eq!(change["artifactLocation"]["uri"], "app.json");
        let repl = &change["replacements"][0];
        assert_eq!(repl["deletedRegion"]["startLine"], 4);
        assert_eq!(repl["deletedRegion"]["startColumn"], 12);
        assert_eq!(repl["deletedRegion"]["endLine"], 4);
        assert_eq!(repl["deletedRegion"]["endColumn"], 17);
        assert_eq!(repl["insertedContent"]["text"], "\"v2.0\"");
    }

    #[test]
    fn a_finding_without_proposed_edits_omits_the_fixes_key() {
        // The common case (and every non-fixable finding): no `fixes` key at all,
        // so existing consumers and snapshots are unchanged.
        let report = Report {
            results: vec![RuleResult {
                rule_id: "has-readme".into(),
                level: Level::Error,
                policy_url: None,
                violations: vec![Violation::new("missing")],
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
            }],
        };
        let out = render(&report);
        assert!(out["runs"][0]["results"][0]["fixes"].is_null());
    }

    #[test]
    #[cfg(unix)]
    fn fixes_keep_literal_backslash_and_slash_paths_distinct() {
        let region = EditRegion {
            start_line: 1,
            start_column: 1,
            end_line: 1,
            end_column: 1,
        };
        let mut violation = Violation::new("two files");
        violation.proposed_edits = vec![
            ProposedEdit {
                path: PathBuf::from(r"src\policy.yml"),
                region: region.clone(),
                inserted: "a".to_string(),
            },
            ProposedEdit {
                path: PathBuf::from("src/policy.yml"),
                region,
                inserted: "b".to_string(),
            },
        ];

        let fixes = build_fixes("r", &violation);
        let changes = &fixes[0].artifact_changes;
        assert_eq!(changes.len(), 2);
        let uris: Vec<_> = changes
            .iter()
            .map(|change| change.artifact_location.uri.as_str())
            .collect();
        assert_eq!(uris, vec!["src%5Cpolicy.yml", "src/policy.yml"]);
    }
    #[test]
    #[cfg(unix)]
    fn fixes_keep_distinct_non_utf8_paths_distinct() {
        use std::os::unix::ffi::OsStringExt as _;

        let region = EditRegion {
            start_line: 1,
            start_column: 1,
            end_line: 1,
            end_column: 1,
        };
        let mut violation = Violation::new("two files");
        violation.proposed_edits = [0xff, 0xfe]
            .into_iter()
            .map(|byte| ProposedEdit {
                path: PathBuf::from(std::ffi::OsString::from_vec(vec![
                    b'b', b'a', b'd', byte, b'.', b't', b'x', b't',
                ])),
                region: region.clone(),
                inserted: "x".to_string(),
            })
            .collect();

        let fixes = build_fixes("r", &violation);
        let uris: Vec<_> = fixes[0]
            .artifact_changes
            .iter()
            .map(|change| change.artifact_location.uri.as_str())
            .collect();
        assert_eq!(uris, vec!["bad%FE.txt", "bad%FF.txt"]);
    }

    #[test]
    fn a_pure_deletion_emits_a_region_without_inserted_content() {
        // An empty replacement string is a deletion: a valid `deletedRegion`
        // (always present, SARIF-required) and NO `insertedContent`.
        let mut v = Violation::new("remove").with_path(Path::new("x.txt"));
        v.is_fixable = true;
        v.proposed_edits = vec![ProposedEdit {
            path: PathBuf::from("x.txt"),
            region: EditRegion {
                start_line: 1,
                start_column: 2,
                end_line: 1,
                end_column: 5,
            },
            inserted: String::new(),
        }];
        let report = Report {
            results: vec![RuleResult {
                rule_id: "r".into(),
                level: Level::Warning,
                policy_url: None,
                violations: vec![v],
                notes: Vec::new(),
                is_fixable: true,
                analysis_failures: Vec::new(),
            }],
        };
        let out = render(&report);
        let repl =
            &out["runs"][0]["results"][0]["fixes"][0]["artifactChanges"][0]["replacements"][0];
        assert_eq!(repl["deletedRegion"]["startLine"], 1);
        assert_eq!(repl["deletedRegion"]["startColumn"], 2);
        assert_eq!(repl["deletedRegion"]["endColumn"], 5);
        assert!(repl["insertedContent"].is_null()); // pure deletion
    }

    #[test]
    fn envelope_declares_schema_version_and_driver_metadata() {
        let report = Report { results: vec![] };
        let v = render(&report);

        assert_eq!(v["version"], "2.1.0");
        assert!(
            v["$schema"]
                .as_str()
                .unwrap()
                .contains("sarif-schema-2.1.0.json")
        );
        let driver = &v["runs"][0]["tool"]["driver"];
        assert_eq!(driver["name"], "alint");
        assert_eq!(driver["version"], env!("CARGO_PKG_VERSION"));
        assert!(driver["informationUri"].is_string());
    }

    #[test]
    fn each_rule_result_emits_one_tool_rule_and_one_result() {
        let report = Report {
            results: vec![
                RuleResult {
                    rule_id: "rule-a".into(),
                    level: Level::Error,
                    policy_url: Some("https://example.com/a".into()),
                    violations: vec![Violation::new("va1"), Violation::new("va2")],
                    notes: Vec::new(),
                    is_fixable: false,
                    analysis_failures: Vec::new(),
                },
                RuleResult {
                    rule_id: "rule-b".into(),
                    level: Level::Warning,
                    policy_url: None,
                    violations: vec![Violation::new("vb")],
                    notes: Vec::new(),
                    is_fixable: false,
                    analysis_failures: Vec::new(),
                },
            ],
        };
        let v = render(&report);

        let rules = v["runs"][0]["tool"]["driver"]["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0]["id"], "rule-a");
        assert_eq!(rules[0]["helpUri"], "https://example.com/a");
        assert_eq!(rules[1]["id"], "rule-b");
        assert!(rules[1].get("helpUri").is_none());

        let results = v["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0]["ruleId"], "rule-a");
        assert_eq!(results[0]["level"], "error");
        assert_eq!(results[0]["message"]["text"], "va1");
        assert_eq!(results[2]["ruleId"], "rule-b");
        assert_eq!(results[2]["level"], "warning");
    }

    #[test]
    fn level_off_maps_to_none_and_info_to_note() {
        let report = Report {
            results: vec![
                RuleResult {
                    rule_id: "off".into(),
                    level: Level::Off,
                    policy_url: None,
                    violations: vec![Violation::new("x")],
                    notes: Vec::new(),
                    is_fixable: false,
                    analysis_failures: Vec::new(),
                },
                RuleResult {
                    rule_id: "info".into(),
                    level: Level::Info,
                    policy_url: None,
                    violations: vec![Violation::new("y")],
                    notes: Vec::new(),
                    is_fixable: false,
                    analysis_failures: Vec::new(),
                },
            ],
        };
        let v = render(&report);
        let results = v["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results[0]["level"], "none");
        assert_eq!(results[1]["level"], "note");
    }

    #[test]
    fn physical_location_carries_path_and_region_when_present() {
        let report = Report {
            results: vec![RuleResult {
                rule_id: "r".into(),
                level: Level::Error,
                policy_url: None,
                violations: vec![
                    Violation::new("m")
                        .with_path(Path::new("src/lib.rs"))
                        .with_location(7, 3),
                ],
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
            }],
        };
        let v = render(&report);
        let loc = &v["runs"][0]["results"][0]["locations"][0]["physicalLocation"];
        assert_eq!(loc["artifactLocation"]["uri"], "src/lib.rs");
        assert_eq!(loc["region"]["startLine"], 7);
        assert_eq!(loc["region"]["startColumn"], 3);
    }

    #[test]
    fn ranges_related_locations_and_truncation_are_projected() {
        let mut violation = Violation::new("m")
            .with_path(Path::new("src/lib.rs"))
            .with_range(SourceRange::new(2, 3, 4, 5).unwrap())
            .with_related_location(
                RelatedLocation::new("src/other.rs", "canonical source")
                    .with_range(SourceRange::new(7, 8, 9, 10).unwrap()),
            );
        violation.related_locations_omitted = 2;
        let report = Report {
            results: vec![RuleResult {
                rule_id: "r".into(),
                level: Level::Error,
                policy_url: None,
                violations: vec![violation],
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
            }],
        };

        let rendered = render(&report);
        let result = &rendered["runs"][0]["results"][0];
        let primary = &result["locations"][0]["physicalLocation"]["region"];
        assert_eq!(primary["startLine"], 2);
        assert_eq!(primary["startColumn"], 3);
        assert_eq!(primary["endLine"], 4);
        assert_eq!(primary["endColumn"], 5);
        let related = result["relatedLocations"].as_array().unwrap();
        assert_eq!(related.len(), 2);
        assert_eq!(
            related[0]["physicalLocation"]["artifactLocation"]["uri"],
            "src/other.rs"
        );
        assert_eq!(related[0]["physicalLocation"]["region"]["startLine"], 7);
        assert_eq!(related[0]["physicalLocation"]["region"]["endColumn"], 10);
        assert_eq!(related[0]["message"]["text"], "canonical source");
        assert_eq!(
            related[1]["message"]["text"],
            "and 2 more related locations"
        );
    }

    #[test]
    fn violations_without_path_anchor_on_the_config_file() {
        let report = Report {
            results: vec![RuleResult {
                rule_id: "r".into(),
                level: Level::Error,
                policy_url: None,
                violations: vec![Violation::new("no-path")],
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
            }],
        };
        let v = render(&report);
        let locs = v["runs"][0]["results"][0]["locations"].as_array().unwrap();
        // GitHub Code Scanning drops a result without a location, so a
        // path-less finding is anchored on the config file.
        assert_eq!(locs.len(), 1, "{locs:?}");
        let loc = &locs[0]["physicalLocation"];
        assert_eq!(loc["artifactLocation"]["uri"], DEFAULT_CONFIG_URI);
        assert_eq!(loc["region"]["startLine"], 1);

        // The CLI passes the real config path.
        let mut buf = Vec::new();
        write_sarif_for_config(
            &report,
            None,
            None,
            Path::new("cfg/alint config.yml"),
            &mut buf,
        )
        .unwrap();
        let v: Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(
            v["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "cfg/alint%20config.yml"
        );
    }

    #[test]
    fn run_declares_unicode_code_point_columns() {
        let v = render(&Report { results: vec![] });
        assert_eq!(v["runs"][0]["columnKind"], "unicodeCodePoints");
    }

    /// Structural SARIF 2.1.0 checks every emitted document must pass (the
    /// official schema is too large to vendor): required properties, a
    /// non-empty `locations` per result with a relative `uri` that has no
    /// scheme, 1-based positive line/column numbers, and a valid
    /// `columnKind`.
    #[test]
    fn every_result_is_structurally_valid_sarif() {
        let report = Report {
            results: vec![RuleResult {
                rule_id: "r".into(),
                level: Level::Warning,
                policy_url: Some("https://example.com/p".into()),
                violations: vec![
                    Violation::new("no-path"),
                    Violation::new("colon")
                        .with_path(PathBuf::from("a:b.txt"))
                        .with_location(2, 4),
                ],
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
            }],
        };
        let v = render(&report);
        assert_eq!(v["version"], "2.1.0");
        let run = &v["runs"][0];
        assert_eq!(run["tool"]["driver"]["name"], "alint");
        assert!(
            ["utf16CodeUnits", "unicodeCodePoints"].contains(&run["columnKind"].as_str().unwrap())
        );
        for res in run["results"].as_array().unwrap() {
            assert!(res["message"]["text"].is_string());
            assert!(res["ruleId"].is_string());
            let locs = res["locations"].as_array().unwrap();
            assert!(!locs.is_empty(), "every result has a location: {res}");
            for loc in locs {
                let pl = &loc["physicalLocation"];
                let uri = pl["artifactLocation"]["uri"].as_str().unwrap();
                let first = uri.split('/').next().unwrap();
                assert!(!first.contains(':'), "no scheme-like first segment: {uri}");
                for key in ["startLine", "startColumn", "endLine", "endColumn"] {
                    if let Some(n) = pl["region"].get(key) {
                        assert!(n.as_u64().unwrap() >= 1, "{key} is 1-based: {pl}");
                    }
                }
            }
        }
    }

    #[test]
    fn baseline_marks_suppressed_and_tags_live_findings() {
        use crate::{BaselineMarks, ResultMarks, SuppressedFinding};
        let report = Report {
            results: vec![RuleResult {
                rule_id: "no-todo".into(),
                level: Level::Error,
                policy_url: None,
                violations: vec![Violation::new("new TODO").with_path(Path::new("b.txt"))],
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
            }],
        };
        let marks = BaselineMarks {
            per_result: vec![ResultMarks {
                live_fingerprints: vec!["fp-live".into()],
                suppressed: vec![SuppressedFinding {
                    violation: Violation::new("old TODO").with_path(Path::new("a.txt")),
                    fingerprint: "fp-supp".into(),
                }],
            }],
            suppressed_total: 1,
        };
        let mut buf = Vec::new();
        write_sarif_with_baseline(&report, Some(&marks), &mut buf).unwrap();
        let v: Value = serde_json::from_slice(&buf).unwrap();
        let results = v["runs"][0]["results"].as_array().unwrap();
        assert_eq!(
            results.len(),
            2,
            "live + suppressed both emitted (marked, not removed)"
        );

        // The live (new) finding is tagged but not suppressed.
        let live = &results[0];
        assert_eq!(live["message"]["text"], "new TODO");
        assert_eq!(live["baselineState"], "new");
        assert_eq!(live["partialFingerprints"]["alint/v1"], "fp-live");
        assert!(live.get("suppressions").is_none());

        // The baselined finding is re-emitted, marked dismissed-not-fixed.
        let supp = &results[1];
        assert_eq!(supp["message"]["text"], "old TODO");
        assert_eq!(supp["baselineState"], "unchanged");
        assert_eq!(supp["suppressions"][0]["kind"], "external");
        assert_eq!(supp["partialFingerprints"]["alint/v1"], "fp-supp");
    }

    #[test]
    fn no_baseline_leaves_results_unannotated() {
        let report = Report {
            results: vec![RuleResult {
                rule_id: "r".into(),
                level: Level::Error,
                policy_url: None,
                violations: vec![Violation::new("v").with_path(Path::new("f"))],
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
            }],
        };
        let v = render(&report); // write_sarif — no baseline
        let r = &v["runs"][0]["results"][0];
        assert!(r.get("baselineState").is_none());
        assert!(r.get("suppressions").is_none());
        assert!(r.get("partialFingerprints").is_none());
    }

    #[test]
    fn fingerprints_without_baseline_attach_partialfingerprints_only() {
        // The CLI's no-baseline SARIF path passes canonical fingerprints (indexed
        // [result][violation]) so Code Scanning can correlate alerts without a
        // baseline. Each result must carry its OWN `partialFingerprints` and NO
        // `baselineState` (no baseline is in effect). Distinct fingerprints pin
        // each to its own result — a desync would stamp fp-a1 onto rule-b.
        let report = Report {
            results: vec![
                RuleResult {
                    rule_id: "rule-a".into(),
                    level: Level::Error,
                    policy_url: None,
                    violations: vec![Violation::new("a1").with_path(Path::new("a.txt"))],
                    notes: Vec::new(),
                    is_fixable: false,
                    analysis_failures: Vec::new(),
                },
                RuleResult {
                    rule_id: "rule-b".into(),
                    level: Level::Error,
                    policy_url: None,
                    violations: vec![Violation::new("b1").with_path(Path::new("b.txt"))],
                    notes: Vec::new(),
                    is_fixable: false,
                    analysis_failures: Vec::new(),
                },
                RuleResult {
                    rule_id: "rule-c".into(),
                    level: Level::Error,
                    policy_url: None,
                    // Two violations in ONE result exercises the vi (violation)
                    // axis of the [result][violation] grid; the two results above
                    // only vary the result index.
                    violations: vec![
                        Violation::new("c1").with_path(Path::new("c.txt")),
                        Violation::new("c2").with_path(Path::new("c.txt")),
                    ],
                    notes: Vec::new(),
                    is_fixable: false,
                    analysis_failures: Vec::new(),
                },
            ],
        };
        let fps = vec![
            vec!["fp-a1".to_string()],
            vec!["fp-b1".to_string()],
            vec!["fp-c1".to_string(), "fp-c2".to_string()],
        ];
        let mut buf = Vec::new();
        write_sarif_with_fingerprints(&report, Some(&fps), &mut buf).unwrap();
        let v: Value = serde_json::from_slice(&buf).unwrap();
        let results = v["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results.len(), 4);
        let by_msg = |m: &str| {
            results
                .iter()
                .find(|r| r["message"]["text"] == m)
                .unwrap_or_else(|| panic!("no result for {m}"))
        };
        assert_eq!(by_msg("a1")["partialFingerprints"]["alint/v1"], "fp-a1");
        assert_eq!(by_msg("b1")["partialFingerprints"]["alint/v1"], "fp-b1");
        // vi>0: the second violation of rule-c gets its OWN fingerprint, proving
        // the grid is indexed [result][violation], not just per-result.
        assert_eq!(by_msg("c1")["partialFingerprints"]["alint/v1"], "fp-c1");
        assert_eq!(by_msg("c2")["partialFingerprints"]["alint/v1"], "fp-c2");
        // No baseline in effect → no `baselineState` on any result.
        assert!(results.iter().all(|r| r.get("baselineState").is_none()));
    }

    #[test]
    fn live_fingerprints_attach_to_their_own_result() {
        use crate::{BaselineMarks, ResultMarks};
        // Two rules, each with one live finding carrying a DISTINCT
        // fingerprint. The marks are positionally indexed, so a desync would
        // stamp fp-a1 onto rule-b's finding — this pins each to its own.
        let report = Report {
            results: vec![
                RuleResult {
                    rule_id: "rule-a".into(),
                    level: Level::Error,
                    policy_url: None,
                    violations: vec![Violation::new("a1").with_path(Path::new("a.txt"))],
                    notes: Vec::new(),
                    is_fixable: false,
                    analysis_failures: Vec::new(),
                },
                RuleResult {
                    rule_id: "rule-b".into(),
                    level: Level::Error,
                    policy_url: None,
                    violations: vec![Violation::new("b1").with_path(Path::new("b.txt"))],
                    notes: Vec::new(),
                    is_fixable: false,
                    analysis_failures: Vec::new(),
                },
            ],
        };
        let marks = BaselineMarks {
            per_result: vec![
                ResultMarks {
                    live_fingerprints: vec!["fp-a1".into()],
                    suppressed: Vec::new(),
                },
                ResultMarks {
                    live_fingerprints: vec!["fp-b1".into()],
                    suppressed: Vec::new(),
                },
            ],
            suppressed_total: 0,
        };
        let mut buf = Vec::new();
        write_sarif_with_baseline(&report, Some(&marks), &mut buf).unwrap();
        let v: Value = serde_json::from_slice(&buf).unwrap();
        let results = v["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        let by_msg = |m: &str| {
            results
                .iter()
                .find(|r| r["message"]["text"] == m)
                .unwrap_or_else(|| panic!("no result for {m}"))
        };
        assert_eq!(by_msg("a1")["ruleId"], "rule-a");
        assert_eq!(by_msg("a1")["partialFingerprints"]["alint/v1"], "fp-a1");
        assert_eq!(by_msg("b1")["ruleId"], "rule-b");
        assert_eq!(by_msg("b1")["partialFingerprints"]["alint/v1"], "fp-b1");
    }

    /// A report whose every finding has its own fingerprint, under a baseline
    /// and without one.
    fn unrepeated_fingerprints() -> (Report, Vec<Vec<String>>, crate::BaselineMarks) {
        use crate::{BaselineMarks, ResultMarks, SuppressedFinding};
        let report = Report {
            results: vec![
                RuleResult {
                    rule_id: "rule-a".into(),
                    level: Level::Error,
                    policy_url: None,
                    violations: vec![
                        Violation::new("a1").with_path(Path::new("a.txt")),
                        Violation::new("a2").with_path(Path::new("b.txt")),
                    ],
                    notes: Vec::new(),
                    is_fixable: false,
                    analysis_failures: Vec::new(),
                },
                RuleResult {
                    rule_id: "rule-b".into(),
                    level: Level::Warning,
                    policy_url: None,
                    violations: vec![Violation::new("b1")],
                    notes: Vec::new(),
                    is_fixable: false,
                    analysis_failures: Vec::new(),
                },
            ],
        };
        let fingerprints = vec![
            vec!["fp-a1".to_string(), "fp-a2".to_string()],
            vec!["fp-b1".to_string()],
        ];
        let marks = BaselineMarks {
            per_result: vec![
                ResultMarks {
                    live_fingerprints: fingerprints[0].clone(),
                    suppressed: vec![SuppressedFinding {
                        violation: Violation::new("a0").with_path(Path::new("c.txt")),
                        fingerprint: "fp-a0".into(),
                    }],
                },
                ResultMarks {
                    live_fingerprints: fingerprints[1].clone(),
                    suppressed: Vec::new(),
                },
            ],
            suppressed_total: 1,
        };
        (report, fingerprints, marks)
    }

    /// The SARIF a baselined run of [`unrepeated_fingerprints`] wrote before
    /// repeated fingerprints were told apart, `{version}` for alint's.
    const UNREPEATED_BASELINED: &str = r#"{
  "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json",
  "version": "2.1.0",
  "runs": [
    {
      "tool": {
        "driver": {
          "name": "alint",
          "informationUri": "https://github.com/asamarts/alint",
          "version": "{version}",
          "rules": [
            {
              "id": "rule-a",
              "shortDescription": {
                "text": "alint rule `rule-a`"
              }
            },
            {
              "id": "rule-b",
              "shortDescription": {
                "text": "alint rule `rule-b`"
              }
            }
          ]
        }
      },
      "columnKind": "unicodeCodePoints",
      "results": [
        {
          "ruleId": "rule-a",
          "level": "error",
          "message": {
            "text": "a1"
          },
          "locations": [
            {
              "physicalLocation": {
                "artifactLocation": {
                  "uri": "a.txt"
                }
              }
            }
          ],
          "baselineState": "new",
          "partialFingerprints": {
            "alint/v1": "fp-a1"
          }
        },
        {
          "ruleId": "rule-a",
          "level": "error",
          "message": {
            "text": "a2"
          },
          "locations": [
            {
              "physicalLocation": {
                "artifactLocation": {
                  "uri": "b.txt"
                }
              }
            }
          ],
          "baselineState": "new",
          "partialFingerprints": {
            "alint/v1": "fp-a2"
          }
        },
        {
          "ruleId": "rule-a",
          "level": "error",
          "message": {
            "text": "a0"
          },
          "locations": [
            {
              "physicalLocation": {
                "artifactLocation": {
                  "uri": "c.txt"
                }
              }
            }
          ],
          "suppressions": [
            {
              "kind": "external"
            }
          ],
          "baselineState": "unchanged",
          "partialFingerprints": {
            "alint/v1": "fp-a0"
          }
        },
        {
          "ruleId": "rule-b",
          "level": "warning",
          "message": {
            "text": "b1"
          },
          "locations": [
            {
              "physicalLocation": {
                "artifactLocation": {
                  "uri": ".alint.yml"
                },
                "region": {
                  "startLine": 1
                }
              }
            }
          ],
          "baselineState": "new",
          "partialFingerprints": {
            "alint/v1": "fp-b1"
          }
        }
      ]
    }
  ]
}
"#;

    /// A run whose findings each carry their own fingerprint is written as it
    /// was before repeats were told apart, byte for byte, and each result
    /// keeps the fingerprint it was given.
    #[test]
    fn a_run_without_repeated_fingerprints_is_unchanged() {
        let (report, fingerprints, marks) = unrepeated_fingerprints();
        let mut baselined = Vec::new();
        write_sarif_with_baseline(&report, Some(&marks), &mut baselined).unwrap();
        assert_eq!(
            String::from_utf8(baselined).unwrap(),
            UNREPEATED_BASELINED.replace("{version}", env!("CARGO_PKG_VERSION"))
        );
        let mut plain = Vec::new();
        write_sarif_with_fingerprints(&report, Some(&fingerprints), &mut plain).unwrap();
        let plain: Value = serde_json::from_slice(&plain).unwrap();
        let shown: Vec<&str> = plain["runs"][0]["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|result| result["partialFingerprints"]["alint/v1"].as_str().unwrap())
            .collect();
        assert_eq!(shown, ["fp-a1", "fp-a2", "fp-b1"]);
    }

    /// Findings at different files can share a fingerprint, which is free of
    /// paths for a `constraint` finding: each repeat within a run gets its
    /// own, derived from the fingerprint and its number as GitLab derives it,
    /// so Code Scanning keeps every finding as its own alert. The first keeps
    /// the fingerprint it shares with the baseline and GitLab.
    #[test]
    fn a_repeated_fingerprint_gets_an_occurrence_of_its_own() {
        let finding =
            |message: &'static str, path: &str| Violation::new(message).with_path(Path::new(path));
        let report = Report {
            results: vec![RuleResult {
                rule_id: "names-unique".into(),
                level: Level::Error,
                policy_url: None,
                violations: vec![
                    finding("`name` \"x\" is not unique", "b/package.json"),
                    finding("`name` \"x\" is not unique", "c/package.json"),
                    finding("`name` \"y\" is not unique", "d/package.json"),
                    finding("`name` \"x\" is not unique", "e/package.json"),
                ],
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
            }],
        };
        let fingerprints = vec![vec![
            "fp-x".to_string(),
            "fp-x".to_string(),
            "fp-y".to_string(),
            "fp-x".to_string(),
        ]];
        let shown = |sarif: &[u8]| -> Vec<String> {
            let sarif: Value = serde_json::from_slice(sarif).unwrap();
            sarif["runs"][0]["results"]
                .as_array()
                .unwrap()
                .iter()
                .map(|result| {
                    result["partialFingerprints"]["alint/v1"]
                        .as_str()
                        .unwrap()
                        .to_string()
                })
                .collect()
        };
        let mut sarif = Vec::new();
        write_sarif_with_fingerprints(&report, Some(&fingerprints), &mut sarif).unwrap();
        let sarif = shown(&sarif);
        // GitLab's fingerprints for the same findings, in the same order.
        let mut gitlab = Vec::new();
        crate::gitlab::write_gitlab(&report, Some(&fingerprints), &mut gitlab).unwrap();
        let gitlab: Vec<String> = serde_json::from_slice::<Value>(&gitlab)
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|issue| issue["fingerprint"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(sarif, gitlab);
        assert_eq!(sarif[0], "fp-x");
        assert_eq!(sarif[2], "fp-y");
        let distinct: std::collections::BTreeSet<&String> = sarif.iter().collect();
        assert_eq!(distinct.len(), 4, "{sarif:?}");

        // The numbers follow what each finding shows, not the order the
        // findings are written in, in both formats.
        let reversed = Report {
            results: vec![RuleResult {
                violations: report.results[0].violations.iter().rev().cloned().collect(),
                ..report.results[0].clone()
            }],
        };
        let fingerprints: Vec<Vec<String>> = vec![fingerprints[0].iter().rev().cloned().collect()];
        let mut again = Vec::new();
        write_sarif_with_fingerprints(&reversed, Some(&fingerprints), &mut again).unwrap();
        let mut gitlab_again = Vec::new();
        crate::gitlab::write_gitlab(&reversed, Some(&fingerprints), &mut gitlab_again).unwrap();
        let gitlab_again: Vec<String> = serde_json::from_slice::<Value>(&gitlab_again)
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|issue| issue["fingerprint"].as_str().unwrap().to_string())
            .collect();
        let backwards: Vec<String> = sarif.iter().rev().cloned().collect();
        assert_eq!(shown(&again), backwards);
        assert_eq!(gitlab_again, backwards);
    }

    /// A `unique` rule's findings at `findings` (path and line), all with
    /// one message.
    fn repeats(findings: &[(&str, usize)]) -> Report {
        Report {
            results: vec![RuleResult {
                rule_id: "names-unique".into(),
                level: Level::Error,
                policy_url: None,
                violations: findings
                    .iter()
                    .map(|&(path, line)| {
                        Violation::new("`name` \"x\" is not unique")
                            .with_path(Path::new(path))
                            .with_location(line, 1)
                    })
                    .collect(),
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
            }],
        }
    }

    /// A path-free fingerprint, as a `constraint` rule's `unique` gives.
    fn path_free(_: &str, _: &Violation) -> String {
        "fp-x".to_string()
    }

    /// Each SARIF result of [`repeats`] at `findings` by its path: its
    /// fingerprint and whether it is baselined, with `baseline` applied as
    /// `check` applies it. No two results share a fingerprint.
    fn shown_under(
        baseline: &alint_core::baseline::Baseline,
        findings: &[(&str, usize)],
    ) -> BTreeMap<String, (String, bool)> {
        use crate::{BaselineMarks, ResultMarks, SuppressedFinding};
        let applied = alint_core::baseline::apply(&repeats(findings), baseline, path_free);
        let marks = BaselineMarks {
            per_result: vec![ResultMarks {
                live_fingerprints: applied.live_fingerprints[0].clone(),
                suppressed: applied
                    .suppressed
                    .iter()
                    .map(|suppressed| SuppressedFinding {
                        violation: suppressed.violation.clone(),
                        fingerprint: suppressed.fingerprint.clone(),
                    })
                    .collect(),
            }],
            suppressed_total: applied.suppressed_total,
        };
        let mut sarif = Vec::new();
        write_sarif_with_baseline(&applied.live, Some(&marks), &mut sarif).unwrap();
        let sarif: Value = serde_json::from_slice(&sarif).unwrap();
        let results = sarif["runs"][0]["results"].as_array().unwrap();
        let shown: BTreeMap<String, (String, bool)> = results
            .iter()
            .map(|result| {
                let location = &result["locations"][0]["physicalLocation"];
                (
                    location["artifactLocation"]["uri"]
                        .as_str()
                        .unwrap()
                        .to_string(),
                    (
                        result["partialFingerprints"]["alint/v1"]
                            .as_str()
                            .unwrap()
                            .to_string(),
                        result["baselineState"] == "unchanged",
                    ),
                )
            })
            .collect();
        let distinct: std::collections::BTreeSet<&String> =
            shown.values().map(|(fingerprint, _)| fingerprint).collect();
        assert_eq!(distinct.len(), results.len(), "{shown:?}");
        shown
    }

    /// The results that share a fingerprint are numbered baselined first, then
    /// by what they show, not by the order they are written in. The baseline
    /// suppresses a fingerprint's first findings by path, line, column and
    /// message, up to its count: a repeat keeps its fingerprint while the
    /// repeats that sort before it stay, and a repeat that sorts before it
    /// shifts it, baselined or not, as it can take its place in the baseline.
    #[test]
    fn a_repeat_keeps_its_fingerprint_while_the_repeats_before_it_stay() {
        use alint_core::baseline::{Baseline, FingerprintedViolation};
        // The baseline written while `b` and `d` were the findings.
        let written = repeats(&[("b/package.json", 1), ("d/package.json", 5)]);
        let baseline = Baseline::from_fingerprints(
            None,
            written.results[0]
                .violations
                .iter()
                .map(|violation| FingerprintedViolation {
                    rule_id: "names-unique".into(),
                    path: None,
                    fingerprint: path_free("names-unique", violation),
                    message: Some(violation.message.to_string()),
                }),
        );
        let shown = |findings: &[(&str, usize)]| shown_under(&baseline, findings);
        let at = |shown: &BTreeMap<String, (String, bool)>, path: &str| shown[path].clone();
        let alone = shown(&[("b/package.json", 1), ("d/package.json", 5)]);
        let (b, d) = (at(&alone, "b/package.json"), at(&alone, "d/package.json"));
        assert_eq!(b, ("fp-x".to_string(), true));
        assert!(d.1, "{alone:?}");

        // A repeat that sorts after them leaves both as they were, in either
        // order the findings come in.
        for findings in [
            [
                ("b/package.json", 1),
                ("d/package.json", 5),
                ("e/package.json", 1),
            ],
            [
                ("e/package.json", 1),
                ("d/package.json", 5),
                ("b/package.json", 1),
            ],
        ] {
            let after = shown(&findings);
            assert_eq!(at(&after, "b/package.json"), b, "{after:?}");
            assert_eq!(at(&after, "d/package.json"), d, "{after:?}");
            assert!(!at(&after, "e/package.json").1, "{after:?}");
        }

        // One that sorts between them leaves `b`, and takes `d`'s place in
        // the baseline: `d` is new, under another fingerprint.
        let between = shown(&[
            ("b/package.json", 1),
            ("c/package.json", 1),
            ("d/package.json", 5),
        ]);
        assert_eq!(at(&between, "b/package.json"), b, "{between:?}");
        assert_eq!(at(&between, "c/package.json"), d, "{between:?}");
        assert!(!at(&between, "d/package.json").1, "{between:?}");
        assert_ne!(at(&between, "d/package.json").0, d.0, "{between:?}");

        // One that sorts first shifts both.
        let first = shown(&[
            ("a/package.json", 1),
            ("b/package.json", 1),
            ("d/package.json", 5),
        ]);
        assert_eq!(at(&first, "a/package.json"), b, "{first:?}");
        assert_eq!(at(&first, "b/package.json"), d, "{first:?}");
        assert!(!at(&first, "d/package.json").1, "{first:?}");
    }
}
