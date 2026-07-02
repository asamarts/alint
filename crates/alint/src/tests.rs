//! Unit tests for `main` (the CLI binary) — the `facts` subcommand's
//! renderers, the exit-code classifier, and friends. Extracted from
//! `main.rs` to keep it under the self-lint's file-length threshold. The
//! full evaluation pipelines are exercised in the `trycmd` CLI snapshot
//! tests under `tests/cli/`.

use super::*;
use alint_core::{FactKind, FactSpec, FactValue, FactValues, facts::OneOrMany};
use alint_output::Format;

#[test]
fn error_is_internal_classifies_the_exit_code() {
    // M11: an Internal error (alint bug) → exit 3; a config/usage error
    // (Other, or a plain anyhow message) → exit 2.
    let internal = anyhow::Error::new(alint_core::Error::internal("boom"));
    assert!(error_is_internal(&internal));

    let config = anyhow::Error::new(alint_core::Error::Other("bad config".into()));
    assert!(!error_is_internal(&config));

    let usage = anyhow::anyhow!("missing --config");
    assert!(!error_is_internal(&usage));

    // The chain is searched, so a `.context(...)`-wrapped internal error is
    // still classified as internal (exit 3).
    let wrapped =
        anyhow::Error::new(alint_core::Error::internal("boom")).context("while loading config");
    assert!(error_is_internal(&wrapped));
}

#[test]
fn url_encode_passes_through_unreserved_chars() {
    assert_eq!(url_encode("abcXYZ012-_.~"), "abcXYZ012-_.~");
}

#[test]
fn url_encode_percent_encodes_reserved_and_unsafe() {
    assert_eq!(url_encode(" "), "%20");
    assert_eq!(url_encode("foo bar"), "foo%20bar");
    assert_eq!(url_encode("a&b=c"), "a%26b%3Dc");
    assert_eq!(url_encode("/?:@!$"), "%2F%3F%3A%40%21%24");
}

#[test]
fn url_encode_handles_unicode() {
    // Multi-byte UTF-8 sequences each percent-encode their bytes.
    // "ñ" is 0xC3 0xB1.
    assert_eq!(url_encode("ñ"), "%C3%B1");
}

fn fact_spec(id: &str, kind: FactKind) -> FactSpec {
    FactSpec {
        id: id.to_string(),
        kind,
    }
}

fn any_file_exists_kind(glob: &str) -> FactKind {
    FactKind::AnyFileExists {
        any_file_exists: OneOrMany::One(glob.to_string()),
    }
}

fn count_files_kind(glob: &str) -> FactKind {
    FactKind::CountFiles {
        count_files: glob.to_string(),
    }
}

fn git_branch_kind() -> FactKind {
    FactKind::GitBranch {
        git_branch: alint_core::facts::GitBranchFact {},
    }
}

fn render_to_string<F>(render: F) -> String
where
    F: FnOnce(&mut dyn Write) -> Result<()>,
{
    let mut buf = Vec::new();
    render(&mut buf).expect("render should succeed");
    String::from_utf8(buf).expect("output should be UTF-8")
}

#[test]
fn fact_kind_name_covers_every_variant() {
    assert_eq!(any_file_exists_kind("X").name(), "any_file_exists");
    assert_eq!(count_files_kind("**/*.rs").name(), "count_files");
    assert_eq!(git_branch_kind().name(), "git_branch");
    assert_eq!(
        FactKind::AllFilesExist {
            all_files_exist: OneOrMany::One("X".into()),
        }
        .name(),
        "all_files_exist"
    );
    assert_eq!(
        FactKind::FileContentMatches {
            file_content_matches: alint_core::facts::FileContentMatchesFact {
                paths: OneOrMany::One("X".into()),
                pattern: ".".into(),
            },
        }
        .name(),
        "file_content_matches"
    );
    assert_eq!(
        FactKind::Custom {
            custom: alint_core::facts::CustomFact { argv: vec![] },
        }
        .name(),
        "custom"
    );
}

#[test]
fn fact_value_display_renders_each_variant() {
    assert_eq!(fact_value_display(&FactValue::Bool(true)), "true");
    assert_eq!(fact_value_display(&FactValue::Bool(false)), "false");
    assert_eq!(fact_value_display(&FactValue::Int(0)), "0");
    assert_eq!(fact_value_display(&FactValue::Int(42)), "42");
    assert_eq!(fact_value_display(&FactValue::Int(-1)), "-1");
    // Strings are quoted so leading/trailing whitespace is visible
    // and empty strings don't render as blank columns.
    assert_eq!(
        fact_value_display(&FactValue::String("main".into())),
        "\"main\""
    );
    assert_eq!(
        fact_value_display(&FactValue::String(String::new())),
        "\"\""
    );
}

#[test]
fn fact_value_json_preserves_native_types() {
    assert_eq!(
        fact_value_json(&FactValue::Bool(true)),
        serde_json::json!(true)
    );
    assert_eq!(fact_value_json(&FactValue::Int(42)), serde_json::json!(42));
    assert_eq!(
        fact_value_json(&FactValue::String("main".into())),
        serde_json::json!("main")
    );
}

#[test]
fn human_render_aligns_columns_and_covers_each_value_kind() {
    let facts = vec![
        fact_spec("is_python", any_file_exists_kind("pyproject.toml")),
        fact_spec("n_rs_files", count_files_kind("**/*.rs")),
        fact_spec("branch", git_branch_kind()),
    ];
    let mut values = FactValues::new();
    values.insert("is_python".into(), FactValue::Bool(true));
    values.insert("n_rs_files".into(), FactValue::Int(42));
    values.insert("branch".into(), FactValue::String("main".into()));

    let out = render_to_string(|w| render_facts_human(&facts, &values, w));

    // Every fact id appears once, values render natively, and
    // the kind column sits between them.
    assert!(out.contains("is_python"), "output: {out}");
    assert!(out.contains("n_rs_files"), "output: {out}");
    assert!(out.contains("branch"), "output: {out}");
    assert!(out.contains("true"));
    assert!(out.contains("42"));
    assert!(out.contains("\"main\""));
    assert!(out.contains("any_file_exists"));
    assert!(out.contains("count_files"));
    assert!(out.contains("git_branch"));
    // One line per fact.
    assert_eq!(out.lines().count(), 3);
}

#[test]
fn human_render_reports_no_facts_message() {
    let out = render_to_string(|w| render_facts_human(&[], &FactValues::new(), w));
    assert_eq!(out.trim(), "(no facts declared in config)");
}

#[test]
fn human_render_marks_unresolved_facts_when_value_is_missing() {
    // Simulates a case where `evaluate_facts` was only partially
    // populated — shouldn't crash, should surface the gap.
    let facts = vec![fact_spec("orphan", any_file_exists_kind("X"))];
    let out = render_to_string(|w| render_facts_human(&facts, &FactValues::new(), w));
    assert!(out.contains("(unresolved)"), "output: {out}");
}

#[test]
fn json_render_emits_versioned_document_shape() {
    let facts = vec![
        fact_spec("is_go", any_file_exists_kind("go.mod")),
        fact_spec("n_py", count_files_kind("**/*.py")),
    ];
    let mut values = FactValues::new();
    values.insert("is_go".into(), FactValue::Bool(false));
    values.insert("n_py".into(), FactValue::Int(5));

    let out = render_to_string(|w| render_facts_json(&facts, &values, w));
    let parsed: serde_json::Value =
        serde_json::from_str(&out).expect("render should emit valid JSON");

    let arr = parsed
        .get("facts")
        .and_then(|v| v.as_array())
        .expect("facts: [...]");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["id"], serde_json::json!("is_go"));
    assert_eq!(arr[0]["kind"], serde_json::json!("any_file_exists"));
    assert_eq!(arr[0]["value"], serde_json::json!(false));
    assert_eq!(arr[1]["id"], serde_json::json!("n_py"));
    assert_eq!(arr[1]["kind"], serde_json::json!("count_files"));
    assert_eq!(arr[1]["value"], serde_json::json!(5));
}

#[test]
fn json_render_empty_list_is_empty_array_not_null() {
    let out = render_to_string(|w| render_facts_json(&[], &FactValues::new(), w));
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(parsed["facts"], serde_json::json!([]));
}

#[test]
fn json_render_missing_value_becomes_null() {
    let facts = vec![fact_spec("orphan", any_file_exists_kind("X"))];
    let out = render_to_string(|w| render_facts_json(&facts, &FactValues::new(), w));
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(parsed["facts"][0]["value"], serde_json::Value::Null);
}

#[test]
fn render_facts_dispatches_on_format() {
    let facts = vec![fact_spec("is_py", any_file_exists_kind("py"))];
    let mut values = FactValues::new();
    values.insert("is_py".into(), FactValue::Bool(true));

    let human_out = render_to_string(|w| render_facts(&facts, &values, Format::Human, w));
    assert!(human_out.contains("is_py"));
    assert!(!human_out.contains("\"facts\""));

    let json_out = render_to_string(|w| render_facts(&facts, &values, Format::Json, w));
    assert!(json_out.contains("\"facts\""));

    // `sarif` and `github` fall back to the human renderer
    // rather than emitting a confusing empty document.
    let sarif_out = render_to_string(|w| render_facts(&facts, &values, Format::Sarif, w));
    assert!(sarif_out.contains("is_py"));
    assert!(!sarif_out.contains("\"facts\""));
}
