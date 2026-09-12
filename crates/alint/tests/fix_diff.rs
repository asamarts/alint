//! End-to-end gate for `alint fix --diff`. The flag previews the composed fix
//! result as a unified diff and MUST NOT write: the load-bearing assertion here
//! is `diff_never_mutates_the_tree`, which exercises all four op kinds (in-place
//! modify, create, delete, rename) and fails if any byte on disk changes -- the
//! regression that shipped when whole-file fixers (create/remove/rename) wrote
//! directly during a stage. It also checks the diff renders each kind with the
//! git `/dev/null` and `rename from`/`rename to` conventions, and that the exit
//! code matches a real `fix`.

use std::collections::BTreeMap;
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

/// A repo exercising every fixer op kind at once: an in-place content edit
/// (trailing whitespace), a create (missing README), a delete (a committed
/// `debug.log`), and a rename (a non-snake-case source).
const CONFIG: &str = "\
version: 1
rules:
  - id: no-ws
    kind: no_trailing_whitespace
    paths: \"src/**/*.rs\"
    level: error
    fix: { file_trim_trailing_whitespace: {} }
  - id: no-log
    kind: file_absent
    paths: \"debug.log\"
    level: error
    fix: { file_remove: {} }
  - id: need-readme
    kind: file_exists
    paths: \"README.md\"
    level: error
    fix: { file_create: { content: \"# Project\\n\" } }
  - id: rust-snake
    kind: filename_case
    paths: \"src/**/*.rs\"
    case: snake
    level: error
    fix: { file_rename: {} }
";

fn setup() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join(".alint.yml"), CONFIG).unwrap();
    std::fs::write(tmp.path().join("src/bad.rs"), "fn a() {}   \n").unwrap();
    std::fs::write(tmp.path().join("src/FooBar.rs"), "pub fn a() {}\n").unwrap();
    std::fs::write(tmp.path().join("debug.log"), "noise\n").unwrap();
    tmp
}

/// Snapshot every file under `root` as `relative path -> bytes`, so a before/
/// after comparison catches a create, delete, rename, or content change.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                let rel = path.strip_prefix(root).unwrap().to_path_buf();
                out.insert(rel, std::fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

#[test]
fn diff_never_mutates_the_tree() {
    let tmp = setup();
    let before = snapshot(tmp.path());

    let out = run(tmp.path(), &["fix", "--diff", "."]);
    assert!(out.status.success() || out.status.code() == Some(1));

    let after = snapshot(tmp.path());
    assert_eq!(
        before, after,
        "`fix --diff` must not change any file on disk (create/delete/rename/modify all staged only)"
    );
}

fn git(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("run git")
}

#[test]
fn diff_output_applies_cleanly_with_git_apply() {
    // Round-5 audit: the `--diff` output is documented as consumable by
    // `git apply`. The rename op is a git-only construct that needs a
    // `diff --git` envelope; without it `git apply` silently drops the rename
    // (or rejects the whole patch). This gate applies the real preview to a git
    // tree covering ALL four op kinds and asserts the result matches a real fix.
    let tmp = setup();
    let root = tmp.path();
    // A committed baseline so `git apply` has something to patch against.
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);

    // Capture the preview, then apply it.
    let diff = run(root, &["fix", "--diff", "."]);
    let patch = root.join("fix.patch");
    std::fs::write(&patch, &diff.stdout).unwrap();
    let check = git(root, &["apply", "--check", "fix.patch"]);
    assert!(
        check.status.success(),
        "`git apply --check` must accept the --diff output (all four op kinds); stderr:\n{}\n--- patch ---\n{}",
        String::from_utf8_lossy(&check.stderr),
        String::from_utf8_lossy(&diff.stdout),
    );
    let applied = git(root, &["apply", "fix.patch"]);
    assert!(
        applied.status.success(),
        "git apply failed: {}",
        String::from_utf8_lossy(&applied.stderr)
    );
    std::fs::remove_file(&patch).unwrap();

    // The applied tree must match what a real `alint fix` produces.
    assert!(root.join("README.md").exists(), "create applied");
    assert_eq!(
        std::fs::read_to_string(root.join("README.md")).unwrap(),
        "# Project\n"
    );
    assert!(!root.join("debug.log").exists(), "delete applied");
    assert!(
        root.join("src/foo_bar.rs").exists() && !root.join("src/FooBar.rs").exists(),
        "rename applied (this is the regression: bare rename lines are dropped by git apply)"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("src/bad.rs")).unwrap(),
        "fn a() {}\n",
        "modify applied"
    );
}

#[test]
fn diff_renders_every_op_kind() {
    let tmp = setup();
    let out = run(tmp.path(), &["fix", "--diff", "."]);
    let stdout = String::from_utf8_lossy(&out.stdout);

    // Create: /dev/null on the old side, and the new content line.
    assert!(
        stdout.contains("--- /dev/null") && stdout.contains("+++ b/README.md"),
        "create should diff against /dev/null; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("+# Project"),
        "create should show the new content; stdout:\n{stdout}"
    );
    // Delete: /dev/null on the new side.
    assert!(
        stdout.contains("--- a/debug.log") && stdout.contains("+++ /dev/null"),
        "delete should diff to /dev/null; stdout:\n{stdout}"
    );
    // Rename: explicit git rename header lines.
    assert!(
        stdout.contains("rename from src/FooBar.rs") && stdout.contains("rename to src/foo_bar.rs"),
        "rename should show rename from/to; stdout:\n{stdout}"
    );
    // Modify: an in-place a/ b/ hunk on the trailing-whitespace file.
    assert!(
        stdout.contains("--- a/src/bad.rs") && stdout.contains("+++ b/src/bad.rs"),
        "modify should show an in-place hunk; stdout:\n{stdout}"
    );
}

#[test]
fn diff_exit_code_matches_a_real_fix() {
    // Every finding here is fixable, so a real fix would exit 0; --diff matches.
    let tmp = setup();
    let out = run(tmp.path(), &["fix", "--diff", "."]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "all findings fixable -> --diff exits 0 like a real fix"
    );
}

#[test]
fn diff_exits_nonzero_on_an_unfixable_error() {
    // Add an unfixable error-level rule (a forbidden pattern with no fix): a
    // real fix would exit 1, and so must --diff.
    let tmp = setup();
    let cfg = format!(
        "{CONFIG}  - id: no-todo\n    kind: file_content_forbidden\n    paths: \"src/**/*.rs\"\n    pattern: TODO\n    level: error\n"
    );
    std::fs::write(tmp.path().join(".alint.yml"), cfg).unwrap();
    std::fs::write(tmp.path().join("src/bad.rs"), "fn a() {}   \nTODO\n").unwrap();

    let before = snapshot(tmp.path());
    let out = run(tmp.path(), &["fix", "--diff", "."]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "an unfixable error exits nonzero, matching a real fix"
    );
    // Still no writes, even on the failing path.
    assert_eq!(before, snapshot(tmp.path()), "--diff must not write");
}

#[test]
fn diff_with_fix_only_exits_zero_despite_residual() {
    // --fix-only flips the exit predicate to "0 unless a fix errored"; combined
    // with --diff (which never writes, so never errors) it exits 0.
    let tmp = setup();
    let cfg = format!(
        "{CONFIG}  - id: no-todo\n    kind: file_content_forbidden\n    paths: \"src/**/*.rs\"\n    pattern: TODO\n    level: error\n"
    );
    std::fs::write(tmp.path().join(".alint.yml"), cfg).unwrap();
    std::fs::write(tmp.path().join("src/bad.rs"), "fn a() {}   \nTODO\n").unwrap();

    let out = run(tmp.path(), &["fix", "--diff", "--fix-only", "."]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "--diff --fix-only exits 0 even with an unfixable residual"
    );
}

#[test]
fn diff_ignores_a_finding_oriented_format() {
    // A finding-oriented `--format` (SARIF) is rejected for a real fix report,
    // but --diff emits a diff, not a report, so it is format-independent: the
    // stray flag yields the diff, not an error.
    let tmp = setup();
    let out = run(tmp.path(), &["fix", "--diff", "--format", "sarif", "."]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("rename from src/FooBar.rs") && stdout.contains("--- a/src/bad.rs"),
        "--diff should emit the diff regardless of --format; stdout:\n{stdout}"
    );
    // Not the fix-report-format error.
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("supports only"),
        "--diff must not trip the fix-report-format gate; stderr:\n{stderr}"
    );
}

#[test]
fn diff_still_rejects_a_bogus_format_string() {
    // The format string is still parsed, so a genuine typo fails loudly even
    // under --diff (it just isn't held to the fix-report-format restriction).
    let tmp = setup();
    let out = run(tmp.path(), &["fix", "--diff", "--format", "bogus", "."]);
    assert_ne!(out.status.code(), Some(0), "a bogus format must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown output format"),
        "the parse error should name the bad format; stderr:\n{stderr}"
    );
}
