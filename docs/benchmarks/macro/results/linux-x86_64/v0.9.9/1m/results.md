# alint bench-scale — 1m files

**Platform:** `linux/x86_64`  
**CPU:** `AMD Ryzen 9 3900X 12-Core Processor` (24 cores)  
**RAM:** 62 GB  
**FS:** `ext4`  
**rustc:** `rustc 1.95.0 (59807616e 2026-04-14)`  
**alint:** `0.9.9` (a87d850)  
**hyperfine:** `1.20.0`  
**Tools:** alint=`0.9.9`  
**Seed:** `0xa11e47`  
**Warmup/runs:** 1 / 3  
**Generated:** `unix:1777794418`  

Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md`. Compare numbers like-for-like (same fingerprint), never absolutely.

## Rows

| Tool | Scenario | Mode | Mean (ms) | Stddev | Min | Max | Samples |
|---|---|---|---:|---:|---:|---:|---:|
| alint | S1 | full | 1651.9 | 30.1 | 1627.8 | 1685.6 | 3 |
| alint | S1 | changed | 4345.3 | 16.6 | 4329.0 | 4362.1 | 3 |
| alint | S2 | full | 2957.3 | 65.3 | 2882.2 | 3000.3 | 3 |
| alint | S2 | changed | 4608.8 | 45.1 | 4573.8 | 4659.6 | 3 |
| alint | S3 | full | 13229.1 | 25.6 | 13210.1 | 13258.2 | 3 |
| alint | S3 | changed | 7291.8 | 42.3 | 7257.3 | 7339.0 | 3 |
| alint | S4 | full | 1769.9 | 95.1 | 1712.6 | 1879.7 | 3 |
| alint | S4 | changed | 4573.6 | 107.4 | 4458.5 | 4671.2 | 3 |
| alint | S5 | full | 9760.6 | 216.8 | 9545.2 | 9978.8 | 3 |
| alint | S5 | changed | 4804.3 | 192.8 | 4662.0 | 5023.7 | 3 |
| alint | S6 | full | 11935.9 | 336.6 | 11673.3 | 12315.4 | 3 |
| alint | S6 | changed | 5020.9 | 99.5 | 4953.1 | 5135.1 | 3 |
| alint | S7 | full | 17319.1 | 204.6 | 17086.9 | 17472.9 | 3 |
| alint | S7 | changed | 20246.3 | 325.2 | 19974.6 | 20606.7 | 3 |
| alint | S8 | full | 12376.4 | 222.4 | 12144.7 | 12588.3 | 3 |
| alint | S8 | changed | 6738.5 | 60.9 | 6668.6 | 6780.3 | 3 |
| alint | S9 | full | 7914.5 | 178.5 | 7748.0 | 8102.9 | 3 |
| alint | S9 | changed | 4655.2 | 49.7 | 4598.8 | 4692.5 | 3 |
| alint | S10 | full | 3752.1 | 94.9 | 3669.4 | 3855.7 | 3 |
| alint | S10 | changed | 4623.6 | 131.7 | 4513.1 | 4769.3 | 3 |

Tree shape: monorepo (`packages=5000, files_per_package=198, total=1000000`).
