# v0.7.0 baseline (captured 2026-04-29)

Frozen `target/criterion`-format snapshot of the four
publishable micro-benches that existed at v0.7.0:

- `glob_compile`
- `glob_match`
- `regex_content`
- `rule_engine`

The remaining v0.8 benches (`single_file_rules`,
`cross_file_rules`, `output_formats`, `fix_throughput`,
`dsl_extends`, `structured_query`, `blame_cache`) didn't exist at
v0.7.0 by definition — they're the v0.8.4 cut. They have no
v0.7.0 counterpart to compare against; their own first-run
numbers serve as their baseline.

## How v0.9 uses this

The v0.9 engine optimization cut (per-file dispatch flip,
parallel walker, memory-footprint pass) gates regression on
`xtask bench-compare`. Wire it as:

```sh
xtask bench-compare \
  --before docs/benchmarks/v0.8/baseline-v0.7.0/criterion \
  --after target/criterion \
  --threshold 10
```

Any micro-bench that regresses past 10% relative to v0.7.0
fails the gate. v0.9 PRs that intentionally trade single-rule
throughput for cross-rule throughput should bump the threshold
on the affected bench rather than blanket-disable the gate.

## How this baseline was captured

```sh
git worktree add /tmp/alint-v0.7.0 v0.7.0
cd /tmp/alint-v0.7.0
CARGO_TARGET_DIR=target-baseline \
CRITERION_HOME=$PWD/criterion-results \
  cargo bench -p alint-bench \
    --bench rule_engine --bench glob_match \
    --bench glob_compile --bench regex_content
cp -r criterion-results <repo>/docs/benchmarks/v0.8/baseline-v0.7.0/criterion
git worktree remove /tmp/alint-v0.7.0
```

Captured on the same self-hosted Linux runner (Linux 6.1, x86_64)
the main CI lane runs on. **Cross-machine comparisons are not
meaningful** — `bench-compare` should always run with both sides
captured on the same hardware. Re-capture this baseline if the
runner hardware ever changes.

The snapshot reports `mean.point_estimate` in nanoseconds at the
sample sizes criterion's default config exercises (100 samples
per case, 3-second warmup). The bench-compare tool reads
`mean.point_estimate` as the gate input.
