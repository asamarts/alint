# 2026-07 — v0.14.0 S2 "+17.6%" bench-gate failure: a REAL read-path regression (first misdiagnosed as a harness artifact)

Status: **Resolved (root cause corrected 2026-07-22).** The S2 wall-clock lift is a
genuine, if modest, v0.14 regression on read-heavy scenarios — NOT the pure harness
artifact first concluded. v0.14's OOM-cap fix (`c845f7d3`) routed every content read
through `File::open(p).take(cap+1).read_to_end(Vec::new())`; a `Take<File>` has no
`read_to_end` fstat-preallocation specialization (a bare `File` does), so it
grows-and-rereads, issuing extra `read()` syscalls per file. That is real wall-clock
on read-heavy repos but nearly invisible to the deterministic Valgrind gate (a
`read()` is a few guest instructions but a real kernel round-trip) — which is why
`det_check` Ir looked flat and the first pass called it environmental. **Fixed** by
preallocating the read buffer from the walk-time `FileEntry::size` alint already has
(`alint-core` `walker::read_bounded` + `alint-rules` `io::read_capped_with`),
restoring single-read behaviour while keeping the `take(cap+1)` TOCTOU bound. A
harness component was real but secondary — re-baselining runner-vs-runner only
dropped S2 from +17.6% to +15.1%; the ~13% residual is the code.

## Correction (2026-07-22) — how the "harness artifact" call was overturned

The original conclusion below ("flat `det_check` Ir ⟹ no code regression") was
**incomplete**, and the re-baseline it recommended is what disproved it: with
v0.13.0 (#136) and v0.14.0 (#134) BOTH measured on the self-hosted runner, same
rustc 1.97.0, the S2 gate still failed — `min_ms` +15.1% @100k, +12.9% @10k, +12.5%
@1m. Same harness, same host ⟹ the residual is not environmental.

A back-to-back microbench (same box, so contamination cannot explain it) isolates
the mechanism and the fix — reading 8000 small files ×5:

| read path | min | vs v0.13 |
|---|--:|--:|
| v0.13 `std::fs::read(p)` (`File`-specialized) | 189.6 ms | — |
| v0.14 `File::open` + `take(cap+1).read_to_end(Vec::new())` | 276.9 ms | **+46.0%** |
| fix: preallocate `Vec::with_capacity(walk_size)` | 172.9 ms | **−8.8%** (beats v0.13) |

The fix beats v0.13 because it skips even the `fstat` `std::fs::read` does
internally — alint already has the size from the walk. The +46% here is the pure
read cost; in S2 (which also does rule matching) it dilutes to the ~+13% the corpus
shows, and in read-light scenarios (S1 filename-only, S7 cross-file) to ~0.

### Why the deterministic gate missed it (methodology lesson)

`det_check` under Valgrind is load-immune AND harness-immune — but it is **not
I/O-immune**. It counts guest instructions; a `read()` syscall is a handful of guest
instructions regardless of how long the kernel spends servicing it, so a regression
that lives in *syscall / kernel wall-clock* (extra `read()` round-trips) does not
move Ir or EstimatedCycles. **A flat deterministic gate rules out a compute/cache
regression, NOT an I/O or syscall one.** For read-heavy (or spawn-heavy) scenarios,
confirm with a wall-clock control — a quiet-box back-to-back, or a syscall count —
before concluding "no regression." This caveat belongs in
[`../../../design/deterministic-perf-gating.md`](../../../design/deterministic-perf-gating.md).

### Re-baseline confirmation (all-runner trajectory)

The re-baseline this write-up originally recommended — re-running v0.10–v0.13 through
the *same* runner harness as v0.14 — is what proved the regression real. With every
tag measured identically (drop-caches, `renice`/`ionice`, rustc 1.97.0), v0.10→v0.13
is flat and low-CV, and v0.14 lifts across **every content-reading scenario in
proportion to how read-dominated it is** — the signature of a per-read cost, not a
harness offset (which would not survive harness control):

| scenario | v0.14 vs v0.13 (`min_ms`, ≥10k) | reads content? |
|---|--:|:--|
| **S2** existence + content | **+12.5 … +15.1 %** | yes — most read-dominated |
| **S12** v0.10 per-file | **+11 … +14 %** | yes |
| S3 / S6 / S9 | +5.6 … +7.4 % | yes |
| S7 cross-file relational | ~0 % | diff-dominated (few reads) |
| S1 filename hygiene | ~0 % | no (control) |

The gate flags exactly one cell (S2 100k, +15.1 %) only because most cells sit just
under the +15 % ceiling — a systematic regression the wall-clock gate *barely* catches.
This is also why the original per-scenario table's "broad ~+6.5 % band" was misread as
environmental: the band is real, and it is the read cost hitting every content
scenario at once. All-runner corpus: `../../macro/results/linux-x86_64/` (v0.10–v0.14),
trajectory in [`../../HISTORY.md`](../../HISTORY.md).

### The fix

`read_capped_or_skip` already receives the walk-time `FileEntry::size`, so the fix
threads it into `read_bounded` as a `Vec::with_capacity` hint — the read then fills in
one syscall instead of `Take<File>`'s grow-and-reread. The same one-line change lands
in `alint-rules` `io::read_capped_with`, which already computes the size for its
fast-reject. The `take(cap+1)` stays the sole correctness bound, so the hint is
strictly advisory: a stale or hostile size cannot force an over-read (locked in by the
`read_bounded_bounds_the_actual_read_toctou` test, which now passes a deliberately
lying hint). The microbench above nets −8.8 % vs v0.13 — the fix skips even the `fstat`
`std::fs::read` does internally, since alint has the size for free. Shipped in v0.14.1.

The v0.14.1 runner bench confirms the recovery on-host (same kbench harness, `min_ms`):
**S2 returns to the v0.13 baseline — −0.7 % at 100k, −3.3 % at 1M vs v0.13 (i.e. −13.8 %
/ −14.1 % vs v0.14)** — S12 to +0.5…+2.1 % vs v0.13 (−9.7…−9.9 % vs v0.14), and the less
read-dominated content scenarios (S3/S6/S9) to +2…+5 % vs v0.13 (within the ~3 % run CV),
while the non-reading controls S1/S7 stay flat throughout. The `xtask bench-gate` for
v0.14.1 vs v0.14.0 passes (an improvement never gates). Full trajectory:
[`../../HISTORY.md`](../../HISTORY.md).

Reproduce the mechanism + fix microbench:
[`read-preallocation-microbench/`](read-preallocation-microbench/).

The original analysis is preserved below as the investigative record: it is correct
that `det_check` Ir is flat and that a harness offset exists — only the inference
"therefore no code regression" was wrong.

---

> **Everything below is the INITIAL ANALYSIS, preserved as the investigative
> record.** Its conclusion — "harness artifact, no code fix" — was **overturned**
> (see the Correction above). The data it presents is sound (`det_check` Ir *is*
> flat; a harness offset *is* present); only the inference "flat Ir ⟹ no code
> regression" was wrong, because the deterministic gate is I/O-blind. Read it for
> how the misdiagnosis happened, not for the conclusion.

## TL;DR

The v0.14.0 `bench-record` PR (#131) failed `xtask bench-gate`: **S2 (existence +
content) `10k full` regressed +17.6% `min_ms`** vs v0.13.0, past the +15% gate,
with the other content-scanning scenarios (S3/S5/S6/S8/S9) up ~+6.5–7.3% and the
filename-only S1 / cross-file S7 flat.

The content-scanning correlation looked like a real per-file cost, and a code
scan even surfaced a plausible smoking gun (a v0.14 read-path change that *looked*
like it dropped a buffer preallocation). **The deterministic gate overturned
both.** `det_check` (the same `alint` CLI over the same synthetic trees, measured
under Valgrind, so load- and harness-immune) shows the **instruction count and
estimated cycles for S2 are identical between v0.13.0 and v0.14.0** (−0.2% at
10k). Same work, same instructions, +17.6% wall-clock → the wall-clock delta is
environmental, not v0.14 code.

The environmental difference is the harness: v0.14.0 is the first release benched
through the newly-registered self-hosted runner (`kbench-bench`), while
v0.10–v0.13 were backfilled by hand over SSH. A runner agent doing concurrent
work (log streaming, job bookkeeping) adds a steady background load that inflates
wall-clock most on the longer, CPU-heavier scenarios and barely touches the short
ones — which is exactly the "content-correlated" pattern that looked like code.

**Consequences:** ~~no v0.14.1 perf fix is warranted (the binary is
instruction-for-instruction identical on the hot path)~~ **[SUPERSEDED — see the
Correction above: a v0.14.1 fix IS warranted; "instruction-for-instruction
identical" is true yet does not imply "wall-clock identical" for an I/O regression].**
PR #131's numbers carry BOTH a harness offset and a real read-path regression;
re-baseline the trajectory on one harness so the real regression (and its v0.14.1
fix) can be read cleanly against a like-for-like baseline. A separate real bug was
found on the way: `ci/scripts/det-perf-gate.sh` pins `gungraun-runner` at a
version older than the workspace's `gungraun` library, which breaks the CI
deterministic gate on any post-bump PR (see "Secondary finding").

## Symptom

`xtask bench-gate --results <v0.14.0> --baseline <v0.13.0>` (both on
`linux-x86_64` = kbench):

```
[regression] min_ms vs baseline (gate: ≥10k, +15%)
  FAIL alint S2 10k full   min_ms +17.6%
  ok   alint S2 100k full  min_ms +13.9%
  ok   alint S2 1m full    min_ms +13.5%
  ok   alint S12 1m full   min_ms +8.8%
  ...
bench gate: 1 gating failure(s) — not publishable
```

Quality (within-run CV) PASSED — 100k/1m cells at 0.6% mean CV. So the run was
*stable*, just uniformly shifted up on the content scenarios.

## Full per-scenario wall-clock delta (v0.14.0 vs v0.13.0, `min_ms`, full mode)

The gate only prints the flagged cells; computing every cell is what first
suggested "harness" over "code" (the lift is broad and tracks run length, not any
single rule path):

| Scenario | 10k | 100k | 1M | reads content? |
|---|--:|--:|--:|:--:|
| S1 filename hygiene | +1.9% | −1.2% | −0.1% | no (control) |
| **S2 existence + content** | **+17.6%** | **+13.9%** | **+13.5%** | yes |
| S3 workspace bundle | +6.1% | +6.9% | +6.7% | yes |
| S4 agent hygiene | +7.0% | +0.1% | +0.3% | yes |
| S5 fix pass | +7.5% | +6.8% | +6.5% | yes |
| S6 per-file content | +6.9% | +7.3% | +7.3% | yes |
| S7 cross-file relational | −1.7% | −2.0% | +0.6% | yes (diff path) |
| S8 git overlay | +6.5% | +6.9% | +6.6% | yes |
| S9 nested polyglot | +6.8% | +7.0% | +6.7% | yes |
| S10 scope_filter | +3.6% | +0.7% | +0.7% | mild |
| S11 v0.10 cross-file | +2.8% | +2.2% | +1.6% | mild |
| S12 v0.10 per-file | +8.1% | +8.6% | +8.8% | yes |
| S13 v0.10 single-shot | +2.4% | −0.1% | +0.3% | mild |
| S14 v0.12 featureset | −1.6% | −2.0% | −1.8% | mild |

Pooled mean +4.4%, median +6.3%. A real code regression concentrated in one path
would spike a couple of related cells and leave the rest at ~0; instead a broad
band of unrelated content scenarios sits at ~+6.5% with S2 highest. That is the
shape of a steady background cost, not a hot-path change.

## Diagnosis (what was tested, in order)

| # | Hypothesis | Verdict / killed by |
|---|---|---|
| 1 | Transient contamination on kbench during the run | Partly — but kbench was idle at diagnosis time (load 0.00), CV was low (0.6%), and the lift was reproducible in the committed numbers, so not a one-off blip. |
| 2 | The kbench ACPI GPE storm (a known contamination source, ~1630 int/s burning a core) was firing during the bench | **Ruled out.** `gpe61` reads `enabled masked` and is frozen (0/s) — the kernel auto-masked it after an early-boot storm; it was not firing at bench time. |
| 3 | A real per-content-scan code regression from the W-series security cycle | **Killed by the deterministic gate (row 5).** Looked strong: the lift tracks content scanning, and a code scan found a read-path change (below). |
| 4 | The subagent's "smoking gun": v0.14's `c845f7d3` (crash/FIFO hardening) switched every content read from `std::fs::read(&abs)` to `read_capped_or_skip` → `read_bounded`, which reads into a zero-capacity `Vec::new()` via `Take::read_to_end` — *looks* like it drops `std::fs::read`'s size preallocation | **ACTUALLY RIGHT — the "Wrong" verdict here WAS the misdiagnosis.** The refutation confused two std behaviours: `read_to_end` is specialized to fstat-and-preallocate for a bare `File`, but a `Take<File>` gets NO such specialization and falls back to grow-and-reread. The cost is extra `read()` **syscalls**, not reallocs/memcpys — so it barely moves Ir (row 5) yet is real wall-clock. See the Correction (microbench: +46%). |
| 5 | **Harness / environment difference, not code** | **Partial — overturned as the SOLE cause.** `det_check` Ir/EstimatedCycles ARE flat ±0.4% (so no compute/cache regression, and a harness offset is genuinely present), but Ir is **I/O-blind**: it cannot see the extra `read()` syscalls. The runner-vs-runner re-baseline (still +12–15% S2) + the microbench proved a real ~13% wall-clock regression underneath the harness offset. See the Correction. |

The load- and harness-immunity of the deterministic gate is the whole point: it
measures instructions executed, not time, so a busier runner or a different
measurement session cannot move it. It is the ground truth the wall-clock
`bench-gate` cannot be (this is the design premise in
[`../../../design/deterministic-perf-gating.md`](../../../design/deterministic-perf-gating.md),
and the same call the [`2026-06-v0.12-perf-validation/`](../2026-06-v0.12-perf-validation/)
investigation made).

## Evidence: deterministic `det_check`, v0.14.0 vs v0.13.0

`det_check` runs the real release `alint` CLI over `gen-monorepo` trees for
S1/S2/S6/S7/S12 at 1k/10k under Valgrind. Measured v0.13.0 (`9a341559`, with
`gungraun-runner` 0.19.1) and v0.14.0 (`e77a0074`, with 0.19.3) separately and
diffed the absolute counts (raw data in [`det-check-ir.md`](det-check-ir.md)):

| Scenario | Ir Δ | EstimatedCycles Δ | note |
|---|--:|--:|---|
| s1_1k | −0.2% | −0.2% | control (no content read) |
| s1_10k | −0.1% | −0.1% | control |
| **s2_1k** | **+0.4%** | **+0.4%** | content; wall-clock was elevated |
| **s2_10k** | **−0.2%** | **−0.2%** | content; **wall-clock was +17.6%** |
| s6_1k | −0.1% | −0.1% | content |
| s6_10k | −0.2% | −0.2% | content |
| s7_1k | −0.4% | −0.3% | |
| s7_10k | +0.3% | +0.4% | |
| s12_1k | +0.2% | +0.2% | |
| s12_10k | +0.1% | +0.1% | |

Everything inside measurement noise. In particular **S2 at 10k — the failing cell
— is −0.2% Ir and −0.2% EstimatedCycles.** Ir scales linearly in file count, so a
flat 10k result implies flat 100k/1M as well. There is no per-file, per-byte, or
cache/memory regression in v0.14.0.

## Initial root-cause hypothesis: the harness (SUPERSEDED)

**Superseded — see the Correction.** The re-baseline disproved this: with v0.10–v0.13
re-measured through the *same* runner harness as v0.14, the "content-correlated"
inflation persisted (S2 +12–15 %, all content scenarios ∝ read intensity), so the
runner-load story below explains at most the small S2 harness slice (+17.6 → +15.1 %),
not the regression. It is kept because the reasoning ("broad content band ⟹ steady
background load") is a plausible-but-wrong inference worth recognizing.

The v0.10–v0.13 macro corpus was **backfilled by hand over SSH** on kbench (the
`$SCRATCH/laptop-*.sh` scripts), with nothing else running on the box. v0.14.0 is
the **first release benched through the self-hosted GitHub Actions runner**
(`kbench-bench`), registered as part of the same re-baseline. The runner's agent
processes (`Runner.Listener` + the job's log/upload machinery) do steady
concurrent work throughout the ~3.5 h run. On a 4-core box that steady load
inflates wall-clock, and it inflates the **longer, more CPU-bound scenarios most**
(S2/S3/S5/S6/S8/S9/S12) while barely touching the short filename-only scan (S1) —
producing the "content-correlated" pattern that impersonates a hot-path
regression. The instruction count is untouched, which is why the deterministic
gate is flat.

(A compounding environmental factor cannot be fully excluded after the fact: if
the auto-masked GPE storm fired for part of the v0.14 run but not the earlier
manual runs, it would add the same kind of uniform CPU-bound inflation. Either
way the remedy is the same — measure the whole trajectory under one verified-quiet
harness.)

## Remediation

1. **Do not publish PR #131 as a regression.** Its numbers are not comparable to
   the manual-SSH baselines. Either:
   - Re-run the v0.14.0 macro matrix and re-baseline v0.10–v0.13 through the *same*
     harness (the GitHub Actions runner, now canonical) so the trajectory is
     internally like-for-like again; or
   - characterize #131 with a note that its absolute numbers carry a harness offset
     vs the hand-measured predecessors, and let the next release (measured the same
     way) restore a clean cross-version comparison.
2. **Reduce the runner's own footprint during a bench** so the GHA-runner harness
   matches the quiet-box assumption: give the bench job the box to itself and/or
   `nice`/`ionice` the runner agent while a `bench` job runs. Documented as a
   follow-up in [`../../../design/v0.14/bench-host-migration.md`](../../../design/v0.14/bench-host-migration.md).
3. **Trust the deterministic gate as the release regression signal.** The
   wall-clock `bench-gate` is characterization on a verified-quiet box only; a
   wall-clock "regression" it flags is contamination-until-proven — confirm with
   `det_check`/`det_engine` (Ir) before treating it as real. This is already the
   documented policy in `RELEASING.md`; this investigation is the worked example.

## Secondary finding: `det-perf-gate.sh` gungraun-runner pin is stale (real CI bug)

Running the deterministic gate exposed a genuine defect. `ci/scripts/det-perf-gate.sh`
hardcodes `GUNGRAUN_VERSION=0.19.1` and installs that `gungraun-runner`, but the
workspace bumped the `gungraun` *library* to 0.19.3. The runner refuses to drive a
newer library (`gungraun-runner (0.19.1) is older than gungraun (0.19.3)`), so the
`det_check`/`det_engine` bench exits non-zero — and the script interprets any
non-zero exit as **"Ir/branch regression vs base"**. On CI this means the
deterministic perf-gate (advisory today) mis-reports a *tooling* failure as a perf
regression on every PR after the gungraun bump, and would hard-fail if
`DET_PERF_ADVISORY=0` were ever set.

Two fixes, both worth doing:
- Bump `GUNGRAUN_VERSION` to track the `gungraun` library version (0.19.3), ideally
  derived from `Cargo.lock` rather than hardcoded so it can't drift again.
- Distinguish a bench *tooling* error (non-zero exit with no comparison produced)
  from an actual regression, so a broken runner never masquerades as a regression.

Note for anyone reproducing this locally: because v0.13.0 needs runner 0.19.1 and
v0.14.0 needs 0.19.3, no single runner version drives both checkouts through
`det-perf-gate.sh`'s save-baseline/compare flow. Measure each tag with its own
matching runner and diff the *absolute* `det_check` counts (what this
investigation did), rather than relying on gungraun's built-in baseline compare
across the bump.

## Files

- [`det-check-ir.md`](det-check-ir.md) — the raw `det_check` absolute Ir +
  EstimatedCycles for v0.13.0 and v0.14.0 (S1/S2/S6/S7/S12 × 1k/10k), the numbers
  the Evidence table is computed from. Flat — which correctly narrowed the search to
  I/O, and (misread) is what sent the first pass to "harness artifact".
- [`read-preallocation-microbench/`](read-preallocation-microbench/) — the
  self-contained `rustc` reproduction (`readrepro.rs` + method + numbers) that isolates
  the `Take<File>` specialization loss and validates the fix, independent of alint and
  of the bench host. This is the mechanistic proof.

## Reuse

- Compute the *full* per-scenario delta before trusting a single flagged cell:
  `xtask bench-gate` prints only the failures, but the uniform-vs-targeted shape of
  the whole matrix is the fastest triage for code-vs-environment.
- A kbench GPE storm **auto-masks** — the counter freezes, so a low current reading
  doesn't prove it was quiet earlier. Check `cat /sys/firmware/acpi/interrupts/gpe61`
  for the `masked` flag and whether the kernel cmdline (not just `/etc/default/grub`)
  carries `acpi_mask_gpe=0x61`; the mask only applies after a reboot.
- `read_to_end` is specialized to fstat-and-preallocate for a bare `File` but NOT
  for a `Take<File>` — wrapping a read in `.take(cap+1)` for an OOM/TOCTOU bound
  silently drops that specialization, so `Vec::new()` there DOES regress
  (grow-and-reread → extra `read()` syscalls per file). Preallocate the buffer from
  a size you already have (here the walk-time `FileEntry::size`) to keep the single
  read while retaining the `take` bound.
- A flat deterministic (Valgrind-Ir) gate rules out a compute/cache regression but
  NOT an I/O or syscall one — it is I/O-blind. When wall-clock and a flat Ir gate
  disagree on a read- or spawn-heavy scenario, do NOT assume the gate wins: confirm
  with a quiet-box wall-clock control or a syscall count. This investigation is the
  worked example — the gate was flat while a real regression existed.
