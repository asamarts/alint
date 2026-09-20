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

/// A whole-file normalizer (`no_trailing_whitespace` -> the
/// `file_trim_trailing_whitespace` fixer, which rewrites the whole file via
/// `SetContent`) renders a *minimal* changed span (not a full-artifact rewrite),
/// so independent same-file fixes compose. Trimming `a   \nb\n` -> `a\nb\n` is a
/// pure deletion of the three spaces on line 1.
#[test]
fn sarif_carries_a_whole_file_fix_as_a_minimal_span() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        concat!(
            "version: 1\n",
            "rules:\n",
            "  - id: no-ws\n",
            "    kind: no_trailing_whitespace\n",
            "    paths: \"**/*.txt\"\n",
            "    level: error\n",
            "    fix:\n",
            "      file_trim_trailing_whitespace: {}\n",
        ),
    )
    .unwrap();
    // Line 1 has three trailing spaces (columns 2-4), deleted.
    std::fs::write(dir.path().join("bad.txt"), "a   \nb\n").unwrap();

    let sarif = check_sarif(dir.path());
    let result = &sarif["runs"][0]["results"][0];
    assert_eq!(result["ruleId"], "no-ws");
    let repl = &result["fixes"][0]["artifactChanges"][0]["replacements"][0];
    let region = &repl["deletedRegion"];
    assert_eq!(region["startLine"], 1);
    assert_eq!(region["startColumn"], 2);
    assert_eq!(region["endLine"], 1);
    assert_eq!(region["endColumn"], 5);
    // Pure deletion: no insertedContent.
    assert!(repl["insertedContent"].is_null());
}

/// Fidelity gate (audit HIGH): SARIF must NOT advertise an edit that `alint fix`
/// demotes and never writes. A `remove_value` batch that can only partially
/// remove is refused all-or-nothing (the `Absent` verifier fails), so `fix`
/// writes nothing — and SARIF must therefore carry no `fixes`, not a partial
/// (here: secret-leaking) removal.
#[test]
fn sarif_omits_a_fix_the_pipeline_would_demote() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        concat!(
            "version: 1\n",
            "rules:\n",
            "  - id: no-secret\n",
            "    kind: hcl_path_absent\n",
            "    paths: \"**/*.tf\"\n",
            "    path: \"$..secret\"\n",
            "    level: error\n",
            "    fix: { remove_value: { applicability: safe } }\n",
        ),
    )
    .unwrap();
    // A top-level secret + two block secrets: the block members can't be removed,
    // so the whole batch demotes (all-or-nothing) and `fix` writes nothing.
    std::fs::write(
        dir.path().join("main.tf"),
        "secret = \"top\"\nitem {\n  secret = \"a\"\n}\nitem {\n  secret = \"b\"\n}\n",
    )
    .unwrap();

    let sarif = check_sarif(dir.path());
    let result = &sarif["runs"][0]["results"][0];
    assert_eq!(result["ruleId"], "no-secret");
    assert!(
        result["fixes"].is_null(),
        "a demoted (never-written) fix must not be advertised: {result}"
    );
}

/// Fidelity gate (audit HIGH): overlapping candidate edits (nested `$..a`) must
/// not all be advertised (that is invalid SARIF and would corrupt the file).
/// Only the edit the pipeline's overlap-skip keeps is emitted, and applying it
/// reproduces `alint fix`'s result.
#[test]
fn sarif_advertises_only_the_overlap_survivor() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        concat!(
            "version: 1\n",
            "rules:\n",
            "  - id: no-a\n",
            "    kind: xml_path_absent\n",
            "    paths: \"**/*.xml\"\n",
            "    path: \"$..a\"\n",
            "    level: error\n",
            "    fix: { remove_value: { applicability: safe } }\n",
        ),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("n.xml"),
        "<root>\n  <a>\n    <a>x</a>\n  </a>\n</root>\n",
    )
    .unwrap();

    let sarif = check_sarif(dir.path());
    let replacements =
        sarif["runs"][0]["results"][0]["fixes"][0]["artifactChanges"][0]["replacements"]
            .as_array()
            .unwrap();
    // Exactly one edit (the outer <a>), not the two overlapping candidates.
    assert_eq!(replacements.len(), 1);
    let region = &replacements[0]["deletedRegion"];
    assert_eq!(region["startLine"], 2);
    assert_eq!(region["startColumn"], 1);
    assert_eq!(region["endLine"], 4);
    assert_eq!(region["endColumn"], 8);
}

/// A create fixer (`file_exists` -> `file_create`) surfaces a `CreateFile` fix:
/// an empty region at `(1,1)` with the new file's content. The violation is
/// path-less, so the target comes from the fixer's own edit.
#[test]
fn sarif_carries_a_create_file_fix() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        concat!(
            "version: 1\n",
            "rules:\n",
            "  - id: need-license\n",
            "    kind: file_exists\n",
            "    paths: \"LICENSE\"\n",
            "    level: error\n",
            "    fix: { file_create: { content: \"MIT\\n\" } }\n",
        ),
    )
    .unwrap();

    let sarif = check_sarif(dir.path());
    let change = &sarif["runs"][0]["results"][0]["fixes"][0]["artifactChanges"][0];
    assert_eq!(change["artifactLocation"]["uri"], "LICENSE");
    let repl = &change["replacements"][0];
    assert_eq!(repl["deletedRegion"]["startLine"], 1);
    assert_eq!(repl["deletedRegion"]["startColumn"], 1);
    assert_eq!(repl["deletedRegion"]["endLine"], 1);
    assert_eq!(repl["deletedRegion"]["endColumn"], 1);
    assert_eq!(repl["insertedContent"]["text"], "MIT\n");
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
