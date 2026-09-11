//! `alint fix --changed` blast-radius gate. A full-index rule (existence /
//! cross-file) evaluates the whole tree for its check verdict even under
//! `--changed`, but the FIX must not touch files outside the working-tree diff.
//! Regression for the Phase-0 audit finding where `file_absent` + `file_remove`
//! under `--changed` DELETED unchanged committed files that the diff never
//! touched.

use std::path::{Path, PathBuf};
use std::process::Command;

fn alint() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("run git")
        .status;
    assert!(status.success(), "git {args:?} failed");
}

const CONFIG: &str = "\
version: 1
rules:
  - id: no-logs
    kind: file_absent
    paths: \"**/*.log\"
    level: error
    fix: { file_remove: {} }
";

#[test]
fn fix_changed_does_not_touch_files_outside_the_diff() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("build")).unwrap();
    std::fs::create_dir_all(root.join("logs")).unwrap();
    std::fs::write(root.join("build/old.log"), "old\n").unwrap();
    std::fs::write(root.join("logs/today.log"), "today\n").unwrap();
    std::fs::write(root.join(".alint.yml"), CONFIG).unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);
    // Modify only logs/today.log -> it is the sole working-tree-diff file.
    std::fs::write(root.join("logs/today.log"), "edited\n").unwrap();

    let out = Command::new(alint())
        .args(["fix", "--changed", "."])
        .current_dir(root)
        .output()
        .expect("run alint fix --changed");
    // Exit 0 or 1 both acceptable (a removed error-level violation may remain
    // as residual for the file it did remove); what matters is the blast radius.
    assert!(out.status.code() == Some(0) || out.status.code() == Some(1));

    assert!(
        root.join("build/old.log").exists(),
        "fix --changed must NOT remove the unchanged, out-of-diff build/old.log"
    );
    assert!(
        !root.join("logs/today.log").exists(),
        "the changed logs/today.log is in-scope and should be removed"
    );
}

#[test]
fn full_fix_still_removes_every_match() {
    // Sanity: WITHOUT --changed, the same rule removes both matching files, so
    // the blast-radius guard above only narrows the --changed path.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("build")).unwrap();
    std::fs::write(root.join("build/old.log"), "old\n").unwrap();
    std::fs::write(root.join("a.log"), "a\n").unwrap();
    std::fs::write(root.join(".alint.yml"), CONFIG).unwrap();

    let out = Command::new(alint())
        .args(["fix", "."])
        .current_dir(root)
        .output()
        .expect("run alint fix");
    assert!(out.status.code() == Some(0) || out.status.code() == Some(1));
    assert!(
        !root.join("build/old.log").exists(),
        "full fix removes all matches"
    );
    assert!(!root.join("a.log").exists(), "full fix removes all matches");
}
