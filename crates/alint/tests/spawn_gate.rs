//! Spawn-gate RCE canary — end-to-end, through the real `alint` binary.
//!
//! alint's process-execution trust boundary: a rule kind that shells out
//! (`command`, `command_idempotent`, `generated_file_fresh` — the
//! `alint_dsl::SPAWNING_RULE_KINDS` allow-list) may be declared **only** in
//! the user's own top-level config. Introducing one through any *untrusted*
//! channel — an `extends:`'d ruleset, a `templates:` block, a `require:`
//! sub-rule, or a nested `.alint.yml` — must be refused, because anyone who
//! can land such a config (open a PR, publish a ruleset) would otherwise gain
//! arbitrary code execution on every `alint check`.
//!
//! The DSL-level gate logic is unit-tested in `alint-dsl` (it asserts
//! `load()` returns an `Err`). What those tests can't prove is the property
//! that actually matters: driving the **real binary**, the smuggled command
//! *never runs*. So every case here arms a canary — the smuggled command is
//! `touch <sentinel>` — and asserts the sentinel file does not exist after the
//! run. A `positive_control` test first proves the canary mechanism is real:
//! the identical command, declared in the *trusted* top-level config, DOES
//! create its sentinel. Without that control, "sentinel absent" could pass
//! vacuously (e.g. a malformed command that never touches anything).
//!
//! `#![cfg(unix)]` — the canary uses `/bin/sh` + `touch`.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;

fn alint_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn run(dir: &Path, args: &[&str]) -> Output {
    std::process::Command::new(alint_bin())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn alint")
}

/// The rule fields (minus `id:`) for a spawning `kind` whose `command`
/// touches `sentinel`. Each kind gets exactly the fields it needs so a
/// rejection can never be mistaken for a schema error. A new spawning kind
/// added to the allow-list forces a body here — the `panic!` keeps this
/// matrix from silently skipping it.
fn spawn_rule_fields(kind: &str, sentinel: &Path) -> Vec<String> {
    let mut lines = vec![format!("kind: {kind}")];
    match kind {
        "command" => lines.push("paths: \"**/*\"".to_string()),
        "generated_file_fresh" => lines.push("file: out.txt".to_string()),
        "command_idempotent" => {}
        other => panic!(
            "unhandled spawning kind {other:?}; add its minimal fields to spawn_rule_fields \
             so the spawn-gate canary matrix covers it"
        ),
    }
    // Flow-sequence (JSON-in-YAML) keeps quoting simple. tempfile paths are
    // random alphanumerics under /tmp — no shell-unsafe characters.
    lines.push(format!(
        "command: [\"sh\", \"-c\", \"touch {}\"]",
        sentinel.display()
    ));
    lines.push("level: error".to_string());
    lines
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

/// Assert `alint check` refused to load (exit 2, "arbitrary code" in the
/// diagnostic) and — the security property — the canary never fired.
fn assert_refused_no_rce(vector: &str, kind: &str, dir: &Path, sentinel: &Path) {
    let out = run(dir, &["check", "."]);
    let code = out.status.code();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        code,
        Some(2),
        "[{vector}/{kind}] expected exit 2 (config refused), got {code:?}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("arbitrary code"),
        "[{vector}/{kind}] rejection should explain the RCE risk; stderr: {stderr}"
    );
    assert!(
        stderr.contains(kind),
        "[{vector}/{kind}] rejection should name the offending kind; stderr: {stderr}"
    );
    assert!(
        !sentinel.exists(),
        "[{vector}/{kind}] RCE: the smuggled `touch` ran — the gate did not block execution"
    );
}

/// A throwaway repo with one data file (so a `paths: **/*` rule would have
/// something to run on) and an out-of-band sentinel path that starts absent.
fn scaffold() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("alint-spawngate-")
        .tempdir()
        .expect("tempdir");
    std::fs::write(dir.path().join("data.txt"), "hi\n").unwrap();
    let sentinel = dir.path().join("SENTINEL");
    (dir, sentinel)
}

/// The spawning kinds, read from the crate under test so this matrix tracks
/// the allow-list automatically.
fn spawning_kinds() -> &'static [&'static str] {
    alint_dsl::SPAWNING_RULE_KINDS
}

// ─── Positive control — the canary mechanism is real ────────────────────

#[test]
fn positive_control_trusted_top_level_command_executes() {
    // The SAME `touch <sentinel>` command, declared in the user's own
    // top-level config (the one allowed place), must actually run — proving
    // the sentinel assertions below aren't vacuous and that we don't
    // over-reject the legitimate case.
    let (dir, sentinel) = scaffold();
    let body = format!(
        "version: 1\nrules:\n  - id: canary\n{}\n",
        indent(&spawn_rule_fields("command", &sentinel), 4)
    );
    std::fs::write(dir.path().join(".alint.yml"), body).unwrap();

    let out = run(dir.path(), &["check", "."]);
    assert!(
        sentinel.exists(),
        "positive control failed: a trusted top-level command rule did not run \
         (canary mechanism is broken, making the rejection tests vacuous). \
         exit={:?} stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
}

// ─── Untrusted vectors — every spawning kind must be refused ────────────

#[test]
fn spawning_kind_via_extends_rules_is_refused() {
    // Primary vector: a spawning `kind:` sitting directly in an `extends:`'d
    // ruleset's `rules:`. Covered for every allow-listed kind.
    for &kind in spawning_kinds() {
        let (dir, sentinel) = scaffold();
        let base = format!(
            "version: 1\nrules:\n  - id: smuggled\n{}\n",
            indent(&spawn_rule_fields(kind, &sentinel), 4)
        );
        std::fs::write(dir.path().join("base.yml"), base).unwrap();
        std::fs::write(
            dir.path().join(".alint.yml"),
            "version: 1\nextends: [./base.yml]\nrules: []\n",
        )
        .unwrap();
        assert_refused_no_rce("extends-rules", kind, dir.path(), &sentinel);
    }
}

#[test]
fn spawning_kind_via_extends_template_is_refused() {
    // C1 bypass: a spawning kind hidden in an inherited `templates:` block,
    // pulled in by a `kind`-less `extends_template:` that expands *after* the
    // top-level `rules[].kind` gate.
    for &kind in spawning_kinds() {
        let (dir, sentinel) = scaffold();
        let base = format!(
            "version: 1\ntemplates:\n  - id: t\n{}\nrules:\n  - id: smuggled\n    extends_template: t\n",
            indent(&spawn_rule_fields(kind, &sentinel), 4)
        );
        std::fs::write(dir.path().join("base.yml"), base).unwrap();
        std::fs::write(
            dir.path().join(".alint.yml"),
            "version: 1\nextends: [./base.yml]\nrules: []\n",
        )
        .unwrap();
        assert_refused_no_rce("extends-template", kind, dir.path(), &sentinel);
    }
}

#[test]
fn spawning_kind_via_extends_require_block_is_refused() {
    // Third vector: buried in a `require:` sub-rule of a `for_each_dir`, whose
    // nested `kind` flattens into the parent's options — the top-level `kind`
    // check would miss it, so the gate must recurse.
    for &kind in spawning_kinds() {
        let (dir, sentinel) = scaffold();
        // Fields align at 8 spaces; turn the first (`kind:`) into the `- `
        // list item by swapping its leading indent for `      - ` (same width),
        // leaving the rest at 8 so they stay inside the one require entry.
        let fields = indent(&spawn_rule_fields(kind, &sentinel), 8);
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
        assert_refused_no_rce("extends-require", kind, dir.path(), &sentinel);
    }
}

#[test]
fn spawning_kind_via_nested_config_is_refused() {
    // C2 bypass: a nested `.alint.yml` is as untrusted as an `extends:`'d
    // ruleset (anyone opening a monorepo PR can add one), so it may not
    // declare a spawning kind either.
    for &kind in spawning_kinds() {
        let (dir, sentinel) = scaffold();
        std::fs::write(
            dir.path().join(".alint.yml"),
            "version: 1\nnested_configs: true\nrules: []\n",
        )
        .unwrap();
        let pkg = dir.path().join("packages/foo");
        std::fs::create_dir_all(&pkg).unwrap();
        let nested = format!(
            "version: 1\nrules:\n  - id: smuggled\n{}\n",
            indent(&spawn_rule_fields(kind, &sentinel), 4)
        );
        std::fs::write(pkg.join(".alint.yml"), nested).unwrap();
        assert_refused_no_rce("nested-config", kind, dir.path(), &sentinel);
    }
}
