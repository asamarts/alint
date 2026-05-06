//! Integration tests for v0.9.15 Phase 4 — domain-specific error
//! messages.
//!
//! Each test exercises a specific pitfall from
//! `docs/development/CONFIG-AUTHORING.md` end-to-end (build a
//! malformed `RuleSpec` through the real registry, capture the
//! resulting error message, assert it carries the canonical-correct
//! "hint:" line).
//!
//! Coverage:
//!
//! - Pitfall #10 — `JSONPath` dashed key after `.` segment (also inside
//!   filter expressions)
//! - Pitfall #11 — `scope_filter.has_ancestor` with path separator
//! - Pitfall #12a — `&&` / `||` / `!` in `when:` source
//! - Pitfall #12b — `iter.path.method(...)` method-call shape
//! - Pitfall #15 — `file_starts_with.prefix: ""` empty prefix

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

// --- Pitfall #10 — JSONPath dashed-key after dot ----------------------

#[test]
fn jsonpath_dashed_key_after_dot_suggests_bracket_notation() {
    let yaml = r#"
id: provider-package
kind: yaml_path_matches
level: error
paths: "providers/**/provider.yaml"
path: "$.package-name"
matches: '^apache-airflow-providers-'
"#;
    let err = build_err(yaml);
    assert!(
        err.contains("invalid JSONPath"),
        "missing base error: {err}",
    );
    assert!(
        err.contains("$['package-name']"),
        "missing bracket-notation hint: {err}",
    );
    assert!(err.contains("§ 10"), "missing pitfall reference: {err}");
}

// --- Pitfall #10 in filter context -----------------------------------
//
// (Previously framed as a separate pitfall #18 about outer-parens
// filter syntax. Investigation during Phase 4 against
// `serde_json_path` 0.7.2 showed outer-parens filters parse fine —
// the original report mis-attributed a dashed-key error to the
// parens. The real issue inside a filter is the same as outside: the
// dashed key needs bracket notation. This test exercises that case.)

#[test]
fn jsonpath_dashed_key_inside_filter_suggests_bracket_notation() {
    let yaml = r#"
id: dependabot-actions-grouped
kind: yaml_path_matches
level: error
paths: ".github/dependabot.yml"
path: "$.updates[?(@.package-ecosystem == 'github-actions')]"
matches: '.+'
"#;
    let err = build_err(yaml);
    assert!(err.contains("invalid JSONPath"), "out: {err}");
    assert!(
        err.contains("$['package-ecosystem']") || err.contains("@['package-ecosystem']"),
        "missing bracket-notation hint: {err}",
    );
    assert!(err.contains("§ 10"), "missing pitfall reference: {err}");
}

// --- Pitfall #11 — scope_filter.has_ancestor with separator -----------

#[test]
fn scope_filter_path_separator_suggests_paths_glob() {
    let yaml = r#"
id: airflow-core-no-base-operator
kind: file_content_forbidden
level: warning
paths: "**/*.py"
scope_filter:
  has_ancestor: airflow-core/pyproject.toml
pattern: 'BaseOperator'
"#;
    let err = build_err(yaml);
    assert!(
        err.contains("must be a basename"),
        "missing base error: {err}",
    );
    assert!(err.contains("paths:"), "missing paths-glob hint: {err}");
    assert!(
        err.contains("**/*.py") || err.contains("airflow-core/**/*.py"),
        "hint should illustrate the paths-glob form: {err}",
    );
}

// --- Pitfall #12a — `when:` symbolic operators -----------------------

#[test]
fn when_double_ampersand_suggests_and_keyword() {
    // `when:` parsing happens inside the engine setup, not in
    // RuleRegistry::build — so we exercise the parser directly.
    // The other Phase 4 tests use the rule-build path; this one
    // uses the public `alint_core::when::parse` entry.
    let err = alint_core::when::parse("facts.has_rust && facts.has_node")
        .expect_err("should fail to parse");
    let msg = err.to_string();
    assert!(
        msg.contains("`&&` is not a `when:` operator") && msg.contains("Use the keyword `and`"),
        "missing keyword hint: {msg}",
    );
}

#[test]
fn when_double_pipe_suggests_or_keyword() {
    let err = alint_core::when::parse("facts.has_rust || facts.has_node")
        .expect_err("should fail to parse");
    let msg = err.to_string();
    assert!(
        msg.contains("`||` is not a `when:` operator") && msg.contains("Use the keyword `or`"),
        "missing keyword hint: {msg}",
    );
}

#[test]
fn when_bang_suggests_not_keyword() {
    let err = alint_core::when::parse("!facts.has_rust").expect_err("should fail to parse");
    let msg = err.to_string();
    assert!(
        msg.contains("`!` is not a `when:` operator") && msg.contains("Use the keyword `not`"),
        "missing keyword hint: {msg}",
    );
}

// --- Pitfall #12b — iter.* method-call shape -------------------------

#[test]
fn when_method_call_shape_gets_matches_hint() {
    // `iter.path.contains("foo")` — the `(` after a dotted method
    // is what fails. Should suggest the `matches` operator.
    let err = alint_core::when::parse("iter.path.contains(\"node_modules\")")
        .expect_err("should fail to parse");
    let msg = err.to_string();
    assert!(
        msg.contains("method calls aren't supported")
            && msg.contains("matches")
            && msg.contains("§ 12b"),
        "missing method-call hint: {msg}",
    );
}

// --- Pitfall #15 — file_starts_with.prefix: "" -----------------------

#[test]
fn file_starts_with_empty_prefix_suggests_file_min_lines() {
    let yaml = r#"
id: spelling-dict-non-empty
kind: file_starts_with
level: warning
paths: "spellcheck.dic"
prefix: ""
"#;
    let err = build_err(yaml);
    assert!(
        err.contains("must not be empty"),
        "missing base error: {err}",
    );
    assert!(
        err.contains("file_min_lines") || err.contains("file_min_size"),
        "missing alternative-rule hint: {err}",
    );
}

// --- Negative tests: correct configs don't trigger hints --------------

#[test]
fn jsonpath_correct_form_passes_or_fails_without_hint() {
    // `$.package.edition` is fine — no JSONPath error at all.
    let yaml = r#"
id: cargo-edition
kind: toml_path_matches
level: error
paths: "Cargo.toml"
path: "$.package.edition"
matches: '^(2021|2024)$'
"#;
    let spec: alint_core::RuleSpec = serde_yaml_ng::from_str(yaml).unwrap();
    let registry = builtin_registry();
    // This should build successfully — no error to enrich.
    assert!(registry.build(&spec).is_ok());
}

#[test]
fn when_with_keyword_operators_parses_cleanly() {
    let result = alint_core::when::parse("facts.has_rust and facts.has_node");
    assert!(result.is_ok(), "valid `when:` should parse: {result:?}");
}
