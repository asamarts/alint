//! Bounded child-process execution shared by every spawning site (custom
//! facts, the `command` rule, `generated_file_fresh`, `command_idempotent`,
//! the `command` fix op).
//!
//! Properties, each a past bug class (audit 2026-10):
//!
//! 1. **Concurrent drain.** stdout/stderr are read on their own threads while
//!    the child runs. Reading only after exit deadlocks a child that writes more
//!    than a pipe buffer (~64 KiB): it blocks on the full pipe, never exits, and
//!    the wait loop reports a bogus timeout.
//! 2. **The process-group mode follows whether alint's stdin is a terminal**
//!    ([`use_own_process_group`]).
//!    - *stdin is NOT a terminal* (CI, the LSP, pipes, cron; "grouped mode"):
//!      on unix the child is spawned in its own process group and the timeout
//!      kills the whole GROUP, so a grandchild (`sh -c "sleep 15; echo x"`'s
//!      `sleep`) cannot outlive the deadline.
//!    - *stdin IS a terminal* (an interactive run): the child stays in alint's
//!      own process group. A separate group is outside the terminal's
//!      foreground group, so Ctrl-C would kill alint but orphan the child, and
//!      a child reading `/dev/tty` (a credential prompt) would be stopped by
//!      `SIGTTIN` and hang until the timeout. Here the timeout kills only the
//!      direct child; a grandchild can keep running, and property 3 bounds how
//!      long alint waits on it.
//!
//!    "stdin is a terminal" is the agreed proxy for "interactive session with a
//!    controlling terminal": one portable `isatty`, decided once per process.
//!    Redirecting stdin (`alint check < /dev/null`) opts an interactive run into
//!    grouped mode.
//! 3. **Reader joins are bounded.** Once the direct child has exited (or been
//!    killed), whatever it wrote is already in the pipe and drains at once; a
//!    reader still blocked after [`READER_GRACE`] is held open by a descendant
//!    that survived (ungrouped mode, or one that left the group via `setsid`).
//!    The output read so far is returned and the reader is abandoned (it ends
//!    when that descendant closes the pipe), instead of stalling the run for the
//!    descendant's lifetime.
//! 4. **No deadline overflow.** A `timeout` too large to add to `Instant::now()`
//!    (`timeout: 18446744073709551615`) means "no deadline", not a panic.
//!
//! On Windows there is no process group here (a job object needs `unsafe`
//! FFI, which the workspace forbids): the direct child is killed and property
//! 3 still bounds the wait.

use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex, OnceLock};
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
    /// Killed after exceeding the timeout (with its process group in grouped
    /// mode; see the module docs).
    TimedOut,
}

/// Spawn `cmd` (caller-configured program / args / cwd / env / stdin) with
/// stdout piped and stderr piped when `capture_stderr` (else discarded), and run
/// it to completion within `timeout`. Each captured stream keeps at most `cap`
/// bytes; any excess is read and discarded so the child never blocks on a full
/// pipe. The process-group mode follows [`use_own_process_group`] for alint's
/// stdin; see the module docs for the timeout / descendant semantics.
pub fn run_bounded(cmd: Command, timeout: Duration, cap: u64, capture_stderr: bool) -> RunOutcome {
    static GROUPED: OnceLock<bool> = OnceLock::new();
    let grouped = *GROUPED.get_or_init(|| {
        use std::io::IsTerminal as _;
        use_own_process_group(std::io::stdin().is_terminal())
    });
    run_bounded_in(cmd, timeout, cap, capture_stderr, grouped)
}

/// The process-group rule: the child gets its own group (so a timeout kills
/// the whole tree) only on unix and only when alint's stdin is NOT a terminal.
/// With a terminal the child must stay in the foreground group so Ctrl-C
/// reaches it and it may read `/dev/tty`.
#[must_use]
pub fn use_own_process_group(stdin_is_terminal: bool) -> bool {
    cfg!(unix) && !stdin_is_terminal
}

/// [`run_bounded`] with the process-group mode chosen explicitly.
fn run_bounded_in(
    mut cmd: Command,
    timeout: Duration,
    cap: u64,
    capture_stderr: bool,
    grouped: bool,
) -> RunOutcome {
    cmd.stdout(Stdio::piped()).stderr(if capture_stderr {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    #[cfg(unix)]
    if grouped {
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

    // `None`: the timeout is too large to represent, i.e. no deadline.
    let deadline = Instant::now().checked_add(timeout);
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
                if deadline.is_some_and(|d| Instant::now() >= d) {
                    kill_tree(&mut child, grouped);
                    let grace = Instant::now() + READER_GRACE;
                    let _ = out.finish(grace);
                    let _ = err.finish(grace);
                    return RunOutcome::TimedOut;
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(e) => {
                kill_tree(&mut child, grouped);
                let grace = Instant::now() + READER_GRACE;
                let _ = out.finish(grace);
                let _ = err.finish(grace);
                return RunOutcome::SpawnError(e);
            }
        }
    }
}

/// Kill the child (with its whole process group in grouped mode), then reap it.
fn kill_tree(child: &mut Child, grouped: bool) {
    if grouped {
        kill_group(child.id());
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// SIGKILL process group `pgid` (unix; a no-op elsewhere).
fn kill_group(pgid: u32) {
    #[cfg(unix)]
    {
        // `process_group(0)` made the pgid equal to the child's pid. std has no
        // killpg and the workspace forbids `unsafe`, so signal the group through
        // the POSIX `kill` utility. Best-effort: if it is unavailable the direct
        // child is still killed by `kill_tree` and the bounded reader join still
        // caps the wait.
        let _ = Command::new("kill")
            .args(["-s", "KILL", "--", &format!("-{pgid}")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(unix))]
    let _ = pgid;
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

    /// Wait for EOF until `deadline`; `true` once the reader has finished (or
    /// there is no pipe). Callable more than once, before [`Reader::finish`].
    fn wait_eof(&mut self, deadline: Instant) -> bool {
        let Some(rx) = &self.done else {
            return true;
        };
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                // Finished: later waits must not block on the spent channel.
                self.done = None;
                true
            }
            Err(RecvTimeoutError::Timeout) => false,
        }
    }

    /// Wait for EOF until `deadline`, then return what has been read. A reader
    /// that has not finished by then is abandoned (see the module docs).
    fn finish(mut self, deadline: Instant) -> Vec<u8> {
        let _ = self.wait_eof(deadline);
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
        // Grouped mode (stdin not a terminal).
        let start = Instant::now();
        let out = run_bounded_in(
            sh("sleep 15; echo x"),
            Duration::from_millis(500),
            1024,
            true,
            true,
        );
        assert!(matches!(out, RunOutcome::TimedOut), "{out:?}");
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "timeout not enforced: {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn ungrouped_timeout_is_still_bounded_by_the_reader_grace() {
        // Terminal mode: only `sh` is killed; the grandchild keeps the pipe, so
        // the wait is bounded by READER_GRACE instead.
        let start = Instant::now();
        let out = run_bounded_in(
            sh("sleep 15; echo x"),
            Duration::from_millis(300),
            1024,
            true,
            false,
        );
        assert!(matches!(out, RunOutcome::TimedOut), "{out:?}");
        assert!(
            start.elapsed() < Duration::from_secs(6),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn backgrounded_descendant_does_not_stall_after_exit() {
        for grouped in [true, false] {
            let start = Instant::now();
            let out = run_bounded_in(
                sh("sleep 8 & echo hi"),
                Duration::from_secs(30),
                1024,
                true,
                grouped,
            );
            let RunOutcome::Exited { status, stdout, .. } = out else {
                panic!("expected exit, got {out:?}");
            };
            assert!(status.success());
            assert_eq!(stdout, b"hi\n");
            assert!(
                start.elapsed() < Duration::from_secs(6),
                "grouped={grouped}: stalled on the background descendant: {:?}",
                start.elapsed()
            );
        }
    }

    #[test]
    fn huge_timeout_means_no_deadline_not_a_panic() {
        // `timeout: 18446744073709551615` overflowed `Instant + Duration`.
        for grouped in [true, false] {
            let out = run_bounded_in(sh("echo ok"), Duration::MAX, 1024, true, grouped);
            let RunOutcome::Exited { status, stdout, .. } = out else {
                panic!("expected exit, got {out:?}");
            };
            assert!(status.success());
            assert_eq!(stdout, b"ok\n");
        }
        let out = run_bounded(sh("true"), Duration::from_secs(u64::MAX), 1024, false);
        assert!(matches!(out, RunOutcome::Exited { .. }), "{out:?}");
    }

    #[test]
    fn group_mode_follows_whether_stdin_is_a_terminal() {
        assert!(
            !use_own_process_group(true),
            "a terminal keeps the child in alint's group"
        );
        assert!(use_own_process_group(false));
    }

    /// `(child's own pid, child's pgid)` as reported by `ps`.
    fn child_pid_and_pgid(grouped: bool) -> (String, String) {
        let out = run_bounded_in(
            sh("echo $$; ps -o pgid= -p $$"),
            Duration::from_secs(10),
            1024,
            true,
            grouped,
        );
        let RunOutcome::Exited { stdout, .. } = out else {
            panic!("expected exit, got {out:?}");
        };
        let s = String::from_utf8(stdout).unwrap();
        let mut lines = s.lines().map(|l| l.trim().to_string());
        (lines.next().unwrap(), lines.next().unwrap())
    }

    #[test]
    fn ungrouped_child_stays_in_the_callers_process_group() {
        // Terminal mode: Ctrl-C reaches the child and `/dev/tty` reads work
        // because the child shares alint's (foreground) process group.
        let ours = Command::new("ps")
            .args(["-o", "pgid=", "-p", &std::process::id().to_string()])
            .output()
            .expect("ps");
        let ours = String::from_utf8_lossy(&ours.stdout).trim().to_string();
        let (_, child_pgid) = child_pid_and_pgid(false);
        assert_eq!(child_pgid, ours);
        let (leader, group) = child_pid_and_pgid(true);
        assert_eq!(group, leader, "grouped mode: the child leads its own group");
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
