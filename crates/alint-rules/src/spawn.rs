//! Shared process-spawn helper for the single-shot,
//! command-declaring rule kinds (`generated_file_fresh`,
//! `command_idempotent`) and the `command` fix op.
//!
//! One spawn through [`alint_core::process::run_bounded`]: stdout +
//! stderr captured **concurrently** (so the full output is preserved — the
//! generator's stdout is diffed; the checker's offender list must not be
//! truncated — and a large output cannot deadlock on a full pipe buffer),
//! the timeout kills the child's whole process group on unix (a grandchild
//! cannot hold the pipe open past the deadline), and reader joins are
//! bounded after exit (a backgrounded descendant cannot stall the run).
//!
//! The `command` rule uses the same runner with a small per-file cap.

use std::path::{Component, Path, PathBuf};
use std::process::{Command as StdCommand, ExitStatus, Stdio};
use std::time::Duration;

use alint_core::process::{RunOutcome, run_bounded};

/// Default child timeout (seconds) when the rule does not set
/// `timeout:`. Generous for a single-shot whole-repo generator /
/// checker, yet bounded so a deadlocked child cannot hang CI
/// indefinitely.
pub(crate) const DEFAULT_SPAWN_TIMEOUT_SECS: u64 = 120;

/// Outcome of [`run_capturing`].
pub(crate) enum SpawnOutcome {
    /// Child exited; full stdout/stderr captured.
    Exited {
        status: ExitStatus,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    /// `spawn()` (or the post-spawn wait) failed.
    SpawnError(std::io::Error),
    /// Killed after exceeding the timeout.
    TimedOut { secs: u64 },
}

/// Lexically resolve a rule's `workdir:` to a root-relative path (empty = the
/// root itself), refusing an absolute path or a `..` that climbs out of the
/// repository. Run at build time (a config error) and again before spawning.
pub(crate) fn lexical_workdir(workdir: &str) -> std::result::Result<PathBuf, String> {
    let escapes = || {
        format!(
            "`workdir: {workdir}` escapes the repository root (an absolute path, or a \
             `..` that climbs out of it); it must name a directory inside the repository"
        )
    };
    let mut rel = PathBuf::new();
    for comp in Path::new(workdir).components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !rel.pop() {
                    return Err(escapes());
                }
            }
            Component::Normal(c) => rel.push(c),
            Component::RootDir | Component::Prefix(_) => return Err(escapes()),
        }
    }
    Ok(rel)
}

/// The directory a spawning rule runs its child in: `root` joined with the
/// lexically confined `workdir` ([`lexical_workdir`]), and -- when it exists --
/// verified to still lie inside `root` after symlinks are resolved, so an
/// in-repo symlink (`tools -> /`) cannot move the child's cwd out of the tree.
pub(crate) fn confined_workdir(root: &Path, workdir: &str) -> std::result::Result<PathBuf, String> {
    let dir = root.join(lexical_workdir(workdir)?);
    if let (Ok(root_c), Ok(dir_c)) = (root.canonicalize(), dir.canonicalize())
        && !dir_c.starts_with(&root_c)
    {
        return Err(format!(
            "`workdir: {workdir}` resolves (through a symlink) to {}, outside the \
             repository root; refusing to run the command there",
            dir_c.display()
        ));
    }
    Ok(dir)
}

/// [`run_capturing`] in the rule's `workdir`, confined to `root`
/// ([`confined_workdir`]). An escaping workdir is never spawned in: it comes
/// back as a [`SpawnOutcome::SpawnError`] carrying the reason.
pub(crate) fn run_in_workdir(
    argv: &[String],
    root: &Path,
    workdir: &str,
    env: &[(&str, String)],
    timeout: Duration,
) -> SpawnOutcome {
    match confined_workdir(root, workdir) {
        Ok(cwd) => run_capturing(argv, &cwd, env, timeout),
        Err(reason) => SpawnOutcome::SpawnError(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            reason,
        )),
    }
}

/// Spawn `argv` in `cwd` with stdin closed and `env` pairs set,
/// draining stdout+stderr concurrently while enforcing
/// `timeout`. On exit returns the status + full captured output;
/// on a spawn/wait error returns [`SpawnOutcome::SpawnError`];
/// past `timeout` the child (and, on unix, its process group) is killed and
/// [`SpawnOutcome::TimedOut`] is returned (captured output
/// discarded).
pub(crate) fn run_capturing(
    argv: &[String],
    cwd: &Path,
    env: &[(&str, String)],
    timeout: Duration,
) -> SpawnOutcome {
    let Some((program, rest)) = argv.split_first() else {
        return SpawnOutcome::SpawnError(std::io::Error::other(
            "run_capturing called with an empty argv",
        ));
    };
    let mut cmd = StdCommand::new(program);
    cmd.args(rest).current_dir(cwd).stdin(Stdio::null());
    for (k, v) in env {
        cmd.env(k, v);
    }
    match run_bounded(cmd, timeout, CAPTURE_CAP_BYTES, true) {
        RunOutcome::Exited {
            status,
            stdout,
            stderr,
        } => SpawnOutcome::Exited {
            status,
            stdout,
            stderr,
        },
        RunOutcome::SpawnError(e) => SpawnOutcome::SpawnError(e),
        RunOutcome::TimedOut => SpawnOutcome::TimedOut {
            secs: timeout.as_secs(),
        },
    }
}

/// Per-stream cap on captured child output (L12). The spawning kinds are
/// trust-gated, so this is defense-in-depth: a runaway or compromised
/// generator cannot OOM the run. Generous - every legitimate formatter diff /
/// generated file is orders of magnitude smaller - yet bounded. Excess past the
/// cap is read and discarded so the child can finish writing and exit;
/// truncation is silent but bounded (surfacing it would need `SpawnOutcome`
/// plumbing; tracked as a follow-up).
const CAPTURE_CAP_BYTES: u64 = 64 * 1024 * 1024;

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Instant;

    fn argv(script: &str) -> Vec<String> {
        vec!["sh".into(), "-c".into(), script.into()]
    }

    #[test]
    fn timeout_is_enforced_when_a_grandchild_holds_the_pipe() {
        // Audit 2026-10 finding 5 (generated_file_fresh / command_idempotent /
        // the `command` fix op): `sh -c "sleep 15; echo x"` with a 1s timeout
        // took 15s because the grandchild `sleep` kept the pipe open.
        let start = Instant::now();
        let out = run_capturing(
            &argv("sleep 15; echo x"),
            Path::new("."),
            &[],
            Duration::from_secs(1),
        );
        assert!(matches!(out, SpawnOutcome::TimedOut { secs: 1 }));
        assert!(
            start.elapsed() < Duration::from_secs(6),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn large_output_is_captured_in_full() {
        let SpawnOutcome::Exited { status, stdout, .. } = run_capturing(
            &argv("head -c 300000 /dev/zero | tr '\\0' x"),
            Path::new("."),
            &[],
            Duration::from_secs(10),
        ) else {
            panic!("expected exit");
        };
        assert!(status.success());
        assert_eq!(stdout.len(), 300_000);
    }
}

#[cfg(test)]
mod workdir_tests {
    use super::*;

    #[test]
    fn workdir_must_stay_inside_the_repository() {
        // Defense in depth: a spawning rule's `workdir:` was joined to the root
        // unchecked, so `..` / an absolute path ran the child outside the repo.
        for ok in [".", "", "tools", "a/../b", "./a/./b"] {
            assert!(lexical_workdir(ok).is_ok(), "{ok}");
        }
        assert_eq!(lexical_workdir("a/../b").unwrap(), PathBuf::from("b"));
        let abs = if cfg!(windows) { "C:\\x" } else { "/etc" };
        for bad in ["..", "../x", "a/../..", abs] {
            let err = lexical_workdir(bad).unwrap_err();
            assert!(err.contains("escapes the repository root"), "{bad}: {err}");
        }
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("tools")).unwrap();
        assert_eq!(
            confined_workdir(tmp.path(), "tools").unwrap(),
            tmp.path().join("tools")
        );
        assert!(confined_workdir(tmp.path(), "../").is_err());
        let out = run_in_workdir(
            &["true".to_string()],
            tmp.path(),
            "..",
            &[],
            Duration::from_secs(5),
        );
        assert!(
            matches!(&out, SpawnOutcome::SpawnError(e) if e.to_string().contains("escapes")),
            "an escaping workdir must never be spawned in"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_workdir_cannot_leave_the_repository() {
        use std::os::unix::fs::symlink;
        let outside = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        symlink(outside.path(), repo.path().join("out")).unwrap();
        let err = confined_workdir(repo.path(), "out").unwrap_err();
        assert!(err.contains("outside the"), "{err}");
        // An in-repo symlink to an in-repo dir is fine.
        std::fs::create_dir(repo.path().join("real")).unwrap();
        symlink(repo.path().join("real"), repo.path().join("alias")).unwrap();
        assert!(confined_workdir(repo.path(), "alias").is_ok());
    }
}
