# alint bench-scale — 100k files

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
| alint | S1 | full | 162.6 | 24.5 | 148.5 | 210.6 | 10 |
| alint | S1 | changed | 413.1 | 11.9 | 389.6 | 428.5 | 10 |
| alint | S2 | full | 256.5 | 10.9 | 240.8 | 280.9 | 10 |
| alint | S2 | changed | 412.4 | 9.3 | 403.1 | 436.2 | 10 |
| alint | S3 | full | 1153.3 | 30.5 | 1119.8 | 1200.9 | 10 |
| alint | S3 | changed | 614.0 | 7.9 | 602.7 | 627.1 | 10 |
| alint | S4 | full | 156.3 | 1.9 | 153.7 | 159.8 | 10 |
| alint | S4 | changed | 411.5 | 14.9 | 394.0 | 432.2 | 10 |
| alint | S5 | full | 888.0 | 31.7 | 841.1 | 935.5 | 10 |
| alint | S5 | changed | 767.6 | 204.5 | 552.2 | 1095.2 | 10 |
| alint | S6 | full | 1103.7 | 50.2 | 1045.3 | 1185.2 | 10 |
| alint | S6 | changed | 466.6 | 16.0 | 450.9 | 499.4 | 10 |
| alint | S7 | full | 330.6 | 5.8 | 323.6 | 342.1 | 10 |
| alint | S7 | changed | 601.1 | 15.9 | 579.8 | 624.8 | 10 |
| alint | S8 | full | 1068.7 | 24.8 | 1038.4 | 1105.2 | 10 |
| alint | S8 | changed | 623.3 | 155.9 | 545.2 | 1064.4 | 10 |
| alint | S9 | full | 686.3 | 5.6 | 680.6 | 699.9 | 10 |
| alint | S9 | changed | 422.4 | 14.2 | 412.1 | 451.1 | 10 |
| alint | S10 | full | 342.0 | 16.9 | 328.5 | 383.2 | 10 |
| alint | S10 | changed | 427.0 | 14.4 | 415.5 | 452.0 | 10 |

Tree shape: monorepo (`packages=1000, files_per_package=98, total=100000`).
