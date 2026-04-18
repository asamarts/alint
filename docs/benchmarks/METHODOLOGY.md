# Benchmark methodology

> Short version: two layers. **criterion** for pure-CPU micro-benchmarks
> (stable, cross-platform). **hyperfine** driven by `xtask bench-release`
> for end-to-end CLI wall-time (cross-platform, reproducible, honest about
> variance). Results are committed per version, per platform, under
> `docs/benchmarks/<version>/<platform>.md`.

## What we measure and why

alint's hot path combines two very different cost models:

1. **Syscall-bound**: the `ignore`-crate walk of the repository tree. Cost
   depends heavily on libc/kernel/filesystem + page-cache state.
2. **Pure-CPU**: glob compilation, `GlobSet` matching, regex matching against
   in-memory file contents, engine fan-out and result aggregation.

They need different tools. **criterion** is a bad fit for the syscall-heavy
path (wall-time variance + it's not what we want to regression-gate on);
Valgrind-based tools (iai-callgrind, CodSpeed Instruments) are a bad fit
because syscall instruction counts drift with glibc/kernel versions. So we
split:

- **criterion micro-benches** isolate the pure-CPU kernels where
  instruction-ish patterns are stable.
- **hyperfine macro-benches** measure the actual CLI as users will invoke
  it, across controlled synthetic trees, and publish per-platform numbers.

Explicitly not in v0.1: CodSpeed, Bencher, iai-callgrind/gungraun,
CI-gated wall-time regression detection. Revisit in v0.2 if criterion
baselines aren't sufficient.

## Layer 1 — criterion micro-benches

Location: `crates/alint-bench/benches/`.

| Bench | What it isolates |
|---|---|
| `glob_compile` | Cost of building a `Scope` from 10 / 100 / 1000 glob patterns. |
| `glob_match` | Per-path match throughput against a 5-pattern `Scope` across 1k / 10k / 100k paths. |
| `regex_content` | Byte throughput of a content regex against 1 KiB / 64 KiB / 1 MiB buffers. |
| `rule_engine` | Full engine run over an in-memory `FileIndex` of 1k / 10k / 100k entries — no I/O. |
| `walker` *(feature-gated `--features fs-benches`)* | Full FS walk of a seeded synthetic tree. Noisier than the others; opt-in. |

### Running locally

```bash
# all micro-benches
cargo bench -p alint-bench

# one target
cargo bench -p alint-bench --bench glob_match

# including the filesystem walker
cargo bench -p alint-bench --features fs-benches --bench walker

# save a baseline (e.g. on main), then compare from a branch:
git switch main     && cargo bench -p alint-bench -- --save-baseline main
git switch my-work  && cargo bench -p alint-bench -- --baseline main
```

Criterion emits HTML reports under `target/criterion/` and prints CI-style
deltas to the terminal. Noise is platform-dependent; on a reasonably quiet
machine expect 1-3% stddev on pure-CPU benches, higher on the walker.

## Layer 2 — `xtask bench-release` (hyperfine)

Location: `xtask/src/main.rs`. Rule config: `xtask/src/bench_config.yml`.

The driver:

1. `cargo build --release -p alint-cli`.
2. For each tree size (default 1k / 10k / 100k; `--quick` collapses to a
   single 500-file smoke), generates a deterministic synthetic tree under
   `$TMPDIR` via `alint-bench::tree::generate_tree`. The seed is fixed
   (`0xA11E47` by default, overridable with `--seed`) so every machine
   materializes a byte-identical tree.
3. Copies a ~15-rule representative config (`bench_config.yml`) into the
   tree root.
4. Shells out to `hyperfine` with `--warmup 5 --min-runs 10` and
   `--export-markdown`.
5. Captures a platform fingerprint (OS, arch, rustc version, git SHA,
   timestamp) and emits a markdown report to stdout or `--out <path>`.

### Running locally

```bash
# full report (several minutes)
cargo run -p xtask --release -- bench-release \
    --out docs/benchmarks/v0.1/linux-x86_64.md

# quick smoke (~10 seconds)
cargo run -p xtask --release -- bench-release --quick
```

`hyperfine` must be installed and on `PATH`:

```bash
cargo install hyperfine
# or: apt install hyperfine / brew install hyperfine / choco install hyperfine
```

### What gets committed

Per release cut, the maintainer runs `bench-release` on each supported
platform and commits the markdown to
`docs/benchmarks/<next-version>/<os>-<arch>.md`. Example:

```
docs/benchmarks/v0.1/linux-x86_64.md
docs/benchmarks/v0.1/macos-arm64.md
docs/benchmarks/v0.1/windows-x86_64.md
```

This follows ripgrep's precedent: cross-machine variance is documented by
running on multiple machines and committing results per machine, not
hidden behind a single dashboard number.

## Why not CodSpeed / iai-callgrind for v0.1

- **iai-callgrind / gungraun** is Valgrind-based and Linux-only in
  practice (Apple Silicon is unsupported by upstream Valgrind; Windows is
  unsupported). An alint-specific problem: syscall-heavy code under
  Valgrind reports instruction counts that drift whenever the CI runner's
  glibc or kernel updates — which is exactly the part of alint we most
  want stable numbers for.
- **CodSpeed** uses the same Valgrind substrate for its "Instruments"
  mode, inheriting the same issues. CodSpeed's Walltime Macro Runners
  would give stable wall-time numbers but require a GitHub organization
  account and add complexity for marginal v0.1 value.

Both remain reasonable to add in v0.2 once we have a concrete need for
sub-percent CI-gated regression detection on the pure-CPU components
specifically. The criterion source we ship today is drop-in compatible
with `codspeed-criterion-compat` via a shim — adopting CodSpeed later
won't require touching the bench code.

## Reproducibility caveats (be honest)

- Absolute numbers are not comparable across machines. Always compare
  like-for-like: same platform file, same tree size, same rule count.
- GitHub-hosted `ubuntu-latest` has 5-30% wall-time variance — fine for
  smoke-testing the harness, too noisy for PR-level regression gating.
- Filesystem type matters (tmpfs > ext4 > NTFS > APFS by order of
  magnitude on walk-heavy workloads). Platform fingerprint includes OS +
  arch but not FS type; note it in commit messages when it matters.
- `cargo build --release` is not bit-reproducible across rustc versions
  even with the same source. That's why the fingerprint records the
  rustc version.

## Adding a new bench target

1. Add a `.rs` file under `crates/alint-bench/benches/`.
2. Register it in `crates/alint-bench/Cargo.toml` as a `[[bench]]` entry.
3. Use `criterion_group!` + `criterion_main!`.
4. Prefer `BenchmarkGroup` + `Throughput` to get per-element / per-byte
   numbers.
5. Add a row to the table above.

## Adding a new target platform

1. Install `hyperfine` on the target machine.
2. `cargo run -p xtask --release -- bench-release --out docs/benchmarks/<version>/<os>-<arch>.md`.
3. Sanity-check the numbers; commit the file. Do not auto-commit via CI —
   GitHub runner variance means human eyes should read before recording.
