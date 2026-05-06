//! v0.9.16 uniformity audit: every Options struct in the rule
//! registry MUST carry `#[serde(deny_unknown_fields)]`.
//!
//! Without it, a rule's Options deserialiser silently accepts unknown
//! fields, which means:
//! - The Phase 3 did-you-mean enricher can't fire (no unknown-field
//!   error to enrich).
//! - The Phase 5 JSON Schema spot-checks pass at edit time, but the
//!   parser silently accepts the same unknown-field config at load
//!   time → schema and runtime drift.
//!
//! v0.9.15 had this attribute on ~50 % of rule kinds. v0.9.16 adds it
//! to the remaining 13 (`file_content_matches`, `file_content_forbidden`,
//! `file_header`, `file_footer`, `file_max_lines`, `file_max_size`,
//! `file_min_lines`, `file_min_size`, `file_shebang`, `filename_case`,
//! `filename_regex`, `commented_out_code`, `markdown_paths_resolve`).
//! These integration tests verify each of those 13 kinds now produces
//! a hint-bearing did-you-mean error when given a typo'd field —
//! continuously-verified evidence that the Phase 3 + Phase 5
//! coverage is now uniform across the rule catalogue.
//!
//! The test names match the rule kinds so a regression localises
//! immediately to the Options struct that needs the attr restored.

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

fn assert_unknown_field_caught(yaml: &str, expected_canonical: &str) {
    let err = build_err(yaml);
    assert!(
        err.contains("unknown field"),
        "expected `unknown field` rejection; got:\n{err}",
    );
    // Levenshtein fallback or curated suggestion should fire.
    assert!(
        err.contains("did you mean"),
        "expected did-you-mean enrichment; got:\n{err}",
    );
    assert!(
        err.contains(expected_canonical),
        "expected suggestion to mention `{expected_canonical}`; got:\n{err}",
    );
}

#[test]
fn file_content_matches_rejects_unknown_field() {
    let yaml = r#"
id: t
kind: file_content_matches
level: error
paths: "**/*.md"
pattern: "TODO"
patten: "typo"
"#;
    assert_unknown_field_caught(yaml, "pattern");
}

#[test]
fn file_content_forbidden_rejects_unknown_field() {
    let yaml = r#"
id: t
kind: file_content_forbidden
level: error
paths: "**/*.md"
pattern: "FIXME"
patten: "typo"
"#;
    assert_unknown_field_caught(yaml, "pattern");
}

#[test]
fn file_header_rejects_unknown_field() {
    let yaml = r#"
id: t
kind: file_header
level: error
paths: "**/*.rs"
pattern: "Copyright"
linez: 5
"#;
    assert_unknown_field_caught(yaml, "lines");
}

#[test]
fn file_footer_rejects_unknown_field() {
    let yaml = r#"
id: t
kind: file_footer
level: error
paths: "**/*.rs"
pattern: "EOF"
linez: 5
"#;
    assert_unknown_field_caught(yaml, "lines");
}

#[test]
fn file_max_lines_rejects_unknown_field() {
    let yaml = r#"
id: t
kind: file_max_lines
level: warning
paths: "**/*.rs"
max_lines: 1000
maxlines: 1500
"#;
    assert_unknown_field_caught(yaml, "max_lines");
}

#[test]
fn file_max_size_rejects_unknown_field() {
    let yaml = r#"
id: t
kind: file_max_size
level: warning
paths: "**/*"
max_bytes: 102400
maxbytes: 200000
"#;
    assert_unknown_field_caught(yaml, "max_bytes");
}

#[test]
fn file_min_lines_rejects_unknown_field() {
    let yaml = r#"
id: t
kind: file_min_lines
level: info
paths: "README.md"
min_lines: 3
minlines: 5
"#;
    assert_unknown_field_caught(yaml, "min_lines");
}

#[test]
fn file_min_size_rejects_unknown_field() {
    let yaml = r#"
id: t
kind: file_min_size
level: info
paths: "LICENSE"
min_bytes: 200
minbytes: 500
"#;
    assert_unknown_field_caught(yaml, "min_bytes");
}

#[test]
fn file_shebang_rejects_unknown_field() {
    let yaml = r#"
id: t
kind: file_shebang
level: warning
paths: "**/*.sh"
shebang: "^#!/usr/bin/env bash$"
sheban: "typo"
"#;
    assert_unknown_field_caught(yaml, "shebang");
}

#[test]
fn filename_case_rejects_unknown_field() {
    let yaml = r#"
id: t
kind: filename_case
level: warning
paths: "src/**/*.rs"
case: snake_case
caes: kebab-case
"#;
    assert_unknown_field_caught(yaml, "case");
}

#[test]
fn filename_regex_rejects_unknown_field() {
    let yaml = r#"
id: t
kind: filename_regex
level: warning
paths: "tests/**/*.rs"
pattern: "^test_.+\\.rs$"
patten: "typo"
"#;
    assert_unknown_field_caught(yaml, "pattern");
}

#[test]
fn commented_out_code_rejects_unknown_field() {
    let yaml = r#"
id: t
kind: commented_out_code
level: info
paths: "src/**/*.rs"
language: rust
languag: typo
"#;
    assert_unknown_field_caught(yaml, "language");
}

#[test]
fn markdown_paths_resolve_rejects_unknown_field() {
    let yaml = r#"
id: t
kind: markdown_paths_resolve
level: warning
paths: "AGENTS.md"
prefixes: ["src/", "docs/"]
prefxes: ["other/"]
"#;
    assert_unknown_field_caught(yaml, "prefixes");
}
