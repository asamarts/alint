//! Every Git command alint runs names its repository (`--git-dir` and
//! `--work-tree`), so Git never discovers one: a directory shaped like a bare
//! repository (`HEAD`, `objects/`, `refs/`, a `config` naming a
//! `core.fsmonitor` command) stays content whatever the Git version.
//! `safe.bareRepository=explicit`, which alint also passes, does nothing
//! before Git 2.38.
//!
//! A stub `git` first on `PATH` logs each call, marking one that names no
//! repository `UNNAMED`, then runs the real Git with `safe.bareRepository`
//! taken out of its arguments, as a Git older than 2.38 would ignore it.
//!
//! `#![cfg(unix)]`: the stub is a `sh` script.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A stub `git` in its own directory, and the log it writes.
struct StubGit {
    _dir: tempfile::TempDir,
    bin: PathBuf,
    log: PathBuf,
}

impl StubGit {
    fn new() -> Self {
        let real = Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .expect("run sh");
        assert!(real.status.success(), "git is on PATH");
        let real = String::from_utf8(real.stdout).unwrap().trim().to_string();
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let log = dir.path().join("git.log");
        let script = format!(
            r#"#!/bin/sh
named=
for arg do
  case $arg in --git-dir=*) named=1 ;; esac
done
[ -n "${{GIT_DIR+set}}" ] && named=1
{{ printf '%s' "${{named:-UNNAMED}}"; for arg do printf ' %s' "$arg"; done; printf '\n'; }} >> '{log}'
# A Git older than 2.38 ignores safe.bareRepository.
for arg do
  shift
  case $arg in safe.bareRepository=*) arg=alint.stub.ignored=1 ;; esac
  set -- "$@" "$arg"
done
exec '{real}' "$@"
"#,
            log = log.display(),
        );
        let stub = bin.join("git");
        std::fs::write(&stub, script).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            _dir: dir,
            bin,
            log,
        }
    }

    /// Runs alint in `dir` with the stub first on `PATH` and the host's Git
    /// configuration left out.
    fn alint(&self, dir: &Path, args: &[&str]) -> Output {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let path = std::env::join_paths(
            std::iter::once(self.bin.clone()).chain(std::env::split_paths(&path)),
        )
        .unwrap();
        Command::new(env!("CARGO_BIN_EXE_alint"))
            .args(args)
            .current_dir(dir)
            .env("PATH", path)
            .env("NO_COLOR", "1")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("spawn alint")
    }

    /// Every call the stub logged, one line each.
    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The calls that named no repository.
    fn unnamed(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|call| call.starts_with("UNNAMED"))
            .collect()
    }
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// The real Git, with the host's configuration left out.
fn real_git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(output.status.success(), "git {args:?}: {output:?}");
}

/// The files of a directory shaped like a bare repository whose `config`
/// has Git write `canary` when it reads the index.
fn bare_shaped(dir: &Path, canary: &Path) {
    std::fs::create_dir_all(dir.join("objects")).unwrap();
    std::fs::create_dir_all(dir.join("refs")).unwrap();
    std::fs::write(dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(
        dir.join("config"),
        format!(
            "[core]\n\trepositoryformatversion = 0\n\tbare = false\n\tworktree = .\n\t\
             fsmonitor = \"printf ran > '{}'; false\"\n",
            canary.display()
        ),
    )
    .unwrap();
}

const GIT_RULES: &str = "version: 1\nrules:\n  - id: denied\n    kind: git_no_denied_paths\n    \
     denied: [\"*.secret\"]\n    level: error\n  - id: a-is-kept\n    kind: file_exists\n    \
     paths: a.txt\n    git_tracked_only: true\n    level: error\n";

/// In a repository, alint's Git reads it, every command naming it, from the
/// root and from a subdirectory, with and without `--changed`.
#[test]
fn every_git_command_names_the_repository() {
    let stub = StubGit::new();
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("sub")).unwrap();
    std::fs::write(repo.join(".alint.yml"), GIT_RULES).unwrap();
    std::fs::write(repo.join("a.txt"), "x\n").unwrap();
    std::fs::write(repo.join("leak.secret"), "x\n").unwrap();
    std::fs::write(repo.join("sub/b.txt"), "x\n").unwrap();
    real_git(&repo, &["init", "-q", "-b", "main"]);
    real_git(&repo, &["add", "."]);
    real_git(&repo, &["commit", "-q", "-m", "init"]);
    std::fs::write(repo.join("sub/b.txt"), "y\n").unwrap();

    let whole = stub.alint(&repo, &["check"]);
    let output = text(&whole);
    assert_eq!(whole.status.code(), Some(1), "{output}");
    assert!(
        output.contains("leak.secret"),
        "Git read the index: {output}"
    );
    assert!(!output.contains("a-is-kept"), "a.txt is tracked: {output}");
    let changed = stub.alint(&repo.join("sub"), &["check", "--changed", ".."]);
    assert!(
        changed.status.code().is_some_and(|code| code <= 1),
        "{}",
        text(&changed)
    );

    assert!(!stub.calls().is_empty(), "alint ran Git");
    assert_eq!(stub.unnamed(), Vec::<String>::new());
}

/// A root shaped like a bare repository, in no repository: alint runs no Git
/// command, so its `config` names no command Git runs; the Git rules read as
/// they do outside a repository.
#[test]
fn a_bare_shaped_root_runs_no_command_its_config_names() {
    let stub = StubGit::new();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let canary = tmp.path().join("canary");
    bare_shaped(&root, &canary);
    std::fs::write(root.join(".alint.yml"), GIT_RULES).unwrap();
    std::fs::write(root.join("a.txt"), "x\n").unwrap();

    for args in [&["check"][..], &["check", "--changed"]] {
        let out = stub.alint(&root, args);
        assert!(
            !canary.exists(),
            "{args:?} ran the fsmonitor: {}",
            text(&out)
        );
    }
    assert_eq!(stub.unnamed(), Vec::<String>::new());
}

/// `alint test` with `given.git`: the runner's own Git (`init`, `add`,
/// `commit`) and the check's both name the repository the runner made.
#[test]
fn a_policy_tests_git_names_its_repository() {
    let stub = StubGit::new();
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("alint-tests")).unwrap();
    std::fs::write(
        tmp.path().join("alint-tests/git.yml"),
        "test_version: 1\nname: git names its repository\ngiven:\n  tree:\n    a.txt: \"x\"\n    \
         leak.secret: \"x\"\n  git:\n    init: true\n    add: [a.txt, leak.secret]\n    \
         commit: true\n  config: |\n    version: 1\n    rules:\n      - id: denied\n        \
         kind: git_no_denied_paths\n        denied: [\"*.secret\"]\n        level: error\n\
         when: [check]\nexpect:\n  - exit: policy_failure\n    violations:\n      \
         - { rule: denied, path: leak.secret }\n",
    )
    .unwrap();
    let out = stub.alint(tmp.path(), &["test"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let calls = stub.calls();
    assert!(
        calls.iter().any(|call| call.contains(" init ")),
        "the runner made the repository: {calls:#?}"
    );
    assert_eq!(stub.unnamed(), Vec::<String>::new());
}

/// `alint test` without `given.git` runs no Git at all, so a fixture shaped
/// like a bare repository names no command Git runs.
#[test]
fn a_policy_test_without_git_runs_none() {
    let stub = StubGit::new();
    let tmp = tempfile::tempdir().unwrap();
    let canary = tmp.path().join("canary");
    std::fs::create_dir_all(tmp.path().join("alint-tests")).unwrap();
    std::fs::write(
        tmp.path().join("alint-tests/bare.yml"),
        format!(
            "test_version: 1\nname: a bare-shaped fixture stays content\ngiven:\n  tree:\n    \
             HEAD: \"ref: refs/heads/main\\n\"\n    objects: {{}}\n    refs: {{}}\n    \
             config: |\n      [core]\n      \trepositoryformatversion = 0\n      \tbare = false\n      \
             \tworktree = .\n      \tfsmonitor = \"printf ran > '{canary}'; false\"\n    \
             a.txt: \"x\"\n  config: |\n    {rules}\nwhen: [check]\nexpect:\n  \
             - exit: success\n    violations: []\n",
            canary = canary.display(),
            rules = "version: 1\n    rules:\n      - id: denied\n        kind: git_no_denied_paths\n        \
                     denied: [\"*.secret\"]\n        level: error\n      - id: tracked\n        \
                     kind: file_absent\n        paths: a.txt\n        git_tracked_only: true\n        \
                     level: error",
        ),
    )
    .unwrap();
    let out = stub.alint(tmp.path(), &["test"]);
    assert!(!canary.exists(), "the fsmonitor ran: {}", text(&out));
    assert!(
        text(&out).contains("a bare-shaped fixture stays content"),
        "{}",
        text(&out)
    );
    assert_eq!(stub.calls(), Vec::<String>::new());
}
