# alint bench-scale — 10k files

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
| alint | S1 | full | 20.3 | 1.0 | 18.7 | 21.6 | 10 |
| alint | S1 | changed | 46.2 | 0.8 | 44.8 | 47.5 | 10 |
| alint | S2 | full | 29.9 | 0.8 | 28.4 | 31.5 | 10 |
| alint | S2 | changed | 48.5 | 0.9 | 47.3 | 49.8 | 10 |
| alint | S3 | full | 316.4 | 9.0 | 305.8 | 333.3 | 10 |
| alint | S3 | changed | 276.3 | 2.6 | 272.8 | 281.4 | 10 |

Tree shape: monorepo (`packages=200, files_per_package=48, total=10000`).
