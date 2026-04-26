# alint bench-scale — 1k files

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
| S1 | full | 8.8 | 0.3 | 8.4 | 9.3 | 10 |
| S1 | changed | 13.3 | 0.3 | 13.0 | 13.8 | 10 |
| S2 | full | 14.7 | 0.3 | 14.3 | 15.1 | 10 |
| S2 | changed | 14.6 | 0.5 | 13.8 | 15.6 | 10 |
| S3 | full | 28.9 | 0.7 | 28.0 | 30.0 | 10 |
| S3 | changed | 27.6 | 0.6 | 26.9 | 28.9 | 10 |

Tree shape: monorepo (`packages=50, files_per_package=18, total=1000`).
