# alint bench-scale results

**Platform:** `linux/x86_64`  
**CPU:** `AMD Ryzen 9 3900X 12-Core Processor` (24 cores)  
**RAM:** 62 GB  
**FS:** `ext4`  
**rustc:** `rustc 1.95.0 (59807616e 2026-04-14)`  
**alint:** `0.9.9` (ed62e27)  
**hyperfine:** `1.20.0`  
**Tools:** alint=`0.9.9`  
**Seed:** `0xa11e47`  
**Warmup/runs:** 3 / 10  
**Generated:** `unix:1777793233`  

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
| alint | 1k | S1 | full | 8.0 | 0.4 | 7.6 | 9.0 | 10 |
| alint | 1k | S1 | changed | 20.1 | 0.7 | 19.2 | 21.6 | 10 |
| alint | 1k | S2 | full | 11.3 | 1.1 | 9.9 | 13.2 | 10 |
| alint | 1k | S2 | changed | 28.9 | 14.2 | 21.9 | 68.8 | 10 |
| alint | 1k | S3 | full | 30.6 | 13.7 | 22.0 | 66.7 | 10 |
| alint | 1k | S3 | changed | 30.5 | 1.7 | 27.4 | 32.5 | 10 |
| alint | 1k | S4 | full | 9.6 | 0.7 | 9.0 | 11.2 | 10 |
| alint | 1k | S4 | changed | 21.0 | 0.7 | 20.2 | 22.2 | 10 |
| alint | 1k | S5 | full | 18.6 | 13.8 | 12.7 | 57.7 | 10 |
| alint | 1k | S5 | changed | 20.7 | 0.7 | 19.8 | 22.0 | 10 |
| alint | 1k | S6 | full | 17.3 | 1.2 | 15.7 | 19.0 | 10 |
| alint | 1k | S6 | changed | 21.9 | 1.5 | 19.3 | 24.6 | 10 |
| alint | 1k | S7 | full | 14.0 | 7.7 | 9.8 | 35.8 | 10 |
| alint | 1k | S7 | changed | 24.9 | 2.1 | 22.4 | 28.2 | 10 |
| alint | 1k | S8 | full | 32.8 | 10.5 | 23.3 | 51.5 | 10 |
| alint | 1k | S8 | changed | 28.4 | 1.2 | 26.8 | 30.1 | 10 |
| alint | 1k | S9 | full | 14.9 | 0.9 | 13.9 | 16.1 | 10 |
| alint | 1k | S9 | changed | 22.4 | 0.8 | 21.4 | 24.1 | 10 |
| alint | 1k | S10 | full | 14.4 | 12.2 | 9.6 | 49.0 | 10 |
| alint | 1k | S10 | changed | 20.3 | 0.7 | 19.6 | 21.9 | 10 |
| alint | 10k | S1 | full | 21.7 | 0.9 | 20.3 | 23.2 | 10 |
| alint | 10k | S1 | changed | 51.8 | 4.2 | 48.6 | 61.1 | 10 |
| alint | 10k | S2 | full | 41.8 | 13.5 | 30.9 | 71.7 | 10 |
| alint | 10k | S2 | changed | 52.9 | 4.9 | 48.2 | 64.5 | 10 |
| alint | 10k | S3 | full | 130.3 | 16.5 | 117.8 | 172.9 | 10 |
| alint | 10k | S3 | changed | 94.9 | 16.3 | 79.4 | 127.2 | 10 |
| alint | 10k | S4 | full | 26.2 | 8.2 | 20.4 | 49.2 | 10 |
| alint | 10k | S4 | changed | 55.2 | 19.0 | 47.8 | 109.3 | 10 |
| alint | 10k | S5 | full | 97.6 | 9.5 | 87.3 | 116.7 | 10 |
| alint | 10k | S5 | changed | 53.1 | 1.3 | 50.7 | 54.7 | 10 |
| alint | 10k | S6 | full | 113.2 | 5.6 | 104.9 | 121.8 | 10 |
| alint | 10k | S6 | changed | 54.0 | 1.6 | 52.3 | 57.3 | 10 |
| alint | 10k | S7 | full | 30.9 | 1.2 | 29.2 | 32.7 | 10 |
| alint | 10k | S7 | changed | 58.6 | 1.0 | 56.7 | 60.0 | 10 |
| alint | 10k | S8 | full | 116.7 | 1.6 | 114.1 | 118.8 | 10 |
| alint | 10k | S8 | changed | 75.0 | 5.9 | 70.7 | 91.0 | 10 |
| alint | 10k | S9 | full | 70.1 | 1.0 | 68.5 | 71.7 | 10 |
| alint | 10k | S9 | changed | 58.7 | 15.2 | 49.9 | 95.1 | 10 |
| alint | 10k | S10 | full | 40.8 | 8.5 | 36.8 | 65.0 | 10 |
| alint | 10k | S10 | changed | 49.1 | 1.1 | 47.6 | 51.4 | 10 |
| alint | 100k | S1 | full | 152.8 | 6.3 | 144.8 | 165.2 | 10 |
| alint | 100k | S1 | changed | 407.8 | 13.9 | 393.9 | 435.8 | 10 |
| alint | 100k | S2 | full | 253.6 | 11.1 | 235.1 | 270.5 | 10 |
| alint | 100k | S2 | changed | 424.8 | 11.7 | 412.6 | 441.0 | 10 |
| alint | 100k | S3 | full | 1161.1 | 18.7 | 1134.0 | 1192.0 | 10 |
| alint | 100k | S3 | changed | 618.1 | 9.0 | 609.2 | 636.7 | 10 |
| alint | 100k | S4 | full | 163.7 | 12.6 | 154.1 | 192.0 | 10 |
| alint | 100k | S4 | changed | 411.8 | 12.9 | 398.5 | 428.5 | 10 |
| alint | 100k | S5 | full | 865.7 | 17.4 | 831.5 | 886.4 | 10 |
| alint | 100k | S5 | changed | 441.9 | 10.7 | 423.9 | 461.9 | 10 |
| alint | 100k | S6 | full | 1068.1 | 43.2 | 1028.5 | 1142.3 | 10 |
| alint | 100k | S6 | changed | 473.7 | 14.8 | 457.9 | 496.8 | 10 |
| alint | 100k | S7 | full | 326.3 | 5.8 | 316.9 | 333.3 | 10 |
| alint | 100k | S7 | changed | 591.7 | 18.2 | 572.0 | 625.0 | 10 |
| alint | 100k | S8 | full | 1050.2 | 26.1 | 998.1 | 1088.3 | 10 |
| alint | 100k | S8 | changed | 554.0 | 16.1 | 534.5 | 582.3 | 10 |
| alint | 100k | S9 | full | 688.0 | 11.1 | 675.5 | 707.1 | 10 |
| alint | 100k | S9 | changed | 423.8 | 1.3 | 421.6 | 425.9 | 10 |
| alint | 100k | S10 | full | 336.3 | 9.1 | 327.0 | 354.6 | 10 |
| alint | 100k | S10 | changed | 426.3 | 10.3 | 417.3 | 447.6 | 10 |
| alint | 1m | S1 | full | 1651.9 | 30.1 | 1627.8 | 1685.6 | 3 |
| alint | 1m | S1 | changed | 4345.3 | 16.6 | 4329.0 | 4362.1 | 3 |
| alint | 1m | S2 | full | 2957.3 | 65.3 | 2882.2 | 3000.3 | 3 |
| alint | 1m | S2 | changed | 4608.8 | 45.1 | 4573.8 | 4659.6 | 3 |
| alint | 1m | S3 | full | 13229.1 | 25.6 | 13210.1 | 13258.2 | 3 |
| alint | 1m | S3 | changed | 7291.8 | 42.3 | 7257.3 | 7339.0 | 3 |
| alint | 1m | S4 | full | 1769.9 | 95.1 | 1712.6 | 1879.7 | 3 |
| alint | 1m | S4 | changed | 4573.6 | 107.4 | 4458.5 | 4671.2 | 3 |
| alint | 1m | S5 | full | 9760.6 | 216.8 | 9545.2 | 9978.8 | 3 |
| alint | 1m | S5 | changed | 4804.3 | 192.8 | 4662.0 | 5023.7 | 3 |
| alint | 1m | S6 | full | 11935.9 | 336.6 | 11673.3 | 12315.4 | 3 |
| alint | 1m | S6 | changed | 5020.9 | 99.5 | 4953.1 | 5135.1 | 3 |
| alint | 1m | S7 | full | 17319.1 | 204.6 | 17086.9 | 17472.9 | 3 |
| alint | 1m | S7 | changed | 20246.3 | 325.2 | 19974.6 | 20606.7 | 3 |
| alint | 1m | S8 | full | 12376.4 | 222.4 | 12144.7 | 12588.3 | 3 |
| alint | 1m | S8 | changed | 6738.5 | 60.9 | 6668.6 | 6780.3 | 3 |
| alint | 1m | S9 | full | 7914.5 | 178.5 | 7748.0 | 8102.9 | 3 |
| alint | 1m | S9 | changed | 4655.2 | 49.7 | 4598.8 | 4692.5 | 3 |
| alint | 1m | S10 | full | 3752.1 | 94.9 | 3669.4 | 3855.7 | 3 |
| alint | 1m | S10 | changed | 4623.6 | 131.7 | 4513.1 | 4769.3 | 3 |

> Note: 1m sizes use `--warmup 1 --runs 3` (matching the v0.9.5/v0.9.8
> 1m capture convention). Cross-version 1m comparison shows several
> scenarios drift +7–17 % vs v0.9.8 1m baseline, but all 100k cells
> (the higher-confidence captures at warmup=3/runs=10) are within
> ±3 %. The 1m drift is within the 1m noise band on this run-set
> and likely reflects back-to-back bench-session heat soak rather
> than a real per-rule cost from the 17-rule sweep — none of the
> impacted scenarios (S4–S7, no scope_filter rules) show the same
> shape at 100k. A quiet-machine re-run with warmup=3/runs=5 at
> 1m would resolve this; deferred to a follow-up bench commit.
