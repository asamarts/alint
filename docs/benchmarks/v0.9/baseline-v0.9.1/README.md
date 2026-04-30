# Pre-v0.9.2 baseline (captured 2026-04-30)

Frozen `target/criterion`-format snapshot of every bench
v0.9.2's memory pass can plausibly affect, captured on
commit `32726b6` (`fix(ci): use local action ref in
selftest…`) — i.e., the v0.9.1 tag plus the action-selftest
fix, with no Phase-1 type changes yet applied.

Bench groups:
- `glob_compile`, `glob_match`, `regex_content` *(v0.7.0
  baseline; gate target — must stay flat)*
- `rule_engine` *(v0.7.0 baseline; gate target — should
  improve modestly via the Arc clones replacing PathBuf
  clones on the violation hot path)*
- `walker` *(v0.8.4 bench; expected flat — type pass
  doesn't touch walker logic)*
- `single_file_*` *(v0.8.4; expected modest improvement on
  rules that emit many violations)*
- `cross_file_*` *(v0.8.4; expected modest improvement;
  unique_by / no_case_conflicts now group on Arc<Path>
  instead of PathBuf)*

## v0.9.2 deltas

For the per-phase delta against this baseline (which is the
right comparison: same hardware, same day, only the v0.9.2
type-pass code differs), use:

```sh
xtask bench-compare \
  --before docs/benchmarks/v0.9/baseline-v0.9.1/criterion \
  --after  docs/benchmarks/v0.9/v0.9.2-memory-pass/criterion \
  --threshold 10
```

For the gate against the v0.7.0 floor (which v0.9.x phases
are required to stay green against), use:

```sh
xtask bench-compare \
  --before docs/benchmarks/v0.8/baseline-v0.7.0/criterion \
  --after  docs/benchmarks/v0.9/v0.9.2-memory-pass/criterion \
  --threshold 10
```

## Methodology

Captured on the same self-hosted Linux runner (Linux 6.1,
x86_64, 8-core) the main CI lane runs on. Ran via:

```sh
CARGO_TARGET_DIR=target-baseline-v0.9.2 \
  cargo bench -p alint-bench --features fs-benches \
    --bench walker --bench rule_engine \
    --bench glob_compile --bench glob_match --bench regex_content \
    --bench single_file_rules --bench cross_file_rules
cp -r crates/alint-bench/target-baseline-v0.9.2/criterion \
  docs/benchmarks/v0.9/baseline-v0.9.1/criterion
```

The same-day-and-hardware constraint matters: the v0.7.0
baseline numbers in `docs/benchmarks/v0.8/baseline-v0.7.0/`
were captured on a different day and exhibit ~5% drift on
nanosecond-scale benches (`regex_content` especially); the
per-phase delta against this baseline isolates the v0.9.2
change cleanly.
