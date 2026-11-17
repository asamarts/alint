//! End-to-end contracts for fail-loud analysis failures.

use std::path::{Path, PathBuf};
use std::process::Output;

fn alint_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn run(root: &Path, args: &[&str]) -> Output {
    crate::hermetic::command(alint_bin())
        .args(args)
        .current_dir(root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

#[test]
fn malformed_warning_input_is_a_failure_but_valid_mismatch_remains_a_warning() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::write(
        fixture.path().join(".alint.yml"),
        r"version: 1
rules:
  - id: enabled
    kind: json_path_equals
    paths: data.json
    path: $.enabled
    equals: true
    level: warning
",
    )
    .unwrap();

    std::fs::write(fixture.path().join("data.json"), "{ malformed\n").unwrap();
    let malformed = run(fixture.path(), &["--format", "json", "check"]);
    assert_eq!(
        malformed.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&malformed.stderr),
    );
    let json: serde_json::Value = serde_json::from_slice(&malformed.stdout).unwrap();
    assert_eq!(json["summary"]["total_violations"], 0);
    assert_eq!(json["summary"]["total_analysis_failures"], 1);
    assert_eq!(
        json["results"][0]["analysis_failures"][0]["kind"],
        "resource_parse"
    );
    assert_eq!(
        json["results"][0]["analysis_failures"][0]["path"],
        "data.json"
    );

    std::fs::write(fixture.path().join("data.json"), r#"{"enabled":false}"#).unwrap();
    let mismatch = run(fixture.path(), &["--format", "json", "check"]);
    assert!(
        mismatch.status.success(),
        "{}",
        String::from_utf8_lossy(&mismatch.stderr),
    );
    let json: serde_json::Value = serde_json::from_slice(&mismatch.stdout).unwrap();
    assert_eq!(json["summary"]["total_violations"], 1);
    assert_eq!(json["summary"]["total_analysis_failures"], 0);
    assert_eq!(
        json["results"][0]["violations"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        json["results"][0]["analysis_failures"]
            .as_array()
            .unwrap()
            .as_slice(),
        &[] as &[serde_json::Value]
    );
}
