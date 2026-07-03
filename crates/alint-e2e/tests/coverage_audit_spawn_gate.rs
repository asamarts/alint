//! Spawn-gate drift audit — keep the RCE allow-list honest.
//!
//! alint refuses a process-spawning rule kind from any untrusted source
//! (`extends:`, nested config, template, `require:` sub-rule). The set of
//! spawning kinds is a hand-maintained allow-list,
//! [`alint_dsl::SPAWNING_RULE_KINDS`]. The rejection logic is thoroughly
//! tested elsewhere (`alint-dsl` unit tests; the CLI canary in
//! `alint::tests::spawn_gate`) — but every one of those tests names the
//! kinds it checks. None of them would catch the failure that actually
//! shipped once (`gff`): a rule that *spawns a process* but was never added
//! to the allow-list, so the gate waved it through and adopting a ruleset
//! became arbitrary code execution.
//!
//! This audit is the missing cross-check. It scans the rule sources for the
//! primitives that actually launch a subprocess and asserts that set of
//! modules is *exactly* the allow-list — no more (an ungated spawner), no
//! less (a stale allow-list entry). A new rule that shells out fails here
//! until its kind is gated.

use std::path::{Path, PathBuf};

/// `crates/alint-rules/src`, resolved relative to this test crate so the
/// audit is independent of the working directory.
fn rules_src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ parent of alint-e2e")
        .join("alint-rules/src")
}

/// The callable `fn` surface of the shared spawn chokepoint (`spawn.rs`),
/// extracted dynamically. A rule that calls any of these launches a process,
/// so reading them from the source (rather than hard-coding `run_capturing`)
/// means a *new* chokepoint entry point — `run_streaming`, `run_with_stdin`, …
/// — is covered the moment it lands, without editing this audit.
fn spawn_helper_fns(src: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(src.join("spawn.rs")).expect("read spawn.rs");
    let mut names = Vec::new();
    for line in text.lines() {
        let l = line.trim_start();
        // Only the callable-from-a-rule surface (`pub` / `pub(crate)`); a
        // private helper can't be reached cross-module anyway.
        for prefix in ["pub(crate) fn ", "pub fn "] {
            if let Some(rest) = l.strip_prefix(prefix)
                && let Some(name) = rest.split('(').next()
            {
                let name = name.trim();
                if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    names.push(name.to_string());
                }
                break;
            }
        }
    }
    assert!(
        !names.is_empty(),
        "no pub(crate)/pub fn found in spawn.rs — the chokepoint scan would be blind"
    );
    names
}

/// Does this source line launch a subprocess? A rule spawns iff it either
/// constructs a `Command` directly (`Command::new(` — also matches
/// `StdCommand::new(`, `process::Command::new(`, `tokio::process::…`) or calls
/// one of the shared chokepoint's `fn`s (`crate::spawn::<fn>(`). Line comments
/// are skipped so prose naming the primitive can't trip it.
///
/// Heuristic, deliberately: a spawn smuggled behind a bespoke alias
/// (`use std::process::Command as Cmd; Cmd::new()`) would evade it. That is
/// acceptable — the audit exists to catch the realistic regression (a new rule
/// shelling out the ordinary way, à la `gff`), not an adversary editing alint's
/// own source to hide a spawn from its own test suite.
fn line_spawns(line: &str, helpers: &[String]) -> bool {
    let code = line.trim_start();
    if code.starts_with("//") {
        return false;
    }
    if code.contains("Command::new(") {
        return true;
    }
    helpers
        .iter()
        .any(|h| code.contains(&format!("crate::spawn::{h}(")))
}

/// Every `.rs` under `alint-rules/src` — **recursively**, so a rule split into
/// a `foo/mod.rs` + submodules can't hide a spawn from the flat top level —
/// that reaches a spawn primitive, as a path relative to `src/` (e.g.
/// `command.rs`, `foo/mod.rs`). Excludes the shared `spawn.rs` helper itself
/// (it *is* the chokepoint, not a rule kind). Sorted.
fn spawning_module_paths() -> Vec<String> {
    let src = rules_src_dir();
    let helpers = spawn_helper_fns(&src);
    let spawn_helper = src.join("spawn.rs");
    let mut found = Vec::new();
    let mut stack = vec![src.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read alint-rules/src dir") {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") || path == spawn_helper {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read rule source");
            if text.lines().any(|l| line_spawns(l, &helpers)) {
                let rel = path
                    .strip_prefix(&src)
                    .expect("under src")
                    .to_string_lossy()
                    .replace('\\', "/");
                found.push(rel);
            }
        }
    }
    found.sort();
    found
}

#[test]
fn spawning_allowlist_matches_the_rules_that_actually_spawn() {
    let found = spawning_module_paths();

    // The allow-list as `src`-relative module paths. Each spawning kind lives in
    // a top-level module named for the kind (`command` → command.rs, etc.); the
    // audit relies on that convention and calls it out if it ever breaks.
    let mut expected: Vec<String> = alint_dsl::SPAWNING_RULE_KINDS
        .iter()
        .map(|k| format!("{k}.rs"))
        .collect();
    expected.sort();

    assert_eq!(
        found, expected,
        "\n\nSPAWN-GATE DRIFT — the rule modules that launch a subprocess do not \
         match alint_dsl::SPAWNING_RULE_KINDS.\n\
         \n  modules that actually spawn : {found:?}\
         \n  SPAWNING_RULE_KINDS (as .rs) : {expected:?}\n\
         \nIf you added a rule that shells out, add its kind to SPAWNING_RULE_KINDS \
         (crates/alint-dsl/src/lib.rs) — without it, an `extends:`'d or nested \
         ruleset can run the rule as arbitrary code (the `gff` regression class). \
         If you removed a spawner, drop its stale allow-list entry. If a spawning \
         module is not named `<kind>.rs` at the top level, teach this audit the \
         mapping."
    );
}

#[test]
fn spawning_allowlist_is_non_empty_and_each_entry_has_a_module() {
    // Cheap companion: a typo'd or emptied allow-list is itself a security
    // regression (it would silently stop gating). Pin that every entry names
    // a real rule module.
    assert!(
        !alint_dsl::SPAWNING_RULE_KINDS.is_empty(),
        "SPAWNING_RULE_KINDS is empty — the process-spawn trust gate would gate nothing"
    );
    let src = rules_src_dir();
    for kind in alint_dsl::SPAWNING_RULE_KINDS {
        let module = src.join(format!("{kind}.rs"));
        assert!(
            module.exists(),
            "SPAWNING_RULE_KINDS names {kind:?} but {} does not exist — stale or \
             mistyped allow-list entry",
            module.display()
        );
    }
}

#[test]
fn spawn_detection_is_dynamic_and_precise() {
    // Guards the scan's own robustness (adversarial review of #112):
    // the chokepoint fn set is read from spawn.rs (not hard-coded), so a future
    // entry point is covered; and a bare timeout-const reference is not a spawn.
    let helpers = spawn_helper_fns(&rules_src_dir());
    assert!(
        helpers.iter().any(|h| h == "run_capturing"),
        "spawn.rs's real fn surface should be discovered, got {helpers:?}"
    );

    // A hypothetical future chokepoint fn is detected the moment it exists —
    // no edit to this audit needed.
    let future = ["run_capturing".to_string(), "run_streaming".to_string()];
    assert!(
        line_spawns("        crate::spawn::run_streaming(argv)?;", &future),
        "a new chokepoint fn must be caught once spawn.rs exports it"
    );

    // Direct construction is always caught (alias `StdCommand` included).
    assert!(line_spawns(
        "    let mut cmd = StdCommand::new(program);",
        &helpers
    ));
    // A reference to the timeout *const* (not a fn call) is NOT a spawn —
    // guards against a false RED that a bare `crate::spawn::` prefix would cause.
    assert!(!line_spawns(
        "            .unwrap_or(crate::spawn::DEFAULT_SPAWN_TIMEOUT_SECS),",
        &helpers
    ));
    // Prose mentioning the primitive is ignored.
    assert!(!line_spawns(
        "        // eventually calls Command::new( for you",
        &helpers
    ));
}
