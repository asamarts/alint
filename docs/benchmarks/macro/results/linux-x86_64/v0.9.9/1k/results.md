# alint bench-scale — 1k files

**Platform:** `linux/x86_64`  
**CPU:** `AMD Ryzen 9 3900X 12-Core Processor` (24 cores)  
**RAM:** 62 GB  
**FS:** `ext4`  
**rustc:** `rustc 1.95.0 (59807616e 2026-04-14)`  
**alint:** `0.9.9` (ed62e27)  
**hyperfine:** `1.20.0`  
**Tools:** alint=`0.9.9`  
**Seed:** `0xa11e47`  
**Warmup/runs:** 3 / 10  
**Generated:** `unix:1777793233`  

Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md`. Compare numbers like-for-like (same fingerprint), never absolutely.

## Rows

| Tool | Scenario | Mode | Mean (ms) | Stddev | Min | Max | Samples |
|---|---|---|---:|---:|---:|---:|---:|
| alint | S1 | full | 8.0 | 0.4 | 7.6 | 9.0 | 10 |
| alint | S1 | changed | 20.1 | 0.7 | 19.2 | 21.6 | 10 |
| alint | S2 | full | 11.3 | 1.1 | 9.9 | 13.2 | 10 |
| alint | S2 | changed | 28.9 | 14.2 | 21.9 | 68.8 | 10 |
| alint | S3 | full | 30.6 | 13.7 | 22.0 | 66.7 | 10 |
| alint | S3 | changed | 30.5 | 1.7 | 27.4 | 32.5 | 10 |
| alint | S4 | full | 9.6 | 0.7 | 9.0 | 11.2 | 10 |
| alint | S4 | changed | 21.0 | 0.7 | 20.2 | 22.2 | 10 |
| alint | S5 | full | 18.6 | 13.8 | 12.7 | 57.7 | 10 |
| alint | S5 | changed | 20.7 | 0.7 | 19.8 | 22.0 | 10 |
| alint | S6 | full | 17.3 | 1.2 | 15.7 | 19.0 | 10 |
| alint | S6 | changed | 21.9 | 1.5 | 19.3 | 24.6 | 10 |
| alint | S7 | full | 14.0 | 7.7 | 9.8 | 35.8 | 10 |
| alint | S7 | changed | 24.9 | 2.1 | 22.4 | 28.2 | 10 |
| alint | S8 | full | 32.8 | 10.5 | 23.3 | 51.5 | 10 |
| alint | S8 | changed | 28.4 | 1.2 | 26.8 | 30.1 | 10 |
| alint | S9 | full | 14.9 | 0.9 | 13.9 | 16.1 | 10 |
| alint | S9 | changed | 22.4 | 0.8 | 21.4 | 24.1 | 10 |
| alint | S10 | full | 14.4 | 12.2 | 9.6 | 49.0 | 10 |
| alint | S10 | changed | 20.3 | 0.7 | 19.6 | 21.9 | 10 |

Tree shape: monorepo (`packages=50, files_per_package=18, total=1000`).
