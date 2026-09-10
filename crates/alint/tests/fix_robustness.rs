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
