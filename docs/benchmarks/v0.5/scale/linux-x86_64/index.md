# alint bench-scale results

**Platform:** `linux/x86_64`  
**CPU:** `AMD Ryzen 9 3900X 12-Core Processor` (24 cores)  
**RAM:** 62 GB  
**FS:** `ext4`  
**rustc:** `rustc 1.95.0 (59807616e 2026-04-14)`  
**alint:** `0.5.6` (8cda086)  
**hyperfine:** `1.20.0`  
**Seed:** `0xa11e47`  
**Warmup/runs:** 3 / 10  
**Generated:** `unix:1777191168`  

Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md`. Compare numbers like-for-like (same fingerprint), never absolutely.

Per-size detail under `<size>/results.md`. JSON: `results.json`.

## Scenarios

- **S1** — Filename hygiene (8 rules)
- **S2** — Existence + content (8 rules)
- **S3** — Workspace bundle (oss-baseline + rust + monorepo + cargo-workspace)

## Summary (mean ± stddev, ms)

| Size | Scenario | Mode | Mean | Stddev | Min | Max | Samples |
|---|---|---|---:|---:|---:|---:|---:|
| 1k | S1 | full | 8.8 | 0.3 | 8.4 | 9.3 | 10 |
| 1k | S1 | changed | 13.3 | 0.3 | 13.0 | 13.8 | 10 |
| 1k | S2 | full | 14.7 | 0.3 | 14.3 | 15.1 | 10 |
| 1k | S2 | changed | 14.6 | 0.5 | 13.8 | 15.6 | 10 |
| 1k | S3 | full | 28.9 | 0.7 | 28.0 | 30.0 | 10 |
| 1k | S3 | changed | 27.6 | 0.6 | 26.9 | 28.9 | 10 |
| 10k | S1 | full | 42.5 | 0.5 | 42.0 | 43.6 | 10 |
| 10k | S1 | changed | 70.1 | 1.7 | 67.2 | 72.4 | 10 |
| 10k | S2 | full | 108.8 | 12.4 | 100.3 | 133.3 | 10 |
| 10k | S2 | changed | 77.1 | 2.4 | 74.3 | 82.0 | 10 |
| 10k | S3 | full | 350.4 | 7.7 | 342.8 | 363.1 | 10 |
| 10k | S3 | changed | 329.5 | 13.7 | 316.5 | 360.0 | 10 |
| 100k | S1 | full | 359.4 | 8.8 | 352.0 | 377.8 | 10 |
| 100k | S1 | changed | 621.2 | 20.3 | 589.2 | 654.9 | 10 |
| 100k | S2 | full | 960.6 | 14.5 | 936.0 | 990.4 | 10 |
| 100k | S2 | changed | 690.0 | 13.0 | 669.6 | 713.0 | 10 |
| 100k | S3 | full | 11436.0 | 109.6 | 11258.1 | 11608.8 | 10 |
| 100k | S3 | changed | 11068.2 | 113.0 | 10937.5 | 11231.3 | 10 |
