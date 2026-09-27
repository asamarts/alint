//! Cross-formatter consistency tests — the keystone of v0.8.2.
//!
//! Each formatter has its own unit tests in its module; what
//! those don't catch is divergence between formatters when the
//! same `Report` is rendered eight different ways. This file
//! constructs a single canonical Report, renders it through
//! every `Format` variant via `write_with_options`, and asserts
//! invariants that must hold across all formats:
//!
//! - Every rule with violations appears in every format's output.
//! - Every violation's path appears in every format's output.
//! - Output is non-empty for every format.
//! - JSON-shaped formats (`json`, `sarif`, `gitlab`, `agent`)
//!   parse as valid JSON.
//! - The `junit` format parses as well-formed XML (sanity-check
//!   start/end tag balance — full XML conformance is the
//!   formatter's own concern).
//! - Severity counts match the input report.
//!
//! New formatters must keep these invariants. A formatter that
//! silently drops violations — historically a regression vector
//! — will fail the rule_id-presence check immediately.

use alint_core::{Level, Report, RuleResult, Violation};
use alint_output::Format;

fn canonical_report() -> Report {
    // 3 rules across all three actionable severities, 5
    // violations across varied paths and locations:
    //   - error rule: 2 violations (one with a line, one without)
    //   - warning rule: 2 violations (different files)
    //   - info rule: 1 violation (cross-tree, no path)
    Report {
        results: vec![
            RuleResult {
                rule_id: "no-debugger-statements".into(),
                level: Level::Error,
                policy_url: Some("https://example.com/policy".into()),
                violations: vec![
                    Violation::new("`debugger;` left in committed source")
                        .with_path(std::path::Path::new("src/main.ts"))
                        .with_location(42, 1),
                    Violation::new("`breakpoint()` left in committed source")
                        .with_path(std::path::Path::new("src/util.ts"))
                        .with_location(7, 4),
                ],
                notes: Vec::new(),
                is_fixable: false,
            },
            RuleResult {
                rule_id: "no-trailing-whitespace".into(),
                level: Level::Warning,
                policy_url: None,
                violations: vec![
                    Violation::new("trailing whitespace on line 12")
                        .with_path(std::path::Path::new("README.md"))
                        .with_location(12, 1),
                    Violation::new("trailing whitespace on line 3")
                        .with_path(std::path::Path::new("CONTRIBUTING.md"))
                        .with_location(3, 1),
                ],
                notes: Vec::new(),
                is_fixable: true,
            },
            RuleResult {
                rule_id: "stable-rule-id-format".into(),
                level: Level::Info,
                policy_url: None,
                violations: vec![Violation::new("informational notice — no path")],
                notes: Vec::new(),
                is_fixable: false,
            },
        ],
    }
}

/// All formats that consume a check-`Report`.
const FORMATS: &[(Format, &str)] = &[
    (Format::Human, "human"),
    (Format::Json, "json"),
    (Format::Sarif, "sarif"),
    (Format::Github, "github"),
    (Format::Markdown, "markdown"),
    (Format::Junit, "junit"),
    (Format::Gitlab, "gitlab"),
    (Format::Agent, "agent"),
];

fn render(format: Format) -> String {
    let report = canonical_report();
    let mut buf = Vec::new();
    format
        .write(&report, &mut buf)
        .unwrap_or_else(|e| panic!("formatter panicked: {e}"));
    String::from_utf8(buf).unwrap_or_else(|e| panic!("formatter emitted non-UTF-8: {e}"))
}

#[test]
fn every_format_produces_non_empty_output() {
    for (format, name) in FORMATS {
        let out = render(*format);
        assert!(
            !out.is_empty(),
            "format `{name}` produced empty output for a non-empty report",
        );
    }
}

#[test]
fn every_format_includes_every_rule_id() {
    let report = canonical_report();
    let rule_ids: Vec<&str> = report.results.iter().map(|r| r.rule_id.as_ref()).collect();
    for (format, name) in FORMATS {
        let out = render(*format);
        for rule_id in &rule_ids {
            assert!(
                out.contains(rule_id),
                "format `{name}` is missing rule_id `{rule_id}`. Output:\n{out}",
            );
        }
    }
}

#[test]
fn every_format_includes_every_path() {
    let paths = ["src/main.ts", "src/util.ts", "README.md", "CONTRIBUTING.md"];
    for (format, name) in FORMATS {
        let out = render(*format);
        for p in &paths {
            assert!(
                out.contains(p),
                "format `{name}` is missing path `{p}`. Output:\n{out}",
            );
        }
    }
}

#[test]
fn json_shaped_formats_parse_as_valid_json() {
    for (format, name) in &[
        (Format::Json, "json"),
        (Format::Sarif, "sarif"),
        (Format::Gitlab, "gitlab"),
        (Format::Agent, "agent"),
    ] {
        let out = render(*format);
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&out);
        assert!(
            parsed.is_ok(),
            "format `{name}` failed to parse as JSON: {:?}\n=== output ===\n{out}",
            parsed.err().unwrap(),
        );
    }
}

#[test]
fn junit_xml_has_balanced_root_tags() {
    let out = render(Format::Junit);
    assert!(
        out.contains("<testsuites"),
        "junit missing <testsuites> root: {out}",
    );
    assert!(
        out.contains("</testsuites>"),
        "junit missing </testsuites> close: {out}",
    );
}

#[test]
fn json_format_carries_stable_schema_version() {
    // Downstream consumers pin to alint's JSON schema by version.
    // Locking the field's presence here is a backstop for
    // accidental shape changes.
    let out = render(Format::Json);
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(
        parsed.get("schema_version").is_some(),
        "JSON format missing `schema_version` key: {out}",
    );
}

#[test]
fn json_format_records_each_violation() {
    let out = render(Format::Json);
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    // Sum violations across results.
    let results = parsed["results"]
        .as_array()
        .expect("`results` must be an array");
    let total: usize = results
        .iter()
        .map(|r| r["violations"].as_array().map_or(0, Vec::len))
        .sum();
    assert_eq!(total, 5, "JSON format should carry all 5 violations: {out}");
}

#[test]
fn sarif_format_includes_every_rule_in_driver() {
    // SARIF requires every rule referenced by a result to also
    // appear in `runs[0].tool.driver.rules`. A formatter bug
    // that drops driver entries is a known regression vector.
    let out = render(Format::Sarif);
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    let drivers = parsed["runs"][0]["tool"]["driver"]["rules"]
        .as_array()
        .expect("SARIF runs[0].tool.driver.rules must be an array");
    let driver_ids: Vec<&str> = drivers
        .iter()
        .map(|r| r["id"].as_str().unwrap_or(""))
        .collect();
    for rule_id in [
        "no-debugger-statements",
        "no-trailing-whitespace",
        "stable-rule-id-format",
    ] {
        assert!(
            driver_ids.contains(&rule_id),
            "SARIF driver missing rule `{rule_id}`. Driver ids: {driver_ids:?}",
        );
    }
}

#[test]
fn agent_format_carries_agent_instruction_field() {
    // The agent format's distinguishing feature is the
    // `agent_instruction` field on each violation, shaped for
    // self-correction loops. Lock its presence.
    let out = render(Format::Agent);
    assert!(
        out.contains("agent_instruction"),
        "agent format missing `agent_instruction` field: {out}",
    );
}

#[test]
fn empty_report_renders_cleanly_in_every_format() {
    // The other regression vector: empty reports should not
    // panic, emit invalid JSON, or carry violation tags with
    // empty content.
    let empty = Report { results: vec![] };
    for (format, name) in FORMATS {
        let mut buf = Vec::new();
        format
            .write(&empty, &mut buf)
            .unwrap_or_else(|e| panic!("format `{name}` panicked on empty report: {e}"));
        let text =
            String::from_utf8(buf).unwrap_or_else(|e| panic!("`{name}` emitted non-UTF-8: {e}"));
        // Format-specific shape sanity checks for non-trivial
        // formats. The human/markdown/github "no violations" line
        // is enough for those formats.
        match format {
            Format::Json | Format::Sarif | Format::Gitlab | Format::Agent => {
                serde_json::from_str::<serde_json::Value>(&text).unwrap_or_else(|e| {
                    panic!("`{name}` empty-report output didn't parse as JSON: {e}\n{text}")
                });
            }
            Format::Junit => {
                assert!(
                    text.contains("<testsuites") && text.contains("</testsuites>"),
                    "junit empty-report missing balanced testsuites tags: {text}",
                );
            }
            _ => {}
        }
    }
}

#[test]
fn from_str_accepts_every_canonical_format_name() {
    use std::str::FromStr;
    for (format, name) in FORMATS {
        let parsed = Format::from_str(name).unwrap_or_else(|e| panic!("`{name}` parse error: {e}"));
        assert_eq!(parsed, *format, "name `{name}` round-tripped wrong");
    }
}

const CHECK_REPORT_SCHEMA: &str = include_str!("../../../schemas/v1/check-report.json");

#[test]
fn json_format_validates_against_published_schema() {
    let json_text = render(Format::Json);
    let instance: serde_json::Value =
        serde_json::from_str(&json_text).expect("json formatter emits valid JSON");
    let schema_value: serde_json::Value =
        serde_json::from_str(CHECK_REPORT_SCHEMA).expect("schema is valid JSON");
    let validator =
        jsonschema::validator_for(&schema_value).expect("schema compiles as Draft 2020-12");
    let errs: Vec<String> = validator
        .iter_errors(&instance)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect();
    assert!(
        errs.is_empty(),
        "JSON output failed schema validation:\n{}\n\noutput:\n{json_text}",
        errs.join("\n"),
    );
}

#[test]
fn empty_report_json_validates_against_published_schema() {
    let report = Report {
        results: Vec::new(),
    };
    let mut buf = Vec::new();
    Format::Json.write(&report, &mut buf).unwrap();
    let json_text = String::from_utf8(buf).unwrap();
    let instance: serde_json::Value = serde_json::from_str(&json_text).unwrap();
    let schema_value: serde_json::Value = serde_json::from_str(CHECK_REPORT_SCHEMA).unwrap();
    let validator = jsonschema::validator_for(&schema_value).unwrap();
    let errs: Vec<String> = validator
        .iter_errors(&instance)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect();
    assert!(
        errs.is_empty(),
        "empty-report JSON failed schema validation:\n{}",
        errs.join("\n"),
    );
}

const AGENT_REPORT_SCHEMA: &str = include_str!("../../../schemas/v1/agent-report.json");

/// Validate `instance` against `schema_str`, returning the (path-annotated) errors.
fn schema_errors(instance: &serde_json::Value, schema_str: &str) -> Vec<String> {
    let schema: serde_json::Value = serde_json::from_str(schema_str).expect("schema is valid JSON");
    let validator = jsonschema::validator_for(&schema).expect("schema compiles as Draft 2020-12");
    validator
        .iter_errors(instance)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect()
}

// AUDIT R2 (HIGH-2): the `agent` format ALWAYS emits `fix_command` + `proposed_edit`
// for a fix-available finding, but the published agent-report.json omitted them, so
// every agent run with a fixable finding was schema-INVALID. This exercises exactly
// that path (the canonical report has a fixable warning) with a proposed_edit
// attached, and validates against the real schema.
#[test]
fn agent_format_validates_against_published_schema() {
    let report = report_with_a_proposed_edit();
    let mut buf = Vec::new();
    alint_output::write_agent(&report, &mut buf).unwrap();
    let instance: serde_json::Value = serde_json::from_slice(&buf).unwrap();
    let errs = schema_errors(&instance, AGENT_REPORT_SCHEMA);
    assert!(
        errs.is_empty(),
        "agent output failed agent-report.json validation:\n{}\n\noutput:\n{}",
        errs.join("\n"),
        String::from_utf8_lossy(&buf)
    );
}

// AUDIT R2 (HIGH-1): `check --format json --include-fixes` emits `proposed_edit` on a
// violation, which check-report.json's `additionalProperties: false` rejected. The
// existing `json_format_validates_*` uses a report with NO proposed_edit, so it was a
// false-green. This attaches a real proposed_edit and validates.
#[test]
fn json_with_a_proposed_edit_validates_against_published_schema() {
    let report = report_with_a_proposed_edit();
    let mut buf = Vec::new();
    Format::Json.write(&report, &mut buf).unwrap();
    let instance: serde_json::Value = serde_json::from_slice(&buf).unwrap();
    let errs = schema_errors(&instance, CHECK_REPORT_SCHEMA);
    assert!(
        errs.is_empty(),
        "json+proposed_edit failed check-report.json validation:\n{}",
        errs.join("\n"),
    );
}

// AUDIT R2 (HIGH-1b): `check --format json --baseline` adds `summary.baselined_suppressed`
// (and, with `--show-baselined`, a top-level `baselined` list) that the schema rejected.
#[test]
fn json_with_baseline_validates_against_published_schema() {
    use alint_core::RuleResult;
    use alint_output::{BaselineMarks, ResultMarks, SuppressedFinding};
    let report = Report {
        results: vec![RuleResult::new(
            "no-todo".into(),
            Level::Error,
            None,
            vec![Violation::new("new").with_path(std::path::Path::new("a.rs"))],
            false,
        )],
    };
    let marks = BaselineMarks {
        per_result: vec![ResultMarks {
            live_fingerprints: vec!["fp-live".into()],
            suppressed: vec![SuppressedFinding {
                violation: Violation::new("old").with_path(std::path::Path::new("a.rs")),
                fingerprint: "fp-supp".into(),
            }],
        }],
        suppressed_total: 1,
    };
    for show in [true, false] {
        let mut buf = Vec::new();
        alint_output::write_json_with_baseline(&report, Some(&marks), show, &mut buf).unwrap();
        let instance: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        let errs = schema_errors(&instance, CHECK_REPORT_SCHEMA);
        assert!(
            errs.is_empty(),
            "json+baseline (show_baselined={show}) failed check-report.json validation:\n{}",
            errs.join("\n"),
        );
    }
}

// AUDIT R2 MED-1: `github` / `markdown` (like `human`) must neutralize terminal
// control bytes from an attacker-named path or message -- previewing either format
// in a terminal on an untrusted repo must not fire an escape sequence. No raw ESC or
// BEL may survive. (These machine formats emit no alint color of their own, so any
// ESC/BEL is an injected leak.)
#[test]
fn github_and_markdown_neutralize_terminal_control_bytes() {
    let report = Report {
        results: vec![RuleResult {
            rule_id: "ctl".into(),
            level: Level::Error,
            policy_url: None,
            violations: vec![
                Violation::new("msg with \u{1b}[2J and \u{7} bell")
                    .with_path(std::path::Path::new("ev\u{1b}[2Il\u{7}.md"))
                    .with_location(1, 1),
            ],
            notes: Vec::new(),
            is_fixable: false,
        }],
    };
    for (format, name) in [(Format::Github, "github"), (Format::Markdown, "markdown")] {
        let mut buf = Vec::new();
        format.write(&report, &mut buf).unwrap();
        assert!(
            !buf.contains(&0x1b),
            "[{name}] leaked a raw ESC (0x1b) to the terminal"
        );
        assert!(
            !buf.contains(&0x07),
            "[{name}] leaked a raw BEL (0x07) to the terminal"
        );
    }
}

/// The canonical report with a `proposed_edit` attached to its fixable violation, so
/// the json / agent fix-carrying fields are exercised against their schemas.
fn report_with_a_proposed_edit() -> Report {
    use alint_core::{EditRegion, ProposedEdit};
    let mut report = canonical_report();
    // The warning rule (index 1) is `is_fixable`; attach a concrete edit to its first
    // violation so `proposed_edit` (and, for agent, `fix_command`) appears.
    report.results[1].violations[0].proposed_edits = vec![ProposedEdit {
        path: std::path::PathBuf::from("README.md"),
        region: EditRegion {
            start_line: 12,
            start_column: 1,
            end_line: 12,
            end_column: 3,
        },
        inserted: String::new(),
    }];
    report
}
