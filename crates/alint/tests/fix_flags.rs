//! CLI behavior for `alint fix` tier flags. `--fix-only` reports only what was
//! applied (residual findings suppressed) and exits 0 unless a fix errored;
//! the plain `fix` reports residuals and exits nonzero on an unfixable error.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn alint() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(alint())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn alint")
}

// A fixable rule (trailing whitespace) plus an unfixable one (a forbidden
// pattern with no fix): the fix applies the first and leaves the second as an
// error-level residual.
const CONFIG: &str = "\
version: 1
rules:
  - id: no-ws
    kind: no_trailing_whitespace
    paths: \"**/*.rs\"
    level: error
    fix:
      file_trim_trailing_whitespace: {}
  - id: no-todo
    kind: file_content_forbidden
    paths: \"**/*.rs\"
    pattern: TODO
    level: error
";

fn setup() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join(".alint.yml"), CONFIG).unwrap();
    std::fs::write(tmp.path().join("a.rs"), "fn a() {}   \nTODO\n").unwrap();
    tmp
}

#[test]
fn fix_only_suppresses_residual_and_exits_zero() {
    let tmp = setup();
    let out = run(tmp.path(), &["fix", "--fix-only", "."]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    // The fixable rule applied (file trimmed)...
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("a.rs")).unwrap(),
        "fn a() {}\nTODO\n"
    );
    assert!(
        stdout.contains("no-ws"),
        "applied rule shown; stdout:\n{stdout}"
    );
    // ...the unfixable residual is suppressed...
    assert!(
        !stdout.contains("no-todo"),
        "residual must be suppressed under --fix-only; stdout:\n{stdout}"
    );
    // ...and with no fix error, the exit is success.
    assert_eq!(
        out.status.code(),
        Some(0),
        "fix-only exits 0 absent a fix error"
    );
}

#[test]
fn plain_fix_reports_residual_and_exits_nonzero() {
    let tmp = setup();
    let out = run(tmp.path(), &["fix", "."]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("no-todo"),
        "plain fix reports the unfixable residual; stdout:\n{stdout}"
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "an unfixable error exits nonzero without --fix-only"
    );
}
