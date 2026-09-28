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
    BinaryBenchmarkConfig, Callgrind, Command, EventKind, binary_benchmark, binary_benchmark_group,
    main,
};

/// 0xA11E47 — the canonical bench seed (byte-identical trees across runs).
const SEED: u64 = 10_559_047;

// The real scenario configs, shared with the wall-clock bench (minimal drift —
// one source of truth). A spread of dispatch classes that exercise the regular
// gen-monorepo tree: S1 = filename-only (isolates the walker); S2 = existence +
// content; S6 = dense per-file content; S7 = cross-file relational; S12 = the
// v0.10 per-file dispatch class.
const S1: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../xtask/src/bench/scenarios/s1_filename.yml"
));
const S2: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../xtask/src/bench/scenarios/s2_existence_content.yml"
));
const S6: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../xtask/src/bench/scenarios/s6_per_file_content.yml"
));
const S7: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../xtask/src/bench/scenarios/s7_cross_file_relational.yml"
));
const S12: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../xtask/src/bench/scenarios/s12_v010_per_file.yml"
));
// A fix scenario: one content fixer (trim trailing whitespace). The
// `materialize_fixable` setup makes every `.rs` line a violation, so
// `fix --dry-run` re-reads every source file through the fixer's collect step.
const SFIX_TRIM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../xtask/src/bench/scenarios/sfix_trim.yml"
));

fn workspace_target() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target")
}

fn alint_bin() -> PathBuf {
    workspace_target().join("release/alint")
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
#[bench::s6_1k("s6", S6, 1_000)]
#[bench::s6_10k("s6", S6, 10_000)]
#[cfg_attr(feature = "det-100k", bench::s6_100k("s6", S6, 100_000))]
#[bench::s7_1k("s7", S7, 1_000)]
#[bench::s7_10k("s7", S7, 10_000)]
#[cfg_attr(feature = "det-100k", bench::s7_100k("s7", S7, 100_000))]
#[bench::s12_1k("s12", S12, 1_000)]
#[bench::s12_10k("s12", S12, 10_000)]
#[cfg_attr(feature = "det-100k", bench::s12_100k("s12", S12, 100_000))]
fn check(scenario: &str, config: &str, n: usize) -> Command {
    let _ = config; // consumed by `materialize` (setup); not needed to build the command
    Command::new(alint_bin())
        .arg("check")
        .arg(tree_path(scenario, n))
        .build()
}

binary_benchmark_group!(name = check_grp, benchmarks = check);

// `alint fix --dry-run` end to end over a fixable tree. Pairs with the
// wall-clock `fix_throughput.rs` (which the deterministic Ir signal here cannot
// replace: callgrind is I/O-blind, so a collect read-path regression that adds
// syscalls but not instructions stays flat here). One scenario at the per-PR
// sizes; 100k is release-gated like the check cells.
#[binary_benchmark(setup = materialize_fixable)]
#[bench::fix_trim_1k("sfix-trim", SFIX_TRIM, 1_000)]
#[bench::fix_trim_10k("sfix-trim", SFIX_TRIM, 10_000)]
#[cfg_attr(
    feature = "det-100k",
    bench::fix_trim_100k("sfix-trim", SFIX_TRIM, 100_000)
)]
fn fix(scenario: &str, config: &str, n: usize) -> Command {
    let _ = config; // consumed by `materialize_fixable` (setup)
    Command::new(alint_bin())
        .arg("fix")
        .arg("--dry-run")
        .arg(tree_path(scenario, n))
        .build()
}

binary_benchmark_group!(name = fix_grp, benchmarks = fix);

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
    binary_benchmark_groups = [check_grp, fix_grp]
);
