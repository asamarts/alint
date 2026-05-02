# alint bench-scale results

**Platform:** `linux/x86_64`  
**CPU:** `AMD Ryzen 9 3900X 12-Core Processor` (24 cores)  
**RAM:** 62 GB  
**FS:** `ext4`  
**rustc:** `rustc 1.95.0 (59807616e 2026-04-14)`  
**alint:** `0.9.6` (cc6465b)  
**hyperfine:** `1.20.0`  
**Tools:** alint=`0.9.6`  
**Seed:** `0xa11e47`  
**Warmup/runs:** 2 / 5  
**Generated:** `unix:1777697945`  

Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md`. Compare numbers like-for-like (same fingerprint), never absolutely.

Per-size detail under `<size>/results.md`. JSON: `results.json`.

## Scenarios

- **S1** — Filename hygiene (8 rules)
- **S2** — Existence + content (8 rules)
- **S3** — Workspace bundle (oss-baseline + rust + monorepo + cargo-workspace)
- **S4** — Agent-era hygiene (5 rules: backup/scratch/debug/affirmation/model-TODO)
- **S5** — Fix-pass throughput (4 content-editing fix ops)
- **S6** — Per-file content fan-out (13 content rules over `**/*.rs`)
- **S7** — Cross-file relational (pair / unique_by / for_each_dir / for_each_file / dir_only_contains / every_matching_has)
- **S8** — Git-tracked overlay (S3 + git_no_denied_paths + git_tracked_only over a real git repo)
- **S9** — Nested polyglot monorepo (rust + node + python rulesets over crates/ + packages/ + apps/)

## Summary (mean ± stddev, ms)

| Tool | Size | Scenario | Mode | Mean | Stddev | Min | Max | Samples |
|---|---|---|---|---:|---:|---:|---:|---:|
| alint | 10k | S1 | full | 20.9 | 0.5 | 20.1 | 21.5 | 0 |
| alint | 10k | S2 | full | 32.1 | 1.2 | 30.0 | 32.9 | 0 |
| alint | 10k | S3 | full | 125.1 | 11.4 | 118.7 | 145.4 | 0 |
| alint | 10k | S4 | full | 23.1 | 0.8 | 22.1 | 23.9 | 0 |
| alint | 10k | S5 | full | 92.2 | 2.6 | 89.4 | 95.5 | 0 |
| alint | 10k | S6 | full | 119.3 | 4.6 | 113.5 | 124.1 | 0 |
| alint | 10k | S7 | full | 206.1 | 4.5 | 200.8 | 212.2 | 0 |
| alint | 10k | S8 | full | 115.4 | 4.4 | 111.1 | 122.7 | 0 |
| alint | 10k | S9 | full | 73.6 | 1.4 | 72.4 | 76.0 | 0 |
| alint | 100k | S1 | full | 159.6 | 6.4 | 152.5 | 168.6 | 0 |
| alint | 100k | S2 | full | 250.0 | 10.9 | 234.3 | 261.1 | 0 |
| alint | 100k | S3 | full | 1135.3 | 23.5 | 1094.3 | 1153.9 | 0 |
| alint | 100k | S4 | full | 155.6 | 1.3 | 154.3 | 157.5 | 0 |
| alint | 100k | S5 | full | 917.2 | 17.3 | 894.5 | 942.8 | 0 |
| alint | 100k | S6 | full | 1221.3 | 23.7 | 1197.8 | 1252.5 | 0 |
| alint | 100k | S7 | full | 10785.3 | 1176.0 | 9742.4 | 13273.8 | 0 |
| alint | 100k | S8 | full | 1071.9 | 16.9 | 1052.7 | 1097.5 | 0 |
| alint | 100k | S9 | full | 738.6 | 31.5 | 706.3 | 789.8 | 0 |
