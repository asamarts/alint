# alint perf history

One row per published release tag, chronologically newest at top. Headline
cells are platform-fingerprinted to `linux-x86_64` (AMD Ryzen 9 3900X /
ext4 / rustc 1.95) — see [`METHODOLOGY.md`](METHODOLOGY.md) for the
hardware contract and why cross-machine comparisons need like-for-like.

| Tag | Date | 1M S3 full | 1M S3 changed | 100k S3 full | 10k S3 full | Headline change |
|---|---|---:|---:|---:|---:|---|
| **v0.9.6** | 2026-05-02 | — | — | 11.20 s | 316 ms | `scope_filter:` primitive + bundled-ruleset migration; new S9 = 688 ms at 100k. |
| v0.9.5 | 2026-05-01 | 11.194 s ± 0.154 | 6.728 s ± 0.059 | 11.20 s | 316 ms | Cross-file dispatch fast paths (path-index on FileIndex) — 65× / 108× over v0.9.4. |
| v0.9.4 | 2026-04-30 | 731.856 s ± 5.349 | 724.362 s ± 2.132 | 11.20 s | 316 ms | Content-rule mechanical migration (16 rules to PerFileRule). |
| v0.9.3 | 2026-04-30 | — | — | 11.39 s | 355 ms | Per-file dispatch flip + 8-rule reference migration. |
| v0.9.2 | 2026-04-30 | — | — | 11.39 s | 355 ms | Memory-footprint pass (Arc<Path> / Cow types). |
| v0.9.1 | 2026-04-30 | — | — | — | — | Parallel walker (-64 % at 10k). |
| v0.8.x | 2026-04 | — | — | — | — | Test foundation; no measured perf change. |
| v0.7.0 | 2026-04 | — | — | — | — | `bench-compare` floor — every later release gates against this. |
| v0.5.7 | 2026-03 | — | — | 11.39 s | 355 ms | First publish-grade `bench-scale` matrix at 1k/10k/100k. |
| v0.5.6 | 2026-03 | 569.078 s ± 60.911 | 528.103 s ± 2.537 | — | — | Prep run that captured the only pre-v0.9 1M S3 numbers. |

Data sources:

- 1M cells: [`macro/results/linux-x86_64/<tag>/1m/results.md`](macro/results/linux-x86_64/)
- 100k / 10k cells: same dir, `<size>/results.md`
- v0.9.5 cells: [`macro/results/linux-x86_64/v0.9.5/`](macro/results/linux-x86_64/v0.9.5/) — captured `--warmup 1 --runs 3` (others used `--warmup 3 --runs 10`); the smaller-N runs are honest about per-iteration cost since the path-index fix dropped wall time below where 10 measurements add a meaningful signal-to-noise.

## How to add a row

When a release tag lands:

1. Run `xtask bench-scale --include-1m --scenarios S1,S2,S3` against `--out docs/benchmarks/macro/results/<arch>/<version>/`. Default `--warmup 3 --runs 10` for new publications.
2. Copy the headline cells (1M S3 full / changed, 100k S3 full, 10k S3 full) into a new row at the top of the table above.
3. The "Headline change" cell is one sentence — what *would* a reader want to know. Avoid jargon; link to the design doc for depth.

## Cross-version perf investigations

Significant deltas (anything > 20% across a release) get an investigation
write-up under [`investigations/<YYYY-MM-topic>/`](investigations/) that
captures the diagnostic data (traces, flamegraphs, bisect notes). The
v0.9.5 cliff investigation lives at
[`investigations/2026-05-cross-file-rules/`](investigations/2026-05-cross-file-rules/).
