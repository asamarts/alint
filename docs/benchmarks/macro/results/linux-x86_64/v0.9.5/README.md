# 1M S3 bench after the path-index fix (captured 2026-05-01)

Hyperfine numbers for the 1M-scale S3 scenario captured after the
two cross-file-rule perf commits landed:

- `1cc6c5c` — `perf(core): lazy path-index on FileIndex + scaling-profile instrumentation`
- `26075f3` — `perf(rules): O(1) literal-path fast paths in file_exists, structured_path, iter.has_file`

The fingerprint inside `index.md` reports
`alint: 0.9.4 (9050745)` because the crate version wasn't bumped
yet and `9050745` was HEAD when xtask snapshotted the rev. The
binary actually tested was post-fix — re-confirmable by checking
out either of the two commits above and running

```sh
xtask bench-scale --include-1m --sizes 1m --scenarios S3 \
    --modes full,changed --warmup 1 --runs 3 \
    --out docs/benchmarks/macro/results/linux-x86_64/v0.9.5
```

(the harness will rebuild `alint` first, so HEAD's binary is what
gets timed regardless of the snapshot label).

## Headline vs published v0.9.4 baseline

```sh
cat docs/benchmarks/macro/results/linux-x86_64/v0.9.4/index.md
cat docs/benchmarks/macro/results/linux-x86_64/v0.9.5/1m/results.md
```

| Cell | v0.9.4 baseline | After fix | Speedup |
|---|---:|---:|---:|
| `1m S3 full` | 731.856 s | 11.194 s ± 0.154 | **65.4×** |
| `1m S3 changed` | 724.362 s | 6.728 s ± 0.059 | **107.7×** |

## Headline vs v0.5.6 (pre-regression baseline, same 1M S3 corpus)

```sh
cat docs/benchmarks/macro/results/linux-x86_64/v0.5.6/1m/results.md
```

| Cell | v0.5.6 | After fix | Speedup |
|---|---:|---:|---:|
| `1m S3 full` | 569.078 s | 11.194 s | **50.8×** |
| `1m S3 changed` | 528.103 s | 6.728 s | **78.5×** |

So this fix doesn't just close the v0.9.x regression — it makes
1M-scale S3 runs ~50–80× faster than they have ever been published
in a release. The investigation that produced the fix is documented
under [`docs/benchmarks/investigations/2026-05-cross-file-rules/`](../../../investigations/2026-05-cross-file-rules/).
