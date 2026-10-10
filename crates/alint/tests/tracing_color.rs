//! Audit 2026-10 finding 9: `ALINT_LOG` diagnostics on stderr ignored
//! `--color`, `NO_COLOR` and the stderr TTY check, so ANSI escapes landed in
//! redirected logs. The tracing layer now follows the same color decision as
//! the rest of the CLI (applied to stderr).

use std::path::Path;
use std::process::{Command, Output};

fn run(dir: &Path, args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_alint"));
    cmd.args(args)
        .current_dir(dir)
        .env("ALINT_LOG", "debug")
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR_FORCE");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().expect("spawn alint")
}

fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join(".alint.yml"), "version: 1\nrules: []\n").unwrap();
    tmp
}

fn has_esc(o: &Output) -> bool {
    o.stderr.contains(&0x1b)
}

#[test]
fn piped_stderr_logs_carry_no_ansi_by_default() {
    let tmp = fixture();
    let o = run(tmp.path(), &["check"], &[]);
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("DEBUG"),
        "{o:?}"
    );
    assert!(!has_esc(&o), "{:?}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn color_never_and_no_color_disable_log_ansi() {
    let tmp = fixture();
    assert!(!has_esc(&run(
        tmp.path(),
        &["check", "--color", "never"],
        &[]
    )));
    assert!(!has_esc(&run(tmp.path(), &["check"], &[("NO_COLOR", "1")])));
}

#[test]
fn color_always_enables_log_ansi() {
    let tmp = fixture();
    assert!(has_esc(&run(
        tmp.path(),
        &["check", "--color", "always"],
        &[]
    )));
}
