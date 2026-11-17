//! The environment of every process these tests start: `alint`, and their own
//! `git`, without the host's Git setup.
//!
//! `alint` walks with the user's global excludes file, as it should for a
//! user, and Git reads the system and global configuration. On a machine
//! whose global excludes list `*.md` or `*.yml`, or whose global configuration
//! sets `commit.gpgsign`, the files a test wrote went missing and its commits
//! failed. So no system or global configuration file
//! (`GIT_CONFIG_NOSYSTEM`, `GIT_CONFIG_GLOBAL`), no `$XDG_CONFIG_HOME/git`
//! (the default global excludes and attributes files, which
//! `GIT_CONFIG_GLOBAL` does not cover): each is the null device, and a path
//! below it reads as no file. Outside Windows, `HOME` is an empty directory of
//! the test build's own ([`HOME`]), so the walker reads no `~/.gitconfig`
//! (which can name an excludes file) while a remote `extends` can still be
//! cached there. A test of the global excludes file sets its own values after
//! these, which replace them.

use std::ffi::OsStr;
use std::process::Command;

/// What `/dev/null` is called here.
pub const NULL_DEVICE: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

/// The home directory of every process these tests start, outside Windows:
/// in the directory Cargo gives integration tests, and empty but for what
/// those processes write (a remote `extends` cache). The null device would
/// do for Git and the walker, but a fetch could then cache nothing and never
/// start, and a test that a fetch does not happen would pass for that reason.
pub const HOME: &str = concat!(env!("CARGO_TARGET_TMPDIR"), "/alint-test-home");

/// The variables [`command`] sets. `HOME` is left alone on Windows, where
/// Git and the walker find the home directory without it.
pub fn environment() -> Vec<(&'static str, &'static str)> {
    let mut environment = vec![
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_CONFIG_GLOBAL", NULL_DEVICE),
        ("XDG_CONFIG_HOME", NULL_DEVICE),
    ];
    if !cfg!(windows) {
        std::fs::create_dir_all(HOME).expect("create the tests' home directory");
        environment.push(("HOME", HOME));
    }
    environment
}

/// `program` (usually `alint`) in the [`environment`].
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    command.envs(environment());
    command
}

/// `git` in the [`environment`], for a test's own setup.
pub fn git() -> Command {
    command("git")
}
