# Perf investigations

Ad-hoc deep-dives that don't fit in a commit message: traces,
flamegraphs, bisect notes, hypothesis-and-result write-ups. One
directory per investigation.

## When to file one

A perf investigation belongs here when:

- The diagnostic data outlives the PR. Per-phase trace logs, profiler
  outputs, intermediate measurements — anything a future engineer
  hunting a similar regression would want to read.
- The investigation chain ran across multiple commits / sessions and a
  single commit message can't fit the writeup.
- The headline number lands in [`../HISTORY.md`](../HISTORY.md) but
  the *why* belongs in a longer narrative.

For a one-line fix the diagnostic of which fits in a commit message,
just put it in the commit message.

## Folder convention

`<YYYY-MM>-<slug>/` — chronological scanning is natural this way. The
slug is short and concrete (the regression name, the rule family
involved, the dispatch shape investigated).

Each investigation directory ships:

- `README.md` — the narrative. What was the symptom, what hypotheses
  did we test, what was the root cause, what changed.
- Raw trace / profile data (e.g. `*.phase.log`, `flamegraph.svg`) —
  unedited, kept for future cross-reference. Trimmed to the events
  that mattered (don't commit gigabytes of raw `perf record` data).
- Optional: a `bisect.md` with commit-by-commit numbers if a bisect
  was done.

## Existing investigations

### [`2026-05-bench-runner-instability/`](2026-05-bench-runner-instability/)

The v0.9.23 CV-gate failures that turned out to be **gate miscalibration, not a
degraded host**. A full cross-version corpus analysis showed the fingerprint
(kernel / rustc / RAM / fs / hyperfine / CPU) byte-identical across releases and the
`RELEASING.md` CV>10% rule never met by any shipped release; it produced the
programmatic `xtask bench-gate` (quality CV on 100k+ only; regression `min_ms` vs the
previous release). Dir name kept so the PR #32 backlink stays valid.

### [`2026-05-cross-file-rules/`](2026-05-cross-file-rules/)

The v0.9.4 1M S3 cliff investigation that produced the cross-file
dispatch fast-path fix shipping in v0.9.5. The published 1M S3 wall
had drifted +28-37 % vs the v0.5.6 baseline; the trace logs at
10k / 100k / 1m localised the bottleneck to `for_each_dir` rules
running O(D × N) over 5,000 packages × 1M entries (~5 billion
glob-match ops per rule × 4 rules). Fix: lazy
`OnceLock<HashSet<Arc<Path>>>` on `FileIndex` + literal-path fast
paths in `file_exists`, `structured_path`, and the `iter.has_file`
`when_iter:` builtin.

The investigation README documents the diagnostic trick: capture
`tracing::info!`-emitted phase + per-rule timings at 10k / 100k / 1m
for the *same* binary and look for rules whose `elapsed_us` grows
super-linearly in file count. Functions whose share grows
monotonically are super-linear suspects, even when the wall-time
absolute number doesn't yet flag them as a regression.

### [`2026-05-scope-filter-baseline-drift/`](2026-05-scope-filter-baseline-drift/)

v0.9.6 `scope_filter` Phase 2: a `bench-compare` flagged three >10% regressions
(`single_file_file_hash`, a junit formatter cell) that were **baseline drift, not an
engine regression** - the published floor was recorded under different conditions. An
apples-to-apples same-box re-measure cleared it. An early instance of the recurring
"stale baseline reads as a regression" trap.

### [`2026-05-v0.10-s13-100k-margin/`](2026-05-v0.10-s13-100k-margin/)

v0.10.0 bench-record: `S13 100k full` marginal CV failures across three runs on the
(then 3900X `kbox`) host - **host-side contention** from sister runners / dev stacks,
not code. The two busy runs failed on *different* cells each; a quiescent run passed
with one residual borderline cell accepted with a note. Motivates measuring only on a
quiescent host.

### [`2026-06-v0.12-perf-validation/`](2026-06-v0.12-perf-validation/)

v0.12's apparent wall-clock regression proven to be **co-tenant contamination, not
code**, via the deterministic Valgrind `Ir` gate (flat -> compute path unchanged).
This is the investigation that established the load-immune deterministic gate as the
authoritative regression signal - with the later caveat (added 2026-07) that `Ir` is
**I/O-blind**: it rules out compute/cache regressions but not syscall / read-path ones
(see the v0.14-S2 sibling).

### [`2026-07-1m-writeback-contention/`](2026-07-1m-writeback-contention/)

`S2/1m/full` blows the 10 % CV gate (37–51 %) on a 16 GB bench host,
while the *same cell, same box, same tag, same flags* measured in
isolation is clean (0.4 %). Not an alint regression: a measurement
artifact. A single `bench-scale` invocation writes ~16 GB (two 1M
trees — S9 forces a second — plus git objects) and then starts
hyperfine immediately, while the kernel is still draining gigabytes
of dirty pages; `S2` is the first scenario that reads the whole tree's
*content*, so it eats the contention. The 1M `runs = 3`
auto-reduction turns one stalled run into a gate failure.

Ten hypotheses were tested and falsified before the right one, and the
README records all of them — because a future engineer seeing high 1M
CV will reach for most of them first. Notably **it is not RAM**
(`MemAvailable` never below 14.8 GB of 15.8 GB) and **not thermal**
(NVMe peaked 44 °C against a ~70 °C throttle point).

The diagnostic that cracked it: compare each 1M cell's **isolated** vs
**in-matrix** mean. The CV gate only catches *variance*, so a
uniformly-slow cell would pass while being wrong; that comparison
proves the inflation is real and confined to one cell.

Also: the harness bug is **latent on the 62 GB 3900X**, not absent —
that box just holds the whole tree in page cache, so its reads never
touch the disk.

### [`2026-07-v0.14-s2-harness-artifact/`](2026-07-v0.14-s2-harness-artifact/)

v0.14.0's `bench-record` failed the wall-clock gate: **S2 `10k full` +17.6 %
`min_ms`** vs v0.13.0, with the other content scenarios up ~+6.5 % and the
filename-only S1 flat. A code scan surfaced the culprit — v0.14's OOM-cap fix
(`c845f7d3`) routed every content read through `File::open + take(cap+1)
.read_to_end(Vec::new())`.

**First misdiagnosed as a harness artifact, then corrected to a REAL regression.**
`det_check` under Valgrind (Ir + EstimatedCycles) is flat ±0.4 % including S2, which
the first pass read as "not in the binary." But Ir is **I/O-blind**: a `Take<File>`
loses `File`'s `read_to_end` fstat-preallocation, so it grows-and-rereads → extra
`read()` **syscalls** per file — real wall-clock, ~zero guest instructions.
Re-baselining v0.13 and v0.14 on the *same* runner still showed S2 +12–15 %, and a
back-to-back microbench measured +46 % (fixed: −8.8 %). Fixed by preallocating the
read buffer from the walk-time `FileEntry::size` (`walker::read_bounded` +
`io::read_capped_with`), keeping the `take(cap+1)` TOCTOU bound. A harness offset
was real but secondary — it explained only +17.6 → +15.1 %.

Reusable traps: `read_to_end` is specialized to preallocate for a bare `File` but
NOT a `Take<File>` — a `.take()` OOM/TOCTOU wrapper silently drops the specialization
and regresses read-heavy paths; preallocate from a size you already have. And a flat
deterministic (Valgrind-Ir) gate rules out a compute/cache regression but is
**I/O-blind** — confirm read- or spawn-heavy scenarios with a quiet-box wall-clock
control before concluding "no regression." A real CI bug fell out on the way:
`det-perf-gate.sh` pins `gungraun-runner` older than the `gungraun` library, so the
deterministic gate mis-reports a tooling failure as an Ir regression on any
post-bump PR.

### [`2026-09-v0.16-changed-mode-bench-artifact/`](2026-09-v0.16-changed-mode-bench-artifact/)

v0.16.0's `bench-record` flagged the small `changed`-mode cells +40-92%; a tight,
same-host, same-conditions 30-run A/B proved **v0.16.0 == v0.15.0 (no code
regression)**. The flag is a version-independent environment artifact: the same
v0.15.0 binary measures `changed/10k` at 43 ms (committed baseline) vs 117 ms (fresh
kbench) while `full` is flat, and `RAYON_NUM_THREADS=1` returns 43 ms for both
versions - the floor-level `changed` cells are dominated by the multi-core wakeup cost
of a tiny parallel burst, which grew on current kbench (idle cores at 800 MHz, deep
C-states, turbo off). Two wrong hypotheses (runner contamination; a `with_worker_pool`
rayon regression) were refuted along the way. Recommends the small `changed` cells be
regression-advisory, as they already are for the CV gate.

## Tooling

- `ALINT_LOG=alint_core=info target/release/alint check <root>` —
  emits per-phase + per-cross-file-rule wall-time events at INFO
  level. The structured fields are stable: `phase`, `elapsed_us`,
  optional `rules` / `files`. Grep stdout for `engine.phase` to
  isolate the bench-relevant lines.
- `xtask gen-monorepo --size {1k|10k|100k|1m} --out PATH` — persistent
  monorepo tree for ad-hoc profiler runs. Skips the 5+ minutes of
  tree-gen between iterations.
- `cargo install flamegraph; cargo flamegraph -p alint --bin alint --
  check <path>` — sampling profile via `perf` (Linux). Requires
  `perf_event_paranoid` at 1 or lower; consult your distro.
- `dhat` (heap profile) — wired up under
  `crates/alint-bench/Cargo.toml`'s `dhat` feature; see the v0.9.2
  memory-pass design doc for the pattern.

## Closing an investigation

When the fix lands and the perf number is restored, leave the
investigation directory in place. Update its README with a
"resolution" section pointing at the commit(s) and the published
[`HISTORY.md`](../HISTORY.md) row that captures the headline number.

Do not delete investigation directories — they're the only durable
record of *how we figured this out*, and the next regression of a
similar shape will benefit from being able to follow the same chain.
