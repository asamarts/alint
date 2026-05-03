# v0.9.7 bench-scale capture (2026-05-02)

Full publish-grade S1–S9 × {1k, 10k, 100k, 1m} × {full, changed}
matrix — the first version captured at full breadth (S8 changed
inclusive) thanks to the `init_git_for_changed_mode` harness fix
that landed alongside v0.9.7's `bench-record.yml`.

v0.9.7's engine code is identical to v0.9.6 except for the
`scope_filter:` runtime fix (which only changes per-file rule
behaviour when `scope_filter:` is set in the config — none of
S1–S8 use it; S9 inherits it from the bundled rulesets it
extends, but the dispatch shape is unchanged at S9's small
ruleset count).

S7 1M still measures ~614 s — the cross-file dispatch cliff
this version doesn't address. v0.9.8 targets it via
`FileIndex::children_of` (see
[`docs/design/v0.9.8/cross-file-fast-paths-v2.md`](../../../../design/v0.9.8/cross-file-fast-paths-v2.md))
with an acceptance gate of "drops below 100 s, target 50 s".

## How this run was captured

```sh
xtask bench-scale --include-1m \
    --sizes 1k,10k,100k,1m \
    --scenarios S1,S2,S3,S4,S5,S6,S7,S8,S9 \
    --modes full,changed \
    --tools alint \
    --warmup 3 --runs 10 \
    --json-only \
    --out docs/benchmarks/macro/results/linux-x86_64/v0.9.7/main
```

72 cells captured. The harness fix lets S8 changed mode run
without the "git nothing to commit" failure that previously
required running S8 in `full` only (see v0.9.5/v0.9.6 captures).

## Hardware fingerprint

`linux-x86_64` (AMD Ryzen 9 3900X 12-core / 62 GB / ext4 / rustc 1.95).
See [`../METHODOLOGY.md`](../METHODOLOGY.md) for the cross-version
comparison contract.
