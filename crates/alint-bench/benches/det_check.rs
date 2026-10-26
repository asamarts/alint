//! Deterministic (Callgrind) BINARY benchmark — end-to-end `alint check` over
//! fixed `gen-monorepo` trees, measuring the REAL release `alint` binary.
//!
//! Unlike the in-process `det_engine` library bench, this runs the actual CLI
//! as a separate process under Valgrind — no toggle/inlining concerns — so it
//! catches walker / IO / dispatch regressions end to end (e.g. the walker
//! `filter_entry` indirect-mispredict class from
//! `docs/benchmarks/investigations/2026-06-v0.12-perf-validation/`). This is the
//! load-bearing regression gate.
//!
//! Build the release binary first, then run:
//!
//! ```sh
//! cargo build --release -p alint
//! cargo bench -p alint-bench --bench det_check
//! ```
//!
//! Gate: `Ir` (+2%) and `EstimatedCycles` (+5%) vs baseline. Branch mispredicts
//! (`Bcm`/`Bim`) are DIAGNOSTIC-ONLY — collected + printed via `--branch-sim`,
//! but not gated (they false-positive on benign branch-pattern shifts). Design:
//! `docs/design/deterministic-perf-gating.md`.

use std::path::{Path, PathBuf};

use gungraun::{
    BinaryBenchmarkConfig, Callgrind, Command, EventKind, ExitWith, binary_benchmark,
    binary_benchmark_group, main,
};

/// 0xA11E47 — the canonical bench seed (byte-identical trees across runs).
const SEED: u64 = 10_559_047;

// The real scenario configs, shared with the wall-clock bench (minimal drift —
// one source of truth). A spread of dispatch classes over the regular
// gen-monorepo tree: s1 = layout/walk-bound (isolates the walker); s2 = per-file
// content; s3 = cross-file relational + graph; relation_coalescing = relation-layer
// read coalescing (`key_parity` + a per-file rule over the same TOML manifests).
// (s4_workspace is deliberately excluded: its `extends:` pulls the bundled
// rulesets shipped in the binary, whose rule set evolves per release — a moving
// target unfit for a fixed deterministic Ir gate. The relation scenario has no
// v0.18.2 counterpart, so it lives in its own group and is budgeted separately;
// see `docs/benchmarks/REFERENCE-v0.18.2.md`.)
const S1: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../xtask/src/bench/scenarios/s1_layout.yml"
));
const S2: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../xtask/src/bench/scenarios/s2_content.yml"
));
const S3: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../xtask/src/bench/scenarios/s3_relational.yml"
));
const RELATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../xtask/src/bench/scenarios/relation_coalescing.yml"
));
// The consolidated auto-fix scenario. `materialize_fixable` makes every `.rs`
// line a trailing-whitespace violation, so the `**/*.rs`-scoped content fixers
// re-read every source file through the fixer's collect step (the read-heavy
// path). The `sfix/`-scoped ops (structured / cross-file / path) no-op here —
// this tree has no `sfix/` subtree — which is fine: the collect read cost is
// what this deterministic gate tracks; full 24-op coverage is the wall-clock
// `bench-scale` SFIX scenario's job.
const SFIX_ALL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../xtask/src/bench/scenarios/sfix_all.yml"
));

const ANALYSIS_TIME: &str = r#"
version: 1
rules:
  - id: stale-todos
    kind: git_blame_age
    paths: "**/*.rs"
    pattern: TODO
    max_age_days: 30
    level: warning
"#;

fn workspace_target() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target")
}

/// The binary under measurement: `ALINT_DET_BIN` when set (the reference perf
/// gate measures another release this way without touching `target/`), else
/// the workspace's release build.
fn alint_bin() -> PathBuf {
    std::env::var_os("ALINT_DET_BIN")
        .map_or_else(|| workspace_target().join("release/alint"), PathBuf::from)
}

/// `(packages, files_per_package)` for a size, matching the bench tree shapes.
fn shape(n: usize) -> (usize, usize) {
    match n {
        10_000 => (200, 48),
        100_000 => (1_000, 98),
        _ => (50, 18), // 1k
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dst = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dst);
        } else {
            std::fs::copy(entry.path(), &dst).unwrap();
        }
    }
}

/// The deterministic fixed path for a `(scenario, n)` tree. A fixed path (not
/// the gen'd `TempDir`) keeps the absolute paths — and thus `Ir` — byte-stable
/// run to run. Shared by `materialize` (setup) and `check` (the bench fn), which
/// both receive the same `#[bench::…]` args.
fn tree_path(scenario: &str, n: usize) -> PathBuf {
    workspace_target()
        .join("det-trees")
        .join(format!("{scenario}-{n}"))
}

/// setup: materialize the fixed tree + drop the scenario config in as
/// `.alint.yml`. A pure side effect — the bench fn recomputes the same path.
fn materialize(scenario: &str, config: &str, n: usize) {
    let (packages, fpp) = shape(n);
    let tree = alint_bench::tree::generate_monorepo(packages, fpp, SEED).unwrap();
    let dest = tree_path(scenario, n);
    let _ = std::fs::remove_dir_all(&dest);
    copy_dir(tree.root(), &dest);
    std::fs::write(dest.join(".alint.yml"), config).unwrap();
}

fn analysis_time_tree_path() -> PathBuf {
    workspace_target().join("det-trees/analysis-time-cli")
}

fn materialize_analysis_time() {
    let root = analysis_time_tree_path();
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(".alint.yml"), ANALYSIS_TIME).unwrap();
    let run_git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .env("GIT_AUTHOR_DATE", "2024-01-02T03:04:05Z")
            .env("GIT_COMMITTER_DATE", "2024-01-02T03:04:05Z")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run_git(&["init", "-q", "-b", "main"]);
    run_git(&["config", "user.name", "Test"]);
    run_git(&["config", "user.email", "test@example.com"]);
    run_git(&["config", "commit.gpgsign", "false"]);
    run_git(&["add", ".alint.yml"]);
    run_git(&["commit", "-qm", "fixed"]);
}

/// setup for the `fix` cell: like [`materialize`], then inject a trailing space
/// before every newline in every `.rs` file, so every source line is a
/// `no_trailing_whitespace` violation and `fix --dry-run` re-reads every file
/// through the fixer's collect step (the read-heavy path). The transform is
/// deterministic over the fixed-seed tree, so the fixable tree is byte-stable
/// run to run and `Ir` stays comparable. Setup is not part of the callgrind
/// measurement, so the rewrite is free.
fn materialize_fixable(scenario: &str, config: &str, n: usize) {
    materialize(scenario, config, n);
    inject_trailing_ws(&tree_path(scenario, n));
}

fn inject_trailing_ws(dir: &Path) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            inject_trailing_ws(&path);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let mut out = Vec::with_capacity(bytes.len() + bytes.len() / 32 + 1);
            for &b in &bytes {
                if b == b'\n' {
                    out.push(b' '); // trailing space -> a violation on that line
                }
                out.push(b);
            }
            let _ = std::fs::write(&path, out);
        }
    }
}

// Per-PR gate runs 1k + 10k (seconds under valgrind). The 100k tier is heavier
// (~100s/cell) and gated behind the `det-100k` feature for release-time runs
// (`cargo bench -p alint-bench --bench det_check --features det-100k`).
#[binary_benchmark(setup = materialize)]
#[bench::s1_1k("s1", S1, 1_000)]
#[bench::s1_10k("s1", S1, 10_000)]
#[cfg_attr(feature = "det-100k", bench::s1_100k("s1", S1, 100_000))]
#[bench::s2_1k("s2", S2, 1_000)]
#[bench::s2_10k("s2", S2, 10_000)]
#[cfg_attr(feature = "det-100k", bench::s2_100k("s2", S2, 100_000))]
#[bench::s3_1k("s3", S3, 1_000)]
#[bench::s3_10k("s3", S3, 10_000)]
#[cfg_attr(feature = "det-100k", bench::s3_100k("s3", S3, 100_000))]
fn check(scenario: &str, config: &str, n: usize) -> Command {
    let _ = config; // consumed by `materialize` (setup); not needed to build the command
    Command::new(alint_bin())
        .arg("check")
        .arg(tree_path(scenario, n))
        .build()
}

binary_benchmark_group!(name = check_grp, benchmarks = check);

// v0.21 R3 relation scenario. It is separate from check_grp because it has no
// v0.18.2 counterpart: the absolute reference gate filters check_grp only.
#[binary_benchmark(setup = materialize)]
#[bench::relation_1k("relation", RELATION, 1_000)]
#[bench::relation_10k("relation", RELATION, 10_000)]
#[cfg_attr(
    feature = "det-100k",
    bench::relation_100k("relation", RELATION, 100_000)
)]
fn relation(scenario: &str, config: &str, n: usize) -> Command {
    let _ = config; // consumed by `materialize` (setup)
    Command::new(alint_bin())
        .arg("check")
        .arg(tree_path(scenario, n))
        .build()
}

binary_benchmark_group!(name = relation_grp, benchmarks = relation);

// `alint fix --dry-run` end to end over a fixable tree. Pairs with the
// wall-clock `fix_throughput.rs` (which the deterministic Ir signal here cannot
// replace: callgrind is I/O-blind, so a collect read-path regression that adds
// syscalls but not instructions stays flat here). One scenario at the per-PR
// sizes; 100k is release-gated like the check cells.
#[binary_benchmark(setup = materialize_fixable)]
#[bench::fix_all_1k("sfix-all", SFIX_ALL, 1_000)]
#[bench::fix_all_10k("sfix-all", SFIX_ALL, 10_000)]
#[cfg_attr(
    feature = "det-100k",
    bench::fix_all_100k("sfix-all", SFIX_ALL, 100_000)
)]
fn fix(scenario: &str, config: &str, n: usize) -> Command {
    let _ = config; // consumed by `materialize_fixable` (setup)
    // Mirror the wall-clock `Mode::Fix` invocation: `--unsafe-fixes` so every
    // op applies through compose, `--dry-run` so nothing is written.
    Command::new(alint_bin())
        .arg("fix")
        .arg("--unsafe-fixes")
        .arg("--dry-run")
        .arg(tree_path(scenario, n))
        // `sfix_all.yml`'s `sfix/`-scoped error-level rules (create / sync_from /
        // create_and_register) cannot be satisfied on this plain tree (no `sfix/`
        // overlay), so `fix --dry-run` DETERMINISTICALLY exits 1. gungraun expects
        // exit 0 by default, which failed this cell and aborted the whole det_check
        // run (the gate then logged "tooling failure, NOT gating" on every PR).
        // Pin the exact code so a crash / exit-2 oscillation still fails loudly.
        .exit_with(ExitWith::Code(1))
        .build()
}

binary_benchmark_group!(name = fix_grp, benchmarks = fix);

// One fixed repository and one time-capable configured rule. `explain` executes
// exactly the HEAD-time fallback command without also invoking `git blame`, so
// Callgrind's child-process tracing includes the deterministic process cost.
#[binary_benchmark(setup = materialize_analysis_time)]
#[bench::head_commit()]
fn explain_analysis_time() -> Command {
    Command::new(alint_bin())
        .current_dir(analysis_time_tree_path())
        .arg("explain")
        .arg("stale-todos")
        .arg("--config")
        .arg(analysis_time_tree_path().join(".alint.yml"))
        .arg("--format")
        .arg("json")
        .build()
}

binary_benchmark_group!(name = analysis_time_grp, benchmarks = explain_analysis_time);

main!(
    config = BinaryBenchmarkConfig::default().tool(
        Callgrind::default()
            .args(["--cache-sim=yes", "--branch-sim=yes"])
            // Gate on Ir (work, +2%) and EstimatedCycles (net work + cache +
            // branch penalties, +5%). Branch mispredicts (Bcm/Bim) are
            // diagnostic-only — collected + printed, NOT gated: they swing wildly
            // (+73..217% for v0.12's benign walker symlink-security closure — see
            // docs/benchmarks/investigations/2026-06-v0.12-perf-validation/) while
            // moving net cycles <1%, so gating them only false-positives.
            .soft_limits([(EventKind::Ir, 2.0), (EventKind::EstimatedCycles, 5.0)]),
    ),
    binary_benchmark_groups = [check_grp, relation_grp, fix_grp, analysis_time_grp]
);
