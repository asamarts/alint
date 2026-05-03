# alint bench-scale — 100k files

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
| alint | S1 | full | 151.4 | 15.1 | 142.5 | 189.1 | 10 |
| alint | S1 | changed | 420.0 | 12.1 | 397.3 | 436.3 | 10 |
| alint | S2 | full | 252.4 | 10.9 | 236.6 | 274.7 | 10 |
| alint | S2 | changed | 423.4 | 17.2 | 393.5 | 443.8 | 10 |
| alint | S3 | full | 1130.3 | 24.9 | 1096.5 | 1173.5 | 10 |
| alint | S3 | changed | 611.1 | 14.8 | 594.4 | 643.8 | 10 |
| alint | S4 | full | 161.2 | 15.3 | 147.9 | 193.7 | 10 |
| alint | S4 | changed | 408.4 | 17.7 | 391.1 | 439.7 | 10 |
| alint | S5 | full | 848.4 | 22.5 | 820.2 | 884.4 | 10 |
| alint | S5 | changed | 441.1 | 14.6 | 423.5 | 461.8 | 10 |
| alint | S6 | full | 1015.9 | 30.3 | 972.0 | 1067.5 | 10 |
| alint | S6 | changed | 473.8 | 17.7 | 448.2 | 495.1 | 10 |
| alint | S7 | full | 334.1 | 9.9 | 325.3 | 354.6 | 10 |
| alint | S7 | changed | 609.5 | 16.3 | 574.8 | 631.0 | 10 |
| alint | S8 | full | 1064.8 | 11.5 | 1041.4 | 1076.5 | 10 |
| alint | S8 | changed | 572.8 | 24.8 | 542.0 | 605.9 | 10 |
| alint | S9 | full | 664.3 | 21.0 | 644.9 | 715.2 | 10 |
| alint | S9 | changed | 435.8 | 15.7 | 408.8 | 450.7 | 10 |
| alint | S10 | full | 329.5 | 13.7 | 319.3 | 361.4 | 10 |
| alint | S10 | changed | 438.0 | 12.9 | 415.1 | 453.8 | 10 |

Tree shape: monorepo (`packages=1000, files_per_package=98, total=100000`).
