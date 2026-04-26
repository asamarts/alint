# alint bench-scale — 1k files

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

## Rows

| Scenario | Mode | Mean (ms) | Stddev | Min | Max | Samples |
|---|---|---:|---:|---:|---:|---:|
| S1 | full | 8.7 | 0.3 | 8.2 | 9.1 | 10 |
| S1 | changed | 21.0 | 0.6 | 20.4 | 22.4 | 10 |
| S2 | full | 15.3 | 0.4 | 14.9 | 16.3 | 10 |
| S2 | changed | 22.6 | 0.5 | 22.2 | 23.6 | 10 |
| S3 | full | 29.6 | 1.4 | 28.0 | 33.0 | 10 |
| S3 | changed | 40.1 | 11.6 | 33.8 | 72.1 | 10 |

Tree shape: monorepo (`packages=50, files_per_package=18, total=1000`).
