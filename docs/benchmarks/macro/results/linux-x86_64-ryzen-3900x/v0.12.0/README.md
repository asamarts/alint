# v0.12.0 macro benchmark — CHARACTERIZATION (not a regression gate)

These are **characterization** numbers: absolute throughput captured by
`xtask bench-scale` (S1–S14 × {1k,10k,100k,1M} × {full,changed}, warmup 3 /
10 runs) on the dev box (self-hosted AMD Ryzen 9 3900X), on a **verified-quiet**
runner (mid-run 1-min load 0.89). They are NOT the regression gate — the
load-immune deterministic gate (`ci/scripts/det-perf-gate.sh`, design:
`docs/design/deterministic-perf-gating.md`) is.

## Heads-up: the S12 "+16% vs v0.11.0" is a stale-baseline artifact, NOT a regression

`xtask bench-gate` flags S12 (10k/100k/1M) at +15–16% `min_ms` against the v0.11.0
`results.json` in the sibling directory, and the *passing* cells are uniformly
elevated too (S1 +8.0%, S4 +6.3%, S13 +4.9%) — the signature of a baseline recorded
under faster box conditions, not a localized code regression. The v0.12 **code**
does not regress:

A load-immune det_check A/B (Valgrind; v0.11.0 binary vs v0.12.0 binary; identical
trees + config) puts the S12 engine path within **<1%** on both work and cycles:

| | `Ir` (instructions / work) | Estimated Cycles |
|---|---|---|
| S12 10k | +0.34% | +0.37% |
| S12 1k | +0.72% | +0.85% |

A +16% wall-clock slowdown is arithmetically impossible on +0.34% more work. The
only real delta is a benign indirect-branch-mispredict increase from v0.12's walker
symlink-security filter (path confinement), whose net cycle cost is the <1% above.

Full proof + the per-scenario table:
`docs/benchmarks/investigations/2026-06-v0.12-perf-validation/` (Phase 1c +
`s12-loadimmune-confirmation.txt`).

This is exactly why wall-clock-vs-stale-baseline is treated as characterization
here while the deterministic gate is the regression signal. A same-conditions
v0.11.0 re-baseline (or simply trusting the deterministic gate) collapses the S12
delta to the ~+2–3% real branch-mispredict cost.
