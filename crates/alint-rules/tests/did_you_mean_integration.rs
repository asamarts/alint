//! Integration tests for the v0.9.15 Phase 3 did-you-mean parse-error
//! enrichment.
//!
//! The unit tests in `crates/alint-core/src/did_you_mean.rs` cover
//! the message-rewriting logic in isolation. These tests exercise the
//! full path: build a malformed `RuleSpec` through the real
//! `RuleRegistry::build`, confirm the resulting error message contains
//! a "did you mean" line pointing at the canonical-correct field name.
//!
//! One test per high-drift schema rename catalogued in
//! `docs/development/CONFIG-AUTHORING.md` (the curated overrides) +
//! one Levenshtein-fallback test + one negative test.
//!
//! YAML shape note: `RuleSpec` uses `#[serde(flatten)]` to collect
//! rule-kind-specific fields into an `extra:` mapping. So the kind's
//! options live at the top level of the YAML alongside `id`, `kind`,
//! `level`, `paths`, etc. — not nested under any wrapping key.

use alint_core::Error;
use alint_rules::builtin_registry;

fn build_err(yaml: &str) -> String {
    let spec: alint_core::RuleSpec = serde_yaml_ng::from_str(yaml).unwrap();
    let registry = builtin_registry();
    match registry.build(&spec) {
        Err(Error::RuleConfig { message, .. }) => message,
        Err(other) => panic!("expected RuleConfig error, got: {other:?}"),
        Ok(_) => panic!("expected build to fail; YAML was:\n{yaml}"),
    }
}

#[test]
fn argv_on_command_rule_suggests_command() {
    // Pitfall #1 — `argv:` is the conventional Go/Rust/JS name for
    // an argv field; alint's `command` rule uses `command:`.
    let yaml = r#"
id: my-shellcheck
kind: command
level: error
paths: "**/*.sh"
argv: ["shellcheck", "-x", "{path}"]
"#;
    let err = build_err(yaml);
    assert!(
        err.contains("did you mean: `command`"),
        "expected did-you-mean for `argv` → `command`; got:\n{err}",
    );
}

#[test]
fn secondary_on_pair_rule_suggests_partner() {
    // Pitfall #4 — the natural English word for the second of a pair.
    let yaml = r#"
id: c-h-pair
kind: pair
level: error
primary: "src/**/*.c"
secondary: "{dir}/{stem}.h"
"#;
    let err = build_err(yaml);
    assert!(
        err.contains("did you mean: `partner`"),
        "expected did-you-mean for `secondary` → `partner`; got:\n{err}",
    );
}

#[test]
fn style_on_line_endings_suggests_target() {
    // Pitfall #8 — most editor configs call this `style`.
    let yaml = r#"
id: lf-only
kind: line_endings
level: warning
paths: "**/*"
style: lf
"#;
    let err = build_err(yaml);
    assert!(
        err.contains("did you mean: `target`"),
        "expected did-you-mean for `style` → `target`; got:\n{err}",
    );
}

#[test]
fn pattern_on_file_starts_with_suggests_prefix() {
    // Pitfall #9 — broader content rules use `pattern:`; literal-anchor
    // rules use `prefix:`.
    let yaml = r#"
id: gui-prefix
kind: file_starts_with
level: warning
paths: "**/*.goml"
pattern: "// "
"#;
    let err = build_err(yaml);
    assert!(
        err.contains("did you mean: `prefix`"),
        "expected did-you-mean for `pattern` → `prefix`; got:\n{err}",
    );
}

#[test]
fn pattern_on_file_ends_with_suggests_suffix() {
    let yaml = r#"
id: footer-check
kind: file_ends_with
level: warning
paths: "**/*.md"
pattern: "<!-- end -->"
"#;
    let err = build_err(yaml);
    assert!(
        err.contains("did you mean: `suffix`"),
        "expected did-you-mean for `pattern` → `suffix`; got:\n{err}",
    );
}

#[test]
fn matches_on_path_equals_suggests_equals() {
    // Pitfall #16 — bool-matching: writers reach for `matches:` because
    // that's what the family's name suggests; `*_path_equals` only
    // accepts `equals:`.
    let yaml = r#"
id: cargo-publish-false
kind: toml_path_equals
level: warning
paths: "Cargo.toml"
path: "$.package.publish"
matches: "^false$"
"#;
    let err = build_err(yaml);
    assert!(
        err.contains("did you mean: `equals`"),
        "expected did-you-mean for `matches` → `equals` on toml_path_equals; got:\n{err}",
    );
}

#[test]
fn equals_on_path_matches_suggests_matches() {
    let yaml = r#"
id: cargo-edition
kind: toml_path_matches
level: error
paths: "Cargo.toml"
path: "$.package.edition"
equals: "2021"
"#;
    let err = build_err(yaml);
    assert!(
        err.contains("did you mean: `matches`"),
        "expected did-you-mean for `equals` → `matches` on toml_path_matches; got:\n{err}",
    );
}

// --- Levenshtein fallback path -------------------------------------

#[test]
fn typo_close_to_expected_field_suggests_via_levenshtein() {
    // `paths` is a top-level RuleSpec field so a typo there gets
    // caught before we hit the rule's options. Use a kind-specific
    // typo instead — `commnd` (typo of `command`) on the command
    // rule. Edit distance 1 from `command`.
    let yaml = r#"
id: my-shellcheck
kind: command
level: error
paths: "**/*.sh"
commnd: ["shellcheck"]
"#;
    let err = build_err(yaml);
    assert!(
        err.contains("did you mean:"),
        "expected SOME did-you-mean for typo `commnd`; got:\n{err}",
    );
}

// --- Negative test --------------------------------------------------

#[test]
fn unrelated_field_with_no_close_match_passes_through() {
    // `xyzqq` is far from any expected field on `command` (closest
    // expected field is `command`, distance 6). No curated entry.
    // Should not produce a did-you-mean line.
    let yaml = r#"
id: my-shellcheck
kind: command
level: error
paths: "**/*.sh"
command: ["shellcheck"]
xyzqq: "value"
"#;
    let err = build_err(yaml);
    assert!(
        !err.contains("did you mean"),
        "expected NO did-you-mean for far-typo `xyzqq`; got:\n{err}",
    );
}
