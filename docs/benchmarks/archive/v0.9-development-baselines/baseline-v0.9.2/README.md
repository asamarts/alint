# Pre-v0.9.3 baseline (captured 2026-04-30)

Frozen `target/criterion`-format snapshot of every bench
v0.9.3's dispatch flip can plausibly affect, captured on
commit `0744aa7` (the v0.9.2 release commit) — i.e., the
v0.9.2 type-pass code with no engine restructure or rule
migrations yet applied.

Bench groups:
- `glob_compile`, `glob_match`, `regex_content` *(v0.7.0 baseline; gate target — must stay flat; dispatch flip doesn't touch glob/regex paths)*
- `rule_engine` *(v0.7.0 baseline; gate target — should improve modestly when scenarios pack multiple per-file rules sharing a scope)*
- `walker` *(v0.8.4 bench; expected flat — dispatch flip doesn't touch walker)*
- `single_file_*` *(v0.8.4; the read-coalescing win surfaces here when scenarios use multiple rules per file)*
- `cross_file_*` *(v0.8.4; expected flat — these rules opt out of `as_per_file` and stay on the rule-major path)*
- `output_formats_*` *(v0.8.4; expected flat — formatters unchanged)*

## v0.9.3 deltas

For the per-phase delta (the right comparison: same hardware,
same day, only v0.9.3 code differs):

```sh
xtask bench-compare \
  --before docs/benchmarks/v0.9/baseline-v0.9.2/criterion \
  --after  docs/benchmarks/v0.9/v0.9.3-dispatch-flip/criterion \
  --threshold 10
```

For the gate against the v0.7.0 floor:

```sh
xtask bench-compare \
  --before docs/benchmarks/v0.8/baseline-v0.7.0/criterion \
  --after  docs/benchmarks/v0.9/v0.9.3-dispatch-flip/criterion \
  --threshold 10
```

## Methodology

Captured on the same self-hosted Linux runner (Linux 6.1,
x86_64, 8-core) the main CI lane runs on. Ran via:

```sh
CARGO_TARGET_DIR=target-baseline-v0.9.3 \
  cargo bench -p alint-bench --features fs-benches \
    --bench walker --bench rule_engine \
    --bench glob_compile --bench glob_match --bench regex_content \
    --bench single_file_rules --bench cross_file_rules --bench output_formats
cp -r crates/alint-bench/target-baseline-v0.9.3/criterion \
  docs/benchmarks/v0.9/baseline-v0.9.2/criterion
```

Same-day-and-hardware baselines are how the v0.9.x phased
flow isolates per-phase deltas — the v0.7.0 floor at
`docs/benchmarks/v0.8/baseline-v0.7.0/` was captured on a
different day and exhibits ~5% drift on nanosecond-scale
benches.
