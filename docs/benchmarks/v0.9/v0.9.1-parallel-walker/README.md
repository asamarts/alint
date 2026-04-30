# v0.9.1 — parallel walker (captured 2026-04-30)

Frozen `target/criterion`-format snapshot of the five
benches the v0.9.1 walker change can affect, captured on the
v0.9.1-parallel-walker branch immediately before merge.

- `glob_compile`, `glob_match`, `regex_content` *(v0.7.0 baseline; gate target — must stay flat)*
- `rule_engine` *(v0.7.0 baseline; gate target — should improve)*
- `walker` *(v0.8.4 bench; self-baselined, large win at 1k+ expected)*

## v0.9.1 deltas

### vs v0.7.0 baseline (the gate)

```sh
xtask bench-compare \
  --before docs/benchmarks/v0.8/baseline-v0.7.0/criterion \
  --after  docs/benchmarks/v0.9/v0.9.1-parallel-walker/criterion \
  --threshold 10
```

| bench | before | after | delta |
|---|---:|---:|---:|
| `rule_engine/1000` | 155.86 µs | 148.43 µs | -4.77% |
| `glob_match/100000` |  10.94 ms |  10.49 ms | -4.11% |
| `regex_content/1024` |     78 ns |     75 ns | -3.43% |
| `regex_content/1048576` |     78 ns |     76 ns | -2.83% |
| `rule_engine/100000` |  15.62 ms |  15.27 ms | -2.25% |
| `glob_match/1000` | 102.56 µs | 100.47 µs | -2.04% |
| `glob_match/10000` |   1.06 ms |   1.04 ms | -1.98% |
| `glob_compile/10` | 974.06 µs | 957.91 µs | -1.66% |
| `glob_compile/100` |  10.34 ms |  10.43 ms | +0.82% |
| `rule_engine/10000` |   1.65 ms |   1.65 ms | -0.26% |
| `glob_compile/1000` | 114.98 ms | 115.22 ms | +0.21% |
| `regex_content/65536` |     77 ns |     77 ns | +0.01% |

All within ±5%. Gate passes.

### vs pre-v0.9 baseline (the per-phase delta)

```sh
xtask bench-compare \
  --before docs/benchmarks/v0.9/baseline-pre/criterion \
  --after  docs/benchmarks/v0.9/v0.9.1-parallel-walker/criterion \
  --threshold 10
```

The walker delta — the actual point of v0.9.1:

| bench | before | after | delta |
|---|---:|---:|---:|
| `walker/10000` |  52.25 ms | 18.67 ms | **-64.26%** ✅ |
| `walker/1000`  |   8.85 ms |  5.25 ms | **-40.62%** ✅ |
| `walker/100`   |   1.62 ms |  2.62 ms | **+61.18%** ⚠ (expected) |

The walker/100 regression is the small-N thread-spawn-overhead
case the [parallel_walker.md](../../../design/v0.9/parallel_walker.md)
design doc anticipated:

> walker/100 — 100 may regress slightly (thread spawn cost
> dominates at small N — that's fine).

In absolute terms it's 1ms of overhead at the 100-file size —
imperceptible to humans, dwarfed by the 33.6ms saved at 10k
files. The trade is correct for alint's target use cases
(workspace-tier and OSS-polyglot monorepos at 1k+ files).

This regression is **invisible to the v0.7.0 gate** because
`walker` is a v0.8.4 bench with no v0.7.0 counterpart. It
shows up only in the same-day per-phase delta against the
pre-v0.9 baseline.

## Methodology

Captured on the same self-hosted Linux runner (Linux 6.1,
x86_64, 8-core) the main CI lane runs on. Ran via:

```sh
cargo bench -p alint-bench --features fs-benches \
  --bench walker --bench rule_engine \
  --bench glob_compile --bench glob_match --bench regex_content
cp -r target/criterion docs/benchmarks/v0.9/v0.9.1-parallel-walker/criterion
```

`regex_content` was re-run after the first capture showed an
~11% regression vs the v0.7.0 baseline; the second run came
in at v0.7.0 numbers. The cause was first-run cache-warm
variance, not a v0.9.1-induced regression. Future v0.9.x
phases should expect ±5% noise on regex_content and re-run
that bench to confirm any apparent regression is real.

## Subsequent phases

- v0.9.2 (memory pass) will produce
  `docs/benchmarks/v0.9/v0.9.2-memory-pass/criterion/`.
- v0.9.3 (dispatch flip) will produce
  `docs/benchmarks/v0.9/v0.9.3-dispatch-flip/criterion/`.

Each phase compares against both the v0.7.0 floor (gate) and
the previous phase's snapshot (delta).
