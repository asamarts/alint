# alint bench-scale — 100k files

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
| S1 | full | 359.4 | 8.8 | 352.0 | 377.8 | 10 |
| S1 | changed | 621.2 | 20.3 | 589.2 | 654.9 | 10 |
| S2 | full | 960.6 | 14.5 | 936.0 | 990.4 | 10 |
| S2 | changed | 690.0 | 13.0 | 669.6 | 713.0 | 10 |
| S3 | full | 11436.0 | 109.6 | 11258.1 | 11608.8 | 10 |
| S3 | changed | 11068.2 | 113.0 | 10937.5 | 11231.3 | 10 |

Tree shape: monorepo (`packages=1000, files_per_package=98, total=100000`).
