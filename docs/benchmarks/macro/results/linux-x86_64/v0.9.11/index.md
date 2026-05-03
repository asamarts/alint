# alint bench-scale results

**Platform:** `linux/x86_64`  
**CPU:** `AMD Ryzen 9 3900X 12-Core Processor` (24 cores)  
**RAM:** 62 GB  
**FS:** `ext4`  
**rustc:** `rustc 1.95.0 (59807616e 2026-04-14)`  
**alint:** `0.9.10` (8c39208)  
**hyperfine:** `1.20.0`  
**Tools:** alint=`0.9.10`  
**Seed:** `0xa11e47`  
**Warmup/runs:** 3 / 10  
**Generated:** `unix:1777848984`  

Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md`. Compare numbers like-for-like (same fingerprint), never absolutely.

Per-size detail under `<size>/results.md`. JSON: `results.json`.

## Scenarios

- **S8** — Git-tracked overlay (S3 + git_no_denied_paths + git_tracked_only over a real git repo)

## Summary (mean ± stddev, ms)

| Tool | Size | Scenario | Mode | Mean | Stddev | Min | Max | Samples |
|---|---|---|---|---:|---:|---:|---:|---:|
| alint | 1k | S8 | full | 21.9 | 1.3 | 20.3 | 24.9 | 10 |
| alint | 1k | S8 | changed | 20.4 | 1.2 | 19.3 | 23.1 | 10 |
| alint | 10k | S8 | full | 118.0 | 2.5 | 114.6 | 121.7 | 10 |
| alint | 10k | S8 | changed | 74.7 | 1.8 | 72.3 | 78.2 | 10 |
| alint | 100k | S8 | full | 1064.0 | 31.6 | 1029.8 | 1130.1 | 10 |
| alint | 100k | S8 | changed | 579.0 | 23.3 | 542.6 | 612.8 | 10 |
