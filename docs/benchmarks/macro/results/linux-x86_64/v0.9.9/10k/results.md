# alint bench-scale — 10k files

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
| alint | S1 | full | 21.7 | 0.9 | 20.3 | 23.2 | 10 |
| alint | S1 | changed | 51.8 | 4.2 | 48.6 | 61.1 | 10 |
| alint | S2 | full | 41.8 | 13.5 | 30.9 | 71.7 | 10 |
| alint | S2 | changed | 52.9 | 4.9 | 48.2 | 64.5 | 10 |
| alint | S3 | full | 130.3 | 16.5 | 117.8 | 172.9 | 10 |
| alint | S3 | changed | 94.9 | 16.3 | 79.4 | 127.2 | 10 |
| alint | S4 | full | 26.2 | 8.2 | 20.4 | 49.2 | 10 |
| alint | S4 | changed | 55.2 | 19.0 | 47.8 | 109.3 | 10 |
| alint | S5 | full | 97.6 | 9.5 | 87.3 | 116.7 | 10 |
| alint | S5 | changed | 53.1 | 1.3 | 50.7 | 54.7 | 10 |
| alint | S6 | full | 113.2 | 5.6 | 104.9 | 121.8 | 10 |
| alint | S6 | changed | 54.0 | 1.6 | 52.3 | 57.3 | 10 |
| alint | S7 | full | 30.9 | 1.2 | 29.2 | 32.7 | 10 |
| alint | S7 | changed | 58.6 | 1.0 | 56.7 | 60.0 | 10 |
| alint | S8 | full | 116.7 | 1.6 | 114.1 | 118.8 | 10 |
| alint | S8 | changed | 75.0 | 5.9 | 70.7 | 91.0 | 10 |
| alint | S9 | full | 70.1 | 1.0 | 68.5 | 71.7 | 10 |
| alint | S9 | changed | 58.7 | 15.2 | 49.9 | 95.1 | 10 |
| alint | S10 | full | 40.8 | 8.5 | 36.8 | 65.0 | 10 |
| alint | S10 | changed | 49.1 | 1.1 | 47.6 | 51.4 | 10 |

Tree shape: monorepo (`packages=200, files_per_package=48, total=10000`).
