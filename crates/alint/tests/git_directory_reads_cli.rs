//! No rule reads a file in a Git directory, from any configuration, the
//! user's own top-level one included: `.git/config` can hold credentials,
//! such as the `http.<url>.extraheader` that `actions/checkout` persists. A
//! path a rule names there is a load error; one it reaches through a symlink
//! is refused at read. Through the real binary, with the secret checked
//! absent from everything alint prints.
//!
//! `#![cfg(unix)]`: the symlink cases.

#![cfg(unix)]

use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The base64 that the persisted header carries ("secret").
const SECRET: &str = "c2VjcmV0";

fn isolated(program: &str, dir: &Path) -> Command {
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

/// Stdout and stderr, with the human report's wrapping collapsed.
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

/// A repository whose `.git/config` holds the header `actions/checkout`
/// persists, with a `README.md` and an ordinary `real.ini`. Keep the guard.
fn checkout() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::Builder::new()
        .prefix("alint-gitread-")
        .tempdir()
        .unwrap();
    let repo = tmp.path().to_path_buf();
    for args in [
        &["init", "-q", "."][..],
        &[
            "config",
            "--local",
            "http.https://github.com/.extraheader",
            "AUTHORIZATION: basic c2VjcmV0",
        ][..],
    ] {
        let out = isolated("git", &repo).args(args).output().expect("run git");
        assert!(out.status.success(), "{out:?}");
    }
    std::fs::write(repo.join("README.md"), "x\n").unwrap();
    std::fs::write(repo.join("real.ini"), "extraheader = public\n").unwrap();
    (tmp, repo)
}

/// A `cross_file` rule comparing the header `source` extracts with
/// `README.md`, which prints both values on a mismatch.
fn leak_rule(source: &str) -> String {
    format!(
        "version: 1\nrules:\n  - id: leak\n    kind: cross_file\n    \
         source: {{ file: {source}, extract: {{ regex: 'extraheader = (.*)' }} }}\n    \
         targets: [{{ file: README.md, extract: {{ regex: '(.*)' }} }}]\n    \
         relation: equals\n    level: error\n"
    )
}

/// The audit's repro: a load error naming the option, and no secret.
#[test]
fn a_rule_that_names_the_git_configuration_is_a_load_error() {
    let (_tmp, repo) = checkout();
    std::fs::write(repo.join(".alint.yml"), leak_rule(".git/config")).unwrap();
    let out = run(&repo, &["check", "."]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        text(&out).contains(
            "rule \"leak\": `source.file` .git/config is in a Git directory; rules never read \
             there"
        ),
        "{}",
        text(&out)
    );
    assert!(!text(&out).contains(SECRET), "{}", text(&out));
}

/// Every other option a rule reads by name, in any spelling of `.git`.
#[test]
fn every_named_read_of_a_git_directory_is_a_load_error() {
    let (_tmp, repo) = checkout();
    let cases = [
        (
            "json_schema_passes",
            "paths: \"*.json\"\n    schema_path: .git/config",
            "`schema_path` .git/config",
        ),
        (
            "pair_hash",
            "source: \"*.md\"\n    target: ./.git/config",
            "`target` ./.git/config",
        ),
        (
            "registry_paths_resolve",
            "source: .GIT/config\n    extract: { regex: '(.*)' }",
            "`source` .GIT/config",
        ),
        (
            "key_parity",
            "reference: .git/config.json\n    peers: \"*.json\"",
            "`reference` .git/config.json",
        ),
        (
            "cross_file",
            "source: { file: README.md, extract: { regex: '(.*)' } }\n    \
             targets: [{ file: sub/.git/config, extract: { regex: '(.*)' } }]\n    \
             relation: equals",
            "`targets[0].file` sub/.git/config",
        ),
        (
            "key_parity",
            "reference: en.json\n    peers: .git/fr.json",
            "`peers` .git/fr.json",
        ),
        (
            "generated_file_fresh",
            "command: [\"true\"]\n    file: git~1/config",
            "`file` git~1/config",
        ),
        (
            "file_exists",
            "paths: NOTICE\n    fix: { file_create: { content_from: .git/config } }",
            "`fix.file_create.content_from` .git/config",
        ),
    ];
    for (kind, options, named) in cases {
        std::fs::write(
            repo.join(".alint.yml"),
            format!(
                "version: 1\nrules:\n  - id: r\n    kind: {kind}\n    {options}\n    level: error\n"
            ),
        )
        .unwrap();
        let out = run(&repo, &["check", "."]);
        assert_eq!(out.status.code(), Some(2), "{kind}: {}", text(&out));
        assert!(
            text(&out).contains(&format!(
                "rule \"r\": {named} is in a Git directory; rules never read there"
            )),
            "{kind}: {}",
            text(&out)
        );
        assert!(!text(&out).contains(SECRET), "{kind}: {}", text(&out));
    }
}

/// A name that reaches `.git/config` through a symlink is refused when the
/// rule reads it, an analysis failure in the same terms, and the secret
/// stays out of the report.
#[test]
fn a_named_read_through_a_symlink_into_the_git_directory_is_refused() {
    let (_tmp, repo) = checkout();
    symlink(".git/config", repo.join("cfg.ini")).unwrap();
    symlink(".git", repo.join("gitdata")).unwrap();
    for source in ["cfg.ini", "gitdata/config"] {
        std::fs::write(repo.join(".alint.yml"), leak_rule(source)).unwrap();
        let out = run(&repo, &["check", ".", "--format", "json"]);
        assert_eq!(out.status.code(), Some(1), "{source}: {}", text(&out));
        assert!(
            text(&out).contains("path is in a Git directory; rules never read there"),
            "{source}: {}",
            text(&out)
        );
        assert!(!text(&out).contains(SECRET), "{source}: {}", text(&out));
    }
    // The same through `confine_read` (`json_schema_passes`), and through
    // `relation: identical`, which reads both files whole.
    std::fs::write(repo.join("a.json"), "{}\n").unwrap();
    for rule in [
        "kind: json_schema_passes\n    paths: a.json\n    schema_path: cfg.ini",
        "kind: cross_file\n    source: { file: cfg.ini }\n    \
         targets: [{ file: README.md }]\n    relation: identical",
    ] {
        std::fs::write(
            repo.join(".alint.yml"),
            format!("version: 1\nrules:\n  - id: r\n    {rule}\n    level: error\n"),
        )
        .unwrap();
        let out = run(&repo, &["check", "."]);
        assert!(
            text(&out).contains("cfg.ini is in a Git directory; rules never read there"),
            "{rule}: {}",
            text(&out)
        );
        assert!(!text(&out).contains(SECRET), "{rule}: {}", text(&out));
    }
}

/// An ordinary named read, a gitignored one included, still works.
#[test]
fn an_ordinary_named_read_still_works() {
    let (_tmp, repo) = checkout();
    std::fs::write(repo.join(".gitignore"), "secret.ini\n").unwrap();
    std::fs::write(repo.join("secret.ini"), "extraheader = x\n").unwrap();
    symlink("real.ini", repo.join("alias.ini")).unwrap();
    for (source, value) in [
        ("real.ini", "public"),
        ("alias.ini", "public"),
        ("secret.ini", "x"),
    ] {
        std::fs::write(repo.join(".alint.yml"), leak_rule(source)).unwrap();
        let out = run(&repo, &["check", "."]);
        assert_eq!(out.status.code(), Some(1), "{source}: {}", text(&out));
        assert!(
            text(&out).contains(&format!("{source} value \"{value}\"")),
            "{source}: {}",
            text(&out)
        );
    }
}
