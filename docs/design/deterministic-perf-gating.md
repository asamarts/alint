# Deterministic performance gating

**Status:** IN PROGRESS (design approved 2026-06-07)

## Motivation

`docs/benchmarks/investigations/2026-06-v0.12-perf-validation/` proved that
deterministic profiling (instruction / cache / branch counts via Valgrind) is
**load-immune**: it gave an airtight regression verdict in ~30 min on a *busy*
shared box, where the wall-clock `bench-scale` needed a 5-hour quiet window and
*still* got contaminated by co-tenants (v0.11.1 AND v0.12.0). Wall-clock benches
on the shared kbox are chronically unreliable as a regression gate.

**Goal:** make deterministic counts the **primary, automated, per-PR regression
gate**, decoupled from the contaminated self-hosted runner, with minimal drift.
Demote wall-clock `bench-scale` to absolute-throughput *characterization*.

## Decision: adopt `gungraun` (formerly `iai-callgrind`)

`gungraun` (v0.19.x, the renamed `iai-callgrind`) wraps Callgrind + Cachegrind +
DHAT to produce deterministic metrics, is explicitly designed for noisy/CI
environments ("comparable between different systems, negating environment
noise"), runs each bench once (fast), and has built-in regression gates
(`--callgrind-limits` / `--cachegrind-limits` fail on a per-event breach;
`--save-baseline` / `--baseline` for comparison). Hand-rolling callgrind parsing
+ baseline + gate would reinvent it and add drift surface.

(Decisions locked 2026-06-07: (1) adopt gungraun; (2) widen scenario coverage +
add 100k at release time; (3) branch mispredicts ADVISORY with gating only at
very-high deltas — RECALIBRATED 2026-06-08 to diagnostic-only + an `EstimatedCycles`
+5% gate, after the +50% ceiling false-positived on benign v0.12 walker drift
(investigation Phase 1c).)

## Architecture — two deterministic layers

**Layer 1 — function-level (micro), gungraun library benches**
(`crates/alint-bench/benches/det_engine.rs`):
- `Engine::run` over a fixed in-memory `FileIndex` (dispatch + aggregation; H2 class)
- the walker over a fixed tree (H1 — the +788k indirect mispredicts found in the
  investigation)
- `Scope::matches` / the per-file `evaluate_file` dispatch

**Layer 2 — end-to-end (macro), gungraun binary benches**
(`crates/alint-bench/benches/det_check.rs`):
- `alint check <tree>` under Callgrind + Cachegrind, setup fn materializes the
  fixed `gen-monorepo` tree (reuse `crates/alint-bench/src/tree.rs`, seed `0xA11E47`)
- **Scenario coverage (widened per decision 2):** a broad subset of S1–S14 —
  walk (S1), per-file content (S2, S5, S6), per-file v0.10/v0.12 kinds (S12, S14),
  cross-file (S7, S11), git (S8), polyglot/scope_filter (S9, S10). Per-PR gate runs
  **1k + 10k**; the **100k tier is added at release time** (still load-immune, just
  slower under valgrind).

## Gating policy (mirrors `gate.rs` gating-vs-advisory split)

| Metric (source) | Role |
|---|---|
| **Instruction count `Ir`** (callgrind) | **GATING**, tight (+2%/bench) — the real-work signal; proved the +300% external (+0.08%). |
| **`EstimatedCycles`** (callgrind: work + cache + branch penalties) | **GATING**, +5% — the net-effect signal; catches a real cycle regression that flat `Ir` would miss, while absorbing benign branch noise (S12's +217% `Bim` is <1% here). |
| **Branch mispredicts** (cachegrind `Bcm`+`Bim`) | **DIAGNOSTIC-ONLY** (recalibrated 2026-06-08, investigation Phase 1c): collected + printed, NOT gated. The original +50% ceiling (decision 3) still false-positived — v0.12's benign walker symlink-security closure moved `Bim` +73–217% at <1% net cycles. `EstimatedCycles` is the gate that catches a *real* branch blowup instead. |
| **D1 / LL cache misses** (cachegrind) | DIAGNOSTIC-ONLY |
| **Syscalls** (strace, supplementary) | optional check — new per-file syscall (H1 `lstat`) |

## Automation — load-immune per-PR CI gate

`ci.yml` job `perf-gate` (gated on `changes.outputs.rust`, PRs only), on the
**self-hosted runner** (`[self-hosted, linux, alint]`): the gate is itself
load-immune, so co-tenant noise doesn't matter and no GitHub-hosted quiescence is
needed. Install pinned valgrind → build + bench the **merge-base** and the PR head
(`cargo bench --bench det_engine --bench det_check`) → gungraun compares the two
**in-CI** (no committed baseline; the raw output is ~18 MB) → on an `Ir` /
`EstimatedCycles` breach it emits a `::warning` and **exits 0** — advisory while
`DET_PERF_ADVISORY=1`. Flip `DET_PERF_ADVISORY=0` to make a breach fail the PR
once the `Ir` limit is calibrated against real PR noise. Each bench runs once ⇒
fast; regressions surface at PR time, deterministically — not 5 h later in a
contaminated tag bench.

## Drift control

`Ir` is byte-stable for a fixed binary + inputs. Every source pinned; baseline
regeneration is an explicit, documented trigger:

| Source | Pin | Regen trigger |
|---|---|---|
| rustc / LLVM | `rust-toolchain.toml` + existing `codegen-units = 1` (deterministic codegen order) | toolchain bump |
| valgrind (cache/branch model) | `ARG VALGRIND_VERSION` in `bench/Dockerfile` + pinned install in the CI job | valgrind bump |
| dependencies | committed `Cargo.lock` | Ir-changing dep bump |
| gungraun + its runner | pinned dev-dep + `cargo install gungraun-runner --version =X` | gungraun bump |
| input tree | seed `0xA11E47` | frozen |

There is **no committed baseline**: the gate builds and benches the PR's
merge-base in the same CI run and compares against it (same-env, zero drift —
chosen because the raw gungraun output is ~18 MB). A toolchain/valgrind bump
therefore moves both sides together rather than silently invalidating a stored
baseline. The bench Docker image still gains pinned valgrind for reproducibility
anywhere.

## Relationship to wall-clock bench

`bench-scale` / `bench-gate` stay for absolute-throughput + cross-tool numbers, but
are **demoted from the regression gate** (contamination-prone). `RELEASING.md` +
the bench-record review updated: deterministic gate = primary regression signal;
wall-clock = characterization, trusted only on a verified-quiet box. Separate track:
a dedicated / cpuset-pinned bench runner so wall-clock characterization is
trustworthy too.

## Phased rollout

1. **Adopt + prototype** — gungraun dev-dep (pinned) + runner; port S1/S6/S12 binary
   benches + 2–3 library benches; first baseline; pin valgrind in the Docker image.
2. **CI advisory** — `perf-gate` runs + reports on PRs but does not fail (~1 week) to
   calibrate the `Ir` limit against real PR noise.
3. **Flip `Ir` to gating** — once calibrated; branch/cache stay advisory (+ the
   very-high branch gate).
4. **Widen scenarios + 100k-at-release**; document + demote wall-clock; optional
   strace syscall check.
5. **Runner-isolation fix** (separate track) — dedicated/cpuset-pinned bench runner.

## Findings / progress

- **Phase 1a — library bench DONE + pushed (`4fa13774`).** `det_engine` measures
  `Engine::run` over a fixed in-memory `FileIndex` (1k/10k) under Callgrind w/
  cache+branch sim; `Ir` gated +2% (branch `Bcm`/`Bim` shipped at a +50% ceiling —
  later recalibrated to diagnostic-only + an `EstimatedCycles` +5% gate, see above).
  **Gotcha found + fixed:** the workspace `[profile.release] strip = true` zeroes
  all counts (gungraun's `--toggle-collect=<bench-fn>` matches no symbol) →
  added `[profile.bench]` (symbols + no LTO-inlining of the toggle target).
- **Phase 1b — binary bench DONE + pushed (`da22d832`).** `det_check` runs the
  REAL release `alint check` over fixed `gen-monorepo` trees (S1/S6/S12 @ 1k/10k,
  configs via `include_str!` of the xtask scenario YAMLs — one source of truth).
  Separate process ⇒ no toggle/inlining concern; trees materialized to a fixed
  path so `Ir` is byte-stable. Verified: s1_10k=322M Ir (walk), s6_10k=1.76B
  (content), branch live. gungraun-runner + valgrind installed (passwordless sudo).
- **Phase 1c — CI gate DONE + pushed (`24915fdb`).** `perf-gate` CI job +
  `ci/scripts/det-perf-gate.sh`: PR vs merge-base, gungraun `soft_limits` gate,
  load-immune (runs on the self-hosted runner regardless of co-tenants),
  ADVISORY-first (`DET_PERF_ADVISORY=1`). **No committed baseline** — the raw
  gungraun output is ~18 MB, so the in-CI fresh merge-base comparison is used
  (zero drift, exact same-env; user approved 2026-06-07).
- **Polish DONE:** `det_check` widened to S1/S2/S6/S7/S12 (× 1k/10k); the 100k
  tier added behind the `det-100k` cargo feature (release-time, via `cfg_attr` on
  the bench cells — compile-verified both ways); valgrind pinned in
  `bench/Dockerfile` (bookworm base = 3.19); RELEASING.md demotes wall-clock
  `bench-scale` to characterization + names the deterministic gate primary.
- **Remaining:** flip `Ir` advisory→gating (`DET_PERF_ADVISORY=0`) after the
  rollout calibrates against real PR noise; optional strace syscall check;
  runner-isolation (separate track).

### Reusable notes
- gungraun crate = **`gungraun` 0.19.1** (renamed from `iai-callgrind`); runner =
  `cargo install gungraun-runner --version 0.19.1` (must version-match). cargo-deny
  clean. `Callgrind::default().args(["--cache-sim=yes","--branch-sim=yes"])`
  (append — `with_args` REPLACES gungraun's collection toggle → 0 counts).
- Library bench: `#[library_benchmark(setup = fixture)]` + `#[bench::id(args)]`.
  Binary bench: `#[binary_benchmark(setup = materialize)]` + `#[bench::id(args)]`
  where the SAME args go to BOTH `setup` and the bench fn (which returns a
  `gungraun::Command`); setup's return is ignored.
