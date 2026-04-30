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
//! `fingerprint` (SHA-256 hex of `rule_id|path|message` for
//! cross-run de-duplication), `severity` (one of
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

use alint_core::{Level, Report, RuleResult, Violation};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub fn write_gitlab(report: &Report, w: &mut dyn Write) -> std::io::Result<()> {
    let mut issues: Vec<Issue> = Vec::new();
    for result in &report.results {
        if result.level == Level::Off {
            continue;
        }
        for violation in &result.violations {
            issues.push(build_issue(result, violation));
        }
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

fn build_issue(result: &RuleResult, violation: &Violation) -> Issue {
    let path = violation
        .path
        .as_ref()
        .map_or_else(|| ".".to_string(), |p| p.display().to_string());

    Issue {
        description: violation.message.to_string(),
        check_name: result.rule_id.to_string(),
        fingerprint: fingerprint(&result.rule_id, &path, &violation.message),
        severity: severity(result.level),
        location: Location {
            path,
            // `begin: 0` is the conventional "no line info" value;
            // GitLab's UI shows the violation at file-level rather
            // than at a specific line. Some consumers reject 0 —
            // bump to 1 (top of file) for those.
            lines: Lines {
                begin: violation.line.unwrap_or(1).max(1),
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

/// Stable per-issue fingerprint for cross-run de-duplication.
/// SHA-256 hex of `rule_id|path|message` — the line number is
/// intentionally omitted so a violation that drifts up or down
/// by a few lines stays the same issue from GitLab's
/// perspective. Two violations of the same rule with the same
/// message in the same file collide, which is the right shape:
/// GitLab will treat them as one ongoing issue.
fn fingerprint(rule_id: &str, path: &str, message: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(rule_id.as_bytes());
    hasher.update(b"|");
    hasher.update(path.as_bytes());
    hasher.update(b"|");
    hasher.update(message.as_bytes());
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
        write_gitlab(report, &mut buf).unwrap();
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
            is_fixable: false,
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
                vec![Violation {
                    path: Some(Path::new("src/lib.rs").into()),
                    message: "TODO marker found".into(),
                    line: Some(12),
                    column: Some(4),
                }],
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
                is_fixable: false,
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
                vec![Violation {
                    path: Some(Path::new("a.rs").into()),
                    message: "x".into(),
                    line: Some(0),
                    column: None,
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
                vec![Violation {
                    path: Some(Path::new("a.rs").into()),
                    message: "msg".into(),
                    line: Some(7),
                    column: None,
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
                vec![Violation {
                    path: Some(Path::new("a.rs").into()),
                    message: "same-msg".into(),
                    line: Some(line),
                    column: None,
                }],
            )],
        };
        let fp_a = parse(&render(&mk(7)))[0]["fingerprint"].clone();
        let fp_b = parse(&render(&mk(42)))[0]["fingerprint"].clone();
        assert_eq!(fp_a, fp_b);
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
                vec![Violation {
                    path: Some(std::path::Path::new(r#"a"b\c.rs"#).into()),
                    message: r#"contains "quotes" and \backslashes\ and newline\nliteral"#.into(),
                    line: Some(1),
                    column: None,
                }],
            )],
        };
        let out = render(&report);
        // Must round-trip through serde_json without error.
        let parsed: Value = serde_json::from_str(&out).unwrap();
        let issue = &parsed[0];
        assert_eq!(issue["check_name"], "r/special");
        assert_eq!(issue["location"]["path"], r#"a"b\c.rs"#);
    }
}
