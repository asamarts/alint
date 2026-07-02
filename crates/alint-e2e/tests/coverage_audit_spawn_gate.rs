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

/// Does this source line launch a subprocess? A rule spawns iff it either
/// constructs a `Command` directly (`Command::new(` — also matches
/// `StdCommand::new(`, `process::Command::new(`, `tokio::process::…`) or
/// calls the shared spawn chokepoint (`crate::spawn::run_capturing`).
/// Line comments are skipped so prose naming the primitive can't trip it.
///
/// Heuristic, deliberately: a spawn smuggled behind a bespoke alias
/// (`use std::process::Command as Cmd; Cmd::new()`) would evade it. That is
/// acceptable — the audit exists to catch the realistic regression (a new
/// rule shelling out the ordinary way, à la `gff`), not an adversary editing
/// alint's own source to hide a spawn from its own test suite.
fn line_spawns(line: &str) -> bool {
    let code = line.trim_start();
    if code.starts_with("//") {
        return false;
    }
    code.contains("Command::new(") || code.contains("crate::spawn::run_capturing")
}

/// Module file stems under `alint-rules/src` that reach a spawn primitive,
/// excluding the shared `spawn.rs` helper itself (it *is* the chokepoint,
/// not a rule kind). Sorted.
fn spawning_module_stems() -> Vec<String> {
    let mut stems = Vec::new();
    for entry in std::fs::read_dir(rules_src_dir()).expect("read alint-rules/src") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("utf-8 file stem")
            .to_string();
        if stem == "spawn" {
            continue; // the helper, not a rule
        }
        let text = std::fs::read_to_string(&path).expect("read rule source");
        if text.lines().any(line_spawns) {
            stems.push(stem);
        }
    }
    stems.sort();
    stems
}

#[test]
fn spawning_allowlist_matches_the_rules_that_actually_spawn() {
    let found = spawning_module_stems();

    // The allow-list as module stems. Each spawning kind lives in a module
    // named for the kind (`command` → command.rs, etc.); the audit relies on
    // that convention and calls it out if it ever breaks.
    let mut expected: Vec<String> = alint_dsl::SPAWNING_RULE_KINDS
        .iter()
        .map(|k| (*k).to_string())
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
         module is not named `<kind>.rs`, teach this audit the mapping."
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
