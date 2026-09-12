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
fn dry_run_reports_the_same_skip_as_the_real_run_for_an_oversized_file() {
    // Round-4 audit: `fix --dry-run` must run the same read/size/binary guards as
    // the real `fix` and report Skipped for files the real run skips -- not a
    // false optimistic "would apply". A gate using `--dry-run` must not
    // false-green. Here `fix_size_limit` makes the only file oversized.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nfix_size_limit: 100\nrules:\n  - id: no-ws\n    \
         kind: no_trailing_whitespace\n    paths: \"**/*.rs\"\n    level: error\n    \
         fix:\n      file_trim_trailing_whitespace: {}\n",
    )
    .unwrap();
    let big = "let _ = 1;   \n".repeat(50); // > 100 bytes, all with trailing ws
    std::fs::write(root.join("big.rs"), &big).unwrap();

    let dry = run(root, &["fix", "--dry-run", "."]);
    let real = run(root, &["fix", "."]);
    let dry_out = String::from_utf8_lossy(&dry.stdout);
    let real_out = String::from_utf8_lossy(&real.stdout);

    // Both must report the file as SKIPPED (not applied), and agree on the exit
    // code -- the dry run cannot be optimistic where the real run declines.
    assert!(
        dry_out.contains("0 applied") && dry_out.contains("1 skipped"),
        "dry-run must skip the oversized file, not report would-apply; got:\n{dry_out}"
    );
    assert!(
        real_out.contains("0 applied") && real_out.contains("1 skipped"),
        "real run skips the oversized file; got:\n{real_out}"
    );
    assert_eq!(
        dry.status.code(),
        real.status.code(),
        "dry-run and real fix must agree on the exit code"
    );
    // The tree is untouched by both.
    assert_eq!(std::fs::read_to_string(root.join("big.rs")).unwrap(), big);
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
