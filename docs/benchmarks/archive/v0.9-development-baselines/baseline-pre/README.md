# Pre-v0.9 baseline (captured 2026-04-30)

Frozen `target/criterion`-format snapshot of the five
benches the v0.9.1 walker change can plausibly affect:

- `glob_compile` *(v0.7.0 baseline; gate target)*
- `glob_match` *(v0.7.0 baseline; gate target)*
- `regex_content` *(v0.7.0 baseline; gate target)*
- `rule_engine` *(v0.7.0 baseline; gate target)*
- `walker` *(v0.8.4 bench; self-baselined per phase)*

Captured on commit `bec0cf4` (`docs(design): v0.9 design pass
— drafts for the engine cut`), which is the v0.9 starting
point: same code as v0.8.2 plus the v0.9 design pass; no
engine code has changed yet.

## How v0.9 phases use this

The v0.7.0 floor (`docs/benchmarks/v0.8/baseline-v0.7.0/`)
is the regression gate every v0.9.x phase must pass:

```sh
xtask bench-compare \
  --before docs/benchmarks/v0.8/baseline-v0.7.0/criterion \
  --after target/criterion \
  --threshold 10
```

This pre-v0.9 snapshot is the *delta* baseline — the per-
phase improvement target. Use it as the "before" side when
quantifying what each phase actually shipped:

```sh
xtask bench-compare \
  --before docs/benchmarks/v0.9/baseline-pre/criterion \
  --after target/criterion \
  --threshold 10
```

## How this baseline was captured

```sh
CARGO_TARGET_DIR=target-baseline \
  cargo bench -p alint-bench --features fs-benches \
    --bench walker --bench rule_engine \
    --bench glob_compile --bench glob_match --bench regex_content
cp -r crates/alint-bench/target-baseline/criterion \
  docs/benchmarks/v0.9/baseline-pre/criterion
```

Captured on the same self-hosted Linux runner (Linux 6.1,
x86_64) the main CI lane runs on. Cross-machine comparisons
are not meaningful — `bench-compare` should always run with
both sides captured on the same hardware. Re-capture this
baseline if the runner hardware ever changes.

## Why these specific benches

v0.9.1 (parallel walker) plausibly affects:

| Bench | Expected v0.9.1 direction |
|---|---|
| `walker/100` | Possibly slight regression — thread-spawn cost dominates at small N. |
| `walker/1000` | Improvement — parallelism kicks in. |
| `walker/10000` | Larger improvement — parallelism saturates. |
| `rule_engine/100` | Flat — walker is a small fraction of total run time. |
| `rule_engine/1000` | Improvement — walker is a bigger fraction. |
| `rule_engine/10000` | Improvement. |
| `rule_engine/100000` | Improvement. |
| `glob_compile`, `glob_match`, `regex_content` | Flat (no walker dependency). |

Subsequent phases will add their own per-phase baselines as
they ship: `docs/benchmarks/v0.9/v0.9.1-parallel-walker/`,
`docs/benchmarks/v0.9/v0.9.2-memory-pass/`,
`docs/benchmarks/v0.9/v0.9.3-dispatch-flip/`. The
`baseline-pre/` snapshot stays frozen at this point in
history to support delta comparisons across the entire
v0.9 cut.
