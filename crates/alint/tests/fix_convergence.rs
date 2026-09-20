//! Round-4 audit: detector-domain == fixer-domain convergence.
//!
//! A content rule's DETECTOR must flag exactly the files its FIXER can fix, or a
//! file in the gap is reported "fixable" forever yet never fixed (`alint fix`
//! never converges). Two gaps were found and closed:
//!
//!   * invalid-UTF-8-but-not-binary (a lone `0xFF`, no NUL): the byte-level
//!     detectors (bidi / zero-width / trailing-whitespace) flag it, so the
//!     fixers now strip at the BYTE level and preserve the junk byte instead of
//!     bailing on a strict `from_utf8`. The bidi case is security-relevant: a
//!     Trojan-Source override must not survive `alint fix` just because the
//!     attacker also dropped one invalid byte.
//!   * NUL-bearing binary: the fixers refuse it (editing binary corrupts it), so
//!     the detectors now skip it too -- `check` and `fix` agree.
//!   * unreadable file (mode 000 / permission denied): `check` reads via the
//!     engine's `read_capped_or_skip`, which SKIPS an unreadable file before
//!     dispatch, so `check` exits 0. But `fix` finds violations through each
//!     rule's whole-index `evaluate`, whose read arm flagged an "could not read
//!     file" violation -- unfixable -> `fix` exited 1 on the very tree `check`
//!     called clean. The per-file content rules now fail open on a read error
//!     (skip, matching `check`), closing that exit-code divergence.
//!
//! These live as an integration test rather than a scenario/property because a
//! raw `0xFF` byte cannot be represented in a UTF-8 YAML scenario tree, and an
//! unreadable file needs a runtime `chmod` no fixture tree can express.

use std::path::{Path, PathBuf};
use std::process::Command;

fn alint() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn write(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(p, bytes).unwrap();
}

fn config(root: &Path, yaml: &str) {
    std::fs::write(root.join(".alint.yml"), yaml).unwrap();
}

/// `alint check` exits 0 when the tree is clean, 1 when any violation remains.
/// Robust convergence signal (counting `✗` glyphs double-counts the summary).
fn check_is_clean(root: &Path) -> bool {
    Command::new(alint())
        .args(["check", "."])
        .current_dir(root)
        .output()
        .expect("run alint check")
        .status
        .code()
        == Some(0)
}

fn fix(root: &Path) -> std::process::Output {
    Command::new(alint())
        .args(["fix", "."])
        .current_dir(root)
        .output()
        .expect("run alint fix")
}

/// The security case (F1): a bidi override plus one invalid UTF-8 byte must be
/// stripped by `alint fix`, and the fix must converge. The junk byte survives.
#[test]
fn bidi_control_with_invalid_utf8_byte_is_stripped_and_converges() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // `a` `0xFF` U+202E(RLO, E2 80 AE) `b` `\n`
    write(root, "src/evil.rs", b"a\xFF\xE2\x80\xAEb\n");
    config(
        root,
        "version: 1\nrules:\n  - id: no-bidi\n    kind: no_bidi_controls\n    \
         paths: \"src/**/*.rs\"\n    level: error\n    fix: { file_strip_bidi: {} }\n",
    );
    assert!(!check_is_clean(root), "the bidi override must be flagged");
    let out = fix(root);
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("1 applied"),
        "fix must strip the bidi control, got: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(
        std::fs::read(root.join("src/evil.rs")).unwrap(),
        b"a\xFFb\n",
        "the RLO is gone and the junk 0xFF byte is preserved"
    );
    assert!(
        check_is_clean(root),
        "converged: no bidi violation remains after one fix"
    );
}

#[test]
fn zero_width_with_invalid_utf8_byte_is_stripped_and_converges() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // `a` `0xFF` U+200B(ZWSP, E2 80 8B) `b` `\n`
    write(root, "src/z.rs", b"a\xFF\xE2\x80\x8Bb\n");
    config(
        root,
        "version: 1\nrules:\n  - id: no-zw\n    kind: no_zero_width_chars\n    \
         paths: \"src/**/*.rs\"\n    level: error\n    fix: { file_strip_zero_width: {} }\n",
    );
    assert!(!check_is_clean(root));
    fix(root);
    assert_eq!(std::fs::read(root.join("src/z.rs")).unwrap(), b"a\xFFb\n");
    assert!(check_is_clean(root), "converged");
}

#[test]
fn trailing_whitespace_with_invalid_utf8_byte_is_trimmed_and_converges() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // `a` `0xFF` space space `\n`
    write(root, "src/w.rs", b"a\xFF  \n");
    config(
        root,
        "version: 1\nrules:\n  - id: ws\n    kind: no_trailing_whitespace\n    \
         paths: \"src/**/*.rs\"\n    level: error\n    fix: { file_trim_trailing_whitespace: {} }\n",
    );
    assert!(!check_is_clean(root));
    fix(root);
    assert_eq!(std::fs::read(root.join("src/w.rs")).unwrap(), b"a\xFF\n");
    assert!(check_is_clean(root), "converged");
}

/// F3: every byte-level content rule skips a NUL-bearing binary at the DETECTOR,
/// so `check` reports nothing and `fix` touches nothing (agreement, not the old
/// "flagged fixable forever, never fixed"). One representative file exercised by
/// all six rules on a broad glob.
#[test]
fn detectors_skip_binary_so_check_and_fix_agree() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // NUL-bearing "binary" that would otherwise trip trailing-ws, missing final
    // newline, CRLF, a blank run, a bidi control, and a zero-width char.
    let binary: &[u8] = b"a\x00 \r\n\n\n\n\xE2\x80\xAE\xE2\x80\x8Bb   ";
    write(root, "blob.bin", binary);
    config(
        root,
        "version: 1\nrules:\n\
         \x20 - id: ws\n    kind: no_trailing_whitespace\n    paths: \"**/*.bin\"\n    level: error\n    fix: { file_trim_trailing_whitespace: {} }\n\
         \x20 - id: eof\n    kind: final_newline\n    paths: \"**/*.bin\"\n    level: error\n    fix: { file_append_final_newline: {} }\n\
         \x20 - id: le\n    kind: line_endings\n    paths: \"**/*.bin\"\n    target: lf\n    level: error\n    fix: { file_normalize_line_endings: {} }\n\
         \x20 - id: blanks\n    kind: max_consecutive_blank_lines\n    paths: \"**/*.bin\"\n    max: 1\n    level: error\n    fix: { file_collapse_blank_lines: {} }\n\
         \x20 - id: bidi\n    kind: no_bidi_controls\n    paths: \"**/*.bin\"\n    level: error\n    fix: { file_strip_bidi: {} }\n\
         \x20 - id: zw\n    kind: no_zero_width_chars\n    paths: \"**/*.bin\"\n    level: error\n    fix: { file_strip_zero_width: {} }\n",
    );
    assert!(
        check_is_clean(root),
        "no content rule may flag a NUL-bearing binary (its fixer would refuse it)"
    );
    fix(root);
    assert_eq!(
        std::fs::read(root.join("blob.bin")).unwrap(),
        binary,
        "the binary is left byte-identical"
    );
}

/// Non-convergence (fixpoint cap): two `replace` rules that undo each other
/// (`a` -> `b`, `b` -> `a`) change the tree every pass and never settle. The
/// byte-level fixpoint must hard-stop at the cap and exit `2` ("fix could not
/// complete"), naming the stuck rule on stderr -- never a silent exit-0 success.
/// This is the sole end-to-end exercise of the exit-2 contract: it is reachable
/// only because there is no apply-once shortcut (the loop converges on "a pass
/// changed nothing", so a genuine oscillation runs to the cap).
/// See docs/design/v0.17/fixpoint.md.
#[test]
fn nonconvergent_config_hits_the_cap_and_exits_2() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, "f.txt", b"a\n");
    config(
        root,
        "version: 1\nrules:\n\
         \x20 - id: no-a\n    kind: file_content_forbidden\n    paths: \"*.txt\"\n    pattern: \"a\"\n    level: error\n    fix: { replace: { replacement: \"b\" } }\n\
         \x20 - id: no-b\n    kind: file_content_forbidden\n    paths: \"*.txt\"\n    pattern: \"b\"\n    level: error\n    fix: { replace: { replacement: \"a\" } }\n",
    );
    let out = Command::new(alint())
        .args(["fix", "--unsafe-fixes", "."])
        .current_dir(root)
        .output()
        .expect("run alint fix --unsafe-fixes");
    assert_eq!(
        out.status.code(),
        Some(2),
        "a non-convergent config must exit 2, not silently succeed"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("did not settle"),
        "the cap must warn loudly on stderr; got: {stderr}"
    );
    assert!(
        stderr.contains("no-a") || stderr.contains("no-b"),
        "the warning must name a stuck rule; got: {stderr}"
    );
    // The same non-convergence must be visible STRUCTURALLY in the JSON summary
    // (the only machine signal of exit 2 -- the items are all `applied`, so
    // without it a capped run is indistinguishable from a clean one).
    let json = Command::new(alint())
        .args(["fix", "--unsafe-fixes", "--format", "json", "."])
        .current_dir(root)
        .output()
        .expect("run alint fix --format json");
    assert_eq!(json.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&json.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("fix json parses");
    assert_eq!(
        parsed["summary"]["non_convergent"],
        serde_json::json!(true),
        "the JSON summary must flag non-convergence; got: {stdout}"
    );
}

/// Audit MED-1: `--fix-only --format json` rebuilds a filtered report for
/// rendering and USED to hardcode `non_convergent: false`, so a capped run
/// reported clean in the only machine signal of exit 2 (the exit code itself was
/// still correct). The flag must reflect the real convergence verdict.
#[test]
fn nonconvergent_fix_only_json_still_flags_non_convergence() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, "f.txt", b"a\n");
    config(
        root,
        "version: 1\nrules:\n\
         \x20 - id: no-a\n    kind: file_content_forbidden\n    paths: \"*.txt\"\n    pattern: \"a\"\n    level: error\n    fix: { replace: { replacement: \"b\" } }\n\
         \x20 - id: no-b\n    kind: file_content_forbidden\n    paths: \"*.txt\"\n    pattern: \"b\"\n    level: error\n    fix: { replace: { replacement: \"a\" } }\n",
    );
    let json = Command::new(alint())
        .args([
            "fix",
            "--fix-only",
            "--unsafe-fixes",
            "--format",
            "json",
            ".",
        ])
        .current_dir(root)
        .output()
        .expect("run alint fix --fix-only --format json");
    assert_eq!(json.status.code(), Some(2));
    let parsed: serde_json::Value = serde_json::from_slice(&json.stdout).expect("fix json parses");
    assert_eq!(
        parsed["summary"]["non_convergent"],
        serde_json::json!(true),
        "--fix-only --format json must flag non-convergence, not hardcode false: {}",
        String::from_utf8_lossy(&json.stdout)
    );
}

/// Structured non-convergence: two `set_value` rules pinning ONE node to
/// different values oscillate (each pass overlap-skips one, changes the file,
/// the re-walk applies the other) and must hit the cap + exit 2. The `replace`
/// path already exercises the exit-2 contract; this drives it through the
/// STRUCTURED verify + overlap-skip code path (Phase 2), which the audit found
/// was not covered end-to-end. `set_value` is Safe, so a bare `fix` applies it.
#[test]
fn nonconvergent_set_value_hits_the_cap_and_exits_2() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, "app.json", b"{\n  \"port\": 0\n}\n");
    config(
        root,
        "version: 1\nrules:\n\
         \x20 - id: a\n    kind: json_path_equals\n    paths: \"*.json\"\n    path: \"$.port\"\n    equals: 1\n    level: error\n    fix: { set_value: {} }\n\
         \x20 - id: b\n    kind: json_path_equals\n    paths: \"*.json\"\n    path: \"$.port\"\n    equals: 2\n    level: error\n    fix: { set_value: {} }\n",
    );
    let out = Command::new(alint())
        .args(["fix", "."])
        .current_dir(root)
        .output()
        .expect("run alint fix");
    assert_eq!(
        out.status.code(),
        Some(2),
        "structured non-convergence must exit 2, not silently succeed"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("did not settle"),
        "the cap must warn on stderr; got: {stderr}"
    );
}

/// Follow-up 2 regression: many WHOLE-DOCUMENT rules (TOML `set_value`) on ONE
/// file must converge in a SINGLE pass and exit 0. Before `minimal_replace`, each
/// rule emitted a `0..len` whole-file edit; N such edits on one file OVERLAP, so
/// the engine applied one per pass -> 12 rules > `MAX_PASSES` (10) -> a FALSE exit
/// 2 on a perfectly resolvable config. Reducing each rewrite to its minimal changed
/// span makes the 12 edits DISJOINT -> all co-apply in one pass. (Integration test,
/// not a scenario: the >10-rules-per-file shape is the point, and the old bug was
/// an exit code / pass-cap interaction a scenario cannot assert.)
#[test]
fn many_whole_doc_rules_on_one_file_converge_in_one_pass() {
    use std::fmt::Write as _;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let n = 12; // > MAX_PASSES (10)
    let mut toml = String::new();
    let mut cfg = String::from("version: 1\nrules:\n");
    for i in 0..n {
        let _ = writeln!(toml, "k{i} = 0");
        let _ = writeln!(
            cfg,
            "  - {{id: r{i}, kind: toml_path_equals, paths: \"**/*.toml\", \
             path: \"$['k{i}']\", equals: {}, level: error, fix: {{ set_value: {{}} }}}}",
            i + 100
        );
    }
    write(root, "app.toml", toml.as_bytes());
    config(root, &cfg);
    let out = Command::new(alint())
        .args(["fix", "."])
        .current_dir(root)
        .output()
        .expect("run alint fix");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{n} whole-doc rules on one file must converge (exit 0), not falsely cap; \
         stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Every key was rewritten and a fresh check is clean.
    assert!(check_is_clean(root), "the tree must be clean after the fix");
    let fixed = std::fs::read_to_string(root.join("app.toml")).unwrap();
    assert!(
        fixed.contains("k11 = 111") && fixed.contains("k0 = 100"),
        "got: {fixed}"
    );
}

/// Audit regression (false negative, size path): a located `replace` on a file
/// larger than `fix_size_limit` (default 1 MiB) but smaller than the check read
/// cap used to produce NO report item -> `fix` exited 0 while the forbidden
/// pattern stayed on disk. It must now report the size-skip and exit 1, agreeing
/// with `check`. See docs/design/v0.17/fixpoint.md.
#[test]
fn located_fix_over_size_limit_is_reported_not_dropped() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let mut big = b"FORBIDDEN\n".to_vec();
    big.resize(1_600_000, b'x'); // > 1 MiB fix cap, < 256 MiB check cap
    write(root, "big.txt", &big);
    config(
        root,
        "version: 1\nrules:\n  - id: no-forbidden\n    kind: file_content_forbidden\n    paths: \"*.txt\"\n    pattern: \"FORBIDDEN\"\n    level: error\n    fix: { replace: { replacement: \"OK\" } }\n",
    );
    let out = Command::new(alint())
        .args(["fix", "--unsafe-fixes", "."])
        .current_dir(root)
        .output()
        .expect("run alint fix");
    assert_eq!(
        out.status.code(),
        Some(1),
        "an over-size located fix must REPORT the skip and exit 1, not silently exit 0"
    );
    assert!(
        !check_is_clean(root),
        "the forbidden pattern genuinely still stands; fix and check agree"
    );
}

/// Audit regression (false positive, sticky phantom): a rule that Applies on one
/// file and size-skips a large file must not strand a phantom skip after ANOTHER
/// rule deletes the large file -> `fix` must exit 0 on the resulting clean tree.
#[test]
fn no_phantom_skip_for_a_file_another_rule_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, "small.txt", b"hi \n"); // trailing ws, fixable
    let mut big = b"y \n".to_vec(); // trailing ws on line 1
    big.resize(1_800_000, b'y'); // > 1 MiB -> ntw size-skips it during fix
    write(root, "big.txt", &big);
    config(
        root,
        "version: 1\nrules:\n  - id: ntw\n    kind: no_trailing_whitespace\n    paths: \"*.txt\"\n    level: error\n    fix: { file_trim_trailing_whitespace: {} }\n  - id: drop-big\n    kind: file_absent\n    paths: \"big.txt\"\n    level: error\n    fix: { file_remove: {} }\n",
    );
    let out = Command::new(alint())
        .args(["fix", "--unsafe-fixes", "."])
        .current_dir(root)
        .output()
        .expect("run alint fix");
    assert_eq!(
        out.status.code(),
        Some(0),
        "big.txt was removed by another rule, so its ntw size-skip is a phantom -> exit 0"
    );
    assert!(check_is_clean(root), "the tree is genuinely clean");
    assert!(!root.join("big.txt").exists(), "big.txt was removed");
}

/// Audit regression (R3, fix-vs-check exit-code divergence on an unreadable file):
/// `check` skips a mode-000 file via the engine's `read_capped_or_skip` and exits
/// 0, but every per-file content rule's whole-index `evaluate` (the read path
/// `fix` uses) flagged it "could not read file" -> unfixable -> `fix` exited 1 on
/// the same tree. The rules now fail open on a read error (skip, matching
/// `check`), so `fix` and `check` agree. Exercises both distinct read paths:
/// `file_content_forbidden` reads via `read_capped`, `file_is_text` via
/// `read_prefix`. `#[cfg(unix)]` -- mode 000 is the portable "unreadable" proxy.
#[cfg(unix)]
#[test]
fn unreadable_file_is_skipped_by_both_check_and_fix() {
    use std::os::unix::fs::PermissionsExt as _;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // Content that WOULD be flagged if readable: it contains FORBIDDEN and is
    // valid text -- so the skip below is meaningful, not a trivial no-match.
    write(root, "secret.txt", b"FORBIDDEN\n");
    let p = root.join("secret.txt");
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
    // Running as root (common in CI containers) bypasses mode bits, so the file
    // stays readable and this test cannot exercise the unreadable path. Probe and
    // skip rather than assert a premise that only holds unprivileged.
    if std::fs::read(&p).is_ok() {
        eprintln!("skipping: mode 000 file is still readable (running as root?)");
        return;
    }
    config(
        root,
        "version: 1\nrules:\n\
         \x20 - id: no-forbidden\n    kind: file_content_forbidden\n    paths: \"*.txt\"\n    pattern: \"FORBIDDEN\"\n    level: error\n    fix: { replace: { replacement: \"OK\" } }\n\
         \x20 - id: must-be-text\n    kind: file_is_text\n    paths: \"*.txt\"\n    level: error\n",
    );
    // check already skips the unreadable file (its read path is
    // `read_capped_or_skip`), so the tree reads "clean" -> exit 0.
    assert!(
        check_is_clean(root),
        "check must skip an unreadable file, not flag it"
    );
    // fix must AGREE: skip the unreadable file and exit 0. Before R3, `evaluate`
    // flagged "could not read file" (both rules) -> unfixable -> exit 1.
    let out = fix(root);
    assert_eq!(
        out.status.code(),
        Some(0),
        "fix must skip an unreadable file and exit 0, agreeing with check; stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !combined.contains("could not read file"),
        "fix must not surface a 'could not read file' violation; got: {combined}"
    );
    // Restore perms so tempdir cleanup is unencumbered on exotic platforms.
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
}
