//! ADR-0030 decision 1: the fingerprint of a finding whose path alint hides
//! (a path chosen by a field read from the environment) leaves the path out,
//! in the baseline file, SARIF and GitLab output alike. v0.22 also matched the
//! v0.21 form, which hashed the path; v0.23 does not, so a v0.21 baseline file
//! needs regenerating.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Output;

use alint_core::baseline::{Baseline, BaselineEntry, violation_fingerprint};

const RULE: &str = "no-ws";
const CONFIG: &str = "version: 1\nrules:\n  - id: no-ws\n    kind: no_trailing_whitespace\n    \
     paths: \"{{env.GLOB}}\"\n    level: error\n    fix: { file_trim_trailing_whitespace: {} }\n";

/// A tree with one finding in each of `files`.
fn tree(files: &[&str]) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join(".alint.yml"), CONFIG).unwrap();
    for file in files {
        std::fs::write(tmp.path().join(file), "a  \nb\n").unwrap();
    }
    tmp
}

fn alint(root: &Path, args: &[&str]) -> Output {
    crate::hermetic::command(PathBuf::from(env!("CARGO_BIN_EXE_alint")))
        .args(args)
        .current_dir(root)
        .env("GLOB", "*.txt")
        .output()
        .expect("spawn alint")
}

fn entries(root: &Path, file: &str) -> Vec<BaselineEntry> {
    let text = std::fs::read_to_string(root.join(file)).unwrap();
    Baseline::load(&text).unwrap().entries
}

/// The v0.21 fingerprint of a hidden-path finding: the path, hashed.
fn v021(path: &str) -> String {
    violation_fingerprint(
        RULE,
        Some(Path::new(path)),
        None,
        Some("sensitive"),
        "",
        None,
    )
}

fn write_v021_baseline(root: &Path, paths: &[&str]) {
    let baseline = Baseline {
        alint_version: Some("0.21.2".into()),
        entries: paths
            .iter()
            .map(|path| BaselineEntry {
                rule_id: RULE.into(),
                path: Some("<redacted>".into()),
                fingerprint: v021(path),
                count: 1,
                message: None,
                paths: Vec::new(),
            })
            .collect(),
    };
    std::fs::write(root.join("old.json"), baseline.to_jsonl()).unwrap();
}

fn json_strings(output: &Output, pointer: &str, each: &str) -> BTreeSet<String> {
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON output");
    let items = value
        .pointer(pointer)
        .and_then(serde_json::Value::as_array)
        .expect("an array");
    items
        .iter()
        .map(|item| {
            item.pointer(each)
                .and_then(|v| v.as_str())
                .unwrap()
                .to_string()
        })
        .collect()
}

#[test]
fn the_written_fingerprint_says_nothing_about_the_path() {
    let one = tree(&["alpha.txt", "beta.txt"]);
    let two = tree(&["gamma.txt", "delta.txt"]);
    let mut fingerprints = Vec::new();
    for root in [one.path(), two.path()] {
        let out = alint(root, &["baseline", "--output", "bl.json", "."]);
        assert!(out.status.success(), "{out:?}");
        let written = entries(root, "bl.json");
        assert_eq!(written.len(), 1, "one entry per rule: {written:?}");
        assert_eq!(written[0].count, 2);
        assert_eq!(written[0].path.as_deref(), Some("<redacted>"));
        assert_eq!(
            written[0].paths,
            Vec::<String>::new(),
            "hidden-path baseline entry must never store paths (ADR-0030/ADR-0049)"
        );
        fingerprints.push(written[0].fingerprint.clone());

        let check = alint(root, &["check", "--baseline", "bl.json", "."]);
        assert_eq!(check.status.code(), Some(0), "{check:?}");
    }
    assert_eq!(
        fingerprints[0], fingerprints[1],
        "other names, same fingerprint"
    );
    assert_ne!(fingerprints[0], v021("alpha.txt"));

    // SARIF and GitLab carry the same path-free identities in both trees: the
    // first finding the shared fingerprint, the repeat one derived from it and
    // its occurrence, so each stays its own alert.
    let sarif = |root| {
        json_strings(
            &alint(root, &["check", "--format", "sarif", "."]),
            "/runs/0/results",
            "/partialFingerprints/alint~1v1",
        )
    };
    assert_eq!(
        sarif(one.path()).len(),
        2,
        "SARIF keeps its fingerprints unique"
    );
    assert!(sarif(one.path()).contains(&fingerprints[0]));
    assert_eq!(sarif(one.path()), sarif(two.path()));
    let gitlab = |root| {
        json_strings(
            &alint(root, &["check", "--format", "gitlab", "."]),
            "",
            "/fingerprint",
        )
    };
    assert_eq!(
        gitlab(one.path()).len(),
        2,
        "GitLab keeps its fingerprints unique"
    );
    assert_eq!(gitlab(one.path()), gitlab(two.path()));
    assert_eq!(gitlab(one.path()), sarif(one.path()));
}

/// A new hidden-path finding beside a baselined one: both show as
/// `<redacted>` at the same line, so only being baselined tells them apart in
/// SARIF, and the baselined one keeps the fingerprint the baseline gives it.
#[test]
fn a_baselined_hidden_finding_keeps_its_fingerprint_beside_a_new_one() {
    let tmp = tree(&["alpha.txt"]);
    let root = tmp.path();
    let out = alint(root, &["baseline", "--output", "bl.json", "."]);
    assert!(out.status.success(), "{out:?}");
    let written = entries(root, "bl.json");
    assert_eq!(written.len(), 1, "{written:?}");
    std::fs::write(root.join("beta.txt"), "a  \nb\n").unwrap();
    let out = alint(
        root,
        &["check", "--baseline", "bl.json", "--format", "sarif", "."],
    );
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let sarif: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let results: Vec<(String, String)> = sarif["runs"][0]["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|result| {
            (
                result["baselineState"].as_str().unwrap().to_string(),
                result["partialFingerprints"]["alint/v1"]
                    .as_str()
                    .unwrap()
                    .to_string(),
            )
        })
        .collect();
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(
        results.contains(&("unchanged".to_string(), written[0].fingerprint.clone())),
        "{results:?}"
    );
    assert!(
        results
            .iter()
            .any(|(state, fingerprint)| state == "new" && *fingerprint != written[0].fingerprint),
        "{results:?}"
    );
}

#[test]
fn a_v021_baseline_no_longer_matches() {
    let root = tree(&["alpha.txt", "beta.txt"]);
    let root = root.path();
    write_v021_baseline(root, &["alpha.txt", "beta.txt"]);

    // Both findings are new, and both old entries are stale.
    // Because the baseline was written before 0.22 and has unmatched
    // hidden-path entries, alint prints a one-line regeneration hint (#547).
    let check = alint(root, &["check", "--baseline", "old.json", "."]);
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(stderr.contains("2 baseline entr"), "{stderr}");
    assert!(stderr.contains("no longer fire"), "{stderr}");
    assert!(
        stderr.contains("before 0.22") && stderr.contains("alint baseline"),
        "expected pre-0.22 regeneration hint on stderr: {stderr}"
    );
    assert!(
        !stderr.contains("baselined violation(s) suppressed"),
        "{stderr}"
    );

    // Regenerating counts them as new debt, so it needs --accept-new, and then
    // writes the current form.
    let regenerate = alint(root, &["baseline", "--output", "old.json", "."]);
    assert!(!regenerate.status.success(), "{regenerate:?}");
    assert!(
        String::from_utf8_lossy(&regenerate.stderr).contains("2 new violation(s) (+2 / -2)"),
        "{regenerate:?}"
    );
    let regenerate = alint(
        root,
        &["baseline", "--accept-new", "--output", "old.json", "."],
    );
    assert!(regenerate.status.success(), "{regenerate:?}");
    let written = entries(root, "old.json");
    assert_eq!(written.len(), 1, "{written:?}");
    assert_eq!(written[0].count, 2);
    assert_ne!(written[0].fingerprint, v021("alpha.txt"));
    let check = alint(root, &["check", "--baseline", "old.json", "."]);
    assert_eq!(check.status.code(), Some(0), "{check:?}");
}

/// `fix --baseline` fixes what a v0.21 baseline file recorded, as new findings.
#[test]
fn fix_with_a_v021_baseline_fixes_the_findings() {
    let root = tree(&["alpha.txt", "beta.txt"]);
    let root = root.path();
    write_v021_baseline(root, &["alpha.txt", "beta.txt"]);
    let fix = alint(root, &["fix", "--baseline", "old.json", "."]);
    assert_eq!(fix.status.code(), Some(0), "{fix:?}");
    let stdout = String::from_utf8_lossy(&fix.stdout);
    assert!(stdout.contains("2 applied, 0 skipped"), "{stdout}");
    assert_eq!(
        std::fs::read_to_string(root.join("alpha.txt")).unwrap(),
        "a\nb\n"
    );
}

/// The fix loop keys findings across passes by a fingerprint that keeps the
/// hidden path: b.txt's finding appears only in the second pass, after a.txt's
/// fix in the first, and must not be taken for it.
#[test]
fn fix_still_tells_hidden_paths_apart() {
    let root = tree(&["a.txt"]);
    let root = root.path();
    let create = "  - id: need-b\n    kind: file_exists\n    paths: b.txt\n    level: error\n    \
                  fix: { file_create: { content: \"b  \\n\" } }\n";
    std::fs::write(root.join(".alint.yml"), format!("{CONFIG}{create}")).unwrap();
    let fix = alint(root, &["fix", "."]);
    assert_eq!(fix.status.code(), Some(0), "{fix:?}");
    let stdout = String::from_utf8_lossy(&fix.stdout);
    assert!(stdout.contains("3 applied, 0 skipped"), "{stdout}");
    for (file, content) in [("a.txt", "a\nb\n"), ("b.txt", "b\n")] {
        assert_eq!(
            std::fs::read_to_string(root.join(file)).unwrap(),
            content,
            "{file}"
        );
    }
}

/// A current-version baseline with unmatched hidden-path entries (for example
/// after the findings were fixed) does NOT print the pre-0.22 regeneration
/// hint (#547).
#[test]
fn current_version_baseline_prints_no_pre_022_hint() {
    let tmp = tree(&["alpha.txt"]);
    let root = tmp.path();
    let out = alint(root, &["baseline", "--output", "cur.json", "."]);
    assert!(out.status.success(), "{out:?}");

    // Fix the finding so the hidden-path entry becomes stale/unmatched.
    std::fs::write(root.join("alpha.txt"), "a\nb\n").unwrap();
    let check = alint(root, &["check", "--baseline", "cur.json", "."]);
    assert_eq!(check.status.code(), Some(0), "{check:?}");
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(
        stderr.contains("1 baseline entry no longer fires"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("before 0.22"),
        "current baseline must not show pre-0.22 hint: {stderr}"
    );
}
