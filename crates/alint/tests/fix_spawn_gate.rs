//! Fix-op spawn-gate RCE canary (R-SPAWNGATE, part 2 of 2) — end-to-end, through
//! the real `alint` binary.
//!
//! A *spawning fix op* (`git_untrack`, which runs `git rm --cached`) is the
//! file-writing/RCE analogue of a spawning rule kind: it may be declared **only**
//! in the user's own top-level config. Introducing one through any *untrusted*
//! channel — an `extends:`'d ruleset, a `templates:` block, a `require:`
//! sub-rule, or a nested `.alint.yml` — must be refused at LOAD, so the `git`
//! subprocess never runs.
//!
//! The DSL-level gate is unit-tested in `alint-dsl` (asserting `load()` errors).
//! What those can't prove is the property that matters: driving the **real
//! binary** in the exact mode that WOULD spawn (`fix --unsafe-fixes`, since
//! `git_untrack` is Unsafe), the smuggled untrack *never runs*. So every vector
//! arms a canary — a tracked `secret.key` in a real git repo — and asserts it is
//! STILL tracked after the run (the `git rm --cached` never fired). A
//! `positive_control` first proves the canary is real: the identical fix, in the
//! *trusted* top-level config, DOES untrack the file.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn alint_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

/// Is `git` on PATH? The canary needs a real repo, so skip cleanly on a git-less
/// box; CI always has `git`.
fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(alint_bin())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn alint")
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("run git")
        .status
        .success();
    assert!(ok, "git {args:?} failed");
}

/// A throwaway git repo with a tracked `secret.key` that a `git_untrack` rule
/// targets — the canary. It starts tracked; a refused fix must leave it tracked.
fn scaffold_git_repo() -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .prefix("alint-fix-spawngate-")
        .tempdir()
        .expect("tempdir");
    std::fs::write(dir.path().join("secret.key"), "KEY\n").unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["add", "--", "secret.key"]);
    dir
}

fn is_tracked(dir: &Path, name: &str) -> bool {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["ls-files", "--", name])
        .output()
        .expect("git ls-files");
    !out.stdout.is_empty()
}

/// The `git_untrack` fix rule fields (minus `id:`), targeting the tracked key.
/// The `fix:` block is two lines, so callers indent the whole vec uniformly.
fn untrack_rule_fields() -> Vec<String> {
    vec![
        "kind: file_absent".to_string(),
        "paths: \"**/*.key\"".to_string(),
        "git_tracked_only: true".to_string(),
        "level: error".to_string(),
        "fix:".to_string(),
        "  git_untrack: {}".to_string(),
    ]
}

/// Indent every line by `n` spaces (blank lines left empty).
fn indent(lines: &[String], n: usize) -> String {
    let pad = " ".repeat(n);
    lines
        .iter()
        .map(|l| format!("{pad}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Assert `alint fix --unsafe-fixes` refused to load (exit 2, naming the op + the
/// RCE risk) and — the security property — the `git rm --cached` never ran, so
/// `secret.key` is still tracked.
fn assert_refused_no_untrack(vector: &str, dir: &Path) {
    let out = run(dir, &["fix", "--unsafe-fixes", "."]);
    let code = out.status.code();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        code,
        Some(2),
        "[{vector}] expected exit 2 (config refused), got {code:?}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("arbitrary code"),
        "[{vector}] rejection should explain the RCE risk; stderr: {stderr}"
    );
    assert!(
        stderr.contains("git_untrack"),
        "[{vector}] rejection should name the offending op; stderr: {stderr}"
    );
    assert!(
        is_tracked(dir, "secret.key"),
        "[{vector}] RCE: the smuggled `git rm --cached` ran — the gate did not block the spawn"
    );
}

// ─── Positive control — the canary mechanism is real ────────────────────────

#[test]
fn positive_control_trusted_top_level_git_untrack_runs() {
    if !git_available() {
        return;
    }
    // The SAME git_untrack fix, in the user's own top-level config (the one
    // allowed place), must actually untrack — proving the "still tracked"
    // assertions below aren't vacuous and we don't over-reject the legit case.
    let dir = scaffold_git_repo();
    let body = format!(
        "version: 1\nrules:\n  - id: canary\n{}\n",
        indent(&untrack_rule_fields(), 4)
    );
    std::fs::write(dir.path().join(".alint.yml"), body).unwrap();

    let out = run(dir.path(), &["fix", "--unsafe-fixes", "."]);
    assert!(
        !is_tracked(dir.path(), "secret.key"),
        "positive control failed: a trusted top-level git_untrack did not untrack \
         (canary mechanism is broken, making the rejection tests vacuous). \
         exit={:?} stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    // ...and the file stays on disk (untrack ≠ delete).
    assert!(
        dir.path().join("secret.key").exists(),
        "untrack keeps the file"
    );
}

// ─── Untrusted vectors — every one must be refused before `git rm` runs ─────

#[test]
fn git_untrack_via_extends_rules_is_refused() {
    if !git_available() {
        return;
    }
    let dir = scaffold_git_repo();
    let base = format!(
        "version: 1\nrules:\n  - id: smuggled\n{}\n",
        indent(&untrack_rule_fields(), 4)
    );
    std::fs::write(dir.path().join("base.yml"), base).unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        "version: 1\nextends: [./base.yml]\nrules: []\n",
    )
    .unwrap();
    assert_refused_no_untrack("extends-rules", dir.path());
}

#[test]
fn git_untrack_via_extends_template_is_refused() {
    if !git_available() {
        return;
    }
    // The template-splice bypass: the fix hides in an inherited `templates:`
    // block, pulled in by a `kind`-less `extends_template:` that expands after
    // the rule-level gate.
    let dir = scaffold_git_repo();
    let base = format!(
        "version: 1\ntemplates:\n  - id: t\n{}\nrules:\n  - id: smuggled\n    level: error\n    extends_template: t\n",
        indent(&untrack_rule_fields(), 4)
    );
    std::fs::write(dir.path().join("base.yml"), base).unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        "version: 1\nextends: [./base.yml]\nrules: []\n",
    )
    .unwrap();
    assert_refused_no_untrack("extends-template", dir.path());
}

#[test]
fn git_untrack_via_extends_require_block_is_refused() {
    if !git_available() {
        return;
    }
    // Buried in a `require:` sub-rule of a `for_each_dir`, whose nested fix would
    // flatten in — the gate must recurse into `require:`.
    let dir = scaffold_git_repo();
    // Fields at 8 spaces; turn the first (`kind:`) into the `- ` list item by
    // swapping its leading indent for `      - ` (same width), leaving the rest at
    // 8 so they stay inside the one require entry.
    let fields = indent(&untrack_rule_fields(), 8);
    let fields = fields.replacen("        kind:", "      - kind:", 1);
    let base = format!(
        "version: 1\nrules:\n  - id: outer\n    kind: for_each_dir\n    select: \"**/\"\n    require:\n{fields}\n    level: error\n",
    );
    std::fs::write(dir.path().join("base.yml"), base).unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        "version: 1\nextends: [./base.yml]\nrules: []\n",
    )
    .unwrap();
    assert_refused_no_untrack("extends-require", dir.path());
}

#[test]
fn git_untrack_via_nested_config_is_refused() {
    if !git_available() {
        return;
    }
    // A nested `.alint.yml` is as untrusted as an `extends:`'d ruleset.
    let dir = scaffold_git_repo();
    std::fs::write(
        dir.path().join(".alint.yml"),
        "version: 1\nnested_configs: true\nrules: []\n",
    )
    .unwrap();
    let pkg = dir.path().join("packages/foo");
    std::fs::create_dir_all(&pkg).unwrap();
    let nested = format!(
        "version: 1\nrules:\n  - id: smuggled\n{}\n",
        indent(&untrack_rule_fields(), 4)
    );
    std::fs::write(pkg.join(".alint.yml"), nested).unwrap();
    assert_refused_no_untrack("nested-config", dir.path());
}
