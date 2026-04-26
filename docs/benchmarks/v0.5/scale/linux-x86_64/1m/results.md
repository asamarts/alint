# alint bench-scale — 1m files

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
| S1 | full | 3534.7 | 104.1 | 3460.8 | 3653.7 | 3 |
| S1 | changed | 6377.7 | 212.2 | 6195.6 | 6610.8 | 3 |
| S2 | full | 10289.5 | 662.8 | 9786.4 | 11040.5 | 3 |
| S2 | changed | 6782.6 | 35.9 | 6751.3 | 6821.8 | 3 |
| S3 | full | 569077.5 | 60910.6 | 532134.4 | 639380.5 | 3 |
| S3 | changed | 528103.3 | 2536.8 | 525966.7 | 530906.9 | 3 |

Tree shape: monorepo (`packages=5000, files_per_package=198, total=1000000`).
