# alint bench-scale — 1k files

**Platform:** `linux/x86_64`  
**CPU:** `AMD Ryzen 9 3900X 12-Core Processor` (24 cores)  
**RAM:** 62 GB  
**FS:** `ext4`  
**rustc:** `rustc 1.95.0 (59807616e 2026-04-14)`  
**alint:** `0.9.10` (a75dd26)  
**hyperfine:** `1.20.0`  
**Tools:** alint=`0.9.10`  
**Seed:** `0xa11e47`  
**Warmup/runs:** 3 / 10  
**Generated:** `unix:1777829895`  

Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md`. Compare numbers like-for-like (same fingerprint), never absolutely.

## Rows

| Tool | Scenario | Mode | Mean (ms) | Stddev | Min | Max | Samples |
|---|---|---|---:|---:|---:|---:|---:|
| alint | S1 | full | 8.2 | 0.7 | 6.9 | 9.4 | 10 |
| alint | S1 | changed | 20.1 | 0.8 | 18.6 | 20.9 | 10 |
| alint | S2 | full | 10.8 | 1.3 | 8.9 | 13.1 | 10 |
| alint | S2 | changed | 21.5 | 0.8 | 20.4 | 23.0 | 10 |
| alint | S3 | full | 23.4 | 1.0 | 21.8 | 25.0 | 10 |
| alint | S3 | changed | 28.7 | 0.9 | 27.4 | 30.3 | 10 |
| alint | S4 | full | 9.4 | 0.9 | 8.2 | 11.5 | 10 |
| alint | S4 | changed | 25.7 | 14.4 | 20.6 | 66.6 | 10 |
| alint | S5 | full | 19.0 | 12.2 | 13.9 | 53.6 | 10 |
| alint | S5 | changed | 20.2 | 0.7 | 19.1 | 21.1 | 10 |
| alint | S6 | full | 17.2 | 0.7 | 16.3 | 18.5 | 10 |
| alint | S6 | changed | 20.5 | 0.4 | 20.0 | 21.6 | 10 |
| alint | S7 | full | 11.3 | 0.6 | 10.4 | 12.0 | 10 |
| alint | S7 | changed | 22.8 | 1.1 | 22.0 | 25.5 | 10 |
| alint | S8 | full | 24.2 | 6.9 | 20.7 | 43.6 | 10 |
| alint | S8 | changed | 29.9 | 8.7 | 26.4 | 54.4 | 10 |
| alint | S9 | full | 14.7 | 0.6 | 14.0 | 15.6 | 10 |
| alint | S9 | changed | 22.4 | 1.1 | 20.8 | 24.1 | 10 |
| alint | S10 | full | 10.6 | 0.7 | 9.3 | 11.9 | 10 |
| alint | S10 | changed | 20.7 | 1.1 | 19.8 | 23.3 | 10 |

Tree shape: monorepo (`packages=50, files_per_package=18, total=1000`).
