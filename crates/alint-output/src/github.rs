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
            Level::Error => Some("error"),
            Level::Warning => Some("warning"),
            Level::Info => Some("notice"),
            Level::Off => None,
        };
        if let Some(keyword) = keyword {
            for v in &rr.violations {
                let mut props: Vec<String> = vec![format!("title={}", escape_prop(&rr.rule_id))];
                push_location_props(
                    &mut props,
                    v.report_path(),
                    v.report_line(),
                    v.report_column(),
                );
                let body = escape_body(&v.message);
                writeln!(w, "::{keyword} {}::{body}", props.join(","))?;
            }
        }
        for failure in &rr.analysis_failures {
            let mut props = vec![format!(
                "title={}",
                escape_prop(&format!("{} analysis failure", rr.rule_id))
            )];
            push_location_props(
                &mut props,
                failure.report_path(),
                failure.report_line(),
                failure.report_column(),
            );
            let body = escape_body(&format!("{}: {}", failure.kind.as_str(), failure.message));
            writeln!(w, "::error {}::{body}", props.join(","))?;
        }
    }
    let note_count = report.total_notes();
    if note_count > 0 {
        writeln!(
            w,
            "::notice title=alint notes::{}",
            escape_body(&crate::note_summary(note_count))
        )?;
    }
    Ok(())
}

fn push_location_props(
    props: &mut Vec<String>,
    path: Option<&std::path::Path>,
    line: Option<usize>,
    col: Option<usize>,
) {
    let Some(path) = path else {
        // GitHub Actions workflow commands require `file=` whenever `line=` or
        // `col=` is specified.
        return;
    };
    props.push(format!("file={}", escape_prop(&crate::slash_path(path))));
    let line = line.filter(|&n| n > 0);
    let col = col.filter(|&n| n > 0);
    // GitHub Actions requires `line=` whenever `col=` is specified and both
    // coordinates must be 1-based positive integers.
    if let Some(line) = line.or(col.is_some().then_some(1)) {
        props.push(format!("line={line}"));
        if let Some(col) = col {
            props.push(format!("col={col}"));
        }
    }
}

fn escape_prop(s: &str) -> String {
    // GitHub's `%`/`\r`/`\n`/`:`/`,` escaping FIRST, then neutralize any remaining
    // terminal control bytes (ESC/BEL/...) from an attacker-named path or message so
    // they can't fire when a developer previews `--format github` in a terminal
    // (audit R2 MED-1). `\r`/`\n` are already `%`-escaped above, so `sanitize_terminal`
    // only touches the other control bytes.
    let escaped = s
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
        .replace(':', "%3A")
        .replace(',', "%2C");
    crate::sanitize::sanitize_terminal(&escaped).into_owned()
}

fn escape_body(s: &str) -> String {
    let escaped = s
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A");
    crate::sanitize::sanitize_terminal(&escaped).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{Report, RuleResult, Violation};
    use std::path::Path;

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
                    notes: Vec::new(),
                    is_fixable: false,
                    analysis_failures: Vec::new(),
                },
                RuleResult {
                    rule_id: "rule-warn".into(),
                    level: Level::Warning,
                    policy_url: None,
                    violations: vec![Violation::new("careful")],
                    notes: Vec::new(),
                    is_fixable: false,
                    analysis_failures: Vec::new(),
                },
                RuleResult {
                    rule_id: "rule-info".into(),
                    level: Level::Info,
                    policy_url: None,
                    violations: vec![Violation::new("fyi")],
                    notes: Vec::new(),
                    is_fixable: false,
                    analysis_failures: Vec::new(),
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
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
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
                violations: vec![
                    Violation::new("bad")
                        .with_path(Path::new("src/lib.rs"))
                        .with_location(12, 4),
                ],
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
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
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
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
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
            }],
        };
        let out = render(&report);
        assert!(out.contains("::a: b%0Ac"));
    }

    #[test]
    fn location_props_pair_line_with_col_drop_zero_and_require_file() {
        let mut col_only = Violation::new("c").with_path(Path::new("src/a.rs"));
        col_only.column = Some(5);
        let mut zero = Violation::new("z").with_path(Path::new("src/b.rs"));
        zero.line = Some(0);
        zero.column = Some(0);
        let mut no_file = Violation::new("nf");
        no_file.line = Some(2);
        no_file.column = Some(3);
        let report = Report {
            results: vec![RuleResult {
                rule_id: "r".into(),
                level: Level::Error,
                policy_url: None,
                violations: vec![col_only, zero, no_file],
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
            }],
        };
        let out = render(&report);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "::error title=r,file=src/a.rs,line=1,col=5::c");
        assert_eq!(lines[1], "::error title=r,file=src/b.rs::z");
        assert_eq!(lines[2], "::error title=r::nf");
    }
}
