//! A policy test from a package the user does not trust stays in its sandbox:
//! without `capabilities: { spawn: true }` and `--allow-spawn` it runs no
//! command, writes nothing outside its sandbox and reads none of the host's
//! environment or files. Each test arms a canary outside the sandbox (a file
//! a command would write, a directory a write would land in) and checks that
//! it stays untouched.
//!
//! A test that needs a host variable set (`TMPDIR`, `PATH`, `HOME`, a host
//! secret) runs its body in a child process of this test binary, since Rust
//! 2024 makes `set_var` unsafe and this workspace forbids `unsafe`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use alint_testkit::RunnerOptions;
use alint_testkit::{PolicyTest, Runner};

/// Set in the child process: the directory the parent prepared for it.
const CHILD_DIRECTORY: &str = "ALINT_TESTKIT_SANDBOX_CHILD";

fn policy_file(package: &Path, name: &str, yaml: &str) -> (PathBuf, PolicyTest) {
    let source = package.join(name);
    std::fs::write(&source, yaml).expect("write policy test");
    let test = PolicyTest::from_yaml(yaml).expect("parse policy test");
    (source, test)
}

/// Run this binary's test `name` again in a child process, with `directory`
/// in [`CHILD_DIRECTORY`] and `environment` set, and require that it passes.
fn run_in_child(name: &str, directory: &Path, environment: &[(&str, OsString)], unset: &[&str]) {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(CHILD_DIRECTORY, directory);
    for (variable, value) in environment {
        command.env(variable, value);
    }
    for variable in unset {
        command.env_remove(variable);
    }
    let output = command.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "child {name} failed:\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
fn entries(directory: &Path) -> Vec<OsString> {
    let mut names: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    names
}

/// Run every policy test in `package` (its `*.yml` files, by name) with
/// `runner`, whatever each one's outcome.
#[cfg(unix)]
fn run_package(runner: &Runner, package: &Path) {
    let mut sources: Vec<_> = std::fs::read_dir(package)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "yml"))
        .collect();
    sources.sort();
    for source in sources {
        let test = PolicyTest::from_yaml(&std::fs::read_to_string(&source).unwrap()).unwrap();
        let _ = runner.preflight(&test, &source);
        let _ = runner.run(&test, &source);
    }
}

/// An inline tree whose symlink the text reads as in the tree, but which the
/// file system resolves two levels above the repository root (`a/c/t` leads
/// to `b`, so `a/c/t/../../..` is above the root, not at it), must not write
/// there, and must not plant a `git` the runner then runs from `PATH`.
#[cfg(unix)]
#[test]
fn an_inline_symlink_through_a_symlink_writes_nothing_outside_the_sandbox() {
    if let Some(directory) = std::env::var_os(CHILD_DIRECTORY) {
        run_package(
            &Runner::default(),
            &PathBuf::from(directory).join("package"),
        );
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let host_temp = directory.path().join("tmp");
    let escape = host_temp.join("rev522-escape-target");
    let canary = directory.path().join("canary");
    let package = directory.path().join("package");
    for path in [&escape, &canary, &package] {
        std::fs::create_dir_all(path).unwrap();
    }
    let tree = r#"    a: { c: { t: { "$symlink": "../../b" } } }
    b: {}
    rev522-escape-target: {}
    s: { "$symlink": "a/c/t/../../../rev522-escape-target" }"#;
    std::fs::write(
        package.join("escape.yml"),
        format!(
            r#"test_version: 1
name: a write through the link
given:
  tree:
{tree}
    "s/pwned.txt": "written outside the sandbox\n"
  config: "version: 1\nrules: []\n"
when: [check]
expect:
  - exit: success
    violations: []
"#
        ),
    )
    .unwrap();
    std::fs::write(
        package.join("planted-git.yml"),
        format!(
            r##"test_version: 1
name: a git planted through the link
given:
  tree:
{tree}
    "s/git": {{ "$exec": "#!/bin/sh\nprintf ran > '{canary}/git.txt'\n" }}
    a.txt: "x"
  git:
    init: true
    add: [a.txt]
  config: |
    version: 1
    rules:
      - id: tracked
        kind: file_exists
        paths: a.txt
        git_tracked_only: true
        level: error
when: [check]
expect:
  - exit: success
    violations: []
"##,
            canary = canary.display()
        ),
    )
    .unwrap();

    let path = std::env::join_paths(
        std::iter::once(escape.clone())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    run_in_child(
        "policy_runner_sandbox::an_inline_symlink_through_a_symlink_writes_nothing_outside_the_sandbox",
        directory.path(),
        &[("TMPDIR", host_temp.into_os_string()), ("PATH", path)],
        &[],
    );
    assert_eq!(entries(&escape), Vec::<OsString>::new());
    assert_eq!(entries(&canary), Vec::<OsString>::new());
}

/// The rules of a `fix` step that create a repository the next step's Git
/// reads run a command from: `core.fsmonitor` in a configuration it writes.
#[cfg(unix)]
fn git_directory_fix_rules(files: &[(&str, &str)]) -> String {
    use std::fmt::Write as _;
    let mut rules = String::new();
    for (index, (path, content)) in files.iter().enumerate() {
        write!(
            rules,
            r#"      - id: create-{index}
        kind: file_exists
        paths: ["{path}"]
        level: error
        fix:
          file_create:
            path: "{path}"
            content: {content:?}
            create_parents: true
"#
        )
        .unwrap();
    }
    rules
}

/// A `fix` step must not write a repository into the sandbox: not into
/// `.git` (`HEAD`, `config`, `objects`, `refs`), not as a `.git` file naming a
/// directory the tree holds, and not as a `.git/commondir` beside the
/// repository `given.git` made. Each would have the next step's Git read a
/// `config` that names a command, which ran with no `spawn` capability.
#[cfg(unix)]
#[test]
fn a_fix_cannot_make_git_run_a_command() {
    let canary_dir = tempfile::tempdir().unwrap();
    let canary = canary_dir.path().display().to_string();
    let config = format!(
        "[core]\n\trepositoryformatversion = 0\n\tbare = false\n\tfsmonitor = \"printf ran > '{canary}/fsmonitor.txt'; false\"\n"
    );
    let head = "ref: refs/heads/main\n";
    let variants = [
        (
            "a .git directory",
            String::new(),
            git_directory_fix_rules(&[
                (".git/HEAD", head),
                (".git/config", &config),
                (".git/objects/keep", ""),
                (".git/refs/keep", ""),
            ]),
        ),
        (
            "a .git file",
            format!(
                "    gitdata:\n      HEAD: {head:?}\n      config: {config:?}\n      objects: {{ keep: \"\" }}\n      refs: {{ keep: \"\" }}\n"
            ),
            git_directory_fix_rules(&[(".git", "gitdir: gitdata\n")]),
        ),
        (
            "a .git/commondir",
            format!(
                "    common:\n      config: {config:?}\n      objects: {{ keep: \"\" }}\n      refs: {{ keep: \"\" }}\n"
            ),
            git_directory_fix_rules(&[(".git/commondir", "../common\n")]),
        ),
    ];
    for (variant, tree, rules) in variants {
        let git = if variant == "a .git/commondir" {
            "  git:\n    init: true\n    add: [a.txt]\n"
        } else {
            ""
        };
        let package = tempfile::tempdir().unwrap();
        let yaml = format!(
            r#"test_version: 1
name: {variant}
given:
  tree:
    a.txt: "x"
{tree}{git}  config: |
    version: 1
    rules:
{rules}      - id: tracked
        kind: file_exists
        paths: a.txt
        git_tracked_only: true
        level: error
when: [fix, check]
expect:
  - exit: policy_failure
    applied: []
  - exit: policy_failure
    violations: []
"#
        );
        let (source, test) = policy_file(package.path(), "fix.yml", &yaml);

        Runner::default().preflight(&test, &source).unwrap();
        let run = Runner::default().run(&test, &source).unwrap();
        assert!(
            run.observations[0].applied.is_empty(),
            "{variant}: {:#?}",
            run.observations[0]
        );
        assert_eq!(
            entries(canary_dir.path()),
            Vec::<OsString>::new(),
            "{variant}"
        );
    }
}

/// Without `given.git` the repository has no `.git`, and Git's discovery
/// must stop at the sandbox rather than read a repository the host's
/// temporary directory is in: the `git_commit_message` rule reports that
/// the root is not a repository, not the host's HEAD.
#[cfg(unix)]
#[test]
fn git_discovery_stops_at_the_sandbox() {
    if let Some(directory) = std::env::var_os(CHILD_DIRECTORY) {
        let package = PathBuf::from(directory).join("package");
        let (source, test) = policy_file(
            &package,
            "discovery.yml",
            r#"test_version: 1
name: no repository above the sandbox
given:
  tree:
    a.txt: "x"
  config: |
    version: 1
    rules:
      - id: msg
        kind: git_commit_message
        subject_max_length: 5
        level: error
when: [check]
expect:
  - exit: policy_failure
    violations: []
    analysis_failures:
      - { rule: msg, message: { regex: "not a usable Git repository" } }
"#,
        );
        let run = Runner::default().run(&test, &source).unwrap();
        assert!(run.passed(), "{:#?}", run.mismatches);
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let host = directory.path().join("host");
    let host_temp = host.join("tmp");
    std::fs::create_dir_all(&host_temp).unwrap();
    std::fs::create_dir(directory.path().join("package")).unwrap();
    for args in [
        &["init", "-q"][..],
        &[
            "-c",
            "user.name=host",
            "-c",
            "user.email=host@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "a host commit subject the test must not read",
        ][..],
    ] {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&host)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }

    run_in_child(
        "policy_runner_sandbox::git_discovery_stops_at_the_sandbox",
        directory.path(),
        &[("TMPDIR", host_temp.into_os_string())],
        &[],
    );
}

/// With the host keyring granted the runner's Git gets the host's home, but
/// not the global excludes file there: `check_changed` still sees an
/// untracked file the host's `~/.config/git/ignore` lists.
#[cfg(unix)]
#[test]
fn the_hosts_global_excludes_hide_nothing_with_the_host_keyring() {
    if let Some(directory) = std::env::var_os(CHILD_DIRECTORY) {
        let package = PathBuf::from(directory).join("package");
        for host_keyring in [false, true] {
            let (source, test) = policy_file(
                &package,
                "changed.yml",
                &format!(
                    r#"test_version: 1
name: global excludes
capabilities:
  host_keyring: {host_keyring}
given:
  tree:
    a.txt: "x"
    secret-1.txt: "x"
  git:
    init: true
    add: [a.txt]
    commit: true
  config: |
    version: 1
    rules:
      - id: no-secrets
        kind: file_absent
        paths: "secret-*.txt"
        level: error
when: [check_changed]
expect:
  - exit: policy_failure
    violations:
      - {{ rule: no-secrets, path: secret-1.txt }}
"#
                ),
            );
            let runner = Runner::new(RunnerOptions {
                allow_spawn: false,
                allow_host_keyring: true,
                update_snapshots: false,
            });
            let run = runner.run(&test, &source).unwrap();
            assert!(
                run.passed(),
                "host_keyring: {host_keyring}: {:#?}",
                run.mismatches
            );
        }
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    std::fs::create_dir_all(home.join(".config/git")).unwrap();
    std::fs::write(home.join(".config/git/ignore"), "secret-*.txt\n").unwrap();
    std::fs::create_dir(directory.path().join("package")).unwrap();

    run_in_child(
        "policy_runner_sandbox::the_hosts_global_excludes_hide_nothing_with_the_host_keyring",
        directory.path(),
        &[("HOME", home.into_os_string())],
        &["XDG_CONFIG_HOME"],
    );
}

/// A `fix` step's `when:` reads the test's environment, not the host's: a
/// variable set only on the host leaves the rule, and its fix, inactive.
#[test]
fn a_fix_condition_reads_the_tests_environment_not_the_hosts() {
    if let Some(directory) = std::env::var_os(CHILD_DIRECTORY) {
        assert_eq!(std::env::var("HOST_SECRET").as_deref(), Ok("hunter2"));
        let package = PathBuf::from(directory).join("package");
        let (source, test) = policy_file(
            &package,
            "fix-when.yml",
            r#"test_version: 1
name: fix conditions see given.env only
given:
  tree:
    a.txt: "x"
  config: |
    version: 1
    rules:
      - id: from-host
        kind: file_exists
        paths: CREATED.md
        when: 'env.HOST_SECRET == "hunter2"'
        level: error
        fix:
          file_create:
            content: "x\n"
when: [fix]
expect:
  - exit: success
    applied: []
expect_tree:
  a.txt: "x"
"#,
        );
        let run = Runner::default().run(&test, &source).unwrap();
        assert!(run.passed(), "{:#?}", run.mismatches);
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("package")).unwrap();
    run_in_child(
        "policy_runner_sandbox::a_fix_condition_reads_the_tests_environment_not_the_hosts",
        directory.path(),
        &[("HOST_SECRET", "hunter2".into())],
        &[],
    );
}

/// The host's `PATH`, which the runner passes its granted commands, is not
/// part of the environment a condition or a `${VAR}` expansion reads.
#[test]
fn a_condition_does_not_read_the_hosts_path() {
    assert!(std::env::var_os("PATH").is_some());
    let package = tempfile::tempdir().unwrap();
    let (source, test) = policy_file(
        package.path(),
        "path.yml",
        r#"test_version: 1
name: conditions do not see the host's PATH
given:
  tree:
    a.txt: "x"
    d/a.txt: "x"
  config: |
    version: 1
    rules:
      - id: from-host-path
        kind: file_exists
        paths: MISSING.md
        when: env.PATH
        level: error
      - id: nested-from-host-path
        kind: for_each_dir
        select: d
        require:
          - kind: file_exists
            paths: "{path}/MISSING.md"
            when: env.PATH
        level: error
      - id: from-ceiling
        kind: file_exists
        paths: MISSING.md
        when: env.GIT_CEILING_DIRECTORIES
        level: error
      - id: from-sandbox-home
        kind: file_exists
        paths: MISSING.md
        when: env.HOME
        level: error
      - id: from-xdg
        kind: file_exists
        paths: MISSING.md
        when: env.XDG_CONFIG_HOME
        level: error
when: [check]
expect:
  - exit: success
    violations: []
"#,
    );

    let run = Runner::default().run(&test, &source).unwrap();
    assert!(run.passed(), "{:#?}", run.mismatches);
}

/// A fix that moves a symlink changes where its relative text leads:
/// `a/b/m -> ../../t` leads to the tree's `t`, but moved to the root by
/// `relocate` it leads two levels above the repository, where `expect_tree`
/// would read a host file. The runner refuses the test after that step.
#[cfg(unix)]
#[test]
fn a_fix_cannot_move_a_symlink_out_of_the_repository() {
    let package = tempfile::tempdir().unwrap();
    let (source, test) = policy_file(
        package.path(),
        "relocate.yml",
        r#"test_version: 1
name: a relocated link
given:
  tree:
    t: "in the tree\n"
    a: { b: { m: { "$symlink": "../../t" } } }
  config: |
    version: 1
    rules:
      - id: no-nested-m
        kind: file_absent
        paths: "**/*/m"
        level: error
        fix:
          relocate: { applicability: safe }
when: [fix]
expect:
  - exit: success
    applied: [no-nested-m]
expect_tree_mode: contains
expect_tree:
  m: "a host file\n"
"#,
    );

    let error = Runner::default().run(&test, &source).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("symlink leads outside the test repository"),
        "{error}"
    );
}

/// A fixture's symlink is checked where it resolves in the fixture, but its
/// text is copied as it is, and resolves from the copy's place. In
/// `package/a/b/c/fx`, `leak -> ../../../secret` leads to `package/a/secret`
/// and, through it, back into the fixture; copied to
/// `$TMPDIR/alint-policy-test-*/repository` it leads to `$TMPDIR/../secret`,
/// a host file `expect_tree` would read. The runner refuses the test.
#[cfg(unix)]
#[test]
fn a_fixture_symlink_cannot_lead_out_of_the_copy() {
    let Some(host) = std::env::var_os(CHILD_DIRECTORY).map(PathBuf::from) else {
        let host = tempfile::tempdir().unwrap();
        std::fs::create_dir(host.path().join("tmp")).unwrap();
        std::fs::write(host.path().join("secret"), "a host file\n").unwrap();
        run_in_child(
            "policy_runner_sandbox::a_fixture_symlink_cannot_lead_out_of_the_copy",
            host.path(),
            &[("TMPDIR", host.path().join("tmp").into_os_string())],
            &[],
        );
        return;
    };
    let package = host.join("package");
    let fixture = package.join("a/b/c/fx");
    std::fs::create_dir_all(&fixture).unwrap();
    std::fs::write(fixture.join("inner"), "in the fixture\n").unwrap();
    std::os::unix::fs::symlink("b/c/fx/inner", package.join("a/secret")).unwrap();
    std::os::unix::fs::symlink("../../../secret", fixture.join("leak")).unwrap();
    let (source, test) = policy_file(
        &package,
        "fixture.yml",
        r#"test_version: 1
name: a copied link
given:
  fixture: a/b/c/fx
  config: |
    version: 1
    rules:
      - id: has-inner
        kind: file_exists
        paths: inner
        level: error
when: [check]
expect:
  - exit: success
    violations: []
expect_tree_mode: contains
expect_tree:
  leak: "a host file\n"
"#,
    );

    let error = Runner::default().run(&test, &source).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("symlink leads outside the test repository"),
        "{error}"
    );
}
