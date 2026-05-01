# alint bench-scale — 1k files

**Platform:** `linux/x86_64`  
**CPU:** `AMD Ryzen 9 3900X 12-Core Processor` (24 cores)  
**RAM:** 62 GB  
**FS:** `ext4`  
**rustc:** `rustc 1.95.0 (59807616e 2026-04-14)`  
**alint:** `0.9.4` (90c4efa)  
**hyperfine:** `1.20.0`  
**Tools:** alint=`0.9.4`  
**Seed:** `0xa11e47`  
**Warmup/runs:** 3 / 10  
**Generated:** `unix:1777597105`  

Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md`. Compare numbers like-for-like (same fingerprint), never absolutely.

## Rows

| Tool | Scenario | Mode | Mean (ms) | Stddev | Min | Max | Samples |
|---|---|---|---:|---:|---:|---:|---:|
| alint | S1 | full | 7.8 | 0.5 | 6.9 | 8.8 | 10 |
| alint | S1 | changed | 13.4 | 1.1 | 12.4 | 15.9 | 10 |
| alint | S2 | full | 10.4 | 1.0 | 9.3 | 11.8 | 10 |
| alint | S2 | changed | 13.6 | 0.9 | 12.2 | 14.9 | 10 |
| alint | S3 | full | 27.8 | 1.4 | 26.0 | 31.2 | 10 |
| alint | S3 | changed | 26.4 | 1.0 | 25.3 | 28.5 | 10 |

Tree shape: monorepo (`packages=50, files_per_package=18, total=1000`).
