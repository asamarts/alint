use std::borrow::Cow;
use std::io::Write;
use std::path::Path;

use alint_core::{
    AnalysisFailure, FixReport, FixStatus, Level, ProposedEdit, RelatedLocation, Report, Violation,
};
use serde::Serialize;

use crate::BaselineMarks;

/// Render a violation path as a (possibly lossy) UTF-8 string. A repo file
/// whose name isn't valid UTF-8 — legal on Unix — must not fail the whole
/// document: serde's `&Path` serializer *errors* on non-UTF-8, so route
/// through `to_string_lossy` (invalid bytes → U+FFFD) instead, matching
/// every other renderer (SARIF, GitLab, `JUnit`, GitHub, human).
fn lossy_path(path: Option<&Path>) -> Option<Cow<'_, str>> {
    path.map(Path::to_string_lossy)
}

/// A finding's related locations, as SARIF's `relatedLocations` and the
/// language server's `relatedInformation` carry them: the 20 at most that
/// the finding retains, in the order it holds them, and how many more it
/// left out. Both are omitted when there are none, so a finding without related
/// locations renders as before. A finding whose path came from protected
/// configuration has none here, as in SARIF: a related path could reveal it.
#[derive(Serialize)]
struct JsonRelated<'a> {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    related_locations: Vec<JsonRelatedLocation<'a>>,
    #[serde(skip_serializing_if = "is_zero")]
    related_locations_omitted: usize,
}

/// One related location: its path, its label, and its range when it has
/// one (1-based lines and columns, the end exclusive, as SARIF's region).
#[derive(Serialize)]
struct JsonRelatedLocation<'a> {
    path: Cow<'a, str>,
    label: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_column: Option<usize>,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's `skip_serializing_if` passes a reference.
fn is_zero(count: &usize) -> bool {
    *count == 0
}

fn related(v: &Violation) -> JsonRelated<'_> {
    if v.path_is_sensitive() {
        return JsonRelated {
            related_locations: Vec::new(),
            related_locations_omitted: 0,
        };
    }
    JsonRelated {
        related_locations: v.related_locations.iter().map(related_location).collect(),
        related_locations_omitted: v.related_locations_omitted,
    }
}

fn related_location(location: &RelatedLocation) -> JsonRelatedLocation<'_> {
    let range = location.range.as_ref();
    JsonRelatedLocation {
        path: location.path.to_string_lossy(),
        label: &location.label,
        line: range.map(alint_core::SourceRange::start_line),
        column: range.map(alint_core::SourceRange::start_column),
        end_line: range.map(alint_core::SourceRange::end_line),
        end_column: range.map(alint_core::SourceRange::end_column),
    }
}

#[derive(Serialize)]
struct JsonReport<'a> {
    schema_version: u32,
    summary: Summary,
    results: Vec<JsonResult<'a>>,
    /// The baselined (suppressed) findings, included only under
    /// `--show-baselined`; omitted otherwise so non-baseline output stays
    /// byte-identical.
    #[serde(skip_serializing_if = "Option::is_none")]
    baselined: Option<Vec<JsonBaselined<'a>>>,
}

#[derive(Serialize)]
struct Summary {
    failing_rules: usize,
    passing_rules: usize,
    total_violations: usize,
    total_analysis_failures: usize,
    has_errors: bool,
    has_warnings: bool,
    /// Occurrences suppressed by the baseline this run, when one is in effect.
    /// Absent without `--baseline` so the envelope is unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    baselined_suppressed: Option<u64>,
}

/// A baselined finding in the JSON envelope's `baselined` list.
#[derive(Serialize)]
struct JsonBaselined<'a> {
    id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<Cow<'a, str>>,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    column: Option<usize>,
    fingerprint: &'a str,
    #[serde(flatten)]
    related: JsonRelated<'a>,
}

#[derive(Serialize)]
struct JsonResult<'a> {
    id: &'a str,
    level: Level,
    passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_url: Option<&'a str>,
    /// Whether the rule declares a fixer. Useful for downstream
    /// tools that want to decide whether suggesting `alint fix`
    /// makes sense for this rule.
    fixable: bool,
    violations: Vec<JsonViolation<'a>>,
    /// Informational notes (non-violation findings). Omitted entirely
    /// when empty, so results without notes are byte-identical to
    /// pre-v0.11 output.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    notes: Vec<JsonNote<'a>>,
    analysis_failures: Vec<JsonAnalysisFailure<'a>>,
}

#[derive(Serialize)]
struct JsonViolation<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<Cow<'a, str>>,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    column: Option<usize>,
    /// Whether a bare `alint fix` resolves THIS specific violation (a Safe,
    /// applicable fix). PER-VIOLATION, unlike the rule-level `fixable` (which only
    /// means the rule declares a fixer): an Unsafe fix, or one whose `can_fix`
    /// declines this finding, is `false` here but may leave the rule `fixable`.
    /// Matches the `agent` format's `fix_available` and the human `[fixable]` tag.
    fixable: bool,
    /// The concrete Safe fix(es) alint would apply to resolve this violation
    /// (source region + replacement text). Present only under `--include-fixes`,
    /// only for a fixable finding, and only when every target path has an exact
    /// JSON string representation; a note never carries one.
    #[serde(skip_serializing_if = "Option::is_none")]
    proposed_edit: Option<&'a [ProposedEdit]>,
    #[serde(flatten)]
    related: JsonRelated<'a>,
}

#[derive(Serialize)]
struct JsonNote<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<Cow<'a, str>>,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    column: Option<usize>,
    kind: &'static str,
    fixable: bool,
    #[serde(flatten)]
    related: JsonRelated<'a>,
}
#[derive(Serialize)]
struct JsonAnalysisFailure<'a> {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<Cow<'a, str>>,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    column: Option<usize>,
}

pub fn write_json(report: &Report, w: &mut dyn Write) -> std::io::Result<()> {
    write_json_with_baseline(report, None, false, w)
}

/// JSON with baseline awareness: the summary gains a `baselined_suppressed`
/// count, and (under `show_baselined`) the envelope gains a `baselined` list of
/// the suppressed findings. Without a baseline the envelope is byte-identical.
pub fn write_json_with_baseline(
    report: &Report,
    baseline: Option<&BaselineMarks>,
    show_baselined: bool,
    w: &mut dyn Write,
) -> std::io::Result<()> {
    let summary = Summary {
        failing_rules: report.failing_rules(),
        passing_rules: report.passing_rules(),
        total_violations: report.total_violations(),
        total_analysis_failures: report.total_analysis_failures(),
        has_errors: report.has_errors(),
        has_warnings: report.has_warnings(),
        baselined_suppressed: baseline.map(|b| b.suppressed_total),
    };
    let results: Vec<JsonResult<'_>> = report
        .results
        .iter()
        .map(|r| JsonResult {
            id: r.rule_id.as_ref(),
            level: r.level,
            passed: r.passed(),
            policy_url: r.policy_url.as_deref(),
            fixable: r.is_fixable,
            violations: r
                .violations
                .iter()
                .map(|v| JsonViolation {
                    path: lossy_path(v.report_path()),
                    message: v.message.as_ref(),
                    line: v.line,
                    column: v.column,
                    fixable: v.is_fixable,
                    proposed_edit: crate::utf8_proposed_edits(v),
                    related: related(v),
                })
                .collect(),
            notes: r
                .notes
                .iter()
                .map(|v| JsonNote {
                    path: lossy_path(v.report_path()),
                    message: v.message.as_ref(),
                    line: v.line,
                    column: v.column,
                    kind: v
                        .note_kind()
                        .unwrap_or(alint_core::NoteKind::Informational)
                        .as_str(),
                    fixable: false,
                    related: related(v),
                })
                .collect(),
            analysis_failures: r
                .analysis_failures
                .iter()
                .map(json_analysis_failure)
                .collect(),
        })
        .collect();
    let baselined = if show_baselined {
        baseline.map(|b| {
            report
                .results
                .iter()
                .zip(&b.per_result)
                .flat_map(|(rr, m)| {
                    m.suppressed.iter().map(move |sf| JsonBaselined {
                        id: rr.rule_id.as_ref(),
                        path: lossy_path(sf.violation.report_path()),
                        message: sf.violation.message.as_ref(),
                        line: sf.violation.line,
                        column: sf.violation.column,
                        fingerprint: sf.fingerprint.as_str(),
                        related: related(&sf.violation),
                    })
                })
                .collect()
        })
    } else {
        None
    };
    let out = JsonReport {
        schema_version: 1,
        summary,
        results,
        baselined,
    };
    serde_json::to_writer_pretty(&mut *w, &out)?;
    writeln!(w)?;
    Ok(())
}

#[derive(Serialize)]
struct JsonFixReport<'a> {
    schema_version: u32,
    /// `true` for `alint fix --dry-run`: nothing was written, and every
    /// `applied` item is what WOULD be applied. Distinguishes a preview from
    /// a real run without parsing the human-oriented `detail` text.
    dry_run: bool,
    summary: FixSummary,
    results: Vec<JsonFixRuleResult<'a>>,
}

#[derive(Serialize)]
struct FixSummary {
    applied: usize,
    skipped: usize,
    unfixable: usize,
    /// Fixes available but not applied (below the tier threshold, a
    /// suggestion, or verification-demoted). Always present (a machine
    /// format keeps a stable envelope); `0` when none.
    suggested: usize,
    total_analysis_failures: usize,
    /// `true` when the fix loop hit its pass cap without settling (a
    /// non-convergent config). This is the ONLY structured signal of the
    /// distinct exit 2 ("fix could not complete"): without it a capped run --
    /// whose items are mostly `applied` because the fixes kept re-firing --
    /// is indistinguishable in this envelope from a clean run. Always present;
    /// `false` for a converged run. See docs/design/v0.17/fixpoint.md.
    non_convergent: bool,
}

#[derive(Serialize)]
struct JsonFixRuleResult<'a> {
    id: &'a str,
    level: Level,
    items: Vec<JsonFixItem<'a>>,
    analysis_failures: Vec<JsonAnalysisFailure<'a>>,
}

#[derive(Serialize)]
struct JsonFixItem<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<Cow<'a, str>>,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    column: Option<usize>,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<&'a str>,
    /// For a `skipped` item, WHY it was not fixed: `declined` (the fixer
    /// declined), `errored` (an I/O error), `baselined` (grandfathered by
    /// `fix --baseline`), or `unresolved` (the fix ran, or would run, but the
    /// rule still reports the violation). Absent for other statuses.
    #[serde(skip_serializing_if = "Option::is_none")]
    skip_kind: Option<&'static str>,
}

pub fn write_fix_json(report: &FixReport, w: &mut dyn Write) -> std::io::Result<()> {
    write_fix_json_with_mode(report, false, w)
}

/// [`write_fix_json`] for a run that may be a dry run: `dry_run` is emitted
/// as the top-level `dry_run` field (statuses stay `applied` etc. for
/// compatibility; `dry_run: true` says none of them were written).
pub fn write_fix_json_with_mode(
    report: &FixReport,
    dry_run: bool,
    w: &mut dyn Write,
) -> std::io::Result<()> {
    let results: Vec<JsonFixRuleResult<'_>> = report
        .results
        .iter()
        .map(|r| JsonFixRuleResult {
            id: r.rule_id.as_ref(),
            level: r.level,
            items: r
                .items
                .iter()
                .map(|it| {
                    let (status, detail, skip_kind) = match &it.status {
                        FixStatus::Applied(s) => ("applied", Some(s.as_str()), None),
                        FixStatus::Skipped { reason: s, kind } => {
                            ("skipped", Some(s.as_str()), Some(kind.as_str()))
                        }
                        FixStatus::Suggested { summary, .. } => {
                            ("suggested", Some(summary.as_str()), None)
                        }
                        FixStatus::Unfixable => ("unfixable", None, None),
                    };
                    JsonFixItem {
                        path: lossy_path(it.violation.report_path()),
                        message: it.violation.message.as_ref(),
                        line: it.violation.line,
                        column: it.violation.column,
                        status,
                        detail,
                        skip_kind,
                    }
                })
                .collect(),
            analysis_failures: r
                .analysis_failures
                .iter()
                .map(json_analysis_failure)
                .collect(),
        })
        .collect();
    let out = JsonFixReport {
        schema_version: 1,
        dry_run,
        summary: FixSummary {
            applied: report.applied(),
            skipped: report.skipped(),
            unfixable: report.unfixable(),
            suggested: report.suggested(),
            total_analysis_failures: report.total_analysis_failures(),
            non_convergent: report.non_convergent,
        },
        results,
    };
    serde_json::to_writer_pretty(&mut *w, &out)?;
    writeln!(w)?;
    Ok(())
}

fn json_analysis_failure(failure: &AnalysisFailure) -> JsonAnalysisFailure<'_> {
    JsonAnalysisFailure {
        kind: failure.kind.as_str(),
        path: lossy_path(failure.report_path()),
        message: failure.message.as_ref(),
        line: failure.line,
        column: failure.column,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{RuleResult, Violation};

    fn render(report: &Report) -> String {
        let mut buf = Vec::new();
        write_json(report, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn notes_array_present_and_excluded_from_pass_fail() {
        let r = RuleResult::new(
            "reg".into(),
            Level::Warning,
            None,
            vec![Violation::new("skipped non-literal entry \"${X}\"").as_note()],
            false,
        );
        let out = render(&Report { results: vec![r] });
        assert!(
            out.contains("\"notes\""),
            "notes array should appear: {out}"
        );
        assert!(out.contains("skipped non-literal"), "{out}");
        // A note never counts as a violation.
        assert!(out.contains("\"passed\": true"), "{out}");
        assert!(out.contains("\"total_violations\": 0"), "{out}");
    }

    #[test]
    fn notes_omitted_when_empty() {
        let r = RuleResult::new(
            "v".into(),
            Level::Error,
            None,
            vec![Violation::new("real")],
            false,
        );
        let out = render(&Report { results: vec![r] });
        assert!(
            !out.contains("\"notes\""),
            "empty notes must be omitted: {out}"
        );
    }

    #[test]
    fn fix_json_renders_suggested_status_and_detail() {
        use alint_core::{FixEdit, FixItem, FixReport, FixRuleResult};
        use std::path::PathBuf;
        let report = FixReport {
            non_convergent: false,
            results: vec![FixRuleResult {
                rule_id: "demo".into(),
                level: Level::Error,
                items: vec![FixItem {
                    violation: Violation::new("v").with_path(PathBuf::from("a.txt")),
                    status: FixStatus::Suggested {
                        summary: "would set $.x to 1".into(),
                        edit: Some(FixEdit::SetContent {
                            path: PathBuf::from("a.txt"),
                            content: Vec::new(),
                        }),
                    },
                }],
                analysis_failures: Vec::new(),
            }],
        };
        let mut buf = Vec::new();
        write_fix_json(&report, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("\"suggested\""), "status should render: {s}");
        assert!(
            s.contains("would set $.x to 1"),
            "detail should render: {s}"
        );
    }

    #[test]
    fn baseline_adds_suppressed_count_and_list() {
        use crate::{BaselineMarks, ResultMarks, SuppressedFinding};
        let r = RuleResult::new(
            "no-todo".into(),
            Level::Error,
            None,
            vec![Violation::new("new")],
            false,
        );
        let report = Report { results: vec![r] };
        let marks = BaselineMarks {
            per_result: vec![ResultMarks {
                live_fingerprints: vec!["fp-live".into()],
                suppressed: vec![SuppressedFinding {
                    violation: Violation::new("old"),
                    fingerprint: "fp-supp".into(),
                }],
            }],
            suppressed_total: 1,
        };
        let mut buf = Vec::new();
        write_json_with_baseline(&report, Some(&marks), true, &mut buf).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(v["summary"]["baselined_suppressed"], 1);
        let baselined = v["baselined"].as_array().unwrap();
        assert_eq!(baselined.len(), 1);
        assert_eq!(baselined[0]["id"], "no-todo");
        assert_eq!(baselined[0]["message"], "old");
        assert_eq!(baselined[0]["fingerprint"], "fp-supp");

        // Without `--show-baselined` the list is omitted but the count stays.
        let mut buf2 = Vec::new();
        write_json_with_baseline(&report, Some(&marks), false, &mut buf2).unwrap();
        let v2: serde_json::Value = serde_json::from_slice(&buf2).unwrap();
        assert_eq!(v2["summary"]["baselined_suppressed"], 1);
        assert!(v2.get("baselined").is_none());
    }

    #[test]
    fn no_baseline_omits_suppressed_fields() {
        let r = RuleResult::new(
            "r".into(),
            Level::Error,
            None,
            vec![Violation::new("v")],
            false,
        );
        let out = render(&Report { results: vec![r] });
        assert!(!out.contains("baselined_suppressed"), "{out}");
        assert!(!out.contains("\"baselined\""), "{out}");
    }

    /// A finding with three related locations, the second without a range,
    /// that left out two more.
    fn related_finding(message: &'static str) -> Violation {
        let range = |line, column| alint_core::SourceRange::new(line, column, line, column + 4);
        let mut finding = Violation::new(message)
            .with_path(std::path::PathBuf::from("a.json"))
            .with_related_locations([
                alint_core::RelatedLocation::new("b.json", "a row with another value")
                    .with_range(range(2, 3).unwrap()),
                alint_core::RelatedLocation::new("c.json", "the source row"),
                alint_core::RelatedLocation::new("b.json", "another row with this value")
                    .with_range(range(1, 9).unwrap()),
            ]);
        finding.related_locations_omitted = 2;
        finding
    }

    /// What `related_locations` holds for each finding: `path:line:column-
    /// end_line:end_column label`, or `path label` without a range.
    fn shown_related(finding: &serde_json::Value) -> Vec<String> {
        finding["related_locations"]
            .as_array()
            .map(|locations| {
                locations
                    .iter()
                    .map(|location| match location.get("line") {
                        Some(line) => format!(
                            "{}:{line}:{}-{}:{} {}",
                            location["path"].as_str().unwrap(),
                            location["column"],
                            location["end_line"],
                            location["end_column"],
                            location["label"].as_str().unwrap()
                        ),
                        None => format!(
                            "{} {}",
                            location["path"].as_str().unwrap(),
                            location["label"].as_str().unwrap()
                        ),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn related_locations_reach_violations_notes_and_baselined_findings_in_order() {
        use crate::{BaselineMarks, ResultMarks, SuppressedFinding};
        let finding = related_finding("live");
        let held: Vec<String> = finding
            .related_locations
            .iter()
            .map(|location| format!("{} {}", location.path.display(), location.label))
            .collect();
        let report = Report {
            results: vec![RuleResult::new(
                "r".into(),
                Level::Error,
                None,
                vec![
                    finding,
                    related_finding("a note").as_note(),
                    Violation::new("plain"),
                ],
                false,
            )],
        };
        let marks = BaselineMarks {
            per_result: vec![ResultMarks {
                live_fingerprints: vec!["fp-live".into(), "fp-plain".into()],
                suppressed: vec![SuppressedFinding {
                    violation: related_finding("old"),
                    fingerprint: "fp-old".into(),
                }],
            }],
            suppressed_total: 1,
        };
        let mut buf = Vec::new();
        write_json_with_baseline(&report, Some(&marks), true, &mut buf).unwrap();
        let out: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        // The order the finding holds, which sorts them by path and range.
        let expected = [
            "b.json:1:9-1:13 another row with this value",
            "b.json:2:3-2:7 a row with another value",
            "c.json the source row",
        ];
        assert_eq!(
            held,
            [
                "b.json another row with this value",
                "b.json a row with another value",
                "c.json the source row"
            ]
        );
        let result = &out["results"][0];
        for finding in [
            &result["violations"][0],
            &result["notes"][0],
            &out["baselined"][0],
        ] {
            assert_eq!(shown_related(finding), expected, "{finding}");
            assert_eq!(finding["related_locations_omitted"], 2, "{finding}");
        }
        // Neither field appears on a finding without related locations.
        let plain = &result["violations"][1];
        assert_eq!(plain["message"], "plain");
        assert!(plain.get("related_locations").is_none(), "{plain}");
        assert!(plain.get("related_locations_omitted").is_none(), "{plain}");
    }

    #[test]
    fn a_protected_path_leaves_out_every_related_location_as_sarif_does() {
        let mut finding = related_finding("protected");
        finding.path_sensitive = true;
        let report = Report {
            results: vec![RuleResult::new(
                "r".into(),
                Level::Error,
                None,
                vec![finding],
                false,
            )],
        };
        let out: serde_json::Value = serde_json::from_str(&render(&report)).unwrap();
        let violation = &out["results"][0]["violations"][0];
        assert_eq!(violation["path"], "<redacted>");
        assert!(violation.get("related_locations").is_none(), "{violation}");
        assert!(
            violation.get("related_locations_omitted").is_none(),
            "{violation}"
        );
        let mut sarif = Vec::new();
        crate::write_sarif(&report, &mut sarif).unwrap();
        let sarif: serde_json::Value = serde_json::from_slice(&sarif).unwrap();
        assert!(
            sarif["runs"][0]["results"][0]
                .get("relatedLocations")
                .is_none()
        );
    }
}
