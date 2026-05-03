# alint bench-scale — 10k files

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
| alint | S1 | full | 21.1 | 0.9 | 19.9 | 22.2 | 10 |
| alint | S1 | changed | 47.2 | 0.9 | 46.2 | 49.1 | 10 |
| alint | S2 | full | 32.4 | 1.0 | 30.5 | 33.9 | 10 |
| alint | S2 | changed | 49.4 | 0.5 | 48.4 | 50.2 | 10 |
| alint | S3 | full | 129.8 | 18.7 | 117.3 | 171.0 | 10 |
| alint | S3 | changed | 84.4 | 14.7 | 78.2 | 126.2 | 10 |
| alint | S4 | full | 22.5 | 0.8 | 20.8 | 23.9 | 10 |
| alint | S4 | changed | 121.4 | 58.7 | 46.7 | 208.8 | 10 |
| alint | S5 | full | 101.5 | 8.1 | 94.4 | 120.6 | 10 |
| alint | S5 | changed | 57.8 | 12.7 | 52.0 | 93.6 | 10 |
| alint | S6 | full | 113.1 | 2.2 | 108.0 | 115.8 | 10 |
| alint | S6 | changed | 53.5 | 0.7 | 52.0 | 54.5 | 10 |
| alint | S7 | full | 30.9 | 0.7 | 29.7 | 32.1 | 10 |
| alint | S7 | changed | 85.5 | 38.3 | 58.8 | 154.3 | 10 |
| alint | S8 | full | 139.7 | 9.1 | 125.7 | 154.9 | 10 |
| alint | S8 | changed | 87.3 | 14.1 | 76.6 | 112.9 | 10 |
| alint | S9 | full | 80.1 | 3.7 | 75.6 | 87.9 | 10 |
| alint | S9 | changed | 58.9 | 12.9 | 51.6 | 86.0 | 10 |
| alint | S10 | full | 38.9 | 0.9 | 37.7 | 40.8 | 10 |
| alint | S10 | changed | 50.3 | 1.0 | 48.6 | 51.5 | 10 |

Tree shape: monorepo (`packages=200, files_per_package=48, total=10000`).
