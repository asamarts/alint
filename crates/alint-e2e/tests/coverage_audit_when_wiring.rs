//! Hard audit: every cross-file iteration rule must wire
//! `when_iter:` through the shared `parse_when_iter` helper
//! AND consult the resulting `Option<WhenExpr>` in its
//! evaluate path. Mirrors the recurrence-risk audits added in
//! v0.9.10 (`coverage_audit_scope_owns_filter.rs`) and v0.9.11
//! (`coverage_audit_git_tracked_only.rs`) — the bug class
//! is "rule type X ships with field Y but forgets to wire Y
//! through the engine," and the audit catches it at PR time.
//!
//! `when_iter:` lives on `for_each_dir`, `for_each_file`, and
//! `every_matching_has` (the three cross-file rules that gain
//! per-iteration filtering). Each calls
//! `for_each_dir::parse_when_iter` at build time and threads
//! the resulting `Option<WhenExpr>` into the shared
//! `evaluate_for_each` helper, which evaluates it per
//! iteration. A new cross-file iteration rule that adds
//! `when_iter:` to its YAML schema but skips one of these
//! steps would silently no-op the filter.
//!
//! The audit asserts each known iteration rule's source
//! contains both wire-up calls. Adding a 4th iteration rule
//! requires updating both the rule's source AND this audit's
//! `EXPECTED` list — the test failure on the new rule is the
//! signal to verify the wiring.

use std::fs;
use std::path::Path;

/// Cross-file iteration rules that support `when_iter:`. Add
/// a new entry here when introducing another iteration rule —
/// the test failure makes the wiring requirement visible.
const EXPECTED: &[&str] = &["for_each_dir", "for_each_file", "every_matching_has"];

#[test]
fn cross_file_iteration_rules_wire_when_iter() {
    let rules_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("alint-rules/src");
    assert!(
        rules_dir.is_dir(),
        "expected alint-rules/src/ at {}",
        rules_dir.display(),
    );

    let mut violations: Vec<String> = Vec::new();
    for rule in EXPECTED {
        let path = rules_dir.join(format!("{rule}.rs"));
        if !path.is_file() {
            violations.push(format!(
                "{rule}.rs not found at {} — EXPECTED list out of sync with the rules tree",
                path.display(),
            ));
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();

        // The build-time parse: every iteration rule must call
        // the shared helper to compile its `when_iter:` source
        // string into an `Option<WhenExpr>`. for_each_dir is
        // the helper's owner so it must DEFINE the helper; the
        // other two must CALL it.
        let parses_when_iter = if *rule == "for_each_dir" {
            src.contains("pub(crate) fn parse_when_iter")
        } else {
            src.contains("parse_when_iter(")
        };
        if !parses_when_iter {
            violations.push(format!(
                "{rule}.rs: does not call `parse_when_iter(...)` at build time — \
                 a `when_iter:` field on the rule's YAML would be silently dropped \
                 (never compiled into a `WhenExpr`)",
            ));
        }

        // The evaluate-time consultation: every iteration rule
        // must thread its compiled `Option<WhenExpr>` into the
        // shared `evaluate_for_each` helper (which is the only
        // dispatch site that actually consults the filter per
        // iteration). A rule that holds a `when_iter:
        // Option<WhenExpr>` field but doesn't pass it through
        // would silently no-op.
        let consults_in_dispatch =
            src.contains("evaluate_for_each(") && src.contains("self.when_iter");
        if !consults_in_dispatch {
            violations.push(format!(
                "{rule}.rs: does not thread `self.when_iter` into `evaluate_for_each(...)` \
                 — `when_iter:` would be parsed at build but ignored at evaluate",
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "when_iter wiring incomplete:\n  - {}",
        violations.join("\n  - "),
    );
}
