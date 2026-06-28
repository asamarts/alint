# alint benchmarks

How fast is alint, how do we measure it, and where do the numbers live.

## TL;DR — current published numbers

`linux-x86_64` (AMD Ryzen 9 3900X 12-core / 62 GB / ext4 / rustc 1.95).
Latest published release: **v0.13.0** (2026-06-17). Headline full-run
wall-times at 1M files (sourced from [`HISTORY.md`](HISTORY.md)):

| Workload (1M, full) | v0.13.0 |
|---|---:|
| S3 workspace bundle | 11.82 s ± 0.06 |
| S6 per-file content fan-out | 11.57 s ± 0.27 |
| S7 cross-file relational | 15.41 s ± 0.13 |
| S9 nested polyglot | 7.48 s ± 0.11 |

The full per-scenario, per-size trajectory (every release × 1k/10k/100k/1M ×
full/changed) lives in [`HISTORY.md`](HISTORY.md); per-version raw snapshots
under [`macro/results/linux-x86_64/v0.13.0/`](macro/results/linux-x86_64/v0.13.0/)
and [`micro/results/linux-x86_64/v0.13.0/`](micro/results/linux-x86_64/v0.13.0/).

## Layout

```
docs/benchmarks/
├── README.md            ← you are here
├── METHODOLOGY.md       — how the harness works (criterion + hyperfine)
├── HISTORY.md           — per-release perf changelog (one row per release)
├── RUNNING.md           — how to run benches yourself
│
├── micro/               — criterion micro-benchmarks
│   ├── README.md        — what each of the 12 micro-benches measures
│   └── results/<arch>/<version>/criterion/   — published snapshots
│
├── macro/               — hyperfine bench-scale (S1-S14, full e2e wall-time)
│   ├── README.md        — what each scenario tests + tool matrix
│   └── results/<arch>/<version>/             — published snapshots
│
├── investigations/      — ad-hoc deep-dives (traces, flamegraphs, write-ups)
│   ├── README.md
│   └── <YYYY-MM-topic>/
│
└── archive/             — superseded snapshots, kept for cross-version diffs
    └── README.md
```

## Reading guide

- **"How fast is alint at scale?"** → [`macro/`](macro/) → pick the latest version under `results/<arch>/`.
- **"Did this PR regress a hot path?"** → run `cargo bench -p alint-bench` locally, then `xtask bench-compare --before docs/benchmarks/micro/results/linux-x86_64/<prior>/criterion --after target/criterion`.
- **"What did we measure across releases?"** → [`HISTORY.md`](HISTORY.md).
- **"How do I add a new benchmark?"** → [`RUNNING.md`](RUNNING.md) and the per-section READMEs under `micro/` / `macro/`.
- **"What was the v0.9.5 perf investigation that found the +28% regression?"** → [`investigations/2026-05-cross-file-rules/`](investigations/2026-05-cross-file-rules/).
- **"Where are the v0.9 development-cycle phase snapshots?"** → [`archive/v0.9-development-phases/`](archive/) (kept for cross-phase diffs; do not edit).

## Two layers

alint's hot path combines two cost models, so we measure each at its own
granularity:

| Layer | Tool | What it captures | When to look |
|---|---|---|---|
| **Micro** | [`criterion`](https://docs.rs/criterion) via `cargo bench -p alint-bench` | Pure-CPU primitives: glob compile/match, regex content scans, engine fan-out, walker, formatters | After every change to `alint-core` or `alint-rules`. Fast (seconds), stable, cross-platform. |
| **Macro** | [`hyperfine`](https://github.com/sharkdp/hyperfine) via `xtask bench-scale` | End-to-end CLI wall-time over deterministic synthetic monorepos at 1k / 10k / 100k / 1M files | Before each release tag. Slow (minutes to hours at 1M), platform-dependent, honest about variance. |

Numbers are published per platform under `<layer>/results/<arch>/<version>/`.
Cross-machine comparisons require like-for-like fingerprints — see
[`METHODOLOGY.md`](METHODOLOGY.md) for the rationale.

## Regression gate

Per PR, CI runs two bench jobs (`ci.yml`): `bench-smoke` — a fast hyperfine
smoke check that the macro harness still runs end-to-end (**non-gating**) — and
`perf-gate`, the deterministic gungraun gate (`ci/scripts/det-perf-gate.sh`: `Ir`
+2% / `EstimatedCycles` +5% vs the PR's merge-base), currently **advisory**
(`DET_PERF_ADVISORY=1`, so it annotates rather than fails). Wall-clock regression
is gated per-release by `xtask bench-gate` (cross-version `min_ms`; the publish
criterion in [`../../RELEASING.md`](../../RELEASING.md)), trustworthy only on a
verified-quiet box; `xtask bench-compare` against the v0.7.0 micro floor under
[`micro/results/linux-x86_64/v0.7.0/criterion/`](micro/results/linux-x86_64/v0.7.0/)
is a local helper, not wired into any workflow. See [`METHODOLOGY.md`](METHODOLOGY.md)
and [`../design/deterministic-perf-gating.md`](../design/deterministic-perf-gating.md).

A new release tag MUST land with a fresh `macro/results/<arch>/<version>/`
snapshot. The bench-coverage soft warning at
`crates/alint-e2e/tests/coverage_audit_bench_listing.rs` lists which rule
kinds aren't yet exercised by any S* scenario — see
[`macro/README.md`](macro/README.md) for how to extend.
