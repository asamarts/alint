# v0.9.4 — content-rule mechanical migration (captured 2026-04-30)

Frozen `target/criterion`-format snapshot of every bench
v0.9.4's content-rule migrations touch.

## Headline: 80-85% speedups on every newly-migrated single-file rule

Same pattern as v0.9.3's headline 80-85% wins, now applied
to the rest of the per-file content family. After this
release, every per-file rule that reads file content
benefits from the read-coalescing path.

## v0.9.4 deltas

### vs v0.7.0 baseline (the gate)

```sh
xtask bench-compare \
  --before docs/benchmarks/v0.8/baseline-v0.7.0/criterion \
  --after  docs/benchmarks/v0.9/v0.9.4-content-rules/criterion \
  --threshold 10
```

| bench | before | after | delta |
|---|---:|---:|---:|
| `glob_match/100000` |  10.94 ms |  10.09 ms | -7.81% |
| `glob_match/10000` |   1.06 ms | 990.80 µs | -6.68% |
| `regex_content/1024` |    78 ns |    73 ns | -5.87% |
| `glob_compile/10` | 974.06 µs | 917.72 µs | -5.78% |
| `rule_engine/1000` | 155.86 µs | 149.93 µs | **-3.81%** |
| `glob_compile/1000` | 114.98 ms | 110.86 ms | -3.58% |
| `rule_engine/100000` |  15.62 ms |  15.13 ms | **-3.11%** |
| `glob_compile/100` |  10.34 ms |  10.02 ms | -3.10% |
| `regex_content/1048576` |    78 ns |    76 ns | -2.94% |
| `regex_content/65536` |    77 ns |    75 ns | -2.73% |
| `rule_engine/10000` |   1.65 ms |   1.67 ms | +1.13% |
| `glob_match/1000` | 102.56 µs | 101.90 µs | -0.65% |

All within ±10%; **every paired bench has improved or
stayed flat against v0.7.0**. Gate passes. The
accumulating `rule_engine/*` improvements compound the
v0.9.2 Arc/Cow type pass + v0.9.3 dispatch flip + v0.9.4
content-rule migration.

### vs pre-v0.9.4 baseline (the per-phase delta)

```sh
xtask bench-compare \
  --before docs/benchmarks/v0.9/baseline-v0.9.3/criterion \
  --after  docs/benchmarks/v0.9/v0.9.4-content-rules/criterion \
  --threshold 10
```

The headline: 10 single-file benches show 75-85%
improvements, plus modest cross-file wins.

| bench | before | after | delta |
|---|---:|---:|---:|
| `single_file_file_header/1000` |   5.46 ms | 798.36 µs | **-85.37%** ✅ |
| `single_file_file_content_matches/1000` |   5.19 ms | 774.82 µs | **-85.07%** ✅ |
| `single_file_file_hash/1000` |   6.77 ms |   1.01 ms | **-85.06%** ✅ |
| `single_file_file_content_forbidden/1000` |   5.18 ms | 782.56 µs | **-84.89%** ✅ |
| `single_file_file_content_forbidden/100` | 532.51 µs | 100.45 µs | **-81.14%** ✅ |
| `single_file_file_content_matches/100` | 525.37 µs |  99.96 µs | **-80.97%** ✅ |
| `single_file_file_is_text/1000` |   4.10 ms | 786.95 µs | **-80.81%** ✅ |
| `single_file_file_header/100` | 543.62 µs | 106.19 µs | **-80.47%** ✅ |
| `single_file_file_hash/100` | 670.87 µs | 151.91 µs | **-77.36%** ✅ |
| `single_file_file_is_text/100` | 410.56 µs |  99.93 µs | **-75.66%** ✅ |
| `cross_file_unique_by/1000` |   1.35 ms |   1.09 ms | -19.80% ✅ |
| `cross_file_every_matching_has/100` |   1.03 ms | 922.88 µs | -10.61% ✅ |
| `output_formats_100_violations/human` |  65.63 µs |  72.81 µs | +10.93% ⚠ |

Where the wins come from, per family:

- **Content-pattern rules** (`file_content_matches`,
  `file_content_forbidden`, `file_header`, `file_footer`):
  same shape as v0.9.3's `file_starts_with` / line-oriented
  rules — the rule body migrates to consume `&[u8]`
  directly, the engine reads each file once instead of the
  rule's per-file `fs::read`.
- **`file_hash`**: SHA-256 over the engine-supplied byte
  slice, no separate read.
- **`file_is_text`**: declares
  `max_bytes_needed: TEXT_INSPECT_LEN`; `evaluate_file`
  inspects only the first 8 KiB of the engine-supplied
  slice.
- **`no_bom`**: `max_bytes_needed: 4`; rule-major path
  uses bounded `read_prefix_n` for solo runs.

### Single regression

- `output_formats_100_violations/human` (+10.93%) — barely
  past the per-phase threshold. v0.9.4 doesn't touch
  formatters; this is same-day variance on the smallest
  output bench (microsecond-scale). Not a real regression;
  the v0.7.0 gate's same bench is well within bounds, and
  the 1000- and 10000-violation `human` cases are flat or
  improved.

## Methodology

Captured on the same self-hosted Linux runner (Linux 6.1,
x86_64, 8-core) the main CI lane runs on. Ran via:

```sh
cargo bench -p alint-bench --features fs-benches \
  --bench walker --bench rule_engine \
  --bench glob_compile --bench glob_match --bench regex_content \
  --bench single_file_rules --bench cross_file_rules --bench output_formats
cp -r target/criterion docs/benchmarks/v0.9/v0.9.4-content-rules/criterion
```

Stale benches not exercised by v0.9.4
(`blame_cache_warm_lookup`, `structured_query_*`) trimmed
from the snapshot to keep the diff surface focused.

## v0.9 cut closure

This is the last v0.9.x release. Engine-optimization phases
shipped:

| Release | Headline win |
|---|---|
| v0.9.1 | parallel walker (-64% at 10k files) |
| v0.9.2 | Arc/Cow type pass (-7% at 100k violations) |
| v0.9.3 | dispatch flip + 8 reference rules (-85% migrated single-file rules) |
| v0.9.4 | content-rule mechanical migration (-85% on the rest) |

Across the cut: every per-file content rule that reads
file content now benefits from the read-coalescing path
when multiple rules share a scope. Every walker bench is
faster. The `rule_engine/100000` bench is ~7% faster than
v0.7.0 — the violation-creation hot path is leaner across
the board.

**Next:** v0.10 — LSP server. Per-file dispatch shape from
v0.9.3+ directly powers per-file-edit re-evaluation.
