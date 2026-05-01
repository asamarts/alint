# alint bench-scale — 1m files

**Platform:** `linux/x86_64`  
**CPU:** `AMD Ryzen 9 3900X 12-Core Processor` (24 cores)  
**RAM:** 62 GB  
**FS:** `ext4`  
**rustc:** `rustc 1.95.0 (59807616e 2026-04-14)`  
**alint:** `0.9.4` (90c4efa)  
**hyperfine:** `1.20.0`  
**Tools:** alint=`0.9.4`  
**Seed:** `0xa11e47`  
**Warmup/runs:** 3 / 10  
**Generated:** `unix:1777597105`  

Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md`. Compare numbers like-for-like (same fingerprint), never absolutely.

## Rows

| Tool | Scenario | Mode | Mean (ms) | Stddev | Min | Max | Samples |
|---|---|---|---:|---:|---:|---:|---:|
| alint | S1 | full | 1533.6 | 31.6 | 1514.8 | 1570.2 | 3 |
| alint | S1 | changed | 4181.9 | 40.7 | 4141.6 | 4223.0 | 3 |
| alint | S2 | full | 2360.0 | 126.8 | 2232.5 | 2486.0 | 3 |
| alint | S2 | changed | 4289.5 | 36.7 | 4247.4 | 4314.2 | 3 |
| alint | S3 | full | 731856.2 | 5348.9 | 726818.5 | 737469.6 | 3 |
| alint | S3 | changed | 724362.3 | 2132.4 | 722705.0 | 726768.1 | 3 |

Tree shape: monorepo (`packages=5000, files_per_package=198, total=1000000`).
