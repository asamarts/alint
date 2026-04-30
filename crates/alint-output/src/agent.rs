//! `--format=agent` — JSON output shaped for AI coding agents that
//! consume alint inside their own self-correction loops.
//!
//! Differences vs. `--format=json`:
//!
//! - **Flat list of violations**, not a list of rule-results each
//!   containing a list of violations. An agent typically wants to
//!   address one violation at a time; the flat shape removes the
//!   nested-loop step.
//! - **`agent_instruction` field per violation** — a templated
//!   sentence describing what to do to resolve it, optimised for an
//!   LLM to act on. The existing `--format=json` shape leaves that
//!   synthesis to the consumer.
//! - **`severity` is the lowercase string** (`"error"` / `"warning"`
//!   / `"info"`) rather than the SARIF mapping. Easier to surface
//!   in agent prompts.
//! - **No fix-report variant.** `alint fix` already emits a clean
//!   JSON report via `--format=json`; an agent that wants to confirm
//!   a fix landed reads that. The `agent` format is purely
//!   check-side.
//!
//! The format is stable behind `schema_version: 1`. Field additions
//! are non-breaking; field removals or semantic changes bump the
//! version.

use std::io::Write;
use std::path::Path;

use alint_core::{Level, Report, RuleResult, Violation};
use serde::Serialize;

#[derive(Serialize)]
struct AgentReport<'a> {
    schema_version: u32,
    format: &'static str,
    summary: AgentSummary,
    violations: Vec<AgentViolation<'a>>,
}

#[derive(Serialize)]
struct AgentSummary {
    total_violations: usize,
    by_severity: BySeverity,
    fixable_violations: usize,
    passing_rules: usize,
    failing_rules: usize,
}

#[derive(Serialize)]
struct BySeverity {
    error: usize,
    warning: usize,
    info: usize,
}

#[derive(Serialize)]
struct AgentViolation<'a> {
    rule_id: &'a str,
    severity: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<&'a Path>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    column: Option<usize>,
    /// The rule's violation message verbatim, suitable for human
    /// review. Mirror of the `message` field in `--format=json`.
    human_message: &'a str,
    /// Templated remediation phrasing optimised for an LLM to act
    /// on. Composed from severity, message, location, fix
    /// availability, and policy URL — see `build_agent_instruction`.
    agent_instruction: String,
    fix_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_url: Option<&'a str>,
}

pub fn write_agent(report: &Report, w: &mut dyn Write) -> std::io::Result<()> {
    let mut violations: Vec<AgentViolation<'_>> = Vec::new();
    let mut by_sev = BySeverity {
        error: 0,
        warning: 0,
        info: 0,
    };
    let mut fixable_violations = 0;

    for r in &report.results {
        for v in &r.violations {
            match r.level {
                Level::Error => by_sev.error += 1,
                Level::Warning => by_sev.warning += 1,
                Level::Info => by_sev.info += 1,
                Level::Off => {} // off rules don't produce violations, but be explicit
            }
            if r.is_fixable {
                fixable_violations += 1;
            }
            violations.push(AgentViolation {
                rule_id: r.rule_id.as_ref(),
                severity: severity_str(r.level),
                file: v.path.as_deref(),
                line: v.line,
                column: v.column,
                human_message: v.message.as_ref(),
                agent_instruction: build_agent_instruction(r, v),
                fix_available: r.is_fixable,
                policy_url: r.policy_url.as_deref(),
            });
        }
    }

    let total = violations.len();
    let out = AgentReport {
        schema_version: 1,
        format: "agent",
        summary: AgentSummary {
            total_violations: total,
            by_severity: by_sev,
            fixable_violations,
            passing_rules: report.passing_rules(),
            failing_rules: report.failing_rules(),
        },
        violations,
    };
    serde_json::to_writer_pretty(&mut *w, &out)?;
    writeln!(w)?;
    Ok(())
}

fn severity_str(level: Level) -> &'static str {
    match level {
        Level::Error => "error",
        Level::Warning => "warning",
        Level::Info => "info",
        Level::Off => "off",
    }
}

/// Compose the per-violation `agent_instruction`. Structure:
///
/// ```text
/// <severity>: <human_message>
/// To resolve: <action><location>[<fix_hint>][<policy_hint>]
/// ```
///
/// Concrete shapes:
///
/// - Path-bound, fixable: `"warning: README is missing. To
///   resolve: edit README — or run `alint fix --only
///   readme-exists` to apply the auto-fix."`
/// - Path-bound, non-fixable: `"warning: console.log left in
///   non-test source. To resolve: edit src/api.ts:2:1."`
/// - Cross-file (no path): `"error: Multiple lockfiles present.
///   To resolve: this is a repository-level rule with no
///   specific file location; resolve the underlying invariant
///   the rule enforces."`
fn build_agent_instruction(rule: &RuleResult, violation: &Violation) -> String {
    let mut out = String::new();
    out.push_str(severity_str(rule.level));
    out.push_str(": ");
    out.push_str(&violation.message);
    if !out.ends_with('.') {
        out.push('.');
    }

    out.push_str(" To resolve: ");
    match &violation.path {
        Some(path) => {
            out.push_str("edit ");
            out.push_str(&path.display().to_string());
            if let Some(line) = violation.line {
                out.push(':');
                out.push_str(&line.to_string());
                if let Some(column) = violation.column {
                    out.push(':');
                    out.push_str(&column.to_string());
                }
            }
        }
        None => {
            out.push_str(
                "this is a repository-level rule with no specific \
                 file location; resolve the underlying invariant the \
                 rule enforces",
            );
        }
    }

    if rule.is_fixable {
        out.push_str(" — or run `alint fix --only ");
        out.push_str(&rule.rule_id);
        out.push_str("` to apply the auto-fix");
    }

    if let Some(url) = rule.policy_url.as_deref() {
        out.push_str(". See ");
        out.push_str(url);
        out.push_str(" for the policy this rule enforces");
    }

    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{Level, Report, RuleResult, Violation};
    use std::path::PathBuf;

    fn run(results: Vec<RuleResult>) -> String {
        let report = Report { results };
        let mut buf = Vec::new();
        write_agent(&report, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn empty_report_has_zero_violations_and_summary() {
        let out = run(Vec::new());
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["format"], "agent");
        assert_eq!(v["summary"]["total_violations"], 0);
        assert_eq!(v["summary"]["passing_rules"], 0);
        assert_eq!(v["summary"]["failing_rules"], 0);
        assert!(v["violations"].as_array().unwrap().is_empty());
    }

    #[test]
    fn path_bound_violation_renders_location_in_agent_instruction() {
        let result = RuleResult {
            rule_id: "agent-no-console-log".into(),
            level: Level::Warning,
            policy_url: None,
            violations: vec![
                Violation::new("console.log left in non-test source.")
                    .with_path(PathBuf::from("src/api.ts"))
                    .with_location(2, 1),
            ],
            is_fixable: false,
        };
        let out = run(vec![result]);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let viol = &v["violations"][0];
        assert_eq!(viol["rule_id"], "agent-no-console-log");
        assert_eq!(viol["severity"], "warning");
        assert_eq!(viol["file"], "src/api.ts");
        assert_eq!(viol["line"], 2);
        assert_eq!(viol["column"], 1);
        assert_eq!(viol["fix_available"], false);
        let inst = viol["agent_instruction"].as_str().unwrap();
        assert!(inst.starts_with("warning: console.log"), "got: {inst}");
        assert!(inst.contains("edit src/api.ts:2:1"), "got: {inst}");
        assert!(
            !inst.contains("alint fix"),
            "non-fixable shouldn't suggest fix: {inst}"
        );
    }

    #[test]
    fn fixable_violation_suggests_alint_fix_in_instruction() {
        let result = RuleResult {
            rule_id: "readme-exists".into(),
            level: Level::Error,
            policy_url: Some("https://example.com/policy".into()),
            violations: vec![Violation::new("A README is required at the root.")],
            is_fixable: true,
        };
        let out = run(vec![result]);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let inst = v["violations"][0]["agent_instruction"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(
            inst.contains("alint fix --only readme-exists"),
            "got: {inst}"
        );
        assert!(
            inst.contains("https://example.com/policy"),
            "policy URL should be cited: {inst}"
        );
        assert_eq!(v["violations"][0]["fix_available"], true);
        assert_eq!(v["summary"]["fixable_violations"], 1);
    }

    #[test]
    fn cross_file_violation_uses_repository_level_phrasing() {
        let result = RuleResult {
            rule_id: "lockfiles-only-one".into(),
            level: Level::Error,
            policy_url: None,
            violations: vec![Violation::new("Multiple lockfiles found.")],
            is_fixable: false,
        };
        let out = run(vec![result]);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let inst = v["violations"][0]["agent_instruction"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(
            inst.contains("repository-level"),
            "expected cross-file phrasing: {inst}"
        );
        assert!(v["violations"][0].get("file").is_none() || v["violations"][0]["file"].is_null());
    }

    #[test]
    fn severity_counts_aggregate_correctly() {
        let results = vec![
            RuleResult {
                rule_id: "rule-a".into(),
                level: Level::Error,
                policy_url: None,
                violations: vec![Violation::new("a")],
                is_fixable: false,
            },
            RuleResult {
                rule_id: "rule-b".into(),
                level: Level::Warning,
                policy_url: None,
                violations: vec![Violation::new("b1"), Violation::new("b2")],
                is_fixable: false,
            },
            RuleResult {
                rule_id: "rule-c".into(),
                level: Level::Info,
                policy_url: None,
                violations: vec![Violation::new("c")],
                is_fixable: false,
            },
            RuleResult {
                rule_id: "rule-d".into(),
                level: Level::Warning,
                policy_url: None,
                violations: vec![],
                is_fixable: false,
            },
        ];
        let out = run(results);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let sev = &v["summary"]["by_severity"];
        assert_eq!(sev["error"], 1);
        assert_eq!(sev["warning"], 2);
        assert_eq!(sev["info"], 1);
        assert_eq!(v["summary"]["total_violations"], 4);
        assert_eq!(v["summary"]["passing_rules"], 1);
        assert_eq!(v["summary"]["failing_rules"], 3);
    }
}
