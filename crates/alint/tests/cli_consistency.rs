//! Regression tests for cross-subcommand CLI consistency (class guards):
//!   * `--only` is global so the bare default `alint --only <id>` works, but it
//!     only applies to `check`/`fix`; every other subcommand must REJECT it
//!     rather than silently ignore a (possibly typo'd) value.
//!   * a subcommand that supports only a subset of `--format` values must
//!     REJECT the others (exit 2), not silently degrade them to human.
//!   * `check`/`fix`/`baseline` operate on a directory; a FILE path must error,
//!     not silently "pass" (a false green).

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

fn code(o: &Output) -> i32 {
    o.status.code().unwrap_or(-1)
}

/// A minimal valid repo: one rule (`r`) and a file that violates it.
fn fixture() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join(".alint.yml"),
        "version: 1\n\
         rules:\n\
         \x20 - id: r\n\
         \x20   kind: final_newline\n\
         \x20   paths: [\"**/*.txt\"]\n\
         \x20   level: error\n",
    )
    .unwrap();
    std::fs::write(d.path().join("a.txt"), "no newline").unwrap();
    d
}

/// Subcommands that do NOT honor `--only`. (`check`/`fix` do and are excluded.)
const NON_ONLY_SUBCOMMANDS: &[&[&str]] = &[
    &["list"],
    &["explain", "r"],
    &["facts"],
    &["validate-config"],
    &["suggest"],
    &["baseline"],
];

#[test]
fn only_is_rejected_on_subcommands_that_do_not_honor_it() {
    let d = fixture();
    for sub in NON_ONLY_SUBCOMMANDS {
        let mut args: Vec<&str> = sub.to_vec();
        args.push("--only");
        args.push("some-id");
        let out = run(d.path(), &args);
        assert_ne!(
            code(&out),
            0,
            "`alint {} --only ...` must be rejected, not silently ignored\nstderr: {}",
            sub.join(" "),
            String::from_utf8_lossy(&out.stderr),
        );
    }
    // Sanity: check/fix DO accept --only (a real id parses + runs).
    assert_ne!(
        code(&run(d.path(), &["check", "--only", "r"])),
        2,
        "`check --only <real-id>` must be accepted",
    );
}

/// `list`/`explain`/`facts` have only a human and a json shape. Every other
/// `--format` must be rejected (exit 2), not silently rendered as human.
#[test]
fn format_limited_subcommands_reject_unsupported_formats() {
    let d = fixture();
    let unsupported = ["sarif", "github", "junit", "gitlab", "markdown", "agent"];
    // `validate-config` is format-limited too (M13): the subcommand-position
    // flag is rejected by clap's value parser; the global-position flag by the
    // handler gate (covered by the `validate-config-format-rejected` trycmd).
    let cases: &[&[&str]] = &[
        &["list"],
        &["facts"],
        &["explain", "r"],
        &["validate-config"],
    ];
    for base in cases {
        for fmt in unsupported {
            let mut args: Vec<&str> = base.to_vec();
            args.push("--format");
            args.push(fmt);
            let out = run(d.path(), &args);
            assert_eq!(
                code(&out),
                2,
                "`alint {} --format {fmt}` must be rejected (exit 2), not degraded to human",
                base.join(" "),
            );
        }
        // human + json are supported.
        for fmt in ["human", "json"] {
            let mut args: Vec<&str> = base.to_vec();
            args.push("--format");
            args.push(fmt);
            assert_ne!(
                code(&run(d.path(), &args)),
                2,
                "`alint {} --format {fmt}` must be accepted",
                base.join(" "),
            );
        }
    }
}

#[test]
fn check_fix_baseline_reject_a_file_path() {
    let d = fixture();
    // `a.txt` is a real FILE in the repo; passing it as the PATH must error.
    for sub in ["check", "fix", "baseline"] {
        let out = run(d.path(), &[sub, "a.txt"]);
        assert_ne!(
            code(&out),
            0,
            "`alint {sub} a.txt` (a file, not a dir) must error, not silently pass",
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("file") || stderr.contains("directory"),
            "`{sub}` file-path error should explain the directory requirement: {stderr}",
        );
    }
    // Sanity: the same subcommands accept the directory.
    assert_ne!(
        code(&run(d.path(), &["check", "."])),
        2,
        "`check .` (a directory) must be accepted",
    );
}
