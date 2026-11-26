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

use std::borrow::Cow;
use std::fmt::Write as _;
use std::io::Write;

use alint_core::{AnalysisFailure, Level, Report, Violation};

pub fn write_junit(report: &Report, w: &mut dyn Write) -> std::io::Result<()> {
    // `Level::Off` suppresses policy violations and passing cases, but an
    // analysis failure is independent of policy severity and always becomes an
    // errored testcase.
    let total_violations: usize = report
        .results
        .iter()
        .filter(|result| result.level != Level::Off)
        .map(|result| result.violations.len())
        .sum();
    let total_analysis_failures = report.total_analysis_failures();
    let passing_rules = report
        .results
        .iter()
        .filter(|result| result.level != Level::Off && result.passed())
        .count();
    let total_cases = passing_rules + total_violations + total_analysis_failures;

    writeln!(w, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
    writeln!(
        w,
        r#"<testsuites name="alint" tests="{total_cases}" failures="{total_violations}" errors="{total_analysis_failures}" time="0">"#
    )?;
    writeln!(
        w,
        r#"  <testsuite name="alint" tests="{total_cases}" failures="{total_violations}" errors="{total_analysis_failures}" time="0">"#
    )?;

    // Stable iteration order: results in their original order
    // (which the engine already sorts by rule), violations in
    // the order the rule produced them. Consumers don't sort
    // testcases themselves so determinism here matters for
    // report-diff workflows.
    for result in &report.results {
        let needs_rule_attr = result.level != Level::Off || !result.analysis_failures.is_empty();
        if !needs_rule_attr {
            continue;
        }
        let safe_rule = xml_attr_sanitized(&result.rule_id);
        if result.level != Level::Off {
            if result.passed() {
                // Passing rule → one self-closed testcase. Provides
                // a "tests run" denominator for consumers that show
                // pass-rate metrics.
                writeln!(
                    w,
                    r#"    <testcase classname="alint.{safe_rule}" name="{safe_rule}" time="0"/>"#,
                )?;
            } else {
                let level_attr = match result.level {
                    Level::Error => "error",
                    Level::Warning => "warning",
                    Level::Info => "info",
                    // `Off` rules never reach this path — they're filtered
                    // out above. Match arm exists for exhaustiveness only.
                    Level::Off => "off",
                };
                let safe_policy_url = result
                    .policy_url
                    .as_deref()
                    .filter(|url| !url.is_empty())
                    .map(xml_text_sanitized);
                for violation in &result.violations {
                    write_failure_case(
                        w,
                        &safe_rule,
                        level_attr,
                        safe_policy_url.as_deref(),
                        violation,
                    )?;
                }
            }
        }
        for failure in &result.analysis_failures {
            write_error_case(w, &safe_rule, failure)?;
        }
    }

    let note_count = report.total_notes();
    if note_count > 0 {
        writeln!(
            w,
            "    <system-out>{}</system-out>",
            xml_text_sanitized(&crate::note_summary(note_count))
        )?;
    }

    writeln!(w, "  </testsuite>")?;
    writeln!(w, "</testsuites>")?;
    Ok(())
}

fn write_failure_case(
    w: &mut dyn Write,
    safe_rule: &str,
    level_attr: &str,
    safe_policy_url: Option<&str>,
    violation: &Violation,
) -> std::io::Result<()> {
    let report_path = violation.report_path().map(crate::slash_path_cow);
    let case_name = report_path.as_deref().unwrap_or("(repository)");

    writeln!(
        w,
        r#"    <testcase classname="alint.{safe_rule}" name="{name}" time="0">"#,
        name = xml_attr_sanitized(case_name),
    )?;
    write!(
        w,
        r#"      <failure message="{msg}" type="{level_attr}">"#,
        msg = xml_attr_sanitized(&violation.message),
    )?;
    if let Some(path_str) = report_path.as_deref() {
        w.write_all(xml_text_sanitized(path_str).as_bytes())?;
        if let Some(line) = violation.report_line() {
            write!(w, ":{line}")?;
            if let Some(col) = violation.report_column() {
                write!(w, ":{col}")?;
            }
        }
        w.write_all(b": ")?;
    }
    w.write_all(xml_text_sanitized(&violation.message).as_bytes())?;
    if let Some(url) = safe_policy_url {
        // Match single-buffer `\r\n` normalization when `violation.message`
        // ended with a bare `\r` (which already emitted `\n` above).
        if violation.message.ends_with('\r') {
            w.write_all(b"Policy: ")?;
        } else {
            w.write_all(b"\nPolicy: ")?;
        }
        w.write_all(url.as_bytes())?;
    }
    w.write_all(b"</failure>\n    </testcase>\n")?;
    Ok(())
}

fn write_error_case(
    w: &mut dyn Write,
    safe_rule: &str,
    failure: &AnalysisFailure,
) -> std::io::Result<()> {
    let report_path = failure.report_path().map(crate::slash_path_cow);
    let case_name = report_path.as_deref().unwrap_or("(repository)");

    writeln!(
        w,
        r#"    <testcase classname="alint.{safe_rule}" name="{name}" time="0">"#,
        name = xml_attr_sanitized(case_name),
    )?;
    write!(
        w,
        r#"      <error message="{message}" type="analysis_failure">"#,
        message = xml_attr_sanitized(&failure.message),
    )?;
    if let Some(path_str) = report_path.as_deref() {
        w.write_all(xml_text_sanitized(path_str).as_bytes())?;
        if let Some(line) = failure.report_line() {
            write!(w, ":{line}")?;
            if let Some(col) = failure.report_column() {
                write!(w, ":{col}")?;
            }
        }
        w.write_all(b": ")?;
    }
    write!(
        w,
        "{}: {}</error>\n    </testcase>\n",
        failure.kind.as_str(),
        xml_text_sanitized(&failure.message),
    )?;
    Ok(())
}

/// Single-pass display sanitizer + XML element content encoder (`<elem>HERE</elem>`).
/// Escapes DEL/C1 controls (`\xNN`) and display-deceptive Unicode characters
/// (`\u{xxxx}`), strips XML 1.0-illegal C0 controls, normalizes `\r\n` and bare
/// `\r` to `\n`, and escapes `& < >`. Returns borrowed input with zero
/// allocation on clean ASCII.
fn xml_text_sanitized(s: &str) -> Cow<'_, str> {
    if s.bytes().all(|b| {
        ((0x20..0x7f).contains(&b) || b == b'\n' || b == b'\t') && !matches!(b, b'&' | b'<' | b'>')
    }) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        let cp = ch as u32;
        if (0x7f..=0x9f).contains(&cp) {
            let _ = write!(out, "\\x{cp:02x}");
        } else if alint_core::invisible::is_deceptive_on_display(ch) {
            let _ = write!(out, "\\u{{{cp:04x}}}");
        } else if !is_xml_illegal_control(ch) {
            match ch {
                '\r' => {
                    let _ = chars.next_if_eq(&'\n');
                    out.push('\n');
                }
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                _ => out.push(ch),
            }
        }
    }
    Cow::Owned(out)
}

/// Single-pass display sanitizer + XML attribute encoder (`name="HERE"`).
/// Escapes DEL/C1 controls (`\xNN`) and display-deceptive Unicode characters
/// (`\u{xxxx}`), strips XML 1.0-illegal C0 controls, normalizes whitespace
/// controls (`\t`/`\n`/`\r`) to space, and escapes `& < > " '`. Returns
/// borrowed input with zero allocation on clean ASCII.
fn xml_attr_sanitized(s: &str) -> Cow<'_, str> {
    if s.bytes()
        .all(|b| (0x20..0x7f).contains(&b) && !matches!(b, b'&' | b'<' | b'>' | b'"' | b'\''))
    {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        let cp = ch as u32;
        if (0x7f..=0x9f).contains(&cp) {
            let _ = write!(out, "\\x{cp:02x}");
        } else if alint_core::invisible::is_deceptive_on_display(ch) {
            let _ = write!(out, "\\u{{{cp:04x}}}");
        } else if !is_xml_illegal_control(ch) {
            match ch {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&apos;"),
                '\n' | '\r' | '\t' => out.push(' '),
                _ => out.push(ch),
            }
        }
    }
    Cow::Owned(out)
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
            notes: Vec::new(),
            is_fixable: false,
            analysis_failures: Vec::new(),
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
                vec![
                    Violation::new("TODO marker found")
                        .with_path(Path::new("src/lib.rs"))
                        .with_location(12, 4),
                ],
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
                vec![Violation::new("<bad> & \"quoted\"").with_path(Path::new("a&b.rs"))],
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
                vec![Violation::new("before\u{0001}\u{0008}after").with_path(Path::new("a.rs"))],
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
                vec![Violation::new("line1\nline2").with_path(Path::new("a"))],
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
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
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
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
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

    #[test]
    fn bidi_and_zero_width_escaped_across_entire_junit_document() {
        use alint_core::AnalysisFailureKind;

        let bidi_path = PathBuf::from("a\x1b[31m\u{202E}\u{200B}\u{180E}Z.txt");
        let bidi_rule = "rule-\u{202E}\u{200B}\u{009B}id";
        let msg = "file is forbidden (matches [*.txt]): a\x1b[31m\u{202E}\u{200B}\u{180E}\u{0085}\rfake\r\nZ.txt";
        let report = Report {
            results: vec![
                RuleResult {
                    rule_id: bidi_rule.into(),
                    level: Level::Error,
                    policy_url: None,
                    violations: vec![Violation::new(msg).with_path(bidi_path.clone())],
                    notes: Vec::new(),
                    is_fixable: false,
                    analysis_failures: vec![
                        AnalysisFailure::new(AnalysisFailureKind::ResourceParse, msg)
                            .with_path(bidi_path),
                    ],
                },
                rule("pass-\u{202E}\u{200B}ok", Level::Error, vec![]),
            ],
        };
        let out = render(&report);
        // Assert across the ENTIRE JUnit XML document (including `<testcase>` attrs).
        for raw in [
            '\x1b', '\r', '\u{202E}', '\u{200B}', '\u{180E}', '\u{0085}', '\u{009B}',
        ] {
            assert!(
                !out.contains(raw),
                "raw U+{:04X} survived in JUnit output:\n{out}",
                raw as u32
            );
        }
        assert!(out.contains("\\u{202e}"), "missing bidi escape: {out}");
        assert!(out.contains("\\u{200b}"), "missing zwsp escape: {out}");
        assert!(out.contains("\\u{180e}"), "missing mvs escape: {out}");
        assert!(out.contains("\\x85"), "missing C1 NEL escape: {out}");
        assert!(out.contains("\\x9b"), "missing C1 CSI escape: {out}");
        assert!(out.contains("fake\nZ.txt"), "CR normalized to LF: {out}");
        assert!(
            out.contains(r#"classname="alint.rule-\u{202e}\u{200b}\x9bid""#),
            "testcase classname must be sanitized: {out}"
        );
        assert!(
            out.contains(r#"name="pass-\u{202e}\u{200b}ok""#),
            "passing testcase name must be sanitized: {out}"
        );
    }

    #[test]
    fn single_pass_xml_sanitizers_match_two_step_reference() {
        fn ref_attr_for(sample: &str) -> String {
            let ref_sanitized = crate::sanitize::sanitize_xml_display(sample);
            let mut ref_attr = String::new();
            for ch in ref_sanitized.chars() {
                if is_xml_illegal_control(ch) {
                    continue;
                }
                match ch {
                    '&' => ref_attr.push_str("&amp;"),
                    '<' => ref_attr.push_str("&lt;"),
                    '>' => ref_attr.push_str("&gt;"),
                    '"' => ref_attr.push_str("&quot;"),
                    '\'' => ref_attr.push_str("&apos;"),
                    '\n' | '\r' | '\t' => ref_attr.push(' '),
                    _ => ref_attr.push(ch),
                }
            }
            ref_attr
        }

        fn ref_text_for(sample: &str) -> String {
            let ref_sanitized = crate::sanitize::sanitize_xml_display(sample);
            let mut ref_text = String::new();
            let mut chars = ref_sanitized.chars().peekable();
            while let Some(ch) = chars.next() {
                if is_xml_illegal_control(ch) {
                    continue;
                }
                match ch {
                    '\r' => {
                        let _ = chars.next_if_eq(&'\n');
                        ref_text.push('\n');
                    }
                    '&' => ref_text.push_str("&amp;"),
                    '<' => ref_text.push_str("&lt;"),
                    '>' => ref_text.push_str("&gt;"),
                    _ => ref_text.push(ch),
                }
            }
            ref_text
        }

        // Exhaustive parity across every valid Unicode scalar value.
        let mut buf = [0u8; 4];
        for cp in 0u32..=0x0010_FFFF {
            let Some(ch) = char::from_u32(cp) else {
                continue;
            };
            let s = ch.encode_utf8(&mut buf);
            assert_eq!(
                xml_attr_sanitized(s).as_ref(),
                ref_attr_for(s),
                "attr mismatch at U+{cp:04X}"
            );
            assert_eq!(
                xml_text_sanitized(s).as_ref(),
                ref_text_for(s),
                "text mismatch at U+{cp:04X}"
            );
        }

        // Multi-character interaction sequences (CRLF, double CR, CR + illegal
        // C0 control, XML noncharacters U+FFFE/U+FFFF, display-deceptive chars).
        let samples = [
            "plain/ascii-123.rs",
            "a&b<c>d\"e'f",
            "ctrl\x00\x01\x07\x08\x09\x0a\x0b\x0c\x0d\x1b\x1f\x7fend",
            "crlf\r\nline\rnext\r\r\x01\n\r",
            "nonchar\u{fffe}\u{ffff}end",
            "unicode\u{0085}\u{009b}\u{202e}\u{200b}\u{180e}\u{feff}\u{e0069}caf\u{e9}",
        ];
        for sample in samples {
            assert_eq!(
                xml_attr_sanitized(sample).as_ref(),
                ref_attr_for(sample),
                "{sample:?}"
            );
            assert_eq!(
                xml_text_sanitized(sample).as_ref(),
                ref_text_for(sample),
                "{sample:?}"
            );
        }

        // Verify trailing `\r` + policy URL produces the exact single `\n` before `Policy:`.
        let report = Report {
            results: vec![RuleResult {
                rule_id: "r".into(),
                level: Level::Error,
                policy_url: Some("https://example.com/p".into()),
                violations: vec![Violation::new("msg\r").with_path(PathBuf::from("a.rs"))],
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
            }],
        };
        let out = render(&report);
        assert!(
            out.contains("a.rs: msg\nPolicy: https://example.com/p</failure>"),
            "{out}"
        );
    }

    #[test]
    fn level_off_with_analysis_failures_emits_error_cases() {
        // `Level::Off` with analysis failures still emits `<error>` cases
        // while keeping `failures="0"` and omitting suppressed policy findings.
        let off_with_err = Report {
            results: vec![RuleResult {
                rule_id: "off-rule".into(),
                level: Level::Off,
                policy_url: None,
                violations: vec![Violation::new("ignored").with_path(PathBuf::from("a.rs"))],
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: vec![
                    AnalysisFailure::new(
                        alint_core::AnalysisFailureKind::ResourceParse,
                        "bad\r\x01file",
                    )
                    .with_path(PathBuf::from("bad.yml")),
                ],
            }],
        };
        let off_out = render(&off_with_err);
        assert!(
            off_out.contains(r#"tests="1" failures="0" errors="1""#),
            "{off_out}"
        );
        assert!(
            off_out.contains(r#"classname="alint.off-rule" name="bad.yml""#),
            "{off_out}"
        );
        assert!(off_out.contains(r#"type="analysis_failure""#), "{off_out}");
        assert!(!off_out.contains("<failure"), "{off_out}");
    }
}
