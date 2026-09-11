//! Rung-8 fix-coverage gate (auto-fix plan section 2). Every fix op in
//! [`FixSpec::ALL_OP_NAMES`] must be, across the inline `fix/**/*.yml` corpus:
//!
//! 1. **exercised** - named in an `applied:` / `suggested:` list of a `fix` or
//!    `fix_unsafe` step whose rule declares that op inline; and
//! 2. **proven convergent** - covered by a scenario whose second pass is a
//!    no-op, accepted in either shape: `[.., fix, check]` ending in
//!    `violations: []` (15 of the 17 shipped scenarios), or `[.., fix, fix]`
//!    whose trailing fix reports `applied: []` with no residual `skipped:`.
//!
//! Ops are resolved from **inline** rules only: an `applied:` id supplied by an
//! `extends:`'d ruleset is skipped, so an external scenario can't red-herring
//! the gate. The Phase-0 back-fill was empty (all 12 shipped ops already have
//! both), so this gate exists to stop future drift: a new op that ships without
//! a scenario, or without an idempotence proof, reds here.
//!
//! Mechanism mirrors `coverage_audit_pass_fail.rs`: parse each scenario's
//! `given.config` YAML string, map every rule `id` to its single `fix:` op key
//! (R-TWOOP guarantees one), then walk `when`/`expect` in lockstep.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use alint_core::FixSpec;
use serde_yaml_ng::Value;

/// Ops exempt from the convergence requirement. A user-supplied `command`-fix
/// (Phase 3) runs an arbitrary command, so idempotence is the command author's
/// business, not the harness's. Empty until that op ships; the tripwire test
/// `convergence_exempt_stays_empty` fails if a future entry is added silently.
const CONVERGENCE_EXEMPT: &[&str] = &[];

/// Every `*.yml` under `dir`, recursively.
fn yml_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|s| s.to_str()) == Some("yml") {
                out.push(path);
            }
        }
    }
    out
}

/// Map every rule `id` in a parsed `given.config` to its single `fix:` op key.
/// Walks nested blocks too (a `for_each_dir` wrapping a fixable rule).
fn collect_id_to_op(v: &Value, out: &mut HashMap<String, String>) {
    match v {
        Value::Mapping(m) => {
            let id = m.get(Value::String("id".into())).and_then(Value::as_str);
            let fix = m
                .get(Value::String("fix".into()))
                .and_then(Value::as_mapping);
            if let (Some(id), Some(fix)) = (id, fix)
                && let Some((key, _)) = fix.iter().next()
                && let Some(op) = key.as_str()
            {
                out.insert(id.to_string(), op.to_string());
            }
            for (_, child) in m {
                collect_id_to_op(child, out);
            }
        }
        Value::Sequence(seq) => {
            for child in seq {
                collect_id_to_op(child, out);
            }
        }
        _ => {}
    }
}

/// The string entries of a sequence-valued expect field (`applied`, `skipped`,
/// `suggested`); empty when the field is absent or not a sequence.
fn str_list(exp: &Value, field: &str) -> Vec<String> {
    exp.get(field)
        .and_then(Value::as_sequence)
        .map(|s| {
            s.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// True when an expect step carries `violations:` present and empty (a clean
/// check). Absent `violations:` is not a clean check.
fn violations_empty(exp: &Value) -> bool {
    exp.get("violations")
        .and_then(Value::as_sequence)
        .is_some_and(std::vec::Vec::is_empty)
}

/// Insert the op key of every resolvable inline id into `set`.
fn resolve_into(ids: &[String], id_to_op: &HashMap<String, String>, set: &mut BTreeSet<String>) {
    for id in ids {
        if let Some(op) = id_to_op.get(id) {
            set.insert(op.clone());
        }
    }
}

fn is_fix_step(step: &str) -> bool {
    step == "fix" || step == "fix_unsafe"
}

#[test]
fn convergence_exempt_stays_empty() {
    assert!(
        CONVERGENCE_EXEMPT.is_empty(),
        "the convergence-exempt list should stay empty until a `command`-fix op \
         ships (Phase 3); a new entry means an op is skipping its idempotence \
         proof - confirm that is really the user's-command exemption first"
    );
}

// One linear pass: scan every scenario, resolve applied ids to ops, and
// classify coverage/convergence, then emit a structured report. Splitting the
// scan from the reporting would obscure the single accumulation loop.
#[allow(clippy::too_many_lines)]
#[test]
fn every_fix_op_is_exercised_and_proven_convergent() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scenarios")
        .join("fix");

    let mut covered: BTreeSet<String> = BTreeSet::new();
    let mut converged: BTreeSet<String> = BTreeSet::new();

    for path in yml_files(&dir) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(scn) = serde_yaml_ng::from_str::<Value>(&text) else {
            continue;
        };
        let Some(config) = scn
            .get("given")
            .and_then(|g| g.get("config"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let Ok(cfg) = serde_yaml_ng::from_str::<Value>(config) else {
            continue;
        };
        let mut id_to_op = HashMap::new();
        collect_id_to_op(&cfg, &mut id_to_op);
        if id_to_op.is_empty() {
            continue;
        }

        let when: Vec<String> = scn
            .get("when")
            .and_then(Value::as_sequence)
            .map(|s| {
                s.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let expect: Vec<Value> = scn
            .get("expect")
            .and_then(Value::as_sequence)
            .cloned()
            .unwrap_or_default();

        for (i, step) in when.iter().enumerate() {
            if !is_fix_step(step) {
                continue;
            }
            let Some(exp) = expect.get(i) else {
                continue;
            };
            // Exercised: every applied/suggested id this fix step names.
            let mut fired = str_list(exp, "applied");
            fired.extend(str_list(exp, "suggested"));
            resolve_into(&fired, &id_to_op, &mut covered);

            // Convergence: this fix step's applied ops, when the *next* step
            // shows the pass reached a fixpoint.
            let (Some(next_step), Some(next_exp)) = (when.get(i + 1), expect.get(i + 1)) else {
                continue;
            };
            let applied = str_list(exp, "applied");
            // Case B: a clean check right after the fix.
            let clean_check = next_step == "check" && violations_empty(next_exp);
            // Case A: a trailing fix that changed nothing (and skipped nothing).
            let noop_refix = is_fix_step(next_step)
                && str_list(next_exp, "applied").is_empty()
                && str_list(next_exp, "skipped").is_empty();
            if clean_check || noop_refix {
                resolve_into(&applied, &id_to_op, &mut converged);
            }
        }
    }

    let all: Vec<&str> = FixSpec::ALL_OP_NAMES.to_vec();
    let exempt: BTreeSet<&str> = CONVERGENCE_EXEMPT.iter().copied().collect();

    let missing_covered: Vec<&str> = all
        .iter()
        .copied()
        .filter(|op| !covered.contains(*op))
        .collect();
    let missing_converged: Vec<&str> = all
        .iter()
        .copied()
        .filter(|op| !exempt.contains(*op) && !converged.contains(*op))
        .collect();

    if missing_covered.is_empty() && missing_converged.is_empty() {
        return;
    }

    let mut report = String::new();
    if !missing_covered.is_empty() {
        let _ = writeln!(
            report,
            "{} of {} fix ops have NO inline scenario exercising them:",
            missing_covered.len(),
            all.len(),
        );
        for op in &missing_covered {
            let _ = writeln!(report, "  - {op}");
        }
        let _ = writeln!(
            report,
            "  (add scenarios/fix/<name>.yml whose rule declares `fix: {{ <op>: ... }}` \
             and whose fix step lists the rule id under `applied:`)",
        );
    }
    if !missing_converged.is_empty() {
        let _ = writeln!(
            report,
            "{} of {} fix ops have NO convergence proof:",
            missing_converged.len(),
            all.len(),
        );
        for op in &missing_converged {
            let _ = writeln!(report, "  - {op}");
        }
        let _ = writeln!(
            report,
            "  (add a scenario with `when: [.., fix, check]` ending in \
             `violations: []`, or `when: [.., fix, fix]` whose second fix is \
             `applied: []` with no `skipped:`)",
        );
    }
    panic!("\n{report}");
}
