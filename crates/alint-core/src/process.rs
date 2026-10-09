//! Bounded child-process execution shared by every spawning site (custom
//! facts, the `command` rule, `generated_file_fresh`, `command_idempotent`,
//! the `command` fix op).
//!
//! Three properties, each a past bug class (audit 2026-10):
//!
//! 1. **Concurrent drain.** stdout/stderr are read on their own threads while
//!    the child runs. Reading only after exit deadlocks a child that writes more
//!    than a pipe buffer (~64 KiB): it blocks on the full pipe, never exits, and
//!    the wait loop reports a bogus timeout.
//! 2. **The timeout kills the whole tree.** On unix the child is spawned in its
//!    own process group and the GROUP is killed on timeout, so a grandchild
//!    (`sh -c "sleep 15; echo x"`'s `sleep`) cannot outlive the deadline. Killing
//!    only the direct child left the grandchild holding the pipe's write end, and
//!    the reader join then blocked until it exited on its own.
//! 3. **Reader joins are bounded.** Once the direct child has exited (or been
//!    killed), whatever it wrote is already in the pipe and drains at once; a
//!    reader still blocked after [`READER_GRACE`] is held open by a backgrounded
//!    descendant (`sh -c "sleep 6 & echo hi"`). The output read so far is
//!    returned and the reader is abandoned (it ends when that descendant closes
//!    the pipe), instead of stalling the run for the descendant's lifetime.
//!
//! On Windows there is no process group here (a job object needs `unsafe`
//! FFI, which the workspace forbids): the direct child is killed and property
//! 3 still bounds the wait.

use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Poll granularity of the wait loop.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// How long to wait for a reader to reach EOF after the direct child has
/// exited or been killed. Data the child itself wrote is already buffered in
/// the pipe and drains in microseconds; this only bounds the wait for a
/// descendant that inherited the pipe. Generous so a heavily loaded machine
/// never truncates a legitimate child's output.
pub const READER_GRACE: Duration = Duration::from_secs(2);

/// Outcome of [`run_bounded`].
#[derive(Debug)]
pub enum RunOutcome {
    /// The child exited; captured stdout/stderr (each capped).
    Exited {
        status: ExitStatus,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    /// `spawn()` (or the post-spawn wait) failed.
    SpawnError(std::io::Error),
    /// Killed (with its process group, on unix) after exceeding the timeout.
    TimedOut,
}

/// Spawn `cmd` (caller-configured program / args / cwd / env / stdin) with
/// stdout piped and stderr piped when `capture_stderr` (else discarded), and run
/// it to completion within `timeout`. Each captured stream keeps at most `cap`
/// bytes; any excess is read and discarded so the child never blocks on a full
/// pipe. See the module docs for the timeout / grandchild semantics.
pub fn run_bounded(
    mut cmd: Command,
    timeout: Duration,
    cap: u64,
    capture_stderr: bool,
) -> RunOutcome {
    cmd.stdout(Stdio::piped()).stderr(if capture_stderr {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        // Own process group (pgid == child pid) so a timeout can kill every
        // descendant that did not deliberately leave the group.
        cmd.process_group(0);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return RunOutcome::SpawnError(e),
    };
    let out = Reader::start(child.stdout.take(), cap);
    let err = Reader::start(child.stderr.take(), cap);

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let grace = Instant::now() + READER_GRACE;
                return RunOutcome::Exited {
                    status,
                    stdout: out.finish(grace),
                    stderr: err.finish(grace),
                };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    kill_tree(&mut child);
                    let grace = Instant::now() + READER_GRACE;
                    let _ = out.finish(grace);
                    let _ = err.finish(grace);
                    return RunOutcome::TimedOut;
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(e) => {
                kill_tree(&mut child);
                let grace = Instant::now() + READER_GRACE;
                let _ = out.finish(grace);
                let _ = err.finish(grace);
                return RunOutcome::SpawnError(e);
            }
        }
    }
}

/// Kill the child and (on unix) its whole process group, then reap it.
fn kill_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        // `process_group(0)` made the pgid equal to the child's pid. std has no
        // killpg and the workspace forbids `unsafe`, so signal the group through
        // the POSIX `kill` utility. Best-effort: if it is unavailable the direct
        // child is still killed below and the bounded reader join still caps
        // the wait.
        let _ = Command::new("kill")
            .args(["-s", "KILL", "--", &format!("-{}", child.id())])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// A pipe drained on its own thread into a shared, capped buffer.
struct Reader {
    buf: Arc<Mutex<Vec<u8>>>,
    done: Option<Receiver<()>>,
}

impl Reader {
    fn start(pipe: Option<impl std::io::Read + Send + 'static>, cap: u64) -> Self {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let Some(mut pipe) = pipe else {
            return Self { buf, done: None };
        };
        let (tx, rx) = channel();
        let shared = Arc::clone(&buf);
        let cap = usize::try_from(cap).unwrap_or(usize::MAX);
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        let mut b = shared
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        let room = cap.saturating_sub(b.len());
                        b.extend_from_slice(&chunk[..n.min(room)]);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            let _ = tx.send(());
        });
        Self {
            buf,
            done: Some(rx),
        }
    }

    /// Wait for EOF until `deadline`, then return what has been read. A reader
    /// that has not finished by then is abandoned (see the module docs).
    fn finish(self, deadline: Instant) -> Vec<u8> {
        if let Some(rx) = &self.done {
            let left = deadline.saturating_duration_since(Instant::now());
            // EOF, a dead reader, or the grace expiring all end the wait.
            let _: Result<(), RecvTimeoutError> = rx.recv_timeout(left);
        }
        std::mem::take(
            &mut *self
                .buf
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn sh(script: &str) -> Command {
        let mut c = Command::new("sh");
        c.args(["-c", script]).stdin(Stdio::null());
        c
    }

    #[test]
    fn large_output_does_not_deadlock_or_time_out() {
        // > a pipe buffer, written before exit: must drain concurrently.
        let start = Instant::now();
        let out = run_bounded(
            sh("head -c 200000 /dev/zero | tr '\\0' x; exit 0"),
            Duration::from_secs(10),
            1 << 20,
            true,
        );
        let RunOutcome::Exited { status, stdout, .. } = out else {
            panic!("expected exit, got {out:?}");
        };
        assert!(status.success());
        assert_eq!(stdout.len(), 200_000);
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn output_is_capped_and_excess_drained() {
        let out = run_bounded(
            sh("head -c 200000 /dev/zero | tr '\\0' x"),
            Duration::from_secs(10),
            1000,
            true,
        );
        let RunOutcome::Exited { stdout, .. } = out else {
            panic!("expected exit, got {out:?}");
        };
        assert_eq!(stdout.len(), 1000);
    }

    #[test]
    fn timeout_kills_the_grandchild_holding_the_pipe() {
        // `sleep` is a grandchild that inherits stdout; killing only `sh` left it
        // holding the pipe and the run took the full 15s.
        let start = Instant::now();
        let out = run_bounded(
            sh("sleep 15; echo x"),
            Duration::from_millis(500),
            1024,
            true,
        );
        assert!(matches!(out, RunOutcome::TimedOut), "{out:?}");
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timeout not enforced: {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn backgrounded_descendant_does_not_stall_after_exit() {
        let start = Instant::now();
        let out = run_bounded(sh("sleep 8 & echo hi"), Duration::from_secs(30), 1024, true);
        let RunOutcome::Exited { status, stdout, .. } = out else {
            panic!("expected exit, got {out:?}");
        };
        assert!(status.success());
        assert_eq!(stdout, b"hi\n");
        assert!(
            start.elapsed() < Duration::from_secs(6),
            "stalled on the background descendant: {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn stderr_discarded_when_not_captured() {
        let out = run_bounded(
            sh("echo out; echo err >&2"),
            Duration::from_secs(10),
            1024,
            false,
        );
        let RunOutcome::Exited { stdout, stderr, .. } = out else {
            panic!("expected exit, got {out:?}");
        };
        assert_eq!(stdout, b"out\n");
        assert_eq!(stderr, b"");
    }
}
