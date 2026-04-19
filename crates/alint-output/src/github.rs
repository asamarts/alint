//! GitHub Actions workflow-command annotations.
//!
//! Each violation emits one line of the form:
//!
//! ```text
//! ::<level> title=<rule-id>,file=<path>,line=<L>,col=<C>::<message>
//! ```
//!
//! `<level>` maps `Error → error`, `Warning → warning`, `Info → notice`.
//! GitHub renders these inline on the PR files-changed view and in the
//! workflow log.
//!
//! Escaping follows the toolkit rules: property values (after the space,
//! before the final `::`) additionally escape `,` and `:`; message bodies
//! only escape `%`, `\r`, `\n`.

use std::io::Write;

use alint_core::{Level, Report};

pub fn write_github(report: &Report, w: &mut dyn Write) -> std::io::Result<()> {
    for rr in &report.results {
        let keyword = match rr.level {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Info => "notice",
            Level::Off => continue,
        };
        for v in &rr.violations {
            let mut props: Vec<String> = vec![format!("title={}", escape_prop(&rr.rule_id))];
            if let Some(path) = &v.path {
                props.push(format!("file={}", escape_prop(&path.display().to_string())));
            }
            if let Some(line) = v.line {
                props.push(format!("line={line}"));
            }
            if let Some(col) = v.column {
                props.push(format!("col={col}"));
            }
            let body = escape_body(&v.message);
            writeln!(w, "::{keyword} {}::{body}", props.join(","))?;
        }
    }
    Ok(())
}

fn escape_prop(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
        .replace(':', "%3A")
        .replace(',', "%2C")
}

fn escape_body(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{Report, RuleResult, Violation};
    use std::path::PathBuf;

    fn render(report: &Report) -> String {
        let mut buf = Vec::new();
        write_github(report, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn error_warning_info_map_to_distinct_keywords() {
        let report = Report {
            results: vec![
                RuleResult {
                    rule_id: "rule-err".into(),
                    level: Level::Error,
                    policy_url: None,
                    violations: vec![Violation::new("boom")],
                },
                RuleResult {
                    rule_id: "rule-warn".into(),
                    level: Level::Warning,
                    policy_url: None,
                    violations: vec![Violation::new("careful")],
                },
                RuleResult {
                    rule_id: "rule-info".into(),
                    level: Level::Info,
                    policy_url: None,
                    violations: vec![Violation::new("fyi")],
                },
            ],
        };
        let out = render(&report);
        assert!(out.contains("::error title=rule-err::boom"));
        assert!(out.contains("::warning title=rule-warn::careful"));
        assert!(out.contains("::notice title=rule-info::fyi"));
    }

    #[test]
    fn level_off_is_skipped() {
        let report = Report {
            results: vec![RuleResult {
                rule_id: "silenced".into(),
                level: Level::Off,
                policy_url: None,
                violations: vec![Violation::new("should not appear")],
            }],
        };
        assert_eq!(render(&report), "");
    }

    #[test]
    fn path_line_column_are_emitted_as_properties() {
        let report = Report {
            results: vec![RuleResult {
                rule_id: "at-loc".into(),
                level: Level::Error,
                policy_url: None,
                violations: vec![Violation {
                    path: Some(PathBuf::from("src/lib.rs")),
                    message: "bad".into(),
                    line: Some(12),
                    column: Some(4),
                }],
            }],
        };
        let out = render(&report);
        assert_eq!(
            out.trim_end(),
            "::error title=at-loc,file=src/lib.rs,line=12,col=4::bad"
        );
    }

    #[test]
    fn property_commas_and_colons_are_escaped() {
        let report = Report {
            results: vec![RuleResult {
                rule_id: "r,1:x".into(),
                level: Level::Error,
                policy_url: None,
                violations: vec![Violation::new("m")],
            }],
        };
        let out = render(&report);
        assert!(out.contains("title=r%2C1%3Ax"));
    }

    #[test]
    fn message_body_escapes_newlines_but_keeps_colons() {
        let report = Report {
            results: vec![RuleResult {
                rule_id: "r".into(),
                level: Level::Error,
                policy_url: None,
                violations: vec![Violation::new("a: b\nc")],
            }],
        };
        let out = render(&report);
        assert!(out.contains("::a: b%0Ac"));
    }
}
