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

use std::borrow::Cow;
use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

use alint_core::{Level, ProposedEdit, Report};
use serde::Serialize;

#[derive(Serialize)]
struct AgentReport<'a> {
    schema_version: u32,
    format: &'static str,
    summary: AgentSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<AgentNotes>,
    violations: Vec<AgentViolation<'a>>,
    analysis_failures: Vec<AgentAnalysisFailure<'a>>,
}

#[derive(Serialize)]
struct AgentSummary {
    total_violations: usize,
    by_severity: BySeverity,
    fixable_violations: usize,
    passing_rules: usize,
    failing_rules: usize,
    total_analysis_failures: usize,
}

#[derive(Serialize)]
struct AgentNotes {
    count: usize,
    details_format: &'static str,
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
    // Lossy UTF-8: a non-UTF-8 repo path (legal on Unix) must not error the
    // whole document the way serde's `&Path` serializer does. Matches the
    // human `agent_instruction` (built from `path.display()`) and every other
    // renderer. Invalid bytes → U+FFFD.
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    column: Option<usize>,
    /// The rule's violation message sanitized against terminal escapes, bidi
    /// controls, and zero-width characters (#600), suitable for human and
    /// agent review.
    human_message: Cow<'a, str>,
    /// Templated remediation phrasing optimised for an LLM to act
    /// on. Composed from severity, message, location, fix
    /// availability, and policy URL — see `build_agent_instruction`.
    agent_instruction: String,
    fix_available: bool,
    /// Exact argv (after the `alint` program name) an agent can run to
    /// auto-fix this single violation, present iff `fix_available`.
    /// Lets an agent execute the fix programmatically instead of
    /// parsing the command out of the English `agent_instruction` —
    /// and is checked by a CLI-parse regression test, so it can never
    /// name a flag the binary doesn't accept.
    ///
    /// Scope note: this is always whole-tree (`fix --only <id>`). When the
    /// producing `check` ran under `--changed`, the suggested fix is *not*
    /// scoped to the diff — an agent that wants to stay within its changed
    /// set should add `--changed` (and `--base <ref>`) itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    fix_command: Option<Vec<&'a str>>,
    /// The concrete Safe fix edit(s) alint would apply for this violation
    /// (source region + replacement text) -- distinct from `fix_command` (the
    /// argv to run). Lets an agent apply the fix directly. Present for a
    /// Safe-fixable finding only when every target path has an exact JSON string
    /// representation; `fix_command` remains available otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    proposed_edit: Option<&'a [ProposedEdit]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_url: Option<Cow<'a, str>>,
}
#[derive(Serialize)]
struct AgentAnalysisFailure<'a> {
    rule_id: &'a str,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    column: Option<usize>,
    human_message: Cow<'a, str>,
}

#[allow(clippy::too_many_lines)]
pub fn write_agent(report: &Report, w: &mut dyn Write) -> std::io::Result<()> {
    let total_violations: usize = report.results.iter().map(|r| r.violations.len()).sum();
    let total_failures: usize = report
        .results
        .iter()
        .map(|r| r.analysis_failures.len())
        .sum();
    let mut violations: Vec<AgentViolation<'_>> = Vec::with_capacity(total_violations);
    let mut by_sev = BySeverity {
        error: 0,
        warning: 0,
        info: 0,
    };
    let mut fixable_violations = 0;
    let mut analysis_failures = Vec::with_capacity(total_failures);

    for r in &report.results {
        if !r.violations.is_empty() {
            let severity = severity_str(r.level);
            let safe_rule_id = crate::sanitize::sanitize_terminal(&r.rule_id);
            let safe_policy_url = r
                .policy_url
                .as_deref()
                .map(crate::sanitize::sanitize_terminal);
            for v in &r.violations {
                match r.level {
                    Level::Error => by_sev.error += 1,
                    Level::Warning => by_sev.warning += 1,
                    Level::Info => by_sev.info += 1,
                    Level::Off => {} // off rules don't produce violations, but be explicit
                }
                // Per-VIOLATION fixability, not per-rule `r.is_fixable`: an agent
                // acts on a single violation, and `fix_command` is documented as the
                // argv to fix THIS violation. A rule with a fixer that skips this
                // violation (an unconvertible stem) -- or an Unsafe-tier fixer a bare
                // `alint fix` only suggests -- must not advertise a fix here, or an
                // agent runs the command, the violation persists, and it loops.
                if v.is_fixable {
                    fixable_violations += 1;
                }
                let file = v.report_path().map(Path::to_string_lossy);
                let line = v.report_line();
                let column = v.report_column();
                let human_message =
                    crate::sanitize::sanitize_terminal_multiline(v.message.as_ref());
                let agent_instruction = build_agent_instruction(
                    severity,
                    &human_message,
                    file.as_deref(),
                    line,
                    column,
                    v.is_fixable,
                    &safe_rule_id,
                    safe_policy_url.as_deref(),
                );
                violations.push(AgentViolation {
                    rule_id: r.rule_id.as_ref(),
                    severity,
                    file,
                    line,
                    column,
                    human_message,
                    agent_instruction,
                    fix_available: v.is_fixable,
                    fix_command: v
                        .is_fixable
                        .then(|| vec!["fix", "--only", r.rule_id.as_ref()]),
                    proposed_edit: crate::utf8_proposed_edits(v),
                    policy_url: safe_policy_url.clone(),
                });
            }
        }
        analysis_failures.extend(
            r.analysis_failures
                .iter()
                .map(|failure| AgentAnalysisFailure {
                    rule_id: r.rule_id.as_ref(),
                    kind: failure.kind.as_str(),
                    file: failure.report_path().map(Path::to_string_lossy),
                    line: failure.report_line(),
                    column: failure.report_column(),
                    human_message: crate::sanitize::sanitize_terminal_multiline(
                        failure.message.as_ref(),
                    ),
                }),
        );
    }

    let total = violations.len();
    let note_count = report.total_notes();
    let out = AgentReport {
        schema_version: 1,
        format: "agent",
        summary: AgentSummary {
            total_violations: total,
            by_severity: by_sev,
            fixable_violations,
            passing_rules: report.passing_rules(),
            failing_rules: report.failing_rules(),
            total_analysis_failures: analysis_failures.len(),
        },
        notes: (note_count > 0).then_some(AgentNotes {
            count: note_count,
            details_format: "json",
        }),
        violations,
        analysis_failures,
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
///   resolve: edit README (or run `alint fix --only
///   readme-exists` to apply the auto-fix)."`
/// - Path-bound, non-fixable: `"warning: console.log left in
///   non-test source. To resolve: edit src/api.ts:2:1."`
/// - Cross-file (no path): `"error: Multiple lockfiles present.
///   To resolve: this is a repository-level rule with no
///   specific file location; resolve the underlying invariant
///   the rule enforces."`
#[allow(clippy::too_many_arguments)]
fn build_agent_instruction(
    severity: &str,
    human_message: &str,
    file: Option<&str>,
    line: Option<usize>,
    column: Option<usize>,
    is_fixable: bool,
    safe_rule_id: &str,
    safe_policy_url: Option<&str>,
) -> String {
    let extra = file.map_or(112, |p| p.len() + 32)
        + if is_fixable {
            safe_rule_id.len() + 48
        } else {
            0
        }
        + safe_policy_url.map_or(0, |u| u.len() + 40);
    let mut out = String::with_capacity(severity.len() + human_message.len() + 20 + extra);
    out.push_str(severity);
    out.push_str(": ");
    out.push_str(human_message);
    if !out.ends_with('.') {
        out.push('.');
    }

    out.push_str(" To resolve: ");
    match file {
        Some(path) => {
            out.push_str("edit ");
            out.push_str(&crate::sanitize::sanitize_terminal(path));
            if let Some(line) = line {
                let _ = write!(out, ":{line}");
                if let Some(column) = column {
                    let _ = write!(out, ":{column}");
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

    if is_fixable {
        out.push_str(" (or run `alint fix --only ");
        out.push_str(safe_rule_id);
        out.push_str("` to apply the auto-fix)");
    }

    if let Some(url) = safe_policy_url {
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
        assert_eq!(
            v["violations"].as_array().unwrap(),
            &Vec::<serde_json::Value>::new()
        );
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
            notes: Vec::new(),
            is_fixable: false,
            analysis_failures: Vec::new(),
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
        let mut v = Violation::new("A README is required at the root.");
        v.is_fixable = true; // a bare `alint fix` would create it (Safe tier)
        let result = RuleResult {
            rule_id: "readme-exists".into(),
            level: Level::Error,
            policy_url: Some("https://example.com/policy".into()),
            violations: vec![v],
            notes: Vec::new(),
            is_fixable: true,
            analysis_failures: Vec::new(),
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
            inst.contains("(or run `alint fix --only readme-exists` to apply the auto-fix)"),
            "fixable instruction should carry the parenthetical fix hint: {inst}"
        );
        assert!(
            !inst.contains('\u{2014}'),
            "agent_instruction must not contain an em-dash: {inst}"
        );
        assert!(
            inst.contains("https://example.com/policy"),
            "policy URL should be cited: {inst}"
        );
        assert_eq!(v["violations"][0]["fix_available"], true);
        assert_eq!(v["summary"]["fixable_violations"], 1);
    }

    #[test]
    fn fix_affordance_is_per_violation_not_per_rule() {
        // Round-7 (A3-F1): the agent format must derive fix_available / fix_command
        // / the instruction hint / the fixable_violations count from the
        // PER-VIOLATION flag, not the per-rule one. A rule that declares a fixer
        // (is_fixable: true) but has a violation a bare fix won't resolve (an
        // unconvertible stem, or an Unsafe file_remove) must NOT advertise a fix
        // for that violation -- else an agent runs the command, the violation
        // persists, and it loops.
        let mut ok =
            Violation::new("stem myFile is not snake_case").with_path(PathBuf::from("myFile.rs"));
        ok.is_fixable = true;
        let stuck =
            Violation::new("stem café is not snake_case").with_path(PathBuf::from("café.rs"));
        // stuck.is_fixable stays false
        let result = RuleResult {
            rule_id: "snake-names".into(),
            level: Level::Warning,
            policy_url: None,
            violations: vec![stuck, ok],
            notes: Vec::new(),
            is_fixable: true, // the RULE has a fixer
            analysis_failures: Vec::new(),
        };
        let out = run(vec![result]);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v["summary"]["fixable_violations"], 1,
            "count is per-violation"
        );
        let by_file = |needle: &str| -> serde_json::Value {
            v["violations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|viol| viol["file"].as_str().is_some_and(|f| f.contains(needle)))
                .unwrap()
                .clone()
        };
        let myfile = by_file("myFile");
        let cafe = by_file("café");
        assert_eq!(myfile["fix_available"], true);
        assert!(
            !myfile["fix_command"].is_null(),
            "convertible has a fix_command"
        );
        assert!(
            myfile["agent_instruction"]
                .as_str()
                .unwrap()
                .contains("alint fix"),
            "convertible instruction offers the fix"
        );
        assert_eq!(cafe["fix_available"], false, "unconvertible has no fix");
        assert!(
            cafe["fix_command"].is_null(),
            "unconvertible has no fix_command"
        );
        assert!(
            !cafe["agent_instruction"]
                .as_str()
                .unwrap()
                .contains("alint fix"),
            "unconvertible instruction must not offer a fix that won't apply"
        );
    }

    #[test]
    fn cross_file_violation_uses_repository_level_phrasing() {
        let result = RuleResult {
            rule_id: "lockfiles-only-one".into(),
            level: Level::Error,
            policy_url: None,
            violations: vec![Violation::new("Multiple lockfiles found.")],
            notes: Vec::new(),
            is_fixable: false,
            analysis_failures: Vec::new(),
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
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
            },
            RuleResult {
                rule_id: "rule-b".into(),
                level: Level::Warning,
                policy_url: None,
                violations: vec![Violation::new("b1"), Violation::new("b2")],
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
            },
            RuleResult {
                rule_id: "rule-c".into(),
                level: Level::Info,
                policy_url: None,
                violations: vec![Violation::new("c")],
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
            },
            RuleResult {
                rule_id: "rule-d".into(),
                level: Level::Warning,
                policy_url: None,
                violations: vec![],
                notes: Vec::new(),
                is_fixable: false,
                analysis_failures: Vec::new(),
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

    #[test]
    fn bidi_zero_width_and_esc_escaped_in_agent_messages_but_not_file_field() {
        use alint_core::{AnalysisFailure, AnalysisFailureKind};

        let raw_path = "a\x1b[31m\u{202E}\u{200B}\u{180E}Z.txt";
        let raw_msg = format!("file is forbidden: {raw_path}");
        let raw_url = "https://example.invalid/\u{202E}\u{200B}\x1b[31mpolicy";
        let mut violation = Violation::new(raw_msg.clone()).with_path(PathBuf::from(raw_path));
        violation.is_fixable = true;
        let result = RuleResult {
            rule_id: "no-\u{202E}txt".into(),
            level: Level::Error,
            policy_url: Some(raw_url.into()),
            violations: vec![violation],
            notes: Vec::new(),
            is_fixable: true,
            analysis_failures: vec![
                AnalysisFailure::new(AnalysisFailureKind::ResourceParse, raw_msg)
                    .with_path(PathBuf::from(raw_path)),
            ],
        };
        let out = run(vec![result]);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();

        let viol = &v["violations"][0];
        assert_eq!(viol["file"].as_str(), Some(raw_path));
        for field in ["human_message", "agent_instruction", "policy_url"] {
            let s = viol[field].as_str().unwrap();
            for raw in ['\x1b', '\u{202E}', '\u{200B}', '\u{180E}'] {
                assert!(
                    !s.contains(raw),
                    "raw U+{:04X} survived in agent.{field}: {s}",
                    raw as u32
                );
            }
            assert!(s.contains("\\x1b"), "missing ESC escape in {field}: {s}");
            assert!(
                s.contains("\\u{202e}"),
                "missing bidi escape in {field}: {s}"
            );
            assert!(
                s.contains("\\u{200b}"),
                "missing zwsp escape in {field}: {s}"
            );
        }
        assert!(
            viol["human_message"]
                .as_str()
                .unwrap()
                .contains("\\u{180e}"),
            "missing mvs escape in human_message"
        );
        assert!(
            viol["agent_instruction"]
                .as_str()
                .unwrap()
                .contains("no-\\u{202e}txt"),
            "fix hint rule_id must be sanitized in agent_instruction"
        );

        let af = &v["analysis_failures"][0];
        assert_eq!(af["file"].as_str(), Some(raw_path));
        let af_msg = af["human_message"].as_str().unwrap();
        assert!(!af_msg.contains('\x1b'));
        assert!(!af_msg.contains('\u{202E}'));
        assert!(af_msg.contains("\\x1b"));
        assert!(af_msg.contains("\\u{202e}"));
    }
}
