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
    &["rules", "list"],
    &["rules", "categories"],
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
                stdout.chars().take(200).collect::<String>(),
            );
        }
    }
}

// ─── Explain completeness — output surfaces the rule's configured detail ─
//
// `explain` used to print only id/level/policy_url, silently dropping the
// rule's kind, paths, and author `message` — all present in the loaded config,
// and a byte-exact snapshot froze that thin output as "correct". This gate is a
// POSITIVE completeness invariant: for a rule that sets kind/paths/message,
// explain's human AND json output must actually contain those values, so the
// `RuleSpec` -> `RuleEntry` projection can't quietly go lossy again. The
// generalisation to every rule kind is tracked in ADR-0012.
#[test]
fn explain_surfaces_configured_rule_detail() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(".alint.yml"),
        "version: 1\n\
         rules:\n\
        \x20 - id: needs-readme\n\
        \x20   kind: file_exists\n\
        \x20   paths: README.md\n\
        \x20   level: error\n\
        \x20   message: \"README.md must exist at the repository root.\"\n",
    )
    .unwrap();

    // JSON: the machine contract must carry the rule's kind, message, paths,
    // and categories — not merely parse as *some* JSON (the old G1a-only gate).
    let out = run(dir.path(), &["explain", "needs-readme", "--format", "json"]);
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("explain --format json must be JSON");
    assert_eq!(
        v["rule_kind"], "file_exists",
        "explain json dropped the rule's kind: {v}"
    );
    assert_eq!(
        v["message"], "README.md must exist at the repository root.",
        "explain json dropped the author message: {v}"
    );
    assert_eq!(
        v["paths"]["include"][0], "README.md",
        "explain json dropped the rule's paths: {v}"
    );
    assert!(
        v["categories"].as_array().is_some_and(|c| !c.is_empty()),
        "explain json dropped the kind's categories: {v}"
    );

    // Human: the same detail must be visible, not just id/level.
    let out = run(dir.path(), &["explain", "needs-readme"]);
    let s = String::from_utf8_lossy(&out.stdout);
    for needle in [
        "kind:",
        "file_exists",
        "paths:",
        "README.md",
        "message:",
        "README.md must exist at the repository root.",
    ] {
        assert!(
            s.contains(needle),
            "explain human output is missing {needle:?}:\n{s}"
        );
    }
}

// ─── Explain edge cases — negation paths + blank message ───────────
//
// Guards two adversarial-review findings: an inline `!` negation in a paths
// list must report as an exclude (not a bogus include), and a blank message
// must render as absent (no dangling `message:` label, json null).
#[test]
fn explain_handles_negation_paths_and_blank_message() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(".alint.yml"),
        r#"version: 1
rules:
  - id: neg
    kind: file_content_forbidden
    paths: ["src/**", "!src/gen/**"]
    pattern: "TODO"
    level: warning
  - id: blank
    kind: file_exists
    paths: X
    level: error
    message: ""
"#,
    )
    .unwrap();

    // An inline `!` negation is an exclude, mirroring the scope matcher.
    let v: serde_json::Value =
        serde_json::from_slice(&run(dir.path(), &["explain", "neg", "--format", "json"]).stdout)
            .expect("json");
    assert_eq!(v["paths"]["include"][0], "src/**");
    assert_eq!(
        v["paths"]["exclude"][0], "src/gen/**",
        "inline `!` negation must report as an exclude, not an include: {v}"
    );
    let human = String::from_utf8_lossy(&run(dir.path(), &["explain", "neg"]).stdout).into_owned();
    assert!(
        human.contains("(excluding src/gen/**)"),
        "human paths must show the negation as an exclusion:\n{human}"
    );

    // A blank message renders as absent: json null, no human label.
    let v: serde_json::Value =
        serde_json::from_slice(&run(dir.path(), &["explain", "blank", "--format", "json"]).stdout)
            .expect("json");
    assert!(
        v["message"].is_null(),
        "blank message must be json null: {v}"
    );
    let human =
        String::from_utf8_lossy(&run(dir.path(), &["explain", "blank"]).stdout).into_owned();
    assert!(
        !human.contains("message:"),
        "blank message must not print a dangling label:\n{human}"
    );
}

// ─── Explain covers every registered kind (all_kinds.yaml) ─────────
//
// ADR-0012's generalisation of the per-rule completeness gate: drive the
// `all_kinds.yaml` fixture (a rule for ~every registered kind) through
// `explain --format json` and assert each renders valid JSON with its id and
// kind round-tripping and categories present. A newly registered kind that
// rendered blank, mis-reported its kind, or crashed `explain` fails here.
#[test]
fn explain_covers_every_registered_kind() {
    let fixture = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../alint-dsl/tests/fixtures/all_kinds.yaml"
    ))
    .expect("read all_kinds.yaml");
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".alint.yml"), &fixture).unwrap();

    let list: serde_json::Value =
        serde_json::from_slice(&run(dir.path(), &["list", "--format", "json"]).stdout)
            .expect("list --format json must be JSON");
    let rules = list["rules"].as_array().expect("rules array");
    assert!(
        rules.len() > 60,
        "all_kinds fixture should exercise many kinds (got {})",
        rules.len()
    );

    for r in rules {
        let id = r["id"].as_str().expect("rule id");
        let kind = r["kind"].as_str().expect("rule kind");
        let out = run(dir.path(), &["explain", id, "--format", "json"]);
        let ev: serde_json::Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|_| panic!("explain {id} --format json must be valid JSON"));
        assert_eq!(ev["id"], id, "explain mis-reported the id for {id}: {ev}");
        assert_eq!(
            ev["rule_kind"], kind,
            "explain mis-reported the kind for {id}: {ev}"
        );
        assert!(
            ev["categories"].as_array().is_some(),
            "explain dropped categories for {id}: {ev}"
        );
    }
}

// ─── list human surfaces kind + fixable/conditional markers ────────
//
// #163 added `kind`, `[fix]` (fixable), and `[when]` (conditional) markers to
// `alint list`'s human output (parity with `list --format json`). The
// `list.stdout` snapshot fixture has no fixable/conditional rule, so it
// exercises only `kind`; this asserts the markers directly.
#[test]
fn list_human_surfaces_kind_and_markers() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(".alint.yml"),
        r#"version: 1
rules:
  - id: rule-a
    kind: file_exists
    paths: .editorconfig
    level: error
    fix: { file_create: { content: "x\n" } }
  - id: rule-b
    kind: file_exists
    paths: rust-toolchain.toml
    level: info
    when: "facts.has_rust"
  - id: rule-c
    kind: file_absent
    paths: "**/*.bak"
    level: warning
"#,
    )
    .unwrap();

    let s = String::from_utf8_lossy(&run(dir.path(), &["list"]).stdout).into_owned();
    // Every rule shows its kind (parity with `list --format json`).
    assert!(
        s.contains("file_exists") && s.contains("file_absent"),
        "list human dropped the kind:\n{s}"
    );
    let line = |id: &str| s.lines().find(|l| l.contains(id)).unwrap_or("").to_string();
    // A fixable rule shows `[fix]`; a non-fixable one does not.
    assert!(
        line("rule-a").contains("[fix]"),
        "fixable rule missing [fix]: {:?}",
        line("rule-a")
    );
    assert!(
        !line("rule-c").contains("[fix]"),
        "non-fixable rule has a stray [fix]: {:?}",
        line("rule-c")
    );
    // A conditional rule shows `[when]`.
    assert!(
        line("rule-b").contains("[when]"),
        "conditional rule missing [when]: {:?}",
        line("rule-b")
    );
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

// ─── Phase 2: `alint rules` catalog + `list --category` ────────────────
//
// The catalog (`rules`) is config-independent; `list --category` is
// config-scoped. Both map kinds through the generated in-crate bridge.

/// ADR-0009 invariant: `alint rules` NEVER reads a config (succeeds with none),
/// while `alint list` REQUIRES one. Backs the "config-independent" claim.
#[test]
fn rules_are_config_independent_but_list_is_not() {
    let dir = tempfile::tempdir().expect("tempdir"); // deliberately no .alint.yml
    for args in [&["rules", "list"][..], &["rules", "categories"][..]] {
        let out = run(dir.path(), args);
        assert!(
            out.status.success(),
            "`alint {args:?}` must succeed without a config; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !out.stdout.is_empty(),
            "`alint {args:?}` produced no output"
        );
    }
    let out = run(dir.path(), &["list"]);
    assert!(!out.status.success(), "`alint list` must require a config");
    let out = run(dir.path(), &["list", "--category", "naming"]);
    assert!(
        !out.status.success(),
        "`alint list --category` must not relax the config requirement"
    );
}

/// `rules list`/`categories` output: canonical-only rows, aliases annotated,
/// `--category` filters, unknown slug errors, vocabulary listed.
#[test]
fn rules_catalog_output() {
    let dir = tempfile::tempdir().expect("tempdir");

    let out = run(dir.path(), &["rules", "list"]);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("no_bidi_controls"),
        "catalog missing a kind: {s}"
    );
    assert!(
        s.contains("file_content_matches") && s.contains("(alias: content_matches)"),
        "aliases must be annotated on their canonical row"
    );
    assert!(
        !s.lines()
            .any(|l| l.trim_start().starts_with("content_matches ")),
        "an alias must not be its own catalog row"
    );

    let out = run(dir.path(), &["rules", "list", "--category", "naming"]);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("filename_case") && s.contains("filename_regex"),
        "naming filter dropped a naming kind: {s}"
    );
    assert!(
        !s.contains("no_bidi_controls"),
        "naming filter leaked a non-naming kind"
    );

    let out = run(dir.path(), &["rules", "list", "--category", "nope"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown category"));

    let out = run(dir.path(), &["rules", "categories"]);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("security-unicode-sanity") && s.contains("Security / Unicode sanity"));
}

/// `alint list --category` filters THIS config's rules; an alias-kind rule maps
/// through the bridge; a category matching no loaded rule reports it distinctly.
#[test]
fn list_category_filters_config_rules() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(".alint.yml"),
        r#"version: 1
rules:
  - id: name_rule
    kind: filename_case
    paths: "**/*"
    case: snake
    level: warning
  - id: content_rule
    kind: content_matches
    paths: "**/*.md"
    pattern: "x"
    level: warning
"#,
    )
    .unwrap();

    let out = run(dir.path(), &["list", "--category", "naming"]);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("name_rule") && !s.contains("content_rule"),
        "naming: {s}"
    );

    // content_rule's kind is the alias `content_matches` -> resolves to Content.
    let out = run(dir.path(), &["list", "--category", "content"]);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("content_rule") && !s.contains("name_rule"),
        "content (alias-kind): {s}"
    );

    let out = run(dir.path(), &["list", "--category", "git-hygiene"]);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("no loaded rules are in category"),
        "empty-category message: {s}"
    );

    // The filtered JSON exposes kind + categories (parity with `rules list`).
    let out = run(
        dir.path(),
        &["list", "--category", "naming", "--format", "json"],
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    let rules = v["rules"].as_array().expect("rules array");
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["kind"], "filename_case");
    assert_eq!(rules[0]["categories"][0], "naming");
}

// ─── M13 — `init` / `lsp` fail loud on an unsupported `--format` ─────
//
// The "never silently ignore `--format`" contract (M13) is applied to every
// report-producing subcommand; `init` and `lsp` produce no formatted report and
// used to silently no-op the global flag. They must reject a non-default
// `--format` (exit 2) like their siblings, not run as if it were absent.
#[test]
fn init_and_lsp_reject_a_non_human_format() {
    let dir = tempfile::tempdir().unwrap();

    let out = run(dir.path(), &["init", ".", "--format", "sarif"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "`init --format sarif` must fail loud, not silently ignore the flag"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("does not support"),
        "the rejection should name the unsupported flag; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // `lsp` would otherwise block on stdio; the format check happens before it
    // starts serving, so it returns immediately.
    let out = Command::new(alint_bin())
        .args(["lsp", "--format", "json"])
        .current_dir(dir.path())
        .stdin(std::process::Stdio::null())
        .output()
        .expect("spawn alint");
    assert_eq!(
        out.status.code(),
        Some(2),
        "`lsp --format json` must fail loud before serving"
    );
}
