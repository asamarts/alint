//! The walk reads ignore files up to the repository boundary, as git does: a
//! `.gitignore` in a directory above the repository (or above the linted root,
//! outside any repository) cannot hide the tree from every rule, and a bad one
//! there cannot stop the run.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn alint() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// `alint` with git's global and system config, and the excludes files
/// `ignore` falls back to, pointed into an empty `home`, so a developer's
/// global excludes file (`.env` is a common entry) cannot change the walk.
fn isolated_alint(home: &Path) -> Command {
    let config = home.join("gitconfig");
    std::fs::write(&config, "").unwrap();
    let mut command = Command::new(alint());
    command
        .env("GIT_CONFIG_GLOBAL", &config)
        .env("GIT_CONFIG_SYSTEM", &config)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("HOME", home)
        .env("USERPROFILE", home);
    command
}

fn check(root: &Path) -> Output {
    let home = tempfile::tempdir().unwrap();
    isolated_alint(home.path())
        .args(["check", "--format", "json"])
        .current_dir(root)
        .output()
        .expect("spawn alint")
}

const CONFIG: &str = "version: 1\nrules:\n  - id: no-env\n    kind: file_absent\n    \
                      paths: .env\n    level: error\n";

/// The rule ids that reported a violation.
fn failing_rules(output: &Output) -> Vec<String> {
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    report["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|result| result["passed"] == false)
        .map(|result| result["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_gitignore_above_the_repository_does_not_hide_its_files() {
    // A home directory kept in git with `*` in its `.gitignore`. The walk used
    // to apply it to every repository below, index nothing and pass every rule.
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    write(home, ".gitignore", "*\n");
    write(home, ".git/info/exclude", ".env\n");
    let repo = home.join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    write(&repo, ".alint.yml", CONFIG);
    write(&repo, ".env", "TOKEN=x\n");

    let output = check(&repo);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(failing_rules(&output), ["no-env"]);

    // The repository's own `.gitignore` still applies.
    write(&repo, ".gitignore", ".env\n");
    let output = check(&repo);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}

#[test]
fn outside_a_repository_the_linted_root_is_the_boundary() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), ".gitignore", "*\n");
    let tree = tmp.path().join("unpacked");
    write(&tree, ".alint.yml", CONFIG);
    write(&tree, ".env", "TOKEN=x\n");

    let output = check(&tree);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(failing_rules(&output), ["no-env"]);
}

#[test]
fn a_bad_ignore_file_above_the_root_does_not_stop_the_run() {
    // `ignore` reads the ignore files above the root, applied or not, and a
    // line it cannot parse there used to end the run with a walk error (exit
    // 2). git never reads them; the same line in the repository's own
    // `.gitignore` was already skipped.
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    write(home, ".gitignore", "a\\\n");
    write(home, ".git/info/exclude", "x{\n");
    let repo = home.join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    write(&repo, ".alint.yml", CONFIG);
    write(&repo, ".env", "TOKEN=x\n");
    let output = check(&repo);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(failing_rules(&output), ["no-env"]);

    let tree = home.join("unpacked");
    write(home, ".gitignore", "[z-a]\n");
    write(&tree, ".alint.yml", CONFIG);
    write(&tree, ".env", "TOKEN=x\n");
    let output = check(&tree);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(failing_rules(&output), ["no-env"]);
}
