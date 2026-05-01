# Cross-file rule perf investigation (2026-05-01)

Captured `tracing::info!` phase + per-rule timings during the
investigation that produced commits `1cc6c5c` and `26075f3`. The
goal was to localize a +28–37 % regression in the 1M S3 hyperfine
numbers between v0.5.6 (528 s changed) and v0.9.4 (724 s changed).

## How the traces were captured

```sh
ALINT_LOG=alint_core=info \
    target/release/alint check \
    --config /tmp/alint-prof-<size>/.alint.yml \
    /tmp/alint-prof-<size> \
    >/dev/null 2>/dev/null
```

stdout is the violation report; stderr is the tracing fmt layer
output but in alint's setup `tracing-subscriber::fmt()` writes to
stdout, so the engine.phase events end up mixed with the report.
Each `*.phase.log` here was extracted with
`grep "engine.phase" raw.log`.

The trees are deterministic monorepo fixtures generated via
`xtask gen-monorepo --size {10k|100k|1m}`. Same shape as
`bench-scale`'s internal trees → directly comparable to the
published bench corpus.

## What the traces show

Each row is a phase or per-rule wall-time event. Phase order in
each file:

```
evaluate_facts
git_setup
build_filtered_index
cross_file_partition       # total, then …
cross_file_rule … (×N, descending by elapsed)
per_file_partition
assembly
engine_run_total
```

The diagnostic trick: read three traces at increasing N (10k, 100k,
1m) for the *same* binary and look at how each rule's `elapsed_us`
grows. Functions whose share-of-total grows monotonically are
super-linear; functions whose share holds steady are linear. At
v0.9.4, four `for_each_dir × crates/*` rules grew ~50× per 10× file
count = quadratic in (D × N).

## Files

| File | What it shows |
|---|---|
| `trace-{10k,100k}-baseline.phase.log` | v0.9.4 unmodified — establishes the regression's shape |
| `trace-*-after-file-exists.phase.log` | After the `file_exists` literal-path fast path landed |
| `trace-*-after-structured-path.phase.log` | Plus the `structured_path` and `iter.has_file` fast paths — final state matching commits `1cc6c5c` + `26075f3` |

The 1M `baseline` trace isn't here because the v0.9.4 1M run took
12 minutes per shot and we already had the macro number from
`docs/benchmarks/v0.9/scale/linux-x86_64/1m/results.md`. Generate
it locally if needed by checking out `9050745` and running the
`gen-monorepo` + tracing recipe above.

## Headline numbers

| Phase | 10k | 100k | 1m |
|---|---:|---:|---:|
| `engine_run_total` v0.9.4 baseline | 226 ms | 10.7 s | ~530 s |
| `engine_run_total` after fixes | 23 ms | 186 ms | 0.7 s |
| Speedup | 9.8× | 57× | ~750× |

Wall-time speedup at 1M is "only" 65× / 108× because the engine
is no longer the bottleneck — walker + report formatting now
dominate the ~11 s remaining wall.

## Reuse

The instrumentation is permanent. To diagnose the next cross-file
perf regression:

```sh
ALINT_LOG=alint_core=info alint check . 2>&1 | grep engine.phase
```

— the rule whose `elapsed_us` doesn't match its peers is the
suspect, and its share-of-total tells you whether the cost is
linear or super-linear without needing a full bisect.
