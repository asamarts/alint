# v0.9.2 — memory pass (type-only) (captured 2026-04-30)

Frozen `target/criterion`-format snapshot of every bench
v0.9.2's type-pass touches. Captured on the
v0.9.2-memory-pass branch immediately before merge.

Bench groups:
- `glob_compile`, `glob_match`, `regex_content` *(v0.7.0 baseline; gate target)*
- `rule_engine` *(v0.7.0 baseline; gate target — Arc<Path> savings on the violation hot path land here)*
- `walker` *(v0.8.4 bench; expected flat — type pass doesn't touch walker logic)*
- `single_file_*`, `cross_file_*`, `output_formats_*` *(v0.8.4)*

## v0.9.2 deltas

### vs v0.7.0 baseline (the gate)

```sh
xtask bench-compare \
  --before docs/benchmarks/v0.8/baseline-v0.7.0/criterion \
  --after  docs/benchmarks/v0.9/v0.9.2-memory-pass/criterion \
  --threshold 10
```

| bench | before | after | delta |
|---|---:|---:|---:|
| `regex_content/1024` |    78 ns |    73 ns | -5.40% |
| `glob_match/1000` | 102.56 µs | 107.32 µs | +4.64% |
| `rule_engine/100000` |  15.62 ms |  14.98 ms | **-4.08%** |
| `regex_content/65536` |    77 ns |    74 ns | -3.53% |
| `glob_compile/10` | 974.06 µs | 943.41 µs | -2.76% |
| `glob_compile/1000` | 114.98 ms | 111.80 ms | -2.76% |
| `regex_content/1048576` |    78 ns |    77 ns | -1.77% |
| `rule_engine/10000` |   1.65 ms |   1.67 ms | +1.47% |
| `glob_match/10000` |   1.06 ms |   1.08 ms | +1.37% |
| `glob_match/100000` |  10.94 ms |  10.87 ms | -0.66% |
| `rule_engine/1000` | 155.86 µs | 155.26 µs | -0.39% |
| `glob_compile/100` |  10.34 ms |  10.32 ms | -0.22% |

All within ±5.4%. Gate passes. The `rule_engine/100000`
improvement (-4.08%) is the Arc<Path> + Arc<str> savings
on the violation hot path materialising on the largest
bench scenario where there are enough violations for the
saved clones to register against the constant overhead.
At smaller scenarios (1k / 10k) the wins are within noise
because there aren't enough violations for the Arc savings
to dominate.

### vs pre-v0.9.2 baseline (the per-phase delta)

```sh
xtask bench-compare \
  --before docs/benchmarks/v0.9/baseline-v0.9.1/criterion \
  --after  docs/benchmarks/v0.9/v0.9.2-memory-pass/criterion \
  --threshold 10
```

The v0.9.2 type-pass-only delta — same hardware, same day,
only the Arc/Cow type changes differ. Top 12 deltas by
magnitude:

| bench | before | after | delta |
|---|---:|---:|---:|
| `cross_file_unique_by/100000` | 133.22 ms | 123.36 ms | **-7.40%** |
| `regex_content/1024` |    79 ns |    73 ns | -7.23% |
| `rule_engine/100000` |  16.12 ms |  14.98 ms | **-7.08%** |
| `glob_compile/1000` | 119.67 ms | 111.80 ms | -6.57% |
| `cross_file_unique_by/1000` |   1.09 ms |   1.01 ms | **-6.54%** |
| `cross_file_for_each_dir/100` | 770.62 µs | 723.52 µs | -6.11% |
| `glob_compile/10` |   1.00 ms | 943.41 µs | -5.73% |
| `glob_match/100000` |  10.30 ms |  10.87 ms | +5.52% |
| `glob_compile/100` |  10.91 ms |  10.32 ms | -5.39% |
| `glob_match/1000` | 102.14 µs | 107.32 µs | +5.08% |
| `walker/100` |   2.55 ms |   2.67 ms | +4.92% |
| `single_file_file_is_text/1000` |   4.12 ms |   3.92 ms | -4.81% |

Where the type pass actually shipped its win:

- **`rule_engine/100000` -7.08%** — the violation hot
  path. ~100k violations, each previously cloning a
  PathBuf and a String, now sharing Arcs.
- **`cross_file_unique_by/100000` -7.40%** — group-by
  bucketing now stores `Arc<Path>` instead of `PathBuf`.
- **`cross_file_unique_by/1000` -6.54%**,
  **`cross_file_for_each_dir/100` -6.11%** — same shape
  on smaller scenarios.

Where the deltas are noise (no expected change):

- `glob_*` and `regex_content` (±5%) — type pass doesn't
  touch glob compilation or regex matching; the deltas are
  same-day measurement variance, particularly on
  nanosecond-scale benches.
- `walker/*` (±5%) — type pass doesn't touch the walker
  code; the delta is variance.

Nothing past the ±10% threshold; gate passes.

## Methodology

Captured on the same self-hosted Linux runner (Linux 6.1,
x86_64, 8-core) the main CI lane runs on. Ran via:

```sh
cargo bench -p alint-bench --features fs-benches \
  --bench walker --bench rule_engine \
  --bench glob_compile --bench glob_match --bench regex_content \
  --bench single_file_rules --bench cross_file_rules --bench output_formats
cp -r target/criterion docs/benchmarks/v0.9/v0.9.2-memory-pass/criterion
```

Stale benches not exercised by v0.9.2 (`blame_cache`,
`structured_query_*`) were trimmed from the snapshot to
keep the diff surface focused on what actually got measured.

## Subsequent phases

- v0.9.3 (dispatch flip + per-rule scanning conversions
  absorbed from this phase's deferred scope) will produce
  `docs/benchmarks/v0.9/v0.9.3-dispatch-flip/criterion/`.

The pre-v0.9.2 baseline at
[`baseline-v0.9.1`](../baseline-v0.9.1/) stays frozen for
delta comparisons across the rest of the v0.9 cut.
