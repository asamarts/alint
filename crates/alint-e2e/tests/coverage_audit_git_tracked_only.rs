//! Hard audit: any rule that holds a `git_tracked_only: bool`
//! field MUST advertise its narrowing mode via
//! [`Rule::git_tracked_mode`] (returning `FileOnly` or `DirAware`
//! when the user opts in via the YAML field). The engine reads
//! this trait method to (a) collect the git-tracked path set and
//! (b) build a pre-filtered `FileIndex` it hands to the rule —
//! the per-rule runtime `is_git_tracked(...)` /
//! `dir_has_tracked_files(...)` checks that lived on
//! `evaluate()` pre-v0.9.11 are gone (subsumed by the engine
//! narrowing).
//!
//! Rationale: same recurrence-risk shape that produced the
//! v0.9.6 / v0.9.7 / v0.9.9 silent-no-op `scope_filter:` bugs
//! (see `docs/design/v0.9/scope-owns-scope-filter.md`). v0.9.10
//! closed the `scope_filter` bug class structurally via
//! `Scope` ownership; v0.9.11 closes the `git_tracked_only`
//! bug class structurally via the engine-side filtered
//! `FileIndex` (see
//! `docs/design/v0.9/git-tracked-filtered-index.md`).
//!
//! This audit catches both directions:
//! 1. A rule that ships `git_tracked_only: bool` but forgets
//!    `git_tracked_mode()` → `Off` default → engine won't
//!    pre-filter and the rule appears to ignore the user's opt-in.
//! 2. A rule that re-introduces a per-evaluate
//!    `is_git_tracked` / `dir_has_tracked_files` runtime check
//!    → potentially conflicts with the engine pre-filter
//!    (double-filtering or, worse, divergent semantics).
//!
//! See `git_tracked_only` design notes in
//! `crates/alint-core/src/config.rs::RuleSpec`.

use std::fs;
use std::path::Path;

#[test]
fn git_tracked_only_field_is_fully_wired() {
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
    for entry in fs::read_dir(&rules_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        // Has the field declared on the rule struct?
        // Match the canonical declaration shape so we don't
        // false-positive on doc comments or string literals.
        let has_field = src.contains("git_tracked_only: bool,");
        if !has_field {
            continue;
        }

        // v0.9.11: must advertise `git_tracked_mode` so the
        // engine knows to pre-filter the index handed to the
        // rule. The default `Off` means "rule does not opt in"
        // — but if the rule has the YAML field, the override
        // is required.
        let has_mode_override = src.contains("fn git_tracked_mode");
        if !has_mode_override {
            violations.push(format!(
                "{name}: declares `git_tracked_only: bool` but does not override \
                 `Rule::git_tracked_mode()` — engine defaults to `GitTrackedMode::Off` \
                 and won't pre-filter the index, so the user's `git_tracked_only: true` \
                 has no observable effect (the rule fires on every match regardless of \
                 git-tracked state)",
            ));
        }

        // v0.9.11: per-rule runtime `is_git_tracked` /
        // `dir_has_tracked_files` checks are gone. Re-
        // introducing them risks double-filtering or
        // divergent semantics from the engine pre-filter.
        let has_runtime_check =
            src.contains("is_git_tracked(") || src.contains("dir_has_tracked_files(");
        if has_runtime_check {
            violations.push(format!(
                "{name}: declares `git_tracked_only: bool` AND calls \
                 `ctx.is_git_tracked(...)` or `ctx.dir_has_tracked_files(...)` \
                 inside `evaluate` — these are now subsumed by the engine's \
                 per-rule pre-filtered FileIndex (v0.9.11 structural fix). \
                 Drop the runtime check; the engine's filtered index handed via \
                 `pick_ctx` covers the narrowing.",
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "git_tracked_only wiring incomplete:\n  - {}",
        violations.join("\n  - "),
    );
}
