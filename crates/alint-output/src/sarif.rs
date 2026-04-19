//! SARIF 2.1.0 output. Each `alint` invocation becomes one `run` with a
//! `tool.driver` section describing alint and every rule that contributed
//! to the report. Each violation becomes one `result` with a
//! `physicalLocation` anchored on the violating path (+ line/column when
//! the rule recorded them).
//!
//! Targets GitHub Code Scanning's SARIF uploader; fields are deliberately
//! a subset of SARIF 2.1.0 — just enough that GitHub renders the findings
//! in the Security → Code scanning tab.

use std::io::Write;

use alint_core::{Level, Report};
use serde::Serialize;

pub fn write_sarif(report: &Report, w: &mut dyn Write) -> std::io::Result<()> {
    let sarif = build_sarif(report);
    serde_json::to_writer_pretty(&mut *w, &sarif)?;
    writeln!(w)?;
    Ok(())
}

fn build_sarif(report: &Report) -> Sarif {
    let mut rules = Vec::with_capacity(report.results.len());
    let mut results = Vec::new();

    for rr in &report.results {
        rules.push(SarifRule {
            id: rr.rule_id.clone(),
            short_description: SarifText {
                text: format!("alint rule `{}`", rr.rule_id),
            },
            help_uri: rr.policy_url.clone(),
        });

        for v in &rr.violations {
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
            results.push(SarifResult {
                rule_id: rr.rule_id.clone(),
                level: level_to_sarif(rr.level),
                message: SarifText {
                    text: v.message.clone(),
                },
                locations,
            });
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
    use std::path::PathBuf;

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
                },
                RuleResult {
                    rule_id: "rule-b".into(),
                    level: Level::Warning,
                    policy_url: None,
                    violations: vec![Violation::new("vb")],
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
                },
                RuleResult {
                    rule_id: "info".into(),
                    level: Level::Info,
                    policy_url: None,
                    violations: vec![Violation::new("y")],
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
                    path: Some(PathBuf::from("src/lib.rs")),
                    message: "m".into(),
                    line: Some(7),
                    column: Some(3),
                }],
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
            }],
        };
        let v = render(&report);
        let locs = v["runs"][0]["results"][0]["locations"].as_array().unwrap();
        assert!(locs.is_empty());
    }
}
