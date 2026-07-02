//! CLI output-contract gates — two regression classes that shipped
//! broken in 0.13.0 and must never silently reappear:
//!
//! * **G1a — `--format json` is never a silent no-op.** `alint list`
//!   and `alint explain` advertised the global `--format` flag but
//!   ignored it, printing the *human* rule list with a *success* exit
//!   code — so automation built on `list --format json` silently got
//!   colourised text. The invariant: for every subcommand that emits
//!   to stdout, `--format json` must yield parseable JSON OR exit
//!   non-zero. It may never print non-JSON to stdout *and* exit 0.
//!
//! * **G1b — the `agent` format only emits runnable commands.** The
//!   agent format told agents to run `alint fix --only <id>`, a flag
//!   that did not exist (`fix`/`check` rejected `--only` with exit 2).
//!   The invariant: every command the agent format emits — the
//!   structured `fix_command` argv *and* any `` `alint …` `` command
//!   inside an `agent_instruction` — must parse against the real CLI.
//!
//! * **G1c — `fix` never silently degrades an output format.** `fix`
//!   only renders `human` / `json` / `markdown`; the finding-oriented
//!   formats (SARIF / GitHub / `JUnit` / GitLab) and the check-side
//!   `agent` format used to fall through to human text with a *success*
//!   exit code. Because `fix` mutates the tree, the invariant is
//!   stricter than for read-only subcommands: an unrenderable format
//!   must fail (exit 2) *before* any file is touched.
//!
//! All gates drive the actual binary (`CARGO_BIN_EXE_alint`) so they
//! exercise the same clap surface and renderers users hit.

use std::path::{Path, PathBuf};
use std::process::Command;

fn alint_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

/// A throwaway repo that loads real rules (extends the bundled
/// `oss-baseline`) and contains a fixable violation (a file with no
/// trailing newline), so `check`/`fix`/`agent` all have something to
/// say.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(".alint.yml"),
        "version: 1\nextends:\n  - alint://bundled/oss-baseline@v1\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("bad.md"), "no trailing newline").unwrap();
    dir
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(alint_bin())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn alint")
}

// ─── G1a — `--format json` is never a silent human no-op ────────────

/// Every subcommand that writes a report to stdout. `explain` needs a
/// rule id, injected at call time. `fix` runs `--dry-run` so the gate
/// never mutates the fixture.
const JSON_STDOUT_SUBCOMMANDS: &[&[&str]] = &[
    &["check"],
    &["list"],
    &["fix", "--dry-run"],
    &["facts"],
    &["suggest"],
    &["validate-config"],
    // explain is appended with a concrete id below.
];

#[test]
fn format_json_is_never_a_silent_human_fallthrough() {
    let dir = fixture();

    // Discover a real rule id from the (now-JSON) list inventory so
    // `explain` is exercised against an id that exists.
    let list = run(dir.path(), &["list", "--format", "json"]);
    let list_json: serde_json::Value =
        serde_json::from_slice(&list.stdout).expect("list --format json must be JSON");
    let some_id = list_json["rules"][0]["id"]
        .as_str()
        .expect("at least one rule in the fixture")
        .to_string();

    let mut cases: Vec<Vec<String>> = JSON_STDOUT_SUBCOMMANDS
        .iter()
        .map(|c| c.iter().map(|s| (*s).to_string()).collect())
        .collect();
    cases.push(vec!["explain".into(), some_id]);

    for case in &cases {
        let mut args: Vec<&str> = case.iter().map(String::as_str).collect();
        args.push("--format");
        args.push("json");
        let out = run(dir.path(), &args);

        // The silent-failure signature is: exit 0 with non-JSON on
        // stdout. Honouring the flag (valid JSON) or rejecting it
        // (non-zero exit) are both acceptable; printing human text
        // and claiming success is not.
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.trim().is_empty() {
                continue; // nothing emitted is fine (e.g. no findings)
            }
            assert!(
                serde_json::from_slice::<serde_json::Value>(&out.stdout).is_ok(),
                "`alint {} --format json` exited 0 but stdout was not JSON \
                 (silent --format no-op regression). stdout begins:\n{}",
                case.join(" "),
                &stdout.chars().take(200).collect::<String>(),
            );
        }
    }
}

// ─── G1b — the agent format only emits commands the CLI accepts ─────

/// Pull `` `alint …` `` commands out of an `agent_instruction` string:
/// the substring between a backtick-quoted `alint ` and the next
/// backtick, returned as argv (program name dropped).
fn alint_commands_in(prose: &str) -> Vec<Vec<String>> {
    let mut cmds = Vec::new();
    let mut rest = prose;
    while let Some(start) = rest.find("`alint ") {
        let after = &rest[start + 1..]; // skip the opening backtick
        if let Some(end) = after.find('`') {
            let cmd = &after[..end];
            let argv: Vec<String> = cmd
                .split_whitespace()
                .skip(1) // drop "alint"
                .map(str::to_string)
                .collect();
            if !argv.is_empty() {
                cmds.push(argv);
            }
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    cmds
}

/// Assert an argv parses against the real CLI: a clap parse failure
/// exits 2 with "unexpected argument" / "error:" on stderr. Append
/// `--dry-run` so a `fix` command never writes during the check.
fn assert_parses(dir: &Path, argv: &[String]) {
    let mut invocation: Vec<&str> = argv.iter().map(String::as_str).collect();
    if argv.first().map(String::as_str) == Some("fix") {
        invocation.push("--dry-run");
    }
    let out = run(dir, &invocation);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.code() != Some(2) && !stderr.contains("unexpected argument"),
        "agent format emitted a command the CLI rejects: `alint {}`\n\
         exit={:?}\nstderr: {}",
        argv.join(" "),
        out.status.code(),
        stderr.trim(),
    );
}

#[test]
fn agent_format_only_emits_runnable_commands() {
    let dir = fixture();
    let out = run(dir.path(), &["check", "--format", "agent"]);
    let report: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("agent format is JSON");
    let violations = report["violations"].as_array().expect("violations array");

    let mut checked_fix_command = false;
    let mut checked_prose = false;

    for v in violations {
        // 1. The structured fix_command argv must parse.
        if let Some(cmd) = v["fix_command"].as_array() {
            let argv: Vec<String> = cmd
                .iter()
                .map(|s| s.as_str().expect("argv element is a string").to_string())
                .collect();
            assert_parses(dir.path(), &argv);
            checked_fix_command = true;

            // And it must agree with fix_available.
            assert_eq!(
                v["fix_available"].as_bool(),
                Some(true),
                "fix_command present but fix_available is not true: {v}"
            );
        }

        // 2. Any `alint …` command embedded in the prose must parse
        //    too — prose drift can't reintroduce a dead command.
        if let Some(instr) = v["agent_instruction"].as_str() {
            for argv in alint_commands_in(instr) {
                assert_parses(dir.path(), &argv);
                checked_prose = true;
            }
        }
    }

    // The fixture is constructed to contain a fixable violation, so
    // both code paths must actually have run — otherwise the gate is
    // vacuously green.
    assert!(
        checked_fix_command,
        "fixture produced no fixable violation; gate did not exercise fix_command"
    );
    assert!(
        checked_prose,
        "fixture produced no agent_instruction with an `alint …` command"
    );
}

// ─── G1c — `fix` rejects formats it can't render, before mutating ───

/// The finding-oriented formats (SARIF / GitHub / `JUnit` / GitLab) and the
/// check-side `agent` format have no fix-report renderer. `fix` used to
/// degrade them *silently* to human output with a success exit code; it must
/// now fail loudly (exit 2). And because `fix` mutates the tree, the reject
/// must land *before* any fix is applied — hence a non-dry-run invocation
/// here, asserting the fixable file is left byte-for-byte untouched.
#[test]
fn fix_rejects_unrenderable_formats_without_mutating() {
    let dir = fixture();
    let bad = dir.path().join("bad.md");
    let original = std::fs::read(&bad).unwrap();

    for fmt in ["sarif", "github", "junit", "gitlab", "agent"] {
        let out = run(dir.path(), &["fix", "--format", fmt]);
        assert_eq!(
            out.status.code(),
            Some(2),
            "`alint fix --format {fmt}` must exit 2, not silently degrade to human"
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("supports only"),
            "`alint fix --format {fmt}` should name the supported set; stderr: {stderr}"
        );
        assert_eq!(
            std::fs::read(&bad).unwrap(),
            original,
            "`alint fix --format {fmt}` mutated the tree before rejecting the format"
        );
    }
}

/// Guard against over-rejection: the three formats `fix` *can* render must
/// still pass the gate. `json` must additionally be real JSON on stdout.
#[test]
fn fix_accepts_its_renderable_formats() {
    let dir = fixture();
    for fmt in ["human", "json", "markdown"] {
        let out = run(dir.path(), &["fix", "--dry-run", "--format", fmt]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("supports only"),
            "`alint fix --format {fmt}` was wrongly rejected; stderr: {stderr}"
        );
        if fmt == "json" {
            assert!(
                serde_json::from_slice::<serde_json::Value>(&out.stdout).is_ok(),
                "`alint fix --dry-run --format json` stdout was not JSON"
            );
        }
    }
}
