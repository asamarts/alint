# v0.9.6-postfix bench-scale capture (2026-05-02)

Re-capture of the v0.9.6 macro matrix after the post-release
`scope_filter:` runtime fix. The released v0.9.6 binary parses
`scope_filter:` into the spec, ships the `ScopeFilter` runtime type,
and the engine consults `Rule::scope_filter()` on the per-file
dispatch path — but no per-file rule builder threaded the parsed
filter onto the built rule, so the trait method always returned
`None` and the engine's gate was a silent no-op for every per-file
rule (including the bundled ecosystem rulesets that motivated the
primitive).

The fix wires `RuleSpec::parse_scope_filter()` into all 25 per-file
rule builders + adds the runtime check to each rule's
rule-major fallback path (so `alint fix` honours the filter the same
way `alint check` does). See `CHANGELOG.md` "Unreleased" / "Fixed".

## How this run was captured

```sh
xtask bench-scale \
    --sizes 10k,100k \
    --scenarios S1,S2,S3,S4,S5,S6,S7,S8,S9 \
    --modes full \
    --tools alint \
    --warmup 2 --runs 5 \
    --json-only \
    --out docs/benchmarks/macro/results/linux-x86_64/v0.9.6-postfix
```

Same matrix as the v0.9.6 capture, same machine fingerprint, same
seed (`0xa11e47`).

## Comparison vs published v0.9.6

| Workload | Published v0.9.6 | Postfix | Δ |
|---|---:|---:|---:|
| 10k S1 full | 20.89 ms | 21.19 ms | +1.4 % |
| 10k S2 full | 32.14 ms | 32.31 ms | +0.5 % |
| 10k S3 full | 125.14 ms | 120.42 ms | -3.8 % |
| 10k S4 full | 23.13 ms | 30.63 ms | (σ=19.4 ms outlier; ignore) |
| 10k S5 full | 92.19 ms | 91.83 ms | -0.4 % |
| 10k S6 full | 119.33 ms | 110.41 ms | **-7.5 %** |
| 10k S7 full | 206.13 ms | 207.78 ms | +0.8 % |
| 10k S8 full | 115.38 ms | 113.95 ms | -1.2 % |
| 10k S9 full | 73.59 ms | 71.21 ms | -3.2 % |
| 100k S1 full | 159.56 ms | 159.69 ms | +0.1 % |
| 100k S2 full | 249.99 ms | 254.96 ms | +2.0 % |
| 100k S3 full | 1135.27 ms | 1169.97 ms | +3.1 % |
| 100k S4 full | 155.57 ms | 158.32 ms | +1.8 % |
| 100k S5 full | 917.17 ms | 896.03 ms | -2.3 % |
| 100k S6 full | 1221.27 ms | 1066.68 ms | **-12.7 %** |
| 100k S7 full | 10785.33 ms | 9935.25 ms | **-7.9 %** |
| 100k S8 full | 1071.94 ms | 1077.47 ms | +0.5 % |
| 100k S9 full | 738.58 ms | 691.83 ms | **-6.3 %** |

## Reading the deltas

- **No regression anywhere.** S1, S2, S3, S4 (ignoring the σ-19 outlier),
  S5, S8 are all within run-to-run noise (±5 %). The released v0.9.6
  shape held — the fix does not slow down rules that don't use
  `scope_filter:`.
- **S6 / S7 / S9 are 6 – 13 % faster.** S6 is the per-file
  content-fan-out scenario, S7 is cross-file relational, S9 is the
  nested-polyglot showcase. All three exercise the bundled rulesets;
  S9 directly extends `rust@v1` + `node@v1` + `python@v1`, which now
  correctly skip out-of-scope files via the wired `scope_filter` gate
  instead of evaluating every matched file.
- **S9 specifically validates the v0.9.6 design intent.** The published
  v0.9.6 number captured "what alint did when `scope_filter:` was a
  silent no-op" — i.e., per-file rules firing on every matched file in
  the polyglot tree. The postfix number captures the design's actual
  semantics — files outside their ecosystem subtree are skipped. The
  delta is the cost of the bug.

## Caveats

- 5 runs is faster than the publication-grade 7-10 runs; treat the
  individual deltas as direction-correct, not as headline numbers. The
  cross-scenario shape (no regressions, modest speedup on
  scope_filter-using scenarios) is robust to the lower run count.
- The S4 10k cell has σ = 19.4 ms, indicating concurrent system load
  during that single cell (a parallel `cargo bench` was running). The
  median (22.1 ms) is consistent with the published 23.1 ms; ignore
  the mean.
- The 100k S7 cell is high-variance in both runs (published σ = 1175 ms;
  postfix σ = 198 ms). S7 is the cross-file-relational scenario whose
  cost is dominated by the lazy-path-index build and a few thousand
  glob-match operations; first-run cache effects dominate. Both runs
  agree the headline is "≈10 s", so the apparent -7.9 % delta is
  within the noise envelope on this scenario specifically.

## Next steps

- The v0.9.6 published numbers should be treated as historical (released
  binary's actual behaviour, including the bug). The postfix numbers
  here are what the next release (v0.9.7 or v0.10) will start from.
- The next publish-grade capture should be the v0.10 release; until
  then this directory is the working baseline.
