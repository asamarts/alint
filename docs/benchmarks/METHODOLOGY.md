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
  where instruction-ish patterns are stable. 13 criterion bench
  files under `crates/alint-bench/benches/` (the two `det_*`
  gungraun benches there back the deterministic gate below, not
  this layer); the catalogue with per-bench rationale lives in
  [`micro/README.md`](micro/README.md).
- **hyperfine macro-benches** measure the actual CLI as
  users will invoke it, across controlled synthetic trees,
  and publish per-platform numbers. Five scenarios (S1-S4
  plus SFIX)
  under `xtask/src/bench/scenarios/`; catalogue in
  [`macro/README.md`](macro/README.md).

## How the macro layer works

`xtask bench-scale` is the entry point. Each
`(size, scenario, mode)` triple becomes one hyperfine row in
the published `results.json`. The harness:

1. **Builds** `alint` in release mode via `cargo build
   --release -p alint`.
2. **Captures** a hardware fingerprint (OS, arch, rustc
   version, CPU model, RAM size, filesystem type, hyperfine
   version, and tool versions); the report separately records
   the seed and warmup/run counts.
3. **Generates** a deterministic synthetic monorepo via
   `alint_bench::tree::generate_monorepo(packages,
   files_per_package, seed)`. The seed is fixed (`0xA11E47`
   by default) so every machine materialises a byte-identical
   tree. S4 uses the nested polyglot generator with committed
   version/toolchain policy anchors and a real Git repository so its
   workspace, versioning, toolchain, and git-aware paths fire.
4. **Stages** the scenario overlay and the current tool's configuration at the
   tree root, removing that tool-specific config before the next competitor so
   shared trees do not accumulate extra benchmark inputs.
5. **Validates** every row before timing it. Competitive S1/S2
   full-mode rows receive a temporary, scenario-specific violation;
   the command must return the exact expected status and report the
   planted filename or rule ID. This distinguishes a real finding
   from a startup/configuration failure, including Repolinter's
   ambiguous exit code 1.
6. **Shells out** to hyperfine with `--warmup 3 --runs 10`
   by default — `3` warmup runs to fill the page cache and
   amortise JIT/CPU-frequency-scaling settling, `10`
   measured runs for a stddev that's small enough to detect
   10% deltas with high confidence. The 1m tier caps this at
   1 warmup and 3 measured runs so the full matrix remains tractable;
   each result row records its actual sample count. The harness ignores
   only the exit codes explicitly associated with findings for that tool;
   setup and internal failures remain fatal.
7. **Checks scaling** when a run contains both 1k and 100k
   full-tree rows. A 100k/1k mean below 2× produces a warning,
   since that shape usually means the tool never walked the tree.
8. **Writes** per-size `results.md` plus an aggregated
   `index.md` and the machine-readable `results.json`, including every
   measured sample in milliseconds as well as the aggregate statistics.

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
`min_ms` at a **+15 % ceiling vs the prior release** for `full`/`fix`
cells at `>= 10k` and every 100k/1m cell. `changed`/1k and
`changed`/10k are the documented exception below; `1k` is otherwise
unreliable at every statistic. Each result also records a semantic
workload revision per scenario. When both reports declare different
revisions, the gate still applies the new run's within-run quality check
but skips that scenario's cross-version regression comparison: different
resolved rules or fixture semantics are different work, not evidence of
an engine regression. Scenario contracts anchor the revision's resolved
rule count and, for fact-gated workloads, the fact truth and active-rule
profiles.

The published `HISTORY.md` tables and the alint.org trajectory
deliberately stay `mean +/- stddev`: every historical row and the
hardcoded v0.5.6 baseline are mean-based, and restating the
public corpus on `min_ms` is a separable, externally-visible
change held out of scope. Standard split: gate on the robust
statistic, publish the full distribution. Cross-machine and
cross-hyperfine-version comparisons still require a like-for-like
fingerprint, and cross-version comparisons require the same scenario
workload revision.

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

**The 1k `changed` cells had a second, harness-made cause (found 2026-10).** Git, built
without `USE_NSEC` (its default), treats a tracked file whose mtime falls in the same
wall-clock second as `.git/index` as *racily clean* and re-hashes its content on every
`git ls-files --modified`, the command `--changed` runs; `ls-files` never rewrites the
index, so the cost repeats on every timed run. The harness generates and commits a 1k
tree within one second, so anywhere from none to all of it was racy depending on where
the second boundary fell: published 1k `changed` cells flipped between about 11 and
about 20 ms from release to release with unchanged code (up to 43M extra instructions
in git per run). `init_git_for_changed_mode` now waits out the second and lets `git
status` rewrite the index (`settle_racy_index`); 10k and larger trees take more than a
second to commit and were never affected
([`investigations/2026-10-v0.20-bench-increases/`](investigations/2026-10-v0.20-bench-increases/)).
On the bench host, five fresh-tree 1k runs per harness took S1 `changed` from 11.1-19.8 ms
(79% spread) to 10.6-12.0 ms, and S2 from 65% to 12%. The cells still vary about ±6-9%
around their median, the same floor as the 1k `full` cells (which never gated), so 1k
`changed` stays regression-advisory.

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

Comparing alint vs ls-lint vs shell pipelines vs Repolinter on a
developer's laptop is dishonest: each laptop has a
different `ls-lint` version, different `find`, grep, and ripgrep versions, a
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

The image also pins its benchmark-only Rust compiler independently of
alint's user-facing MSRV. The mounted checkout's moving `stable` override
cannot silently change that compiler, and the exact rustc remains in every
result fingerprint.

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
  62 GB 3900X reference desktop, whose retained series lives under
  [`macro/results/linux-x86_64-ryzen-3900x/`](macro/results/linux-x86_64-ryzen-3900x/))
  never reclaims and must leave the flag OFF — it needs passwordless sudo
  for `drop_caches` and would only add overhead.
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
   `EstimatedCycles` (+5%) vs the base, load-immune so it runs on any
   runner (GitHub-hosted for PRs today) and posts its per-bench table to
   the job summary. It runs on pull requests (base = the PR's base) and
   on pushes to `main` (base = the tip before the push), because most
   engine changes land as direct pushes. **Advisory today**
   (`DET_PERF_ADVISORY=1` — a regression annotates, doesn't fail; a
   gate that couldn't run fails the job as a tooling failure); see
   [`../design/deterministic-perf-gating.md`](../design/deterministic-perf-gating.md).
   `det_engine` runs with one rayon worker and collects that worker's
   jobs: until 2026-10 it counted only the benchmark thread, about 0.06%
   of the engine's work. The gate now fails as a tooling failure if
   `engine_run n10k` counts fewer than 10M instructions.
3. **Micro subset** (same job, `ci/scripts/micro-det-gate.sh`): the curated
   low-noise criterion micro benchmarks counted by `xtask micro-det`
   (below), head vs the same base. Advisory: a bench growing more than +3%
   in instructions annotates the run.

At release, `bench-record.yml` runs three more checks on the tagged commit:

- **`bench-gate`, automated.** After the macro matrix it runs against the
  previous release's committed `results.json` and the long-horizon anchor,
  and records the verdict in the job summary and the results PR (it does
  not abort the job, so the results are still published for review).
- **`micro-det` job** (GitHub-hosted, in parallel): the full criterion suite
  in instructions, vs the previous release (+3% per gated bench) and the
  anchor (+10%). A regression fails the job; the results PR's checklist
  requires it to be green.
- **Same-session micro A/B** (report-only): the previous release's criterion
  suite runs first on the bench host as baseline `prev`, then the new
  ref's with `--baseline-lenient prev`, so the PR shows each benchmark's
  wall-clock change measured minutes apart. This covers the I/O-heavy
  benches (walker, file content rules, blame cache) whose cost instruction
  counts cannot see.

### Deterministic micro counts (`xtask micro-det`)

Wall-clock micro results from one release to the next cannot tell a 10%
step from run-to-run noise: v0.20.0's published `every_matching_has/1000`
rose 14% with identical instructions and simulated cycles. `xtask
micro-det` makes the micro suite deterministic:

1. Each ref is checked out in a worktree and its own, unmodified criterion
   benches are built against a counting stand-in for `criterion`
   (`xtask/src/bench/micro_det_criterion_shim.rs`, patched over the crates.io
   crate with `--config patch.crates-io.criterion.path=…`).
2. Each benchmark runs under Callgrind at 1 and K iterations
   (`RAYON_NUM_THREADS=1`), collecting only inside the stand-in's
   per-iteration `__inv_measure` frame and the rayon jobs it dispatches.
   `(Ir(K) - Ir(1)) / (K - 1)` is the exact steady-state instruction count
   of one iteration on every thread, with setup and first-call warm-up
   excluded.
3. Head is compared with base (and the anchor). The gate covers every
   benchmark whose count is stable: in calibration (two full runs of v0.19.0
   and v0.20.0) 56+ benchmarks moved under 1% between identical runs. Two
   kinds are reported only (`REPORT_ONLY` in `xtask/src/bench/micro_det.rs`):
   the wall-clock-bound walker, blame-cache and dry-run fix benches, and the
   benches that read real files (single-file content rules, most structured
   queries), whose count moved 1-18% because glibc's allocator takes
   history-dependent paths there. The same-session wall-clock A/B covers
   those. The per-PR subset is a cheap gated selection covering each
   regression-prone area.

```sh
cargo run -q -p xtask -- micro-det --base v0.20.0 --head HEAD            # curated subset
cargo run -q -p xtask -- micro-det --base v0.20.0 --suite full --anchor auto
```

It needs `valgrind` and takes a few minutes for the subset. Instruction
counts depend on the compiler, so both refs are always built in the same
run with the same toolchain; never compare counts across runs.

### Long-horizon anchor

Every gate above compares a release with its predecessor, so creep can pass
every step: config loading grew +41%, +16% and +13% over three releases. The
anchor file [`ANCHOR`](ANCHOR) names a reference release. `bench-gate --anchor
auto` (macro gating cells, `min_ms`) and `micro-det --anchor auto`
(instructions) fail when a release exceeds it by more than 10% cumulative.
Accepting a deliberate cost means bumping the anchor in a reviewed commit, with
the reason in the file, so creep becomes a recorded decision.

The relation layer has its own, stricter reference: v0.18.2 at +3% on the
deterministic S1-S3 cells, run on relation-layer pull requests and every Rust push to
main (ADR-0023, ADR-0024). It
is described, with its recorded measurement, in
[`REFERENCE-v0.18.2.md`](REFERENCE-v0.18.2.md). The separate relation scenario
(`relation_coalescing.yml`: `key_parity` plus a per-file rule over the same files) runs in
`det_check`'s `relation_grp`, outside the absolute S1-S3 filter, and its first measurement is
recorded there as the basis for later budgets.

`xtask bench-compare` (micro vs the v0.7.0 floor) is a **local** helper,
not wired into any workflow. Wall-clock regression is gated
**per-release** (manual, before tag) by `xtask bench-gate`
(cross-version `min_ms`; the publish criterion in `RELEASING.md`),
trustworthy only on a verified-quiet box — `bench-record.yml`'s
`xtask bench-scale` matrix (S1-S4 full/changed plus SFIX fix, each at
1k/10k/100k/1m) is otherwise characterization. A gate failure — or any
> 20 % drift in a non-advisory cell even when the gate passes — gets an
investigation under [`investigations/`](investigations/) (v0.14.0's S2
read regression failed the gate at +15 % and got one, below). Small
`changed` cells follow the standing artifact investigation above unless
a gated 100k/1m cell corroborates the drift.

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
