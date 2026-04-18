use std::io::Write;
use std::path::PathBuf;

use alint_core::{Level, Report};
use serde::Serialize;

#[derive(Serialize)]
struct JsonReport<'a> {
    schema_version: u32,
    summary: Summary,
    results: Vec<JsonResult<'a>>,
}

#[derive(Serialize)]
struct Summary {
    failing_rules: usize,
    passing_rules: usize,
    total_violations: usize,
    has_errors: bool,
    has_warnings: bool,
}

#[derive(Serialize)]
struct JsonResult<'a> {
    id: &'a str,
    level: Level,
    passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_url: Option<&'a str>,
    violations: Vec<JsonViolation>,
}

#[derive(Serialize)]
struct JsonViolation {
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<PathBuf>,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    column: Option<usize>,
}

pub fn write_json(report: &Report, w: &mut dyn Write) -> std::io::Result<()> {
    let summary = Summary {
        failing_rules: report.failing_rules(),
        passing_rules: report.passing_rules(),
        total_violations: report.total_violations(),
        has_errors: report.has_errors(),
        has_warnings: report.has_warnings(),
    };
    let results: Vec<JsonResult<'_>> = report
        .results
        .iter()
        .map(|r| JsonResult {
            id: &r.rule_id,
            level: r.level,
            passed: r.passed(),
            policy_url: r.policy_url.as_deref(),
            violations: r
                .violations
                .iter()
                .map(|v| JsonViolation {
                    path: v.path.clone(),
                    message: v.message.clone(),
                    line: v.line,
                    column: v.column,
                })
                .collect(),
        })
        .collect();
    let out = JsonReport {
        schema_version: 1,
        summary,
        results,
    };
    serde_json::to_writer_pretty(&mut *w, &out)?;
    writeln!(w)?;
    Ok(())
}
