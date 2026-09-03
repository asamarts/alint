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
  where instruction-ish patterns are stable. 12 criterion bench
  files under `crates/alint-bench/benches/` (the two `det_*`
  gungraun benches there back the deterministic gate below, not
  this layer); the catalogue with per-bench rationale lives in
  [`micro/README.md`](micro/README.md).
- **hyperfine macro-benches** measure the actual CLI as
  users will invoke it, across controlled synthetic trees,
  and publish per-platform numbers. 14 scenarios (S1-S14)
  under `xtask/src/bench/scenarios/`; catalogue in
  [`macro/README.md`](macro/README.md).

## How the macro layer works

`xtask bench-scale` is the entry point. Each
`(size, scenario, mode)` triple becomes one hyperfine row in
the published `results.json`. The harness:

1. **Builds** `alint` in release mode via `cargo build
   --release -p alint`.
2. **Generates** a deterministic synthetic monorepo via
   `alint_bench::tree::generate_monorepo(packages,
   files_per_package, seed)`. The seed is fixed (`0xA11E47`
   by default) so every machine materialises a byte-identical
   tree. S8 uses `generate_git_monorepo`, which additionally
   runs `git init && git add -A && git commit` so the
   engine's git-aware paths actually fire.
3. **Stages** the scenario's config YAML at the tree root.
4. **Captures** a hardware fingerprint (OS, arch, rustc
   version, CPU model, RAM size, filesystem type, hyperfine
   version, tool versions, seed, warmup/runs counts) and
   writes it to the `index.md` header.
5. **Shells out** to hyperfine with `--warmup 3 --runs 10`
   by default — `3` warmup runs to fill the page cache and
   amortise JIT/CPU-frequency-scaling settling, `10`
   measured runs for a stddev that's small enough to detect
   10% deltas with high confidence.
6. **Writes** per-size `results.md` plus an aggregated
   `index.md` and the machine-readable `results.json`.

Macro-specific design choices worth flagging:

### Why the publish gate uses min_ms (1k/10k advisory)

A full cross-version corpus analysis (v0.5.7 -> v0.9.22; see
[`investigations/2026-05-bench-runner-instability/`](investigations/2026-05-bench-runner-instability/))
showed per-cell within-run CV is a fixed absolute jitter floor
(~1 ms median, ~12-19 ms tail on 1k/10k) over a tiny mean: every
shipped v0.9.x release had 7-16 cells over the old "CV > 10 %"
line, so that flat gate was never met and never enforced.
Cross-version reproducibility tells the real story:

| statistic | 1k | 10k | 100k | 1m |
|---|--:|--:|--:|--:|
| `mean_ms` | 13.4% | 8.8% | 2.7% | 3.4% |
| `min_ms` | 11.9% | 2.7% | 2.7% | 2.8% |

So `xtask bench-gate` (the publish criterion; `RELEASING.md`
step 1) gates within-run CV only on 100k/1m (1k/10k are advisory
measurement-floor noise) and gates cross-version regression on
`min_ms` for `>= 10k` at a **+15 % ceiling vs the prior release**
(`1k` is unreliable at every statistic).
The published `HISTORY.md` tables and the alint.org trajectory
deliberately stay `mean +/- stddev`: every historical row and the
hardcoded v0.5.6 baseline are mean-based, and restating the
public corpus on `min_ms` is a separable, externally-visible
change held out of scope. Standard split: gate on the robust
statistic, publish the full distribution. Cross-machine and
cross-hyperfine-version comparisons still require a like-for-like
fingerprint.

**Caveat on the small `changed`-mode cells (added 2026-09).** The `changed`/1k and
`changed`/10k cells are unreliable for the `min_ms` REGRESSION gate too, not just for
CV. `changed` mode spawns `git` single-threaded to find the changed subset, then
dispatches the rule `par_iter` over a *tiny* subset, so these cells are dominated by
the cost of waking the other cores for that brief parallel burst - which is highly
sensitive to the host's C-state / frequency behavior and drifts across baseline
captures independent of any code change. v0.16.0's bench-record flagged them +40-92%,
yet the same v0.15.0 binary re-measured on the same host moved 43 -> 117 ms on
`changed`/10k (with `full` flat, and `RAYON_NUM_THREADS=1` pinned at the old 43 ms for
both versions), so it was a version-independent environment artifact
([`investigations/2026-09-v0.16-changed-mode-bench-artifact/`](investigations/2026-09-v0.16-changed-mode-bench-artifact/)).
`xtask bench-gate` now treats small `changed` cells (1k/10k) as regression-advisory as
well as CV-advisory (`gate.rs` `CHANGED_REGRESSION_ADVISORY_SIZES`): they report their
`min_ms` delta as `ADV` but do not fail the gate, while `full`/10k and all 100k/1m cells
still gate. The deterministic gate stays the authoritative regression signal.

### Why hyperfine and not a custom Rust harness

hyperfine measures wall-time of an external command from
*outside* the process. That's exactly the cost shape a CLI
user pays. A Rust-internal harness would skip:

- Process startup (`alint` binary cold-start, dynamic
  linker overhead).
- Stdio buffering / TTY detection / format-selection cost.
- The shell-quoting + arg-parsing path that real users hit.

These add up to non-trivial fixed overhead at small tree
sizes (S1/1k is well under 10 ms; process startup is
visible there). Hyperfine + the right warmup count is the
honest measurement.

### Why a deterministic synthetic monorepo and not a real-world repo

Cross-machine reproducibility requires byte-identical
inputs. Pinning to a specific real-world repo (the Linux
kernel, the Rust compiler) trades reproducibility for
"ecological validity" — but the latter is fake here, since
the rules we ship don't depend on the *content* of files,
only their shape (filenames, paths, structure, content
patterns). The synthetic tree:

- Is byte-identical across machines given the same seed.
- Produces a Cargo-workspace shape that exercises the rule
  catalogue's actual hot paths (S3 extends bundled
  monorepo + cargo-workspace rulesets that REQUIRE a
  workspace-shaped layout to fire).
- Sizes are exact powers (1k = 1,001 files, 1M =
  1,000,001) so cross-version comparisons aren't
  contaminated by tree-size drift.

### Why Docker for `--tools all` runs

Comparing alint vs ls-lint vs grep vs Repolinter on a
developer's laptop is dishonest: each laptop has a
different `ls-lint` version, a different `grep` flavour, a
different Node runtime under Repolinter. Numbers from such
a run aren't comparable to any other machine's run.

The `bench/Dockerfile` at the repo root pins every
benchmarked tool's version inside one image
(`ghcr.io/asamarts/alint-bench:<tag>`). `xtask bench-scale
--docker --tools all` runs the matrix inside that image; a
given image tag IS the canonical methodology version for
its release. Bumping any tool's version requires
re-publishing the image and re-running the competitive
numbers. The full rationale + tool-version pin list lives
in [`macro/README.md`](macro/README.md)'s "Reproducible
competitive runs" section.

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
- **Small-RAM hosts (<= ~16 GB) must run with
  `ALINT_BENCH_DROP_CACHES=1`.** The full 1M matrix leaves the page
  cache saturated (two 1M trees + git objects, ~16 GB), and the first
  content-heavy 1M scenario then stalls in page-cache reclaim mid-
  measurement (`allocstall`) — a variance artifact invisible to disk-util
  and `MemAvailable` that spikes one cell's CV to 40-50 %. The flag drops
  the page cache once per size phase after tree-gen; warmup re-reads the
  tree so measured runs stay warm. A large-RAM host (e.g. the now-retired
  62 GB 3900X reference desktop, whose series lives at
  [`/benchmarks-1/`](https://alint.org/benchmarks-1/)) never reclaims and
  must leave the flag OFF — it needs passwordless sudo for `drop_caches`
  and would only add overhead.
  Investigation: [`investigations/2026-07-1m-writeback-contention/`](investigations/2026-07-1m-writeback-contention/).

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

The CI bench jobs (per PR, `ci.yml`) are **not** the wall-clock
`bench-compare`:

1. **`bench-smoke`** — a fast hyperfine smoke check that the macro
   harness still runs end-to-end. **Non-gating** (a perf smoke check,
   not a correctness gate).
2. **`perf-gate`** — the deterministic gungraun gate
   (`ci/scripts/det-perf-gate.sh`): instruction-count `Ir` (+2%) and
   `EstimatedCycles` (+5%) vs the PR's merge-base, load-immune so it
   runs on the self-hosted runner regardless of co-tenants. **Advisory
   today** (`DET_PERF_ADVISORY=1` — it annotates, doesn't fail); see
   [`../design/deterministic-perf-gating.md`](../design/deterministic-perf-gating.md).

`xtask bench-compare` (micro vs the v0.7.0 floor) is a **local** helper,
not wired into any workflow. Wall-clock regression is gated
**per-release** (manual, before tag) by `xtask bench-gate`
(cross-version `min_ms`; the publish criterion in `RELEASING.md`),
trustworthy only on a verified-quiet box — `bench-record.yml`'s
`xtask bench-scale` matrix (S1-S14 × {1k,10k,100k,1m} × {full,changed})
is otherwise characterization. A gate failure — or any > 20 % drift even
when the gate passes — gets an investigation under
[`investigations/`](investigations/) (v0.14.0's S2 read regression failed
the gate at +15 % and got one, below).

**The deterministic `perf-gate` is load-immune but I/O-blind — this is the concrete
case for keeping BOTH layers.** It counts *guest instructions*, so a regression that
lives in syscall / kernel wall-clock (extra `read()`s per file, an added `stat`, a
spawn) barely moves `Ir` / `EstimatedCycles` while costing real time. v0.14.0 shipped
exactly such a read-path regression — the OOM cap dropped `File`'s `read_to_end`
preallocation, so content reads grew-and-reread — that the deterministic gate passed
flat (±0.4 %) and **only the wall-clock `bench-gate` caught** (S2 +12-15 %). So a
flat-`Ir`-but-slow scenario is *not* automatically contamination: it can be a real
I/O regression the deterministic layer structurally cannot see. Disambiguate with a
syscall count (deterministic, like `Ir`, but sensitive to the I/O path) or a
quiet-box wall-clock control before concluding either way. Worked example + microbench:
[`investigations/2026-07-v0.14-s2-harness-artifact/`](investigations/2026-07-v0.14-s2-harness-artifact/);
the gate's own statement of the limit is in
[`../design/deterministic-perf-gating.md`](../design/deterministic-perf-gating.md)
("Known blind spot").

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
