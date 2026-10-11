# 2026-07 — S2/1m/full blows the CV gate on a 16 GB bench host

Status: **Resolved — page-cache reclaim confirmed; `ALINT_BENCH_DROP_CACHES` fix validated.**

## Resolution (2026-07-15)

The mechanism was NOT RAM-by-`MemAvailable`, NOT thermal, NOT writeback (all
falsified with instrumentation — see below); it was **page-cache reclaim
pressure**. On a 16 GB host the full matrix leaves the cache saturated (two 1M
trees plus git objects, ~16 GB); the first content-heavy 1M scenario (`S2`)
then allocates + reads 3.9 GB and stalls in reclaim (`allocstall`), invisible to
both disk-util and `MemAvailable`. A 62 GB host holds it all and never reclaims,
which is why the 3900X never shows the artifact.

**Fix (`xtask/src/bench/run.rs`, opt-in `ALINT_BENCH_DROP_CACHES`):** `sync` +
drop the page cache once per size phase, after tree materialization and before
the benchmark loop. hyperfine's warmup re-reads the tree, so measured runs stay
warm; the drop only evicts the stale cross-phase pollution that forced reclaim.

**Validation (same full publish-grade matrix that broke S2/1m/full 3/3 times at
37 % / 43 % / 51 % CV):** with the flag, S2/1m/full = **3656 ms, CV 0.5 %** (the
isolated-clean baseline is 3667 ms / 0.4 %), every 1M cell clean, and
**`bench-gate: PASS`**. Comparability (T5): no non-S2 cell moved materially
(median |move| 1.3 % in the earlier A/B). An A/B on the `100k,1m` SUBSET was
inconclusive because that subset reproduces the artifact only intermittently
(the extra 1k/10k phases are what make the full matrix break reliably) — so the
FULL matrix is the correct reproduction shape.

The flag is off by default (a large-RAM host neither needs nor should pay for
it) and becomes part of the small-RAM bench-host contract in `METHODOLOGY.md`.

---

Original write-up (superseded by the resolution above):
Symptom is a *measurement artifact*, not an alint regression: the
same cell measured in isolation on the same box, same tag, same
flags is clean (CV 0.1–0.5 %).

Context: this arose while qualifying a second bench host (an
i7-6700HQ laptop, 16 GB RAM) as a replacement for the contended
3900X dev box. The 3900X (62 GB) does **not** show this. See
[Host qualification](#host-qualification) for why that matters.

## TL;DR

- **One cell out of 112 fails the publish gate**: `S2 / 1m / full`,
  CV 37.5 % → 42.9 % → 51.3 % across three full-matrix runs. The gate
  ceiling is 10 % on 100k+ cells.
- **It is not a real slowdown.** The same cell run in isolation is
  3 667 ms at CV 0.4 %; inside the full matrix it reads 8 541 ms at
  CV 51.3 % — a **2.33× inflation** of an artifact.
- **It is NOT memory exhaustion.** `MemAvailable` never drops below
  **14.8 GB of 15.8 GB**. This was the first hypothesis and it is wrong.
- **It is NOT thermal.** NVMe peaked at **44 °C** (SM951 throttles
  ~70 °C); CPU peaked 72 °C with the clock pinned at 2 600 MHz and a
  measured floor of 2 599 MHz across 81 minutes.
- **Mechanism: disk writeback contention.** A single `bench-scale`
  invocation writes ~16 GB (two 1M trees — S9 forces a second one —
  plus ~8 GB of git objects), then starts hyperfine **immediately**,
  while the kernel is still draining gigabytes of dirty pages. `S2`
  is the first scenario that *reads the whole tree's content*, so its
  reads collide with the drain. `S1` is stat-only and sails through;
  `S2/changed` and `S9` run later, after the drain, and are clean.
- **The 1M auto-reduction turns a stall into a gate failure.**
  `run.rs` forces `runs = 3` at 1M. With three samples, one stalled
  run is enough to wreck the CV.
- **Only this cell is affected.** Every other 1M cell reproduces its
  isolated value within 1–3 % (table below). The 1M corpus is not
  globally contaminated.

## Evidence

### 1. Isolated vs full-matrix, same box / tag / flags

| 1M cell | isolated | in full matrix | inflation |
|---|--:|--:|--:|
| S1 / full | 2 416 ms | 2 496 ms | 1.03× |
| S1 / changed | 4 279 ms | 4 346 ms | 1.02× |
| **S2 / full** | **3 667 ms** | **8 541 ms** | **2.33×** |
| S2 / changed | 4 317 ms | 4 256 ms | 0.99× |
| S9 / full | 11 912 ms | 11 936 ms | 1.00× |
| S9 / changed | 4 869 ms | 4 777 ms | 0.98× |

This is the load-bearing table. It proves (a) the inflation is real
and (b) it is confined to one cell. The CV gate only catches
*variance*, so a uniformly-slow cell would pass while being wrong —
that is explicitly not happening here.

### 2. The failing cell's distribution

```
S2/1m/full:  samples=3  min=4177ms  median=8505ms  max=12941ms
             mean=8541ms  stddev=4382ms  CV=51.3%
S2/1m/changed (sibling):  mean=4256ms  stddev=12ms   CV=0.3%
```

`samples=3` is **by design**, not a bug: `xtask/src/bench/run.rs`
auto-reduces `runs` to 3 at the 1M size. Note that `min`/`median`/`max`
are order-independent — the JSON does **not** record execution order,
so no claim about "each run getting slower" can be made from this.

### 3. Hypotheses tested and falsified

Recorded because the falsifications are the useful part; a future
engineer seeing high 1M CV will reach for most of these first.

| # | Hypothesis | Killed by |
|---|---|---|
| 1 | 16 GB RAM ceiling (naive) | The *heaviest* 1M cells (S3, S7 ≈ 17 s) run at 0.7 % CV |
| 2 | S2 is an inherently noisy scenario | 3900X runs S2/1m/full at 0.4–1.5 % across v0.11–v0.13 |
| 3 | Cold cache from `git add` evicting the tree | Reproduced those exact conditions (`--modes full,changed`) → 0.5 % |
| 4 | Insufficient warmup | `--warmup 10` in isolation changed nothing (0.6 %) — but see caveat below |
| 5 | Background systemd timers | None fired in the window; all masked since |
| 6 | "It was a transient" | **Wrong.** Reproduced 3/3 full-matrix runs, worsening each time |
| 7 | ext4 writeback from *prior phase teardown* | Bisect with a preceding 100k phase → 0.1 % |
| 8 | S9's second 1M tree blowing the working set | Forced both trees (21.5 GB on disk) → 0.4 %; min RAM 13.8 GB |
| 9 | NVMe thermal throttling | NVMe peaked **44 °C**; SM951 throttles ~70 °C |
| 10 | **Memory exhaustion** | **`MemAvailable` never below 14.8 GB** |

Caveat on #4: the warmup test ran in an *uncontended* isolated run —
there was no writeback backlog to absorb, so it proved nothing about
the contended case. It must be retested under contention (T2 below).

### 4. Why the 3900X never sees this

62 GB of RAM holds the entire 3.9 GB tree in page cache, so S2's
reads never touch the disk and therefore never contend with the
writeback drain. **The harness bug is latent there, not absent.** The
quiesce fix below is worth landing on its own merits.

### 5. Kernel writeback configuration (the bench host, stock)

```
vm.dirty_ratio             = 20     # ~3.2 GB dirty before writers throttle
vm.dirty_background_ratio  = 10     # ~1.6 GB before background flush starts
vm.dirty_expire_centisecs  = 3000
vm.dirty_writeback_centisecs = 500
```

A ~16 GB write burst hammers this ceiling continuously, and the flush
keeps draining *after* the writes stop — straight into the benchmark
window. Peak `Dirty` observed: **1 165 MB**.

## Fix strategy

### Core fix: quiesce, do NOT drop caches

The standard benchmarking recipe (`sync; echo 3 > /proc/sys/vm/drop_caches`)
is **wrong here**. It forces every read cold, which changes *what we
measure* and breaks comparability with the entire published corpus
(measured warm). We want the **disk idle**, not the **cache empty**.

So: `sync()`, then poll `/proc/meminfo` `Dirty` + `Writeback` until both
fall under a small threshold (with a timeout), at three boundaries:

1. after tree materialisation (regular + polyglot),
2. after `git init/add` + the `changed`-mode file touching,
3. after each size phase's tree teardown (`rm -rf` of a 1M tree is
   itself heavy metadata writeback).

This belongs in `xtask/src/bench/run.rs`: the tree-gen → hyperfine
boundary is *internal* to one invocation, so no external orchestration
can settle it.

### Constraint: the backfill benches old tags with their own xtask

A fix on `main` does not reach v0.10–v0.12. Levers that work on every
tag without a code change:

- **Kernel writeback tuning (primary candidate).** Absolute limits so
  the backlog can never grow large:
  `vm.dirty_background_bytes=64MB`, `vm.dirty_bytes=256MB`,
  `dirty_writeback_centisecs=100`, `dirty_expire_centisecs=500`.
  Writeback becomes continuous and small instead of a multi-GB burst.
- **Raise `--warmup` at 1M.** Warmup runs are unmeasured, so they can
  absorb the drain window.
- **Split S9 into its own invocation.** S9 is what forces the second
  1M tree; running it separately halves the write burst per invocation.
  Results merge cleanly.
- **Graft the fixed xtask onto old tags** (`git checkout main -- xtask/`).
  Proper fix everywhere, but risks API drift — must be validated.
- **Cap the corpus at ≤100k.** Fallback. `bench-gate` PASSES on the
  84 non-1M cells today.

Ruled out: **tmpfs for the trees** (no writeback at all, but two 1M
trees ≈ 22 GB > 15.8 GB RAM — fine at ≤100k, infeasible at 1M).

## Test plan

| # | Test | Success criterion |
|---|---|---|
| T0 | Reproduce with a full 100k phase + 1M, logging MemAvailable / Dirty / Writeback / disk-util | Dirty+Writeback high and disk saturated *during the S2/1m/full window*, MemAvailable high |
| T1 | Apply the sysctl tuning, re-run the reproduction | S2/1m/full CV ≤ 10 % **and** mean ≈ 3 667 ms (its isolated value) |
| T2 | Same, `--warmup 8`, under contention | Does a CLI-only lever suffice for old tags? |
| T3 | Same, minus S9 | Quantifies the second tree's contribution |
| T4 | Implement the quiesce patch in xtask; full matrix on `main` | S2/1m clean; `bench-gate` PASS |
| T5 | **Comparability check**: all *other* 111 cells, before vs after | They must **not** move. A fix that shifts unaffected cells is contaminating the measurement, not cleaning it. |
| T6 | Full matrix re-run with the winning combination | `bench-gate: PASS` — the only thing that authorises publishing |
| T7 | Backfill v0.10.0–v0.12.0 with that configuration | All gate-green |

T5 is the discriminating test: it separates "removed an artifact" from
"changed the experiment."

## Host qualification

the bench host otherwise qualified cleanly and is a *better* measurement host
than the contended dev box:

- 100k cells: **worst CV 2.6 %, median 0.5 %** (gate: 10 %).
- Heat-soak drift over 3 back-to-back matrix passes: **median 0.2 %**.
- Clock pinned at 2 600 MHz (turbo off, `performance` governor); floor
  measured at **2 599 MHz** over 81 minutes — no throttling.
- A stuck ACPI GPE (`gpe61`) was firing ~1 630 interrupts/sec and
  burning a full core 24/7; masked via `acpi_mask_gpe=0x61`. Idle went
  67 °C / load 1.00 → 44 °C / load 0.02. **Check this on any laptop
  before trusting it as a bench host.**

## Files

- `thermal-matrix.csv` — NVMe + CPU temperature, 5 s samples, across a
  full 3 h matrix run. Kills hypothesis #9.
- `s2instr.csv` — MemAvailable / Dirty / Writeback / disk-util, 1 s
  samples, across the T0 reproduction. Kills #10 and localises the
  writeback window.
- `isolated-vs-matrix.md` — the raw jq output behind the table above.

## Related

- [`../2026-07-v0.14-s2-harness-artifact/`](../2026-07-v0.14-s2-harness-artifact/) — the
  same `S2`-reads-the-whole-tree's-content property that makes this cell eat writeback
  contention also makes it the first to show a *code* read-path regression: v0.14.0
  dropped a whole-file-read buffer preallocation and S2 took the biggest hit. That one is
  a real regression (fixed in v0.14.1), not a measurement artifact — the contrast is the
  useful bit when triaging a slow S2: is it the box (this doc) or the binary (that one)?
