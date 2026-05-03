# v0.9.8 bench-scale capture (2026-05-02)

Full publish-grade S1–S9 × {1k, 10k, 100k, 1m} × {full, changed}
matrix from the v0.9.8 binary. Same shape as v0.9.7's capture
(72 cells); same machine, same `--warmup 3 --runs 10` flags.

v0.9.8 closes the cross-file dispatch cliff that the v0.9.5 fix
didn't fully cover. The 1M S7 cell — stuck at ~614 s across
v0.9.5 / v0.9.6 / v0.9.7 — drops to **15 s on /full and 18 s on
/changed** (40× and 34× speedups). All other 1M cells stay
within ±5 % of v0.9.7 (no regression on the per-file dispatch
fast paths).

## How this run was captured

```sh
xtask bench-scale --include-1m \
    --sizes 1k,10k,100k,1m \
    --scenarios S1,S2,S3,S4,S5,S6,S7,S8,S9 \
    --modes full,changed \
    --tools alint \
    --warmup 3 --runs 10 \
    --json-only \
    --out docs/benchmarks/macro/results/linux-x86_64/v0.9.8/main
```

72 cells captured. The harness fix from v0.9.7 lets S8 run
in `changed` mode in the same matrix as the other scenarios
(no separate `s8full/` sub-run needed).

## Optimization design

Two engine-internal changes drive the v0.9.5 → v0.9.8 delta:

1. **`FileIndex::children_of(dir)`** — lazy direct-children
   index. `dir_only_contains` and `dir_contains` previously
   scanned `ctx.index.entries.iter()` per matched dir
   (O(D × N) = 5 billion ops at 1M / 5K dirs); now they iterate
   only the dir's actual children (~200 entries each).
2. **`evaluate_for_each` literal-path bypass** — when a nested
   rule (under `for_each_dir`, `for_each_file`,
   `every_matching_has`) has a single-literal `paths:` template
   AND opts into `as_per_file()`, dispatch via `evaluate_file`
   directly against the in-index entry instead of running the
   rule's full `evaluate(ctx)` (which would iterate
   `ctx.index.files()` per call). Closes the 484 s
   `every-lib-has-content` cell (S7's `for_each_file ×
   file_min_lines` shape).

Full design doc:
[`docs/design/v0.9.8/cross-file-fast-paths-v2.md`](../../../../design/v0.9.8/cross-file-fast-paths-v2.md).

## Hardware fingerprint

`linux-x86_64` (AMD Ryzen 9 3900X 12-core / 62 GB / ext4 / rustc 1.95).
Same machine as every prior published v0.9.x cell — cross-version
comparison is like-for-like.
