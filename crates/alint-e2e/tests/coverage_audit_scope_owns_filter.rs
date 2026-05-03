//! Hard audit: no rule in `crates/alint-rules/src/` may hold a
//! standalone `scope_filter: Option<ScopeFilter>` field, and no
//! rule may override `Rule::scope_filter()`. The v0.9.10
//! structural fix moved `Option<ScopeFilter>` into [`Scope`]
//! so a single `Scope::matches(path, &FileIndex)` call covers
//! both path-glob AND ancestor-manifest narrowing — the bug
//! class where v0.9.6, v0.9.7, and v0.9.9 each shipped with
//! one rule class silently dropping `scope_filter:` can no
//! longer recur.
//!
//! Failure means a contributor added a rule that holds its
//! own `scope_filter` field. The fix is to use
//! `Scope::from_spec(spec)` in `build()` and remove the
//! standalone field; the Scope's `matches` will consult the
//! filter automatically. See `docs/design/v0.9/scope-owns-scope-filter.md`.

use std::fs;
use std::path::Path;

#[test]
fn no_rule_holds_standalone_scope_filter_field() {
    let rules_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("alint-rules/src");
    assert!(
        rules_dir.is_dir(),
        "expected alint-rules/src/ at {}",
        rules_dir.display(),
    );

    let mut offenders: Vec<String> = Vec::new();
    for entry in fs::read_dir(&rules_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        // Hits the canonical shape exactly. We're matching the
        // FIELD declaration (struct body), not local variables
        // or function args.
        if src.contains("scope_filter: Option<ScopeFilter>") {
            offenders.push(path.file_name().unwrap().to_string_lossy().to_string());
        }
    }
    assert!(
        offenders.is_empty(),
        "the following rules still hold a standalone `scope_filter: Option<ScopeFilter>` \
         field — use `Scope::from_spec(spec)` in `build()` instead and let the Scope own \
         the filter (v0.9.10 structural fix). Offenders: {offenders:?}",
    );
}

#[test]
fn no_rule_overrides_rule_scope_filter_method() {
    // The trait method `Rule::scope_filter` was removed in
    // v0.9.10 (Phase J.3). Any rule that re-declares it would
    // be dead code AND would suggest the contributor is
    // re-introducing the per-rule field pattern.
    let rules_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("alint-rules/src");
    let mut offenders: Vec<String> = Vec::new();
    for entry in fs::read_dir(&rules_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        // Skip test-fixture and helper signatures — only flag
        // a top-level `fn scope_filter(&self) -> Option<&ScopeFilter>`
        // shape that looks like the deleted Rule trait method.
        if src.contains("fn scope_filter(&self) -> Option<&ScopeFilter>") {
            offenders.push(path.file_name().unwrap().to_string_lossy().to_string());
        }
    }
    assert!(
        offenders.is_empty(),
        "the following rules override the deleted `Rule::scope_filter` method — \
         delete the override; the Scope's `matches` consults the filter (v0.9.10). \
         Offenders: {offenders:?}",
    );
}
