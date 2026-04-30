//! Schema-validation test for `alint fix --format json`.
//!
//! Pairs with the check-report schema test in `cross_formatter`.
//! Constructs a canonical `FixReport` exercising every
//! `FixStatus` variant and asserts the rendered JSON validates
//! against the published `schemas/v1/fix-report.json`.
//!
//! Locks the public contract for downstream tools that consume
//! `alint fix --format json` (CI fix-applier scripts, IDE
//! integrations).

use alint_core::{FixItem, FixReport, FixRuleResult, FixStatus, Level, Violation};
use alint_output::write_fix_json;

const FIX_REPORT_SCHEMA: &str = include_str!("../../../schemas/v1/fix-report.json");

fn canonical_fix_report() -> FixReport {
    FixReport {
        results: vec![
            FixRuleResult {
                rule_id: "no-trailing-whitespace".into(),
                level: Level::Warning,
                items: vec![
                    FixItem {
                        violation: Violation::new("trailing whitespace on line 12")
                            .with_path(std::path::Path::new("README.md"))
                            .with_location(12, 1),
                        status: FixStatus::Applied("rewrote 1 line".into()),
                    },
                    FixItem {
                        violation: Violation::new("trailing whitespace on line 3")
                            .with_path(std::path::Path::new("CONTRIBUTING.md"))
                            .with_location(3, 1),
                        status: FixStatus::Skipped("file no longer exists".into()),
                    },
                ],
            },
            FixRuleResult {
                rule_id: "no-debugger-statements".into(),
                level: Level::Error,
                items: vec![FixItem {
                    violation: Violation::new("`debugger;` left in committed source")
                        .with_path(std::path::Path::new("src/main.ts"))
                        .with_location(42, 1),
                    status: FixStatus::Unfixable,
                }],
            },
        ],
    }
}

fn render_json(report: &FixReport) -> String {
    let mut buf = Vec::new();
    write_fix_json(report, &mut buf).unwrap();
    String::from_utf8(buf).unwrap()
}

fn validate(json_text: &str) {
    let instance: serde_json::Value =
        serde_json::from_str(json_text).expect("fix-report JSON parses");
    let schema_value: serde_json::Value =
        serde_json::from_str(FIX_REPORT_SCHEMA).expect("schema is valid JSON");
    let validator =
        jsonschema::validator_for(&schema_value).expect("schema compiles as Draft 2020-12");
    let errs: Vec<String> = validator
        .iter_errors(&instance)
        .map(|e| format!("{} at {}", e, e.instance_path))
        .collect();
    assert!(
        errs.is_empty(),
        "fix-report JSON failed schema validation:\n{}\n\noutput:\n{json_text}",
        errs.join("\n"),
    );
}

#[test]
fn fix_report_validates_against_published_schema() {
    let report = canonical_fix_report();
    validate(&render_json(&report));
}

#[test]
fn empty_fix_report_validates_against_published_schema() {
    let report = FixReport {
        results: Vec::new(),
    };
    validate(&render_json(&report));
}

#[test]
fn fix_report_shape_matches_expected_keys() {
    let report = canonical_fix_report();
    let json: serde_json::Value = serde_json::from_str(&render_json(&report)).unwrap();
    assert_eq!(json["schema_version"], 1);
    let summary = &json["summary"];
    assert_eq!(summary["applied"], 1);
    assert_eq!(summary["skipped"], 1);
    assert_eq!(summary["unfixable"], 1);
    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
}
