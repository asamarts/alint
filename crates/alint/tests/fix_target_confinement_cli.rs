//! Where `alint fix` writes, end to end through the real binary: never into a
//! Git directory through a symlink the repository commits, never alint's own
//! configuration for a ruleset reached through `extends:` (which may not run
//! commands, while a configuration file may), and never outside the root for
//! one, whatever the user's `allow_out_of_root` says. Each case plants a
//! canary that only a command run through the written file would create.
//!
//! `#![cfg(unix)]`: the vectors are symlinks and `sh` commands.

#![cfg(unix)]

use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A command with the host's Git configuration left out, so only the
/// repository's own `.git/config` can name a command.
fn isolated(program: impl AsRef<std::ffi::OsStr>, dir: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
    command
}

fn run(dir: &Path, args: &[&str]) -> Output {
    isolated(env!("CARGO_BIN_EXE_alint"), dir)
        .args(args)
        .output()
        .expect("spawn alint")
}

/// Stdout and stderr, with whitespace runs (the human report's wrapping)
/// collapsed to single spaces.
fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
}

/// `base/{repo,canary}`, with `repo` a Git repository. Keep the guard alive.
fn git_repo() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let base = tempfile::Builder::new()
        .prefix("alint-fixtarget-")
        .tempdir()
        .unwrap();
    let repo = base.path().join("repo");
    let canary = base.path().join("canary");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&canary).unwrap();
    let init = isolated("git", &repo)
        .args(["init", "-q", "."])
        .output()
        .expect("run git");
    assert!(init.status.success(), "{init:?}");
    (base, repo, canary)
}

fn canaries(canary: &Path) -> Vec<String> {
    std::fs::read_dir(canary)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

// ─── SEC-2: a content fix never writes through a symlink into `.git` ──────

/// An untrusted repository commits `src/evil.rs -> ../.git/config` and a
/// `HEADER.txt` holding a `core.fsmonitor`. The user's own `file_header` rule
/// on `src/**/*.rs` prepends `HEADER.txt`. Before the fix, `alint fix` wrote
/// the header into `.git/config`, and the Git command its `git_tracked_only`
/// rule runs in the same run ran the attacker's command. A symlink to an
/// ordinary file is still fixed through.
#[test]
fn a_content_fix_never_writes_through_a_symlink_into_the_git_directory() {
    let (_base, repo, canary) = git_repo();
    std::fs::create_dir(repo.join("src")).unwrap();
    symlink("../.git/config", repo.join("src/evil.rs")).unwrap();
    std::fs::write(repo.join("src/real.rs"), "fn f() {}\n").unwrap();
    symlink("real.rs", repo.join("src/alias.rs")).unwrap();
    std::fs::write(
        repo.join("HEADER.txt"),
        format!(
            "[core]\n\tfsmonitor = \"printf ran > '{}/fsmonitor'; false\"\n# SPDX\n",
            canary.display()
        ),
    )
    .unwrap();
    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nrules:\n\
         - {id: header, kind: file_header, paths: \"src/**/*.rs\", pattern: SPDX, \
         level: error, fix: {file_prepend: {content_from: HEADER.txt}}}\n\
         - {id: tracked, kind: file_exists, paths: HEADER.txt, git_tracked_only: true, \
         level: warning}\n",
    )
    .unwrap();
    let before = std::fs::read(repo.join(".git/config")).unwrap();

    let out = run(&repo, &["fix", "."]);
    assert_eq!(
        std::fs::read(repo.join(".git/config")).unwrap(),
        before,
        "the fix wrote .git/config:\n{}",
        text(&out)
    );
    assert_eq!(canaries(&canary), Vec::<String>::new(), "{}", text(&out));
    assert!(!text(&out).contains("src/evil.rs"), "{}", text(&out));
    // The alias was fixed through; it is still a link to the file it named.
    assert!(
        std::fs::read_to_string(repo.join("src/real.rs"))
            .unwrap()
            .contains("# SPDX"),
        "{}",
        text(&out)
    );
    assert!(
        std::fs::symlink_metadata(repo.join("src/alias.rs"))
            .unwrap()
            .is_symlink()
    );

    // A second run (Git reads the configuration again) still runs nothing.
    let again = run(&repo, &["check", "."]);
    assert_eq!(canaries(&canary), Vec::<String>::new(), "{}", text(&again));
}

/// A directory link `gitdata -> .git` no longer indexes `gitdata/config`,
/// `gitdata/HEAD` and the rest, and a link back to an ancestor of `.git`
/// (`sub/up -> ..`) reaches nothing in it either: a `file_absent` on
/// `**/config` and `**/HEAD` stays silent, and `no_symlinks` still sees the
/// pruned link.
#[test]
fn a_symlink_into_the_git_directory_is_not_walked() {
    let (_base, repo, _canary) = git_repo();
    symlink(".git", repo.join("gitdata")).unwrap();
    std::fs::create_dir(repo.join("sub")).unwrap();
    symlink("..", repo.join("sub/up")).unwrap();
    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nrules:\n\
         - {id: no-git-files, kind: file_absent, paths: [\"**/config\", \"**/HEAD\"], \
         level: error, fix: {file_remove: {applicability: safe}}}\n\
         - {id: links, kind: no_symlinks, paths: \"gitdata\", level: warning}\n",
    )
    .unwrap();

    let out = run(&repo, &["check", ".", "--format", "json"]);
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let flagged: Vec<(String, String)> = report["results"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|result| {
            let rule = result["id"].as_str().unwrap_or_default().to_string();
            result["violations"]
                .as_array()
                .unwrap()
                .iter()
                .map(move |v| {
                    (
                        rule.clone(),
                        v["path"].as_str().unwrap_or_default().to_string(),
                    )
                })
        })
        .collect();
    assert_eq!(
        flagged,
        [("links".to_string(), "gitdata".to_string())],
        "{}",
        text(&out)
    );
    let fixed = run(&repo, &["fix", "."]);
    assert!(repo.join(".git/config").is_file(), "{}", text(&fixed));
    assert!(repo.join(".git/HEAD").is_file(), "{}", text(&fixed));
}

// ─── SEC-1: a fix from `extends:` never writes alint's configuration ──────

/// `.alint.yml` extends a vendored ruleset, which may not declare a
/// `kind: command` rule. Before the fix, its `file_create` wrote
/// `.alint.d/housekeeping.yml` holding one, and the next `alint check` ran it.
#[test]
fn an_extended_ruleset_cannot_create_a_drop_in() {
    let (_base, repo, canary) = git_repo();
    std::fs::create_dir(repo.join("vendor")).unwrap();
    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nextends: [./vendor/policy.yml]\nrules: []\n",
    )
    .unwrap();
    std::fs::write(
        repo.join("vendor/policy.yml"),
        format!(
            "version: 1\nrules:\n  - id: housekeeping\n    kind: file_exists\n    \
             paths: .alint.d/housekeeping.yml\n    level: warning\n    fix:\n      \
             file_create:\n        create_parents: true\n        content: |\n          \
             version: 1\n          rules:\n            - id: run\n              \
             kind: command\n              paths: .alint.yml\n              \
             command: [\"sh\", \"-c\", \"printf ran > '{}/check'\"]\n              \
             level: warning\n",
            canary.display()
        ),
    )
    .unwrap();

    let out = run(&repo, &["fix", "."]);
    assert!(!repo.join(".alint.d").exists(), "{}", text(&out));
    assert!(
        text(&out).contains(
            ".alint.d/housekeeping.yml is an alint configuration file; a fix from an \
             extended ruleset never writes one"
        ),
        "{}",
        text(&out)
    );
    let check = run(&repo, &["check", "."]);
    assert_eq!(canaries(&canary), Vec::<String>::new(), "{}", text(&check));
}

/// The same through a content fix: a `file_content_matches` rule on the
/// top-level `.alint.yml` whose `file_append` adds a `rules:` block.
#[test]
fn an_extended_ruleset_cannot_append_to_the_configuration() {
    let (_base, repo, canary) = git_repo();
    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nextends: [./policy.yml]\n",
    )
    .unwrap();
    std::fs::write(
        repo.join("policy.yml"),
        format!(
            "version: 1\nrules:\n  - id: append\n    kind: file_content_matches\n    \
             paths: .alint.yml\n    pattern: \"^rules:\"\n    level: warning\n    \
             fix:\n      file_append:\n        content: |\n          rules:\n            \
             - id: run\n              kind: command\n              paths: .alint.yml\n              \
             command: [\"sh\", \"-c\", \"printf ran > '{}/check'\"]\n              \
             level: warning\n",
            canary.display()
        ),
    )
    .unwrap();
    let before = std::fs::read(repo.join(".alint.yml")).unwrap();

    let out = run(&repo, &["fix", "."]);
    assert_eq!(
        std::fs::read(repo.join(".alint.yml")).unwrap(),
        before,
        "{}",
        text(&out)
    );
    assert!(
        text(&out).contains(
            ".alint.yml is an alint configuration file; a fix from an extended ruleset \
             never writes one"
        ),
        "{}",
        text(&out)
    );
    let check = run(&repo, &["check", "."]);
    assert_eq!(canaries(&canary), Vec::<String>::new(), "{}", text(&check));
}

/// The same for a policy test: `alint test` reads every YAML document below
/// `alint-tests/`, and a test may run a command rule (with `--allow-spawn`).
/// Before the fix, a `file_create` from an extended ruleset wrote one there.
#[test]
fn an_extended_ruleset_cannot_create_a_policy_test() {
    let (_base, repo, canary) = git_repo();
    let policy_test = format!(
        "test_version: 1\nname: housekeeping\ncapabilities:\n  spawn: true\ngiven:\n  \
         tree:\n    input.txt: \"x\\n\"\n  config: |\n    version: 1\n    rules:\n      \
         - id: run\n        kind: command\n        paths: input.txt\n        \
         command: [\"sh\", \"-c\", \"printf ran > '{}/test'\"]\n        level: error\n\
         when: [check]\nexpect:\n  - exit: success\n    violations: []\n",
        canary.display()
    );
    let content: String = policy_test
        .lines()
        .flat_map(|line| ["          ", line, "\n"])
        .collect();
    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nextends: [./policy.yml]\nrules: []\n",
    )
    .unwrap();
    for target in ["alint-tests/housekeeping.yml", "ALINT-~1/housekeeping.yml"] {
        std::fs::write(
            repo.join("policy.yml"),
            format!(
                "version: 1\nrules:\n  - id: housekeeping\n    kind: file_exists\n    \
                 paths: {target}\n    level: warning\n    fix:\n      file_create:\n        \
                 create_parents: true\n        content: |\n{content}"
            ),
        )
        .unwrap();

        let out = run(&repo, &["fix", "."]);
        assert!(!repo.join(target).exists(), "{target}: {}", text(&out));
        assert!(
            text(&out).contains(&format!(
                "{target} is an alint configuration file; a fix from an extended ruleset \
                 never writes one"
            )),
            "{target}: {}",
            text(&out)
        );
    }
    let test = run(&repo, &["test", "--allow-spawn"]);
    assert_eq!(canaries(&canary), Vec::<String>::new(), "{}", text(&test));
}

/// The user's own top-level rule still writes a drop-in.
#[test]
fn a_top_level_rule_still_creates_a_drop_in() {
    let (_base, repo, _canary) = git_repo();
    std::fs::write(repo.join("policy.yml"), "version: 1\nrules: []\n").unwrap();
    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nextends: [./policy.yml]\nrules:\n\
         - {id: team, kind: file_exists, paths: .alint.d/x.yml, level: warning, \
         fix: {file_create: {create_parents: true, content: \"version: 1\\n\"}}}\n",
    )
    .unwrap();

    let out = run(&repo, &["fix", "."]);
    assert_eq!(
        std::fs::read_to_string(repo.join(".alint.d/x.yml")).unwrap(),
        "version: 1\n",
        "{}",
        text(&out)
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
}

// ─── SEC-3: `allow_out_of_root` never lets an extended ruleset write out ──

/// The user's `allow_out_of_root: true` still lets their own `file_create`
/// write outside the root; one from a ruleset they extend never does, and the
/// report says it was refused. The user's own write, which no re-check of the
/// root can see, is reported `unresolved`, never `skipped`.
#[test]
fn allow_out_of_root_never_lets_an_extended_ruleset_write_outside() {
    let base = tempfile::tempdir().unwrap();
    let repo = base.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(base.path().join("outside")).unwrap();
    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nallow_out_of_root: true\nextends: [./v.yml]\nrules:\n\
         - {id: own, kind: file_exists, paths: ../outside/mine.txt, level: error, \
         fix: {file_create: {content: \"mine\\n\"}}}\n",
    )
    .unwrap();
    std::fs::write(
        repo.join("v.yml"),
        "version: 1\nrules:\n\
         - {id: plant, kind: file_exists, paths: ../outside/planted.sh, level: error, \
         fix: {file_create: {content: \"echo planted\\n\"}}}\n",
    )
    .unwrap();

    let out = run(&repo, &["fix", "."]);
    assert!(
        !base.path().join("outside/planted.sh").exists(),
        "{}",
        text(&out)
    );
    assert!(
        text(&out).contains(
            "../outside/planted.sh escapes the repo root; a fix from an extended ruleset \
             never writes outside it"
        ),
        "{}",
        text(&out)
    );
    assert!(
        !text(&out).contains("(skipped: fix ran"),
        "a written file was reported skipped: {}",
        text(&out)
    );
    assert!(
        text(&out).contains("(unresolved: fix ran (created ../outside/mine.txt)"),
        "{}",
        text(&out)
    );
    assert!(
        text(&out).contains("0 applied, 1 skipped, 0 unfixable, 1 unresolved."),
        "{}",
        text(&out)
    );
    assert_eq!(
        std::fs::read_to_string(base.path().join("outside/mine.txt")).unwrap(),
        "mine\n"
    );
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
}

/// The read half of SEC-3: under the user's `allow_out_of_root: true`, a
/// `file_create` from a ruleset they extend read its `content_from` outside
/// the root and copied the file into the repository. It is refused now; the
/// user's own rule still reads there.
#[test]
fn allow_out_of_root_never_lets_an_extended_ruleset_read_outside() {
    let base = tempfile::tempdir().unwrap();
    let repo = base.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(base.path().join("secret"), "TOKEN=hunter2\n").unwrap();
    for (name, fix) in [
        ("v.yml", "{file_create: {content_from: ../secret}}"),
        ("w.yml", "{file_create: {content_from: ./../secret}}"),
    ] {
        std::fs::write(
            repo.join(name),
            format!(
                "version: 1\nrules:\n\
                 - {{id: copy-{name}, kind: file_exists, paths: stolen-{name}, level: error, \
                 fix: {fix}}}\n"
            ),
        )
        .unwrap();
    }
    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nallow_out_of_root: true\nextends: [./v.yml, ./w.yml]\nrules:\n\
         - {id: own, kind: file_exists, paths: mine.txt, level: error, \
         fix: {file_create: {content_from: ../secret}}}\n",
    )
    .unwrap();

    let out = run(&repo, &["fix", "."]);
    for name in ["v.yml", "w.yml"] {
        assert!(
            !repo.join(format!("stolen-{name}")).exists(),
            "{}",
            text(&out)
        );
    }
    assert!(
        text(&out).contains(
            "../secret escapes the repo root; a fix from an extended ruleset never reads \
             outside it"
        ),
        "{}",
        text(&out)
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("mine.txt")).unwrap(),
        "TOKEN=hunter2\n",
        "{}",
        text(&out)
    );
}
