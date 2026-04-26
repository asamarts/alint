# alint bench-scale — 10k files

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
| alint | S1 | full | 43.1 | 0.7 | 41.6 | 43.6 | 10 |
| alint | S1 | changed | 72.8 | 6.3 | 69.6 | 86.1 | 10 |
| ls-lint | S1 | full | 28.2 | 0.9 | 26.5 | 29.6 | 10 |
| grep | S1 | full | 194.5 | 2.6 | 191.3 | 200.9 | 10 |
| alint | S2 | full | 103.6 | 1.3 | 101.5 | 105.8 | 10 |
| alint | S2 | changed | 83.0 | 14.8 | 74.0 | 112.8 | 10 |
| grep | S2 | full | 133.1 | 11.5 | 127.2 | 165.1 | 10 |
| repolinter | S2 | full | 1637.9 | 37.0 | 1592.8 | 1700.8 | 10 |
| alint | S3 | full | 355.9 | 9.8 | 343.3 | 374.1 | 10 |
| alint | S3 | changed | 324.3 | 4.3 | 318.3 | 331.1 | 10 |

Tree shape: monorepo (`packages=200, files_per_package=48, total=10000`).
