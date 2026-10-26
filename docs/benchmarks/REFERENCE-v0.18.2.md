# The v0.18.2 performance reference

ADR-0019 decision DEC-13 holds the general-linter program to cumulative budgets against the frozen
v0.18.2 release, and ADR-0023 decision V21-6 makes that reference blocking for relation-layer
changes, measured as ADR-0024 decision 6 describes. This file records the reference measurement (v0.21 work item R0) and how the gate uses
it. `docs/benchmarks/ANCHOR` (v0.20.0, +10%) is a separate reference that every release's bench
gates use; it is unchanged.

## The gate

`ci/scripts/reference-perf-gate.sh`, run by ci.yml's `perf-gate-reference` job on pull requests
that `ci/scripts/detect-changes.sh` classifies as relation-layer changes, and on every Rust push to
`main` (the comparison is absolute, so whatever crossed the budget is caught where it landed):

- **Instructions.** The job builds the v0.18.2 release binary (with lints capped, so a new lint
  cannot fail frozen source) and HEAD's with the same toolchain, and runs both through HEAD's
  `det_check` harness, passing each as `ALINT_DET_BIN`: the same generated trees, Valgrind and
  gungraun runner, with only the alint binary differing. Each of the six S1-S3 cells (1k and 10k)
  must be present with a v0.18.2 measurement, and none may be more than +3% over it; a
  regression is gungraun's "Regressed" verdict, and any other failure is a tooling failure. The
  numbers below are not a baseline the gate reads: it measures both sides in the same job, so
  neither the host nor the toolchain can drift the comparison.
- **Syscalls.** Four families (read, stat, open, directory listing; `ci/scripts/syscall-counts.py`
  holds the list) for the alint process and its threads, spawned children excluded, on the 10k
  trees. A family more than 32 calls over v0.18.2 fails. A trace that is empty, lacks the
  program's `execve`, or has a family with no calls, and a traced run that exited above 1, are
  tooling failures, never a count of zero.
- **Not S4.** S4 extends the `versioning@v1` and `toolchain-pins@v1` bundles, which arrived in
  v0.19, so the v0.18.2 binary cannot load it. The per-PR deterministic gate excludes S4 too; it
  is checked at release time by `bench-gate` against the previous release and `ANCHOR`.

Both checks fail the job and count in the CI summary (a cancelled or timed-out gate counts as
failed). `main` has no required checks, so on a push the gate reports after the merge.

## Recorded measurement (R0)

Measured on 2026-10-24 at `fb9820f5e` on the shared development host (x86-64, Arch Linux, rustc
1.99.1, Valgrind 3.25.1, gungraun 0.20.0), with both binaries built by the same toolchain and run
through the same harness. Instruction counts depend on the toolchain and the C library, so these
are a record, not a baseline.

### Instructions (Valgrind `Ir`, `det_check`)

| Cell | v0.18.2 | main (`fb9820f5e`) | Change |
|---|---:|---:|---:|
| S1 1k | 52,691,903 | 48,983,783 | -7.04% |
| S1 10k | 431,761,845 | 396,279,941 | -8.22% |
| S2 1k | 194,618,264 | 189,117,197 | -2.83% |
| S2 10k | 1,872,799,790 | 1,810,427,475 | -3.33% |
| S3 1k | 2,119,721,372 | 2,115,991,900 | -0.18% |
| S3 10k | 22,383,599,179 | 22,374,765,762 | -0.04% |

`main` is at or below v0.18.2 on every cell, so V21-6's stop condition (the budget already spent
before any relation-layer change) does not apply. S3 has the least headroom: the relation layer's
whole +3% budget is still available there, and nothing more.

### Syscalls (10k trees, alint process only)

| Tree | Family | v0.18.2 | main | Extra |
|---|---|---:|---:|---:|
| S1 | read | 41 | 42 | 1 |
| S1 | stat | 21,298 | 21,297 | -1 |
| S1 | open | 431 | 433 | 2 |
| S1 | dents | 804 | 804 | 0 |
| S2 | read | 19,240 | 19,240 | 0 |
| S2 | stat | 10,895 | 10,894 | -1 |
| S2 | open | 10,030 | 10,031 | 1 |
| S2 | dents | 804 | 804 | 0 |
| S3 | read | 40,048 | 40,048 | 0 |
| S3 | stat | 28,206 | 28,205 | -1 |
| S3 | open | 20,434 | 20,435 | 1 |
| S3 | dents | 804 | 804 | 0 |

The differences are fixed per run: `main` also probes for a `.alint.d` fragment directory, and
some runs (of either binary) read `/sys/devices/system/cpu/online` while others do not, which is
why two identical runs can differ by a call or two. A per-file regression adds thousands of calls
at 10k files, hence the 32-call allowance.

## Relation scenario

v0.21 R3 adds `xtask/src/bench/scenarios/relation_coalescing.yml`: `key_parity` over
`crates/*/Cargo.toml`, plus a per-file `toml_path_matches` over the same
manifests. It has no v0.18.2 counterpart, so it is not part of the absolute
reference comparison. It runs in `det_check`'s `relation_grp`, which
`reference-perf-gate.sh` excludes by filtering only `check_grp`; the per-PR
deterministic gate still measures it, and applies its normal +2% `Ir` / +5%
`EstimatedCycles` limits once the comparison base contains the cell. The first
committed measurement is therefore the budget baseline for later releases.

Measured on 2026-10-25 on the shared development host (x86-64, Arch Linux,
rustc 1.99.1, Valgrind 3.25.1, gungraun 0.20.0):

| Cell | Instructions | Estimated cycles |
|---|---:|---:|
| relation 1k | 42,816,797 | 63,368,993 |
| relation 10k | 297,682,329 | 421,614,713 |

Syscalls on the relation 10k tree, with the same counter and process restrictions as
S1-S3:

| Family | Calls |
|---|---:|
| read | 839 |
| stat | 10,898 |
| open | 632 |
| dents | 804 |

The same run shows the R2 read-coalescing property: with `key_parity` alone the 10k tree makes
839 read calls; with the per-file rule alone, 441; with both, 839. The two consumers share the
run-scoped read cache instead of adding 441 calls.

## Validation

- `ci/scripts/test-reference-perf-gate.sh` pins the outcome handling with stubbed tools: pass,
  an instruction breach, syscall growth over and within the allowance, every tooling failure
  (missing reference, missing cell, a cell without a v0.18.2 measurement, a non-regression exit,
  an untraceable or incomplete run, an empty trace), that `target/release/alint` is never
  overwritten, and the syscall counter's child exclusion and refusals.
- The gate passed on its own pull request (#363) in CI, on a hosted runner: S1 -7.2% / -7.8%, S2
  -2.8% / -3.1%, S3 -0.96% / -0.62%, syscall extras at most 2.
- A synthetic regression against the saved v0.18.2 measurement, using the gate's gungraun filter
  and limit (`--callgrind-limits='ir=3%'`) in a direct run (the script's own failure path is
  covered by the stubbed test): a loop added where the walker builds each entry.

  | Synthetic cost per walked entry | S1 1k | S1 10k | Gate |
  |---|---:|---:|---|
  | about 1,600 instructions (+4.5% / +4.2% over `main`) | -2.85% | -4.40% | pass |
  | about 5,400 instructions | +5.22% | +5.14% | fail ("Performance has regressed") |

  The first row shows what a cumulative budget means: `main` starts 7-8% below v0.18.2 on S1, so a
  change costing over 4% there still passes. S3 has no such headroom.

## Noise

Instruction counts are not perfectly repeatable for the multi-threaded binary: rayon and walker
threads that spin while idle are counted too. Re-running the unchanged `main` binary moved S3 10k
by 0.73% (22,374,765,762 then 22,211,411,264) and S3 1k by 0.17%; S1 and S2 moved by less. Both
sides of a comparison are single runs, so on S3 10k their noise can add up to about 1.5%, and the
+3% limit leaves about 1.5% of reliable room there. If that produces false failures, the fix is
to pin the thread counts in `det_check`, which needs a walker thread knob as well as
`RAYON_NUM_THREADS`.
