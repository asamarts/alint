# alint bench-scale — 10k files

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

## Rows

| Scenario | Mode | Mean (ms) | Stddev | Min | Max | Samples |
|---|---|---:|---:|---:|---:|---:|
| S1 | full | 42.5 | 0.5 | 42.0 | 43.6 | 10 |
| S1 | changed | 70.1 | 1.7 | 67.2 | 72.4 | 10 |
| S2 | full | 108.8 | 12.4 | 100.3 | 133.3 | 10 |
| S2 | changed | 77.1 | 2.4 | 74.3 | 82.0 | 10 |
| S3 | full | 350.4 | 7.7 | 342.8 | 363.1 | 10 |
| S3 | changed | 329.5 | 13.7 | 316.5 | 360.0 | 10 |

Tree shape: monorepo (`packages=200, files_per_package=48, total=10000`).
