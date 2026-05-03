# alint bench-scale results

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
- **S10** — scope_filter on rules outside the PerFileRule path (file_max_size / no_empty_files / no_symlinks / filename_case / filename_regex with has_ancestor narrowing)

## Summary (mean ± stddev, ms)

| Tool | Size | Scenario | Mode | Mean | Stddev | Min | Max | Samples |
|---|---|---|---|---:|---:|---:|---:|---:|
| alint | 1k | S1 | full | 8.2 | 0.7 | 6.9 | 9.4 | 10 |
| alint | 1k | S1 | changed | 20.1 | 0.8 | 18.6 | 20.9 | 10 |
| alint | 1k | S2 | full | 10.8 | 1.3 | 8.9 | 13.1 | 10 |
| alint | 1k | S2 | changed | 21.5 | 0.8 | 20.4 | 23.0 | 10 |
| alint | 1k | S3 | full | 23.4 | 1.0 | 21.8 | 25.0 | 10 |
| alint | 1k | S3 | changed | 28.7 | 0.9 | 27.4 | 30.3 | 10 |
| alint | 1k | S4 | full | 9.4 | 0.9 | 8.2 | 11.5 | 10 |
| alint | 1k | S4 | changed | 25.7 | 14.4 | 20.6 | 66.6 | 10 |
| alint | 1k | S5 | full | 19.0 | 12.2 | 13.9 | 53.6 | 10 |
| alint | 1k | S5 | changed | 20.2 | 0.7 | 19.1 | 21.1 | 10 |
| alint | 1k | S6 | full | 17.2 | 0.7 | 16.3 | 18.5 | 10 |
| alint | 1k | S6 | changed | 20.5 | 0.4 | 20.0 | 21.6 | 10 |
| alint | 1k | S7 | full | 11.3 | 0.6 | 10.4 | 12.0 | 10 |
| alint | 1k | S7 | changed | 22.8 | 1.1 | 22.0 | 25.5 | 10 |
| alint | 1k | S8 | full | 24.2 | 6.9 | 20.7 | 43.6 | 10 |
| alint | 1k | S8 | changed | 29.9 | 8.7 | 26.4 | 54.4 | 10 |
| alint | 1k | S9 | full | 14.7 | 0.6 | 14.0 | 15.6 | 10 |
| alint | 1k | S9 | changed | 22.4 | 1.1 | 20.8 | 24.1 | 10 |
| alint | 1k | S10 | full | 10.6 | 0.7 | 9.3 | 11.9 | 10 |
| alint | 1k | S10 | changed | 20.7 | 1.1 | 19.8 | 23.3 | 10 |
| alint | 10k | S1 | full | 21.1 | 0.9 | 19.9 | 22.2 | 10 |
| alint | 10k | S1 | changed | 47.2 | 0.9 | 46.2 | 49.1 | 10 |
| alint | 10k | S2 | full | 32.4 | 1.0 | 30.5 | 33.9 | 10 |
| alint | 10k | S2 | changed | 49.4 | 0.5 | 48.4 | 50.2 | 10 |
| alint | 10k | S3 | full | 129.8 | 18.7 | 117.3 | 171.0 | 10 |
| alint | 10k | S3 | changed | 84.4 | 14.7 | 78.2 | 126.2 | 10 |
| alint | 10k | S4 | full | 22.5 | 0.8 | 20.8 | 23.9 | 10 |
| alint | 10k | S4 | changed | 121.4 | 58.7 | 46.7 | 208.8 | 10 |
| alint | 10k | S5 | full | 101.5 | 8.1 | 94.4 | 120.6 | 10 |
| alint | 10k | S5 | changed | 57.8 | 12.7 | 52.0 | 93.6 | 10 |
| alint | 10k | S6 | full | 113.1 | 2.2 | 108.0 | 115.8 | 10 |
| alint | 10k | S6 | changed | 53.5 | 0.7 | 52.0 | 54.5 | 10 |
| alint | 10k | S7 | full | 30.9 | 0.7 | 29.7 | 32.1 | 10 |
| alint | 10k | S7 | changed | 85.5 | 38.3 | 58.8 | 154.3 | 10 |
| alint | 10k | S8 | full | 139.7 | 9.1 | 125.7 | 154.9 | 10 |
| alint | 10k | S8 | changed | 87.3 | 14.1 | 76.6 | 112.9 | 10 |
| alint | 10k | S9 | full | 80.1 | 3.7 | 75.6 | 87.9 | 10 |
| alint | 10k | S9 | changed | 58.9 | 12.9 | 51.6 | 86.0 | 10 |
| alint | 10k | S10 | full | 38.9 | 0.9 | 37.7 | 40.8 | 10 |
| alint | 10k | S10 | changed | 50.3 | 1.0 | 48.6 | 51.5 | 10 |
| alint | 100k | S1 | full | 162.6 | 24.5 | 148.5 | 210.6 | 10 |
| alint | 100k | S1 | changed | 413.1 | 11.9 | 389.6 | 428.5 | 10 |
| alint | 100k | S2 | full | 256.5 | 10.9 | 240.8 | 280.9 | 10 |
| alint | 100k | S2 | changed | 412.4 | 9.3 | 403.1 | 436.2 | 10 |
| alint | 100k | S3 | full | 1153.3 | 30.5 | 1119.8 | 1200.9 | 10 |
| alint | 100k | S3 | changed | 614.0 | 7.9 | 602.7 | 627.1 | 10 |
| alint | 100k | S4 | full | 156.3 | 1.9 | 153.7 | 159.8 | 10 |
| alint | 100k | S4 | changed | 411.5 | 14.9 | 394.0 | 432.2 | 10 |
| alint | 100k | S5 | full | 888.0 | 31.7 | 841.1 | 935.5 | 10 |
| alint | 100k | S5 | changed | 767.6 | 204.5 | 552.2 | 1095.2 | 10 |
| alint | 100k | S6 | full | 1103.7 | 50.2 | 1045.3 | 1185.2 | 10 |
| alint | 100k | S6 | changed | 466.6 | 16.0 | 450.9 | 499.4 | 10 |
| alint | 100k | S7 | full | 330.6 | 5.8 | 323.6 | 342.1 | 10 |
| alint | 100k | S7 | changed | 601.1 | 15.9 | 579.8 | 624.8 | 10 |
| alint | 100k | S8 | full | 1068.7 | 24.8 | 1038.4 | 1105.2 | 10 |
| alint | 100k | S8 | changed | 623.3 | 155.9 | 545.2 | 1064.4 | 10 |
| alint | 100k | S9 | full | 686.3 | 5.6 | 680.6 | 699.9 | 10 |
| alint | 100k | S9 | changed | 422.4 | 14.2 | 412.1 | 451.1 | 10 |
| alint | 100k | S10 | full | 342.0 | 16.9 | 328.5 | 383.2 | 10 |
| alint | 100k | S10 | changed | 427.0 | 14.4 | 415.5 | 452.0 | 10 |
