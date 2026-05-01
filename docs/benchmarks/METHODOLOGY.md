# Benchmark methodology

> Short version: two layers. **criterion** for pure-CPU
> micro-benchmarks (stable, cross-platform). **hyperfine**
> driven by `xtask bench-scale` for end-to-end CLI wall-time
> (cross-platform, reproducible, honest about variance).
> Results are committed per version, per platform, under
> [`micro/results/`](micro/) and [`macro/results/`](macro/).
>
> This document explains the *why* behind that split. For
> *how to run them*, see [`RUNNING.md`](RUNNING.md). For
> *what each one measures*, see [`micro/README.md`](micro/README.md)
> and [`macro/README.md`](macro/README.md). For *current
> published numbers*, see [`README.md`](README.md) and
> [`HISTORY.md`](HISTORY.md).

## What we measure and why

alint's hot path combines two very different cost models:

1. **Syscall-bound**: the `ignore`-crate walk of the
   repository tree. Cost depends heavily on libc/kernel/
   filesystem + page-cache state.
2. **Pure-CPU**: glob compilation, `GlobSet` matching, regex
   matching against in-memory file contents, engine fan-out
   and result aggregation.

They need different tools. **criterion** is a bad fit for
the syscall-heavy path (wall-time variance + it's not what
we want to regression-gate on); Valgrind-based tools
(iai-callgrind, CodSpeed Instruments) are a bad fit because
syscall instruction counts drift with glibc/kernel versions.
So we split:

- **criterion micro-benches** isolate the pure-CPU kernels
  where instruction-ish patterns are stable. 12 bench files
  under `crates/alint-bench/benches/`; the catalogue with
  per-bench rationale lives in [`micro/README.md`](micro/README.md).
- **hyperfine macro-benches** measure the actual CLI as
  users will invoke it, across controlled synthetic trees,
  and publish per-platform numbers. 8 scenarios (S1-S8)
  under `xtask/src/bench/scenarios/`; catalogue in
  [`macro/README.md`](macro/README.md).

## Reproducibility caveats (be honest)

- **Absolute numbers are not comparable across machines.**
  Always compare like-for-like: same platform fingerprint
  (OS / arch / rustc / CPU / RAM / FS), same tree size,
  same scenario. The platform fingerprint is captured in
  every published `index.md`'s header.
- **GitHub-hosted `ubuntu-latest` has 5-30 % wall-time
  variance** — fine for smoke-testing the harness, too
  noisy for PR-level regression gating. Publication-grade
  numbers come from a self-hosted runner with a known
  fingerprint (per `docs/benchmarks/README.md`'s TL;DR).
- **Filesystem type matters** (tmpfs > ext4 > NTFS > APFS
  by order of magnitude on walk-heavy workloads). Platform
  fingerprint includes OS + arch but not FS type explicitly;
  note it in commit messages or `index.md` headers when it
  matters.
- **`cargo build --release` is not bit-reproducible across
  rustc versions** even with the same source. That's why
  the fingerprint records the rustc version.

## Why not CodSpeed / iai-callgrind / Bencher

- **iai-callgrind / gungraun** is Valgrind-based and
  Linux-only in practice (Apple Silicon is unsupported by
  upstream Valgrind; Windows is unsupported). An
  alint-specific problem: syscall-heavy code under Valgrind
  reports instruction counts that drift whenever the CI
  runner's glibc or kernel updates — exactly the part of
  alint we most want stable numbers for.
- **CodSpeed** uses the same Valgrind substrate for its
  "Instruments" mode, inheriting the same issues. CodSpeed's
  Walltime Macro Runners would give stable wall-time numbers
  but require a GitHub organization account and add
  complexity for marginal value at our publication cadence.
- **Bencher** is a thin SaaS wrapper around criterion +
  hyperfine outputs; we already produce those, and the
  wrapper's value (visualisation, alerting) doesn't yet
  justify the new external dependency.

The criterion source we ship is drop-in compatible with
`codspeed-criterion-compat` via a shim — adopting CodSpeed
later won't require touching the bench code.

## Regression gates

Two gates run in CI:

1. **Per-PR**: `xtask bench-compare --before <floor> --after
   target/criterion --threshold 10` against the v0.7.0
   floor under
   [`micro/results/linux-x86_64/v0.7.0/criterion/`](micro/results/linux-x86_64/v0.7.0/). Catches any micro-bench whose mean
   has grown more than 10 % vs the v0.7.0 publication.
2. **Per-release** (manual, before tag): `xtask bench-scale`
   at the publication-grade matrix; eyeball the headline
   cells in [`HISTORY.md`](HISTORY.md). Anything > 20 %
   drift gets an investigation under
   [`investigations/`](investigations/).

Per-phase gating during a release cut (e.g. v0.9.x's four
phases) compared each phase against the prior phase's
snapshot under
[`archive/v0.9-development-baselines/`](archive/v0.9-development-baselines/) — see the v0.9 design doc for that
convention.

## Adding a new bench

See [`micro/README.md`](micro/README.md) and
[`macro/README.md`](macro/README.md) for the per-layer
recipes. Both layers ship with a "soft" coverage warning
test (`coverage_audit_bench_listing.rs` for macro;
`coverage_audit.rs` already covers e2e correctness for
micro-benched rule kinds via the e2e scenarios) that
surfaces uncovered rule kinds — useful as a triage list
when picking what shape to add next.

## Adding a new target platform

1. Install `hyperfine` on the target machine and ensure
   `cargo bench` works.
2. Run the publication-grade matrix:

   ```sh
   cargo bench -p alint-bench --features fs-benches
   xtask publish-benches --trim
   xtask bench-scale --include-1m --scenarios S1,S2,S3 --warmup 3 --runs 10
   ```

3. The defaults write to
   `docs/benchmarks/{micro,macro}/results/<os>-<arch>/v<workspace-version>/`;
   verify the new dirs are present, sanity-check the
   numbers, commit the file. Do not auto-commit via CI —
   per-machine variance means human eyes should read before
   recording.
