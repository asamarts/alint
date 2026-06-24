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

use alint_core::{Level, Report, Violation};
use serde::Serialize;

use crate::{BaselineMarks, ResultMarks};

/// `partialFingerprints` key carrying alint's baseline fingerprint, so GitHub
/// Code Scanning's alert correlation aligns with the baseline.
const FINGERPRINT_KEY: &str = "alint/v1";

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
    let sarif = build_sarif(report, baseline);
    serde_json::to_writer_pretty(&mut *w, &sarif)?;
    writeln!(w)?;
    Ok(())
}

fn build_sarif(report: &Report, baseline: Option<&BaselineMarks>) -> Sarif {
    let mut rules = Vec::with_capacity(report.results.len());
    let mut results = Vec::new();

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
            let mut res = base_result(rr.rule_id.as_ref(), rr.level, v);
            if let Some(fp) = marks.and_then(|m| m.live_fingerprints.get(vi)) {
                res.baseline_state = Some("new");
                res.partial_fingerprints = Some(fingerprint_map(fp));
            }
            results.push(res);
        }

        // Baselined findings — marked, not removed, so Code Scanning dismisses
        // (rather than closes-then-reopens) the alert.
        if let Some(m) = marks {
            for sf in &m.suppressed {
                let mut res = base_result(rr.rule_id.as_ref(), rr.level, &sf.violation);
                res.suppressions = vec![SarifSuppression { kind: "external" }];
                res.baseline_state = Some("unchanged");
                res.partial_fingerprints = Some(fingerprint_map(&sf.fingerprint));
                results.push(res);
            }
        }
    }

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

/// A SARIF result with the shared fields filled in (no baseline annotations).
fn base_result(rule_id: &str, level: Level, v: &Violation) -> SarifResult {
    let region = if v.line.is_some() || v.column.is_some() {
        Some(SarifRegion {
            start_line: v.line,
            start_column: v.column,
        })
    } else {
        None
    };
    let locations = if let Some(path) = &v.path {
        vec![SarifLocation {
            physical_location: SarifPhysicalLocation {
                artifact_location: SarifArtifactLocation {
                    uri: path.display().to_string(),
                },
                region,
            },
        }]
    } else {
        Vec::new()
    };
    SarifResult {
        rule_id: rule_id.to_string(),
        level: level_to_sarif(level),
        message: SarifText {
            text: v.message.to_string(),
        },
        locations,
        suppressions: Vec::new(),
        baseline_state: None,
        partial_fingerprints: None,
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
    results: Vec<SarifResult>,
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
    message: SarifText,
    locations: Vec<SarifLocation>,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{Report, RuleResult, Violation};
    use serde_json::Value;
    use std::path::Path;

    fn render(report: &Report) -> Value {
        let mut buf = Vec::new();
        write_sarif(report, &mut buf).unwrap();
        serde_json::from_slice(&buf).unwrap()
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
                },
                RuleResult {
                    rule_id: "rule-b".into(),
                    level: Level::Warning,
                    policy_url: None,
                    violations: vec![Violation::new("vb")],
                    notes: Vec::new(),
                    is_fixable: false,
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
                },
                RuleResult {
                    rule_id: "info".into(),
                    level: Level::Info,
                    policy_url: None,
                    violations: vec![Violation::new("y")],
                    notes: Vec::new(),
                    is_fixable: false,
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
                violations: vec![Violation {
                    path: Some(Path::new("src/lib.rs").into()),
                    message: "m".into(),
                    line: Some(7),
                    column: Some(3),
                    is_note: false,
                    baseline_key: None,
                }],
                notes: Vec::new(),
                is_fixable: false,
            }],
        };
        let v = render(&report);
        let loc = &v["runs"][0]["results"][0]["locations"][0]["physicalLocation"];
        assert_eq!(loc["artifactLocation"]["uri"], "src/lib.rs");
        assert_eq!(loc["region"]["startLine"], 7);
        assert_eq!(loc["region"]["startColumn"], 3);
    }

    #[test]
    fn violations_without_path_emit_empty_locations() {
        let report = Report {
            results: vec![RuleResult {
                rule_id: "r".into(),
                level: Level::Error,
                policy_url: None,
                violations: vec![Violation::new("no-path")],
                notes: Vec::new(),
                is_fixable: false,
            }],
        };
        let v = render(&report);
        let locs = v["runs"][0]["results"][0]["locations"].as_array().unwrap();
        assert!(locs.is_empty());
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
            }],
        };
        let v = render(&report); // write_sarif — no baseline
        let r = &v["runs"][0]["results"][0];
        assert!(r.get("baselineState").is_none());
        assert!(r.get("suppressions").is_none());
        assert!(r.get("partialFingerprints").is_none());
    }
}
