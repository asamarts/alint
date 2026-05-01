# alint bench-scale results

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

Per-size detail under `<size>/results.md`. JSON: `results.json`.

## Scenarios

- **S1** — Filename hygiene (8 rules)
- **S2** — Existence + content (8 rules)
- **S3** — Workspace bundle (oss-baseline + rust + monorepo + cargo-workspace)

## Summary (mean ± stddev, ms)

| Tool | Size | Scenario | Mode | Mean | Stddev | Min | Max | Samples |
|---|---|---|---|---:|---:|---:|---:|---:|
| alint | 1k | S1 | full | 7.8 | 0.5 | 6.9 | 8.8 | 10 |
| alint | 1k | S1 | changed | 13.4 | 1.1 | 12.4 | 15.9 | 10 |
| alint | 1k | S2 | full | 10.4 | 1.0 | 9.3 | 11.8 | 10 |
| alint | 1k | S2 | changed | 13.6 | 0.9 | 12.2 | 14.9 | 10 |
| alint | 1k | S3 | full | 27.8 | 1.4 | 26.0 | 31.2 | 10 |
| alint | 1k | S3 | changed | 26.4 | 1.0 | 25.3 | 28.5 | 10 |
| alint | 10k | S1 | full | 20.3 | 1.0 | 18.7 | 21.6 | 10 |
| alint | 10k | S1 | changed | 46.2 | 0.8 | 44.8 | 47.5 | 10 |
| alint | 10k | S2 | full | 29.9 | 0.8 | 28.4 | 31.5 | 10 |
| alint | 10k | S2 | changed | 48.5 | 0.9 | 47.3 | 49.8 | 10 |
| alint | 10k | S3 | full | 316.4 | 9.0 | 305.8 | 333.3 | 10 |
| alint | 10k | S3 | changed | 276.3 | 2.6 | 272.8 | 281.4 | 10 |
| alint | 100k | S1 | full | 154.2 | 13.9 | 144.4 | 184.9 | 10 |
| alint | 100k | S1 | changed | 419.7 | 15.8 | 387.6 | 436.1 | 10 |
| alint | 100k | S2 | full | 236.7 | 11.6 | 212.9 | 250.6 | 10 |
| alint | 100k | S2 | changed | 423.4 | 15.1 | 402.6 | 447.9 | 10 |
| alint | 100k | S3 | full | 11200.6 | 131.1 | 10913.9 | 11368.9 | 10 |
| alint | 100k | S3 | changed | 10945.0 | 260.9 | 10707.5 | 11479.0 | 10 |
| alint | 1m | S1 | full | 1533.6 | 31.6 | 1514.8 | 1570.2 | 3 |
| alint | 1m | S1 | changed | 4181.9 | 40.7 | 4141.6 | 4223.0 | 3 |
| alint | 1m | S2 | full | 2360.0 | 126.8 | 2232.5 | 2486.0 | 3 |
| alint | 1m | S2 | changed | 4289.5 | 36.7 | 4247.4 | 4314.2 | 3 |
| alint | 1m | S3 | full | 731856.2 | 5348.9 | 726818.5 | 737469.6 | 3 |
| alint | 1m | S3 | changed | 724362.3 | 2132.4 | 722705.0 | 726768.1 | 3 |
