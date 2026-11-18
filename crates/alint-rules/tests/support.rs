//! Keeps the integration tests' Git setup off the host's Git configuration.

use std::path::Path;
use std::process::Command;

/// What `/dev/null` is called here.
const NULL_DEVICE: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

/// The environment of a test's own `git` commands: no system or global
/// configuration file (a host's `commit.gpgsign` fails every commit), and no
/// global excludes or attributes file (`$XDG_CONFIG_HOME/git/ignore`, which
/// `GIT_CONFIG_GLOBAL` does not cover; a path below the null device reads as
/// no file), so `git add` takes every file a test writes.
pub const HERMETIC_GIT: [(&str, &str); 3] = [
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_CONFIG_GLOBAL", NULL_DEVICE),
    ("XDG_CONFIG_HOME", NULL_DEVICE),
];

/// A `git` command for a test's own setup, in [`HERMETIC_GIT`].
pub fn git() -> Command {
    let mut command = Command::new("git");
    command.envs(HERMETIC_GIT);
    command
}

/// Point the repository at `root` at no excludes or attributes file of the
/// user's: the code under test runs Git with the host's environment, and a
/// repository's own setting outranks the global default it would read.
pub fn ignore_global_excludes(root: &Path) {
    for key in ["core.excludesFile", "core.attributesFile"] {
        let output = git()
            .arg("-C")
            .arg(root)
            .args(["config", key, NULL_DEVICE])
            .output()
            .expect("git invocation");
        assert!(output.status.success(), "git config {key} failed");
    }
}

/// The walk options of a test: without the user's global excludes file.
pub fn walk_options() -> alint_core::WalkOptions {
    alint_core::WalkOptions::default().with_global_git_excludes(false)
}
