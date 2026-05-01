# alint bench-scale — 100k files

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
| alint | S1 | full | 154.2 | 13.9 | 144.4 | 184.9 | 10 |
| alint | S1 | changed | 419.7 | 15.8 | 387.6 | 436.1 | 10 |
| alint | S2 | full | 236.7 | 11.6 | 212.9 | 250.6 | 10 |
| alint | S2 | changed | 423.4 | 15.1 | 402.6 | 447.9 | 10 |
| alint | S3 | full | 11200.6 | 131.1 | 10913.9 | 11368.9 | 10 |
| alint | S3 | changed | 10945.0 | 260.9 | 10707.5 | 11479.0 | 10 |

Tree shape: monorepo (`packages=1000, files_per_package=98, total=100000`).
