//! Markdown formatter — meant for PR comments and other rendered
//! markdown surfaces (Slack via webhook bridges, mkdocs report
//! pages, etc.). Output is GitHub-Flavored Markdown.
//!
//! Layout: an H1 banner, a one-line summary, then one H2 section
//! per file (alphabetically sorted), each containing a bulleted
//! list of violations. Path-less / cross-file violations get
//! their own "Cross-file" section after the per-file sections,
//! matching the human formatter's "Repository-level" lead bucket
//! but reordered so a reviewer sees their changed-file findings
//! first.
//!
//! Determinism: same `Report` produces byte-identical output —
//! buckets are a `BTreeMap`, violations within a bucket are
//! sorted by `(rule_id, line, column)`. Important for PR-comment
//! workflows that diff alint output across runs to detect
//! regressions.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use alint_core::{FixReport, FixStatus, Level, Report, RuleResult, Violation};

pub fn write_markdown(report: &Report, w: &mut dyn Write) -> std::io::Result<()> {
    writeln!(w, "# alint check")?;
    writeln!(w)?;

    let total = report.total_violations();
    if total == 0 {
        writeln!(w, "No violations found.")?;
        return Ok(());
    }

    write_summary_line(w, report)?;
    writeln!(w)?;

    let (by_file, cross_file) = bucket_violations(report);

    for (path, items) in &by_file {
        writeln!(w, "## `{}` ({})", md_inline_code(&path.display().to_string()), items.len())?;
        writeln!(w)?;
        for (result, violation) in items {
            write_violation_bullet(w, result, violation)?;
        }
        writeln!(w)?;
    }

    if !cross_file.is_empty() {
        writeln!(w, "## Cross-file ({})", cross_file.len())?;
        writeln!(w)?;
        for (result, violation) in &cross_file {
            write_violation_bullet(w, result, violation)?;
        }
        writeln!(w)?;
    }

    Ok(())
}

pub fn write_fix_markdown(report: &FixReport, w: &mut dyn Write) -> std::io::Result<()> {
    writeln!(w, "# alint fix")?;
    writeln!(w)?;

    let applied = report.applied();
    let skipped = report.skipped();
    let unfixable = report.unfixable();

    if applied + skipped + unfixable == 0 {
        writeln!(w, "No violations found.")?;
        return Ok(());
    }

    writeln!(
        w,
        "**{applied} applied**, **{skipped} skipped**, **{unfixable} unfixable**.",
    )?;
    writeln!(w)?;

    for r in &report.results {
        if r.items.is_empty() {
            continue;
        }
        writeln!(w, "## `{}` ({})", md_inline_code(&r.rule_id), r.items.len())?;
        writeln!(w)?;
        for item in &r.items {
            let status_label = match &item.status {
                FixStatus::Applied(msg) => format!("**applied** — {}", md_escape(msg)),
                FixStatus::Skipped(msg) => format!("**skipped** — {}", md_escape(msg)),
                FixStatus::Unfixable => "**unfixable**".to_string(),
            };
            let path_part = item
                .violation
                .path
                .as_ref()
                .map(|p| format!("`{}` — ", md_inline_code(&p.display().to_string())))
                .unwrap_or_default();
            writeln!(
                w,
                "- {path_part}{status_label}: {}",
                md_escape(&item.violation.message)
            )?;
        }
        writeln!(w)?;
    }

    Ok(())
}

// ─── Helpers ───────────────────────────────────────────────────────

fn write_summary_line(w: &mut dyn Write, report: &Report) -> std::io::Result<()> {
    let total = report.total_violations();
    let (errors, warnings, infos) = level_counts(report);
    let bucket_count = file_bucket_count(report);

    let file_phrase = if bucket_count == 1 {
        "1 file".to_string()
    } else {
        format!("{bucket_count} files")
    };

    let mut breakdown: Vec<String> = Vec::new();
    if errors > 0 {
        breakdown.push(format!("{errors} error{}", plural_s(errors)));
    }
    if warnings > 0 {
        breakdown.push(format!("{warnings} warning{}", plural_s(warnings)));
    }
    if infos > 0 {
        breakdown.push(format!("{infos} info"));
    }

    if breakdown.is_empty() {
        writeln!(w, "**{total} violation{} across {file_phrase}.**", plural_s(total))?;
    } else {
        writeln!(
            w,
            "**{total} violation{} across {file_phrase}** ({}).",
            plural_s(total),
            breakdown.join(", "),
        )?;
    }
    Ok(())
}

fn write_violation_bullet(
    w: &mut dyn Write,
    result: &RuleResult,
    violation: &Violation,
) -> std::io::Result<()> {
    let level = level_word(result.level);
    let mut loc = String::new();
    if let (Some(line), Some(col)) = (violation.line, violation.column) {
        loc = format!(" (line {line}, col {col})");
    } else if let Some(line) = violation.line {
        loc = format!(" (line {line})");
    }

    let rule_part = match &result.policy_url {
        Some(url) if !url.is_empty() => {
            format!("[`{}`]({})", md_inline_code(&result.rule_id), md_url(url))
        }
        _ => format!("`{}`", md_inline_code(&result.rule_id)),
    };

    writeln!(
        w,
        "- **{level}** {rule_part}{loc} — {}",
        md_escape(&violation.message)
    )?;
    Ok(())
}

fn level_word(level: Level) -> &'static str {
    match level {
        Level::Error => "error",
        Level::Warning => "warning",
        Level::Info => "info",
        // Off rules are filtered upstream (passed() == true)
        Level::Off => "off",
    }
}

fn level_counts(report: &Report) -> (usize, usize, usize) {
    let mut e = 0;
    let mut w = 0;
    let mut i = 0;
    for r in &report.results {
        let n = r.violations.len();
        match r.level {
            Level::Error => e += n,
            Level::Warning => w += n,
            Level::Info => i += n,
            Level::Off => {}
        }
    }
    (e, w, i)
}

fn file_bucket_count(report: &Report) -> usize {
    let mut paths: std::collections::BTreeSet<Option<PathBuf>> = std::collections::BTreeSet::new();
    for r in &report.results {
        if r.passed() {
            continue;
        }
        for v in &r.violations {
            paths.insert(v.path.clone());
        }
    }
    paths.len()
}

type BucketedViolations<'a> = (
    BTreeMap<PathBuf, Vec<(&'a RuleResult, &'a Violation)>>,
    Vec<(&'a RuleResult, &'a Violation)>,
);

fn bucket_violations(report: &Report) -> BucketedViolations<'_> {
    let mut by_file: BTreeMap<PathBuf, Vec<(&RuleResult, &Violation)>> = BTreeMap::new();
    let mut cross_file: Vec<(&RuleResult, &Violation)> = Vec::new();
    for result in &report.results {
        if result.passed() {
            continue;
        }
        for violation in &result.violations {
            match &violation.path {
                Some(p) => by_file.entry(p.clone()).or_default().push((result, violation)),
                None => cross_file.push((result, violation)),
            }
        }
    }
    // Sort within each bucket by (rule_id, line, column) for
    // deterministic output.
    for items in by_file.values_mut() {
        items.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
    }
    cross_file.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
    (by_file, cross_file)
}

fn sort_key<'a>(p: &'a (&'a RuleResult, &'a Violation)) -> (&'a str, usize, usize) {
    (p.0.rule_id.as_str(), p.1.line.unwrap_or(0), p.1.column.unwrap_or(0))
}

fn plural_s(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Escape a string for safe inclusion in markdown body text.
/// Escapes the standard GFM punctuation set so a violation
/// message containing `*` / `_` / `` ` `` doesn't accidentally
/// turn the rest of the comment into bold/italic/code.
fn md_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '<' | '>' | '(' | ')'
            | '#' | '+' | '-' | '.' | '!' | '|' | '~' => {
                out.push('\\');
                out.push(ch);
            }
            // Newlines inside bullet text break the list — collapse to a space.
            '\n' | '\r' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

/// Escape a string for inclusion inside a backtick code span.
/// Backticks inside code spans are tricky in `CommonMark` — the
/// canonical workaround is to switch the delimiter, but for
/// rule ids and paths the simpler choice is to substitute a
/// visually-similar character. Embedded backticks in a path are
/// vanishingly rare; embedded backticks in a rule id are
/// disallowed by the schema. Net: this only matters for
/// truly adversarial inputs.
fn md_inline_code(s: &str) -> String {
    s.replace('`', "ʼ")
}

/// Escape a URL for use in a markdown link target.
/// `CommonMark` allows most characters but parens need balancing
/// inside `(...)`. We percent-encode parens conservatively;
/// browsers handle the rest.
fn md_url(s: &str) -> String {
    s.replace('(', "%28").replace(')', "%29")
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{FixItem, FixReport, FixRuleResult, FixStatus, Report, RuleResult, Violation};
    use std::path::PathBuf;

    fn render(report: &Report) -> String {
        let mut buf = Vec::new();
        write_markdown(report, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn render_fix(report: &FixReport) -> String {
        let mut buf = Vec::new();
        write_fix_markdown(report, &mut buf).unwrap();
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
    fn empty_report_renders_clean_banner() {
        let out = render(&Report { results: Vec::new() });
        assert_eq!(out, "# alint check\n\nNo violations found.\n");
    }

    #[test]
    fn passing_rules_render_clean_banner() {
        let report = Report {
            results: vec![rule("ok", Level::Error, vec![])],
        };
        let out = render(&report);
        assert!(out.contains("No violations found."));
        assert!(!out.contains("##"));
    }

    #[test]
    fn single_violation_groups_under_file_heading() {
        let report = Report {
            results: vec![rule(
                "no-todo",
                Level::Error,
                vec![Violation {
                    path: Some(PathBuf::from("src/lib.rs")),
                    message: "TODO marker found".into(),
                    line: Some(12),
                    column: Some(4),
                }],
            )],
        };
        let out = render(&report);
        assert!(out.contains("**1 violation across 1 file** (1 error)."));
        assert!(out.contains("## `src/lib.rs` (1)"));
        assert!(out.contains("- **error** `no-todo` (line 12, col 4) — TODO marker found"));
    }

    #[test]
    fn multiple_files_are_alphabetically_sorted() {
        let report = Report {
            results: vec![
                rule(
                    "r-z",
                    Level::Warning,
                    vec![Violation {
                        path: Some(PathBuf::from("zeta.rs")),
                        message: "z".into(),
                        line: None,
                        column: None,
                    }],
                ),
                rule(
                    "r-a",
                    Level::Error,
                    vec![Violation {
                        path: Some(PathBuf::from("alpha.rs")),
                        message: "a".into(),
                        line: None,
                        column: None,
                    }],
                ),
            ],
        };
        let out = render(&report);
        let alpha = out.find("## `alpha.rs`").unwrap();
        let zeta = out.find("## `zeta.rs`").unwrap();
        assert!(alpha < zeta, "alpha must precede zeta");
    }

    #[test]
    fn level_counts_breakdown_in_summary() {
        let report = Report {
            results: vec![
                rule(
                    "e1",
                    Level::Error,
                    vec![Violation::new("x").with_path(PathBuf::from("a"))],
                ),
                rule(
                    "w1",
                    Level::Warning,
                    vec![
                        Violation::new("x").with_path(PathBuf::from("b")),
                        Violation::new("x").with_path(PathBuf::from("c")),
                    ],
                ),
                rule(
                    "i1",
                    Level::Info,
                    vec![Violation::new("x").with_path(PathBuf::from("d"))],
                ),
            ],
        };
        let out = render(&report);
        assert!(out.contains("**4 violations across 4 files** (1 error, 2 warnings, 1 info)."));
    }

    #[test]
    fn cross_file_violations_get_dedicated_section() {
        let report = Report {
            results: vec![rule(
                "unique-pkg",
                Level::Error,
                vec![Violation::new("duplicate package name 'pkg-001'")],
            )],
        };
        let out = render(&report);
        assert!(out.contains("## Cross-file (1)"));
        assert!(out.contains("- **error** `unique-pkg` — duplicate package name"));
    }

    #[test]
    fn cross_file_section_appears_after_per_file_sections() {
        let report = Report {
            results: vec![
                rule(
                    "no-todo",
                    Level::Error,
                    vec![Violation::new("x").with_path(PathBuf::from("a.rs"))],
                ),
                rule("unique-pkg", Level::Error, vec![Violation::new("dup")]),
            ],
        };
        let out = render(&report);
        let file_idx = out.find("## `a.rs`").unwrap();
        let cross_idx = out.find("## Cross-file").unwrap();
        assert!(file_idx < cross_idx);
    }

    #[test]
    fn policy_url_renders_as_link() {
        let report = Report {
            results: vec![RuleResult {
                rule_id: "r1".into(),
                level: Level::Error,
                policy_url: Some("https://example.com/policy".into()),
                violations: vec![Violation::new("x").with_path(PathBuf::from("a.rs"))],
                is_fixable: false,
            }],
        };
        let out = render(&report);
        assert!(out.contains("[`r1`](https://example.com/policy)"));
    }

    #[test]
    fn message_special_chars_are_escaped() {
        let report = Report {
            results: vec![rule(
                "r1",
                Level::Error,
                vec![Violation::new("use **emphasis** [carefully]")
                    .with_path(PathBuf::from("a.rs"))],
            )],
        };
        let out = render(&report);
        assert!(out.contains(r"use \*\*emphasis\*\* \[carefully\]"));
    }

    #[test]
    fn newline_in_message_collapses_to_space() {
        let report = Report {
            results: vec![rule(
                "r1",
                Level::Error,
                vec![Violation::new("line1\nline2").with_path(PathBuf::from("a.rs"))],
            )],
        };
        let out = render(&report);
        assert!(out.contains("line1 line2"));
        assert!(!out.contains("line1\nline2"));
    }

    #[test]
    fn line_only_no_column() {
        let report = Report {
            results: vec![rule(
                "r1",
                Level::Warning,
                vec![Violation {
                    path: Some(PathBuf::from("a.rs")),
                    message: "x".into(),
                    line: Some(7),
                    column: None,
                }],
            )],
        };
        let out = render(&report);
        assert!(out.contains("(line 7) — x"));
        assert!(!out.contains("col"));
    }

    #[test]
    fn output_is_deterministic_across_input_order() {
        let v1 = Violation {
            path: Some(PathBuf::from("a.rs")),
            message: "a".into(),
            line: Some(1),
            column: Some(1),
        };
        let v2 = Violation {
            path: Some(PathBuf::from("a.rs")),
            message: "b".into(),
            line: Some(2),
            column: Some(1),
        };
        let r1 = Report {
            results: vec![rule("r1", Level::Error, vec![v1.clone(), v2.clone()])],
        };
        let r2 = Report {
            results: vec![rule("r1", Level::Error, vec![v2, v1])],
        };
        assert_eq!(render(&r1), render(&r2));
    }

    #[test]
    fn fix_report_empty_renders_clean() {
        let out = render_fix(&FixReport { results: Vec::new() });
        assert!(out.contains("No violations found."));
    }

    #[test]
    fn fix_report_groups_by_rule_with_status() {
        let report = FixReport {
            results: vec![FixRuleResult {
                rule_id: "trim".into(),
                level: Level::Warning,
                items: vec![
                    FixItem {
                        violation: Violation {
                            path: Some(PathBuf::from("a.rs")),
                            message: "trailing whitespace".into(),
                            line: Some(1),
                            column: None,
                        },
                        status: FixStatus::Applied("removed 3 trailing spaces".into()),
                    },
                    FixItem {
                        violation: Violation {
                            path: Some(PathBuf::from("b.rs")),
                            message: "trailing whitespace".into(),
                            line: None,
                            column: None,
                        },
                        status: FixStatus::Unfixable,
                    },
                ],
            }],
        };
        let out = render_fix(&report);
        assert!(out.contains("## `trim` (2)"));
        assert!(out.contains("**applied**"));
        assert!(out.contains("**unfixable**"));
        assert!(out.contains("**1 applied**, **0 skipped**, **1 unfixable**."));
    }
}
