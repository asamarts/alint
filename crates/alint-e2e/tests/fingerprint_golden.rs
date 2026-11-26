//! W6 baseline-identity gate: recompute every policy-finding fingerprint over the
//! scenario corpus and compare it with the checked-in golden file.
//!
//! On Linux every scenario runs and must match the complete golden snapshot.
//! On Windows and macOS, scenarios that `alint-testkit` skips for the platform
//! (Unix permissions, case-sensitive filesystem fixtures, POSIX shell plugins)
//! are omitted and every portable scenario must produce the exact same
//! `(rule_id, path, fingerprint)` entries as Linux (#579). Refresh after an
//! intentional fingerprint migration (on Linux) with:
//!
//! `UPDATE_SNAPSHOTS=1 cargo test -p alint-e2e --test it -- fingerprint_golden::`

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use alint_testkit::strategies::with_steps;
use alint_testkit::{PolicyTest, Runner, RunnerOptions, TestStatus, TestStep, TreeExpectationMode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Serialize, Deserialize)]
struct GoldenFingerprint {
    scenario: String,
    rule_id: String,
    path: Option<String>,
    fingerprint: String,
}

fn yml_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir)
            .unwrap_or_else(|error| panic!("read scenario directory {}: {error}", dir.display()))
        {
            let path = entry.expect("read scenario directory entry").path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("yml") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn collect_policy_test(
    corpus_root: &Path,
    path: &Path,
    records: &mut Vec<GoldenFingerprint>,
    skipped: &mut BTreeSet<String>,
) {
    let yaml = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read policy test {}: {error}", path.display()));
    let policy_test = PolicyTest::from_yaml(&yaml)
        .unwrap_or_else(|error| panic!("parse policy test {}: {error}", path.display()));

    // Evaluate every corpus fixture from its declared initial tree, including
    // fix-only fixtures. The public runner supplies deterministic Git dates.
    let mut policy_test = with_steps(policy_test, vec![TestStep::Check]);
    policy_test.expect_tree = None;
    policy_test.expect_tree_mode = TreeExpectationMode::default();
    policy_test.snapshot = None;
    let runner = Runner::new(RunnerOptions {
        allow_spawn: true,
        allow_host_keyring: true,
        update_snapshots: false,
    });
    let run = runner
        .run(&policy_test, path)
        .unwrap_or_else(|error| panic!("run policy test {}: {error}", path.display()));
    let scenario_path = slash(
        path.strip_prefix(corpus_root)
            .expect("policy-test path is below corpus root"),
    );
    if run.status == TestStatus::Skipped {
        const PLATFORM_SKIP_TAGS: &[&str] =
            &["linux-only", "unix-only", "requires-case-sensitive-fs"];
        assert!(
            policy_test
                .tags
                .iter()
                .any(|tag| PLATFORM_SKIP_TAGS.contains(&tag.as_str())),
            "portable scenario {scenario_path} unexpectedly returned Skipped (tags: {:?})",
            policy_test.tags,
        );
        skipped.insert(scenario_path);
        return;
    }
    let [observation] = run.observations.as_slice() else {
        panic!(
            "forced check did not return one observation: {}",
            path.display()
        );
    };
    for violation in &observation.violations {
        records.push(GoldenFingerprint {
            scenario: scenario_path.clone(),
            rule_id: violation.rule.clone(),
            path: violation.path.clone(),
            fingerprint: violation.fingerprint.clone(),
        });
    }
}

#[test]
fn policy_test_fingerprints_match_golden() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let scenarios_dir = manifest.join("scenarios");
    let ruleset_tests_root = manifest
        .parent()
        .expect("crates directory")
        .join("alint-dsl/ruleset-tests");
    let mut records = Vec::new();
    let mut skipped = BTreeSet::new();
    for path in yml_files(&scenarios_dir) {
        collect_policy_test(&scenarios_dir, &path, &mut records, &mut skipped);
    }
    let mut rev_dirs: Vec<PathBuf> = std::fs::read_dir(&ruleset_tests_root)
        .expect("read ruleset-tests directory")
        .filter_map(|e| e.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect();
    rev_dirs.sort();
    for rev_dir in rev_dirs {
        // Keep v1 scenario keys strip-prefixed to preserve existing golden
        // snapshot entries; namespace newer revisions (v2, v3, ...) with
        // `<rev>/...` to avoid cross-revision key collisions.
        let base = if rev_dir.file_name().and_then(|n| n.to_str()) == Some("v1") {
            rev_dir.as_path()
        } else {
            ruleset_tests_root.as_path()
        };
        for path in yml_files(&rev_dir) {
            collect_policy_test(base, &path, &mut records, &mut skipped);
        }
    }
    records.sort();
    assert!(
        records.len() >= 550,
        "fingerprint gate collected only {} findings; the policy-test run is probably incomplete",
        records.len(),
    );
    assert!(
        records.iter().any(|record| {
            record.scenario
                == "check/cross_file/for_each_nested_twice_message_token_spelled_in_a_name.yml"
        }),
        "cross-platform path-token scenario (#579) must be covered by the golden gate",
    );

    let golden = manifest.join("tests/snapshots/scenario-fingerprints.json");
    if cfg!(target_os = "linux") {
        assert!(
            skipped.is_empty(),
            "scenarios unexpectedly skipped on Linux: {skipped:?}",
        );
        let mut actual = serde_json::to_string_pretty(&records).expect("serialize fingerprints");
        actual.push('\n');
        if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
            std::fs::create_dir_all(golden.parent().expect("snapshot parent"))
                .expect("create snapshot directory");
            std::fs::write(&golden, &actual).expect("write fingerprint snapshot");
            eprintln!("wrote {}", golden.display());
            return;
        }

        let expected = std::fs::read_to_string(&golden).unwrap_or_else(|error| {
            panic!(
                "read fingerprint snapshot {}: {error}; regenerate with \
                 UPDATE_SNAPSHOTS=1 cargo test -p alint-e2e --test it -- fingerprint_golden::",
                golden.display(),
            )
        });
        assert_eq!(
            actual, expected,
            "scenario fingerprints changed; related locations and presentation metadata MUST NOT alter \
             baseline identity. If this is an intentional baseline migration, review the complete diff \
             and refresh with UPDATE_SNAPSHOTS=1 cargo test -p alint-e2e --test it -- \
             fingerprint_golden::",
        );
    } else {
        let expected_json = std::fs::read_to_string(&golden).unwrap_or_else(|error| {
            panic!("read fingerprint snapshot {}: {error}", golden.display())
        });
        let mut expected: Vec<GoldenFingerprint> =
            serde_json::from_str(&expected_json).expect("parse golden fingerprint snapshot");
        expected.retain(|record| !skipped.contains(&record.scenario));
        expected.sort();
        assert_eq!(
            records, expected,
            "cross-platform scenario fingerprints diverged from Linux golden snapshot (#579)",
        );
    }
}
