# alint bench-scale — 1k files

**Platform:** `linux/x86_64`  
**CPU:** `AMD Ryzen 9 3900X 12-Core Processor` (24 cores)  
**RAM:** 62 GB  
**FS:** `ext4`  
**rustc:** `rustc 1.95.0 (59807616e 2026-04-14)`  
**alint:** `0.5.7` (7ff4ed5)  
**hyperfine:** `1.20.0`  
**Tools:** alint=`0.5.7`, grep=`ripgrep 15.1.0`, ls-lint=`ls-lint v2.2.3`, repolinter=`0.11.2`  
**Seed:** `0xa11e47`  
**Warmup/runs:** 3 / 10  
**Generated:** `unix:1777233844`  

Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md`. Compare numbers like-for-like (same fingerprint), never absolutely.

## Rows

| Tool | Scenario | Mode | Mean (ms) | Stddev | Min | Max | Samples |
|---|---|---|---:|---:|---:|---:|---:|
| alint | S1 | full | 8.9 | 0.2 | 8.6 | 9.4 | 10 |
| alint | S1 | changed | 13.3 | 0.3 | 12.9 | 14.0 | 10 |
| ls-lint | S1 | full | 27.9 | 0.7 | 27.1 | 29.8 | 10 |
| grep | S1 | full | 58.4 | 0.8 | 57.5 | 60.1 | 10 |
| alint | S2 | full | 14.8 | 0.2 | 14.4 | 15.3 | 10 |
| alint | S2 | changed | 14.7 | 0.3 | 14.4 | 15.3 | 10 |
| grep | S2 | full | 42.5 | 1.0 | 40.8 | 43.8 | 10 |
| repolinter | S2 | full | 486.5 | 25.3 | 444.5 | 532.0 | 10 |
| alint | S3 | full | 28.6 | 1.1 | 27.4 | 31.3 | 10 |
| alint | S3 | changed | 27.6 | 0.5 | 27.2 | 28.9 | 10 |

Tree shape: monorepo (`packages=50, files_per_package=18, total=1000`).
