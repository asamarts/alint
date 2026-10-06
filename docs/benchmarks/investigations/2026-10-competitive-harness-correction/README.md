# Competitive benchmark harness correction

## Summary

The v0.5.7 competitive report did not measure ls-lint linting the synthetic
repositories. The harness wrote `.ls-lint.yml` inside each generated tree, but
invoked `ls-lint -workdir <tree>` without `-config`. ls-lint resolves its default
config relative to the process working directory, immediately failed to open
the file, and returned exit code 1. The harness told hyperfine to ignore every
non-zero status, so that configuration error became a very fast “result.”

The tell was its impossible scaling shape: 27.9 ms at 1k files, 28.2 ms at 10k,
and 27.4 ms at 100k. A full-tree linter cannot remain flat across 100 times as
many files. Those historical ls-lint rows remain available for provenance but
are invalid and must not be compared with alint.

This investigation repairs the harness and publishes a replacement
current-checkout matrix. It does not rewrite the historical v0.5.7 report.

## What changed

- ls-lint now receives both `-workdir <tree>` and the explicit
  `-config <tree>/.ls-lint.yml`; `-warn` makes findings exit successfully while
  startup and config failures remain fatal.
- Hyperfine receives a per-tool finding-status allowlist instead of a blanket
  `--ignore-failure`. ls-lint ignores no non-zero statuses. alint, the shell
  baseline, and Repolinter permit only status 1 after an untimed integrity
  probe has established what that status means for the row.
- Every competitive S1/S2 row gets a temporary planted violation before it is
  timed. The tool must return the exact expected status and its output must
  contain the planted filename or alint/Repolinter rule ID. The shell probe
  uses a diagnostic command that does not suppress output.
- Repolinter 0.11.2 was tested directly: both a genuine finding and malformed
  `repolinter.json` return status 1. Its probe therefore requires the
  `no-todo-rust` rule ID, not merely status 1.
- Runs containing 1k and 100k full rows warn when a tool/scenario has less than
  2× growth. The warning is advisory, but it would have caught the v0.5.7
  ls-lint result (0.98×).
- The former `grep` label is now `shell`. Its fingerprint records all commands
  used: GNU find and grep for S1, plus ripgrep for S2.
- The benchmark image now contains Hyperfine 1.20.0, whose
  `--ignore-failure=<codes>` form is required here, and a bench-only Rust
  1.88.0 toolchain. Node.js is pinned at 20.20.2 rather than following the
  moving NodeSource 20.x repository head. The image overrides the mounted
  checkout's moving `stable` toolchain request.
- Docker wrapper arguments and permissions were corrected: seeds are forwarded
  in the decimal syntax accepted by the inner CLI, host competitor binaries
  are not required for a container run, the Cargo directories are writable by
  the host UID/GID, and generated result files remain host-owned.

## Corrected matrix

The replacement run used the current Ryzen 9 3900X benchmark machine and the
repaired local image. These are within-run comparisons only; they do not replace
the canonical per-release performance history on a different host.

| Tool | Scenario | 1k mean | 10k mean | 100k mean | 100k / 1k |
|---|---|---:|---:|---:|---:|
| alint | S1 layout/path | 10.8 ms | 48.2 ms | 425.4 ms | 39.41× |
| ls-lint | S1 filename hygiene | 35.0 ms | 67.7 ms | 371.0 ms | 10.59× |
| shell (`find` + grep) | S1 layout/path approximation | 21.8 ms | 74.7 ms | 463.0 ms | 21.21× |
| alint | S2 per-file content | 18.0 ms | 119.7 ms | 1,198.1 ms | 66.71× |
| shell (`test` + `find` + ripgrep) | S2 approximation | 26.8 ms | 72.6 ms | 480.5 ms | 17.95× |
| Repolinter | S2 existence/content subset | 394.2 ms | 1,394.3 ms | 13,374.7 ms | 33.93× |

All six ratios are comfortably above the 2× diagnostic floor. The corrected
ls-lint series now grows 10.59× from 1k to 100k instead of the invalid 0.98×.
The tools do not implement identical rule sets, so the table characterizes
their supported workload shapes; it is not a claim of semantic equivalence.

Hyperfine flagged the first measured Repolinter 100k sample (16.141 s) as
slower than the rest. It is retained in the raw result rather than discarded;
the cell's 8.5% coefficient of variation remains inside the macro gate's 10%
quality ceiling for 100k rows.

Raw per-sample statistics, commands, and the complete machine/tool fingerprint
are in [`results/index.md`](results/index.md) and
[`results/results.json`](results/results.json).

## Reproduction

The image was built from this checkout:

```sh
docker build -t alint-bench:issue-270 -f bench/Dockerfile bench
```

The committed results use the publication-grade sample count:

```sh
ALINT_BENCH_IMAGE=alint-bench:issue-270 \
  cargo run -q -p xtask --release -- bench-scale --docker \
  --sizes 1k,10k,100k --scenarios S1,S2 --modes full --tools all \
  --warmup 3 --runs 10 \
  --out docs/benchmarks/investigations/2026-10-competitive-harness-correction/results
```

The run completed without a readiness or flat-scaling warning. Its fingerprint
records Linux/x86_64, AMD Ryzen 9 3900X (24 logical cores), 62 GB RAM, overlay
filesystem, rustc 1.88.0, Hyperfine 1.20.0, ls-lint 2.2.3, Repolinter 0.11.2
on Node.js 20.20.2, ripgrep 15.1.0, and alint 0.17.0 at `fb4407a6`.
