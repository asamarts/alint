//! `JUnit` XML output — the de-facto-standard CI test-report
//! format consumed by Jenkins, Azure DevOps, GitLab CI's `JUnit`
//! integration, GitHub's `dorny/test-reporter`, and similar
//! tooling.
//!
//! The shipped shape is the common-denominator schema (no
//! Surefire / Ant-Junit extensions): a single `<testsuites>`
//! wrapping a single `<testsuite name="alint">`, with one
//! `<testcase>` per (rule, file/path-less-bucket). A passing
//! rule contributes one passing testcase; each violation
//! contributes one testcase with a `<failure>` child. The
//! `failures` count on `<testsuite>` equals the total violation
//! count regardless of level — consumers filter by the failure
//! `type` attribute (`error` / `warning` / `info`) when they
//! want level-specific behaviour.
//!
//! XML 1.0 disallows most C0 control characters in element /
//! attribute content; we strip them on the way out so a stray
//! NUL or `\x01` in a violation message doesn't produce
//! consumer-rejected XML.

use std::io::Write;

use alint_core::{Level, Report, RuleResult, Violation};

pub fn write_junit(report: &Report, w: &mut dyn Write) -> std::io::Result<()> {
    // `Level::Off` rules are silenced at format time — they
    // contribute neither testcases nor failures, matching the
    // human / github / sarif formatters.
    let active: Vec<&RuleResult> = report
        .results
        .iter()
        .filter(|r| r.level != Level::Off)
        .collect();

    let total_violations: usize = active.iter().map(|r| r.violations.len()).sum();
    let passing_rules = active.iter().filter(|r| r.passed()).count();
    let total_cases = passing_rules + total_violations;

    writeln!(w, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
    writeln!(
        w,
        r#"<testsuites name="alint" tests="{total_cases}" failures="{total_violations}" errors="0" time="0">"#
    )?;
    writeln!(
        w,
        r#"  <testsuite name="alint" tests="{total_cases}" failures="{total_violations}" errors="0" time="0">"#
    )?;

    // Stable iteration order: results in their original order
    // (which the engine already sorts by rule), violations in
    // the order the rule produced them. Consumers don't sort
    // testcases themselves so determinism here matters for
    // report-diff workflows.
    for result in active {
        if result.passed() {
            // Passing rule → one self-closed testcase. Provides
            // a "tests run" denominator for consumers that show
            // pass-rate metrics.
            writeln!(
                w,
                r#"    <testcase classname="alint.{}" name="{}" time="0"/>"#,
                xml_attr(&result.rule_id),
                xml_attr(&result.rule_id),
            )?;
            continue;
        }
        for violation in &result.violations {
            write_failure_case(w, result, violation)?;
        }
    }

    writeln!(w, "  </testsuite>")?;
    writeln!(w, "</testsuites>")?;
    Ok(())
}

fn write_failure_case(
    w: &mut dyn Write,
    result: &RuleResult,
    violation: &Violation,
) -> std::io::Result<()> {
    let case_name = match &violation.path {
        Some(p) => p.display().to_string(),
        None => "(repository)".to_string(),
    };
    let level_attr = match result.level {
        Level::Error => "error",
        Level::Warning => "warning",
        Level::Info => "info",
        // `Off` rules never reach this path — they're filtered
        // out at the top of `write_junit`. Match arm exists for
        // exhaustiveness only.
        Level::Off => "off",
    };

    writeln!(
        w,
        r#"    <testcase classname="alint.{rule}" name="{name}" time="0">"#,
        rule = xml_attr(&result.rule_id),
        name = xml_attr(&case_name),
    )?;
    writeln!(
        w,
        r#"      <failure message="{msg}" type="{level_attr}">{body}</failure>"#,
        msg = xml_attr(&violation.message),
        body = xml_text(&format_failure_body(result, violation)),
    )?;
    writeln!(w, "    </testcase>")?;
    Ok(())
}

/// Body text that goes inside the `<failure>` element. Includes
/// path:line:col and the policy URL when present, so consumers
/// that show only the failure body (and not the message attr)
/// still get the full picture.
fn format_failure_body(result: &RuleResult, violation: &Violation) -> String {
    let mut s = String::new();
    if let Some(p) = &violation.path {
        s.push_str(&p.display().to_string());
        if let Some(line) = violation.line {
            s.push(':');
            s.push_str(&line.to_string());
            if let Some(col) = violation.column {
                s.push(':');
                s.push_str(&col.to_string());
            }
        }
        s.push_str(": ");
    }
    s.push_str(&violation.message);
    if let Some(url) = &result.policy_url
        && !url.is_empty()
    {
        s.push_str("\nPolicy: ");
        s.push_str(url);
    }
    s
}

/// Escape a string for XML element content (`<elem>HERE</elem>`).
/// Strips XML 1.0-illegal control characters; replaces `& < >`
/// with their entity references. Quotes don't need escaping in
/// element content but it's harmless to do.
fn xml_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if is_xml_illegal_control(ch) {
            continue;
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Escape a string for an XML attribute value (`name="HERE"`).
/// Same as element content, plus quotes and apostrophes —
/// every XML attribute parser respects either, so we escape
/// both rather than picking the right pair.
fn xml_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if is_xml_illegal_control(ch) {
            continue;
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Tabs / newlines / CR survive in attribute values
            // but normalize to space per XML attr-value rules.
            // We pre-replace so the consumer sees a single line.
            '\n' | '\r' | '\t' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

/// XML 1.0 disallows most of the C0 control range. The only
/// permitted ones are TAB (0x09), LF (0x0A), CR (0x0D); the
/// rest must be stripped or the whole document is invalid.
fn is_xml_illegal_control(ch: char) -> bool {
    let cp = ch as u32;
    (cp < 0x20 && cp != 0x09 && cp != 0x0A && cp != 0x0D) || cp == 0xFFFE || cp == 0xFFFF
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{Report, RuleResult, Violation};
    use std::path::{Path, PathBuf};

    fn render(report: &Report) -> String {
        let mut buf = Vec::new();
        write_junit(report, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn rule(id: &str, level: Level, violations: Vec<Violation>) -> RuleResult {
        RuleResult {
            rule_id: id.into(),
            level,
            policy_url: None,
            violations,
            is_fixable: false,
        }
    }

    #[test]
    fn empty_report_has_zero_tests_zero_failures() {
        let out = render(&Report {
            results: Vec::new(),
        });
        assert!(out.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"));
        assert!(out.contains(r#"<testsuites name="alint" tests="0" failures="0" errors="0""#));
        assert!(out.contains("</testsuite>\n</testsuites>\n"));
    }

    #[test]
    fn passing_rule_emits_self_closed_testcase() {
        let report = Report {
            results: vec![rule("ok", Level::Error, vec![])],
        };
        let out = render(&report);
        assert!(out.contains(r#"<testcase classname="alint.ok" name="ok" time="0"/>"#));
        assert!(out.contains(r#"tests="1" failures="0""#));
    }

    #[test]
    fn single_violation_renders_failure_with_path_line_col() {
        let report = Report {
            results: vec![rule(
                "no-todo",
                Level::Error,
                vec![Violation {
                    path: Some(Path::new("src/lib.rs").into()),
                    message: "TODO marker found".into(),
                    line: Some(12),
                    column: Some(4),
                }],
            )],
        };
        let out = render(&report);
        assert!(out.contains(r#"<testcase classname="alint.no-todo" name="src/lib.rs" time="0">"#));
        assert!(out.contains(r#"<failure message="TODO marker found" type="error">"#));
        assert!(out.contains("src/lib.rs:12:4: TODO marker found"));
        assert!(out.contains(r#"tests="1" failures="1""#));
    }

    #[test]
    fn level_warning_and_info_use_distinct_failure_types() {
        let report = Report {
            results: vec![
                rule(
                    "w",
                    Level::Warning,
                    vec![Violation::new("warn-msg").with_path(PathBuf::from("a"))],
                ),
                rule(
                    "i",
                    Level::Info,
                    vec![Violation::new("info-msg").with_path(PathBuf::from("b"))],
                ),
            ],
        };
        let out = render(&report);
        assert!(out.contains(r#"type="warning""#));
        assert!(out.contains(r#"type="info""#));
        assert!(out.contains(r#"failures="2""#));
    }

    #[test]
    fn cross_file_violation_uses_repository_marker_for_name() {
        let report = Report {
            results: vec![rule(
                "unique-pkg",
                Level::Error,
                vec![Violation::new("dup")],
            )],
        };
        let out = render(&report);
        assert!(out.contains(r#"name="(repository)""#));
    }

    #[test]
    fn xml_special_chars_are_escaped() {
        let report = Report {
            results: vec![rule(
                "r&<>\"'",
                Level::Error,
                vec![Violation {
                    path: Some(Path::new("a&b.rs").into()),
                    message: "<bad> & \"quoted\"".into(),
                    line: None,
                    column: None,
                }],
            )],
        };
        let out = render(&report);
        assert!(out.contains(r#"classname="alint.r&amp;&lt;&gt;&quot;&apos;""#));
        assert!(out.contains(r#"name="a&amp;b.rs""#));
        assert!(out.contains(r#"message="&lt;bad&gt; &amp; &quot;quoted&quot;""#));
        assert!(out.contains("&lt;bad&gt; &amp; \"quoted\""));
    }

    #[test]
    fn control_characters_are_stripped() {
        let report = Report {
            results: vec![rule(
                "ctrl",
                Level::Error,
                vec![Violation {
                    path: Some(Path::new("a.rs").into()),
                    message: "before\u{0001}\u{0008}after".into(),
                    line: None,
                    column: None,
                }],
            )],
        };
        let out = render(&report);
        assert!(out.contains("beforeafter"));
        assert!(!out.contains('\u{0001}'));
    }

    #[test]
    fn newline_in_attribute_normalizes_to_space() {
        let report = Report {
            results: vec![rule(
                "r",
                Level::Error,
                vec![Violation {
                    path: Some(Path::new("a").into()),
                    message: "line1\nline2".into(),
                    line: None,
                    column: None,
                }],
            )],
        };
        let out = render(&report);
        // The `message` attribute should have the newline replaced.
        assert!(out.contains(r#"message="line1 line2""#));
        // The body keeps the newline (legal in element content).
        assert!(out.contains("line1\nline2"));
    }

    #[test]
    fn policy_url_appended_to_failure_body() {
        let report = Report {
            results: vec![RuleResult {
                rule_id: "r".into(),
                level: Level::Error,
                policy_url: Some("https://example.com/p".into()),
                violations: vec![Violation::new("x").with_path(PathBuf::from("a"))],
                is_fixable: false,
            }],
        };
        let out = render(&report);
        assert!(out.contains("Policy: https://example.com/p"));
    }

    #[test]
    fn level_off_rule_is_silenced_entirely() {
        let report = Report {
            results: vec![RuleResult {
                rule_id: "off".into(),
                level: Level::Off,
                policy_url: None,
                violations: vec![Violation::new("ignored").with_path(PathBuf::from("a"))],
                is_fixable: false,
            }],
        };
        let out = render(&report);
        // `Level::Off` rules contribute zero testcases — neither
        // passing nor failing — to keep the test count meaningful
        // for consumers.
        assert!(out.contains(r#"tests="0" failures="0""#));
        assert!(!out.contains("<testcase"));
        assert!(!out.contains("<failure"));
    }

    #[test]
    fn multiple_violations_one_rule_emit_separate_testcases() {
        let report = Report {
            results: vec![rule(
                "r",
                Level::Error,
                vec![
                    Violation::new("v1").with_path(PathBuf::from("a")),
                    Violation::new("v2").with_path(PathBuf::from("b")),
                ],
            )],
        };
        let out = render(&report);
        assert_eq!(out.matches("<testcase").count(), 2);
        assert_eq!(out.matches("<failure").count(), 2);
        assert!(out.contains(r#"failures="2""#));
    }

    #[test]
    fn output_is_deterministic_for_identical_input() {
        let report = Report {
            results: vec![rule(
                "r",
                Level::Error,
                vec![
                    Violation::new("v1").with_path(PathBuf::from("a")),
                    Violation::new("v2").with_path(PathBuf::from("b")),
                ],
            )],
        };
        assert_eq!(render(&report), render(&report));
    }
}
