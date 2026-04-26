# alint bench-scale — 100k files

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
| alint | S1 | full | 365.7 | 21.0 | 352.6 | 424.2 | 10 |
| alint | S1 | changed | 623.0 | 8.7 | 605.9 | 637.1 | 10 |
| ls-lint | S1 | full | 27.4 | 0.4 | 27.1 | 28.4 | 10 |
| grep | S1 | full | 1412.4 | 10.7 | 1395.2 | 1429.3 | 10 |
| alint | S2 | full | 965.4 | 9.3 | 949.7 | 976.8 | 10 |
| alint | S2 | changed | 685.2 | 12.5 | 659.8 | 699.7 | 10 |
| grep | S2 | full | 926.7 | 8.3 | 913.7 | 940.0 | 10 |
| repolinter | S2 | full | 13761.1 | 108.4 | 13616.8 | 13981.0 | 10 |
| alint | S3 | full | 11391.9 | 73.4 | 11293.4 | 11525.6 | 10 |
| alint | S3 | changed | 11063.4 | 74.9 | 10963.7 | 11181.4 | 10 |

Tree shape: monorepo (`packages=1000, files_per_package=98, total=100000`).
