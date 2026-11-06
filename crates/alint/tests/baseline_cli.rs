//! End-to-end tests for `alint baseline` + `check --baseline`.
//!
//! Drives the built binary against throwaway repos to exercise the full
//! grandfathering flow: snapshot the current violations, then gate on
//! the delta. The matching/fingerprint internals are unit-tested in
//! `alint_core::baseline`; this asserts the CLI wiring and exit codes.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn alint() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

/// A repo with two error rules (a missing-final-newline check and a
/// forbidden-`TODO` check) and two violating files.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(".alint.yml"),
        "version: 1\n\
         rules:\n\
         \x20 - id: needs-newline\n\
         \x20   kind: final_newline\n\
         \x20   paths: [\"**/*.txt\"]\n\
         \x20   level: error\n\
         \x20 - id: no-todo\n\
         \x20   kind: file_content_forbidden\n\
         \x20   paths: [\"**/*.txt\"]\n\
         \x20   pattern: \"TODO\"\n\
         \x20   level: error\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("a.txt"), "no newline").unwrap();
    std::fs::write(dir.path().join("b.txt"), "has a TODO\nmore\n").unwrap();
    dir
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(alint())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn alint")
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap_or(-1)
}

#[test]
fn baseline_rejects_a_non_human_format() {
    // `baseline` writes the baseline file plus a human summary and has no
    // machine-readable report output, so a non-default `--format` is rejected
    // rather than silently ignored.
    let d = fixture();
    let out = run(d.path(), &["baseline", "--format", "json"]);
    assert_eq!(code(&out), 2, "non-human --format is rejected");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("does not support"), "stderr: {stderr}");
    // The default (human) format still writes successfully.
    assert_eq!(code(&run(d.path(), &["baseline"])), 0, "human still works");
}

#[test]
fn baseline_then_check_grandfathers_existing_and_gates_on_new() {
    let d = fixture();
    let root = d.path();

    // Plain check fails on the two pre-existing errors.
    assert_eq!(
        code(&run(root, &["check"])),
        1,
        "two errors before baseline"
    );

    // Snapshot them.
    let out = run(root, &["baseline"]);
    assert_eq!(code(&out), 0, "baseline writes successfully");
    let baseline_path = root.join(".alint-baseline.json");
    assert!(baseline_path.is_file(), "baseline file created");
    let text = std::fs::read_to_string(&baseline_path).unwrap();
    // Header + 2 entries, and the advisory message is present for review.
    assert!(
        text.lines()
            .next()
            .unwrap()
            .contains("\"schema_version\":1")
    );
    assert_eq!(
        text.lines()
            .filter(|l| l.contains("\"fingerprint\""))
            .count(),
        2
    );
    assert!(text.contains("file does not end with a newline"));

    // With the baseline, the pre-existing violations are suppressed → clean.
    let checked = run(root, &["check", "--baseline", ".alint-baseline.json"]);
    assert_eq!(code(&checked), 0, "all baselined → exit 0");
    let stderr = String::from_utf8_lossy(&checked.stderr);
    assert!(
        stderr.contains("2 baselined violation(s) suppressed"),
        "{stderr}"
    );

    // A NEW violation is reported and fails; the old ones stay suppressed.
    std::fs::write(root.join("c.txt"), "another TODO\n").unwrap();
    let with_new = run(root, &["check", "--baseline", ".alint-baseline.json"]);
    assert_eq!(code(&with_new), 1, "a new violation fails the gate");
    let out_s = String::from_utf8_lossy(&with_new.stdout);
    assert!(
        out_s.contains("c.txt"),
        "the new violation is reported: {out_s}"
    );
    assert!(
        !out_s.contains("a.txt"),
        "the baselined one is not: {out_s}"
    );
}

#[test]
fn show_baselined_lists_suppressed_findings() {
    let d = fixture();
    let root = d.path();
    run(root, &["baseline"]);
    let out = run(
        root,
        &[
            "check",
            "--baseline",
            ".alint-baseline.json",
            "--show-baselined",
        ],
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("[needs-newline]"), "{stderr}");
    assert!(stderr.contains("[no-todo]"), "{stderr}");
}

#[test]
fn missing_baseline_file_is_a_hard_error() {
    let d = fixture();
    let out = run(d.path(), &["check", "--baseline", "does-not-exist.json"]);
    assert_eq!(
        code(&out),
        2,
        "missing baseline → config-error exit, not a silent no-op"
    );
}

#[test]
fn unsupported_schema_version_is_rejected() {
    let d = fixture();
    std::fs::write(d.path().join("bad.json"), "{\"schema_version\":999}\n").unwrap();
    let out = run(d.path(), &["check", "--baseline", "bad.json"]);
    assert_eq!(code(&out), 2);
    assert!(String::from_utf8_lossy(&out.stderr).contains("schema_version 999 is unsupported"));
}

#[test]
fn regeneration_refuses_to_grandfather_new_debt_without_accept_new() {
    let d = fixture();
    let root = d.path();
    run(root, &["baseline"]); // baseline the original two
    std::fs::write(root.join("c.txt"), "another TODO\n").unwrap(); // introduce new debt

    // Re-running baseline must refuse to silently grandfather c.txt.
    let refused = run(root, &["baseline"]);
    assert_eq!(code(&refused), 2, "refuses without --accept-new");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("grandfather 1 new violation"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );

    // --accept-new takes it.
    assert_eq!(code(&run(root, &["baseline", "--accept-new"])), 0);
}

#[test]
fn directory_findings_baseline_without_a_read_warning() {
    // A nested rule flags directories. The fingerprint leaves their content
    // out without trying to read them, so neither command warns per directory.
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    for pkg in ["a", "b"] {
        std::fs::create_dir_all(root.join("pkgs").join(pkg)).unwrap();
    }
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: each-readme\n    kind: for_each_dir\n    select: \"pkgs/*\"\n    \
         require:\n      - kind: file_exists\n        paths: \"{path}/README.md\"\n    level: error\n",
    )
    .unwrap();
    for args in [
        &["baseline"][..],
        &["check", "--baseline", ".alint-baseline.json"],
    ] {
        let out = run(root, args);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(code(&out), 0, "{args:?}: {stderr}");
        assert!(!stderr.contains("WARN"), "{args:?}: {stderr}");
    }
}

#[test]
fn baseline_rejects_changed_scope() {
    let d = fixture();
    let out = run(d.path(), &["baseline", "--changed"]);
    assert_ne!(code(&out), 0, "a baseline must be whole-tree");
    assert!(String::from_utf8_lossy(&out.stderr).contains("unexpected argument"));
}

#[test]
fn strict_baseline_fails_on_stale_entries() {
    let d = fixture();
    let root = d.path();
    run(root, &["baseline"]);

    // Fix one of the baselined violations → its entry goes stale.
    std::fs::write(root.join("a.txt"), "clean\n").unwrap();

    // Warn-only (default): stale doesn't fail the build.
    let warn = run(root, &["check", "--baseline", ".alint-baseline.json"]);
    assert_eq!(code(&warn), 0, "warn-only: fixing must not fail");
    assert!(String::from_utf8_lossy(&warn.stderr).contains("no longer fire"));

    // --strict-baseline: stale fails.
    let strict = run(
        root,
        &[
            "check",
            "--baseline",
            ".alint-baseline.json",
            "--strict-baseline",
        ],
    );
    assert_eq!(code(&strict), 1, "strict: stale fails the build");
}

/// A `baseline:` config key makes suppression active with no `--baseline`
/// flag, and `alint baseline` writes to that same path by default — so the
/// writer and reader never split-brain.
#[test]
fn config_key_baseline_suppresses_without_the_flag() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\n\
         baseline: .alint-baseline.json\n\
         rules:\n\
         \x20 - id: needs-newline\n\
         \x20   kind: final_newline\n\
         \x20   paths: [\"**/*.txt\"]\n\
         \x20   level: error\n",
    )
    .unwrap();
    std::fs::write(root.join("a.txt"), "no newline").unwrap();

    // With the config key set but no baseline file yet, `check` is a hard
    // error (exit 2) — a forgotten `alint baseline` must not silently run
    // ungated. You bootstrap with `alint baseline` (which writes), below.
    assert_eq!(
        code(&run(root, &["check"])),
        2,
        "config key + missing file → hard error, not a silent un-suppressed pass",
    );

    // `alint baseline` (no --output) writes to the config-key path.
    assert_eq!(code(&run(root, &["baseline"])), 0);
    assert!(
        root.join(".alint-baseline.json").is_file(),
        "baseline written to the config-key path by default",
    );

    // `alint check` with NO --baseline flag now honors the config key.
    assert_eq!(
        code(&run(root, &["check"])),
        0,
        "config-key baseline suppresses without the flag",
    );

    // A NEW violation still surfaces.
    std::fs::write(root.join("b.txt"), "also no newline").unwrap();
    assert_eq!(code(&run(root, &["check"])), 1, "new violation still gates");
}

/// The `--baseline` flag overrides the `baseline:` config key; and when only
/// the config key is in effect and its file is missing, that's the same hard
/// error as a missing `--baseline` (proving the key is actually consulted).
#[test]
fn baseline_flag_overrides_the_config_key() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\n\
         baseline: does-not-exist.json\n\
         rules:\n\
         \x20 - id: needs-newline\n\
         \x20   kind: final_newline\n\
         \x20   paths: [\"**/*.txt\"]\n\
         \x20   level: error\n",
    )
    .unwrap();
    std::fs::write(root.join("a.txt"), "no newline").unwrap();

    // A real baseline at a different path.
    assert_eq!(code(&run(root, &["baseline", "--output", "real.json"])), 0);

    // --baseline real.json overrides the (missing) config-key path → suppressed.
    assert_eq!(
        code(&run(root, &["check", "--baseline", "real.json"])),
        0,
        "the flag overrides the config key",
    );

    // Without the flag, the config key (a missing file) is consulted → the
    // documented hard error (exit 2), never a silent un-suppressed run.
    assert_eq!(
        code(&run(root, &["check"])),
        2,
        "config key points at a missing file → hard error, proving it's consulted",
    );
}

/// `check --baseline --format sarif` MARKS baselined findings (suppressions +
/// baselineState:unchanged) instead of dropping them, so GitHub Code Scanning
/// dismisses rather than closes-then-reopens the alert. A fully-baselined repo
/// still exits 0 — the exit code is gated on live findings only.
#[test]
fn sarif_marks_baselined_findings_not_removed() {
    let d = fixture();
    let root = d.path();
    assert_eq!(
        code(&run(root, &["baseline"])),
        0,
        "snapshot the 2 findings"
    );

    let out = run(
        root,
        &[
            "check",
            "--baseline",
            ".alint-baseline.json",
            "--format",
            "sarif",
        ],
    );
    assert_eq!(
        code(&out),
        0,
        "fully-baselined → exit 0 (gated on live only)"
    );
    let sarif = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        sarif.matches("\"baselineState\": \"unchanged\"").count(),
        2,
        "both baselined findings are emitted + marked, not removed:\n{sarif}",
    );
    assert_eq!(
        sarif.matches("\"kind\": \"external\"").count(),
        2,
        "each baselined finding carries an external suppression",
    );
    assert!(
        sarif.contains("\"partialFingerprints\""),
        "fingerprints present for Code Scanning correlation",
    );
}

/// `check --baseline --format json` carries the suppressed count in the
/// envelope and the live findings in `results` — exercising the JSON
/// dispatch arm (`write_json_with_baseline`) that the SARIF test doesn't.
#[test]
fn json_baseline_reports_suppressed_count() {
    let d = fixture();
    let root = d.path();
    assert_eq!(code(&run(root, &["baseline"])), 0);

    // Fully baselined → exit 0, the envelope still reports the 2 suppressed.
    let out = run(
        root,
        &[
            "check",
            "--baseline",
            ".alint-baseline.json",
            "--format",
            "json",
        ],
    );
    assert_eq!(code(&out), 0, "fully baselined → exit 0");
    let json = String::from_utf8_lossy(&out.stdout);
    assert!(
        json.contains("\"baselined_suppressed\": 2"),
        "suppressed count in the envelope:\n{json}",
    );

    // A new finding surfaces in `results`; the suppressed count is unchanged.
    std::fs::write(root.join("c.txt"), "another TODO\n").unwrap();
    let out2 = run(
        root,
        &[
            "check",
            "--baseline",
            ".alint-baseline.json",
            "--format",
            "json",
        ],
    );
    assert_eq!(code(&out2), 1, "a new finding fails the gate");
    let json2 = String::from_utf8_lossy(&out2.stdout);
    assert!(json2.contains("\"baselined_suppressed\": 2"), "{json2}");
    assert!(
        json2.contains("c.txt"),
        "the new finding is in results:\n{json2}"
    );
}

/// `alint baseline` is byte-identical across regenerations of an unchanged
/// tree — the committed file must never churn (e.g. on advisory fields), or
/// CI diffs would flap. Guards the determinism `from_fingerprints` enforces.
#[test]
fn baseline_regeneration_is_byte_identical() {
    let d = fixture();
    let root = d.path();
    assert_eq!(code(&run(root, &["baseline", "--output", "one.json"])), 0);
    assert_eq!(code(&run(root, &["baseline", "--output", "two.json"])), 0);
    let one = std::fs::read(root.join("one.json")).unwrap();
    let two = std::fs::read(root.join("two.json")).unwrap();
    assert_eq!(
        one, two,
        "two regenerations of the same tree must be byte-identical",
    );
}

/// Regenerating must refuse a higher OCCURRENCE count on an
/// already-baselined fingerprint — that's fresh debt, not just new
/// fingerprints — unless `--accept-new` is given.
#[test]
fn regeneration_refuses_count_increase_on_existing_fingerprint() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    // `for_each_match` flags every selected line (no per-finding key), so two
    // IDENTICAL offending lines share a fingerprint and collapse to a count.
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\n\
         rules:\n\
         \x20 - id: fem\n\
         \x20   kind: for_each_match\n\
         \x20   paths: [\"**/*.txt\"]\n\
         \x20   select: \"BAD\"\n\
         \x20   require:\n\
         \x20     forbid: [\"BAD\"]\n\
         \x20   level: error\n",
    )
    .unwrap();
    // One offending line → its fingerprint is recorded with count 1.
    std::fs::write(root.join("a.txt"), "BAD\n").unwrap();
    assert_eq!(code(&run(root, &["baseline"])), 0);

    // A second IDENTICAL offending line → same fingerprint, count now 2.
    std::fs::write(root.join("a.txt"), "BAD\nBAD\n").unwrap();
    let refused = run(root, &["baseline"]);
    assert_eq!(
        code(&refused),
        2,
        "a higher count on an existing finding is fresh debt",
    );
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("grandfather 1 new violation"),
        "{}",
        String::from_utf8_lossy(&refused.stderr),
    );

    // --accept-new takes it.
    assert_eq!(code(&run(root, &["baseline", "--accept-new"])), 0);
}

/// CLASS GUARD: stale-entry detection (and its `--strict-baseline` failure) is
/// only valid on a FULL run. A SCOPED run — `--only` (this test) or `--changed`
/// (the next) — leaves out-of-scope baseline entries unevaluated; they are NOT
/// stale and must not red-light the build. Full-scope stale detection is still
/// asserted by `strict_baseline_fails_on_stale_entries`.
#[test]
fn only_scoped_run_does_not_false_fail_strict_baseline() {
    let d = fixture();
    let root = d.path();
    assert_eq!(code(&run(root, &["baseline"])), 0); // grandfather both findings

    // `--only no-todo`: `needs-newline` never runs, so its baseline entry is
    // out of scope — unmatched, but NOT stale.
    let out = run(
        root,
        &[
            "check",
            "--baseline",
            ".alint-baseline.json",
            "--only",
            "no-todo",
            "--strict-baseline",
        ],
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        code(&out),
        0,
        "an out-of-scope entry must not fail --strict-baseline: {stderr}",
    );
    assert!(
        !stderr.contains("no longer fire"),
        "no stale warning should fire under --only: {stderr}",
    );
}

/// The `--changed --baseline --strict-baseline` PR-gate recipe: a PR touching
/// only an unrelated clean file must pass, not red-light on legacy entries
/// outside the diff.
#[test]
fn changed_scoped_run_does_not_false_fail_strict_baseline() {
    let d = fixture();
    let root = d.path();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git");
    };
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&[
        "-c",
        "user.email=t@t",
        "-c",
        "user.name=t",
        "commit",
        "-qm",
        "init",
    ]);
    assert_eq!(code(&run(root, &["baseline"])), 0); // grandfather the legacy findings

    // A PR adds an unrelated CLEAN file; --changed evaluates only it.
    std::fs::write(root.join("new.txt"), "clean\n").unwrap();
    let out = run(
        root,
        &[
            "check",
            ".",
            "--changed",
            "--baseline",
            ".alint-baseline.json",
            "--strict-baseline",
        ],
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        code(&out),
        0,
        "legacy entries outside the --changed diff must not fail the gate: {stderr}",
    );
    assert!(
        !stderr.contains("no longer fire"),
        "no stale warning should fire under --changed: {stderr}",
    );
}

#[test]
fn baseline_file_is_excluded_from_the_walk() {
    // Regression (E2E-found): alint's own committed `.alint-baseline.json` is
    // JSON-Lines, so a broad-glob rule (`**/*.json`, or any `**` content rule)
    // would lint it as a NEW violation the baseline can't contain — the
    // canonical adopt-flow (check -> baseline -> check --baseline) then never
    // reaches exit 0. The baseline file must be excluded from the walk.
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    std::fs::write(root.join("package.json"), "{\"license\":\"BAD\"}\n").unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\n\
         rules:\n\
         \x20 - id: lic\n\
         \x20   kind: json_path_equals\n\
         \x20   paths: [\"**/*.json\"]\n\
         \x20   path: \"$.license\"\n\
         \x20   equals: \"MIT\"\n\
         \x20   level: error\n",
    )
    .unwrap();
    assert_eq!(code(&run(root, &["check"])), 1, "dirty before baseline");
    assert_eq!(code(&run(root, &["baseline"])), 0, "snapshot it");
    let out = run(root, &["check", "--baseline", ".alint-baseline.json"]);
    assert_eq!(
        code(&out),
        0,
        "adopt-flow must reach exit 0; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let compact = run(
        root,
        &["check", "--baseline", ".alint-baseline.json", "--compact"],
    );
    assert!(
        !String::from_utf8_lossy(&compact.stdout).contains(".alint-baseline.json"),
        "the baseline file must not be flagged"
    );
}

#[test]
fn baseline_regen_and_fix_also_exclude_the_artifact() {
    // Regression (adversarial review of #111): the walk-exclusion was
    // `check`-only. With a committed `baseline:` config key, `baseline`
    // regeneration must not treat its own JSON-Lines artifact as fresh debt
    // (was exit 2), and `fix` must not lint/rewrite it (was flagged).
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    std::fs::write(root.join("package.json"), "{\"license\":\"BAD\"}\n").unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\n\
         baseline: .alint-baseline.json\n\
         rules:\n\
         \x20 - id: lic\n\
         \x20   kind: json_path_equals\n\
         \x20   paths: [\"**/*.json\"]\n\
         \x20   path: \"$.license\"\n\
         \x20   equals: \"MIT\"\n\
         \x20   level: error\n",
    )
    .unwrap();
    assert_eq!(code(&run(root, &["baseline"])), 0, "snapshot existing debt");
    // Regeneration must stay exit 0 — the just-written artifact isn't new debt.
    let regen = run(root, &["baseline"]);
    assert_eq!(
        code(&regen),
        0,
        "regen must not grandfather its own artifact; stderr={}",
        String::from_utf8_lossy(&regen.stderr)
    );
    // fix must not mention / lint the artifact.
    let fixout = run(root, &["fix", "--dry-run"]);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&fixout.stdout),
        String::from_utf8_lossy(&fixout.stderr)
    );
    assert!(
        !combined.contains(".alint-baseline.json"),
        "fix must not flag the baseline artifact; got:\n{combined}"
    );
}

#[test]
fn nested_same_named_baseline_is_not_over_excluded() {
    // Regression (adversarial review of #111): the exclusion pattern was
    // separator-less, so `!.alint-baseline.json` matched at ANY depth — a real
    // violation in a nested file sharing the basename was silently dropped. The
    // pattern is now root-anchored (`!/…`), so only the root artifact is skipped.
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\n\
         baseline: .alint-baseline.json\n\
         rules:\n\
         \x20 - id: no-secret\n\
         \x20   kind: file_content_forbidden\n\
         \x20   paths: [\"**/*.json\"]\n\
         \x20   pattern: \"TOPSECRET\"\n\
         \x20   level: error\n",
    )
    .unwrap();
    std::fs::write(root.join("data.txt"), "clean\n").unwrap();
    assert_eq!(
        code(&run(root, &["baseline"])),
        0,
        "empty baseline (clean tree)"
    );
    // A nested file that shares the basename, carrying a real secret.
    std::fs::create_dir(root.join("sub")).unwrap();
    std::fs::write(
        root.join("sub/.alint-baseline.json"),
        "{\"k\":\"TOPSECRET\"}\n",
    )
    .unwrap();
    let compact = run(root, &["check", "--compact"]);
    assert_eq!(
        code(&compact),
        1,
        "the nested same-named file's violation must still fire; stderr={}",
        String::from_utf8_lossy(&compact.stderr)
    );
    assert!(
        // Normalise separators — Windows reports `sub\.alint-baseline.json`.
        String::from_utf8_lossy(&compact.stdout)
            .replace('\\', "/")
            .contains("sub/.alint-baseline.json"),
        "the nested file's TOPSECRET must be reported, not over-excluded"
    );
}

/// GitLab Code Quality's `fingerprint` must be the SAME canonical
/// `baseline::violation_fingerprint` that the baseline file and SARIF's
/// `partialFingerprints` carry — one finding, one identity across every
/// surface. Before the unification the GitLab path used a self-contained
/// occurrence hash, so the same finding had two different fingerprints
/// depending on which format you exported. This anchors the guarantee end to
/// end: snapshot the tree's findings into a baseline (whose entries carry the
/// canonical fingerprints), then export the identical tree as GitLab and prove
/// the fingerprint sets are equal.
#[test]
fn gitlab_fingerprint_equals_canonical_baseline_fingerprint() {
    let d = fixture();
    let root = d.path();

    // 1. Snapshot → `.alint-baseline.json` (JSONL: header line, then one entry
    //    per canonical fingerprint).
    assert_eq!(code(&run(root, &["baseline"])), 0, "snapshot the findings");
    let baseline_raw = std::fs::read_to_string(root.join(".alint-baseline.json")).unwrap();
    let canonical: std::collections::BTreeSet<String> = baseline_raw
        .lines()
        .skip(1) // header
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let entry: serde_json::Value = serde_json::from_str(l).expect("entry is JSON");
            entry["fingerprint"]
                .as_str()
                .expect("entry carries a fingerprint")
                .to_string()
        })
        .collect();
    assert_eq!(canonical.len(), 2, "two distinct findings: {canonical:?}");

    // 2. Export the SAME tree as GitLab Code Quality (no baseline → all findings).
    let gl = run(root, &["check", "--format", "gitlab"]);
    let issues: Vec<serde_json::Value> =
        serde_json::from_slice(&gl.stdout).expect("gitlab output is a JSON array");
    let gitlab_fps: std::collections::BTreeSet<String> = issues
        .iter()
        .map(|i| {
            i["fingerprint"]
                .as_str()
                .expect("issue carries a fingerprint")
                .to_string()
        })
        .collect();

    // 3. The fingerprint sets must be identical — one identity per finding.
    assert_eq!(
        gitlab_fps, canonical,
        "GitLab fingerprints must equal the canonical baseline fingerprints\n\
         gitlab={gitlab_fps:?}\ncanonical={canonical:?}",
    );
}

/// SARIF's `partialFingerprints` must carry the SAME canonical
/// `baseline::violation_fingerprint` even without `--baseline`. Before this the
/// SARIF path emitted fingerprints only when a baseline was in effect, so a
/// plain `check --format sarif` gave GitHub Code Scanning nothing to correlate
/// alerts across runs. This anchors the guarantee: snapshot the tree into a
/// baseline (canonical fingerprints), export the identical tree as SARIF with NO
/// baseline, and prove the `partialFingerprints` set equals the canonical set
/// and that no result is tagged `baselineState` (there is no baseline).
#[test]
fn sarif_fingerprint_equals_canonical_baseline_fingerprint() {
    let d = fixture();
    let root = d.path();

    // 1. Snapshot → canonical fingerprints (one per distinct finding).
    assert_eq!(code(&run(root, &["baseline"])), 0, "snapshot the findings");
    let baseline_raw = std::fs::read_to_string(root.join(".alint-baseline.json")).unwrap();
    let canonical: std::collections::BTreeSet<String> = baseline_raw
        .lines()
        .skip(1) // header
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let entry: serde_json::Value = serde_json::from_str(l).expect("entry is JSON");
            entry["fingerprint"]
                .as_str()
                .expect("entry carries a fingerprint")
                .to_string()
        })
        .collect();
    assert_eq!(canonical.len(), 2, "two distinct findings: {canonical:?}");

    // 2. Export the SAME tree as SARIF with NO baseline → every finding carries
    //    its canonical `partialFingerprints`.
    let sr = run(root, &["check", "--format", "sarif"]);
    let sarif: serde_json::Value =
        serde_json::from_slice(&sr.stdout).expect("sarif output is JSON");
    let results = sarif["runs"][0]["results"]
        .as_array()
        .expect("sarif carries a results array");
    let sarif_fps: std::collections::BTreeSet<String> = results
        .iter()
        .map(|r| {
            r["partialFingerprints"]["alint/v1"]
                .as_str()
                .expect("result carries a partialFingerprint")
                .to_string()
        })
        .collect();

    // 3. Identical sets, and no `baselineState` without a baseline.
    assert_eq!(
        sarif_fps, canonical,
        "SARIF partialFingerprints must equal the canonical baseline fingerprints\n\
         sarif={sarif_fps:?}\ncanonical={canonical:?}",
    );
    assert!(
        results.iter().all(|r| r.get("baselineState").is_none()),
        "no result may carry baselineState without --baseline",
    );
}

/// The no-baseline SARIF path must READ the offending file's bytes to compute a
/// CASE-2 (line-content) fingerprint, exactly as the baseline snapshot does. The
/// parity test above only exercises case-1 (structural key) and case-3
/// (path-only) findings, where `file_bytes` is irrelevant — so a regression that
/// passed no bytes (or the wrong root) into the SARIF fingerprint grid would keep
/// those green while silently diverging any line-content finding from the
/// baseline/GitLab identity. This pins the case-2 path end to end with a real
/// merge-conflict finding.
#[test]
fn sarif_content_discriminator_fingerprint_matches_baseline() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\n\
         rules:\n\
         \x20 - id: no-conflicts\n\
         \x20   kind: no_merge_conflict_markers\n\
         \x20   paths: [\"**/*.md\"]\n\
         \x20   level: error\n",
    )
    .unwrap();
    // Conflict-marker lines fire with the offending line's bytes as the
    // fingerprint discriminator (case 2), so the fingerprint depends on the read.
    std::fs::write(
        root.join("doc.md"),
        "intro\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\ntail\n",
    )
    .unwrap();

    // 1. Snapshot -> canonical case-2 fingerprints.
    assert_eq!(code(&run(root, &["baseline"])), 0, "snapshot");
    let baseline_raw = std::fs::read_to_string(root.join(".alint-baseline.json")).unwrap();
    let canonical: std::collections::BTreeSet<String> = baseline_raw
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l).expect("entry is JSON")["fingerprint"]
                .as_str()
                .expect("entry carries a fingerprint")
                .to_string()
        })
        .collect();
    assert!(!canonical.is_empty(), "conflict markers produce findings");

    // 2. SARIF with NO baseline.
    let sr = run(root, &["check", "--format", "sarif"]);
    let sarif: serde_json::Value = serde_json::from_slice(&sr.stdout).expect("sarif is JSON");
    let results = sarif["runs"][0]["results"]
        .as_array()
        .expect("results array");
    // Confirm these are genuinely line-anchored — the case-2 path, not case-3.
    assert!(
        results
            .iter()
            .any(|r| r["locations"][0]["physicalLocation"]["region"]["startLine"].is_number()),
        "expected a line-anchored (case-2) finding"
    );
    let sarif_fps: std::collections::BTreeSet<String> = results
        .iter()
        .map(|r| {
            r["partialFingerprints"]["alint/v1"]
                .as_str()
                .expect("case-2 finding carries a partialFingerprint")
                .to_string()
        })
        .collect();

    // 3. A None-bytes regression would make these the path-only (case-3) hash
    //    instead, breaking this equality.
    assert_eq!(
        sarif_fps, canonical,
        "SARIF case-2 partialFingerprints must equal the baseline fingerprints\n\
         sarif={sarif_fps:?}\ncanonical={canonical:?}",
    );
}
