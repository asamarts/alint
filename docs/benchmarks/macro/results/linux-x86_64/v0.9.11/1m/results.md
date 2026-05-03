# alint bench-scale — 1m files

**Platform:** `linux/x86_64`  
**CPU:** `AMD Ryzen 9 3900X 12-Core Processor` (24 cores)  
**RAM:** 62 GB  
**FS:** `ext4`  
**rustc:** `rustc 1.95.0 (59807616e 2026-04-14)`  
**alint:** `0.9.11` (fb3a755)  
**hyperfine:** `1.20.0`  
**Tools:** alint=`0.9.11`  
**Seed:** `0xa11e47`  
**Warmup/runs:** 1 / 3  
**Generated:** `unix:1777850579`  

Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md`. Compare numbers like-for-like (same fingerprint), never absolutely.

## Rows

| Tool | Scenario | Mode | Mean (ms) | Stddev | Min | Max | Samples |
|---|---|---|---:|---:|---:|---:|---:|
| alint | S1 | full | 1793.2 | 333.7 | 1575.9 | 2177.5 | 3 |
| alint | S1 | changed | 4776.4 | 338.5 | 4422.5 | 5097.0 | 3 |
| alint | S2 | full | 2911.3 | 17.8 | 2891.1 | 2924.7 | 3 |
| alint | S2 | changed | 4429.1 | 28.9 | 4397.0 | 4453.2 | 3 |
| alint | S3 | full | 11838.3 | 214.4 | 11610.1 | 12035.5 | 3 |
| alint | S3 | changed | 6779.4 | 14.2 | 6769.0 | 6795.6 | 3 |
| alint | S4 | full | 1656.7 | 45.1 | 1614.0 | 1703.9 | 3 |
| alint | S4 | changed | 4418.0 | 54.3 | 4358.7 | 4465.4 | 3 |
| alint | S5 | full | 9527.0 | 747.1 | 9061.7 | 10388.8 | 3 |
| alint | S5 | changed | 4737.8 | 72.7 | 4691.5 | 4821.6 | 3 |
| alint | S6 | full | 11906.7 | 155.8 | 11734.4 | 12037.8 | 3 |
| alint | S6 | changed | 5160.0 | 235.4 | 4995.1 | 5429.6 | 3 |
| alint | S7 | full | 17294.7 | 465.4 | 16938.6 | 17821.3 | 3 |
| alint | S7 | changed | 19920.0 | 308.3 | 19669.3 | 20264.3 | 3 |
| alint | S8 | full | 12731.8 | 410.6 | 12335.6 | 13155.5 | 3 |
| alint | S8 | changed | 7090.5 | 50.1 | 7041.3 | 7141.4 | 3 |
| alint | S9 | full | 8497.8 | 1069.5 | 7584.6 | 9674.4 | 3 |
| alint | S9 | changed | 4672.9 | 116.7 | 4556.7 | 4790.0 | 3 |
| alint | S10 | full | 3802.4 | 82.2 | 3717.4 | 3881.4 | 3 |
| alint | S10 | changed | 4602.9 | 63.6 | 4549.3 | 4673.3 | 3 |

Tree shape: monorepo (`packages=5000, files_per_package=198, total=1000000`).
