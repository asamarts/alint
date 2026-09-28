//! Fix-op spawn-gate drift audit (R-SPAWNGATE, part 1 of 2) — keep the fix-op
//! RCE allow-list honest.
//!
//! A *fixer* that shells out is the file-writing/RCE analogue of a spawning rule
//! kind, gated by [`alint_dsl::SPAWNING_FIX_OPS`] + `reject_spawning_fix_ops_in`.
//! The existing spawn gate keys on the rule KIND, so a spawning FIXER attached to
//! a non-spawning kind (`git_untrack` on `file_absent`) slips past it. The
//! rejection logic is unit-tested in `alint-dsl` and the CLI canary in
//! `alint::tests::fix_spawn_gate`, but — like the rule-level `gff` regression the
//! sibling `coverage_audit_spawn_gate` guards — none of those would catch a NEW
//! fixer that shells out yet was never added to the allow-list.
//!
//! This audit is the missing cross-check: it scans the FIXER sources for the
//! primitives that reach a subprocess and asserts the set of spawning fixer
//! modules is *exactly* the set the allow-list maps to — no more (an ungated
//! spawner), no less (a stale entry).

use std::path::{Path, PathBuf};

/// `crates/alint-rules/src/fixers`, resolved relative to this test crate.
fn fixers_src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ parent of alint-e2e")
        .join("alint-rules/src/fixers")
}

/// Does this source line reach a subprocess? A fixer spawns iff it constructs a
/// `Command` directly (`Command::new(` — also matches `StdCommand::new(` etc.),
/// calls the shared spawn chokepoint (`crate::spawn::<fn>(`), or touches
/// `alint_core::git` (whose readers and mutators all shell out to the `git`
/// binary — this is how `GitUntrackFixer` reaches `git rm --cached`). Line
/// comments are skipped so prose naming a primitive can't trip it.
///
/// Heuristic by design (a bespoke alias could evade it); it exists to catch the
/// realistic regression — a new fixer shelling out the ordinary way — not an
/// adversary editing alint's own source to hide a spawn from its own tests.
fn line_reaches_subprocess(line: &str) -> bool {
    let code = line.trim_start();
    if code.starts_with("//") {
        return false;
    }
    code.contains("Command::new(")
        || code.contains("crate::spawn::")
        || code.contains("alint_core::git::")
}

/// Every `.rs` under `fixers/` (recursively) that reaches a subprocess, as a
/// path relative to `fixers/` (e.g. `git_ops.rs`). Sorted.
fn spawning_fixer_modules() -> Vec<String> {
    let src = fixers_src_dir();
    let mut found = Vec::new();
    let mut stack = vec![src.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read fixers/ dir") {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read fixer source");
            if text.lines().any(line_reaches_subprocess) {
                let rel = path
                    .strip_prefix(&src)
                    .expect("under fixers/")
                    .to_string_lossy()
                    .replace('\\', "/");
                found.push(rel);
            }
        }
    }
    found.sort();
    found
}

/// The documented `fix op -> fixer module` map for the SPAWNING ops. Unlike rule
/// kinds (module named for the kind), a fix op's fixer module name is arbitrary,
/// so the mapping is explicit. Adding a spawning fix op forces a new row here (in
/// tandem with `SPAWNING_FIX_OPS`) — the tripwire that keeps the two in sync.
const SPAWNING_FIX_OP_MODULES: &[(&str, &str)] =
    &[("git_untrack", "git_ops.rs"), ("command", "command_ops.rs")];

#[test]
fn spawning_fix_op_allowlist_matches_the_fixers_that_actually_spawn() {
    use std::collections::BTreeSet;

    // (1) The mapping's op set must equal `SPAWNING_FIX_OPS` exactly — neither a
    // listed op without a module row, nor a stale row for a dropped op.
    let ssot: BTreeSet<&str> = alint_dsl::SPAWNING_FIX_OPS.iter().copied().collect();
    let mapped_ops: BTreeSet<&str> = SPAWNING_FIX_OP_MODULES.iter().map(|(op, _)| *op).collect();
    assert_eq!(
        mapped_ops, ssot,
        "\n\nFIX-OP SPAWN-GATE DRIFT — the op->module map here does not match \
         alint_dsl::SPAWNING_FIX_OPS.\n  mapped ops       : {mapped_ops:?}\n  SPAWNING_FIX_OPS : {ssot:?}\n\
         Add or drop a row in SPAWNING_FIX_OP_MODULES to match the allow-list."
    );

    // (2) The fixer modules that ACTUALLY reach a subprocess must be exactly the
    // modules the allow-list maps to. A new spawning fixer added without listing
    // its op (the RCE gap) makes `found` a superset and fails here.
    let found: BTreeSet<String> = spawning_fixer_modules().into_iter().collect();
    let expected: BTreeSet<String> = SPAWNING_FIX_OP_MODULES
        .iter()
        .map(|(_, m)| (*m).to_string())
        .collect();
    assert_eq!(
        found, expected,
        "\n\nFIX-OP SPAWN-GATE DRIFT — the fixer modules that launch a subprocess do \
         not match the SPAWNING_FIX_OPS allow-list.\n  modules that actually spawn : {found:?}\n\
         allow-listed modules        : {expected:?}\n\
         If you added a fixer that shells out, add its op to SPAWNING_FIX_OPS \
         (crates/alint-dsl/src/lib.rs) AND a row to SPAWNING_FIX_OP_MODULES here — \
         without it, an `extends:`'d or nested ruleset can make `alint fix` run \
         arbitrary code. If a spawning fixer moved modules, update the row."
    );
}

#[test]
fn spawning_fix_op_allowlist_is_non_empty_and_each_module_exists() {
    // A typo'd or emptied allow-list is itself a security regression (it would
    // silently stop gating). Pin that every mapped module is a real fixer source.
    assert!(
        !alint_dsl::SPAWNING_FIX_OPS.is_empty(),
        "SPAWNING_FIX_OPS is empty — the fix-op spawn trust gate would gate nothing"
    );
    let src = fixers_src_dir();
    for (op, module) in SPAWNING_FIX_OP_MODULES {
        let path = src.join(module);
        assert!(
            path.exists(),
            "SPAWNING_FIX_OP_MODULES maps {op:?} to {module:?}, but {} does not exist",
            path.display()
        );
    }
}

#[test]
fn subprocess_detection_is_precise() {
    // Guards the scan's own robustness: the real spawn reaches are caught, and
    // prose / a bare module reference is not a false positive.
    assert!(line_reaches_subprocess(
        "    use alint_core::git::{untrack_path, collect_tracked_paths};"
    ));
    assert!(line_reaches_subprocess(
        "    let mut cmd = Command::new(\"git\");"
    ));
    assert!(line_reaches_subprocess(
        "    crate::spawn::run_capturing(argv, cwd, &[], t)?;"
    ));
    // A line comment naming the primitive is ignored.
    assert!(!line_reaches_subprocess(
        "        // eventually calls Command::new( under the hood"
    ));
    // An unrelated fixer line is not a spawn.
    assert!(!line_reaches_subprocess(
        "        std::fs::set_permissions(&abs, perms)?;"
    ));
}
