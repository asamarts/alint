# v0.9.6 bench-scale capture (re-bench, 2026-05-02)

Comprehensive S1–S9 × {1k, 10k, 100k, 1m} × {full, changed except S8}
matrix captured from the `v0.9.6` tag binary. Replaces the original
v0.9.6 publish that only covered `S1–S9 × {10k, 100k} × full`.

v0.9.6 shipped the `scope_filter:` primitive and the bundled-
ecosystem-ruleset migration (`is_*` → `has_*`), with the latent
runtime no-op bug fixed in v0.9.7. Cross-file dispatch shape is
unchanged from v0.9.5 — S7 1M still measures ~624 s, exposing the
same cliff the v0.9.5 published numbers couldn't show (because
they only ran S3 at 1M).

## How this run was captured

```sh
xtask bench-scale --include-1m \
    --sizes 1k,10k,100k,1m \
    --scenarios S1,S2,S3,S4,S5,S6,S7,S9 \
    --modes full,changed \
    --tools alint \
    --warmup 3 --runs 10 \
    --json-only \
    --out docs/benchmarks/macro/results/linux-x86_64/v0.9.6/main
xtask bench-scale --include-1m \
    --sizes 1k,10k,100k,1m \
    --scenarios S8 \
    --modes full \
    --tools alint \
    --warmup 3 --runs 10 \
    --json-only \
    --out docs/benchmarks/macro/results/linux-x86_64/v0.9.6/s8full
```

S8 split into a sub-run for the same reason as v0.9.5: the
harness `init_git_for_changed_mode` bug isn't fixed until v0.9.7.

## Hardware fingerprint

`linux-x86_64` (AMD Ryzen 9 3900X 12-core / 62 GB / ext4 / rustc 1.95).
See [`../METHODOLOGY.md`](../METHODOLOGY.md) for the cross-version
comparison contract.
