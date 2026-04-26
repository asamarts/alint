# alint bench-scale results

**Platform:** `linux/x86_64`  
**CPU:** `AMD Ryzen 9 3900X 12-Core Processor` (24 cores)  
**RAM:** 62 GB  
**FS:** `ext4`  
**rustc:** `rustc 1.95.0 (59807616e 2026-04-14)`  
**alint:** `0.5.7` (7ff4ed5)  
**hyperfine:** `1.20.0`  
**Tools:** alint=`0.5.7`, grep=`ripgrep 15.1.0`, ls-lint=`ls-lint v2.2.3`, repolinter=`0.11.2`  
**Seed:** `0xa11e47`  
**Warmup/runs:** 3 / 10  
**Generated:** `unix:1777233844`  

Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md`. Compare numbers like-for-like (same fingerprint), never absolutely.

Per-size detail under `<size>/results.md`. JSON: `results.json`.

> **1M-file numbers** (alint-only) live under
> [`1m/results.md`](1m/results.md) — preserved from the
> v0.5.7-prep run. `repolinter` at the 1m / S2 size would
> exceed an hour per row at default sampling, so the
> competitive matrix above tops out at 100k. Future runs
> may include 1m once we publish a sampling-reduced
> "competitive 1m" mode.

## Scenarios

- **S1** — Filename hygiene (8 rules)
- **S2** — Existence + content (8 rules)
- **S3** — Workspace bundle (oss-baseline + rust + monorepo + cargo-workspace)

## Summary (mean ± stddev, ms)

| Tool | Size | Scenario | Mode | Mean | Stddev | Min | Max | Samples |
|---|---|---|---|---:|---:|---:|---:|---:|
| alint | 1k | S1 | full | 8.9 | 0.2 | 8.6 | 9.4 | 10 |
| alint | 1k | S1 | changed | 13.3 | 0.3 | 12.9 | 14.0 | 10 |
| ls-lint | 1k | S1 | full | 27.9 | 0.7 | 27.1 | 29.8 | 10 |
| grep | 1k | S1 | full | 58.4 | 0.8 | 57.5 | 60.1 | 10 |
| alint | 1k | S2 | full | 14.8 | 0.2 | 14.4 | 15.3 | 10 |
| alint | 1k | S2 | changed | 14.7 | 0.3 | 14.4 | 15.3 | 10 |
| grep | 1k | S2 | full | 42.5 | 1.0 | 40.8 | 43.8 | 10 |
| repolinter | 1k | S2 | full | 486.5 | 25.3 | 444.5 | 532.0 | 10 |
| alint | 1k | S3 | full | 28.6 | 1.1 | 27.4 | 31.3 | 10 |
| alint | 1k | S3 | changed | 27.6 | 0.5 | 27.2 | 28.9 | 10 |
| alint | 10k | S1 | full | 43.1 | 0.7 | 41.6 | 43.6 | 10 |
| alint | 10k | S1 | changed | 72.8 | 6.3 | 69.6 | 86.1 | 10 |
| ls-lint | 10k | S1 | full | 28.2 | 0.9 | 26.5 | 29.6 | 10 |
| grep | 10k | S1 | full | 194.5 | 2.6 | 191.3 | 200.9 | 10 |
| alint | 10k | S2 | full | 103.6 | 1.3 | 101.5 | 105.8 | 10 |
| alint | 10k | S2 | changed | 83.0 | 14.8 | 74.0 | 112.8 | 10 |
| grep | 10k | S2 | full | 133.1 | 11.5 | 127.2 | 165.1 | 10 |
| repolinter | 10k | S2 | full | 1637.9 | 37.0 | 1592.8 | 1700.8 | 10 |
| alint | 10k | S3 | full | 355.9 | 9.8 | 343.3 | 374.1 | 10 |
| alint | 10k | S3 | changed | 324.3 | 4.3 | 318.3 | 331.1 | 10 |
| alint | 100k | S1 | full | 365.7 | 21.0 | 352.6 | 424.2 | 10 |
| alint | 100k | S1 | changed | 623.0 | 8.7 | 605.9 | 637.1 | 10 |
| ls-lint | 100k | S1 | full | 27.4 | 0.4 | 27.1 | 28.4 | 10 |
| grep | 100k | S1 | full | 1412.4 | 10.7 | 1395.2 | 1429.3 | 10 |
| alint | 100k | S2 | full | 965.4 | 9.3 | 949.7 | 976.8 | 10 |
| alint | 100k | S2 | changed | 685.2 | 12.5 | 659.8 | 699.7 | 10 |
| grep | 100k | S2 | full | 926.7 | 8.3 | 913.7 | 940.0 | 10 |
| repolinter | 100k | S2 | full | 13761.1 | 108.4 | 13616.8 | 13981.0 | 10 |
| alint | 100k | S3 | full | 11391.9 | 73.4 | 11293.4 | 11525.6 | 10 |
| alint | 100k | S3 | changed | 11063.4 | 74.9 | 10963.7 | 11181.4 | 10 |
