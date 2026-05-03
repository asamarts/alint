# alint bench-scale — 1k files

**Platform:** `linux/x86_64`  
**CPU:** `AMD Ryzen 9 3900X 12-Core Processor` (24 cores)  
**RAM:** 62 GB  
**FS:** `ext4`  
**rustc:** `rustc 1.95.0 (59807616e 2026-04-14)`  
**alint:** `0.9.11` (df7dc57)  
**hyperfine:** `1.20.0`  
**Tools:** alint=`0.9.11`  
**Seed:** `0xa11e47`  
**Warmup/runs:** 3 / 10  
**Generated:** `unix:1777849433`  

Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md`. Compare numbers like-for-like (same fingerprint), never absolutely.

## Rows

| Tool | Scenario | Mode | Mean (ms) | Stddev | Min | Max | Samples |
|---|---|---|---:|---:|---:|---:|---:|
| alint | S1 | full | 8.4 | 0.7 | 6.9 | 9.4 | 10 |
| alint | S1 | changed | 23.5 | 10.8 | 19.2 | 54.3 | 10 |
| alint | S2 | full | 10.4 | 0.6 | 9.6 | 11.1 | 10 |
| alint | S2 | changed | 20.4 | 0.7 | 19.2 | 21.5 | 10 |
| alint | S3 | full | 23.4 | 1.4 | 21.5 | 26.7 | 10 |
| alint | S3 | changed | 29.3 | 1.4 | 27.5 | 32.7 | 10 |
| alint | S4 | full | 9.8 | 0.9 | 8.4 | 11.5 | 10 |
| alint | S4 | changed | 26.0 | 15.7 | 19.1 | 70.7 | 10 |
| alint | S5 | full | 14.3 | 0.9 | 12.8 | 16.0 | 10 |
| alint | S5 | changed | 19.7 | 0.9 | 18.4 | 21.0 | 10 |
| alint | S6 | full | 17.0 | 0.6 | 16.0 | 17.7 | 10 |
| alint | S6 | changed | 20.6 | 0.6 | 20.0 | 21.8 | 10 |
| alint | S7 | full | 10.9 | 0.8 | 9.6 | 12.6 | 10 |
| alint | S7 | changed | 22.1 | 0.9 | 20.8 | 24.1 | 10 |
| alint | S8 | full | 26.3 | 13.6 | 21.0 | 65.0 | 10 |
| alint | S8 | changed | 26.6 | 0.6 | 25.6 | 27.5 | 10 |
| alint | S9 | full | 14.5 | 0.9 | 12.9 | 16.1 | 10 |
| alint | S9 | changed | 21.9 | 1.0 | 20.7 | 23.8 | 10 |
| alint | S10 | full | 10.1 | 0.7 | 8.8 | 11.3 | 10 |
| alint | S10 | changed | 23.6 | 10.7 | 19.2 | 54.0 | 10 |

Tree shape: monorepo (`packages=50, files_per_package=18, total=1000`).
