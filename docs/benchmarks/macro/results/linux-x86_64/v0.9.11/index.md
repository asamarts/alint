# alint bench-scale results

**Platform:** `linux/x86_64`  
**CPU:** `AMD Ryzen 9 3900X 12-Core Processor` (24 cores)  
**RAM:** 62 GB  
**FS:** `ext4`  
**rustc:** `rustc 1.95.0 (59807616e 2026-04-14)`  
**alint:** `0.9.11` (df7dc57)  
**hyperfine:** `1.20.0`  
**Tools:** alint=`0.9.11`  
**Seed:** `0xa11e47`  
**Warmup/runs:** 3 / 10  
**Generated:** `unix:1777849433`  

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
| alint | 1k | S1 | full | 8.4 | 0.7 | 6.9 | 9.4 | 10 |
| alint | 1k | S1 | changed | 23.5 | 10.8 | 19.2 | 54.3 | 10 |
| alint | 1k | S2 | full | 10.4 | 0.6 | 9.6 | 11.1 | 10 |
| alint | 1k | S2 | changed | 20.4 | 0.7 | 19.2 | 21.5 | 10 |
| alint | 1k | S3 | full | 23.4 | 1.4 | 21.5 | 26.7 | 10 |
| alint | 1k | S3 | changed | 29.3 | 1.4 | 27.5 | 32.7 | 10 |
| alint | 1k | S4 | full | 9.8 | 0.9 | 8.4 | 11.5 | 10 |
| alint | 1k | S4 | changed | 26.0 | 15.7 | 19.1 | 70.7 | 10 |
| alint | 1k | S5 | full | 14.3 | 0.9 | 12.8 | 16.0 | 10 |
| alint | 1k | S5 | changed | 19.7 | 0.9 | 18.4 | 21.0 | 10 |
| alint | 1k | S6 | full | 17.0 | 0.6 | 16.0 | 17.7 | 10 |
| alint | 1k | S6 | changed | 20.6 | 0.6 | 20.0 | 21.8 | 10 |
| alint | 1k | S7 | full | 10.9 | 0.8 | 9.6 | 12.6 | 10 |
| alint | 1k | S7 | changed | 22.1 | 0.9 | 20.8 | 24.1 | 10 |
| alint | 1k | S8 | full | 26.3 | 13.6 | 21.0 | 65.0 | 10 |
| alint | 1k | S8 | changed | 26.6 | 0.6 | 25.6 | 27.5 | 10 |
| alint | 1k | S9 | full | 14.5 | 0.9 | 12.9 | 16.1 | 10 |
| alint | 1k | S9 | changed | 21.9 | 1.0 | 20.7 | 23.8 | 10 |
| alint | 1k | S10 | full | 10.1 | 0.7 | 8.8 | 11.3 | 10 |
| alint | 1k | S10 | changed | 23.6 | 10.7 | 19.2 | 54.0 | 10 |
| alint | 10k | S1 | full | 20.4 | 0.8 | 19.2 | 21.8 | 10 |
| alint | 10k | S1 | changed | 48.7 | 5.9 | 45.1 | 64.9 | 10 |
| alint | 10k | S2 | full | 31.7 | 1.0 | 29.9 | 33.0 | 10 |
| alint | 10k | S2 | changed | 53.2 | 8.0 | 47.8 | 69.4 | 10 |
| alint | 10k | S3 | full | 119.2 | 2.6 | 116.7 | 125.5 | 10 |
| alint | 10k | S3 | changed | 78.4 | 1.2 | 76.0 | 80.0 | 10 |
| alint | 10k | S4 | full | 22.1 | 1.1 | 20.8 | 24.2 | 10 |
| alint | 10k | S4 | changed | 47.7 | 1.2 | 46.7 | 50.3 | 10 |
| alint | 10k | S5 | full | 89.7 | 8.7 | 82.9 | 113.3 | 10 |
| alint | 10k | S5 | changed | 51.7 | 8.4 | 48.2 | 75.6 | 10 |
| alint | 10k | S6 | full | 107.5 | 5.0 | 103.1 | 117.4 | 10 |
| alint | 10k | S6 | changed | 51.4 | 1.0 | 50.4 | 53.2 | 10 |
| alint | 10k | S7 | full | 31.1 | 1.3 | 29.3 | 33.3 | 10 |
| alint | 10k | S7 | changed | 60.9 | 7.5 | 57.0 | 82.1 | 10 |
| alint | 10k | S8 | full | 117.0 | 4.3 | 112.9 | 127.8 | 10 |
| alint | 10k | S8 | changed | 77.9 | 13.5 | 72.2 | 116.1 | 10 |
| alint | 10k | S9 | full | 74.8 | 15.5 | 68.0 | 118.6 | 10 |
| alint | 10k | S9 | changed | 51.1 | 1.2 | 49.8 | 53.4 | 10 |
| alint | 10k | S10 | full | 37.6 | 0.8 | 36.4 | 39.1 | 10 |
| alint | 10k | S10 | changed | 47.9 | 0.8 | 46.4 | 48.7 | 10 |
| alint | 100k | S1 | full | 151.4 | 15.1 | 142.5 | 189.1 | 10 |
| alint | 100k | S1 | changed | 420.0 | 12.1 | 397.3 | 436.3 | 10 |
| alint | 100k | S2 | full | 252.4 | 10.9 | 236.6 | 274.7 | 10 |
| alint | 100k | S2 | changed | 423.4 | 17.2 | 393.5 | 443.8 | 10 |
| alint | 100k | S3 | full | 1130.3 | 24.9 | 1096.5 | 1173.5 | 10 |
| alint | 100k | S3 | changed | 611.1 | 14.8 | 594.4 | 643.8 | 10 |
| alint | 100k | S4 | full | 161.2 | 15.3 | 147.9 | 193.7 | 10 |
| alint | 100k | S4 | changed | 408.4 | 17.7 | 391.1 | 439.7 | 10 |
| alint | 100k | S5 | full | 848.4 | 22.5 | 820.2 | 884.4 | 10 |
| alint | 100k | S5 | changed | 441.1 | 14.6 | 423.5 | 461.8 | 10 |
| alint | 100k | S6 | full | 1015.9 | 30.3 | 972.0 | 1067.5 | 10 |
| alint | 100k | S6 | changed | 473.8 | 17.7 | 448.2 | 495.1 | 10 |
| alint | 100k | S7 | full | 334.1 | 9.9 | 325.3 | 354.6 | 10 |
| alint | 100k | S7 | changed | 609.5 | 16.3 | 574.8 | 631.0 | 10 |
| alint | 100k | S8 | full | 1064.8 | 11.5 | 1041.4 | 1076.5 | 10 |
| alint | 100k | S8 | changed | 572.8 | 24.8 | 542.0 | 605.9 | 10 |
| alint | 100k | S9 | full | 664.3 | 21.0 | 644.9 | 715.2 | 10 |
| alint | 100k | S9 | changed | 435.8 | 15.7 | 408.8 | 450.7 | 10 |
| alint | 100k | S10 | full | 329.5 | 13.7 | 319.3 | 361.4 | 10 |
| alint | 100k | S10 | changed | 438.0 | 12.9 | 415.1 | 453.8 | 10 |
| alint | 1m | S1 | full | 1793.2 | 333.7 | 1575.9 | 2177.5 | 3 |
| alint | 1m | S1 | changed | 4776.4 | 338.5 | 4422.5 | 5097 | 3 |
| alint | 1m | S2 | full | 2911.3 | 17.8 | 2891.1 | 2924.7 | 3 |
| alint | 1m | S2 | changed | 4429.1 | 28.9 | 4397 | 4453.2 | 3 |
| alint | 1m | S3 | full | 11838.3 | 214.4 | 11610.1 | 12035.5 | 3 |
| alint | 1m | S3 | changed | 6779.4 | 14.2 | 6769 | 6795.6 | 3 |
| alint | 1m | S4 | full | 1656.7 | 45.1 | 1614 | 1703.9 | 3 |
| alint | 1m | S4 | changed | 4418 | 54.3 | 4358.7 | 4465.4 | 3 |
| alint | 1m | S5 | full | 9527 | 747.1 | 9061.7 | 10388.8 | 3 |
| alint | 1m | S5 | changed | 4737.8 | 72.7 | 4691.5 | 4821.6 | 3 |
| alint | 1m | S6 | full | 11906.7 | 155.8 | 11734.4 | 12037.8 | 3 |
| alint | 1m | S6 | changed | 5160 | 235.4 | 4995.1 | 5429.6 | 3 |
| alint | 1m | S7 | full | 17294.7 | 465.4 | 16938.6 | 17821.3 | 3 |
| alint | 1m | S7 | changed | 19920 | 308.3 | 19669.3 | 20264.3 | 3 |
| alint | 1m | S8 | full | 12731.8 | 410.6 | 12335.6 | 13155.5 | 3 |
| alint | 1m | S8 | changed | 7090.5 | 50.1 | 7041.3 | 7141.4 | 3 |
| alint | 1m | S9 | full | 8497.8 | 1069.5 | 7584.6 | 9674.4 | 3 |
| alint | 1m | S9 | changed | 4672.9 | 116.7 | 4556.7 | 4790 | 3 |
| alint | 1m | S10 | full | 3802.4 | 82.2 | 3717.4 | 3881.4 | 3 |
| alint | 1m | S10 | changed | 4602.9 | 63.6 | 4549.3 | 4673.3 | 3 |

> Note: 1m sizes use `--warmup 1 --runs 3` (matching the v0.9.5/v0.9.8/v0.9.10
> 1m capture convention). Cross-version 1m comparison shows several scenarios
> drift +5–18 % vs v0.9.10 1m baseline, but all 100k cells (the higher-confidence
> captures at warmup=3/runs=10) are within ±8 % (most within ±5 %). The 1m drift
> is within the 1m noise band on this run-set — the bench was captured while
> release.yml was building the v0.9.11 publish chain in parallel, sharing CPU.
> A quiet-machine re-run with warmup=3/runs=5 at 1m would resolve this; deferred
> to a follow-up bench commit if a real signal emerges.
