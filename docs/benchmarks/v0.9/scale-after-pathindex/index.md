# alint bench-scale results

**Platform:** `linux/x86_64`  
**CPU:** `AMD Ryzen 9 3900X 12-Core Processor` (24 cores)  
**RAM:** 62 GB  
**FS:** `ext4`  
**rustc:** `rustc 1.95.0 (59807616e 2026-04-14)`  
**alint:** `0.9.4` (9050745)  
**hyperfine:** `1.20.0`  
**Tools:** alint=`0.9.4`  
**Seed:** `0xa11e47`  
**Warmup/runs:** 1 / 3  
**Generated:** `unix:1777614814`  

Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md`. Compare numbers like-for-like (same fingerprint), never absolutely.

Per-size detail under `<size>/results.md`. JSON: `results.json`.

## Scenarios

- **S3** — Workspace bundle (oss-baseline + rust + monorepo + cargo-workspace)

## Summary (mean ± stddev, ms)

| Tool | Size | Scenario | Mode | Mean | Stddev | Min | Max | Samples |
|---|---|---|---|---:|---:|---:|---:|---:|
| alint | 1m | S3 | full | 11194.0 | 153.6 | 11103.2 | 11371.3 | 3 |
| alint | 1m | S3 | changed | 6728.2 | 59.5 | 6660.2 | 6770.7 | 3 |
