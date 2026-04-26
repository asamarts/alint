# alint bench-scale results

**Platform:** `linux/x86_64`  
**CPU:** `AMD Ryzen 9 3900X 12-Core Processor` (24 cores)  
**RAM:** 62 GB  
**FS:** `ext4`  
**rustc:** `rustc 1.95.0 (59807616e 2026-04-14)`  
**alint:** `0.5.6` (25d3683)  
**hyperfine:** `1.20.0`  
**Seed:** `0xa11e47`  
**Warmup/runs:** 3 / 10  
**Generated:** `unix:1777224389`  

Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md`. Compare numbers like-for-like (same fingerprint), never absolutely.

Per-size detail under `<size>/results.md`. JSON: `results.json`.

## Scenarios

- **S1** — Filename hygiene (8 rules)
- **S2** — Existence + content (8 rules)
- **S3** — Workspace bundle (oss-baseline + rust + monorepo + cargo-workspace)

## Summary (mean ± stddev, ms)

| Size | Scenario | Mode | Mean | Stddev | Min | Max | Samples |
|---|---|---|---:|---:|---:|---:|---:|
| 1k | S1 | full | 8.7 | 0.3 | 8.2 | 9.1 | 10 |
| 1k | S1 | changed | 21.0 | 0.6 | 20.4 | 22.4 | 10 |
| 1k | S2 | full | 15.3 | 0.4 | 14.9 | 16.3 | 10 |
| 1k | S2 | changed | 22.6 | 0.5 | 22.2 | 23.6 | 10 |
| 1k | S3 | full | 29.6 | 1.4 | 28.0 | 33.0 | 10 |
| 1k | S3 | changed | 40.1 | 11.6 | 33.8 | 72.1 | 10 |
| 10k | S1 | full | 43.9 | 0.3 | 43.4 | 44.3 | 10 |
| 10k | S1 | changed | 70.6 | 1.2 | 69.1 | 73.0 | 10 |
| 10k | S2 | full | 105.1 | 1.2 | 103.4 | 106.8 | 10 |
| 10k | S2 | changed | 78.4 | 3.3 | 75.8 | 87.4 | 10 |
| 10k | S3 | full | 360.2 | 9.4 | 350.6 | 378.0 | 10 |
| 10k | S3 | changed | 332.1 | 12.2 | 320.1 | 357.7 | 10 |
| 100k | S1 | full | 369.8 | 6.8 | 360.9 | 381.5 | 10 |
| 100k | S1 | changed | 641.5 | 11.1 | 622.3 | 657.0 | 10 |
| 100k | S2 | full | 986.8 | 15.8 | 962.3 | 1007.2 | 10 |
| 100k | S2 | changed | 705.9 | 16.4 | 666.4 | 723.4 | 10 |
| 100k | S3 | full | 11936.2 | 267.2 | 11683.0 | 12579.9 | 10 |
| 100k | S3 | changed | 11712.4 | 214.4 | 11407.7 | 12063.6 | 10 |
| 1m | S1 | full | 3534.7 | 104.1 | 3460.8 | 3653.7 | 3 |
| 1m | S1 | changed | 6377.7 | 212.2 | 6195.6 | 6610.8 | 3 |
| 1m | S2 | full | 10289.5 | 662.8 | 9786.4 | 11040.5 | 3 |
| 1m | S2 | changed | 6782.6 | 35.9 | 6751.3 | 6821.8 | 3 |
| 1m | S3 | full | 569077.5 | 60910.6 | 532134.4 | 639380.5 | 3 |
| 1m | S3 | changed | 528103.3 | 2536.8 | 525966.7 | 530906.9 | 3 |
