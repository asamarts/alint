//! Regression (adversarial audit of the compose engine): a write failure
//! during the per-file compose flush must NOT abort the whole `alint fix` pass
//! or lose unrelated fixes. A single unwritable location degrades to a Skipped
//! item for that file while every other file is still fixed and reported --
//! exactly as the direct-write path behaves (a failed `write_atomic` inside one
//! fixer surfaces as Skipped and the others proceed).
//!
//! Unix-only: it relies on a read-only directory to make `write_atomic`'s
//! temp+rename fail for one file but not another.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn alint() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(alint())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn alint")
}

const CONFIG: &str = "\
version: 1
rules:
  - id: no-ws
    kind: no_trailing_whitespace
    paths: \"**/*.rs\"
    level: error
    fix:
      file_trim_trailing_whitespace: {}
";

#[test]
fn write_failure_in_one_dir_still_fixes_the_rest() {
    use std::os::unix::fs::PermissionsExt as _;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), CONFIG).unwrap();
    std::fs::write(root.join("good.rs"), "fn a() {}   \n").unwrap();
    std::fs::create_dir(root.join("ro")).unwrap();
    std::fs::write(root.join("ro/blocked.rs"), "fn b() {}   \n").unwrap();
    // Read-only subdir: write_atomic's sibling-temp create cannot land there.
    std::fs::set_permissions(root.join("ro"), std::fs::Permissions::from_mode(0o555)).unwrap();

    let out = run(root, &["fix", "."]);

    // Read results, then restore perms BEFORE asserting so a failing assertion
    // still lets the TempDir clean up.
    let good = std::fs::read_to_string(root.join("good.rs")).unwrap();
    let blocked = std::fs::read_to_string(root.join("ro/blocked.rs")).unwrap();
    std::fs::set_permissions(root.join("ro"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // The writable file is fixed despite the failure elsewhere (no abort).
    assert_eq!(
        good, "fn a() {}\n",
        "good.rs must be fixed; output:\n{combined}"
    );
    // The blocked file is unchanged and reported Skipped, not Applied.
    assert_eq!(blocked, "fn b() {}   \n", "blocked.rs must be unchanged");
    assert!(
        combined.contains("1 applied"),
        "good.rs should be applied; output:\n{combined}"
    );
    assert!(
        combined.contains("1 skipped"),
        "blocked.rs should degrade to Skipped, not abort the pass; output:\n{combined}"
    );
}

const WARNING_CONFIG: &str = "\
version: 1
rules:
  - id: no-ws
    kind: no_trailing_whitespace
    paths: \"**/*.rs\"
    level: warning
    fix:
      file_trim_trailing_whitespace: {}
";

#[test]
fn default_fix_exits_nonzero_when_a_fix_errors_even_at_warning_level() {
    // Round-4 audit: a genuine I/O write error (read-only dir) is not a benign
    // declined skip. The default `alint fix` path must fail (exit 1) on it,
    // regardless of the rule's LEVEL -- matching the `--fix-only` path -- or a
    // warning/info-level hygiene fix silently doesn't land yet the run reports
    // success. (Previously the default path was purely level-gated: exit 0.)
    use std::os::unix::fs::PermissionsExt as _;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), WARNING_CONFIG).unwrap();
    std::fs::write(root.join("good.rs"), "fn a() {}   \n").unwrap();
    std::fs::create_dir(root.join("ro")).unwrap();
    std::fs::write(root.join("ro/blocked.rs"), "fn b() {}   \n").unwrap();
    std::fs::set_permissions(root.join("ro"), std::fs::Permissions::from_mode(0o555)).unwrap();

    let out = run(root, &["fix", "."]);

    let good = std::fs::read_to_string(root.join("good.rs")).unwrap();
    std::fs::set_permissions(root.join("ro"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert_eq!(good, "fn a() {}\n", "the writable file is still fixed");
    assert_eq!(
        out.status.code(),
        Some(1),
        "a genuine write error must fail the default fix even at warning level; output:\n{combined}"
    );
}

#[test]
fn fix_only_surfaces_a_fix_error_and_exits_nonzero() {
    // Round-7 (A3-F2): --fix-only suppresses BENIGN residuals (declined /
    // unfixable / suggested), but a fix that was ATTEMPTED and ERRORED (here a
    // write into a read-only dir) must both fail the run (exit 1 via
    // had_fix_error) AND be VISIBLE in the report -- previously the errored item
    // was filtered out with the benign residuals, leaving a nonzero exit with a
    // report reading "0 applied, 0 skipped" and no cause anywhere (stdout or
    // stderr): a CI operator saw a failed step and no reason.
    use std::os::unix::fs::PermissionsExt as _;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), CONFIG).unwrap();
    std::fs::write(root.join("good.rs"), "fn a() {}   \n").unwrap();
    std::fs::create_dir(root.join("ro")).unwrap();
    std::fs::write(root.join("ro/blocked.rs"), "fn b() {}   \n").unwrap();
    std::fs::set_permissions(root.join("ro"), std::fs::Permissions::from_mode(0o555)).unwrap();

    let out = run(root, &["fix", "--fix-only", "."]);

    let good = std::fs::read_to_string(root.join("good.rs")).unwrap();
    std::fs::set_permissions(root.join("ro"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert_eq!(good, "fn a() {}\n", "the writable file is still fixed");
    // The fix ERROR is surfaced (not suppressed like a benign residual)...
    assert!(
        stdout.contains("blocked.rs"),
        "--fix-only must surface the errored file so the failure has a cause; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("fix error"),
        "the surfaced item must be labeled a fix error; stdout:\n{stdout}"
    );
    // ...and the exit is nonzero.
    assert_eq!(
        out.status.code(),
        Some(1),
        "--fix-only must fail when a fix errored"
    );
}
