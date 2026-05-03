# alint bench-scale — 100k files

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
| alint | S1 | full | 152.8 | 6.3 | 144.8 | 165.2 | 10 |
| alint | S1 | changed | 407.8 | 13.9 | 393.9 | 435.8 | 10 |
| alint | S2 | full | 253.6 | 11.1 | 235.1 | 270.5 | 10 |
| alint | S2 | changed | 424.8 | 11.7 | 412.6 | 441.0 | 10 |
| alint | S3 | full | 1161.1 | 18.7 | 1134.0 | 1192.0 | 10 |
| alint | S3 | changed | 618.1 | 9.0 | 609.2 | 636.7 | 10 |
| alint | S4 | full | 163.7 | 12.6 | 154.1 | 192.0 | 10 |
| alint | S4 | changed | 411.8 | 12.9 | 398.5 | 428.5 | 10 |
| alint | S5 | full | 865.7 | 17.4 | 831.5 | 886.4 | 10 |
| alint | S5 | changed | 441.9 | 10.7 | 423.9 | 461.9 | 10 |
| alint | S6 | full | 1068.1 | 43.2 | 1028.5 | 1142.3 | 10 |
| alint | S6 | changed | 473.7 | 14.8 | 457.9 | 496.8 | 10 |
| alint | S7 | full | 326.3 | 5.8 | 316.9 | 333.3 | 10 |
| alint | S7 | changed | 591.7 | 18.2 | 572.0 | 625.0 | 10 |
| alint | S8 | full | 1050.2 | 26.1 | 998.1 | 1088.3 | 10 |
| alint | S8 | changed | 554.0 | 16.1 | 534.5 | 582.3 | 10 |
| alint | S9 | full | 688.0 | 11.1 | 675.5 | 707.1 | 10 |
| alint | S9 | changed | 423.8 | 1.3 | 421.6 | 425.9 | 10 |
| alint | S10 | full | 336.3 | 9.1 | 327.0 | 354.6 | 10 |
| alint | S10 | changed | 426.3 | 10.3 | 417.3 | 447.6 | 10 |

Tree shape: monorepo (`packages=1000, files_per_package=98, total=100000`).
