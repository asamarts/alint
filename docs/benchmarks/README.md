# alint benchmarks

How fast is alint, how do we measure it, and where do the numbers live.

## TL;DR — current published numbers

`linux-x86_64` (AMD Ryzen 9 3900X 12-core / 62 GB / ext4 / rustc 1.95).
Latest published release: **v0.9.6** (2026-05-02). Working baseline
for the next release: **v0.9.6-postfix** (post-release `scope_filter:`
runtime fix, see CHANGELOG `[Unreleased]`).

| Workload | v0.9.5 | v0.9.6 published | v0.9.6-postfix |
|---|---:|---:|---:|
| 100k S3 full (hyperfine) | — | 1135.27 ms | 1169.97 ms |
| 100k S6 full (hyperfine) | — | 1221.27 ms | **1066.68 ms** (-12.7 %) |
| 100k S9 full (hyperfine) | — | 738.58 ms | **691.83 ms** (-6.3 %) |
| 1M S3 full (hyperfine) | 11.194 s | (not captured at 1M) | (not captured at 1M) |
| 1M S3 changed (hyperfine) | 6.728 s | (not captured at 1M) | (not captured at 1M) |

S9 (nested polyglot, new in v0.9.6) — see [`macro/results/linux-x86_64/v0.9.6/`](macro/results/linux-x86_64/v0.9.6/)
and [`macro/results/linux-x86_64/v0.9.6-postfix/`](macro/results/linux-x86_64/v0.9.6-postfix/)
for the post-fix re-capture (the released v0.9.6 binary's `scope_filter:` field was a runtime no-op; the postfix numbers reflect the gate actually firing).

Source: [`macro/results/linux-x86_64/v0.9.6-postfix/`](macro/results/linux-x86_64/v0.9.6-postfix/) (working baseline) and [`macro/results/linux-x86_64/v0.9.5/`](macro/results/linux-x86_64/v0.9.5/) (prior 1M-scale baseline).

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
├── macro/               — hyperfine bench-scale (S1-S9, full e2e wall-time)
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

Every release runs `xtask bench-compare --threshold 10` against the v0.7.0
floor under [`micro/results/linux-x86_64/v0.7.0/criterion/`](micro/results/linux-x86_64/v0.7.0/). Per-phase deltas (commit-by-commit
during the v0.9 cut) compare against the prior phase's snapshot under
[`archive/v0.9-development-baselines/`](archive/).

A new release tag MUST land with a fresh `macro/results/<arch>/<version>/`
snapshot. The bench-coverage soft warning at
`crates/alint-e2e/tests/coverage_audit_bench_listing.rs` lists which rule
kinds aren't yet exercised by any S* scenario — see
[`macro/README.md`](macro/README.md) for how to extend.
