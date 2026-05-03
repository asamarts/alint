# alint bench-scale — 1m files

**Platform:** `linux/x86_64`  
**CPU:** `AMD Ryzen 9 3900X 12-Core Processor` (24 cores)  
**RAM:** 62 GB  
**FS:** `ext4`  
**rustc:** `rustc 1.95.0 (59807616e 2026-04-14)`  
**alint:** `0.9.10` (839bb77)  
**hyperfine:** `1.20.0`  
**Tools:** alint=`0.9.10`  
**Seed:** `0xa11e47`  
**Warmup/runs:** 1 / 3  
**Generated:** `unix:1777830842`  

Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md`. Compare numbers like-for-like (same fingerprint), never absolutely.

## Rows

| Tool | Scenario | Mode | Mean (ms) | Stddev | Min | Max | Samples |
|---|---|---|---:|---:|---:|---:|---:|
| alint | S1 | full | 1566.8 | 25.4 | 1552.1 | 1596.1 | 3 |
| alint | S1 | changed | 4176.6 | 24.4 | 4153.7 | 4202.3 | 3 |
| alint | S2 | full | 2856.4 | 33.5 | 2819.9 | 2885.7 | 3 |
| alint | S2 | changed | 4198.2 | 31.0 | 4172.1 | 4232.4 | 3 |
| alint | S3 | full | 11619.0 | 383.9 | 11311.9 | 12049.4 | 3 |
| alint | S3 | changed | 6508.6 | 17.8 | 6492.2 | 6527.5 | 3 |
| alint | S4 | full | 1596.0 | 20.5 | 1572.4 | 1609.2 | 3 |
| alint | S4 | changed | 4189.9 | 42.3 | 4141.4 | 4219.7 | 3 |
| alint | S5 | full | 8645.5 | 235.6 | 8501.9 | 8917.4 | 3 |
| alint | S5 | changed | 4473.7 | 33.5 | 4439.6 | 4506.5 | 3 |
| alint | S6 | full | 11219.9 | 496.7 | 10701.5 | 11691.6 | 3 |
| alint | S6 | changed | 4789.9 | 34.9 | 4754.6 | 4824.4 | 3 |
| alint | S7 | full | 15502.9 | 140.9 | 15340.2 | 15586.0 | 3 |
| alint | S7 | changed | 18209.6 | 141.8 | 18048.0 | 18313.5 | 3 |
| alint | S8 | full | 11499.7 | 96.0 | 11427.7 | 11608.7 | 3 |
| alint | S8 | changed | 6245.3 | 39.2 | 6201.4 | 6276.4 | 3 |
| alint | S9 | full | 7213.5 | 27.1 | 7185.0 | 7239.0 | 3 |
| alint | S9 | changed | 4373.1 | 21.9 | 4348.6 | 4390.7 | 3 |
| alint | S10 | full | 3629.3 | 9.8 | 3620.5 | 3639.9 | 3 |
| alint | S10 | changed | 4293.8 | 63.1 | 4255.6 | 4366.6 | 3 |

Tree shape: monorepo (`packages=5000, files_per_package=198, total=1000000`).
