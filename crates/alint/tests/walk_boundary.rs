//! The walk reads ignore files up to the repository boundary, as git does: a
//! `.gitignore` in a directory above the repository (or above the linted root,
//! outside any repository) cannot hide the tree from every rule, and a bad one
//! there cannot stop the run.

use std::fmt::Write as _;
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

/// `git` in `dir`, with git's global and system config empty.
fn git(dir: &Path, args: &[&str]) {
    let config = tempfile::NamedTempFile::new().unwrap();
    let status = Command::new("git")
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", config.path())
        .env("GIT_CONFIG_SYSTEM", config.path())
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

/// A `git_tracked_only` `dir_absent` reports a directory whose tracked files
/// were deleted from the worktree whether or not an ignore file lists it: git
/// still tracks a path below it, and the tracked view lists the directories
/// above every tracked path (#503's audit).
#[test]
fn a_directory_whose_tracked_file_was_deleted_is_tracked_whether_or_not_it_is_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    write(repo, ".gitignore", "ign/\n");
    write(repo, "plain/x/a.class", "c");
    write(repo, "ign/x/a.class", "c");
    git(repo, &["init", "-q"]);
    git(repo, &["add", ".gitignore", "plain"]);
    git(repo, &["add", "-f", "ign"]);
    git(repo, &["commit", "-qm", "base"]);
    let mut config = String::from("version: 1\nrules:\n");
    for dir in ["plain", "plain/x", "ign", "ign/x"] {
        std::fs::remove_file(repo.join(dir).join("a.class")).ok();
        write!(
            config,
            "  - id: {id}\n    kind: dir_absent\n    paths: {dir}\n    git_tracked_only: true\n    \
             level: error\n",
            id = dir.replace('/', "-")
        )
        .unwrap();
    }
    for dir in ["plain/x", "ign/x"] {
        write(repo, &format!("{dir}/untracked.txt"), "u");
    }
    write(repo, ".alint.yml", &config);
    let output = check(repo);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(failing_rules(&output), ["plain", "plain-x", "ign", "ign-x"]);
}

/// The tracked view keeps the walk's path order when it adds what an ignore
/// file hid, so a rule reports a hidden directory where it sorts among the
/// walked ones, not after them.
#[test]
fn a_tracked_view_lists_hidden_paths_in_path_order() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    write(repo, ".gitignore", "a/\n");
    write(repo, "a/build/x.o", "o");
    write(repo, "z/build/y.o", "o");
    write(
        repo,
        ".alint.yml",
        "version: 1\nrules:\n  - id: no-build\n    kind: dir_absent\n    paths: \"**/build\"\n    \
         git_tracked_only: true\n    level: error\n",
    );
    git(repo, &["init", "-q"]);
    git(repo, &["add", ".gitignore", ".alint.yml", "z"]);
    git(repo, &["add", "-f", "a"]);
    git(repo, &["commit", "-qm", "base"]);
    let output = check(repo);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let paths: Vec<&str> = report["results"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|result| result["violations"].as_array().unwrap())
        .map(|violation| violation["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["a/build", "z/build"]);
}
