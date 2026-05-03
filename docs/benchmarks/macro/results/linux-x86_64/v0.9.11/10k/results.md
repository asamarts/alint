# alint bench-scale — 10k files

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
| alint | S1 | full | 20.4 | 0.8 | 19.2 | 21.8 | 10 |
| alint | S1 | changed | 48.7 | 5.9 | 45.1 | 64.9 | 10 |
| alint | S2 | full | 31.7 | 1.0 | 29.9 | 33.0 | 10 |
| alint | S2 | changed | 53.2 | 8.0 | 47.8 | 69.4 | 10 |
| alint | S3 | full | 119.2 | 2.6 | 116.7 | 125.5 | 10 |
| alint | S3 | changed | 78.4 | 1.2 | 76.0 | 80.0 | 10 |
| alint | S4 | full | 22.1 | 1.1 | 20.8 | 24.2 | 10 |
| alint | S4 | changed | 47.7 | 1.2 | 46.7 | 50.3 | 10 |
| alint | S5 | full | 89.7 | 8.7 | 82.9 | 113.3 | 10 |
| alint | S5 | changed | 51.7 | 8.4 | 48.2 | 75.6 | 10 |
| alint | S6 | full | 107.5 | 5.0 | 103.1 | 117.4 | 10 |
| alint | S6 | changed | 51.4 | 1.0 | 50.4 | 53.2 | 10 |
| alint | S7 | full | 31.1 | 1.3 | 29.3 | 33.3 | 10 |
| alint | S7 | changed | 60.9 | 7.5 | 57.0 | 82.1 | 10 |
| alint | S8 | full | 117.0 | 4.3 | 112.9 | 127.8 | 10 |
| alint | S8 | changed | 77.9 | 13.5 | 72.2 | 116.1 | 10 |
| alint | S9 | full | 74.8 | 15.5 | 68.0 | 118.6 | 10 |
| alint | S9 | changed | 51.1 | 1.2 | 49.8 | 53.4 | 10 |
| alint | S10 | full | 37.6 | 0.8 | 36.4 | 39.1 | 10 |
| alint | S10 | changed | 47.9 | 0.8 | 46.4 | 48.7 | 10 |

Tree shape: monorepo (`packages=200, files_per_package=48, total=10000`).
