# 2026-09 - v0.16.0 changed-mode small-cell "regression": a bench artifact, not code

Status: **Resolved (no code change).** v0.16.0's `bench-record` failed the regression
gate with the small `changed`-mode cells inflated **+40 to +92%** vs the committed
v0.15.0 baseline (`S1..S12 1k/10k changed`), while every `full` cell and the large
`changed`/100k + `changed`/1m cells were flat. A tight, same-host, same-conditions A/B
proved **v0.16.0 is indistinguishable from v0.15.0** on these cells; the flag is a
baseline-vs-current-environment artifact on the floor-level `changed`-mode small cells,
which are dominated by the multi-core wakeup cost of a tiny parallel burst and are
version-independent. Do not treat it as a v0.16.0 code regression.

## Symptom

`bench-record` for v0.16.0 (run twice) flagged, both times, only the small `changed`
cells against `docs/benchmarks/macro/results/linux-x86_64/v0.15.0/results.json`:

| mode / size | delta vs committed v0.15.0 |
|---|---|
| `full` (1k/10k/100k/1m) | ~0% (flat) |
| `changed` / 100k, / 1m | +0.2%, +0.4% (flat) |
| `changed` / 10k | +49.6% (run 1), +92.0% (run 2) |
| `changed` / 1k | +40.1%, +43.3% |

The re-run being *worse* while kbench was idle broke the first hypothesis (below).

## Hypotheses tested

1. **Runner contamination (WRONG).** First call: the release-day bench overlapped
   CI + docs-bundle + bench-docker on the shared host. Refuted: `uptime` on kbench
   showed load 0.03, no co-tenant processes; and a reproducible, same-signed +40-92%
   across two idle-box runs is the opposite of random contamination.
2. **A `with_worker_pool` rayon regression (WRONG).** v0.16.0 (PR #223) replaced the
   bare `self.entries.par_iter().collect()` at both engine dispatch sites with
   `with_worker_pool(|| ...)` -> `pool.install(job)` on an explicitly-built cached
   pool. Hypothesis: `install()` blocks the caller and hands the tiny changed-mode
   workload to cold worker threads (a C-state-exit + clock-ramp penalty), which would
   hit only small `changed` cells and be worse on an idler box. A noisy lean A/B
   (single-run mins) seemed to support it (+13% default, 3x faster at 1 thread).
   **Refuted by tight measurement** (see below): at 30 runs / CV ~1%, v0.16.0 and
   v0.15.0 are identical, and the 3x single-thread effect is present in *both*
   versions equally. The `with_worker_pool` change is perf-neutral.

## The measurement that settled it

`S1 10k changed`, `xtask bench-scale --runs 30`, both binaries built from their tags
on kbench and measured back-to-back on the idle box:

| condition | min | median | mean | stddev | CV% |
|---|--:|--:|--:|--:|--:|
| v0.15.0 default (8 threads) | 114.68 | 116.95 | 116.94 | 1.13 | 1.0 |
| v0.16.0 default (8 threads) | 115.02 | 116.84 | 117.16 | 1.45 | 1.2 |
| v0.15.0 `RAYON_NUM_THREADS=1` | 41.30 | 43.60 | 43.57 | 1.08 | 2.5 |
| v0.16.0 `RAYON_NUM_THREADS=1` | 41.37 | 43.16 | 43.28 | 0.96 | 2.2 |

v0.16.0 == v0.15.0 (within 0.1% at 8 threads, 1% at 1 thread). **No regression.**

## Root cause: a version-independent environment artifact on the floor cells

The same v0.15.0 binary, on the same i7-6700HQ, measures differently now than when the
baseline was recorded:

| S1 10k | committed v0.15.0 baseline | fresh v0.15.0 on kbench now |
|---|--:|--:|
| `full` | 25.4 ms | 23.7 ms (flat) |
| `changed` | **43.2 ms** | **109-117 ms** (2.5x) |

Two facts localize it:

- `full` mode is unchanged, so it is not a broad slowdown (clock, kernel, binary size).
- `RAYON_NUM_THREADS=1` today gives **43 ms** - matching the old baseline - while the
  default 8-thread run gives 117 ms, **for both versions**.

So the 43 -> 117 drift is entirely the **multi-core wakeup cost of the tiny
changed-mode parallel burst**. `changed` mode spawns `git` (single-threaded) to find
the changed subset, idling the other cores, then dispatches the rule `par_iter` over a
*tiny* subset - which must wake 7 cores out of deep idle. On current kbench those cores
sit at 800 MHz in deep C-states (C6-C10) with turbo disabled (`no_turbo=1`), so the
wake + ramp dominates a workload whose actual work is ~43 ms. Large `changed` cells
(100k/1m) and all `full` cells keep the cores saturated, so they never pay a per-burst
cold-wake and stay flat. The committed v0.15.0 baseline (43 ms) was captured when this
cost was negligible - the small `changed` cell ran warm inside the full back-to-back
matrix, and/or kbench's idle/C-state behavior differed from its current state (52-day
uptime; no historical power data to pin the exact trigger). The gate then compared
fresh v0.16.0 against that stale-cheap number and reported the environment shift as a
code regression.

This is the same class as the sibling investigations, with the twist that the
"artifact" call was correct here (unlike [`../2026-07-v0.14-s2-harness-artifact/`](../2026-07-v0.14-s2-harness-artifact/),
where an artifact call was *overturned* into a real regression): the discipline is to
verify either way with a same-host, same-conditions, tight-N A/B before concluding.

## Methodology lessons (reusable)

- **Single-run `min_ms` on floor-level cells is noise.** The lean A/B's single mins
  showed a phantom +13% / 3x asymmetry that vanished at 30 runs (CV ~1%). Use tight-N
  medians before asserting a delta on cells under ~50 ms.
- **Reproduce on the ACTUAL bench host and power state.** An earlier A/B ran on a
  24-core contended box and found "flat"; it could not see an effect that only appears
  when 7 of 8 kbench cores wake from deep C-states. Wrong core count *and* wrong power
  state.
- **Compare fresh-vs-fresh, not fresh-vs-committed-baseline, when the environment may
  have drifted.** The committed baseline is a point-in-time capture; a same-host
  re-measure of the *baseline binary* is the only valid regression control (see also
  [`../2026-05-bench-runner-instability/`](../2026-05-bench-runner-instability/)).
- **`RAYON_NUM_THREADS=1` is a cheap isolation knob** - it removes the multi-core
  wakeup, so if a small-cell delta collapses at 1 thread the cost is thread-dispatch /
  core-wake, not the analysis.
- **A quiet box can be *worse* for floor-level parallel-burst cells**, not better -
  idler cores are in deeper C-states, so waking them costs more. "Quiet" is not the
  same as "fast" for these cells.

## Resolution

No alint code change. The `changed`-mode `1k`/`10k` cells are environment-sensitive
floor cells and should not gate cross-version comparisons - they are already advisory
for the CV quality gate and should be advisory for the `min_ms` regression gate too
(tracked against [`../../../design/deterministic-perf-gating.md`](../../../design/deterministic-perf-gating.md);
the load-immune deterministic gate remains the authoritative regression signal). The
v0.16.0 bench numbers reflect current-kbench environment on those cells, not a
regression; the `full` and large-`changed` numbers are sound and comparable.
