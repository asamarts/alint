//! Hard audit: any rule that holds a `git_tracked_only: bool`
//! field MUST wire it through both ends of the contract — the
//! engine-side `wants_git_tracked()` override (so the engine
//! actually populates `Context::git_tracked`) AND the runtime
//! `ctx.is_git_tracked(...)` / `ctx.dir_has_tracked_files(...)`
//! check (so the rule actually narrows its evaluation to
//! tracked entries when the user opts in).
//!
//! Rationale: this is the same recurrence-risk shape that
//! produced the v0.9.6 / v0.9.7 / v0.9.9 silent-no-op
//! `scope_filter:` bugs (see
//! `docs/design/v0.9/scope-owns-scope-filter.md`). v0.9.10
//! collapsed `scope_filter` into `Scope` so the compiler
//! enforces correct usage; `git_tracked_only` carries different
//! semantics (the data lives on `Context`, not on the rule)
//! so the structural fix is harder to land. This audit closes
//! the recurrence gap pragmatically — if a contributor adds a
//! rule with `git_tracked_only:` and forgets either wire-up,
//! CI catches it at PR time rather than in production as
//! "the rule isn't filtering anything."
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

        // Both wire-ups must be present.
        let has_wants_override = src.contains("fn wants_git_tracked");
        let has_runtime_check = src.contains("is_git_tracked")
            || src.contains("dir_has_tracked_files");

        if !has_wants_override {
            violations.push(format!(
                "{name}: declares `git_tracked_only: bool` but does not override \
                 `Rule::wants_git_tracked()` — engine won't populate \
                 `Context::git_tracked` and the rule's runtime check will \
                 always silently no-op",
            ));
        }
        if !has_runtime_check {
            violations.push(format!(
                "{name}: declares `git_tracked_only: bool` but does not call \
                 `ctx.is_git_tracked(...)` or `ctx.dir_has_tracked_files(...)` \
                 inside its `evaluate` — opting in via the YAML field has no \
                 observable effect, the rule will fire on every match \
                 regardless",
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "git_tracked_only wiring incomplete:\n  - {}",
        violations.join("\n  - "),
    );
}
