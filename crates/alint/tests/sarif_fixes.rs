//! End-to-end gate: SARIF `result.fixes[]` for fixable *located* findings.
//!
//! `alint check --format sarif` attaches the concrete edit a fix would make
//! (source region + replacement text) to each fixable finding, so a SARIF
//! consumer (GitHub Code Scanning's "fix" suggestions) can preview/apply it
//! without running `alint fix`. These drive the real binary end-to-end:
//! `engine.run` -> `attach_proposed_edits` -> `write_sarif`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn alint_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn check_sarif(dir: &Path) -> serde_json::Value {
    let out = Command::new(alint_bin())
        .args(["--format", "sarif", "check", "."])
        .current_dir(dir)
        .output()
        .expect("spawn alint");
    // A live finding means a non-zero (findings) exit; stdout carries the SARIF.
    assert_eq!(
        out.status.code(),
        Some(1),
        "expected a findings exit; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid SARIF JSON ({e}):\n{}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

/// A `replace` fix on a `json_path_matches` rule (a located fixer) renders a
/// `result.fixes[]` whose `deletedRegion` is a 1-based line/column span and
/// whose `insertedContent` is the replacement. Only the *violating* node
/// contributes a replacement — an already-compliant sibling does not.
#[test]
fn sarif_carries_a_located_replace_fix_with_line_column_region() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        concat!(
            "version: 1\n",
            "rules:\n",
            "  - id: v-pins\n",
            "    kind: json_path_matches\n",
            "    paths: \"**/*.json\"\n",
            "    path: \"$.deps.*\"\n",
            "    matches: \"^v\"\n",
            "    level: error\n",
            "    fix:\n",
            "      replace:\n",
            "        pattern: \"^\"\n",
            "        replacement: \"v\"\n",
            "        applicability: safe\n",
        ),
    )
    .unwrap();
    // `ok` already matches `^v`; `bad` does not, on line 4.
    std::fs::write(
        dir.path().join("app.json"),
        "{\n  \"deps\": {\n    \"ok\": \"v9\",\n    \"bad\": \"2.0\"\n  }\n}\n",
    )
    .unwrap();

    let sarif = check_sarif(dir.path());
    let result = &sarif["runs"][0]["results"][0];
    assert_eq!(result["ruleId"], "v-pins");

    let changes = &result["fixes"][0]["artifactChanges"];
    assert_eq!(changes[0]["artifactLocation"]["uri"], "app.json");
    let replacements = changes[0]["replacements"].as_array().unwrap();
    // Exactly one replacement: the compliant `ok` node is skipped, not rewritten.
    assert_eq!(replacements.len(), 1);

    let region = &replacements[0]["deletedRegion"];
    assert_eq!(region["startLine"], 4);
    assert_eq!(region["startColumn"], 12);
    assert_eq!(region["endLine"], 4);
    assert_eq!(region["endColumn"], 17);
    assert_eq!(replacements[0]["insertedContent"]["text"], "\"v2.0\"");
}

/// A finding whose rule has no fixer carries no `fixes` key — the ordinary
/// SARIF shape is unchanged for non-fixable findings.
#[test]
fn sarif_omits_fixes_for_a_finding_with_no_fixer() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        concat!(
            "version: 1\n",
            "rules:\n",
            "  - id: no-secrets\n",
            "    kind: json_path_absent\n",
            "    paths: \"**/*.json\"\n",
            "    path: \"$.secret\"\n",
            "    level: error\n",
        ),
    )
    .unwrap();
    std::fs::write(dir.path().join("app.json"), "{\n  \"secret\": \"x\"\n}\n").unwrap();

    let sarif = check_sarif(dir.path());
    let result = &sarif["runs"][0]["results"][0];
    assert_eq!(result["ruleId"], "no-secrets");
    assert!(
        result["fixes"].is_null(),
        "a no-fixer finding must not carry a fixes key"
    );
}
