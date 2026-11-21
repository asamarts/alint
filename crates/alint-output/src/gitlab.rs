//! `GitLab` Code Quality JSON output.
//!
//! GitLab CI consumes this format natively as a Code Quality
//! report artifact (see GitLab's "Code Quality" docs); also
//! used by the `codeclimate` engine ecosystem and read by
//! several MR-comment bots. The shipped shape is the
//! Code Climate "Issue" specification — the canonical source
//! GitLab references — encoded as a JSON array of issue
//! objects, one per violation.
//!
//! Each issue carries: `description`, `check_name`,
//! `fingerprint` (the canonical `baseline::violation_fingerprint` when `alint`
//! supplies it — so GitLab, SARIF, and baseline mode agree on identity — else
//! the self-contained fallback [`self_fingerprint`]), `severity` (one of
//! `info` / `minor` / `major` / `critical` / `blocker`), and
//! `location.path` + `location.lines.begin`.
//!
//! `Level` mapping:
//! - `Error`   → `major`
//! - `Warning` → `minor`
//! - `Info`    → `info`
//! - `Off`     → silenced (filtered upstream)
//!
//! Path-less / cross-file violations are emitted with
//! `location.path = "."` so the report still validates against
//! the GitLab schema (which requires `location.path`); the
//! repository root is the most defensible default. Fingerprint
//! stability still works because the rule id + message
//! disambiguate.

use std::io::Write;

use alint_core::{AnalysisFailure, Level, Report, RuleResult, Violation};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::occurrence::{OccurrenceKey, occurrence_fingerprint, occurrences};

/// `fingerprints`, when supplied, is the per-result / per-violation canonical
/// `baseline::violation_fingerprint` (indexed `[result][violation]`, aligned with
/// `report.results[i].violations[j]`), so an un-collided finding shares ONE
/// identity with baseline mode + SARIF (`partialFingerprints`). Passed by
/// `alint`'s GitLab path (which has file access to compute the content
/// discriminator). `None` falls back to the self-contained hash (generic
/// dispatch, benches, unit tests).
///
/// The canonical fingerprint is deliberately collision-prone — it collapses
/// identical-content findings so baseline mode can count them — but GitLab Code
/// Quality drops findings that share a fingerprint. So this runs a per-report
/// occurrence pass over BOTH fingerprint sources: the findings that share a
/// base identity are numbered by the path, line and column they show, then
/// rule and message, as SARIF numbers them; the first emits it raw
/// (cross-format identity preserved), and each other gets a deterministic
/// suffix, so every emitted issue is unique and none is silently dropped.
pub fn write_gitlab(
    report: &Report,
    fingerprints: Option<&[Vec<String>]>,
    w: &mut dyn Write,
) -> std::io::Result<()> {
    let mut issues: Vec<Issue> = Vec::new();
    // Each finding's issue, at its BASE fingerprint (canonical when supplied,
    // else the self-contained hash), with where it stands among those that
    // share it, and each analysis failure's.
    let mut findings: Vec<(usize, OccurrenceKey<'_>)> = Vec::new();
    let mut failures: Vec<usize> = Vec::new();
    for (ri, result) in report.results.iter().enumerate() {
        if result.level != Level::Off {
            for (vi, violation) in result.violations.iter().enumerate() {
                let precomputed = fingerprints
                    .and_then(|f| f.get(ri))
                    .and_then(|r| r.get(vi))
                    .cloned();
                findings.push((issues.len(), OccurrenceKey::of(&result.rule_id, violation)));
                issues.push(build_issue(result, violation, precomputed));
            }
        }
        for failure in &result.analysis_failures {
            failures.push(issues.len());
            issues.push(build_analysis_failure_issue(result, failure));
        }
    }
    // An in-report collision, which the canonical fingerprint deliberately
    // produces on identical content and leaves free of a hidden path or a
    // `constraint` finding's path, is disambiguated so GitLab keeps every
    // finding (Code Climate requires per-report uniqueness). The findings that
    // share a fingerprint are numbered by the path, line and column they show,
    // then rule and message, as SARIF numbers them (ADR-0047); the first keeps
    // the base. Analysis failures and the note summary follow, in report order.
    let numbers = occurrences(
        &findings
            .iter()
            .map(|&(at, key)| (issues[at].fingerprint.as_str(), key))
            .collect::<Vec<_>>(),
    );
    let mut seen: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for (&(at, _), occurrence) in findings.iter().zip(numbers) {
        let count = seen.entry(issues[at].fingerprint.clone()).or_insert(0);
        *count = (*count).max(occurrence + 1);
        if occurrence > 0 {
            issues[at].fingerprint = occurrence_fingerprint(&issues[at].fingerprint, occurrence);
        }
    }
    for at in failures {
        let base = std::mem::take(&mut issues[at].fingerprint);
        issues[at].fingerprint = unique_fingerprint(base, &mut seen);
    }
    let note_count = report.total_notes();
    if note_count > 0 {
        let fingerprint = unique_fingerprint(note_summary_fingerprint(), &mut seen);
        issues.push(Issue {
            description: crate::note_summary(note_count),
            check_name: "alint-informational-notes".to_string(),
            fingerprint,
            severity: "info",
            location: Location {
                path: ".".to_string(),
                lines: Lines { begin: 1 },
            },
        });
    }
    let json = serde_json::to_string_pretty(&issues)?;
    w.write_all(json.as_bytes())?;
    w.write_all(b"\n")?;
    Ok(())
}

#[derive(Serialize)]
struct Issue {
    description: String,
    check_name: String,
    fingerprint: String,
    severity: &'static str,
    location: Location,
}

#[derive(Serialize)]
struct Location {
    path: String,
    lines: Lines,
}

#[derive(Serialize)]
struct Lines {
    begin: usize,
}

/// The issue for `violation`, at its base fingerprint; `write_gitlab` then
/// tells apart the issues that share one.
fn build_issue(
    result: &RuleResult,
    violation: &Violation,
    precomputed_fingerprint: Option<String>,
) -> Issue {
    let path = violation
        .report_path()
        .map_or_else(|| ".".to_string(), crate::slash_path);

    // Base identity: the canonical `violation_fingerprint` when `alint` supplies
    // it (so a unique finding matches SARIF + baseline), else the self-contained
    // hash. The canonical fingerprint is DELIBERATELY collision-prone — it
    // discriminates on line *content*, not line number, so a rule firing on two
    // identical lines of one file hashes both the same (baseline count-collapse).
    // GitLab Code Quality requires per-report-UNIQUE fingerprints — a collision
    // makes GitLab silently drop the duplicate finding — so `write_gitlab`
    // disambiguates the Nth (N>0) occurrence of any base identity within this
    // report. Occurrence 0 (the common case) keeps the raw base, so
    // cross-format identity is preserved; only genuine in-report collisions get
    // a deterministic suffix.
    let fingerprint = precomputed_fingerprint
        .unwrap_or_else(|| self_fingerprint(&result.rule_id, &path, &violation.message));

    Issue {
        description: violation.message.to_string(),
        check_name: result.rule_id.to_string(),
        fingerprint,
        severity: severity(result.level),
        location: Location {
            path,
            // `begin: 0` is the conventional "no line info" value;
            // GitLab's UI shows the violation at file-level rather
            // than at a specific line. Some consumers reject 0 —
            // bump to 1 (top of file) for those.
            lines: Lines {
                begin: violation.report_line().unwrap_or(1).max(1),
            },
        },
    }
}

/// The issue for `failure`, at its base fingerprint; `write_gitlab` then
/// tells apart the issues that share one.
fn build_analysis_failure_issue(result: &RuleResult, failure: &AnalysisFailure) -> Issue {
    let path = failure
        .report_path()
        .map_or_else(|| ".".to_string(), crate::slash_path);
    let description = format!(
        "analysis failure ({}): {}",
        failure.kind.as_str(),
        failure.message
    );
    let check_name = format!("{} analysis failure", result.rule_id);
    let base = self_fingerprint(&check_name, &path, &description);

    Issue {
        description,
        check_name,
        fingerprint: base,
        severity: "critical",
        location: Location {
            path,
            lines: Lines {
                begin: failure.report_line().unwrap_or(1).max(1),
            },
        },
    }
}

fn severity(level: Level) -> &'static str {
    // `Off` rules are filtered upstream; the `Off` arm only
    // exists for exhaustiveness and shares the `Info` body.
    #[allow(clippy::match_same_arms)]
    match level {
        Level::Error => "major",
        Level::Warning => "minor",
        Level::Info => "info",
        Level::Off => "info",
    }
}

/// The self-contained FALLBACK base identity, used only when `write_gitlab` is
/// called without pre-computed `fingerprints` (the generic dispatch / benches /
/// unit tests). SHA-256 hex of `rule_id|path|message`; the line number is
/// intentionally omitted so a violation that drifts up or down by a few lines
/// stays the same issue from GitLab's perspective. Per-report uniqueness (the
/// Code Climate requirement) is enforced by the caller's occurrence pass, not
/// here.
///
/// `alint`'s GitLab path instead passes the canonical
/// `baseline::violation_fingerprint` (ADR-0006), so an un-collided finding
/// shares ONE identity across GitLab, SARIF, and baseline mode.
fn self_fingerprint(rule_id: &str, path: &str, message: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(rule_id.as_bytes());
    hasher.update(b"|");
    hasher.update(path.as_bytes());
    hasher.update(b"|");
    hasher.update(message.as_bytes());
    hex_digest(hasher)
}

/// Stable identity for the one synthetic note-summary issue. The input domain
/// cannot be produced by a real rule issue, and the shared occurrence grid
/// still disambiguates a precomputed canonical hash that happens to match.
fn note_summary_fingerprint() -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"alint\0gitlab-synthetic\0informational-notes");
    hex_digest(hasher)
}

fn unique_fingerprint(base: String, seen: &mut std::collections::HashMap<String, u32>) -> String {
    let counter = seen.entry(base.clone()).or_insert(0);
    let occurrence = *counter;
    *counter += 1;
    if occurrence == 0 {
        base
    } else {
        occurrence_fingerprint(&base, occurrence)
    }
}

fn hex_digest(hasher: Sha256) -> String {
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{Report, RuleResult, Violation};
    use serde_json::Value;
    use std::path::{Path, PathBuf};

    fn render(report: &Report) -> String {
        let mut buf = Vec::new();
        write_gitlab(report, None, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn parse(out: &str) -> Vec<Value> {
        match serde_json::from_str(out).unwrap() {
            Value::Array(a) => a,
            other => panic!("expected JSON array, got {other:?}"),
        }
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
    fn empty_report_renders_empty_array() {
        let out = render(&Report {
            results: Vec::new(),
        });
        let arr = parse(&out);
        assert_eq!(arr.len(), 0);
    }

    #[test]
    fn single_violation_emits_one_issue() {
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
        let arr = parse(&render(&report));
        assert_eq!(arr.len(), 1);
        let issue = &arr[0];
        assert_eq!(issue["check_name"], "no-todo");
        assert_eq!(issue["description"], "TODO marker found");
        assert_eq!(issue["severity"], "major");
        assert_eq!(issue["location"]["path"], "src/lib.rs");
        assert_eq!(issue["location"]["lines"]["begin"], 12);
        // 64-char hex string.
        assert_eq!(issue["fingerprint"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn level_severity_mapping() {
        let report = Report {
            results: vec![
                rule(
                    "e",
                    Level::Error,
                    vec![Violation::new("e").with_path(PathBuf::from("a"))],
                ),
                rule(
                    "w",
                    Level::Warning,
                    vec![Violation::new("w").with_path(PathBuf::from("b"))],
                ),
                rule(
                    "i",
                    Level::Info,
                    vec![Violation::new("i").with_path(PathBuf::from("c"))],
                ),
            ],
        };
        let arr = parse(&render(&report));
        let sevs: Vec<&str> = arr
            .iter()
            .map(|i| i["severity"].as_str().unwrap())
            .collect();
        assert_eq!(sevs, vec!["major", "minor", "info"]);
    }

    #[test]
    fn level_off_silenced() {
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
        assert_eq!(parse(&render(&report)).len(), 0);
    }

    #[test]
    fn cross_file_violation_uses_repo_root_path() {
        let report = Report {
            results: vec![rule(
                "unique-pkg",
                Level::Error,
                vec![Violation::new("dup")],
            )],
        };
        let arr = parse(&render(&report));
        assert_eq!(arr[0]["location"]["path"], ".");
        // No line info → defaults to 1 (top of file).
        assert_eq!(arr[0]["location"]["lines"]["begin"], 1);
    }

    #[test]
    fn line_zero_normalizes_to_one() {
        let report = Report {
            results: vec![rule(
                "r",
                Level::Error,
                vec![{
                    let mut value = Violation::new("x").with_path(Path::new("a.rs"));
                    value.line = Some(0);
                    value
                }],
            )],
        };
        let arr = parse(&render(&report));
        assert_eq!(arr[0]["location"]["lines"]["begin"], 1);
    }

    #[test]
    fn fingerprint_stable_across_runs() {
        let report = Report {
            results: vec![rule(
                "r",
                Level::Error,
                vec![{
                    let mut value = Violation::new("msg").with_path(Path::new("a.rs"));
                    value.line = Some(7);
                    value
                }],
            )],
        };
        let a = parse(&render(&report))[0]["fingerprint"]
            .as_str()
            .unwrap()
            .to_string();
        let b = parse(&render(&report))[0]["fingerprint"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(a, b);
    }

    #[test]
    fn fingerprint_independent_of_line_number() {
        // Same rule + path + message but different lines should
        // produce identical fingerprints (drift-tolerant).
        let mk = |line: usize| Report {
            results: vec![rule(
                "r",
                Level::Error,
                vec![{
                    let mut value = Violation::new("same-msg").with_path(Path::new("a.rs"));
                    value.line = Some(line);
                    value
                }],
            )],
        };
        let fp_a = parse(&render(&mk(7)))[0]["fingerprint"].clone();
        let fp_b = parse(&render(&mk(42)))[0]["fingerprint"].clone();
        assert_eq!(fp_a, fp_b);
    }

    #[test]
    fn distinct_findings_same_message_get_distinct_fingerprints() {
        // M10: two genuinely-distinct violations sharing rule+path+message
        // (e.g. a generic-message per-line rule firing on two lines) must NOT
        // collide — the Code Climate spec requires per-report uniqueness, and
        // a collision makes GitLab drop the duplicate.
        let mk_v = |line: usize| {
            let mut value = Violation::new("forbidden pattern").with_path(Path::new("a.rs"));
            value.line = Some(line);
            value
        };
        let report = Report {
            results: vec![rule("no-pat", Level::Error, vec![mk_v(3), mk_v(9)])],
        };
        let arr = parse(&render(&report));
        assert_eq!(arr.len(), 2, "both findings emitted");
        assert_ne!(
            arr[0]["fingerprint"], arr[1]["fingerprint"],
            "distinct findings must have distinct fingerprints"
        );
    }

    #[test]
    fn precomputed_canonical_collision_is_disambiguated_but_first_keeps_identity() {
        // The canonical `violation_fingerprint` DELIBERATELY collides on
        // identical content (baseline count-collapse). GitLab drops findings
        // that share a fingerprint, so when `alint` supplies two identical
        // canonical fingerprints for two distinct findings, the emitted GitLab
        // issues must be UNIQUE — while the FIRST keeps the raw canonical so it
        // still correlates with SARIF/baseline. (Regression: the unified path
        // used to emit the canonical verbatim, dropping the 2nd in GitLab.)
        let mk_v = |line: usize| {
            let mut value = Violation::new("same message").with_path(Path::new("notes.txt"));
            value.line = Some(line);
            value
        };
        let report = Report {
            results: vec![rule("r", Level::Error, vec![mk_v(1), mk_v(2)])],
        };
        // Simulate the engine handing GitLab two IDENTICAL canonical fingerprints.
        let canon = "a".repeat(64);
        let fps = vec![vec![canon.clone(), canon.clone()]];
        let mut buf = Vec::new();
        write_gitlab(&report, Some(&fps), &mut buf).unwrap();
        let arr: Vec<Value> = serde_json::from_slice(&buf).unwrap();
        assert_eq!(arr.len(), 2, "both findings emitted");
        assert_ne!(
            arr[0]["fingerprint"], arr[1]["fingerprint"],
            "colliding canonical fingerprints must be disambiguated per-report"
        );
        assert_eq!(
            arr[0]["fingerprint"], canon,
            "the FIRST occurrence keeps the raw canonical (SARIF/baseline parity)"
        );
    }

    #[test]
    fn note_summary_does_not_collide_with_a_real_issue() {
        // This real issue used to share the summary's fallback fingerprint
        // exactly: rule id + root path + "note-summary".
        let mut result = rule(
            "alint-informational-notes",
            Level::Info,
            vec![Violation::new("note-summary")],
        );
        result.notes.push(Violation::new("detail").as_note());
        let report = Report {
            results: vec![result],
        };

        let arr = parse(&render(&report));
        assert_eq!(arr.len(), 2);
        assert_ne!(arr[0]["fingerprint"], arr[1]["fingerprint"]);
    }

    #[test]
    fn fingerprint_changes_when_message_changes() {
        let mk = |msg: &str| Report {
            results: vec![rule(
                "r",
                Level::Error,
                vec![Violation::new(msg.to_string()).with_path(PathBuf::from("a.rs"))],
            )],
        };
        let fp_a = parse(&render(&mk("alpha")))[0]["fingerprint"].clone();
        let fp_b = parse(&render(&mk("beta")))[0]["fingerprint"].clone();
        assert_ne!(fp_a, fp_b);
    }

    #[test]
    fn multiple_violations_emit_separate_issues() {
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
        let arr = parse(&render(&report));
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn json_output_is_well_formed_with_special_chars() {
        let report = Report {
            results: vec![rule(
                "r/special",
                Level::Error,
                vec![{
                    let mut value = Violation::new(
                        r#"contains "quotes" and \backslashes\ and newline\nliteral"#,
                    )
                    .with_path(std::path::Path::new(r#"a"b\c.rs"#));
                    value.line = Some(1);
                    value
                }],
            )],
        };
        let out = render(&report);
        // Must round-trip through serde_json without error.
        let parsed: Value = serde_json::from_str(&out).unwrap();
        let issue = &parsed[0];
        assert_eq!(issue["check_name"], "r/special");
        // `\` is a separator on Windows (normalized to `/`) but an ordinary
        // filename byte on Unix (kept, and JSON-escaped).
        let expected = if cfg!(windows) {
            r#"a"b/c.rs"#
        } else {
            r#"a"b\c.rs"#
        };
        assert_eq!(issue["location"]["path"], expected);
    }
}
