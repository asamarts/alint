# alint bench-scale — 100k files

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
| S1 | full | 369.8 | 6.8 | 360.9 | 381.5 | 10 |
| S1 | changed | 641.5 | 11.1 | 622.3 | 657.0 | 10 |
| S2 | full | 986.8 | 15.8 | 962.3 | 1007.2 | 10 |
| S2 | changed | 705.9 | 16.4 | 666.4 | 723.4 | 10 |
| S3 | full | 11936.2 | 267.2 | 11683.0 | 12579.9 | 10 |
| S3 | changed | 11712.4 | 214.4 | 11407.7 | 12063.6 | 10 |

Tree shape: monorepo (`packages=1000, files_per_package=98, total=100000`).
