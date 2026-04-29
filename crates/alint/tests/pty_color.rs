//! `--color=auto` resolves to "colors on" when stdout is a real
//! TTY. trycmd attaches pipes for stdout/stderr, so its
//! `--color=auto` snapshots only ever exercise the non-TTY
//! branch. This integration test closes that gap by allocating a
//! pseudo-terminal, spawning alint with stdout attached to the
//! pty slave, draining the master, and asserting ANSI escape
//! sequences appear in the output.
//!
//! Unix-only: `portable-pty` works on Windows too, but the
//! Windows `ConPTY` path adds complexity (and Windows
//! `is_terminal()` follows different rules). The unit-test
//! coverage of `crate::progress::Progress::new(Always|Never)`
//! plus this Unix pty test together exercise both branches.

#![cfg(unix)]

use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};

/// Drain pty output, with a wall-clock cap so a hung child
/// can't lock the test runner forever.
fn drain_pty_until_eof(mut reader: Box<dyn Read + Send>, deadline: Duration) -> String {
    let start = Instant::now();
    let mut out = Vec::with_capacity(8 * 1024);
    let mut buf = [0u8; 4096];
    while start.elapsed() < deadline {
        match reader.read(&mut buf) {
            Ok(0) => break, // EOF — child has exited
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {} // retry the loop
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Materialise a tempdir with a minimal `.alint.yml` whose
/// rule fires (so the human formatter has something to colour).
fn fixture_repo() -> tempfile::TempDir {
    let tmp = tempfile::Builder::new()
        .prefix("alint-pty-test-")
        .tempdir()
        .expect("tempdir create");
    std::fs::write(
        tmp.path().join(".alint.yml"),
        b"version: 1\n\
          rules:\n  \
            - id: must-have-license\n    \
              kind: file_exists\n    \
              paths: LICENSE\n    \
              level: warning\n",
    )
    .expect("write .alint.yml");
    tmp
}

#[test]
fn color_auto_emits_ansi_when_stdout_is_a_tty() {
    // Skip when the runner can't allocate a pty (extremely rare
    // on Linux/macOS dev boxes; possible on locked-down CI
    // sandboxes). Better to skip than fail spuriously.
    let pty_system = NativePtySystem::default();
    let pair = match pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("pty unavailable on this runner: {e}; skipping");
            return;
        }
    };

    let alint_bin = env!("CARGO_BIN_EXE_alint");
    let tmp = fixture_repo();

    let mut cmd = CommandBuilder::new(alint_bin);
    cmd.arg("--color=auto");
    cmd.arg("check");
    cmd.arg(tmp.path());
    // Make sure no leaked NO_COLOR / CLICOLOR_FORCE from the
    // host environment skews the test.
    cmd.env_remove("NO_COLOR");
    cmd.env_remove("CLICOLOR_FORCE");
    // alint's miette / anstream rely on TERM for some
    // capability detection. Ensure we look like a real terminal.
    cmd.env("TERM", "xterm-256color");
    cmd.cwd(tmp.path() as &Path);

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .expect("spawn alint inside pty");

    // Read from the master in a worker thread so we can wait on
    // the child without deadlocking on the pty buffer. Child
    // writes go to the slave; master is the read side.
    let reader = pair
        .master
        .try_clone_reader()
        .expect("clone pty master reader");
    let join = std::thread::spawn(move || drain_pty_until_eof(reader, Duration::from_secs(5)));

    // Drop master/slave handles in the parent so the child's
    // exit closes the pty (signalling EOF to the reader thread).
    drop(pair.slave);
    let _exit = child.wait().expect("child wait");
    drop(pair.master);

    let output = join.join().expect("reader thread");
    // We don't pin the exit status — the rule fires (warning),
    // and warnings don't fail the build by default. What we
    // care about is that the human formatter chose to colorise.
    assert!(
        output.contains('\x1b'),
        "expected ANSI escape (\\x1b) in pty output, got: {output:?}",
    );
}

#[test]
fn color_never_strips_ansi_even_on_a_tty() {
    // Symmetric guarantee: `--color=never` MUST suppress colors
    // even when the runtime detects a TTY. A regression here
    // means user explicit-off no longer wins, which would break
    // CI logs that grep alint output.
    let pty_system = NativePtySystem::default();
    let pair = match pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("pty unavailable on this runner: {e}; skipping");
            return;
        }
    };

    let alint_bin = env!("CARGO_BIN_EXE_alint");
    let tmp = fixture_repo();

    let mut cmd = CommandBuilder::new(alint_bin);
    cmd.arg("--color=never");
    cmd.arg("check");
    cmd.arg(tmp.path());
    cmd.env_remove("NO_COLOR");
    cmd.env_remove("CLICOLOR_FORCE");
    cmd.env("TERM", "xterm-256color");
    cmd.cwd(tmp.path() as &Path);

    let mut child = pair.slave.spawn_command(cmd).expect("spawn alint");
    let reader = pair.master.try_clone_reader().expect("clone pty reader");
    let join = std::thread::spawn(move || drain_pty_until_eof(reader, Duration::from_secs(5)));

    drop(pair.slave);
    let _exit = child.wait().expect("child wait");
    drop(pair.master);

    let output = join.join().expect("reader thread");
    assert!(
        !output.contains('\x1b'),
        "expected NO ANSI escape with --color=never, got: {output:?}",
    );
    // Sanity: the rule still fired and the report rendered.
    assert!(
        output.contains("must-have-license"),
        "expected rule id in output: {output:?}",
    );
}
