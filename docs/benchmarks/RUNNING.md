# Running benchmarks

Two layers, one regression-gate, one publish helper.

## Micro (criterion)

Pure-CPU primitives. Runs in seconds, stable across runs. Run after every
change to `alint-core` or `alint-rules` to spot hot-path regressions.

```sh
# All micro-benches (12 files under crates/alint-bench/benches/)
cargo bench -p alint-bench --features fs-benches

# A single bench file (faster iteration during dev)
cargo bench -p alint-bench --features fs-benches --bench rule_engine

# Quick mode — skips the warmup/sample-size phase, useful for smoke-testing
# that the bench compiles and a regression hasn't shown up at headline scale
cargo bench -p alint-bench --features fs-benches --bench rule_engine -- --quick
```

Output lives at `target/criterion/`. Each subdirectory matches a benchmark
group / size; criterion auto-generates HTML reports under
`target/criterion/report/index.html`.

What each bench measures: see [`micro/README.md`](micro/README.md).

## Macro (hyperfine bench-scale)

End-to-end CLI wall-time over deterministic synthetic monorepos. Slow at
the larger sizes; opt-in to 1M.

```sh
# Default — alint-only over 1k/10k/100k × S1/S2/S3 × full/changed
xtask bench-scale

# Include the 1M size (multi-GB working set, slow — ~10 minutes per run)
xtask bench-scale --include-1m

# Focused single cell — useful for perf-investigation iteration
xtask bench-scale --include-1m --sizes 1m --scenarios S3 --modes full \
    --warmup 1 --runs 2

# All scenarios (S1-S8) at every size, both modes
xtask bench-scale --include-1m --scenarios S1,S2,S3,S4,S5,S6,S7,S8

# Compare against ls-lint and grep on the scenarios they support
xtask bench-scale --tools all
```

Defaults to `--out docs/benchmarks/macro/results/<arch>/<workspace-version>/`
so a publish-grade run lands directly under the right per-version
directory. Override with `--out` for ad-hoc / investigation runs that
shouldn't pollute the published corpus.

What each scenario tests: see [`macro/README.md`](macro/README.md).

## Persistent trees for ad-hoc profiling

`bench-scale` materialises trees in tempdirs that get cleaned up at
shutdown. For perf investigations that need the same tree across many
profiler runs (5-10 minutes of tree-gen at 1M is wasteful), use:

```sh
xtask gen-monorepo --size 1m --out /tmp/alint-prof-1m
cp xtask/src/bench/scenarios/s3_workspace.yml /tmp/alint-prof-1m/.alint.yml

# Now run alint check repeatedly without regenerating the tree
ALINT_LOG=alint_core=info \
    target/release/alint check /tmp/alint-prof-1m \
    >/dev/null 2>/tmp/trace.log
```

The `ALINT_LOG=alint_core=info` env var enables per-phase + per-rule
wall-time emission via `tracing::info!` — see
[`investigations/README.md`](investigations/README.md) for what to do with
the output.

## Regression gate

`bench-compare` reads two `target/criterion`-format directories and
exits non-zero when any paired benchmark's mean has grown past the
threshold:

```sh
# Compare against the v0.7.0 floor every release ships gated against
xtask bench-compare \
    --before docs/benchmarks/micro/results/linux-x86_64/v0.7.0/criterion \
    --after  target/criterion \
    --threshold 10
```

CI runs this against the v0.7.0 floor on every push. A per-phase delta
(commit-by-commit during a release cut) compares against the prior phase's
snapshot under [`archive/v0.9-development-baselines/`](archive/) — see
the v0.9 design pass for the convention.

## Publishing benches

After a release tag, copy `target/criterion` into the per-version
published directory:

```sh
xtask publish-benches
# → copies target/criterion/ → docs/benchmarks/micro/results/<arch>/<version>/criterion/
```

For macro benches, `bench-scale --out` already writes to the right
per-version dir; just run the publication-grade matrix:

```sh
xtask bench-scale --include-1m --scenarios S1,S2,S3 --modes full,changed \
    --warmup 3 --runs 10
```

Then verify the new dir is present, commit, tag.

## Hardware fingerprint

`bench-scale` captures a hardware fingerprint with each run (rustc
version, CPU model, RAM, filesystem) and writes it to `index.md`'s
header. Cross-machine comparisons MUST match fingerprints — see
[`METHODOLOGY.md`](METHODOLOGY.md) for the contract.

## Bench coverage

The soft `coverage_audit_bench_listing.rs` test (under
`crates/alint-e2e/tests/`) emits an `eprintln!` listing of rule kinds
in the registry but absent from any bench scenario:

```sh
cargo test -p alint-e2e --test coverage_audit_bench_listing -- --nocapture
```

Use the listing as a triage list when extending S6 / S7 / S8 — kinds
without a bench scenario have no perf gate against the next regression
of their dispatch shape.
