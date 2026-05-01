# Pre-v0.9.4 baseline (captured 2026-04-30)

Frozen `target/criterion`-format snapshot of every bench
v0.9.4's content-rule migration can plausibly affect,
captured on commit `bf7a415` (the v0.9.3 release commit) —
i.e., the 8-rule reference migration shipped, with the
remaining ~22 per-file content rules still on the rule-major
path.

For the per-phase delta (the right comparison: same
hardware, same day, only v0.9.4's content-rule migrations
differ):

```sh
xtask bench-compare \
  --before docs/benchmarks/v0.9/baseline-v0.9.3/criterion \
  --after  docs/benchmarks/v0.9/v0.9.4-content-rules/criterion \
  --threshold 10
```

For the gate against the v0.7.0 floor:

```sh
xtask bench-compare \
  --before docs/benchmarks/v0.8/baseline-v0.7.0/criterion \
  --after  docs/benchmarks/v0.9/v0.9.4-content-rules/criterion \
  --threshold 10
```

## Methodology

Captured on the same self-hosted Linux runner (Linux 6.1,
x86_64, 8-core) the main CI lane runs on. Ran via:

```sh
CARGO_TARGET_DIR=target-baseline-v0.9.4 \
  cargo bench -p alint-bench --features fs-benches \
    --bench walker --bench rule_engine \
    --bench glob_compile --bench glob_match --bench regex_content \
    --bench single_file_rules --bench cross_file_rules --bench output_formats
cp -r crates/alint-bench/target-baseline-v0.9.4/criterion \
  docs/benchmarks/v0.9/baseline-v0.9.3/criterion
```
