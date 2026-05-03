# v0.9.5 bench-scale capture (re-bench, 2026-05-02)

Comprehensive S1–S8 × {1k, 10k, 100k, 1m} × {full, changed} matrix
captured from the `v0.9.5` tag binary, replacing the original
v0.9.5 publish that only covered `1m × S3 × {full, changed}` (the
prior README documented just that subset — see git history).

The S7 1M cells (623 s changed / 652 s full) surfaced the
cross-file dispatch cliff that v0.9.5's path-index fix did NOT
cover: the fix targeted `for_each_dir` specifically; S7
exercises five other cross-file kinds (`pair`, `unique_by`,
`for_each_file`, `dir_only_contains`, `every_matching_has`) of
which `dir_only_contains` retains its O(D × N) `entries.iter()`
scan per matched dir. v0.9.8 targets these cells directly via
`FileIndex::children_of` — see
[`docs/design/v0.9.8/cross-file-fast-paths-v2.md`](../../../../design/v0.9.8/cross-file-fast-paths-v2.md).

## Original v0.9.5 publish — preserved cells

The original v0.9.5 publish captured only `1m × S3 × {full,changed}`
because the perf headline at the time was the lazy-path-index fix
in `for_each_dir` (1M S3 full: 731.86 s → 11.19 s, 65×). Those
cells re-confirmed in the new run:

| Cell | Original publish | This re-bench | Drift |
|---|---:|---:|---:|
| 1M S3 full | 11.194 s ± 0.154 | 12.59 s ± 1.68 | +12 % (within run-to-run noise on a long-running 1M cell) |
| 1M S3 changed | 6.728 s ± 0.059 | 8.86 s ± 0.57 | +32 % (likely concurrent-system-load drift; the changed-mode wall is shorter so any drift shows larger as a percentage) |

Cross-version comparisons should use the per-cell numbers in
`main/results.json`; the original `1m/` subdir is preserved in
git history but no longer present in the working tree (replaced
by the comprehensive `main/` capture).

## How this run was captured

```sh
xtask bench-scale --include-1m \
    --sizes 1k,10k,100k,1m \
    --scenarios S1,S2,S3,S4,S5,S6,S7 \
    --modes full,changed \
    --tools alint \
    --warmup 3 --runs 10 \
    --json-only \
    --out docs/benchmarks/macro/results/linux-x86_64/v0.9.5/main
xtask bench-scale --include-1m \
    --sizes 1k,10k,100k,1m \
    --scenarios S8 \
    --modes full \
    --tools alint \
    --warmup 3 --runs 10 \
    --json-only \
    --out docs/benchmarks/macro/results/linux-x86_64/v0.9.5/s8full
```

S8 was split into a separate sub-run because v0.9.5's bench
harness had a latent bug in `init_git_for_changed_mode` — when
S8 (the only `requires_git_repo` scenario) and `changed` mode
both appeared in the same matrix, the second commit attempt
failed with "nothing to commit" because `generate_git_monorepo`
had already produced the bench base. Fixed in v0.9.7's
`xtask/src/bench/mod.rs` (the `has_initial_commit` short-circuit).
v0.9.5's published numbers honestly omit S8 changed mode.

S9 didn't exist at v0.9.5 (added v0.9.6).

The bench harness `FileIndex::from_entries` migration in commit
`d96ec2e` (a v0.9.6.x commit) was applied to the v0.9.5 worktree
as a `git`-untracked edit so the micro benches under
`crates/alint-bench/benches/` could compile against v0.9.5's
private `path_set` field — pure plumbing, no engine semantics
changed.

## Hardware fingerprint

`linux-x86_64` (AMD Ryzen 9 3900X 12-core / 62 GB / ext4 / rustc 1.95).
See [`../../METHODOLOGY.md`](../../METHODOLOGY.md) for the cross-version
comparison contract.
