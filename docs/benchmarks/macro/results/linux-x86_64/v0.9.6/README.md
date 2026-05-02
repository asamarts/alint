# v0.9.6 bench-scale capture (2026-05-02)

First publish-grade run with the full **S1–S9** matrix on a single
machine fingerprint. v0.9.6 closes the v0.9 cut with the
`scope_filter:` primitive plus the bundled-ecosystem-ruleset
migration that motivated it; the new **S9** scenario (nested
polyglot monorepo: rust + node + python over `crates/` +
`packages/` + `apps/`) captures the dispatch shape
`scope_filter:` was designed for.

## How this run was captured

```sh
xtask bench-scale \
    --sizes 10k,100k \
    --scenarios S1,S2,S3,S4,S5,S6,S7,S8,S9 \
    --modes full \
    --tools alint \
    --warmup 2 --runs 5 \
    --json-only \
    --out docs/benchmarks/macro/results/linux-x86_64/v0.9.6
```

The 100k S6/S7/S8/S9 cells were re-captured with `--warmup 3
--runs 7` after the initial run flagged 19–40 % CV on those four
(concurrent system load — re-running on a quieter system pulled
all four to CV ≤ 11 %). The clean rerun was merged into
`results.json`; the noisy first-pass numbers are not retained.

The 1m size is intentionally absent from this capture — v0.9.6
ships no engine code that would change the 1m S3 numbers vs
v0.9.5 (the last 1m capture). The Phase 5 Run 2 same-machine
A/B (`investigations/2026-05-scope-filter-baseline-drift/`)
already established that the engine commit's null-default
`Rule::scope_filter()` trait method is unmeasurable on rules
that don't override it.

## Reading the numbers

- **S1–S8** are the existing scenarios. v0.9.6 should be flat
  vs v0.9.5 on these — and is, within criterion / hyperfine
  noise (CV typically 2–4 %). The 100k S3 number (1.135 s)
  is the first post-v0.9.5-path-index-fix capture at 100k;
  the prior `HISTORY.md` cell of 11.20 s for 100k S3 was a
  pre-fix carry-over from v0.9.4 and is now stale.
- **S9** is new. 10k = 73.6 ms ± 1.4; 100k = 738.6 ms ± 31.5.
  Per-rule scope_filter ancestor walks under three competing
  rulesets aren't dominating — the dispatch shape stays
  bounded by the v0.9.5 path-index lookups (≈ 150 ns per
  `contains_file` × 5 rules × 100k files = ~75 ms scope_filter
  overhead at full scale, matching the design doc's
  prediction).

## Files

- [`index.md`](./index.md) — top-level summary table for all
  sizes / scenarios
- [`results.json`](./results.json) — machine-readable rows
  with full fingerprint + per-iteration timings
- [`10k/results.md`](./10k/results.md) — 10k size detail
- [`100k/results.md`](./100k/results.md) — 100k size detail

## Cross-version comparison

See [`../../../HISTORY.md`](../../../HISTORY.md) for the
chronological perf table across release tags. Cross-machine
comparisons require like-for-like fingerprints — see
[`../../../METHODOLOGY.md`](../../../METHODOLOGY.md) for the
hardware contract and why ambient machine state matters.
